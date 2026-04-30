use crate::agents::{
    AgentAdapter, AgentExecution, AgentRunContext, ClaudeAgent, CodexAgent, OpenCodeAgent,
};
use crate::command::resolve_windows_program_from_path;
use crate::commit::CommitOptions;
use crate::config::{validate_config, ConfigValidationSeverity};
use crate::constants::{
    CHECKS_FILE, COMMIT_FILE, CONFIG_FILE, DEFAULT_AGENT_TIMEOUT_SECS, DIFF_FILE, KEEL_DIR,
    LOG_FILE, METADATA_FILE, NOOP_OUTPUT_FILE, PUSH_FILE, REPORT_FILE, RUNS_DIR, WORKTREES_DIR,
};
use crate::json::read_json;
use crate::model::{CheckResult, CheckStatus, FailureReason, RunMetadata, RunStatus};
use crate::pr::{PrOptions, PrProvider};
use crate::project::{compare_created_at_for_test, KeelProject};
use crate::push::PushOptions;
use crate::risk::RiskWarningKind;
use anyhow::{bail, Result};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

#[test]
fn discover_requires_git_repo() {
    let temp = TempDir::new().unwrap();
    let error = KeelProject::discover(temp.path()).unwrap_err().to_string();
    assert!(error.contains("git repository"));
}

#[test]
fn init_creates_keel_layout() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();

    let result = project.init().unwrap();

    assert!(result.config_path.exists());
    assert!(result.runs_dir.exists());
    assert!(result.keel_dir.join(WORKTREES_DIR).exists());
    let config = fs::read_to_string(result.config_path).unwrap();
    assert!(config.contains("agent_timeout_secs"));
}

#[test]
fn config_validation_accepts_default_legacy_config() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let report = validate_config(temp.path());

    assert!(report.ok);
    assert_eq!(
        issue_severity(&report, "config.exists"),
        ConfigValidationSeverity::Ok
    );
    assert_eq!(
        issue_severity(&report, "config.parse"),
        ConfigValidationSeverity::Ok
    );
    assert_eq!(
        issue_severity(&report, "checks.commands"),
        ConfigValidationSeverity::Ok
    );
    assert_eq!(
        issue_severity(&report, "agents.codex.timeout_seconds"),
        ConfigValidationSeverity::Ok
    );
}

#[test]
fn config_validation_reports_missing_or_invalid_config() {
    let temp = git_repo();

    let missing = validate_config(temp.path());

    assert!(!missing.ok);
    assert_eq!(
        issue_severity(&missing, "config.exists"),
        ConfigValidationSeverity::Error
    );

    let config_dir = temp.path().join(KEEL_DIR);
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(config_dir.join(CONFIG_FILE), "not = [valid").unwrap();

    let invalid = validate_config(temp.path());

    assert!(!invalid.ok);
    assert_eq!(
        issue_severity(&invalid, "config.parse"),
        ConfigValidationSeverity::Error
    );
}

#[test]
fn config_validation_rejects_zero_timeout_and_empty_check_command() {
    let temp = git_repo();
    let config_dir = temp.path().join(KEEL_DIR);
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join(CONFIG_FILE),
        r#"
[checks]
commands = [""]

[agents.codex]
timeout_seconds = 0
"#,
    )
    .unwrap();

    let report = validate_config(temp.path());

    assert!(!report.ok);
    assert_eq!(
        issue_severity(&report, "checks.commands.empty"),
        ConfigValidationSeverity::Error
    );
    assert_eq!(
        issue_severity(&report, "agents.codex.timeout_seconds"),
        ConfigValidationSeverity::Error
    );
}

#[test]
fn config_validation_warns_for_empty_future_checks_commands() {
    let temp = git_repo();
    let config_dir = temp.path().join(KEEL_DIR);
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join(CONFIG_FILE),
        r#"
[checks]
commands = []
"#,
    )
    .unwrap();

    let report = validate_config(temp.path());

    assert!(report.ok);
    assert_eq!(
        issue_severity(&report, "checks.commands"),
        ConfigValidationSeverity::Warning
    );
}

#[test]
fn config_validation_accepts_default_risk_config() {
    let temp = git_repo();
    let config_dir = temp.path().join(KEEL_DIR);
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join(CONFIG_FILE),
        r#"
[checks]
commands = []
"#,
    )
    .unwrap();

    let report = validate_config(temp.path());

    assert!(report.ok);
    assert_eq!(
        issue_severity(&report, "risk.paths"),
        ConfigValidationSeverity::Ok
    );
    assert_eq!(
        issue_severity(&report, "risk.large_diff_file_threshold"),
        ConfigValidationSeverity::Ok
    );
}

#[test]
fn config_validation_rejects_invalid_risk_config() {
    let temp = git_repo();
    let config_dir = temp.path().join(KEEL_DIR);
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join(CONFIG_FILE),
        r#"
[risk]
paths = ["", "["]
large_diff_file_threshold = 0
"#,
    )
    .unwrap();

    let report = validate_config(temp.path());

    assert!(!report.ok);
    assert_eq!(
        issue_severity(&report, "risk.paths.empty"),
        ConfigValidationSeverity::Error
    );
    assert_eq!(
        issue_severity(&report, "risk.paths.glob"),
        ConfigValidationSeverity::Error
    );
    assert_eq!(
        issue_severity(&report, "risk.large_diff_file_threshold"),
        ConfigValidationSeverity::Error
    );
}

fn issue_severity(
    report: &crate::config::ConfigValidationReport,
    id: &str,
) -> ConfigValidationSeverity {
    report
        .issues
        .iter()
        .find(|issue| issue.id == id)
        .map(|issue| issue.severity)
        .unwrap()
}

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

#[test]
fn pr_manual_dry_run_rejects_invalid_run_states() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let missing = project
        .pr_plan("run-does-not-exist", pr_options(None))
        .unwrap_err()
        .to_string();
    assert!(missing.contains("run `run-does-not-exist` does not exist"));

    let uncommitted = project.run("pr uncommitted", "noop").unwrap();
    let error = project
        .pr_plan(&uncommitted.run_id, pr_options(None))
        .unwrap_err()
        .to_string();
    assert!(error.contains("is not committed"));

    project
        .commit(
            &uncommitted.run_id,
            CommitOptions {
                dry_run: false,
                message: None,
            },
        )
        .unwrap();
    let error = project
        .pr_plan(&uncommitted.run_id, pr_options(None))
        .unwrap_err()
        .to_string();
    assert!(error.contains("is not pushed"));

    let discarded = project.discard(&uncommitted.run_id).unwrap();
    let error = project
        .pr_plan(&discarded.run_id, pr_options(None))
        .unwrap_err()
        .to_string();
    assert!(error.contains("only ready runs can create a PR/MR plan"));
}

#[test]
fn pr_manual_dry_run_builds_plan_from_pushed_metadata() {
    let temp = git_repo();
    let remote = bare_git_repo();
    add_origin(&temp, &remote);
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("manual pr plan", "noop").unwrap();
    let commit = project
        .commit(
            &metadata.run_id,
            CommitOptions {
                dry_run: false,
                message: None,
            },
        )
        .unwrap();
    project.push(&metadata.run_id, push_options(false)).unwrap();
    let before_metadata = read_run_file(&temp, &metadata.run_id, METADATA_FILE);
    let before_report = read_run_file(&temp, &metadata.run_id, REPORT_FILE);

    let plan = project
        .pr_plan(&metadata.run_id, pr_options(Some(PrProvider::Github)))
        .unwrap();

    assert_eq!(plan.provider, PrProvider::Github);
    assert_eq!(plan.request_kind, "pull_request");
    assert_eq!(plan.source_branch, metadata.branch);
    assert_eq!(
        plan.target_branch,
        git_stdout(temp.path(), &["branch", "--show-current"])
    );
    assert_eq!(plan.commit_sha, commit.commit_sha.unwrap());
    assert_eq!(plan.title, "keel: manual pr plan");
    assert!(plan.body.contains(&metadata.run_id));
    assert!(!plan.would_create_request);
    assert!(!plan.would_write_artifact);
    assert!(!plan.would_push);
    assert!(!plan.would_merge);
    assert!(!run_dir(&temp, &metadata.run_id).join("pr.json").exists());
    assert_eq!(
        read_run_file(&temp, &metadata.run_id, METADATA_FILE),
        before_metadata
    );
    assert_eq!(
        read_run_file(&temp, &metadata.run_id, REPORT_FILE),
        before_report
    );
}

#[test]
fn pr_manual_dry_run_infers_provider_from_remote_url() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("provider inference", "noop").unwrap();
    project
        .commit(
            &metadata.run_id,
            CommitOptions {
                dry_run: false,
                message: None,
            },
        )
        .unwrap();
    let mut pushed = read_metadata(&temp, &metadata.run_id);
    pushed.pushed = true;
    pushed.pushed_at = Some("2026-04-30T00:00:00Z".to_string());
    pushed.push_remote = Some("origin".to_string());
    pushed.push_remote_url = Some("git@gitlab.com:owner/repo.git".to_string());
    pushed.pushed_branch = Some(pushed.branch.clone());
    pushed.push = None;
    project.write_metadata(&pushed).unwrap();

    let plan = project.pr_plan(&metadata.run_id, pr_options(None)).unwrap();

    assert_eq!(plan.provider, PrProvider::Gitlab);
    assert_eq!(plan.request_kind, "merge_request");
    assert_eq!(
        plan.repository_url.as_deref(),
        Some("https://gitlab.com/owner/repo")
    );
}

#[test]
fn pr_manual_dry_run_rejects_unknown_provider_without_override() {
    let temp = git_repo();
    let remote = bare_git_repo();
    add_origin(&temp, &remote);
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("unknown provider", "noop").unwrap();
    project
        .commit(
            &metadata.run_id,
            CommitOptions {
                dry_run: false,
                message: None,
            },
        )
        .unwrap();
    project.push(&metadata.run_id, push_options(false)).unwrap();

    let error = project
        .pr_plan(&metadata.run_id, pr_options(None))
        .unwrap_err()
        .to_string();

    assert!(error.contains("could not infer PR provider"));
}

#[test]
fn pr_manual_dry_run_requires_manual_and_dry_run_flags() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("pr mode", "noop").unwrap();

    let error = project
        .pr_plan(
            &metadata.run_id,
            PrOptions {
                manual: false,
                dry_run: true,
                provider: Some(PrProvider::Github),
                target: None,
                title: None,
            },
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("--manual --dry-run"));
}

#[test]
fn list_runs_sorts_newest_first() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let first = project.run("first run", "noop").unwrap();
    let second = project.run("second run", "noop").unwrap();

    let runs = project.list_runs().unwrap();

    assert_eq!(runs[0].run_id, second.run_id);
    assert_eq!(runs[1].run_id, first.run_id);
}

#[test]
fn created_at_sort_prefers_parseable_values_with_string_fallback() {
    assert!(compare_created_at_for_test("2026-04-30T10:01:00Z", "2026-04-30T10:00:00Z").is_lt());
    assert!(
        compare_created_at_for_test("2026-04-30T18:00:00+08:00", "2026-04-30T09:00:00Z").is_lt()
    );
    assert!(compare_created_at_for_test("200", "100").is_lt());
    assert!(compare_created_at_for_test("z-legacy", "a-legacy").is_lt());
}

#[test]
fn list_runs_sorts_legacy_metadata_by_created_at_compatibly() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let older = project.run("older legacy run", "noop").unwrap();
    let newer = project.run("newer legacy run", "noop").unwrap();
    let mut older_metadata = read_metadata(&temp, &older.run_id);
    older_metadata.created_at = "2026-04-30T10:00:00Z".to_string();
    project.write_metadata(&older_metadata).unwrap();
    let mut newer_metadata = read_metadata(&temp, &newer.run_id);
    newer_metadata.created_at = "2026-04-30T10:01:00Z".to_string();
    project.write_metadata(&newer_metadata).unwrap();

    let runs = project.list_runs().unwrap();

    assert_eq!(runs[0].run_id, newer.run_id);
    assert_eq!(runs[1].run_id, older.run_id);
}

#[test]
fn report_includes_artifact_paths_and_next_actions() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("review this run", "noop").unwrap();

    let report = project.report(&metadata.run_id).unwrap();

    assert_eq!(report.metadata.run_id, metadata.run_id);
    assert_eq!(
        report.path,
        run_dir(&temp, &metadata.run_id).join(REPORT_FILE)
    );
    assert!(report.artifacts.iter().any(|artifact| {
        artifact.label == "Metadata"
            && artifact.path == run_dir(&temp, &metadata.run_id).join(METADATA_FILE)
            && artifact.exists
    }));
    assert!(report.artifacts.iter().any(|artifact| {
        artifact.label == "Log"
            && artifact.path == run_dir(&temp, &metadata.run_id).join(LOG_FILE)
            && artifact.exists
    }));
    assert!(report.artifacts.iter().any(|artifact| {
        artifact.label == "Diff"
            && artifact.path == run_dir(&temp, &metadata.run_id).join(DIFF_FILE)
            && artifact.exists
    }));
    assert!(report
        .next_actions
        .contains(&format!("keel diff {}", metadata.run_id)));
    assert!(report
        .next_actions
        .contains(&format!("keel rerun {}", metadata.run_id)));
    assert!(report
        .next_actions
        .contains(&format!("keel discard {}", metadata.run_id)));
}

#[test]
fn report_marks_missing_artifacts_without_failing() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("missing artifact report", "noop").unwrap();
    fs::remove_file(run_dir(&temp, &metadata.run_id).join(LOG_FILE)).unwrap();

    let report = project.report(&metadata.run_id).unwrap();

    assert!(report
        .artifacts
        .iter()
        .any(|artifact| artifact.label == "Log" && !artifact.exists));
}

#[test]
fn log_reads_saved_log_and_empty_log() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("read log", "noop").unwrap();

    let log = project.log(&metadata.run_id).unwrap();

    assert_eq!(log.path, run_dir(&temp, &metadata.run_id).join(LOG_FILE));
    assert!(!log.is_empty);
    assert!(log.content.contains("created run"));

    fs::write(run_dir(&temp, &metadata.run_id).join(LOG_FILE), "").unwrap();
    let empty_log = project.log(&metadata.run_id).unwrap();
    assert!(empty_log.is_empty);
    assert!(empty_log.content.is_empty());
}

#[test]
fn log_errors_when_run_or_file_is_missing() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let missing_run = project.log("run-does-not-exist").unwrap_err().to_string();
    assert!(missing_run.contains("run `run-does-not-exist` does not exist"));

    let metadata = project.run("missing log", "noop").unwrap();
    fs::remove_file(run_dir(&temp, &metadata.run_id).join(LOG_FILE)).unwrap();

    let missing_log = project.log(&metadata.run_id).unwrap_err().to_string();
    assert!(missing_log.contains("log for run"));
}

#[test]
fn diff_reads_saved_patch() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("read diff", "noop").unwrap();

    let diff = project.diff(&metadata.run_id).unwrap();

    assert_eq!(diff.path, run_dir(&temp, &metadata.run_id).join(DIFF_FILE));
    assert!(!diff.is_empty);
    assert!(diff.content.contains(NOOP_OUTPUT_FILE));
}

#[test]
fn diff_reports_empty_patch() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("empty diff", "noop").unwrap();
    fs::write(run_dir(&temp, &metadata.run_id).join(DIFF_FILE), "").unwrap();

    let diff = project.diff(&metadata.run_id).unwrap();

    assert!(diff.is_empty);
    assert!(diff.content.is_empty());
}

#[test]
fn diff_errors_when_run_or_patch_is_missing() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let missing_run = project.diff("run-does-not-exist").unwrap_err().to_string();
    assert!(missing_run.contains("run `run-does-not-exist` does not exist"));

    let metadata = project.run("missing diff", "noop").unwrap();
    fs::remove_file(run_dir(&temp, &metadata.run_id).join(DIFF_FILE)).unwrap();

    let missing_diff = project.diff(&metadata.run_id).unwrap_err().to_string();
    assert!(missing_diff.contains("diff for run"));
}

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
    let source_report = read_run_file(&temp, &source.run_id, REPORT_FILE);
    assert!(source_report.contains("## Rerun"));
    assert!(source_report.contains(&format!("Created rerun: `{}`", child.run_id)));

    let child_report = read_run_file(&temp, &child.run_id, REPORT_FILE);
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
    let source_report = read_run_file(&temp, &source.run_id, REPORT_FILE);
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
    let source_report = read_run_file(&temp, &source.run_id, REPORT_FILE);
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
    let report = read_run_file(&temp, &metadata.run_id, REPORT_FILE);
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

#[test]
fn adapter_failure_still_persists_run_history() {
    struct FailingAgent;

    impl AgentAdapter for FailingAgent {
        fn name(&self) -> &'static str {
            "failing"
        }

        fn command(&self, _context: &AgentRunContext<'_>) -> Vec<String> {
            vec!["failing".to_string()]
        }

        fn run(&self, _context: &AgentRunContext<'_>) -> Result<AgentExecution> {
            bail!("adapter exploded")
        }
    }

    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let error = project
        .run_with_adapter("failure path", &FailingAgent)
        .unwrap_err()
        .to_string();

    assert!(error.contains("adapter exploded"));
    let runs = project.list_runs().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, RunStatus::NotReady);
    assert_eq!(runs[0].failure_reason, Some(FailureReason::AdapterError));
    assert_eq!(runs[0].agent_command, vec!["failing".to_string()]);

    let run_dir = run_dir(&temp, &runs[0].run_id);
    assert_required_artifacts(&run_dir);

    let report = fs::read_to_string(run_dir.join(REPORT_FILE)).unwrap();
    assert!(report.contains("## Failure"));
    assert!(report.contains("adapter exploded"));
}

#[test]
fn unsupported_agent_is_rejected() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let error = project.run("task", "unknown").unwrap_err().to_string();

    assert!(error.contains("unsupported agent"));
}

#[test]
fn codex_adapter_builds_safe_exec_command() {
    let temp = git_repo();
    let worktree = temp.path();
    let adapter = CodexAgent::with_program("codex");
    let command = adapter.command(&AgentRunContext {
        run_id: "run-test",
        task: "do the task",
        worktree,
        agent_timeout_secs: DEFAULT_AGENT_TIMEOUT_SECS,
    });

    assert_eq!(command[0], "codex");
    let approval_index = command
        .iter()
        .position(|arg| arg == "--ask-for-approval")
        .unwrap();
    let exec_index = command.iter().position(|arg| arg == "exec").unwrap();
    assert!(approval_index < exec_index);
    assert!(command
        .windows(2)
        .any(|pair| pair[0] == "--cd" && pair[1] == worktree.to_string_lossy().as_ref()));
    assert!(command
        .windows(2)
        .any(|pair| pair[0] == "--sandbox" && pair[1] == "workspace-write"));
    assert!(command
        .windows(2)
        .any(|pair| pair[0] == "--ask-for-approval" && pair[1] == "on-request"));
    assert!(!command.iter().any(|arg| arg == "--full-auto"));
    assert!(!command
        .iter()
        .any(|arg| arg == "--dangerously-bypass-approvals-and-sandbox"));
}

#[test]
fn codex_adapter_captures_stdout_stderr_and_diff() {
    let temp = git_repo();
    let fake_codex = fake_codex(temp.path(), FakeCodexMode::Success);
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let metadata = project
        .run_with_adapter(
            "make a codex change",
            &CodexAgent::with_program(fake_codex.to_string_lossy()),
        )
        .unwrap();

    assert_eq!(metadata.agent, "codex");
    assert_eq!(metadata.status, RunStatus::Ready);
    let run_dir = run_dir(&temp, &metadata.run_id);
    assert_required_artifacts(&run_dir);

    let log = fs::read_to_string(run_dir.join(LOG_FILE)).unwrap();
    assert!(log.contains("fake codex stdout"));
    assert!(log.contains("fake codex stderr"));
    assert!(log.contains("--ask-for-approval on-request"));
    assert!(!log.contains("--full-auto"));
    assert!(!log.contains("--dangerously-bypass-approvals-and-sandbox"));

    let diff = fs::read_to_string(run_dir.join(DIFF_FILE)).unwrap();
    assert!(diff.contains("codex-output.txt"));
}

#[test]
fn codex_nonzero_exit_still_generates_report() {
    let temp = git_repo();
    let fake_codex = fake_codex(temp.path(), FakeCodexMode::Failure);
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let metadata = project
        .run_with_adapter(
            "fail the codex run",
            &CodexAgent::with_program(fake_codex.to_string_lossy()),
        )
        .unwrap();

    assert_eq!(metadata.status, RunStatus::NotReady);
    assert_eq!(metadata.exit_code, Some(7));
    assert_eq!(metadata.failure_reason, Some(FailureReason::NonzeroExit));
    assert!(metadata
        .readiness_reason
        .contains("agent exited with nonzero status 7"));
    assert!(!metadata.agent_command.is_empty());
    let run_dir = run_dir(&temp, &metadata.run_id);
    assert_required_artifacts(&run_dir);
    let log = fs::read_to_string(run_dir.join(LOG_FILE)).unwrap();
    assert!(log.contains("fake codex failure"));
    let report = fs::read_to_string(run_dir.join(REPORT_FILE)).unwrap();
    assert!(report.contains("Agent Exit Code: `7`"));
    assert!(report.contains("Failure Reason: `nonzero_exit`"));
    assert!(report.contains("fake codex failure"));
}

#[test]
fn missing_codex_cli_still_generates_failure_report() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let missing = missing_codex_path(temp.path());

    let error = project
        .run_with_adapter(
            "missing codex",
            &CodexAgent::with_program(missing.to_string_lossy()),
        )
        .unwrap_err()
        .to_string();

    assert!(error.contains("codex CLI not found"));
    let runs = project.list_runs().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, RunStatus::NotReady);
    assert_eq!(runs[0].failure_reason, Some(FailureReason::MissingCli));
    assert!(!runs[0].agent_command.is_empty());
    let run_dir = run_dir(&temp, &runs[0].run_id);
    assert_required_artifacts(&run_dir);
    let report = fs::read_to_string(run_dir.join(REPORT_FILE)).unwrap();
    assert!(report.contains("## Failure"));
    assert!(report.contains("codex CLI not found"));
    assert!(report.contains("Failure Reason: `missing_cli`"));
}

#[test]
fn claude_adapter_builds_safe_print_command() {
    let temp = git_repo();
    let worktree = temp.path();
    let adapter = ClaudeAgent::with_program("claude");
    let command = adapter.command(&AgentRunContext {
        run_id: "run-test",
        task: "do the task",
        worktree,
        agent_timeout_secs: DEFAULT_AGENT_TIMEOUT_SECS,
    });

    assert_eq!(command[0], "claude");
    assert!(command.iter().any(|arg| arg == "--print"));
    assert!(command
        .windows(2)
        .any(|pair| pair[0] == "--permission-mode" && pair[1] == "acceptEdits"));
    assert!(command
        .iter()
        .any(|arg| arg == "--allowedTools=Read,Edit,MultiEdit,Write,LS,Grep,Glob"));
    assert!(!command.iter().any(|arg| arg == "--allowedTools"));
    assert_eq!(command.last().map(String::as_str), Some("do the task"));
    assert!(!command
        .iter()
        .any(|arg| arg == "--dangerously-skip-permissions"));
    assert!(!command
        .iter()
        .any(|arg| arg == "--allow-dangerously-skip-permissions"));
    assert!(!command.iter().any(|arg| arg == "bypassPermissions"));
}

#[test]
fn claude_adapter_captures_stdout_stderr_and_diff() {
    let temp = git_repo();
    let fake_claude = fake_claude(temp.path(), FakeClaudeMode::Success);
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let metadata = project
        .run_with_adapter(
            "make a claude change",
            &ClaudeAgent::with_program(fake_claude.to_string_lossy()),
        )
        .unwrap();

    assert_eq!(metadata.agent, "claude");
    assert_eq!(metadata.status, RunStatus::Ready);
    let run_dir = run_dir(&temp, &metadata.run_id);
    assert_required_artifacts(&run_dir);

    let log = fs::read_to_string(run_dir.join(LOG_FILE)).unwrap();
    assert!(log.contains("fake claude stdout"));
    assert!(log.contains("fake claude stderr"));
    assert!(log.contains("--permission-mode acceptEdits"));
    assert!(!log.contains("--dangerously-skip-permissions"));
    assert!(!log.contains("bypassPermissions"));

    let diff = fs::read_to_string(run_dir.join(DIFF_FILE)).unwrap();
    assert!(diff.contains("claude-output.txt"));
}

#[test]
fn claude_nonzero_exit_still_generates_report() {
    let temp = git_repo();
    let fake_claude = fake_claude(temp.path(), FakeClaudeMode::Failure);
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let metadata = project
        .run_with_adapter(
            "fail the claude run",
            &ClaudeAgent::with_program(fake_claude.to_string_lossy()),
        )
        .unwrap();

    assert_eq!(metadata.status, RunStatus::NotReady);
    assert_eq!(metadata.exit_code, Some(9));
    assert_eq!(metadata.failure_reason, Some(FailureReason::NonzeroExit));
    let run_dir = run_dir(&temp, &metadata.run_id);
    assert_required_artifacts(&run_dir);
    let log = fs::read_to_string(run_dir.join(LOG_FILE)).unwrap();
    assert!(log.contains("fake claude failure"));
    let report = fs::read_to_string(run_dir.join(REPORT_FILE)).unwrap();
    assert!(report.contains("Agent Exit Code: `9`"));
    assert!(report.contains("Failure Reason: `nonzero_exit`"));
    assert!(report.contains("fake claude failure"));
}

#[test]
fn missing_claude_cli_still_generates_failure_report() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let missing = missing_claude_path(temp.path());

    let error = project
        .run_with_adapter(
            "missing claude",
            &ClaudeAgent::with_program(missing.to_string_lossy()),
        )
        .unwrap_err()
        .to_string();

    assert!(error.contains("claude CLI not found"));
    let runs = project.list_runs().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, RunStatus::NotReady);
    assert_eq!(runs[0].failure_reason, Some(FailureReason::MissingCli));
    assert!(!runs[0].agent_command.is_empty());
    let run_dir = run_dir(&temp, &runs[0].run_id);
    assert_required_artifacts(&run_dir);
    let report = fs::read_to_string(run_dir.join(REPORT_FILE)).unwrap();
    assert!(report.contains("## Failure"));
    assert!(report.contains("claude CLI not found"));
    assert!(report.contains("Failure Reason: `missing_cli`"));
}

#[test]
fn opencode_adapter_builds_safe_run_command() {
    let temp = git_repo();
    let worktree = temp.path();
    let adapter = OpenCodeAgent::with_program("opencode");
    let command = adapter.command(&AgentRunContext {
        run_id: "run-test",
        task: "do the task",
        worktree,
        agent_timeout_secs: DEFAULT_AGENT_TIMEOUT_SECS,
    });

    assert_eq!(command[0], "opencode");
    assert!(command.windows(2).any(|pair| pair[0] == "run"));
    assert!(command
        .windows(2)
        .any(|pair| pair[0] == "--dir" && pair[1] == worktree.to_string_lossy().as_ref()));
    assert!(command.iter().any(|arg| arg == "--pure"));
    assert!(!command
        .iter()
        .any(|arg| arg == "--dangerously-skip-permissions"));
    assert_eq!(command.last().map(String::as_str), Some("do the task"));
}

#[test]
fn opencode_adapter_captures_stdout_stderr_and_diff() {
    let temp = git_repo();
    let fake_opencode = fake_opencode(temp.path(), FakeOpenCodeMode::Success);
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let metadata = project
        .run_with_adapter(
            "make an opencode change",
            &OpenCodeAgent::with_program(fake_opencode.to_string_lossy()),
        )
        .unwrap();

    assert_eq!(metadata.agent, "opencode");
    assert_eq!(metadata.status, RunStatus::Ready);
    let run_dir = run_dir(&temp, &metadata.run_id);
    assert_required_artifacts(&run_dir);

    let log = fs::read_to_string(run_dir.join(LOG_FILE)).unwrap();
    assert!(log.contains("fake opencode stdout"));
    assert!(log.contains("fake opencode stderr"));
    assert!(log.contains("run --dir"));
    assert!(!log.contains("--dangerously-skip-permissions"));

    let diff = fs::read_to_string(run_dir.join(DIFF_FILE)).unwrap();
    assert!(diff.contains("opencode-output.txt"));
}

#[test]
fn opencode_nonzero_exit_still_generates_report() {
    let temp = git_repo();
    let fake_opencode = fake_opencode(temp.path(), FakeOpenCodeMode::Failure);
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let metadata = project
        .run_with_adapter(
            "fail the opencode run",
            &OpenCodeAgent::with_program(fake_opencode.to_string_lossy()),
        )
        .unwrap();

    assert_eq!(metadata.status, RunStatus::NotReady);
    assert_eq!(metadata.exit_code, Some(11));
    assert_eq!(metadata.failure_reason, Some(FailureReason::NonzeroExit));
    let run_dir = run_dir(&temp, &metadata.run_id);
    assert_required_artifacts(&run_dir);
    let log = fs::read_to_string(run_dir.join(LOG_FILE)).unwrap();
    assert!(log.contains("fake opencode failure"));
    let report = fs::read_to_string(run_dir.join(REPORT_FILE)).unwrap();
    assert!(report.contains("Agent Exit Code: `11`"));
    assert!(report.contains("Failure Reason: `nonzero_exit`"));
    assert!(report.contains("fake opencode failure"));
}

#[test]
fn opencode_empty_diff_is_not_ready() {
    let temp = git_repo();
    let fake_opencode = fake_opencode(temp.path(), FakeOpenCodeMode::EmptyDiff);
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let error = project
        .run_with_adapter(
            "opencode exits without changing files",
            &OpenCodeAgent::with_program(fake_opencode.to_string_lossy()),
        )
        .unwrap_err()
        .to_string();

    assert!(error.contains("empty diff"));
    let runs = project.list_runs().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, RunStatus::NotReady);
    assert_eq!(runs[0].failure_reason, Some(FailureReason::EmptyDiff));
    assert!(runs[0]
        .readiness_reason
        .contains("required candidate diff was empty"));
    let run_dir = run_dir(&temp, &runs[0].run_id);
    assert_required_artifacts(&run_dir);
    let report = fs::read_to_string(run_dir.join(REPORT_FILE)).unwrap();
    assert!(report.contains("Failure Reason: `empty_diff`"));
}

#[test]
fn missing_opencode_cli_still_generates_failure_report() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let missing = missing_opencode_path(temp.path());

    let error = project
        .run_with_adapter(
            "missing opencode",
            &OpenCodeAgent::with_program(missing.to_string_lossy()),
        )
        .unwrap_err()
        .to_string();

    assert!(error.contains("opencode CLI not found"));
    let runs = project.list_runs().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, RunStatus::NotReady);
    assert_eq!(runs[0].failure_reason, Some(FailureReason::MissingCli));
    assert!(!runs[0].agent_command.is_empty());
    let run_dir = run_dir(&temp, &runs[0].run_id);
    assert_required_artifacts(&run_dir);
    let report = fs::read_to_string(run_dir.join(REPORT_FILE)).unwrap();
    assert!(report.contains("## Failure"));
    assert!(report.contains("opencode CLI not found"));
    assert!(report.contains("Failure Reason: `missing_cli`"));
}

#[test]
fn codex_timeout_still_generates_not_ready_report() {
    let temp = git_repo();
    let fake_codex = fake_codex(temp.path(), FakeCodexMode::Timeout);
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    write_config(
        &temp,
        r#"version = 1
runs_dir = "runs"
worktrees_dir = "worktrees"
agent_timeout_secs = 1

[[checks]]
name = "git status"
command = ["git", "status", "--short"]
"#,
    );

    let metadata = project
        .run_with_adapter(
            "timeout the codex run",
            &CodexAgent::with_program(fake_codex.to_string_lossy()),
        )
        .unwrap();

    assert_eq!(metadata.status, RunStatus::NotReady);
    assert_eq!(metadata.failure_reason, Some(FailureReason::Timeout));
    assert!(metadata.readiness_reason.contains("timed out"));
    assert!(metadata.duration_ms.is_some());
    let run_dir = run_dir(&temp, &metadata.run_id);
    assert_required_artifacts(&run_dir);
    let report = fs::read_to_string(run_dir.join(REPORT_FILE)).unwrap();
    assert!(report.contains("Failure Reason: `timeout`"));
    assert!(report.contains("process timed out after 1 seconds"));
}

#[test]
fn codex_timeout_kills_spawned_child_process() {
    let temp = git_repo();
    let fake_codex = fake_codex(temp.path(), FakeCodexMode::SpawnChild);
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    write_config(
        &temp,
        r#"version = 1
runs_dir = "runs"
worktrees_dir = "worktrees"
agent_timeout_secs = 1

[[checks]]
name = "git status"
command = ["git", "status", "--short"]
"#,
    );

    let metadata = project
        .run_with_adapter(
            "spawn a child and time out",
            &CodexAgent::with_program(fake_codex.to_string_lossy()),
        )
        .unwrap();

    assert_eq!(metadata.status, RunStatus::NotReady);
    assert_eq!(metadata.failure_reason, Some(FailureReason::Timeout));
    let survivor_path = worktree_dir(&temp, &metadata.run_id).join("process-tree-survivor.txt");
    thread::sleep(Duration::from_secs(4));
    assert!(
        !survivor_path.exists(),
        "timed-out agent child process survived and wrote {}",
        survivor_path.display()
    );
}

#[test]
fn real_codex_smoke_is_opt_in() {
    if !real_codex_smoke_enabled() {
        return;
    }

    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project
            .run(
                "Create a file named codex-real-smoke.txt containing exactly: Keel real Codex smoke test",
                "codex",
            )
            .unwrap();

    assert_eq!(metadata.agent, "codex");
    assert_eq!(metadata.status, RunStatus::Ready);
    let diff = read_run_file(&temp, &metadata.run_id, DIFF_FILE);
    assert!(diff.contains("codex-real-smoke.txt"));
}

#[test]
fn real_codex_rerun_smoke_is_opt_in() {
    if !real_codex_smoke_enabled() {
        return;
    }

    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let source = project
            .run(
                "Create a file named codex-real-rerun-smoke.txt containing exactly: Keel real Codex rerun smoke test",
                "codex",
            )
            .unwrap();
    let child = project.rerun(&source.run_id).unwrap();

    assert_eq!(source.agent, "codex");
    assert_eq!(child.agent, "codex");
    assert_eq!(child.parent_run_id.as_deref(), Some(source.run_id.as_str()));
    assert_ne!(source.run_id, child.run_id);
    assert_ne!(source.worktree_path, child.worktree_path);

    for run_id in [&source.run_id, &child.run_id] {
        let metadata = read_metadata(&temp, run_id);
        assert_eq!(metadata.status, RunStatus::Ready);
        assert_eq!(metadata.failure_reason, None);
        assert!(read_run_file(&temp, run_id, DIFF_FILE).contains("codex-real-rerun-smoke.txt"));
        assert!(project
            .report(run_id)
            .unwrap()
            .summary
            .contains("agent=codex"));
    }

    assert!(read_run_file(&temp, &source.run_id, REPORT_FILE)
        .contains(&format!("Created rerun: `{}`", child.run_id)));

    let discarded_source = project.discard(&source.run_id).unwrap();
    let discarded_child = project.discard(&child.run_id).unwrap();
    assert_eq!(discarded_source.status, RunStatus::Discarded);
    assert_eq!(discarded_child.status, RunStatus::Discarded);
    assert!(!worktree_dir(&temp, &source.run_id).exists());
    assert!(!worktree_dir(&temp, &child.run_id).exists());
    assert!(run_dir(&temp, &source.run_id).join(REPORT_FILE).exists());
    assert!(run_dir(&temp, &child.run_id).join(REPORT_FILE).exists());
}

#[cfg(windows)]
#[test]
fn windows_program_resolution_prefers_pathext_shim() {
    let temp = TempDir::new().unwrap();
    fs::write(temp.path().join("codex"), "not a Windows executable").unwrap();
    fs::write(temp.path().join("codex.cmd"), "@echo off\r\nexit /B 0\r\n").unwrap();

    let resolved = resolve_windows_program_from_path(
        "codex",
        &[temp.path().to_path_buf()],
        &[".EXE".to_string(), ".CMD".to_string()],
    )
    .unwrap();

    assert!(resolved
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.eq_ignore_ascii_case("codex.cmd")));
}

fn git_repo() -> TempDir {
    git_repo_with_files(&[])
}

fn bare_git_repo() -> TempDir {
    let temp = TempDir::new().unwrap();
    git(temp.path(), &["init", "--bare"]);
    temp
}

fn add_origin(repo: &TempDir, remote: &TempDir) {
    git(
        repo.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
}

fn push_options(dry_run: bool) -> PushOptions {
    PushOptions {
        remote: "origin".to_string(),
        dry_run,
    }
}

fn pr_options(provider: Option<PrProvider>) -> PrOptions {
    PrOptions {
        manual: true,
        dry_run: true,
        provider,
        target: None,
        title: None,
    }
}

fn config_path(temp: &TempDir) -> PathBuf {
    temp.path().join(KEEL_DIR).join(CONFIG_FILE)
}

fn run_dir(temp: &TempDir, run_id: &str) -> PathBuf {
    temp.path().join(KEEL_DIR).join(RUNS_DIR).join(run_id)
}

fn worktree_dir(temp: &TempDir, run_id: &str) -> PathBuf {
    temp.path().join(KEEL_DIR).join(WORKTREES_DIR).join(run_id)
}

fn write_config(temp: &TempDir, content: &str) {
    fs::write(config_path(temp), content).unwrap();
}

fn read_checks(temp: &TempDir, run_id: &str) -> Vec<CheckResult> {
    read_json(&run_dir(temp, run_id).join(CHECKS_FILE)).unwrap()
}

fn has_risk_warning(metadata: &RunMetadata, kind: RiskWarningKind) -> bool {
    metadata
        .risk_warnings
        .iter()
        .any(|warning| warning.kind == kind)
}

fn read_metadata(temp: &TempDir, run_id: &str) -> RunMetadata {
    read_json(&run_dir(temp, run_id).join(METADATA_FILE)).unwrap()
}

fn real_codex_smoke_enabled() -> bool {
    std::env::var("KEEL_REAL_CODEX_SMOKE").ok().as_deref() == Some("1")
}

fn read_run_file(temp: &TempDir, run_id: &str, file: &str) -> String {
    fs::read_to_string(run_dir(temp, run_id).join(file)).unwrap()
}

fn git_repo_with_files(files: &[(&str, &str)]) -> TempDir {
    let temp = TempDir::new().unwrap();
    git(temp.path(), &["init"]);
    git(temp.path(), &["config", "user.email", "keel@example.local"]);
    git(temp.path(), &["config", "user.name", "Keel Test"]);
    fs::write(temp.path().join("README.md"), "# temp\n").unwrap();
    for (path, content) in files {
        fs::write(temp.path().join(path), content).unwrap();
    }
    git(temp.path(), &["add", "README.md"]);
    for (path, _) in files {
        git(temp.path(), &["add", path]);
    }
    git(temp.path(), &["commit", "-m", "init"]);
    temp
}

fn branch_exists(temp: &TempDir, branch: &str) -> bool {
    Command::new("git")
        .args(["show-ref", "--verify", "--quiet"])
        .arg(format!("refs/heads/{branch}"))
        .current_dir(temp.path())
        .status()
        .unwrap()
        .success()
}

fn git_stdout(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

fn assert_required_artifacts(run_dir: &Path) {
    assert!(run_dir.join(METADATA_FILE).exists());
    assert!(run_dir.join(LOG_FILE).exists());
    assert!(run_dir.join(DIFF_FILE).exists());
    assert!(run_dir.join(CHECKS_FILE).exists());
    assert!(run_dir.join(REPORT_FILE).exists());
}

enum FakeCodexMode {
    Success,
    Failure,
    Timeout,
    SpawnChild,
}

enum FakeClaudeMode {
    Success,
    Failure,
}

enum FakeOpenCodeMode {
    Success,
    Failure,
    EmptyDiff,
}

struct FileChangeAgent {
    writes: Vec<(&'static str, &'static str)>,
    deletes: Vec<&'static str>,
}

impl FileChangeAgent {
    fn new(writes: &[(&'static str, &'static str)]) -> Self {
        Self {
            writes: writes.to_vec(),
            deletes: Vec::new(),
        }
    }

    fn delete(mut self, path: &'static str) -> Self {
        self.deletes.push(path);
        self
    }
}

impl AgentAdapter for FileChangeAgent {
    fn name(&self) -> &'static str {
        "file-change"
    }

    fn command(&self, _context: &AgentRunContext<'_>) -> Vec<String> {
        vec!["file-change".to_string()]
    }

    fn run(&self, context: &AgentRunContext<'_>) -> Result<AgentExecution> {
        for (path, content) in &self.writes {
            let path = context.worktree.join(path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, content)?;
        }
        for path in &self.deletes {
            fs::remove_file(context.worktree.join(path))?;
        }
        Ok(AgentExecution {
            command: self.command(context),
            exit_code: Some(0),
            stdout: "file changes written\n".to_string(),
            stderr: String::new(),
            timed_out: false,
        })
    }

    fn requires_non_empty_diff(&self) -> bool {
        true
    }
}

fn fake_codex(repo: &Path, mode: FakeCodexMode) -> PathBuf {
    let script = repo.join(if cfg!(windows) {
        "fake-codex.cmd"
    } else {
        "fake-codex"
    });
    let content = match mode {
            FakeCodexMode::Success if cfg!(windows) => {
                "@echo off\r\necho fake codex stdout\r\necho fake codex stderr 1>&2\r\necho codex output>codex-output.txt\r\nexit /B 0\r\n"
            }
            FakeCodexMode::Failure if cfg!(windows) => {
                "@echo off\r\necho fake codex failure 1>&2\r\nexit /B 7\r\n"
            }
            FakeCodexMode::Timeout if cfg!(windows) => {
                "@echo off\r\necho fake codex starting timeout\r\nping -n 3 127.0.0.1 >nul\r\necho late output>codex-timeout-output.txt\r\nexit /B 0\r\n"
            }
            FakeCodexMode::SpawnChild if cfg!(windows) => {
                "@echo off\r\necho fake codex spawning child\r\nstart \"\" /B cmd /C \"ping -n 4 127.0.0.1 >nul & echo child survived>process-tree-survivor.txt\"\r\nping -n 6 127.0.0.1 >nul\r\nexit /B 0\r\n"
            }
            FakeCodexMode::Success => {
                "#!/bin/sh\necho fake codex stdout\necho fake codex stderr >&2\necho codex output > codex-output.txt\nexit 0\n"
            }
            FakeCodexMode::Failure => "#!/bin/sh\necho fake codex failure >&2\nexit 7\n",
            FakeCodexMode::Timeout => {
                "#!/bin/sh\necho fake codex starting timeout\nsleep 2\necho late output > codex-timeout-output.txt\nexit 0\n"
            }
            FakeCodexMode::SpawnChild => {
                "#!/bin/sh\necho fake codex spawning child\n(sh -c 'sleep 3; echo child survived > process-tree-survivor.txt') &\nsleep 5\nexit 0\n"
            }
        };
    fs::write(&script, content).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();
    }
    script
}

fn fake_claude(repo: &Path, mode: FakeClaudeMode) -> PathBuf {
    let script = repo.join(if cfg!(windows) {
        "fake-claude.cmd"
    } else {
        "fake-claude"
    });
    let content = match mode {
        FakeClaudeMode::Success if cfg!(windows) => {
            "@echo off\r\necho fake claude stdout\r\necho fake claude stderr 1>&2\r\necho claude output>claude-output.txt\r\nexit /B 0\r\n"
        }
        FakeClaudeMode::Failure if cfg!(windows) => {
            "@echo off\r\necho fake claude failure 1>&2\r\nexit /B 9\r\n"
        }
        FakeClaudeMode::Success => {
            "#!/bin/sh\necho fake claude stdout\necho fake claude stderr >&2\necho claude output > claude-output.txt\nexit 0\n"
        }
        FakeClaudeMode::Failure => "#!/bin/sh\necho fake claude failure >&2\nexit 9\n",
    };
    fs::write(&script, content).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();
    }
    script
}

fn fake_opencode(repo: &Path, mode: FakeOpenCodeMode) -> PathBuf {
    let script = repo.join(if cfg!(windows) {
        "fake-opencode.cmd"
    } else {
        "fake-opencode"
    });
    let content = match mode {
        FakeOpenCodeMode::Success if cfg!(windows) => {
            "@echo off\r\necho fake opencode stdout\r\necho fake opencode stderr 1>&2\r\necho opencode output>opencode-output.txt\r\nexit /B 0\r\n"
        }
        FakeOpenCodeMode::Failure if cfg!(windows) => {
            "@echo off\r\necho fake opencode failure 1>&2\r\necho failed opencode output>opencode-failure-output.txt\r\nexit /B 11\r\n"
        }
        FakeOpenCodeMode::EmptyDiff if cfg!(windows) => {
            "@echo off\r\necho fake opencode no changes\r\nexit /B 0\r\n"
        }
        FakeOpenCodeMode::Success => {
            "#!/bin/sh\necho fake opencode stdout\necho fake opencode stderr >&2\necho opencode output > opencode-output.txt\nexit 0\n"
        }
        FakeOpenCodeMode::Failure => {
            "#!/bin/sh\necho fake opencode failure >&2\necho failed opencode output > opencode-failure-output.txt\nexit 11\n"
        }
        FakeOpenCodeMode::EmptyDiff => "#!/bin/sh\necho fake opencode no changes\nexit 0\n",
    };
    fs::write(&script, content).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();
    }
    script
}

fn missing_codex_path(repo: &Path) -> PathBuf {
    repo.join(if cfg!(windows) { "codex.exe" } else { "codex" })
}

fn missing_claude_path(repo: &Path) -> PathBuf {
    repo.join(if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    })
}

fn missing_opencode_path(repo: &Path) -> PathBuf {
    repo.join(if cfg!(windows) {
        "opencode.exe"
    } else {
        "opencode"
    })
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
