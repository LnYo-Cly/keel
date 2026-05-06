use super::*;

#[test]
fn commit_dry_run_plans_without_writing_artifacts() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("dry run commit", "noop").unwrap();
    let metadata_before = read_run_file(&temp, &metadata.run_id, artifact_files::METADATA);
    let report_before = read_run_file(&temp, &metadata.run_id, artifact_files::REPORT);

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
    assert!(!run_dir(&temp, &metadata.run_id)
        .join(artifact_files::COMMIT)
        .exists());
    assert_eq!(
        read_run_file(&temp, &metadata.run_id, artifact_files::METADATA),
        metadata_before
    );
    assert_eq!(
        read_run_file(&temp, &metadata.run_id, artifact_files::REPORT),
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
    assert!(run_dir(&temp, &metadata.run_id)
        .join(artifact_files::COMMIT)
        .is_file());

    let updated = read_metadata(&temp, &metadata.run_id);
    assert!(updated.committed);
    assert_eq!(updated.commit_sha.as_deref(), Some(commit_sha.as_str()));
    assert_eq!(
        updated.commit_message.as_deref(),
        Some("keel: custom local commit")
    );
    assert!(updated.committed_at.is_some());
    assert!(updated.commit.is_some());

    let report = read_run_file(&temp, &metadata.run_id, artifact_files::REPORT);
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
fn metadata_review_state_accessors_handle_nested_artifacts_and_legacy_fields() {
    let mut metadata = RunMetadata::new(
        "run-accessors",
        "metadata accessors",
        "noop",
        RunStatus::Ready,
        "2026-05-01T00:00:00Z",
    );

    assert!(!metadata.has_commit_record());
    assert!(!metadata.has_push_record());
    assert!(!metadata.has_pr_record());
    assert!(!metadata.run_artifact_recorded(artifact_keys::COMMIT));
    assert!(!metadata.run_artifact_recorded(artifact_keys::PUSH));
    assert!(!metadata.run_artifact_recorded(artifact_keys::PR));
    assert!(metadata.run_artifact_recorded(artifact_keys::REPORT));

    metadata.commit = Some(crate::CommitArtifact {
        run_id: metadata.run_id.clone(),
        branch: metadata.branch.clone(),
        worktree: metadata.worktree_path.clone(),
        commit_sha: "abc123".to_string(),
        commit_message: "keel: metadata accessors".to_string(),
        committed_at: "2026-05-01T00:01:00Z".to_string(),
        had_uncommitted_changes: true,
        warnings: Vec::new(),
        dry_run: false,
    });
    metadata.push = Some(crate::PushArtifact {
        run_id: metadata.run_id.clone(),
        remote: "origin".to_string(),
        remote_url: "git@github.com:owner/repo.git".to_string(),
        branch: metadata.branch.clone(),
        commit_sha: "abc123".to_string(),
        pushed: true,
        pushed_at: "2026-05-01T00:02:00Z".to_string(),
        dry_run: false,
    });
    metadata.pr = Some(crate::PrArtifact {
        run_id: metadata.run_id.clone(),
        provider: PrProvider::Github,
        provider_name: "GitHub".to_string(),
        request_kind: "pull_request".to_string(),
        remote: "origin".to_string(),
        remote_url: "git@github.com:owner/repo.git".to_string(),
        repository_url: Some("https://github.com/owner/repo".to_string()),
        source_branch: metadata.branch.clone(),
        target_branch: "main".to_string(),
        commit_sha: "abc123".to_string(),
        title: "keel: metadata accessors".to_string(),
        url: "https://github.com/owner/repo/pull/1".to_string(),
        created_at: "2026-05-01T00:03:00Z".to_string(),
        draft: true,
        reused_existing: false,
        dry_run: false,
    });

    assert_eq!(metadata.recorded_commit_sha(), Some("abc123"));
    assert_eq!(metadata.recorded_push_remote(), Some("origin"));
    assert_eq!(
        metadata.recorded_push_remote_url(),
        Some("git@github.com:owner/repo.git")
    );
    assert_eq!(
        metadata.recorded_pushed_branch(),
        Some(metadata.branch.as_str())
    );
    assert_eq!(
        metadata.recorded_pr_url(),
        Some("https://github.com/owner/repo/pull/1")
    );
    assert!(metadata.has_commit_record());
    assert!(metadata.has_push_record());
    assert!(metadata.has_pr_record());
    assert!(metadata.run_artifact_recorded(artifact_keys::COMMIT));
    assert!(metadata.run_artifact_recorded(artifact_keys::PUSH));
    assert!(metadata.run_artifact_recorded(artifact_keys::PR));
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
    let report = read_run_file(&temp, &metadata.run_id, artifact_files::REPORT);
    assert!(report.contains("Branch cleanup: `preserved committed branch`"));
    assert!(report.contains("## Commit"));
    assert!(run_dir(&temp, &metadata.run_id)
        .join(artifact_files::COMMIT)
        .is_file());
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
    let commit = read_run_file(&temp, &metadata.run_id, artifact_files::COMMIT);
    assert!(commit.contains("touched risk path"));
    let report = read_run_file(&temp, &metadata.run_id, artifact_files::REPORT);
    assert!(report.contains("touched risk path"));
}
