use crate::agents::{AgentAdapter, AgentExecution, AgentRunContext, ClaudeAgent, CodexAgent};
use crate::command::resolve_windows_program_from_path;
use crate::constants::{
    CHECKS_FILE, CONFIG_FILE, DEFAULT_AGENT_TIMEOUT_SECS, DIFF_FILE, KEEL_DIR, LOG_FILE,
    METADATA_FILE, NOOP_OUTPUT_FILE, REPORT_FILE, RUNS_DIR, WORKTREES_DIR,
};
use crate::json::read_json;
use crate::model::{CheckResult, CheckStatus, FailureReason, RunMetadata, RunStatus};
use crate::project::KeelProject;
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
fn rerun_rejects_unsupported_source_agent_without_appending_report() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let mut source = project.run("unsupported source agent", "noop").unwrap();
    source.agent = "opencode".to_string();
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

    let error = project.run("task", "opencode").unwrap_err().to_string();

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
