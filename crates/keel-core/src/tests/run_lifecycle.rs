use super::*;

#[test]
fn noop_run_creates_artifacts_and_discard_preserves_history() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let metadata = project.run("test noop run", "noop").unwrap();

    assert_eq!(metadata.agent, "noop");
    assert_eq!(metadata.status, RunStatus::Ready);
    let worktree = worktree_dir(&temp, &metadata.run_id);
    assert!(worktree.join(NOOP_OUTPUT_FILE).exists());

    let run_dir = run_dir(&temp, &metadata.run_id);
    assert_required_artifacts(&run_dir);

    let discarded = project.discard(&metadata.run_id).unwrap();

    assert_eq!(discarded.status, RunStatus::Discarded);
    assert!(!worktree.exists());
    assert!(!branch_exists(&temp, &metadata.branch));
    assert!(run_dir.join(METADATA_FILE).exists());
    assert!(run_dir.join(REPORT_FILE).exists());
    assert!(run_dir.join(LOG_FILE).exists());
    let report = fs::read_to_string(run_dir.join(REPORT_FILE)).unwrap();
    assert!(report.contains("# Keel Run Report"));
    assert!(report.contains("## Artifacts"));
    assert!(report.contains("## Suggested Next Actions"));
    assert!(report.contains("## Discard"));
    assert!(report.contains("Branch cleanup: `deleted`"));
    assert!(report.contains(&format!("keel rerun {}", metadata.run_id)));
    assert!(report.contains("keel-noop-output.txt"));
}

#[test]
fn discard_succeeds_when_candidate_branch_is_already_absent() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let metadata = project.run("missing branch discard", "noop").unwrap();
    git(
        temp.path(),
        &[
            "worktree",
            "remove",
            "--force",
            worktree_dir(&temp, &metadata.run_id).to_str().unwrap(),
        ],
    );
    git(temp.path(), &["branch", "-D", metadata.branch.as_str()]);

    let discarded = project.discard(&metadata.run_id).unwrap();

    assert_eq!(discarded.status, RunStatus::Discarded);
    assert!(!branch_exists(&temp, &metadata.branch));
    let report = read_run_file(&temp, &metadata.run_id, REPORT_FILE);
    assert!(report.contains("Branch cleanup: `already absent`"));
    assert!(discarded.warnings.is_empty());
}

#[test]
fn discard_skips_unexpected_metadata_branch_and_records_warning() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let mut metadata = project.run("invalid branch metadata", "noop").unwrap();
    metadata.branch = "main".to_string();
    project.write_metadata(&metadata).unwrap();

    let discarded = project.discard(&metadata.run_id).unwrap();

    assert_eq!(discarded.status, RunStatus::Discarded);
    assert!(branch_exists(
        &temp,
        &format!("keel/run/{}", metadata.run_id)
    ));
    assert!(discarded
        .warnings
        .iter()
        .any(|warning| warning.contains("metadata branch `main`")));
    let report = read_run_file(&temp, &metadata.run_id, REPORT_FILE);
    assert!(report.contains("Branch cleanup: `skipped invalid metadata`"));
    assert!(report.contains("Warning: candidate branch cleanup skipped"));
}

#[test]
fn noop_run_force_adds_ignored_output_file() {
    let temp = git_repo_with_files(&[(".gitignore", "*.txt\n")]);
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let metadata = project.run("ignored noop output", "noop").unwrap();

    assert_eq!(metadata.status, RunStatus::Ready);
    let diff = read_run_file(&temp, &metadata.run_id, DIFF_FILE);
    assert!(!diff.trim().is_empty());
    assert!(diff.contains(NOOP_OUTPUT_FILE));
}
