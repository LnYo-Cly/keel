use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::{SystemTime, UNIX_EPOCH};

const KEEL_DIR: &str = ".keel";
const RUNS_DIR: &str = "runs";
const WORKTREES_DIR: &str = "worktrees";
const CONFIG_FILE: &str = "config.toml";
const METADATA_FILE: &str = "metadata.json";
const LOG_FILE: &str = "log.txt";
const DIFF_FILE: &str = "diff.patch";
const CHECKS_FILE: &str = "checks.json";
const REPORT_FILE: &str = "report.md";
const NOOP_OUTPUT_FILE: &str = "keel-noop-output.txt";

#[derive(Debug, Clone)]
pub struct KeelProject {
    root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct InitResult {
    pub root: PathBuf,
    pub keel_dir: PathBuf,
    pub config_path: PathBuf,
    pub runs_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ReportInfo {
    pub path: PathBuf,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Created,
    Running,
    Ready,
    NotReady,
    Discarded,
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Created => "created",
            Self::Running => "running",
            Self::Ready => "ready",
            Self::NotReady => "not_ready",
            Self::Discarded => "discarded",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMetadata {
    pub run_id: String,
    pub task: String,
    pub agent: String,
    pub status: RunStatus,
    pub created_at: String,
    pub updated_at: String,
    pub worktree_path: String,
    pub run_dir: String,
    pub branch: String,
    pub base_commit: String,
    pub exit_code: Option<i32>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Passed,
    Failed,
    Skipped,
}

impl std::fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub name: String,
    pub command: String,
    pub status: CheckStatus,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Deserialize)]
struct KeelConfig {
    #[serde(default = "default_checks")]
    checks: Vec<ConfiguredCheck>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConfiguredCheck {
    name: String,
    command: Vec<String>,
    #[serde(default)]
    run_if_path_exists: Option<String>,
}

#[derive(Debug)]
struct CommandCapture {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Default)]
struct RunLog {
    lines: Vec<String>,
}

impl RunLog {
    fn push(&mut self, message: impl AsRef<str>) {
        self.lines
            .push(format!("[{}] {}", now_timestamp(), message.as_ref()));
    }

    fn push_command(&mut self, cwd: &Path, command: &str, capture: &CommandCapture) {
        self.push(format!("$ ({}) {}", cwd.display(), command));
        self.push(format!("exit: {}", exit_code_text(capture.status.code())));
        if !capture.stdout.trim().is_empty() {
            self.push(format!("stdout:\n{}", capture.stdout.trim_end()));
        }
        if !capture.stderr.trim().is_empty() {
            self.push(format!("stderr:\n{}", capture.stderr.trim_end()));
        }
    }

    fn write_to(&self, path: &Path) -> Result<()> {
        fs::write(path, self.lines.join("\n") + "\n")
            .with_context(|| format!("failed to write log {}", path.display()))
    }
}

pub(crate) trait AgentAdapter {
    fn name(&self) -> &'static str;
    fn run(&self, context: &AgentRunContext<'_>) -> Result<AgentExecution>;

    fn requires_non_empty_diff(&self) -> bool {
        false
    }
}

pub struct AgentRunContext<'a> {
    pub run_id: &'a str,
    pub task: &'a str,
    pub worktree: &'a Path,
}

#[derive(Debug, Clone)]
pub struct AgentExecution {
    pub command: Vec<String>,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl AgentExecution {
    fn command_line(&self) -> String {
        self.command.join(" ")
    }
}

struct NoopAgent;

impl AgentAdapter for NoopAgent {
    fn name(&self) -> &'static str {
        "noop"
    }

    fn run(&self, context: &AgentRunContext<'_>) -> Result<AgentExecution> {
        let output = format!(
            "Keel noop agent output\n\nrun_id = {}\ntask = {}\n",
            context.run_id, context.task
        );
        fs::write(context.worktree.join(NOOP_OUTPUT_FILE), output).with_context(|| {
            format!(
                "failed to write noop output in {}",
                context.worktree.display()
            )
        })?;
        Ok(AgentExecution {
            command: vec!["noop".to_string()],
            exit_code: 0,
            stdout: format!("wrote {NOOP_OUTPUT_FILE}\n"),
            stderr: String::new(),
        })
    }

    fn requires_non_empty_diff(&self) -> bool {
        true
    }
}

struct CodexAgent {
    program: String,
}

impl CodexAgent {
    fn new() -> Self {
        Self {
            program: "codex".to_string(),
        }
    }

    #[cfg(test)]
    fn with_program(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
        }
    }

    fn command(&self, context: &AgentRunContext<'_>) -> Vec<String> {
        vec![
            self.program.clone(),
            "exec".to_string(),
            "--cd".to_string(),
            context.worktree.to_string_lossy().to_string(),
            "--sandbox".to_string(),
            "workspace-write".to_string(),
            "--ask-for-approval".to_string(),
            "on-request".to_string(),
            context.task.to_string(),
        ]
    }
}

impl AgentAdapter for CodexAgent {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn run(&self, context: &AgentRunContext<'_>) -> Result<AgentExecution> {
        let command = self.command(context);
        let capture = run_command(context.worktree, &self.program, &command[1..])?;
        Ok(AgentExecution {
            command,
            exit_code: capture.status.code().unwrap_or(1),
            stdout: capture.stdout,
            stderr: capture.stderr,
        })
    }
}

#[derive(Debug)]
struct RunSession {
    run_id: String,
    run_dir: PathBuf,
    metadata: RunMetadata,
    log: RunLog,
    checks: Vec<CheckResult>,
    diff: Option<String>,
    failure: Option<String>,
}

impl RunSession {
    fn start(project: &KeelProject, task: &str, agent: &str) -> Result<Self> {
        let run_id = generate_run_id();
        let run_dir = project.run_dir(&run_id);
        fs::create_dir_all(&run_dir)
            .with_context(|| format!("failed to create run directory {}", run_dir.display()))?;

        let created_at = now_timestamp();
        let metadata = RunMetadata {
            run_id: run_id.clone(),
            task: task.to_string(),
            agent: agent.to_string(),
            status: RunStatus::Created,
            created_at: created_at.clone(),
            updated_at: created_at,
            worktree_path: format!("{KEEL_DIR}/{WORKTREES_DIR}/{run_id}"),
            run_dir: format!("{KEEL_DIR}/{RUNS_DIR}/{run_id}"),
            branch: format!("keel/run/{run_id}"),
            base_commit: String::new(),
            exit_code: None,
            warnings: Vec::new(),
        };
        let session = Self {
            run_id,
            run_dir,
            metadata,
            log: RunLog::default(),
            checks: Vec::new(),
            diff: None,
            failure: None,
        };
        session.persist_metadata()?;
        Ok(session)
    }

    fn persist_metadata(&self) -> Result<()> {
        write_json_pretty(&self.run_dir.join(METADATA_FILE), &self.metadata)
    }

    fn persist_checks(&self) -> Result<()> {
        write_json_pretty(&self.run_dir.join(CHECKS_FILE), &self.checks)
    }

    fn persist_diff(&self) -> Result<()> {
        fs::write(
            self.run_dir.join(DIFF_FILE),
            self.diff.as_deref().unwrap_or(""),
        )
        .with_context(|| format!("failed to write {}", self.run_dir.join(DIFF_FILE).display()))
    }

    fn persist_report(&self) -> Result<()> {
        fs::write(
            self.run_dir.join(REPORT_FILE),
            render_report(
                &self.metadata,
                &self.checks,
                self.diff.as_deref().unwrap_or(""),
                self.failure.as_deref(),
            ),
        )
        .with_context(|| {
            format!(
                "failed to write {}",
                self.run_dir.join(REPORT_FILE).display()
            )
        })
    }

    fn persist_log(&self) -> Result<()> {
        self.log.write_to(&self.run_dir.join(LOG_FILE))
    }

    fn finalize_success(&mut self) -> Result<()> {
        self.persist_metadata()?;
        self.persist_checks()?;
        self.persist_diff()?;
        self.persist_report()?;
        self.log.push("report generated");
        self.persist_log()
    }

    fn finalize_failure(&mut self, error: &anyhow::Error) -> Result<()> {
        self.failure = Some(error.to_string());
        self.metadata.status = RunStatus::NotReady;
        self.metadata.updated_at = now_timestamp();
        self.log.push(format!("run failed: {error}"));
        self.persist_metadata()?;
        self.persist_checks()?;
        self.persist_diff()?;
        self.persist_report()?;
        self.persist_log()
    }
}

impl KeelProject {
    pub fn discover_from_current_dir() -> Result<Self> {
        Self::discover(std::env::current_dir().context("failed to read current directory")?)
    }

    pub fn discover(start: impl AsRef<Path>) -> Result<Self> {
        let start = start.as_ref();
        let output = Command::new("git")
            .arg("rev-parse")
            .arg("--show-toplevel")
            .current_dir(start)
            .output()
            .with_context(|| "failed to execute git rev-parse --show-toplevel")?;

        if !output.status.success() {
            bail!("Keel must be run inside a git repository. Run `git init` first.");
        }

        let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if root.is_empty() {
            bail!("git did not return a repository root");
        }

        Ok(Self {
            root: PathBuf::from(root),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn init(&self) -> Result<InitResult> {
        self.ensure_git_repo()?;

        let keel_dir = self.keel_dir();
        let runs_dir = self.runs_dir();
        let worktrees_dir = self.worktrees_dir();
        fs::create_dir_all(&runs_dir)
            .with_context(|| format!("failed to create {}", runs_dir.display()))?;
        fs::create_dir_all(&worktrees_dir)
            .with_context(|| format!("failed to create {}", worktrees_dir.display()))?;

        let config_path = keel_dir.join(CONFIG_FILE);
        if !config_path.exists() {
            fs::write(&config_path, default_config_toml())
                .with_context(|| format!("failed to write {}", config_path.display()))?;
        }

        Ok(InitResult {
            root: self.root.clone(),
            keel_dir,
            config_path,
            runs_dir,
        })
    }

    pub fn run(&self, task: &str, agent: &str) -> Result<RunMetadata> {
        self.ensure_initialized()?;
        match agent {
            "noop" => self.run_with_adapter(task, &NoopAgent),
            "codex" => self.run_with_adapter(task, &CodexAgent::new()),
            other => bail!("unsupported agent `{other}`; supported agents: noop, codex"),
        }
    }

    fn run_with_adapter(&self, task: &str, adapter: &dyn AgentAdapter) -> Result<RunMetadata> {
        let mut session = RunSession::start(self, task, adapter.name())?;
        session.log.push(format!("created run {}", session.run_id));
        session.log.push(format!("task: {task}"));
        session.log.push(format!("agent: {}", adapter.name()));

        let result = self.execute_run(&mut session, adapter);
        match result {
            Ok(()) => {
                session.finalize_success()?;
                Ok(session.metadata.clone())
            }
            Err(error) => {
                session.finalize_failure(&error)?;
                Err(error)
            }
        }
    }

    fn execute_run(&self, session: &mut RunSession, adapter: &dyn AgentAdapter) -> Result<()> {
        let base_commit = self.git_stdout(&["rev-parse", "HEAD"]).with_context(|| {
            "failed to resolve HEAD; `keel run` requires a git repository with at least one commit"
        })?;
        session.metadata.base_commit = base_commit.clone();
        session.metadata.status = RunStatus::Running;
        session.metadata.updated_at = now_timestamp();
        session.persist_metadata()?;

        let worktree = self.worktree_dir(&session.run_id);
        ensure_safe_run_id(&session.run_id)?;
        ensure_safe_worktree_target(&self.root, &session.run_id, &worktree)?;

        let add_args = vec![
            "worktree".to_string(),
            "add".to_string(),
            "-b".to_string(),
            session.metadata.branch.clone(),
            worktree.to_string_lossy().to_string(),
            base_commit,
        ];
        let add_capture = run_command(&self.root, "git", &add_args)?;
        session
            .log
            .push_command(&self.root, &format_command("git", &add_args), &add_capture);
        if !add_capture.status.success() {
            bail!(
                "failed to create git worktree {}\n{}",
                worktree.display(),
                add_capture.stderr.trim()
            );
        }

        session
            .log
            .push(format!("running agent adapter `{}`", adapter.name()));
        let execution = adapter.run(&AgentRunContext {
            run_id: &session.run_id,
            task: &session.metadata.task,
            worktree: &worktree,
        })?;
        session
            .log
            .push(format!("agent command: {}", execution.command_line()));
        session
            .log
            .push(format!("agent exit code: {}", execution.exit_code));
        if !execution.stdout.trim().is_empty() {
            session
                .log
                .push(format!("agent stdout:\n{}", execution.stdout.trim_end()));
        }
        if !execution.stderr.trim().is_empty() {
            session
                .log
                .push(format!("agent stderr:\n{}", execution.stderr.trim_end()));
        }
        let exit_code = execution.exit_code;
        session.metadata.exit_code = Some(exit_code);

        prepare_untracked_for_diff(&worktree, &mut session.log)?;

        let diff = self.capture_diff(&worktree, &mut session.log)?;
        if adapter.requires_non_empty_diff() && diff.trim().is_empty() {
            bail!("noop run produced an empty diff; refusing to mark candidate ready");
        }
        session.diff = Some(diff);

        let changed_paths = changed_paths(&worktree)?;
        let config = self.read_config()?;
        session.checks = run_checks(&worktree, &config.checks, &mut session.log)?;

        session.metadata.warnings = warnings_for_paths(&changed_paths);
        session.metadata.status = classify_run(exit_code, &session.checks);
        session.metadata.updated_at = now_timestamp();
        Ok(())
    }

    pub fn list_runs(&self) -> Result<Vec<RunMetadata>> {
        self.ensure_initialized()?;
        let mut runs = Vec::new();
        for entry in fs::read_dir(self.runs_dir())
            .with_context(|| format!("failed to read {}", self.runs_dir().display()))?
        {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let metadata_path = entry.path().join(METADATA_FILE);
            if metadata_path.exists() {
                runs.push(read_json(&metadata_path)?);
            }
        }
        runs.sort_by(|left: &RunMetadata, right: &RunMetadata| {
            left.created_at
                .cmp(&right.created_at)
                .then(left.run_id.cmp(&right.run_id))
        });
        Ok(runs)
    }

    pub fn report(&self, run_id: &str) -> Result<ReportInfo> {
        ensure_safe_run_id(run_id)?;
        self.ensure_initialized()?;
        let report_path = self.run_dir(run_id).join(REPORT_FILE);
        if !report_path.exists() {
            bail!(
                "report for run `{run_id}` does not exist at {}",
                report_path.display()
            );
        }

        let metadata = self.read_metadata(run_id)?;
        let summary = format!(
            "run_id={} task={:?} agent={} status={} created_at={} worktree={}",
            metadata.run_id,
            metadata.task,
            metadata.agent,
            metadata.status,
            metadata.created_at,
            metadata.worktree_path
        );
        Ok(ReportInfo {
            path: report_path,
            summary,
        })
    }

    pub fn discard(&self, run_id: &str) -> Result<RunMetadata> {
        ensure_safe_run_id(run_id)?;
        self.ensure_initialized()?;

        let mut metadata = self.read_metadata(run_id)?;
        let run_dir = self.run_dir(run_id);
        let mut log = RunLog::default();
        let log_path = run_dir.join(LOG_FILE);
        if log_path.exists() {
            let existing = fs::read_to_string(&log_path)
                .with_context(|| format!("failed to read {}", log_path.display()))?;
            log.lines.extend(existing.lines().map(str::to_owned));
        }

        let worktree = self.worktree_dir(run_id);
        ensure_safe_worktree_target(&self.root, run_id, &worktree)?;
        let worktree_removed = if worktree.exists() {
            let remove_args = vec![
                "worktree".to_string(),
                "remove".to_string(),
                "--force".to_string(),
                worktree.to_string_lossy().to_string(),
            ];
            let remove_capture = run_command(&self.root, "git", &remove_args)?;
            log.push_command(
                &self.root,
                &format_command("git", &remove_args),
                &remove_capture,
            );
            if !remove_capture.status.success() {
                bail!(
                    "failed to remove worktree {}\n{}",
                    worktree.display(),
                    remove_capture.stderr.trim()
                );
            }
            true
        } else {
            log.push(format!(
                "worktree {} already absent; marking discarded",
                worktree.display()
            ));
            false
        };

        metadata.status = RunStatus::Discarded;
        metadata.updated_at = now_timestamp();
        self.write_metadata(&metadata)?;

        let report_path = run_dir.join(REPORT_FILE);
        let report = match fs::read_to_string(&report_path) {
            Ok(existing_report) => format!(
                "{existing_report}\n\n## Discard\n\n- Status: `discarded`\n- Worktree removed: `{}`\n- Run history preserved at: `{}`\n",
                if worktree_removed { "yes" } else { "already absent" },
                metadata.run_dir
            ),
            Err(_) => render_report(
                &metadata,
                &[],
                "",
                Some("prior report was missing during discard; run history may be incomplete"),
            ),
        };
        fs::write(&report_path, report)
            .with_context(|| format!("failed to update {}", report_path.display()))?;
        log.push(format!("run {run_id} marked discarded"));
        log.write_to(&log_path)?;

        Ok(metadata)
    }

    fn ensure_git_repo(&self) -> Result<()> {
        let is_inside = self.git_stdout(&["rev-parse", "--is-inside-work-tree"])?;
        if is_inside.trim() != "true" {
            bail!("Keel must be run inside a git work tree");
        }
        Ok(())
    }

    fn ensure_initialized(&self) -> Result<()> {
        self.ensure_git_repo()?;
        let config = self.keel_dir().join(CONFIG_FILE);
        if !config.exists() {
            bail!(
                "Keel is not initialized in {}. Run `keel init` first.",
                self.root.display()
            );
        }
        fs::create_dir_all(self.runs_dir())
            .with_context(|| format!("failed to create {}", self.runs_dir().display()))?;
        fs::create_dir_all(self.worktrees_dir())
            .with_context(|| format!("failed to create {}", self.worktrees_dir().display()))?;
        Ok(())
    }

    fn capture_diff(&self, worktree: &Path, log: &mut RunLog) -> Result<String> {
        let diff_args = vec!["diff".to_string(), "--no-ext-diff".to_string()];
        let diff_capture = run_command(worktree, "git", &diff_args)?;
        log.push_command(worktree, &format_command("git", &diff_args), &diff_capture);
        if !diff_capture.status.success() {
            bail!("failed to capture diff\n{}", diff_capture.stderr.trim());
        }
        Ok(diff_capture.stdout)
    }

    fn git_stdout(&self, args: &[&str]) -> Result<String> {
        let args = args
            .iter()
            .map(|arg| (*arg).to_string())
            .collect::<Vec<_>>();
        let capture = run_command(&self.root, "git", &args)?;
        if !capture.status.success() {
            bail!("{}", capture.stderr.trim());
        }
        Ok(capture.stdout.trim().to_string())
    }

    fn keel_dir(&self) -> PathBuf {
        self.root.join(KEEL_DIR)
    }

    fn runs_dir(&self) -> PathBuf {
        self.keel_dir().join(RUNS_DIR)
    }

    fn worktrees_dir(&self) -> PathBuf {
        self.keel_dir().join(WORKTREES_DIR)
    }

    fn run_dir(&self, run_id: &str) -> PathBuf {
        self.runs_dir().join(run_id)
    }

    fn worktree_dir(&self, run_id: &str) -> PathBuf {
        self.worktrees_dir().join(run_id)
    }

    fn read_metadata(&self, run_id: &str) -> Result<RunMetadata> {
        read_json(&self.run_dir(run_id).join(METADATA_FILE))
    }

    fn write_metadata(&self, metadata: &RunMetadata) -> Result<()> {
        write_json_pretty(
            &self.run_dir(&metadata.run_id).join(METADATA_FILE),
            metadata,
        )
    }

    fn read_config(&self) -> Result<KeelConfig> {
        let path = self.keel_dir().join(CONFIG_FILE);
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let mut config = toml::from_str::<KeelConfig>(&content)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        if config.checks.is_empty() {
            config.checks = default_checks();
        }
        Ok(config)
    }
}

fn default_config_toml() -> &'static str {
    r#"version = 1
runs_dir = "runs"
worktrees_dir = "worktrees"

[[checks]]
name = "git status"
command = ["git", "status", "--short"]

[[checks]]
name = "cargo test"
command = ["cargo", "test"]
run_if_path_exists = "Cargo.toml"
"#
}

fn default_checks() -> Vec<ConfiguredCheck> {
    vec![
        ConfiguredCheck {
            name: "git status".to_string(),
            command: vec![
                "git".to_string(),
                "status".to_string(),
                "--short".to_string(),
            ],
            run_if_path_exists: None,
        },
        ConfiguredCheck {
            name: "cargo test".to_string(),
            command: vec!["cargo".to_string(), "test".to_string()],
            run_if_path_exists: Some("Cargo.toml".to_string()),
        },
    ]
}

fn prepare_untracked_for_diff(worktree: &Path, log: &mut RunLog) -> Result<()> {
    let noop_path = worktree.join(NOOP_OUTPUT_FILE);
    if noop_path.exists() {
        intent_to_add(worktree, &[NOOP_OUTPUT_FILE.to_string()], true, log)
            .context("failed to add noop output to candidate diff")?;
    }

    let ls_args = vec![
        "ls-files".to_string(),
        "--others".to_string(),
        "--exclude-standard".to_string(),
        "-z".to_string(),
    ];
    let ls_capture = run_command(worktree, "git", &ls_args)?;
    log.push_command(worktree, &format_command("git", &ls_args), &ls_capture);
    if !ls_capture.status.success() {
        bail!(
            "failed to list untracked files for diff\n{}",
            ls_capture.stderr.trim()
        );
    }

    let paths = ls_capture
        .stdout
        .split('\0')
        .filter(|path| !path.is_empty() && *path != NOOP_OUTPUT_FILE)
        .map(str::to_string)
        .collect::<Vec<_>>();
    if !paths.is_empty() {
        intent_to_add(worktree, &paths, false, log)?;
    }
    Ok(())
}

fn intent_to_add(worktree: &Path, paths: &[String], force: bool, log: &mut RunLog) -> Result<()> {
    let mut args = vec!["add".to_string(), "--intent-to-add".to_string()];
    if force {
        args.push("--force".to_string());
    }
    args.push("--".to_string());
    args.extend(paths.iter().cloned());

    let capture = run_command(worktree, "git", &args)?;
    log.push_command(worktree, &format_command("git", &args), &capture);
    if !capture.status.success() {
        bail!("{}", capture.stderr.trim());
    }
    Ok(())
}

fn run_checks(
    worktree: &Path,
    configured_checks: &[ConfiguredCheck],
    log: &mut RunLog,
) -> Result<Vec<CheckResult>> {
    let mut checks = Vec::new();

    for check in configured_checks {
        let Some((program, args)) = check.command.split_first() else {
            bail!("configured check `{}` has an empty command", check.name);
        };
        let command = format_command(program, args);

        if let Some(required_path) = &check.run_if_path_exists {
            if !worktree.join(required_path).exists() {
                let message = format!("skipped: {required_path} not found in candidate worktree");
                checks.push(CheckResult {
                    name: check.name.clone(),
                    command,
                    status: CheckStatus::Skipped,
                    exit_code: None,
                    stdout: message.clone(),
                    stderr: String::new(),
                });
                log.push(format!("skipped {}: {message}", check.name));
                continue;
            }
        }

        let capture = run_command(worktree, program, args)?;
        log.push_command(worktree, &command, &capture);
        checks.push(check_from_capture(&check.name, &command, capture));
    }

    Ok(checks)
}

fn check_from_capture(name: &str, command: &str, capture: CommandCapture) -> CheckResult {
    CheckResult {
        name: name.to_string(),
        command: command.to_string(),
        status: if capture.status.success() {
            CheckStatus::Passed
        } else {
            CheckStatus::Failed
        },
        exit_code: capture.status.code(),
        stdout: capture.stdout,
        stderr: capture.stderr,
    }
}

fn classify_run(exit_code: i32, checks: &[CheckResult]) -> RunStatus {
    if exit_code != 0 {
        return RunStatus::NotReady;
    }
    if checks
        .iter()
        .any(|check| matches!(check.status, CheckStatus::Failed))
    {
        return RunStatus::NotReady;
    }
    RunStatus::Ready
}

fn changed_paths(worktree: &Path) -> Result<Vec<String>> {
    let args = vec!["status".to_string(), "--short".to_string()];
    let capture = run_command(worktree, "git", &args)?;
    if !capture.status.success() {
        bail!("failed to inspect changed paths\n{}", capture.stderr.trim());
    }

    let mut paths = Vec::new();
    for line in capture.stdout.lines() {
        if line.len() < 4 {
            continue;
        }
        let path = line[3..].trim();
        if let Some((_, right)) = path.split_once(" -> ") {
            paths.push(right.to_string());
        } else {
            paths.push(path.to_string());
        }
    }
    Ok(paths)
}

fn warnings_for_paths(paths: &[String]) -> Vec<String> {
    let mut warnings = Vec::new();
    for path in paths {
        let normalized = path.replace('\\', "/");
        let is_high_risk = normalized == "AGENTS.md"
            || normalized == "CLAUDE.md"
            || normalized == "COPILOT.md"
            || normalized.starts_with(".git")
            || normalized.starts_with(".keel")
            || normalized.starts_with(".github")
            || normalized.contains("/.git")
            || normalized.contains("/.keel");
        if is_high_risk {
            warnings.push(format!("high-risk path changed: {path}"));
        }
    }
    warnings
}

fn render_report(
    metadata: &RunMetadata,
    checks: &[CheckResult],
    diff: &str,
    failure: Option<&str>,
) -> String {
    let warnings = if metadata.warnings.is_empty() {
        "- none\n".to_string()
    } else {
        metadata
            .warnings
            .iter()
            .map(|warning| format!("- {warning}\n"))
            .collect()
    };

    let checks_table = checks
        .iter()
        .map(|check| {
            format!(
                "| {} | {} | {} | `{}` |\n",
                check.name,
                check.status,
                exit_code_text(check.exit_code),
                check.command
            )
        })
        .collect::<String>();

    let failure_section = failure.map_or_else(String::new, |message| {
        format!("## Failure\n\n- {message}\n\n")
    });

    format!(
        "# Keel Run Report\n\n\
         ## Summary\n\n\
         - Run ID: `{}`\n\
         - Task: {}\n\
         - Agent: `{}`\n\
         - Status: `{}`\n\
         - Created At: `{}`\n\
         - Updated At: `{}`\n\
         - Worktree: `{}`\n\
         - Branch: `{}`\n\
         - Base Commit: `{}`\n\
         - Agent Exit Code: `{}`\n\n\
         ## Warnings\n\n\
         {}\
         {}\
         ## Checks\n\n\
         | Name | Status | Exit | Command |\n\
         | --- | --- | --- | --- |\n\
         {}\
         ## Diff\n\n\
         ```diff\n{}\
         ```\n",
        metadata.run_id,
        metadata.task,
        metadata.agent,
        metadata.status,
        metadata.created_at,
        metadata.updated_at,
        metadata.worktree_path,
        metadata.branch,
        metadata.base_commit,
        exit_code_text(metadata.exit_code),
        warnings,
        failure_section,
        checks_table,
        diff
    )
}

fn run_command(dir: &Path, program: &str, args: &[String]) -> Result<CommandCapture> {
    let executable = resolve_program(program);
    let output = Command::new(&executable)
        .args(args.iter().map(OsStr::new))
        .current_dir(dir)
        .output()
        .map_err(|error| command_error(program, args, error))?;

    Ok(CommandCapture {
        status: output.status,
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

fn resolve_program(program: &str) -> PathBuf {
    let program_path = Path::new(program);
    if program_path.is_absolute() || program_path.components().count() > 1 {
        return program_path.to_path_buf();
    }

    #[cfg(windows)]
    {
        if let Some(path) = std::env::var_os("PATH") {
            let extensions = windows_path_extensions();
            for dir in std::env::split_paths(&path) {
                let candidate = dir.join(program);
                if candidate.is_file() {
                    return candidate;
                }

                for extension in &extensions {
                    let candidate = dir.join(format!("{program}{extension}"));
                    if candidate.is_file() {
                        return candidate;
                    }
                }
            }
        }
    }

    program_path.to_path_buf()
}

#[cfg(windows)]
fn windows_path_extensions() -> Vec<String> {
    std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
        .split(';')
        .filter(|extension| !extension.is_empty())
        .map(|extension| {
            if extension.starts_with('.') {
                extension.to_string()
            } else {
                format!(".{extension}")
            }
        })
        .collect()
}

fn command_error(program: &str, args: &[String], error: io::Error) -> anyhow::Error {
    if error.kind() == io::ErrorKind::NotFound && program_name_is(program, "codex") {
        anyhow::anyhow!("codex CLI not found; install Codex CLI or ensure `codex` is on PATH")
    } else {
        anyhow::anyhow!(
            "failed to execute {}: {}",
            format_command(program, args),
            error
        )
    }
}

fn program_name_is(program: &str, expected: &str) -> bool {
    Path::new(program)
        .file_stem()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.eq_ignore_ascii_case(expected))
        || program.eq_ignore_ascii_case(expected)
}

fn format_command(program: &str, args: &[String]) -> String {
    std::iter::once(program.to_string())
        .chain(args.iter().map(|arg| {
            if arg.contains(' ') {
                format!("{arg:?}")
            } else {
                arg.to_string()
            }
        }))
        .collect::<Vec<_>>()
        .join(" ")
}

fn ensure_safe_run_id(run_id: &str) -> Result<()> {
    if run_id.is_empty()
        || !run_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        bail!("invalid run id `{run_id}`");
    }
    Ok(())
}

fn ensure_safe_worktree_target(root: &Path, run_id: &str, target: &Path) -> Result<()> {
    ensure_safe_run_id(run_id)?;
    let expected = root.join(KEEL_DIR).join(WORKTREES_DIR).join(run_id);
    let expected_abs = absolutize(&expected)?;
    let target_abs = absolutize(target)?;
    if target_abs != expected_abs {
        bail!(
            "refusing to operate on unexpected worktree path {}; expected {}",
            target_abs.display(),
            expected_abs.display()
        );
    }
    Ok(())
}

fn absolutize(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .context("failed to read current directory")?
            .join(path))
    }
}

fn generate_run_id() -> String {
    format!("run-{}-{}", unix_millis(), std::process::id())
}

fn now_timestamp() -> String {
    unix_millis().to_string()
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn exit_code_text(code: Option<i32>) -> String {
    code.map_or_else(|| "n/a".to_string(), |code| code.to_string())
}

fn read_json<T>(path: &Path) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("failed to parse JSON {}", path.display()))
}

fn write_json_pretty<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(value)
        .with_context(|| format!("failed to serialize {}", path.display()))?;
    fs::write(path, content + "\n").with_context(|| format!("failed to write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
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
    }

    #[test]
    fn noop_run_creates_artifacts_and_discard_preserves_history() {
        let temp = git_repo();
        let project = KeelProject::discover(temp.path()).unwrap();
        project.init().unwrap();

        let metadata = project.run("test noop run", "noop").unwrap();

        assert_eq!(metadata.agent, "noop");
        assert_eq!(metadata.status, RunStatus::Ready);
        assert!(temp
            .path()
            .join(KEEL_DIR)
            .join(WORKTREES_DIR)
            .join(&metadata.run_id)
            .join(NOOP_OUTPUT_FILE)
            .exists());

        let run_dir = temp
            .path()
            .join(KEEL_DIR)
            .join(RUNS_DIR)
            .join(&metadata.run_id);
        assert!(run_dir.join(METADATA_FILE).exists());
        assert!(run_dir.join(LOG_FILE).exists());
        assert!(run_dir.join(DIFF_FILE).exists());
        assert!(run_dir.join(CHECKS_FILE).exists());
        assert!(run_dir.join(REPORT_FILE).exists());

        let discarded = project.discard(&metadata.run_id).unwrap();

        assert_eq!(discarded.status, RunStatus::Discarded);
        assert!(!temp
            .path()
            .join(KEEL_DIR)
            .join(WORKTREES_DIR)
            .join(&metadata.run_id)
            .exists());
        assert!(run_dir.join(METADATA_FILE).exists());
        assert!(run_dir.join(REPORT_FILE).exists());
        assert!(run_dir.join(LOG_FILE).exists());
        let report = fs::read_to_string(run_dir.join(REPORT_FILE)).unwrap();
        assert!(report.contains("# Keel Run Report"));
        assert!(report.contains("## Discard"));
        assert!(report.contains("keel-noop-output.txt"));
    }

    #[test]
    fn noop_run_force_adds_ignored_output_file() {
        let temp = git_repo_with_files(&[(".gitignore", "*.txt\n")]);
        let project = KeelProject::discover(temp.path()).unwrap();
        project.init().unwrap();

        let metadata = project.run("ignored noop output", "noop").unwrap();

        assert_eq!(metadata.status, RunStatus::Ready);
        let diff = fs::read_to_string(
            temp.path()
                .join(KEEL_DIR)
                .join(RUNS_DIR)
                .join(&metadata.run_id)
                .join(DIFF_FILE),
        )
        .unwrap();
        assert!(!diff.trim().is_empty());
        assert!(diff.contains(NOOP_OUTPUT_FILE));
    }

    #[test]
    fn run_uses_configured_checks() {
        let temp = git_repo();
        let project = KeelProject::discover(temp.path()).unwrap();
        project.init().unwrap();
        fs::write(
            temp.path().join(KEEL_DIR).join(CONFIG_FILE),
            r#"version = 1
runs_dir = "runs"
worktrees_dir = "worktrees"

[[checks]]
name = "custom status"
command = ["git", "status", "--short"]
"#,
        )
        .unwrap();

        let metadata = project.run("custom configured check", "noop").unwrap();

        let checks: Vec<CheckResult> = read_json(
            &temp
                .path()
                .join(KEEL_DIR)
                .join(RUNS_DIR)
                .join(&metadata.run_id)
                .join(CHECKS_FILE),
        )
        .unwrap();
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].name, "custom status");
    }

    #[test]
    fn failing_configured_check_blocks_ready() {
        let temp = git_repo();
        let project = KeelProject::discover(temp.path()).unwrap();
        project.init().unwrap();
        fs::write(
            temp.path().join(KEEL_DIR).join(CONFIG_FILE),
            r#"version = 1
runs_dir = "runs"
worktrees_dir = "worktrees"

[[checks]]
name = "failing check"
command = ["git", "not-a-real-keel-test-command"]
"#,
        )
        .unwrap();

        let metadata = project.run("failing configured check", "noop").unwrap();

        assert_eq!(metadata.status, RunStatus::NotReady);
        let checks: Vec<CheckResult> = read_json(
            &temp
                .path()
                .join(KEEL_DIR)
                .join(RUNS_DIR)
                .join(&metadata.run_id)
                .join(CHECKS_FILE),
        )
        .unwrap();
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

        let run_dir = temp
            .path()
            .join(KEEL_DIR)
            .join(RUNS_DIR)
            .join(&runs[0].run_id);
        assert!(run_dir.join(METADATA_FILE).exists());
        assert!(run_dir.join(LOG_FILE).exists());
        assert!(run_dir.join(DIFF_FILE).exists());
        assert!(run_dir.join(CHECKS_FILE).exists());
        assert!(run_dir.join(REPORT_FILE).exists());

        let report = fs::read_to_string(run_dir.join(REPORT_FILE)).unwrap();
        assert!(report.contains("## Failure"));
        assert!(report.contains("adapter exploded"));
    }

    #[test]
    fn unsupported_agent_is_rejected() {
        let temp = git_repo();
        let project = KeelProject::discover(temp.path()).unwrap();
        project.init().unwrap();

        let error = project.run("task", "claude").unwrap_err().to_string();

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
        });

        assert_eq!(command[0], "codex");
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
        let run_dir = temp
            .path()
            .join(KEEL_DIR)
            .join(RUNS_DIR)
            .join(&metadata.run_id);
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
        let run_dir = temp
            .path()
            .join(KEEL_DIR)
            .join(RUNS_DIR)
            .join(&metadata.run_id);
        assert_required_artifacts(&run_dir);
        let log = fs::read_to_string(run_dir.join(LOG_FILE)).unwrap();
        assert!(log.contains("fake codex failure"));
        let report = fs::read_to_string(run_dir.join(REPORT_FILE)).unwrap();
        assert!(report.contains("Agent Exit Code: `7`"));
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
        let run_dir = temp
            .path()
            .join(KEEL_DIR)
            .join(RUNS_DIR)
            .join(&runs[0].run_id);
        assert_required_artifacts(&run_dir);
        let report = fs::read_to_string(run_dir.join(REPORT_FILE)).unwrap();
        assert!(report.contains("## Failure"));
        assert!(report.contains("codex CLI not found"));
    }

    fn git_repo() -> TempDir {
        git_repo_with_files(&[])
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
            FakeCodexMode::Success => {
                "#!/bin/sh\necho fake codex stdout\necho fake codex stderr >&2\necho codex output > codex-output.txt\nexit 0\n"
            }
            FakeCodexMode::Failure => "#!/bin/sh\necho fake codex failure >&2\nexit 7\n",
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
}
