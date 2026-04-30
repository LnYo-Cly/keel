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
use crate::json::{read_json, report_json, status_json};
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

mod agents;
mod commit;
mod config;
mod pr;
mod project;
mod push;
mod rerun_checks_risk;
mod review;
mod run_lifecycle;

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
