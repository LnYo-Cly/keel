use super::*;

#[test]
fn rerun_creates_fresh_child_and_appends_source_report() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let source = project.run("rerun this task", "noop").unwrap();
    let child = project.rerun(&source.run_id).unwrap();

    assert_ne!(source.run_id, child.run_id);
    assert_eq!(child.task, source.task);
    assert_eq!(child.agent, source.agent);
    assert_eq!(child.parent_run_id, Some(source.run_id.clone()));
    assert!(worktree_dir(&temp, &child.run_id).exists());

    let child_metadata = read_metadata(&temp, &child.run_id);
    assert_eq!(child_metadata.parent_run_id, Some(source.run_id.clone()));
    let source_report = read_run_file(&temp, &source.run_id, artifact_files::REPORT);
    assert!(source_report.contains("## Rerun"));
    assert!(source_report.contains(&format!("Created rerun: `{}`", child.run_id)));

    let child_report = read_run_file(&temp, &child.run_id, artifact_files::REPORT);
    assert!(child_report.contains(&format!("Parent Run ID: `{}`", source.run_id)));
    assert!(child_report.contains("## Artifacts"));
    assert!(child_report.contains("## Suggested Next Actions"));
    assert!(child_report.contains(&format!("keel rerun {}", child.run_id)));
}

#[test]
fn discarded_run_can_be_rerun_without_restoring_source_worktree() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let source = project.run("rerun discarded source", "noop").unwrap();
    project.discard(&source.run_id).unwrap();

    let child = project.rerun(&source.run_id).unwrap();

    assert_eq!(child.parent_run_id, Some(source.run_id.clone()));
    assert!(!worktree_dir(&temp, &source.run_id).exists());
    assert!(worktree_dir(&temp, &child.run_id).exists());
    let source_report = read_run_file(&temp, &source.run_id, artifact_files::REPORT);
    assert!(source_report.contains("## Discard"));
    assert!(source_report.contains("## Rerun"));
}

#[test]
fn discarded_run_remains_reportable_and_diffable() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("review after discard", "noop").unwrap();

    project.discard(&metadata.run_id).unwrap();

    let report = project.report(&metadata.run_id).unwrap();
    assert!(report.is_discarded);
    assert!(!report
        .next_actions
        .contains(&format!("keel discard {}", metadata.run_id)));

    let diff = project.diff(&metadata.run_id).unwrap();
    assert!(!diff.is_empty);
    assert!(diff.content.contains(NOOP_OUTPUT_FILE));

    let log = project.log(&metadata.run_id).unwrap();
    assert!(!log.is_empty);
    assert!(log.content.contains("marked discarded"));
}

#[test]
fn rerun_rejects_unsupported_source_agent_without_appending_report() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let mut source = project.run("unsupported source agent", "noop").unwrap();
    source.agent = "manual".to_string();
    project.write_metadata(&source).unwrap();

    let error = project.rerun(&source.run_id).unwrap_err().to_string();

    assert!(error.contains("unsupported agent"));
    assert_eq!(project.list_runs().unwrap().len(), 1);
    let source_report = read_run_file(&temp, &source.run_id, artifact_files::REPORT);
    assert!(!source_report.contains("## Rerun"));
}

#[test]
fn run_uses_configured_checks() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    write_config(
        &temp,
        r#"version = 1
runs_dir = "runs"
worktrees_dir = "worktrees"

[[checks]]
name = "custom status"
command = ["git", "status", "--short"]
"#,
    );

    let metadata = project.run("custom configured check", "noop").unwrap();

    let checks = read_checks(&temp, &metadata.run_id);
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].name, "custom status");
}

#[test]
fn run_uses_new_check_command_strings() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    write_config(
        &temp,
        r#"version = 1
runs_dir = "runs"
worktrees_dir = "worktrees"

[checks]
commands = ["git status --short"]
"#,
    );

    let metadata = project.run("new configured check strings", "noop").unwrap();

    let checks = read_checks(&temp, &metadata.run_id);
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].name, "check 1");
    assert_eq!(checks[0].command, "git status --short");
    assert_eq!(checks[0].status, CheckStatus::Passed);
}

#[test]
fn failing_configured_check_blocks_ready() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    write_config(
        &temp,
        r#"version = 1
runs_dir = "runs"
worktrees_dir = "worktrees"

[[checks]]
name = "failing check"
command = ["git", "not-a-real-keel-test-command"]
"#,
    );

    let metadata = project.run("failing configured check", "noop").unwrap();

    assert_eq!(metadata.status, RunStatus::NotReady);
    assert_eq!(metadata.failure_reason, Some(FailureReason::CheckFailed));
    assert!(metadata.readiness_reason.contains("failed checks"));
    let checks = read_checks(&temp, &metadata.run_id);
    assert_eq!(checks[0].name, "failing check");
    assert_eq!(checks[0].status, CheckStatus::Failed);
}

#[test]
fn risk_path_warning_is_persisted_in_report_and_metadata() {
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
paths = ["src/auth/**"]
"#,
    );

    let metadata = project
        .run_with_adapter(
            "touch auth session",
            &FileChangeAgent::new(&[("src/auth/session.rs", "session\n")]),
        )
        .unwrap();

    assert_eq!(metadata.status, RunStatus::Ready);
    assert!(has_risk_warning(&metadata, RiskWarningKind::RiskPath));
    assert!(metadata
        .warnings
        .iter()
        .any(|warning| warning.contains("touched risk path: src/auth/session.rs")));
    let report = read_run_file(&temp, &metadata.run_id, artifact_files::REPORT);
    assert!(report.contains("## Warnings"));
    assert!(report.contains("touched risk path: src/auth/session.rs matched src/auth/**"));
}

#[test]
fn built_in_risk_warnings_cover_manifest_lockfile_deleted_and_large_diff() {
    let temp = git_repo_with_files(&[
        ("Cargo.toml", "[package]\nname = \"fixture\"\n"),
        ("Cargo.lock", "# lock\n"),
        ("old.txt", "old\n"),
    ]);
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
large_diff_file_threshold = 1
"#,
    );

    let metadata = project
        .run_with_adapter(
            "touch risky builtins",
            &FileChangeAgent::new(&[
                ("Cargo.toml", "[package]\nname = \"changed\"\n"),
                ("Cargo.lock", "# changed lock\n"),
                ("new.txt", "new\n"),
            ])
            .delete("old.txt"),
        )
        .unwrap();

    assert_eq!(metadata.status, RunStatus::Ready);
    for kind in [
        RiskWarningKind::DependencyManifest,
        RiskWarningKind::Lockfile,
        RiskWarningKind::DeletedFile,
        RiskWarningKind::LargeDiff,
    ] {
        assert!(
            has_risk_warning(&metadata, kind.clone()),
            "missing risk warning kind {kind:?}"
        );
    }
    assert_eq!(metadata.failure_reason, None);
    assert!(metadata
        .readiness_reason
        .contains("agent exited successfully"));
}
