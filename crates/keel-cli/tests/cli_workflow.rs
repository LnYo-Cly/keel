use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use tempfile::TempDir;

const RUN_CREATED_PREFIX: &str = "Run created: ";
const NO_MATCHES_MESSAGE: &str = "No runs matched the provided filters.";
const NOOP_OUTPUT_FILE: &str = "keel-noop-output.txt";

#[test]
fn doctor_reports_errors_outside_git_repo() {
    let temp = tempfile::tempdir().unwrap();

    run_keel_with_path(temp.path(), ["doctor"], &path_with_git_only())
        .assert()
        .failure()
        .stdout(predicate::str::contains("Keel doctor"))
        .stdout(predicate::str::contains(
            "current directory is not inside a git repository",
        ));

    let json = run_keel_output_with_path(
        temp.path(),
        ["doctor", "--json"],
        &path_with_git_only(),
        false,
    );
    let report = parse_json_object(&json);
    assert_eq!(report["ok"], false);
    assert!(report["summary"]["errors"].as_u64().unwrap() > 0);
}

#[test]
fn doctor_reports_missing_keel_layout_before_init() {
    let repo = create_temp_git_repo();

    run_keel_with_path(repo.path(), ["doctor"], &path_with_git_only())
        .assert()
        .failure()
        .stdout(predicate::str::contains(".keel directory is missing"))
        .stdout(predicate::str::contains(".keel/config.toml is missing"))
        .stdout(predicate::str::contains(".keel/runs directory is missing"))
        .stdout(predicate::str::contains(
            ".keel/worktrees directory is missing",
        ));

    let report = parse_json_object(&run_keel_output_with_path(
        repo.path(),
        ["doctor", "--json"],
        &path_with_git_only(),
        false,
    ));
    assert!(report["summary"]["errors"].as_u64().unwrap() >= 4);
}

#[test]
fn doctor_after_init_prints_grouped_human_output() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();

    run_keel_with_path(repo.path(), ["doctor"], &path_with_git_only())
        .assert()
        .success()
        .stdout(predicate::str::contains("Repository"))
        .stdout(predicate::str::contains("Keel"))
        .stdout(predicate::str::contains("Agents"))
        .stdout(predicate::str::contains("Summary"))
        .stdout(predicate::str::contains(".keel/config.toml found"))
        .stdout(predicate::str::contains(".keel/runs directory found"))
        .stdout(predicate::str::contains(".keel/worktrees directory found"))
        .stdout(predicate::str::contains("codex not found in PATH"))
        .stdout(predicate::str::contains("claude not found in PATH"))
        .stdout(predicate::str::contains("opencode not found in PATH"));
}

#[test]
fn doctor_json_after_init_is_parseable_and_structured() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();

    let report = parse_json_object(&run_keel_output_with_path(
        repo.path(),
        ["doctor", "--json"],
        &path_with_git_only(),
        true,
    ));

    assert_eq!(report["ok"], true);
    assert!(report["summary"]["warnings"].as_u64().unwrap() >= 3);
    let checks = report["checks"].as_array().unwrap();
    for id in [
        "repository.git_command",
        "repository.git_repo",
        "repository.git_worktree",
        "repository.working_tree_clean",
        "keel.directory",
        "keel.config",
        "keel.runs",
        "keel.worktrees",
        "agents.codex",
        "agents.claude",
        "agents.opencode",
    ] {
        assert!(
            checks.iter().any(|check| check["id"] == id),
            "missing doctor check {id}"
        );
    }
    assert!(!report.to_string().contains("Keel doctor"));
}

#[test]
fn doctor_dirty_working_tree_is_warning_not_failure() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();
    fs::write(repo.path().join("dirty.txt"), "dirty\n").unwrap();

    run_keel_with_path(repo.path(), ["doctor"], &path_with_git_only())
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "working tree has uncommitted changes",
        ));

    let report = parse_json_object(&run_keel_output_with_path(
        repo.path(),
        ["doctor", "--json"],
        &path_with_git_only(),
        true,
    ));
    assert_eq!(
        check_status(&report, "repository.working_tree_clean"),
        "warning"
    );
    assert_eq!(report["ok"], true);
}

#[test]
fn doctor_missing_agents_are_warnings_and_fake_agents_are_ok() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();

    let missing = parse_json_object(&run_keel_output_with_path(
        repo.path(),
        ["doctor", "--json"],
        &path_with_git_only(),
        true,
    ));
    assert_eq!(check_status(&missing, "agents.codex"), "warning");
    assert_eq!(check_status(&missing, "agents.claude"), "warning");
    assert_eq!(check_status(&missing, "agents.opencode"), "warning");

    let bin = tempfile::tempdir().unwrap();
    create_fake_executable(bin.path(), "codex");
    create_fake_executable(bin.path(), "claude");
    create_fake_executable(bin.path(), "opencode");
    let found = parse_json_object(&run_keel_output_with_path(
        repo.path(),
        ["doctor", "--json"],
        &path_with_git_and(bin.path()),
        true,
    ));
    assert_eq!(check_status(&found, "agents.codex"), "ok");
    assert_eq!(check_status(&found, "agents.claude"), "ok");
    assert_eq!(check_status(&found, "agents.opencode"), "ok");
    assert!(check_details(&found, "agents.codex").contains("codex"));
}

#[test]
fn config_validate_after_init_reports_summary_and_is_json_parseable() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();

    run_keel(repo.path(), ["config", "validate"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Keel config validation"))
        .stdout(predicate::str::contains("Config"))
        .stdout(predicate::str::contains("Summary"));

    let report = parse_json_object(&run_keel_output(
        repo.path(),
        ["config", "validate", "--json"],
    ));
    assert_eq!(report["ok"], true);
    assert!(report["config_path"].as_str().unwrap().contains(".keel"));
    assert_eq!(report["summary"]["errors"].as_u64().unwrap(), 0);
    assert!(report["issues"].as_array().is_some());
}

#[test]
fn config_validate_reports_missing_and_invalid_config() {
    let repo = create_temp_git_repo();

    run_keel(repo.path(), ["config", "validate"])
        .assert()
        .failure()
        .stdout(predicate::str::contains(".keel/config.toml is missing"));

    let missing = parse_json_object(&run_keel_output_with_path(
        repo.path(),
        ["config", "validate", "--json"],
        &path_with_git_only(),
        false,
    ));
    assert!(missing["summary"]["errors"].as_u64().unwrap() > 0);

    run_keel(repo.path(), ["init"]).assert().success();
    fs::write(
        repo.path().join(".keel").join("config.toml"),
        "not = [valid",
    )
    .unwrap();

    run_keel(repo.path(), ["config", "validate"])
        .assert()
        .failure()
        .stdout(predicate::str::contains(
            "failed to parse .keel/config.toml",
        ));

    let invalid = parse_json_object(&run_keel_output_with_path(
        repo.path(),
        ["config", "validate", "--json"],
        &path_with_git_only(),
        false,
    ));
    assert!(invalid["summary"]["errors"].as_u64().unwrap() > 0);
}

#[test]
fn config_validate_rejects_zero_timeout_and_empty_command() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();
    fs::write(
        repo.path().join(".keel").join("config.toml"),
        r#"
[checks]
commands = [""]

[agents.codex]
timeout_seconds = 0
"#,
    )
    .unwrap();

    run_keel(repo.path(), ["config", "validate"])
        .assert()
        .failure()
        .stdout(predicate::str::contains(
            "checks.commands contains an empty command",
        ))
        .stdout(predicate::str::contains(
            "codex timeout_seconds must be greater than 0",
        ));

    let report = parse_json_object(&run_keel_output_with_path(
        repo.path(),
        ["config", "validate", "--json"],
        &path_with_git_only(),
        false,
    ));
    assert!(report["summary"]["errors"].as_u64().unwrap() > 0);
    assert_eq!(
        check_issue_severity(&report, "checks.commands.empty"),
        "error"
    );
    assert_eq!(
        check_issue_severity(&report, "agents.codex.timeout_seconds"),
        "error"
    );
}

#[test]
fn config_validate_rejects_invalid_risk_config() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();
    fs::write(
        repo.path().join(".keel").join("config.toml"),
        r#"
[risk]
paths = ["", "["]
large_diff_file_threshold = 0
"#,
    )
    .unwrap();

    run_keel(repo.path(), ["config", "validate"])
        .assert()
        .failure()
        .stdout(predicate::str::contains(
            "risk.paths contains an empty pattern",
        ))
        .stdout(predicate::str::contains(
            "risk.paths contains an invalid glob pattern",
        ))
        .stdout(predicate::str::contains(
            "risk.large_diff_file_threshold must be greater than 0",
        ));

    let report = parse_json_object(&run_keel_output_with_path(
        repo.path(),
        ["config", "validate", "--json"],
        &path_with_git_only(),
        false,
    ));
    assert_eq!(check_issue_severity(&report, "risk.paths.empty"), "error");
    assert_eq!(check_issue_severity(&report, "risk.paths.glob"), "error");
    assert_eq!(
        check_issue_severity(&report, "risk.large_diff_file_threshold"),
        "error"
    );
}

#[test]
fn doctor_includes_config_validation_summary() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();

    let report = parse_json_object(&run_keel_output_with_path(
        repo.path(),
        ["doctor", "--json"],
        &path_with_git_only(),
        true,
    ));
    assert!(report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["id"] == "keel.config_validation"));
}

#[test]
fn doctor_reports_invalid_config_as_error() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();
    fs::write(
        repo.path().join(".keel").join("config.toml"),
        r#"
[checks]
commands = [""]

[agents.codex]
timeout_seconds = 0
"#,
    )
    .unwrap();

    run_keel(repo.path(), ["doctor"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("config validation failed"));

    let report = parse_json_object(&run_keel_output_with_path(
        repo.path(),
        ["doctor", "--json"],
        &path_with_git_only(),
        false,
    ));
    assert!(report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["id"] == "keel.config_validation" && check["status"] == "error"));
}

#[test]
fn init_and_noop_run_create_run_artifacts() {
    let repo = create_temp_git_repo();

    run_keel(repo.path(), ["init"]).assert().success();
    let run = run_noop(&repo, "cli smoke task");

    assert!(run_dir(&repo, &run.run_id).is_dir());
    for artifact in [
        "metadata.json",
        "log.txt",
        "diff.patch",
        "checks.json",
        "report.md",
    ] {
        assert!(
            run_artifact_path(&repo, &run.run_id, artifact).is_file(),
            "missing artifact {artifact}"
        );
    }

    let metadata = read_run_artifact(&repo, &run.run_id, "metadata.json");
    assert!(metadata.contains("\"task\": \"cli smoke task\""));
    assert!(metadata.contains("\"agent\": \"noop\""));
}

#[test]
fn ledger_self_dogfood_workflow_records_task_evidence_review_and_handoff() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();

    run_keel(repo.path(), ["task", "start", "self dogfood ledger"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Started Keel task"));

    let active_task = repo
        .path()
        .join(".keel")
        .join("ledger")
        .join("active_task.json");
    assert!(active_task.is_file());

    let task = parse_json_object(&run_keel_output(
        repo.path(),
        ["task", "start", "self dogfood json", "--json"],
    ));
    let task_id = task["task_id"].as_str().unwrap().to_string();
    assert_eq!(task["title"], "self dogfood json");
    assert!(repo
        .path()
        .join(".keel")
        .join("ledger")
        .join("tasks")
        .join(&task_id)
        .join("task.json")
        .is_file());

    run_keel(repo.path(), ["checkpoint", "core ledger model added"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Checkpoint recorded"));
    run_keel(repo.path(), ["note", "risk: CLI output changed"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Note recorded"));
    run_keel(
        repo.path(),
        ["evidence", "add", "--cmd", "git status --short"],
    )
    .assert()
    .success()
    .stdout(predicate::str::contains("Evidence recorded"))
    .stdout(predicate::str::contains("Status: passed"));
    let env_echo = env_echo_command("KEEL_LEDGER_TEST");
    let env_review = parse_json_object(&run_keel_output(
        repo.path(),
        [
            "evidence",
            "add",
            "--env",
            "KEEL_LEDGER_TEST=ok",
            "--cmd",
            env_echo.as_str(),
            "--json",
        ],
    ));
    let last_evidence = env_review["evidence"].as_array().unwrap().last().unwrap();
    assert_eq!(last_evidence["env"][0]["key"], "KEEL_LEDGER_TEST");
    assert!(last_evidence["stdout"].as_str().unwrap().contains("ok"));
    fs::write(repo.path().join("README.md"), "# test repo\n\nchanged\n").unwrap();

    let review = parse_json_object(&run_keel_output(repo.path(), ["review", "--json"]));
    assert_eq!(review["task"]["task_id"], task_id);
    assert_eq!(review["summary"]["checkpoints"], 1);
    assert_eq!(review["summary"]["notes"], 1);
    assert_eq!(review["summary"]["evidence_passed"], 2);
    assert_eq!(review["decision"]["ready"], true);
    assert_eq!(review["workspace"]["dirty"], true);
    assert!(review["workspace"]["changed_files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file == "README.md"));
    assert!(review["packet"]["headline"]
        .as_str()
        .unwrap()
        .contains("ready"));
    assert!(review["packet"]["changed_file_groups"]
        .as_array()
        .unwrap()
        .iter()
        .any(|group| group["name"] == "docs"
            && group["files"]
                .as_array()
                .unwrap()
                .iter()
                .any(|file| file == "README.md")));
    assert!(review["packet"]["evidence"]["latest"]["command"]
        .as_str()
        .unwrap()
        .contains("echo"));

    run_keel(repo.path(), ["review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Review packet:"))
        .stdout(predicate::str::contains("Changed file groups:"))
        .stdout(predicate::str::contains("Workspace:"))
        .stdout(predicate::str::contains("Dirty: yes"))
        .stdout(predicate::str::contains("README.md"));

    run_keel(repo.path(), ["verify"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Decision: ready"));

    run_keel(repo.path(), ["handoff"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Keel handoff"))
        .stdout(predicate::str::contains(
            "Last checkpoint: core ledger model added",
        ));

    let task_status =
        parse_json_object(&run_keel_output(repo.path(), ["task", "status", "--json"]));
    assert_eq!(task_status["active_task"]["task_id"], task_id);
    assert!(task_status["recent_tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|task| task["task_id"] == task_id && task["evidence_passed"] == 2));

    run_keel(repo.path(), ["task", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Keel task status"))
        .stdout(predicate::str::contains("Active task: self dogfood json"));

    let finished = parse_json_object(&run_keel_output(repo.path(), ["task", "finish", "--json"]));
    assert_eq!(finished["task_id"], task_id);
    assert_eq!(finished["status"], "finished");

    run_keel(repo.path(), ["task", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Active task: none"))
        .stdout(predicate::str::contains("[finished] self dogfood json"));

    run_keel(repo.path(), ["review"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no active Keel task found"));

    let shown = parse_json_object(&run_keel_output(
        repo.path(),
        ["task", "show", &task_id, "--json"],
    ));
    assert_eq!(shown["task"]["task_id"], task_id);
    assert_eq!(shown["decision"]["ready"], true);

    run_keel(repo.path(), ["task", "show", &task_id])
        .assert()
        .success()
        .stdout(predicate::str::contains("Keel task"))
        .stdout(predicate::str::contains("self dogfood json"));

    let reopened = parse_json_object(&run_keel_output(
        repo.path(),
        ["task", "reopen", &task_id, "--json"],
    ));
    assert_eq!(reopened["task_id"], task_id);
    assert_eq!(reopened["status"], "active");

    run_keel(repo.path(), ["review"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Task: self dogfood json"));

    run_keel(repo.path(), ["task", "show", "../bad"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid task id"));
}

#[test]
fn ledger_failed_evidence_makes_verify_fail() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();
    run_keel(repo.path(), ["task", "start", "failed evidence task"])
        .assert()
        .success();

    run_keel(
        repo.path(),
        ["evidence", "add", "--cmd", "definitely-not-a-keel-command"],
    )
    .assert()
    .success()
    .stdout(predicate::str::contains("Status: failed"));

    run_keel(repo.path(), ["verify"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("Decision: not ready"))
        .stdout(predicate::str::contains("latest evidence command failed"));

    let review = parse_json_object(&run_keel_output(repo.path(), ["review", "--json"]));
    assert_eq!(review["decision"]["ready"], false);
    assert_eq!(review["summary"]["evidence_failed"], 1);
    assert_eq!(review["summary"]["current_evidence_failed"], 0);
    assert!(review["packet"]["evidence"]["failed"]
        .as_array()
        .unwrap()
        .iter()
        .any(|evidence| evidence["command"] == "definitely-not-a-keel-command"));
}

#[test]
fn tui_command_is_exposed_as_read_only_review_ui() {
    Command::cargo_bin("keel")
        .unwrap()
        .args(["--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Local-first control layer for AI-generated code",
        ))
        .stdout(predicate::str::contains("--run"))
        .stdout(predicate::str::contains("tui"))
        .stdout(predicate::str::contains("task"))
        .stdout(predicate::str::contains("checkpoint"))
        .stdout(predicate::str::contains("evidence"))
        .stdout(predicate::str::contains("handoff"))
        .stdout(predicate::str::contains("review"));

    Command::cargo_bin("keel")
        .unwrap()
        .args(["tui", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Open the read-only terminal review UI",
        ))
        .stdout(predicate::str::contains("--filter"))
        .stdout(predicate::str::contains("--run"))
        .stdout(predicate::str::contains("--agent"))
        .stdout(predicate::str::contains("--status"));
}

#[test]
fn status_lists_runs_newest_first_and_filters_review_output() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();
    let first = run_noop(&repo, "first status task");
    let second = run_noop(&repo, "second status task");

    let status = run_keel_output(repo.path(), ["status"]);
    assert_contains_in_order(&status, &second.run_id, &first.run_id);

    let agent_status = run_keel_output(repo.path(), ["status", "--agent", "noop"]);
    assert!(agent_status.contains(&first.run_id));
    assert!(agent_status.contains(&second.run_id));
    assert!(agent_status.contains("noop"));

    let ready_status = run_keel_output(repo.path(), ["status", "--status", "ready"]);
    assert!(ready_status.contains(&first.run_id));
    assert!(ready_status.contains(&second.run_id));
    assert!(ready_status.contains("ready"));

    run_keel(repo.path(), ["status", "--agent", "codex"])
        .assert()
        .success()
        .stdout(predicate::str::contains(NO_MATCHES_MESSAGE));

    insta::assert_snapshot!(
        "status_no_matches",
        normalize_output(&run_keel_output(
            repo.path(),
            ["status", "--agent", "codex"]
        ))
    );
}

#[test]
fn status_limit_filters_before_truncating() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();
    let first = run_noop(&repo, "first limit task");
    let second = run_noop(&repo, "second limit task");

    let limited = run_keel_output(repo.path(), ["status", "--limit", "1"]);
    assert!(limited.contains(&second.run_id));
    assert!(!limited.contains(&first.run_id));

    let agent_limited = run_keel_output(repo.path(), ["status", "--agent", "noop", "--limit", "1"]);
    assert!(agent_limited.contains(&second.run_id));
    assert!(!agent_limited.contains(&first.run_id));

    let status_limited =
        run_keel_output(repo.path(), ["status", "--status", "ready", "--limit", "1"]);
    assert!(status_limited.contains(&second.run_id));
    assert!(!status_limited.contains(&first.run_id));

    run_keel(repo.path(), ["status", "--limit", "0"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
}

#[test]
fn status_json_is_parseable_and_respects_filters_and_limit() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();
    let first = run_noop(&repo, "first json task");
    let second = run_noop(&repo, "second json task");

    let runs = parse_json_array(&run_keel_output(repo.path(), ["status", "--json"]));
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0]["run_id"], second.run_id);
    assert_eq!(runs[0]["agent"], "noop");
    assert_eq!(runs[0]["status"], "ready");
    assert!(runs[0].get("parent_run_id").is_some());
    assert!(runs[0].get("task").is_some());
    assert!(runs[0].get("created_at").is_some());
    assert!(runs[0].get("worktree").is_some());
    assert!(runs[0].get("branch").is_some());
    assert!(runs[0].get("failure_reason").is_some());

    let agent_runs = parse_json_array(&run_keel_output(
        repo.path(),
        ["status", "--json", "--agent", "noop"],
    ));
    assert_eq!(agent_runs.len(), 2);

    let ready_runs = parse_json_array(&run_keel_output(
        repo.path(),
        ["status", "--json", "--status", "ready"],
    ));
    assert_eq!(ready_runs.len(), 2);

    let limited_runs = parse_json_array(&run_keel_output(
        repo.path(),
        ["status", "--json", "--limit", "1"],
    ));
    assert_eq!(limited_runs.len(), 1);
    assert_eq!(limited_runs[0]["run_id"], second.run_id);

    let no_match = parse_json_array(&run_keel_output(
        repo.path(),
        ["status", "--json", "--agent", "codex"],
    ));
    assert!(no_match.is_empty());
    assert_ne!(runs[0]["run_id"], first.run_id);
}

#[test]
fn report_outputs_artifacts_and_suggested_next_actions() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();
    let run = run_noop(&repo, "report workflow task");

    let output = run_keel_output(repo.path(), ["report", run.run_id.as_str()]);
    assert!(output.contains("Report:"));
    assert!(output.contains("report.md"));
    for artifact in [
        "metadata.json",
        "log.txt",
        "diff.patch",
        "checks.json",
        "report.md",
    ] {
        assert!(
            output.contains(artifact),
            "report output missing artifact {artifact}"
        );
    }
    for action in [
        format!("keel diff {}", run.run_id),
        format!("keel rerun {}", run.run_id),
        format!("keel discard {}", run.run_id),
    ] {
        assert!(output.contains(&action), "report output missing {action}");
    }

    insta::assert_snapshot!("report_redacted", normalize_output(&output));
}

#[test]
fn report_json_is_parseable_and_includes_review_summary() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();
    let run = run_noop(&repo, "report json task");

    let output = run_keel_output(repo.path(), ["report", run.run_id.as_str(), "--json"]);
    let report: Value = serde_json::from_str(&output).unwrap();

    assert_eq!(report["run_id"], run.run_id);
    assert_eq!(report["task"], "report json task");
    assert_eq!(report["agent"], "noop");
    assert_eq!(report["status"], "ready");
    assert!(report.get("parent_run_id").is_some());
    assert!(report.get("created_at").is_some());
    assert!(report.get("worktree").is_some());
    assert!(report.get("branch").is_some());
    assert!(report.get("base_commit").is_some());
    assert!(report.get("failure_reason").is_some());
    assert!(report.get("readiness_reason").is_some());
    assert!(report["warnings"].is_array());
    assert!(report["risk_warnings"].is_array());

    for key in ["metadata", "log", "diff", "checks", "report"] {
        assert_eq!(report["artifacts"][key]["exists"], true);
        assert_eq!(report["artifacts"][key]["state"], "present");
        assert!(report["artifacts"][key]["path"]
            .as_str()
            .unwrap()
            .contains(match key {
                "metadata" => "metadata.json",
                "log" => "log.txt",
                "diff" => "diff.patch",
                "checks" => "checks.json",
                "report" => "report.md",
                _ => unreachable!(),
            }));
    }

    let actions = report["next_actions"].as_array().unwrap();
    assert!(actions
        .iter()
        .any(|action| action == &format!("keel diff {}", run.run_id)));
    assert!(actions
        .iter()
        .any(|action| action == &format!("keel rerun {}", run.run_id)));
    assert!(actions
        .iter()
        .any(|action| action == &format!("keel discard {}", run.run_id)));
}

#[test]
fn report_outputs_risk_warnings_in_human_and_json_modes() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();
    fs::write(
        repo.path().join(".keel").join("config.toml"),
        r#"version = 1
runs_dir = "runs"
worktrees_dir = "worktrees"

[[checks]]
name = "git status"
command = ["git", "status", "--short"]

[risk]
paths = ["keel-noop-output.txt"]
"#,
    )
    .unwrap();
    let run = run_noop(&repo, "risk report task");

    run_keel(repo.path(), ["report", run.run_id.as_str()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Warnings:"))
        .stdout(predicate::str::contains(
            "touched risk path: keel-noop-output.txt matched keel-noop-output.txt",
        ));

    let report = parse_json_object(&run_keel_output(
        repo.path(),
        ["report", run.run_id.as_str(), "--json"],
    ));
    assert!(report["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|warning| warning
            .as_str()
            .unwrap()
            .contains("touched risk path: keel-noop-output.txt")));
    let risk_warnings = report["risk_warnings"].as_array().unwrap();
    assert!(risk_warnings.iter().any(|warning| {
        warning["kind"] == "risk_path"
            && warning["path"] == "keel-noop-output.txt"
            && warning["pattern"] == "keel-noop-output.txt"
    }));
}

#[test]
fn report_json_handles_discarded_and_missing_artifacts() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();
    let run = run_noop(&repo, "discarded report json task");
    fs::remove_file(run_artifact_path(&repo, &run.run_id, "checks.json")).unwrap();

    let missing_artifact = parse_json_object(&run_keel_output(
        repo.path(),
        ["report", run.run_id.as_str(), "--json"],
    ));
    assert_eq!(missing_artifact["artifacts"]["checks"]["exists"], false);
    assert_eq!(missing_artifact["artifacts"]["checks"]["state"], "missing");

    run_keel(repo.path(), ["discard", run.run_id.as_str()])
        .assert()
        .success();
    let discarded = parse_json_object(&run_keel_output(
        repo.path(),
        ["report", run.run_id.as_str(), "--json"],
    ));
    let actions = discarded["next_actions"].as_array().unwrap();
    assert!(!actions
        .iter()
        .any(|action| action == &format!("keel discard {}", run.run_id)));

    run_keel(repo.path(), ["report", "run-does-not-exist", "--json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "run `run-does-not-exist` does not exist",
        ));
}

#[test]
fn commit_rejects_missing_not_ready_and_discarded_runs() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();

    run_keel(repo.path(), ["commit", "run-does-not-exist"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "run `run-does-not-exist` does not exist",
        ));

    let run = run_noop(&repo, "not ready cli commit task");
    let metadata_path = run_artifact_path(&repo, &run.run_id, "metadata.json");
    let mut metadata = parse_json_object(&read_run_artifact(&repo, &run.run_id, "metadata.json"));
    metadata["status"] = Value::String("not_ready".to_string());
    fs::write(
        &metadata_path,
        serde_json::to_string_pretty(&metadata).unwrap(),
    )
    .unwrap();
    run_keel(repo.path(), ["commit", run.run_id.as_str()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("only ready runs can be committed"));

    metadata["status"] = Value::String("ready".to_string());
    fs::write(
        &metadata_path,
        serde_json::to_string_pretty(&metadata).unwrap(),
    )
    .unwrap();
    run_keel(repo.path(), ["discard", run.run_id.as_str()])
        .assert()
        .success();
    run_keel(repo.path(), ["commit", run.run_id.as_str()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("only ready runs can be committed"));
}

#[test]
fn commit_dry_run_outputs_plan_and_does_not_write_artifacts() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();
    let run = run_noop(&repo, "cli dry run commit task");
    let metadata_before = read_run_artifact(&repo, &run.run_id, "metadata.json");
    let report_before = read_run_artifact(&repo, &run.run_id, "report.md");

    run_keel(repo.path(), ["commit", run.run_id.as_str(), "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Commit dry-run plan"))
        .stdout(predicate::str::contains("Would run: git add -A"))
        .stdout(predicate::str::contains("Would run: git commit"));

    assert!(!run_artifact_path(&repo, &run.run_id, "commit.json").exists());
    assert_eq!(
        read_run_artifact(&repo, &run.run_id, "metadata.json"),
        metadata_before
    );
    assert_eq!(
        read_run_artifact(&repo, &run.run_id, "report.md"),
        report_before
    );
}

#[test]
fn commit_dry_run_json_is_parseable() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();
    let run = run_noop(&repo, "cli dry run json task");

    let result = parse_json_object(&run_keel_output(
        repo.path(),
        ["commit", run.run_id.as_str(), "--dry-run", "--json"],
    ));

    assert_eq!(result["run_id"], run.run_id);
    assert_eq!(result["committed"], false);
    assert_eq!(result["dry_run"], true);
    assert_eq!(result["would_git_add"], true);
    assert_eq!(result["would_git_commit"], true);
    assert!(result["warnings"].is_array());
}

#[test]
fn commit_success_is_idempotent_and_updates_report_surfaces() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();
    let run = run_noop(&repo, "cli commit success task");

    run_keel(
        repo.path(),
        [
            "commit",
            run.run_id.as_str(),
            "--message",
            "keel: cli local commit",
        ],
    )
    .assert()
    .success()
    .stdout(predicate::str::contains("Committed run"))
    .stdout(predicate::str::contains(
        "Keel did not push or merge anything.",
    ));

    assert!(run_artifact_path(&repo, &run.run_id, "commit.json").is_file());
    let metadata = parse_json_object(&read_run_artifact(&repo, &run.run_id, "metadata.json"));
    assert_eq!(metadata["committed"], true);
    let commit_sha = metadata["commit_sha"].as_str().unwrap().to_string();
    assert!(!commit_sha.is_empty());
    assert_eq!(metadata["commit_message"], "keel: cli local commit");

    let report = read_run_artifact(&repo, &run.run_id, "report.md");
    assert!(report.contains("## Commit"));
    assert!(report.contains("Keel did not push or merge anything."));
    assert!(report.contains(&commit_sha));

    let git_subject = git_output(
        &worktree_dir(&repo, &run.run_id),
        ["log", "-1", "--format=%s"],
    );
    assert_eq!(git_subject.trim(), "keel: cli local commit");

    let second = run_keel_output(repo.path(), ["commit", run.run_id.as_str()]);
    assert!(second.contains("This run is already committed"));
    let after_second = parse_json_object(&read_run_artifact(&repo, &run.run_id, "metadata.json"));
    assert_eq!(after_second["commit_sha"], commit_sha);

    run_keel(repo.path(), ["report", run.run_id.as_str()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Commit:"))
        .stdout(predicate::str::contains(&commit_sha))
        .stdout(predicate::str::contains("commit.json"));

    let report_json = parse_json_object(&run_keel_output(
        repo.path(),
        ["report", run.run_id.as_str(), "--json"],
    ));
    assert_eq!(report_json["commit"]["commit_sha"], commit_sha);
    assert_eq!(report_json["artifacts"]["commit"]["exists"], true);

    let already_json = parse_json_object(&run_keel_output(
        repo.path(),
        ["commit", run.run_id.as_str(), "--json"],
    ));
    assert_eq!(already_json["already_committed"], true);
    assert_eq!(already_json["commit_sha"], commit_sha);

    run_keel(repo.path(), ["discard", run.run_id.as_str()])
        .assert()
        .success();
    assert!(!worktree_dir(&repo, &run.run_id).exists());
    assert!(branch_exists(&repo, metadata["branch"].as_str().unwrap()));
    let discarded_report = read_run_artifact(&repo, &run.run_id, "report.md");
    assert!(discarded_report.contains("Branch cleanup: `preserved committed branch`"));
}

#[test]
fn commit_with_warnings_succeeds_and_preserves_warning_summary() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();
    fs::write(
        repo.path().join(".keel").join("config.toml"),
        r#"version = 1
runs_dir = "runs"
worktrees_dir = "worktrees"

[[checks]]
name = "git status"
command = ["git", "status", "--short"]

[risk]
paths = ["keel-noop-output.txt"]
"#,
    )
    .unwrap();
    let run = run_noop(&repo, "cli warning commit task");

    run_keel(repo.path(), ["commit", run.run_id.as_str()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Warnings:"))
        .stdout(predicate::str::contains("touched risk path"));

    let commit = read_run_artifact(&repo, &run.run_id, "commit.json");
    assert!(commit.contains("touched risk path"));
    let report = read_run_artifact(&repo, &run.run_id, "report.md");
    assert!(report.contains("touched risk path"));
}

#[test]
fn push_rejects_missing_uncommitted_missing_remote_and_discarded_runs() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();

    run_keel(repo.path(), ["publish", "run-does-not-exist"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("publish"));

    run_keel(repo.path(), ["push", "run-does-not-exist"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "run `run-does-not-exist` does not exist",
        ));

    let run = run_noop(&repo, "cli push rejection task");
    run_keel(repo.path(), ["push", run.run_id.as_str()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("is not committed"))
        .stderr(predicate::str::contains("keel commit"));

    run_keel(repo.path(), ["commit", run.run_id.as_str()])
        .assert()
        .success();
    run_keel(repo.path(), ["push", run.run_id.as_str()])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "git remote `origin` does not exist",
        ));

    run_keel(repo.path(), ["discard", run.run_id.as_str()])
        .assert()
        .success();
    run_keel(repo.path(), ["push", run.run_id.as_str()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("only ready runs can be pushed"));
}

#[test]
fn push_dry_run_outputs_plan_and_does_not_write_artifacts() {
    let repo = create_temp_git_repo();
    let remote = create_bare_git_repo();
    git(
        repo.path(),
        ["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    run_keel(repo.path(), ["init"]).assert().success();
    let run = run_noop(&repo, "cli push dry run task");
    run_keel(repo.path(), ["commit", run.run_id.as_str()])
        .assert()
        .success();
    let metadata_before = read_run_artifact(&repo, &run.run_id, "metadata.json");
    let report_before = read_run_artifact(&repo, &run.run_id, "report.md");

    run_keel(repo.path(), ["push", run.run_id.as_str(), "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Push dry-run plan"))
        .stdout(predicate::str::contains("Would run: git push -u origin"))
        .stdout(predicate::str::contains("Remote URL:"));

    assert!(!run_artifact_path(&repo, &run.run_id, "push.json").exists());
    assert_eq!(
        read_run_artifact(&repo, &run.run_id, "metadata.json"),
        metadata_before
    );
    assert_eq!(
        read_run_artifact(&repo, &run.run_id, "report.md"),
        report_before
    );
}

#[test]
fn push_dry_run_json_is_parseable() {
    let repo = create_temp_git_repo();
    let remote = create_bare_git_repo();
    git(
        repo.path(),
        ["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    run_keel(repo.path(), ["init"]).assert().success();
    let run = run_noop(&repo, "cli push dry run json task");
    run_keel(repo.path(), ["commit", run.run_id.as_str()])
        .assert()
        .success();

    let result = parse_json_object(&run_keel_output(
        repo.path(),
        ["push", run.run_id.as_str(), "--dry-run", "--json"],
    ));

    assert_eq!(result["run_id"], run.run_id);
    assert_eq!(result["pushed"], false);
    assert_eq!(result["dry_run"], true);
    assert_eq!(result["would_push"], true);
    assert_eq!(result["remote"], "origin");
    assert!(result["remote_url"]
        .as_str()
        .unwrap()
        .contains(remote.path().to_str().unwrap()));
}

#[test]
fn push_success_is_idempotent_and_updates_report_surfaces() {
    let repo = create_temp_git_repo();
    let remote = create_bare_git_repo();
    git(
        repo.path(),
        ["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    run_keel(repo.path(), ["init"]).assert().success();
    let run = run_noop(&repo, "cli push success task");
    run_keel(repo.path(), ["commit", run.run_id.as_str()])
        .assert()
        .success();
    let metadata_before_push =
        parse_json_object(&read_run_artifact(&repo, &run.run_id, "metadata.json"));
    let branch = metadata_before_push["branch"].as_str().unwrap().to_string();
    let commit_sha = metadata_before_push["commit_sha"]
        .as_str()
        .unwrap()
        .to_string();

    run_keel(repo.path(), ["push", run.run_id.as_str()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Pushed run"))
        .stdout(predicate::str::contains("Keel did not create a PR/MR."))
        .stdout(predicate::str::contains("Keel did not merge anything."));

    assert!(run_artifact_path(&repo, &run.run_id, "push.json").is_file());
    assert!(!run_artifact_path(&repo, &run.run_id, "publish.json").exists());
    assert_eq!(
        git_output(remote.path(), ["rev-parse", branch.as_str()]).trim(),
        commit_sha
    );
    let metadata = parse_json_object(&read_run_artifact(&repo, &run.run_id, "metadata.json"));
    assert_eq!(metadata["pushed"], true);
    assert_eq!(metadata["push_remote"], "origin");
    assert_eq!(metadata["pushed_branch"], branch);
    assert!(metadata.get("published").is_none());
    assert!(metadata.get("publish_remote").is_none());
    assert!(metadata.get("publish").is_none());

    let report = read_run_artifact(&repo, &run.run_id, "report.md");
    assert!(report.contains("## Push"));
    assert!(report.contains("Keel did not create a PR/MR."));
    assert!(report.contains("Keel did not merge anything."));

    let second = run_keel_output(repo.path(), ["push", run.run_id.as_str()]);
    assert!(second.contains("This run is already pushed"));
    let after_second = read_run_artifact(&repo, &run.run_id, "report.md");
    assert_eq!(after_second, report);

    run_keel(repo.path(), ["report", run.run_id.as_str()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Push:"))
        .stdout(predicate::str::contains("push.json"));

    let report_json = parse_json_object(&run_keel_output(
        repo.path(),
        ["report", run.run_id.as_str(), "--json"],
    ));
    assert_eq!(report_json["push"]["commit_sha"], commit_sha);
    assert_eq!(report_json["artifacts"]["push"]["exists"], true);
    assert!(report_json.get("publish").is_none());
    assert!(report_json["artifacts"].get("publish").is_none());

    let already_json = parse_json_object(&run_keel_output(
        repo.path(),
        ["push", run.run_id.as_str(), "--json"],
    ));
    assert_eq!(already_json["already_pushed"], true);
    assert_eq!(already_json["commit_sha"], commit_sha);
    assert!(already_json.get("already_published").is_none());
    assert!(already_json.get("publish_path").is_none());
}

#[test]
fn pr_manual_dry_run_outputs_human_and_json_plan_without_writing_artifacts() {
    let repo = create_temp_git_repo();
    git(
        repo.path(),
        ["remote", "add", "origin", "git@github.com:owner/repo.git"],
    );
    run_keel(repo.path(), ["init"]).assert().success();
    let run = run_noop(&repo, "cli pr manual dry run task");
    run_keel(repo.path(), ["commit", run.run_id.as_str()])
        .assert()
        .success();
    mark_run_pushed(&repo, &run.run_id, "git@github.com:owner/repo.git");
    let metadata_before = read_run_artifact(&repo, &run.run_id, "metadata.json");
    let report_before = read_run_artifact(&repo, &run.run_id, "report.md");

    run_keel(
        repo.path(),
        [
            "pr",
            run.run_id.as_str(),
            "--manual",
            "--dry-run",
            "--provider",
            "github",
        ],
    )
    .assert()
    .success()
    .stdout(predicate::str::contains("PR/MR manual dry-run plan"))
    .stdout(predicate::str::contains("Provider: GitHub"))
    .stdout(predicate::str::contains("Request kind: pull_request"))
    .stdout(predicate::str::contains(
        "Web URL: https://github.com/owner/repo/compare/",
    ))
    .stdout(predicate::str::contains("Keel did not create a PR/MR."))
    .stdout(predicate::str::contains("Keel did not write pr.json."))
    .stdout(predicate::str::contains("Keel did not merge anything."));

    assert!(!run_artifact_path(&repo, &run.run_id, "pr.json").exists());
    assert_eq!(
        read_run_artifact(&repo, &run.run_id, "metadata.json"),
        metadata_before
    );
    assert_eq!(
        read_run_artifact(&repo, &run.run_id, "report.md"),
        report_before
    );

    let json = parse_json_object(&run_keel_output(
        repo.path(),
        [
            "pr",
            run.run_id.as_str(),
            "--manual",
            "--dry-run",
            "--provider",
            "github",
            "--json",
        ],
    ));
    assert_eq!(json["run_id"], run.run_id);
    assert_eq!(json["provider"], "github");
    assert_eq!(json["provider_name"], "GitHub");
    assert_eq!(json["request_kind"], "pull_request");
    assert_eq!(json["manual"], true);
    assert_eq!(json["dry_run"], true);
    assert!(json["repository_url"]
        .as_str()
        .unwrap()
        .starts_with("https://github.com/owner/repo"));
    assert!(json["web_url"]
        .as_str()
        .unwrap()
        .starts_with("https://github.com/owner/repo/compare/"));
    assert!(json["copyable_summary"]
        .as_str()
        .unwrap()
        .contains("cli pr manual dry run task"));
    assert!(json["body"]
        .as_str()
        .unwrap()
        .contains("## Keel Candidate Change"));
    assert!(json["body"].as_str().unwrap().contains("## Artifacts"));
    assert!(json["body"]
        .as_str()
        .unwrap()
        .contains("Keel did not merge this candidate change"));
    assert!(json["artifacts"]["metadata"]
        .as_str()
        .unwrap()
        .contains("metadata.json"));
    assert!(json["artifacts"]["commit"]
        .as_str()
        .unwrap()
        .contains("commit.json"));
    assert!(json["artifacts"]["push"]
        .as_str()
        .unwrap()
        .contains("push.json"));
    assert_eq!(json["would_create_request"], false);
    assert_eq!(json["would_write_artifact"], false);
    assert_eq!(json["would_push"], false);
    assert_eq!(json["would_merge"], false);
    assert!(json["manual_steps"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item
            .as_str()
            .unwrap()
            .contains("Keel did not call any provider API.")));
    assert!(json.get("instructions").is_none());
}

#[test]
fn pr_manual_dry_run_rejects_missing_flags_and_unpushed_runs() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();
    let run = run_noop(&repo, "cli pr rejection task");

    run_keel(
        repo.path(),
        ["pr", run.run_id.as_str(), "--manual", "--dry-run"],
    )
    .assert()
    .failure()
    .stderr(predicate::str::contains("is not committed"));

    run_keel(repo.path(), ["commit", run.run_id.as_str()])
        .assert()
        .success();
    run_keel(
        repo.path(),
        [
            "pr",
            run.run_id.as_str(),
            "--manual",
            "--dry-run",
            "--provider",
            "github",
        ],
    )
    .assert()
    .failure()
    .stderr(predicate::str::contains("is not pushed"));

    run_keel(repo.path(), ["pr", run.run_id.as_str(), "--manual"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--manual --dry-run"));
}

#[test]
fn pr_provider_dry_run_outputs_plan_without_calling_provider_or_writing_artifacts() {
    let repo = create_temp_git_repo();
    git(
        repo.path(),
        ["remote", "add", "origin", "git@github.com:owner/repo.git"],
    );
    run_keel(repo.path(), ["init"]).assert().success();
    let run = run_noop(&repo, "cli pr provider dry run task");
    run_keel(repo.path(), ["commit", run.run_id.as_str()])
        .assert()
        .success();
    mark_run_pushed(&repo, &run.run_id, "git@github.com:owner/repo.git");
    let metadata_before = read_run_artifact(&repo, &run.run_id, "metadata.json");
    let report_before = read_run_artifact(&repo, &run.run_id, "report.md");

    run_keel_with_path(
        repo.path(),
        [
            "pr",
            run.run_id.as_str(),
            "--provider",
            "github",
            "--base",
            "release/v1",
            "--head",
            "owner:feature-branch",
            "--draft",
            "--dry-run",
        ],
        &path_with_git_only(),
    )
    .assert()
    .success()
    .stdout(predicate::str::contains("PR/MR provider dry-run plan"))
    .stdout(predicate::str::contains("Would run: gh pr create"))
    .stdout(predicate::str::contains(
        "Source branch: owner:feature-branch",
    ))
    .stdout(predicate::str::contains("Target branch: release/v1"))
    .stdout(predicate::str::contains("Draft: yes"))
    .stdout(predicate::str::contains("Keel would not merge anything."));

    assert!(!run_artifact_path(&repo, &run.run_id, "pr.json").exists());
    assert_eq!(
        read_run_artifact(&repo, &run.run_id, "metadata.json"),
        metadata_before
    );
    assert_eq!(
        read_run_artifact(&repo, &run.run_id, "report.md"),
        report_before
    );

    let metadata_json = parse_json_object(&metadata_before);
    let expected_source_branch = metadata_json["branch"].as_str().unwrap();
    let json = parse_json_object(&run_keel_output_with_path(
        repo.path(),
        [
            "pr",
            run.run_id.as_str(),
            "--provider",
            "github",
            "--draft",
            "--dry-run",
            "--json",
        ],
        &path_with_git_only(),
        true,
    ));
    assert_eq!(json["created"], false);
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["would_create_request"], true);
    assert_eq!(json["would_write_artifact"], false);
    assert_eq!(json["would_push"], false);
    assert_eq!(json["would_merge"], false);
    assert_eq!(json["draft"], true);
    assert_eq!(json["provider_command"][0], "gh");
    assert_eq!(json["target_branch"], "master");
    assert_eq!(json["source_branch"], expected_source_branch);
    assert!(json["provider_command"]
        .as_array()
        .unwrap()
        .iter()
        .any(|arg| arg == "--draft"));

    let custom_json = parse_json_object(&run_keel_output_with_path(
        repo.path(),
        [
            "pr",
            run.run_id.as_str(),
            "--provider",
            "github",
            "--base",
            "release/v1",
            "--head",
            "owner:feature-branch",
            "--title",
            "custom pr title",
            "--draft",
            "--dry-run",
            "--json",
        ],
        &path_with_git_only(),
        true,
    ));
    assert_eq!(custom_json["target_branch"], "release/v1");
    assert_eq!(custom_json["source_branch"], "owner:feature-branch");
    assert_eq!(custom_json["title"], "custom pr title");
    let command = custom_json["provider_command"].as_array().unwrap();
    assert_eq!(command[5], "--base");
    assert_eq!(command[6], "release/v1");
    assert_eq!(command[7], "--head");
    assert_eq!(command[8], "owner:feature-branch");
    assert_eq!(command[9], "--title");
    assert_eq!(command[10], "custom pr title");
    let body_index = command.iter().position(|arg| arg == "--body").unwrap();
    assert!(command[body_index + 1]
        .as_str()
        .unwrap()
        .contains("Source branch: `owner:feature-branch`"));
    assert!(command.iter().any(|arg| arg == "--body"));
    assert!(command.iter().any(|arg| arg == "--draft"));
}

#[test]
fn pr_provider_rejects_missing_cli_and_unsupported_provider() {
    let repo = create_temp_git_repo();
    git(
        repo.path(),
        ["remote", "add", "origin", "git@github.com:owner/repo.git"],
    );
    run_keel(repo.path(), ["init"]).assert().success();
    let run = run_noop(&repo, "cli pr missing provider task");
    run_keel(repo.path(), ["commit", run.run_id.as_str()])
        .assert()
        .success();
    mark_run_pushed(&repo, &run.run_id, "git@github.com:owner/repo.git");

    run_keel_with_path(
        repo.path(),
        ["pr", run.run_id.as_str(), "--provider", "github"],
        &path_with_git_only(),
    )
    .assert()
    .failure()
    .stderr(predicate::str::contains("gh CLI not found"));
    assert!(!run_artifact_path(&repo, &run.run_id, "pr.json").exists());

    run_keel(
        repo.path(),
        [
            "pr",
            run.run_id.as_str(),
            "--provider",
            "gitee",
            "--dry-run",
        ],
    )
    .assert()
    .failure()
    .stderr(predicate::str::contains(
        "provider-backed PR/MR creation for Gitee is not implemented in v0.5c",
    ));
}

#[test]
fn pr_provider_github_success_is_idempotent_and_updates_report_surfaces() {
    let repo = create_temp_git_repo();
    git(
        repo.path(),
        ["remote", "add", "origin", "git@github.com:owner/repo.git"],
    );
    run_keel(repo.path(), ["init"]).assert().success();
    let run = run_noop(&repo, "cli pr github success task");
    run_keel(repo.path(), ["commit", run.run_id.as_str()])
        .assert()
        .success();
    mark_run_pushed(&repo, &run.run_id, "git@github.com:owner/repo.git");
    let bin = tempfile::tempdir().unwrap();
    create_fake_provider_cli(bin.path(), "gh", "https://github.com/owner/repo/pull/42");

    run_keel_with_path(
        repo.path(),
        ["pr", run.run_id.as_str(), "--provider", "github", "--draft"],
        &path_with_git_and(bin.path()),
    )
    .assert()
    .success()
    .stdout(predicate::str::contains("Created pull_request"))
    .stdout(predicate::str::contains(
        "https://github.com/owner/repo/pull/42",
    ))
    .stdout(predicate::str::contains("Keel did not merge anything."));

    assert!(run_artifact_path(&repo, &run.run_id, "pr.json").is_file());
    let metadata = parse_json_object(&read_run_artifact(&repo, &run.run_id, "metadata.json"));
    assert_eq!(metadata["pr_created"], true);
    assert_eq!(metadata["pr_provider"], "github");
    assert_eq!(metadata["pr_url"], "https://github.com/owner/repo/pull/42");
    assert_eq!(metadata["pr"]["draft"], true);
    assert_eq!(metadata["pr"]["reused_existing"], false);

    let report = read_run_artifact(&repo, &run.run_id, "report.md");
    assert!(report.contains("## PR/MR"));
    assert!(report.contains("https://github.com/owner/repo/pull/42"));
    assert!(report.contains("Draft: `yes`"));
    assert!(report.contains("Reused existing: `no`"));

    let second = run_keel_output_with_path(
        repo.path(),
        ["pr", run.run_id.as_str(), "--provider", "github"],
        &path_with_git_and(bin.path()),
        true,
    );
    assert!(second.contains("already has a PR/MR"));
    assert_eq!(read_run_artifact(&repo, &run.run_id, "report.md"), report);

    run_keel(repo.path(), ["report", run.run_id.as_str()])
        .assert()
        .success()
        .stdout(predicate::str::contains("PR/MR:"))
        .stdout(predicate::str::contains("pr.json"));

    let report_json = parse_json_object(&run_keel_output(
        repo.path(),
        ["report", run.run_id.as_str(), "--json"],
    ));
    assert_eq!(
        report_json["pr"]["url"],
        "https://github.com/owner/repo/pull/42"
    );
    assert_eq!(report_json["artifacts"]["pr"]["exists"], true);

    let already_json = parse_json_object(&run_keel_output_with_path(
        repo.path(),
        ["pr", run.run_id.as_str(), "--provider", "github", "--json"],
        &path_with_git_and(bin.path()),
        true,
    ));
    assert_eq!(already_json["already_created"], true);
    assert_eq!(already_json["created"], true);
}

#[test]
fn pr_workflow_runs_noop_commit_real_push_then_github_provider_boundary() {
    let repo = create_temp_git_repo();
    let remote = create_bare_git_repo();
    let github_remote = "git@github.com:owner/repo.git";
    let rewrite_key = format!("url.{}.insteadOf", git_file_url(remote.path()));
    git(repo.path(), ["config", rewrite_key.as_str(), github_remote]);
    git(repo.path(), ["remote", "add", "origin", github_remote]);
    run_keel(repo.path(), ["init"]).assert().success();
    let run = run_noop(&repo, "cli pr full workflow task");

    run_keel(repo.path(), ["commit", run.run_id.as_str()])
        .assert()
        .success();
    run_keel(repo.path(), ["push", run.run_id.as_str()])
        .assert()
        .success();

    let pushed_metadata =
        parse_json_object(&read_run_artifact(&repo, &run.run_id, "metadata.json"));
    let branch = pushed_metadata["branch"].as_str().unwrap();
    let commit_sha = pushed_metadata["commit_sha"].as_str().unwrap();
    let remote_ref = format!("refs/heads/{branch}");
    assert_eq!(
        git_output(remote.path(), ["rev-parse", remote_ref.as_str()]).trim(),
        commit_sha
    );
    assert_eq!(pushed_metadata["push_remote_url"], github_remote);
    assert!(run_artifact_path(&repo, &run.run_id, "push.json").is_file());

    let bin = tempfile::tempdir().unwrap();
    create_fake_provider_cli(bin.path(), "gh", "https://github.com/owner/repo/pull/123");
    let pr_json = parse_json_object(&run_keel_output_with_path(
        repo.path(),
        ["pr", run.run_id.as_str(), "--provider", "github", "--json"],
        &path_with_git_and(bin.path()),
        true,
    ));

    assert_eq!(pr_json["created"], true);
    assert_eq!(pr_json["already_created"], false);
    assert_eq!(pr_json["reused_existing"], false);
    assert_eq!(pr_json["url"], "https://github.com/owner/repo/pull/123");
    assert_eq!(pr_json["would_push"], false);
    assert_eq!(pr_json["would_merge"], false);
    assert!(run_artifact_path(&repo, &run.run_id, "pr.json").is_file());

    let gh_calls = fs::read_to_string(bin.path().join("gh-args.txt")).unwrap();
    assert!(gh_calls.contains("pr list"));
    assert!(gh_calls.contains("pr create"));
    assert!(gh_calls.contains("--repo owner/repo"));
    assert!(!gh_calls.contains("git push"));
    assert!(!gh_calls.contains("git merge"));

    let report = parse_json_object(&run_keel_output(
        repo.path(),
        ["report", run.run_id.as_str(), "--json"],
    ));
    assert_eq!(report["push"]["remote_url"], github_remote);
    assert_eq!(
        report["pr"]["url"],
        "https://github.com/owner/repo/pull/123"
    );
    assert_eq!(report["artifacts"]["push"]["exists"], true);
    assert_eq!(report["artifacts"]["pr"]["exists"], true);
}

#[test]
fn pr_provider_github_reuses_existing_open_pr_before_create() {
    let repo = create_temp_git_repo();
    git(
        repo.path(),
        ["remote", "add", "origin", "git@github.com:owner/repo.git"],
    );
    run_keel(repo.path(), ["init"]).assert().success();
    let run = run_noop(&repo, "cli pr existing github task");
    run_keel(repo.path(), ["commit", run.run_id.as_str()])
        .assert()
        .success();
    mark_run_pushed(&repo, &run.run_id, "git@github.com:owner/repo.git");
    let bin = tempfile::tempdir().unwrap();
    create_fake_provider_cli_with_existing_pr(
        bin.path(),
        "gh",
        "https://github.com/owner/repo/pull/99",
        "existing pr title",
        true,
    );

    run_keel_with_path(
        repo.path(),
        ["pr", run.run_id.as_str(), "--provider", "github"],
        &path_with_git_and(bin.path()),
    )
    .assert()
    .success()
    .stdout(predicate::str::contains("Reused existing pull_request"))
    .stdout(predicate::str::contains(
        "https://github.com/owner/repo/pull/99",
    ))
    .stdout(predicate::str::contains(
        "Keel did not create a duplicate PR/MR.",
    ));

    let calls = fs::read_to_string(bin.path().join("gh-calls.txt")).unwrap();
    assert!(calls.contains("pr list"));
    assert!(!calls.contains("pr create"));

    let metadata = parse_json_object(&read_run_artifact(&repo, &run.run_id, "metadata.json"));
    assert_eq!(metadata["pr_created"], true);
    assert_eq!(metadata["pr_url"], "https://github.com/owner/repo/pull/99");
    assert_eq!(metadata["pr"]["title"], "existing pr title");
    assert_eq!(metadata["pr"]["draft"], true);
    assert_eq!(metadata["pr"]["reused_existing"], true);

    let report = read_run_artifact(&repo, &run.run_id, "report.md");
    assert!(report.contains("Reused existing: `yes`"));

    let report_json = parse_json_object(&run_keel_output(
        repo.path(),
        ["report", run.run_id.as_str(), "--json"],
    ));
    assert_eq!(report_json["pr"]["reused_existing"], true);
}

#[test]
fn pr_provider_github_normalizes_auth_and_permission_errors() {
    let repo = create_temp_git_repo();
    git(
        repo.path(),
        ["remote", "add", "origin", "git@github.com:owner/repo.git"],
    );
    run_keel(repo.path(), ["init"]).assert().success();
    let run = run_noop(&repo, "cli pr gh error task");
    run_keel(repo.path(), ["commit", run.run_id.as_str()])
        .assert()
        .success();
    mark_run_pushed(&repo, &run.run_id, "git@github.com:owner/repo.git");

    let auth_bin = tempfile::tempdir().unwrap();
    create_fake_provider_cli_failure(
        auth_bin.path(),
        "gh",
        "You are not logged into any GitHub hosts. Run gh auth login.",
    );
    run_keel_with_path(
        repo.path(),
        ["pr", run.run_id.as_str(), "--provider", "github"],
        &path_with_git_and(auth_bin.path()),
    )
    .assert()
    .failure()
    .stderr(predicate::str::contains("GitHub CLI is not authenticated"));

    let permission_bin = tempfile::tempdir().unwrap();
    create_fake_provider_cli_failure(
        permission_bin.path(),
        "gh",
        "HTTP 403: Resource not accessible by integration",
    );
    run_keel_with_path(
        repo.path(),
        ["pr", run.run_id.as_str(), "--provider", "github"],
        &path_with_git_and(permission_bin.path()),
    )
    .assert()
    .failure()
    .stderr(predicate::str::contains(
        "GitHub CLI does not have permission",
    ));
}

#[test]
fn pr_provider_gitlab_auto_create_is_not_supported_but_manual_plan_works() {
    let repo = create_temp_git_repo();
    git(
        repo.path(),
        ["remote", "add", "origin", "git@gitlab.com:owner/repo.git"],
    );
    run_keel(repo.path(), ["init"]).assert().success();
    let run = run_noop(&repo, "cli pr gitlab success task");
    run_keel(repo.path(), ["commit", run.run_id.as_str()])
        .assert()
        .success();
    mark_run_pushed(&repo, &run.run_id, "git@gitlab.com:owner/repo.git");

    run_keel_with_path(
        repo.path(),
        ["pr", run.run_id.as_str(), "--provider", "gitlab", "--json"],
        &path_with_git_only(),
    )
    .assert()
    .failure()
    .stderr(predicate::str::contains(
        "provider-backed PR/MR creation for GitLab is not implemented in v0.5c",
    ));

    let manual = parse_json_object(&run_keel_output(
        repo.path(),
        [
            "pr",
            run.run_id.as_str(),
            "--provider",
            "gitlab",
            "--manual",
            "--dry-run",
            "--json",
        ],
    ));
    assert_eq!(manual["provider"], "gitlab");
    assert_eq!(manual["request_kind"], "merge_request");
    assert_eq!(manual["would_create_request"], false);
    assert!(manual["web_url"]
        .as_str()
        .unwrap()
        .contains("/-/merge_requests/new"));
    assert!(!run_artifact_path(&repo, &run.run_id, "pr.json").is_file());
}

#[test]
fn real_github_pr_smoke_is_opt_in() {
    if std::env::var("KEEL_REAL_GITHUB_PR_SMOKE").ok().as_deref() != Some("1") {
        return;
    }

    run_real_provider_pr_smoke(RealPrSmokeProvider {
        provider: "github",
        cli: "gh",
        auth_args: &["auth", "status"],
        remote_env: "KEEL_REAL_GITHUB_REMOTE",
        target_env: "KEEL_REAL_GITHUB_TARGET",
    });
}

#[test]
fn diff_outputs_saved_patch_and_clear_missing_errors() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();
    let run = run_noop(&repo, "diff workflow task");

    run_keel(repo.path(), ["diff", run.run_id.as_str()])
        .assert()
        .success()
        .stdout(predicate::str::contains(NOOP_OUTPUT_FILE));

    run_keel(repo.path(), ["diff", "run-does-not-exist"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "run `run-does-not-exist` does not exist",
        ));

    fs::remove_file(run_artifact_path(&repo, &run.run_id, "diff.patch")).unwrap();
    run_keel(repo.path(), ["diff", run.run_id.as_str()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("diff for run"));
}

#[test]
fn log_outputs_saved_log_and_clear_missing_or_empty_messages() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();
    let run = run_noop(&repo, "log workflow task");

    run_keel(repo.path(), ["log", run.run_id.as_str()])
        .assert()
        .success()
        .stdout(predicate::str::contains("created run"));

    fs::write(run_artifact_path(&repo, &run.run_id, "log.txt"), "").unwrap();
    run_keel(repo.path(), ["log", run.run_id.as_str()])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "Log for run `{}` is empty.",
            run.run_id
        )));

    run_keel(repo.path(), ["log", "run-does-not-exist"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "run `run-does-not-exist` does not exist",
        ));
}

#[test]
fn discard_preserves_history_and_keeps_report_and_diff_available() {
    let repo = create_temp_git_repo();
    run_keel(repo.path(), ["init"]).assert().success();
    let run = run_noop(&repo, "discard preservation task");

    assert!(worktree_dir(&repo, &run.run_id).is_dir());
    run_keel(repo.path(), ["discard", run.run_id.as_str()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Discarded run:"));

    assert!(!worktree_dir(&repo, &run.run_id).exists());
    assert!(run_dir(&repo, &run.run_id).is_dir());
    for artifact in [
        "metadata.json",
        "log.txt",
        "diff.patch",
        "checks.json",
        "report.md",
    ] {
        assert!(
            run_artifact_path(&repo, &run.run_id, artifact).is_file(),
            "discard removed artifact {artifact}"
        );
    }

    run_keel(repo.path(), ["report", run.run_id.as_str()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Run is already discarded."));
    run_keel(repo.path(), ["diff", run.run_id.as_str()])
        .assert()
        .success()
        .stdout(predicate::str::contains(NOOP_OUTPUT_FILE));
    run_keel(repo.path(), ["log", run.run_id.as_str()])
        .assert()
        .success()
        .stdout(predicate::str::contains("marked discarded"));
}

struct NoopRun {
    run_id: String,
}

struct RealPrSmokeProvider {
    provider: &'static str,
    cli: &'static str,
    auth_args: &'static [&'static str],
    remote_env: &'static str,
    target_env: &'static str,
}

fn run_real_provider_pr_smoke(provider: RealPrSmokeProvider) {
    let remote = std::env::var(provider.remote_env).unwrap_or_else(|_| {
        panic!(
            "{} must be set to a writable test repository remote URL",
            provider.remote_env
        )
    });
    let target = std::env::var(provider.target_env).unwrap_or_else(|_| "main".to_string());

    assert!(
        command_success(provider.cli, provider.auth_args, Path::new(".")),
        "{} is not installed or is not authenticated; run `{}` auth status` first",
        provider.cli,
        provider.cli
    );

    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), ["clone", remote.as_str(), "."]);
    git(
        repo.path(),
        ["config", "user.email", "keel-real-pr@example.local"],
    );
    git(repo.path(), ["config", "user.name", "Keel Real PR Smoke"]);

    run_keel(repo.path(), ["init"]).assert().success();
    let run = run_noop(
        &repo,
        &format!("real {} provider PR smoke", provider.provider),
    );
    run_keel(repo.path(), ["commit", run.run_id.as_str()])
        .assert()
        .success();
    run_keel(repo.path(), ["push", run.run_id.as_str()])
        .assert()
        .success();

    let output = run_keel(
        repo.path(),
        [
            "pr",
            run.run_id.as_str(),
            "--provider",
            provider.provider,
            "--base",
            target.as_str(),
        ],
    )
    .assert()
    .success()
    .get_output()
    .clone();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("http://") || stdout.contains("https://"),
        "provider PR smoke did not print a URL:\n{stdout}"
    );

    let pr_artifact = run_artifact_path(&repo, &run.run_id, "pr.json");
    assert!(pr_artifact.is_file());
    let report = parse_json_object(&run_keel_output(
        repo.path(),
        ["report", run.run_id.as_str(), "--json"],
    ));
    assert_eq!(report["artifacts"]["pr"]["exists"], true);
    assert!(report["pr"]["url"].as_str().unwrap().starts_with("http"));
}

fn create_temp_git_repo() -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), ["init"]);
    git(
        temp.path(),
        ["config", "user.email", "keel-test@example.local"],
    );
    git(temp.path(), ["config", "user.name", "Keel Test"]);
    fs::write(temp.path().join("README.md"), "# test repo\n").unwrap();
    git(temp.path(), ["add", "README.md"]);
    git(temp.path(), ["commit", "-m", "initial commit"]);
    temp
}

fn create_bare_git_repo() -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), ["init", "--bare"]);
    temp
}

fn run_noop(repo: &TempDir, task: &str) -> NoopRun {
    let output = run_keel_output(repo.path(), ["run", task, "--agent", "noop"]);
    let run_id = extract_run_id_from_status_or_runs_dir(repo.path(), &output);
    NoopRun { run_id }
}

fn run_keel<const N: usize>(repo: &Path, args: [&str; N]) -> Command {
    let mut command = Command::cargo_bin("keel").unwrap();
    command.current_dir(repo).args(args);
    command
}

fn run_keel_with_path<const N: usize>(repo: &Path, args: [&str; N], path: &str) -> Command {
    let mut command = run_keel(repo, args);
    command.env("PATH", path_for_test(path));
    command
}

fn run_keel_output<const N: usize>(repo: &Path, args: [&str; N]) -> String {
    let output = run_keel(repo, args).assert().success().get_output().clone();
    String::from_utf8(output.stdout).unwrap()
}

fn run_keel_output_with_path<const N: usize>(
    repo: &Path,
    args: [&str; N],
    path: &str,
    expect_success: bool,
) -> String {
    let assert = run_keel_with_path(repo, args, path).assert();
    let output = if expect_success {
        assert.success().get_output().clone()
    } else {
        assert.failure().get_output().clone()
    };
    String::from_utf8(output.stdout).unwrap()
}

fn extract_run_id_from_status_or_runs_dir(repo: &Path, output: &str) -> String {
    output
        .lines()
        .find_map(|line| line.strip_prefix(RUN_CREATED_PREFIX))
        .map(str::trim)
        .filter(|run_id| !run_id.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| newest_run_id_from_runs_dir(repo))
}

fn newest_run_id_from_runs_dir(repo: &Path) -> String {
    let runs_dir = repo.join(".keel").join("runs");
    let mut entries = fs::read_dir(&runs_dir)
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.path().is_dir())
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.metadata().unwrap().modified().unwrap());
    entries
        .last()
        .and_then(|entry| entry.file_name().into_string().ok())
        .expect("expected at least one run directory")
}

fn read_run_artifact(repo: &TempDir, run_id: &str, artifact: &str) -> String {
    fs::read_to_string(run_artifact_path(repo, run_id, artifact)).unwrap()
}

fn mark_run_pushed(repo: &TempDir, run_id: &str, remote_url: &str) {
    let metadata_path = run_artifact_path(repo, run_id, "metadata.json");
    let mut metadata = parse_json_object(&fs::read_to_string(&metadata_path).unwrap());
    let branch = metadata["branch"].as_str().unwrap().to_string();
    let commit_sha = metadata["commit_sha"].as_str().unwrap().to_string();
    let pushed_at = "2026-04-30T00:00:00Z";
    metadata["pushed"] = Value::Bool(true);
    metadata["pushed_at"] = Value::String(pushed_at.to_string());
    metadata["push_remote"] = Value::String("origin".to_string());
    metadata["push_remote_url"] = Value::String(remote_url.to_string());
    metadata["pushed_branch"] = Value::String(branch.clone());
    metadata["push"] = serde_json::json!({
        "run_id": run_id,
        "remote": "origin",
        "remote_url": remote_url,
        "branch": branch,
        "commit_sha": commit_sha,
        "pushed": true,
        "pushed_at": pushed_at,
        "dry_run": false
    });
    fs::write(
        &metadata_path,
        serde_json::to_string_pretty(&metadata).unwrap(),
    )
    .unwrap();
}

fn parse_json_array(output: &str) -> Vec<Value> {
    serde_json::from_str(output).unwrap()
}

fn parse_json_object(output: &str) -> Value {
    serde_json::from_str(output).unwrap()
}

fn check_status<'a>(report: &'a Value, id: &str) -> &'a str {
    report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == id)
        .and_then(|check| check["status"].as_str())
        .unwrap()
}

fn check_details<'a>(report: &'a Value, id: &str) -> &'a str {
    report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == id)
        .and_then(|check| check["details"].as_str())
        .unwrap()
}

fn check_issue_severity<'a>(report: &'a Value, id: &str) -> &'a str {
    report["issues"]
        .as_array()
        .unwrap()
        .iter()
        .find(|issue| issue["id"] == id)
        .and_then(|issue| issue["severity"].as_str())
        .unwrap()
}

fn run_artifact_path(repo: &TempDir, run_id: &str, artifact: &str) -> PathBuf {
    run_dir(repo, run_id).join(artifact)
}

fn run_dir(repo: &TempDir, run_id: &str) -> PathBuf {
    repo.path().join(".keel").join("runs").join(run_id)
}

fn worktree_dir(repo: &TempDir, run_id: &str) -> PathBuf {
    repo.path().join(".keel").join("worktrees").join(run_id)
}

fn git<const N: usize>(repo: &Path, args: [&str; N]) {
    let output = StdCommand::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git command failed: {}\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_output<const N: usize>(repo: &Path, args: [&str; N]) -> String {
    let output = StdCommand::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git command failed: {}\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn git_file_url(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        format!("file:///{normalized}")
    }
}

fn command_success(program: &str, args: &[&str], cwd: &Path) -> bool {
    StdCommand::new(program)
        .args(args)
        .current_dir(cwd)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn branch_exists(repo: &TempDir, branch: &str) -> bool {
    StdCommand::new("git")
        .args(["show-ref", "--verify", "--quiet"])
        .arg(format!("refs/heads/{branch}"))
        .current_dir(repo.path())
        .status()
        .unwrap()
        .success()
}

fn path_with_git_only() -> String {
    git_executable()
        .parent()
        .unwrap()
        .to_string_lossy()
        .to_string()
}

fn path_with_git_and(extra_dir: &Path) -> String {
    std::env::join_paths([git_executable().parent().unwrap(), extra_dir])
        .unwrap()
        .to_string_lossy()
        .to_string()
}

fn path_for_test(path: &str) -> String {
    path.to_string()
}

fn env_echo_command(key: &str) -> String {
    if cfg!(windows) {
        format!("echo %{key}%")
    } else {
        format!("printf '%s\\n' \"${key}\"")
    }
}

fn git_executable() -> PathBuf {
    let output = StdCommand::new("where")
        .arg("git")
        .output()
        .or_else(|_| StdCommand::new("which").arg("git").output())
        .unwrap();
    assert!(
        output.status.success(),
        "failed to locate git executable\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let first = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap()
        .trim()
        .to_string();
    PathBuf::from(first)
}

fn create_fake_executable(dir: &Path, name: &str) {
    let path = dir.join(if cfg!(windows) {
        format!("{name}.cmd")
    } else {
        name.to_string()
    });
    let content = if cfg!(windows) {
        "@echo off\r\nexit /B 0\r\n"
    } else {
        "#!/bin/sh\nexit 0\n"
    };
    fs::write(&path, content).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
    }
}

fn create_fake_provider_cli(dir: &Path, name: &str, url: &str) {
    let path = dir.join(if cfg!(windows) {
        format!("{name}.cmd")
    } else {
        name.to_string()
    });
    let content = if cfg!(windows) {
        format!(
            "@echo off\r\necho %* >> \"{}\"\r\nif \"%1 %2\"==\"pr list\" (\r\n  echo []\r\n  exit /B 0\r\n)\r\necho {}\r\nexit /B 0\r\n",
            dir.join(format!("{name}-args.txt")).display(),
            url
        )
    } else {
        format!(
            "#!/bin/sh\necho \"$@\" >> '{}'\nif [ \"$1 $2\" = \"pr list\" ]; then\n  echo '[]'\n  exit 0\nfi\necho '{}'\nexit 0\n",
            dir.join(format!("{name}-args.txt")).display(),
            url
        )
    };
    fs::write(&path, content).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
    }
}

fn create_fake_provider_cli_with_existing_pr(
    dir: &Path,
    name: &str,
    url: &str,
    title: &str,
    draft: bool,
) {
    let path = dir.join(if cfg!(windows) {
        format!("{name}.cmd")
    } else {
        name.to_string()
    });
    let calls = dir.join(format!("{name}-calls.txt"));
    let list_json = serde_json::json!([{
        "url": url,
        "title": title,
        "isDraft": draft,
    }])
    .to_string();
    let content = if cfg!(windows) {
        format!(
            "@echo off\r\necho %* >> \"{}\"\r\nif \"%1 %2\"==\"pr list\" (\r\n  echo {}\r\n  exit /B 0\r\n)\r\necho should-not-create\r\nexit /B 0\r\n",
            calls.display(),
            list_json
        )
    } else {
        let list_json = serde_json::json!([{
            "url": url,
            "title": title,
            "isDraft": draft,
        }])
        .to_string();
        format!(
            "#!/bin/sh\necho \"$@\" >> '{}'\nif [ \"$1 $2\" = \"pr list\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\necho should-not-create\nexit 0\n",
            calls.display(),
            list_json.replace('\'', "'\\''")
        )
    };
    fs::write(&path, content).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
    }
}

fn create_fake_provider_cli_failure(dir: &Path, name: &str, stderr: &str) {
    let path = dir.join(if cfg!(windows) {
        format!("{name}.cmd")
    } else {
        name.to_string()
    });
    let content = if cfg!(windows) {
        format!(
            "@echo off\r\necho {} 1>&2\r\nexit /B 1\r\n",
            stderr.replace('"', "")
        )
    } else {
        format!(
            "#!/bin/sh\nprintf '%s\\n' '{}' >&2\nexit 1\n",
            stderr.replace('\'', "'\\''")
        )
    };
    fs::write(&path, content).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
    }
}

fn assert_contains_in_order(output: &str, first: &str, second: &str) {
    let first_index = output
        .find(first)
        .unwrap_or_else(|| panic!("output did not contain {first}:\n{output}"));
    let second_index = output
        .find(second)
        .unwrap_or_else(|| panic!("output did not contain {second}:\n{output}"));
    assert!(
        first_index < second_index,
        "expected {first} before {second} in output:\n{output}"
    );
}

fn normalize_output(output: &str) -> String {
    let temp_root = std::env::temp_dir().to_string_lossy().replace('\\', "/");
    let normalized = output
        .replace('\\', "/")
        .lines()
        .map(|line| {
            line.split_whitespace()
                .map(|token| normalize_token(token, &temp_root))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n");
    redact_long_numbers(&redact_run_ids(&normalized))
}

fn normalize_token(token: &str, temp_root: &str) -> String {
    if token.replace('\\', "/").contains("/.keel/")
        || token.replace('\\', "/").starts_with(temp_root)
    {
        return normalize_path_token(token);
    }
    token.to_string()
}

fn normalize_path_token(token: &str) -> String {
    let normalized = token.replace('\\', "/");
    match normalized.find("/.keel/") {
        Some(index) => format!("<repo>{}", &normalized[index..]),
        None => "<path>".to_string(),
    }
}

fn redact_run_ids(input: &str) -> String {
    let mut output = String::new();
    let mut rest = input;
    while let Some(index) = rest.find("run-") {
        output.push_str(&rest[..index]);
        output.push_str("<run-id>");
        let after_prefix = &rest[index + "run-".len()..];
        let consumed = after_prefix
            .char_indices()
            .find_map(|(offset, ch)| {
                if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                    None
                } else {
                    Some(offset)
                }
            })
            .unwrap_or(after_prefix.len());
        rest = &after_prefix[consumed..];
    }
    output.push_str(rest);
    output
}

fn redact_long_numbers(input: &str) -> String {
    let mut output = String::new();
    let mut digits = String::new();
    for ch in input.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            continue;
        }
        push_redacted_digits(&mut output, &mut digits);
        output.push(ch);
    }
    push_redacted_digits(&mut output, &mut digits);
    output
}

fn push_redacted_digits(output: &mut String, digits: &mut String) {
    if digits.is_empty() {
        return;
    }
    if digits.len() >= 10 {
        output.push_str("<timestamp>");
    } else {
        output.push_str(digits);
    }
    digits.clear();
}
