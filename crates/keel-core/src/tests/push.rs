use super::*;

#[test]
fn push_rejects_uncommitted_missing_remote_and_discarded_runs() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let metadata = project.run("push rejection task", "noop").unwrap();
    let error = project
        .push(&metadata.run_id, push_options(false))
        .unwrap_err()
        .to_string();
    assert!(error.contains("is not committed"));
    assert!(error.contains("keel commit"));

    project
        .commit(
            &metadata.run_id,
            CommitOptions {
                dry_run: false,
                message: None,
            },
        )
        .unwrap();
    let error = project
        .push(&metadata.run_id, push_options(false))
        .unwrap_err()
        .to_string();
    assert!(error.contains("git remote `origin` does not exist"));

    let discarded = project.discard(&metadata.run_id).unwrap();
    let error = project
        .push(&discarded.run_id, push_options(false))
        .unwrap_err()
        .to_string();
    assert!(error.contains("only ready runs can be pushed"));
}

#[test]
fn push_dry_run_plans_without_writing_artifacts() {
    let temp = git_repo();
    let remote = bare_git_repo();
    add_origin(&temp, &remote);
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("push dry run", "noop").unwrap();
    let commit = project
        .commit(
            &metadata.run_id,
            CommitOptions {
                dry_run: false,
                message: None,
            },
        )
        .unwrap();
    let metadata_before = read_run_file(&temp, &metadata.run_id, METADATA_FILE);
    let report_before = read_run_file(&temp, &metadata.run_id, REPORT_FILE);

    let result = project.push(&metadata.run_id, push_options(true)).unwrap();

    assert!(result.dry_run);
    assert!(!result.pushed);
    assert!(result.would_push);
    assert_eq!(result.commit_sha, commit.commit_sha.unwrap());
    assert_eq!(result.branch, metadata.branch);
    assert!(!run_dir(&temp, &metadata.run_id).join(PUSH_FILE).exists());
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
fn push_success_writes_artifact_metadata_report_and_pushes_candidate_branch() {
    let temp = git_repo();
    let remote = bare_git_repo();
    add_origin(&temp, &remote);
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("push success", "noop").unwrap();
    let commit = project
        .commit(
            &metadata.run_id,
            CommitOptions {
                dry_run: false,
                message: None,
            },
        )
        .unwrap();

    let result = project.push(&metadata.run_id, push_options(false)).unwrap();

    assert!(result.pushed);
    assert!(!result.already_pushed);
    assert_eq!(result.remote, "origin");
    assert_eq!(result.branch, metadata.branch);
    assert_eq!(result.commit_sha, commit.commit_sha.unwrap());
    assert!(run_dir(&temp, &metadata.run_id).join(PUSH_FILE).is_file());
    assert_eq!(
        git_stdout(remote.path(), &["rev-parse", &metadata.branch]),
        result.commit_sha
    );

    let updated = read_metadata(&temp, &metadata.run_id);
    assert!(updated.pushed);
    assert_eq!(updated.push_remote.as_deref(), Some("origin"));
    assert_eq!(
        updated.pushed_branch.as_deref(),
        Some(metadata.branch.as_str())
    );
    assert!(updated.pushed_at.is_some());
    assert!(updated.push.is_some());

    let report = read_run_file(&temp, &metadata.run_id, REPORT_FILE);
    assert!(report.contains("## Push"));
    assert!(report.contains("Keel did not create a PR/MR."));
    assert!(report.contains("Keel did not merge anything."));
}

#[test]
fn push_is_idempotent() {
    let temp = git_repo();
    let remote = bare_git_repo();
    add_origin(&temp, &remote);
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("push idempotent", "noop").unwrap();
    project
        .commit(
            &metadata.run_id,
            CommitOptions {
                dry_run: false,
                message: None,
            },
        )
        .unwrap();

    let first = project.push(&metadata.run_id, push_options(false)).unwrap();
    let report_after_first = read_run_file(&temp, &metadata.run_id, REPORT_FILE);
    let second = project.push(&metadata.run_id, push_options(false)).unwrap();

    assert!(first.pushed);
    assert!(second.already_pushed);
    assert_eq!(first.commit_sha, second.commit_sha);
    assert_eq!(
        read_run_file(&temp, &metadata.run_id, REPORT_FILE),
        report_after_first
    );
}

#[test]
fn push_reads_legacy_publish_metadata_and_artifact_without_rewriting() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("legacy publish compatibility", "noop").unwrap();
    let commit = project
        .commit(
            &metadata.run_id,
            CommitOptions {
                dry_run: false,
                message: None,
            },
        )
        .unwrap();
    let commit_sha = commit.commit_sha.unwrap();
    let run_dir = run_dir(&temp, &metadata.run_id);
    let legacy_publish_path = run_dir.join("publish.json");
    let legacy_remote_url = temp.path().join("legacy-remote.git");

    let mut legacy_metadata = read_metadata(&temp, &metadata.run_id);
    let legacy_json = serde_json::json!({
        "run_id": metadata.run_id,
        "parent_run_id": null,
        "task": "legacy publish compatibility",
        "agent": "noop",
        "status": "ready",
        "created_at": legacy_metadata.created_at,
        "updated_at": legacy_metadata.updated_at,
        "started_at": legacy_metadata.started_at,
        "finished_at": legacy_metadata.finished_at,
        "duration_ms": legacy_metadata.duration_ms,
        "worktree_path": legacy_metadata.worktree_path,
        "run_dir": legacy_metadata.run_dir,
        "branch": legacy_metadata.branch,
        "base_commit": legacy_metadata.base_commit,
        "agent_command": legacy_metadata.agent_command,
        "exit_code": legacy_metadata.exit_code,
        "failure_reason": legacy_metadata.failure_reason,
        "readiness_reason": legacy_metadata.readiness_reason,
        "warnings": legacy_metadata.warnings,
        "risk_warnings": legacy_metadata.risk_warnings,
        "committed": true,
        "commit_sha": commit_sha,
        "commit_message": legacy_metadata.commit_message,
        "committed_at": legacy_metadata.committed_at,
        "commit": legacy_metadata.commit,
        "published": true,
        "published_at": "2026-04-30T00:00:00Z",
        "publish_remote": "origin",
        "publish_remote_url": legacy_remote_url.to_string_lossy(),
        "published_branch": legacy_metadata.branch,
        "publish": {
            "run_id": metadata.run_id,
            "remote": "origin",
            "remote_url": legacy_remote_url.to_string_lossy(),
            "branch": legacy_metadata.branch,
            "commit_sha": commit_sha,
            "pushed": true,
            "published_at": "2026-04-30T00:00:00Z",
            "dry_run": false
        }
    });
    fs::write(
        run_dir.join(METADATA_FILE),
        serde_json::to_string_pretty(&legacy_json).unwrap(),
    )
    .unwrap();
    fs::write(
        &legacy_publish_path,
        serde_json::to_string_pretty(&legacy_json["publish"]).unwrap(),
    )
    .unwrap();

    legacy_metadata = read_metadata(&temp, &metadata.run_id);
    assert!(legacy_metadata.pushed);
    assert_eq!(legacy_metadata.push_remote.as_deref(), Some("origin"));
    assert!(legacy_metadata.push.is_some());

    let result = project.push(&metadata.run_id, push_options(false)).unwrap();

    assert!(result.already_pushed);
    assert_eq!(
        normalize_path(result.push_path.as_deref().unwrap()),
        normalize_path(legacy_publish_path.to_str().unwrap())
    );
    assert!(!run_dir.join(PUSH_FILE).exists());

    let report = project.report(&metadata.run_id).unwrap();
    let push_artifact = report
        .artifacts
        .iter()
        .find(|artifact| artifact.label == "Push")
        .unwrap();
    assert!(push_artifact.exists);
    assert_eq!(push_artifact.path, legacy_publish_path);
}

#[test]
fn push_rejects_candidate_branch_head_mismatch() {
    let temp = git_repo();
    let remote = bare_git_repo();
    add_origin(&temp, &remote);
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("push branch mismatch", "noop").unwrap();
    project
        .commit(
            &metadata.run_id,
            CommitOptions {
                dry_run: false,
                message: None,
            },
        )
        .unwrap();
    fs::write(
        worktree_dir(&temp, &metadata.run_id).join("after-commit.txt"),
        "extra\n",
    )
    .unwrap();
    git(
        &worktree_dir(&temp, &metadata.run_id),
        &["add", "after-commit.txt"],
    );
    git(
        &worktree_dir(&temp, &metadata.run_id),
        &["commit", "-m", "extra change"],
    );

    let error = project
        .push(&metadata.run_id, push_options(false))
        .unwrap_err()
        .to_string();

    assert!(error.contains("does not match committed run SHA"));
}
