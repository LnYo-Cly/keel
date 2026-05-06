use super::*;

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

    let report = fs::read_to_string(run_dir.join(artifact_files::REPORT)).unwrap();
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

    let log = fs::read_to_string(run_dir.join(artifact_files::LOG)).unwrap();
    assert!(log.contains("fake codex stdout"));
    assert!(log.contains("fake codex stderr"));
    assert!(log.contains("--ask-for-approval on-request"));
    assert!(!log.contains("--full-auto"));
    assert!(!log.contains("--dangerously-bypass-approvals-and-sandbox"));

    let diff = fs::read_to_string(run_dir.join(artifact_files::DIFF)).unwrap();
    assert!(diff.contains("codex-output.txt"));
}

#[cfg(windows)]
#[test]
fn codex_adapter_runs_powershell_shim_with_timeout_wrapper() {
    let temp = git_repo();
    let fake_codex = fake_codex(temp.path(), FakeCodexMode::PowerShellSuccess);
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let metadata = project
        .run_with_adapter(
            "make a codex ps1 change",
            &CodexAgent::with_program(fake_codex.to_string_lossy()),
        )
        .unwrap();

    assert_eq!(metadata.agent, "codex");
    assert_eq!(metadata.status, RunStatus::Ready);
    let run_dir = run_dir(&temp, &metadata.run_id);
    assert_required_artifacts(&run_dir);

    let log = fs::read_to_string(run_dir.join(artifact_files::LOG)).unwrap();
    assert!(log.contains("fake codex ps1 stdout"));
    assert!(log.contains("fake codex ps1 stderr"));
    assert!(log.contains("--ask-for-approval on-request"));

    let diff = fs::read_to_string(run_dir.join(artifact_files::DIFF)).unwrap();
    assert!(diff.contains("codex-ps1-output.txt"));

    let args_path = worktree_dir(&temp, &metadata.run_id).join("codex-ps1-args.txt");
    let args = fs::read_to_string(args_path).unwrap();
    assert!(args.contains("--sandbox workspace-write"));
    assert!(args.contains("make a codex ps1 change"));
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
    let log = fs::read_to_string(run_dir.join(artifact_files::LOG)).unwrap();
    assert!(log.contains("fake codex failure"));
    let report = fs::read_to_string(run_dir.join(artifact_files::REPORT)).unwrap();
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
    let report = fs::read_to_string(run_dir.join(artifact_files::REPORT)).unwrap();
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

    let log = fs::read_to_string(run_dir.join(artifact_files::LOG)).unwrap();
    assert!(log.contains("fake claude stdout"));
    assert!(log.contains("fake claude stderr"));
    assert!(log.contains("--permission-mode acceptEdits"));
    assert!(!log.contains("--dangerously-skip-permissions"));
    assert!(!log.contains("bypassPermissions"));

    let diff = fs::read_to_string(run_dir.join(artifact_files::DIFF)).unwrap();
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
    let log = fs::read_to_string(run_dir.join(artifact_files::LOG)).unwrap();
    assert!(log.contains("fake claude failure"));
    let report = fs::read_to_string(run_dir.join(artifact_files::REPORT)).unwrap();
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
    let report = fs::read_to_string(run_dir.join(artifact_files::REPORT)).unwrap();
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

    let log = fs::read_to_string(run_dir.join(artifact_files::LOG)).unwrap();
    assert!(log.contains("fake opencode stdout"));
    assert!(log.contains("fake opencode stderr"));
    assert!(log.contains("run --dir"));
    assert!(!log.contains("--dangerously-skip-permissions"));

    let diff = fs::read_to_string(run_dir.join(artifact_files::DIFF)).unwrap();
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
    let log = fs::read_to_string(run_dir.join(artifact_files::LOG)).unwrap();
    assert!(log.contains("fake opencode failure"));
    let report = fs::read_to_string(run_dir.join(artifact_files::REPORT)).unwrap();
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
    let report = fs::read_to_string(run_dir.join(artifact_files::REPORT)).unwrap();
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
    let report = fs::read_to_string(run_dir.join(artifact_files::REPORT)).unwrap();
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
    let report = fs::read_to_string(run_dir.join(artifact_files::REPORT)).unwrap();
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
    let diff = read_run_file(&temp, &metadata.run_id, artifact_files::DIFF);
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
        assert!(read_run_file(&temp, run_id, artifact_files::DIFF)
            .contains("codex-real-rerun-smoke.txt"));
        assert!(project
            .report(run_id)
            .unwrap()
            .summary
            .contains("agent=codex"));
    }

    assert!(read_run_file(&temp, &source.run_id, artifact_files::REPORT)
        .contains(&format!("Created rerun: `{}`", child.run_id)));

    let discarded_source = project.discard(&source.run_id).unwrap();
    let discarded_child = project.discard(&child.run_id).unwrap();
    assert_eq!(discarded_source.status, RunStatus::Discarded);
    assert_eq!(discarded_child.status, RunStatus::Discarded);
    assert!(!worktree_dir(&temp, &source.run_id).exists());
    assert!(!worktree_dir(&temp, &child.run_id).exists());
    assert!(run_dir(&temp, &source.run_id)
        .join(artifact_files::REPORT)
        .exists());
    assert!(run_dir(&temp, &child.run_id)
        .join(artifact_files::REPORT)
        .exists());
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
