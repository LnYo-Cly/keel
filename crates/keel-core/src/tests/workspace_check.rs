use super::*;
use crate::{WorkspaceCheckOptions, WorkspaceCheckStatus};

#[test]
fn workspace_check_dry_run_plans_configured_commands_without_evidence() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    write_config(
        &temp,
        r#"
[checks]
commands = ["git --version"]
"#,
    );
    project.start_ledger_task("check dry run").unwrap();

    let result = project
        .check(WorkspaceCheckOptions { dry_run: true })
        .unwrap();
    let review = project.ledger_review().unwrap();

    assert!(result.ok);
    assert!(result.dry_run);
    assert_eq!(result.summary.planned, 1);
    assert_eq!(result.commands[0].status, WorkspaceCheckStatus::Planned);
    assert_eq!(review.summary.evidence, 0);
}

#[test]
fn workspace_check_records_passing_and_failing_evidence() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    write_config(
        &temp,
        r#"
[checks]
commands = ["git --version", "definitely-not-a-keel-check-command"]
"#,
    );
    project.start_ledger_task("check evidence").unwrap();

    let result = project
        .check(WorkspaceCheckOptions { dry_run: false })
        .unwrap();
    let review = project.ledger_review().unwrap();

    assert!(!result.ok);
    assert_eq!(result.summary.passed, 1);
    assert_eq!(result.summary.failed, 1);
    assert_eq!(result.commands[0].status, WorkspaceCheckStatus::Passed);
    assert_eq!(result.commands[1].status, WorkspaceCheckStatus::Failed);
    assert_eq!(review.summary.evidence, 2);
    assert_eq!(review.summary.evidence_failed, 1);
    assert!(!review.decision.ready);
}

#[test]
fn workspace_check_skips_legacy_check_when_required_path_is_missing() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    write_config(
        &temp,
        r#"
[[checks]]
name = "missing path"
command = ["git", "--version"]
run_if_path_exists = "missing.file"
"#,
    );
    project.start_ledger_task("check skip").unwrap();

    let result = project
        .check(WorkspaceCheckOptions { dry_run: false })
        .unwrap();
    let review = project.ledger_review().unwrap();

    assert!(result.ok);
    assert_eq!(result.summary.skipped, 1);
    assert_eq!(result.commands[0].status, WorkspaceCheckStatus::Skipped);
    assert_eq!(review.summary.evidence, 0);
}

#[test]
fn workspace_check_requires_active_task_and_configured_commands() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let missing_task = project
        .check(WorkspaceCheckOptions { dry_run: false })
        .unwrap_err()
        .to_string();
    assert!(missing_task.contains("no active Keel task found"));

    project.start_ledger_task("missing checks").unwrap();
    write_config(
        &temp,
        r#"
[checks]
commands = []
"#,
    );

    let missing_checks = project
        .check(WorkspaceCheckOptions { dry_run: false })
        .unwrap_err()
        .to_string();
    assert!(missing_checks.contains("no workspace checks configured"));
}
