use super::*;

#[test]
fn commit_dry_run_plans_without_writing_artifacts() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("dry run commit", "noop").unwrap();
    let metadata_before = read_run_file(&temp, &metadata.run_id, METADATA_FILE);
    let report_before = read_run_file(&temp, &metadata.run_id, REPORT_FILE);

    let result = project
        .commit(
            &metadata.run_id,
            CommitOptions {
                dry_run: true,
                message: None,
            },
        )
        .unwrap();

    assert!(result.dry_run);
    assert!(!result.committed);
    assert!(result.would_git_add);
    assert!(result.would_git_commit);
    assert_eq!(result.commit_message, "keel: dry run commit");
    assert!(!run_dir(&temp, &metadata.run_id).join(COMMIT_FILE).exists());
    assert_eq!(
        read_run_file(&temp, &metadata.run_id, METADATA_FILE),
        metadata_before
    );
    assert_eq!(
        read_run_file(&temp, &metadata.run_id, REPORT_FILE),
        report_before
    );
}

#[test]
fn commit_ready_run_writes_artifact_metadata_report_and_git_commit() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("local commit task", "noop").unwrap();

    let result = project
        .commit(
            &metadata.run_id,
            CommitOptions {
                dry_run: false,
                message: Some("keel: custom local commit".to_string()),
            },
        )
        .unwrap();

    assert!(result.committed);
    assert!(!result.already_committed);
    assert_eq!(result.commit_message, "keel: custom local commit");
    let commit_sha = result.commit_sha.as_ref().unwrap();
    assert!(!commit_sha.is_empty());
    assert!(run_dir(&temp, &metadata.run_id).join(COMMIT_FILE).is_file());

    let updated = read_metadata(&temp, &metadata.run_id);
    assert!(updated.committed);
    assert_eq!(updated.commit_sha.as_deref(), Some(commit_sha.as_str()));
    assert_eq!(
        updated.commit_message.as_deref(),
        Some("keel: custom local commit")
    );
    assert!(updated.committed_at.is_some());
    assert!(updated.commit.is_some());

    let report = read_run_file(&temp, &metadata.run_id, REPORT_FILE);
    assert!(report.contains("## Commit"));
    assert!(report.contains("Keel did not push or merge anything."));
    assert!(report.contains(commit_sha));

    let git_subject = git_stdout(
        &worktree_dir(&temp, &metadata.run_id),
        &["log", "-1", "--format=%s"],
    );
    assert_eq!(git_subject, "keel: custom local commit");
}

#[test]
fn commit_is_idempotent() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("idempotent commit", "noop").unwrap();

    let first = project
        .commit(
            &metadata.run_id,
            CommitOptions {
                dry_run: false,
                message: None,
            },
        )
        .unwrap();
    let second = project
        .commit(
            &metadata.run_id,
            CommitOptions {
                dry_run: false,
                message: None,
            },
        )
        .unwrap();

    assert!(second.already_committed);
    assert_eq!(first.commit_sha, second.commit_sha);
    let commit_count = git_stdout(
        &worktree_dir(&temp, &metadata.run_id),
        &["rev-list", "--count", "HEAD"],
    );
    assert_eq!(commit_count, "2");
}

#[test]
fn discard_after_commit_removes_worktree_but_preserves_candidate_branch() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("commit then discard", "noop").unwrap();
    let commit = project
        .commit(
            &metadata.run_id,
            CommitOptions {
                dry_run: false,
                message: None,
            },
        )
        .unwrap();

    let discarded = project.discard(&metadata.run_id).unwrap();

    assert!(commit.committed);
    assert_eq!(discarded.status, RunStatus::Discarded);
    assert!(!worktree_dir(&temp, &metadata.run_id).exists());
    assert!(branch_exists(&temp, &metadata.branch));
    assert_eq!(discarded.commit_sha, commit.commit_sha);
    let report = read_run_file(&temp, &metadata.run_id, REPORT_FILE);
    assert!(report.contains("Branch cleanup: `preserved committed branch`"));
    assert!(report.contains("## Commit"));
    assert!(run_dir(&temp, &metadata.run_id).join(COMMIT_FILE).is_file());
}

#[test]
fn commit_rejects_not_ready_and_discarded_runs() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let mut metadata = project.run("not ready commit", "noop").unwrap();
    metadata.status = RunStatus::NotReady;
    project.write_metadata(&metadata).unwrap();
    let error = project
        .commit(
            &metadata.run_id,
            CommitOptions {
                dry_run: false,
                message: None,
            },
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("only ready runs can be committed"));

    metadata.status = RunStatus::Ready;
    project.write_metadata(&metadata).unwrap();
    let discarded = project.discard(&metadata.run_id).unwrap();
    let error = project
        .commit(
            &discarded.run_id,
            CommitOptions {
                dry_run: false,
                message: None,
            },
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("only ready runs can be committed"));
}

#[test]
fn commit_with_risk_warnings_still_succeeds() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    write_config(
        &temp,
        r#"version = 1
runs_dir = "runs"
worktrees_dir = "worktrees"

[[checks]]
name = "git status"
command = ["git", "status", "--short"]

[risk]
paths = ["keel-noop-output.txt"]
"#,
    );
    let metadata = project.run("commit warning task", "noop").unwrap();
    assert!(!metadata.warnings.is_empty());

    let result = project
        .commit(
            &metadata.run_id,
            CommitOptions {
                dry_run: false,
                message: None,
            },
        )
        .unwrap();

    assert!(result.committed);
    assert!(!result.warnings.is_empty());
    let commit = read_run_file(&temp, &metadata.run_id, COMMIT_FILE);
    assert!(commit.contains("touched risk path"));
    let report = read_run_file(&temp, &metadata.run_id, REPORT_FILE);
    assert!(report.contains("touched risk path"));
}
