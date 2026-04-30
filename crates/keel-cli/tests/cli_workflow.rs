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

fn run_keel_output<const N: usize>(repo: &Path, args: [&str; N]) -> String {
    let output = run_keel(repo, args).assert().success().get_output().clone();
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

fn parse_json_array(output: &str) -> Vec<Value> {
    serde_json::from_str(output).unwrap()
}

fn parse_json_object(output: &str) -> Value {
    serde_json::from_str(output).unwrap()
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
