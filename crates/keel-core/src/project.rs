use crate::agents::{
    AgentAdapter, AgentRunContext, ClaudeAgent, CodexAgent, NoopAgent, OpenCodeAgent,
};
use crate::checks::{classify_run, run_checks};
use crate::command::{format_command, run_command};
use crate::config::{default_checks, default_config_toml, KeelConfig};
use crate::constants::{
    CHECKS_FILE, CONFIG_FILE, DIFF_FILE, KEEL_DIR, LOG_FILE, METADATA_FILE, REPORT_FILE, RUNS_DIR,
    WORKTREES_DIR,
};
use crate::git::{
    ensure_safe_run_id, ensure_safe_worktree_target, expected_run_branch,
    prepare_untracked_for_diff,
};
use crate::json::{read_json, write_json_pretty};
use crate::model::{
    ArtifactInfo, DiffInfo, InitResult, LogInfo, ReportInfo, RunMetadata, RunStatus,
};
use crate::report::render_report;
use crate::risk::{analyze_diff_risk, format_risk_warning};
use crate::run::{RunLog, RunSession};
use crate::time::now_timestamp;
use anyhow::{bail, Context, Result};
use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Debug, Clone)]
pub struct KeelProject {
    root: PathBuf,
}

#[derive(Debug)]
struct BranchCleanup {
    branch: String,
    result: BranchCleanupResult,
    warning: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BranchCleanupResult {
    Deleted,
    AlreadyAbsent,
    SkippedInvalidMetadata,
    Failed,
}

impl std::fmt::Display for BranchCleanupResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Deleted => "deleted",
            Self::AlreadyAbsent => "already absent",
            Self::SkippedInvalidMetadata => "skipped invalid metadata",
            Self::Failed => "failed",
        };
        f.write_str(value)
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
        self.run_supported_agent(task, agent, None)
    }

    pub fn rerun(&self, run_id: &str) -> Result<RunMetadata> {
        ensure_safe_run_id(run_id)?;
        self.ensure_initialized()?;

        let source = self
            .read_metadata(run_id)
            .with_context(|| format!("failed to read source run `{run_id}`"))?;
        let child =
            self.run_supported_agent(&source.task, &source.agent, Some(run_id.to_string()))?;
        self.append_rerun_to_report(run_id, &child.run_id)?;
        Ok(child)
    }

    fn run_supported_agent(
        &self,
        task: &str,
        agent: &str,
        parent_run_id: Option<String>,
    ) -> Result<RunMetadata> {
        match agent {
            "noop" => self.run_with_adapter_parent(task, &NoopAgent, parent_run_id),
            "codex" => self.run_with_adapter_parent(task, &CodexAgent::new(), parent_run_id),
            "claude" => self.run_with_adapter_parent(task, &ClaudeAgent::new(), parent_run_id),
            "opencode" => self.run_with_adapter_parent(task, &OpenCodeAgent::new(), parent_run_id),
            other => bail!(
                "unsupported agent `{other}`; supported agents: noop, codex, claude, opencode"
            ),
        }
    }

    #[cfg(test)]
    pub(crate) fn run_with_adapter(
        &self,
        task: &str,
        adapter: &dyn AgentAdapter,
    ) -> Result<RunMetadata> {
        self.run_with_adapter_parent(task, adapter, None)
    }

    fn run_with_adapter_parent(
        &self,
        task: &str,
        adapter: &dyn AgentAdapter,
        parent_run_id: Option<String>,
    ) -> Result<RunMetadata> {
        let mut session = RunSession::start(self, task, adapter.name(), parent_run_id)?;
        session.log.push(format!("created run {}", session.run_id));
        if let Some(parent_run_id) = &session.metadata.parent_run_id {
            session.log.push(format!("parent run: {parent_run_id}"));
        }
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
        let config = self.read_config()?;
        session.mark_running(base_commit.clone())?;

        let worktree = self.create_run_worktree(session, base_commit)?;

        session
            .log
            .push(format!("running agent adapter `{}`", adapter.name()));
        let run_id = session.run_id.clone();
        let task = session.metadata.task.clone();
        let context = AgentRunContext {
            run_id: &run_id,
            task: &task,
            worktree: &worktree,
            agent_timeout_secs: config.agent_timeout_secs,
        };
        session.record_agent_plan(adapter.command(&context))?;
        let execution = adapter.run(&context)?;
        let (exit_code, timed_out) =
            session.record_agent_execution(execution, config.agent_timeout_secs);

        prepare_untracked_for_diff(&worktree, &mut session.log)?;

        let diff = self.capture_diff(&worktree, &mut session.log)?;
        let requires_non_empty_diff = adapter.requires_non_empty_diff();
        session.record_diff(diff, requires_non_empty_diff)?;
        session.checks = run_checks(&worktree, &config.checks, &mut session.log)?;

        let risk_warnings = analyze_diff_risk(session.diff.as_deref().unwrap_or(""), &config.risk);
        let mut warnings = risk_warnings
            .iter()
            .map(format_risk_warning)
            .collect::<Vec<_>>();
        if session.diff.as_deref().unwrap_or("").trim().is_empty() && !requires_non_empty_diff {
            warnings.push("candidate diff is empty".to_string());
        }
        session.apply_outcome(
            warnings,
            risk_warnings,
            classify_run(exit_code, timed_out, &session.checks),
        );
        Ok(())
    }

    fn create_run_worktree(
        &self,
        session: &mut RunSession,
        base_commit: String,
    ) -> Result<PathBuf> {
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

        Ok(worktree)
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
        runs.sort_by(compare_runs_newest_first);
        Ok(runs)
    }

    pub fn report(&self, run_id: &str) -> Result<ReportInfo> {
        ensure_safe_run_id(run_id)?;
        self.ensure_initialized()?;
        let metadata = self
            .read_metadata(run_id)
            .with_context(|| format!("run `{run_id}` does not exist"))?;
        let report_path = self.run_dir(run_id).join(REPORT_FILE);
        let summary = format!(
            "run_id={} parent={} task={:?} agent={} status={} created_at={} worktree={}",
            metadata.run_id,
            metadata.parent_run_id.as_deref().unwrap_or("none"),
            metadata.task,
            metadata.agent,
            metadata.status,
            metadata.created_at,
            metadata.worktree_path
        );
        Ok(ReportInfo {
            metadata: metadata.clone(),
            path: report_path,
            summary,
            artifacts: self.artifacts_for_run(run_id),
            next_actions: next_actions_for_report(&metadata),
            is_discarded: metadata.status == RunStatus::Discarded,
        })
    }

    pub fn diff(&self, run_id: &str) -> Result<DiffInfo> {
        ensure_safe_run_id(run_id)?;
        self.ensure_initialized()?;
        self.read_metadata(run_id)
            .with_context(|| format!("run `{run_id}` does not exist"))?;

        let path = self.run_dir(run_id).join(DIFF_FILE);
        if !path.exists() {
            bail!(
                "diff for run `{run_id}` does not exist at {}",
                path.display()
            );
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let is_empty = content.trim().is_empty();
        Ok(DiffInfo {
            path,
            content,
            is_empty,
        })
    }

    pub fn log(&self, run_id: &str) -> Result<LogInfo> {
        ensure_safe_run_id(run_id)?;
        self.ensure_initialized()?;
        self.read_metadata(run_id)
            .with_context(|| format!("run `{run_id}` does not exist"))?;

        let path = self.run_dir(run_id).join(LOG_FILE);
        if !path.exists() {
            bail!(
                "log for run `{run_id}` does not exist at {}",
                path.display()
            );
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let is_empty = content.trim().is_empty();
        Ok(LogInfo {
            path,
            content,
            is_empty,
        })
    }

    fn artifacts_for_run(&self, run_id: &str) -> Vec<ArtifactInfo> {
        [
            ("Metadata", METADATA_FILE),
            ("Log", LOG_FILE),
            ("Diff", DIFF_FILE),
            ("Checks", CHECKS_FILE),
            ("Report", REPORT_FILE),
        ]
        .into_iter()
        .map(|(label, file)| {
            let path = self.run_dir(run_id).join(file);
            ArtifactInfo {
                label,
                exists: path.exists(),
                path,
            }
        })
        .collect()
    }

    fn append_rerun_to_report(&self, source_run_id: &str, child_run_id: &str) -> Result<()> {
        let report_path = self.run_dir(source_run_id).join(REPORT_FILE);
        let existing_report = fs::read_to_string(&report_path)
            .with_context(|| format!("failed to read {}", report_path.display()))?;
        let rerun_section = format!(
            "{existing_report}\n\n## Rerun\n\n- Created rerun: `{child_run_id}`\n- Source run preserved: `{source_run_id}`\n"
        );
        fs::write(&report_path, rerun_section)
            .with_context(|| format!("failed to update {}", report_path.display()))
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

        let branch_cleanup = self.cleanup_candidate_branch(run_id, &metadata.branch, &mut log)?;
        if let Some(warning) = &branch_cleanup.warning {
            metadata.warnings.push(warning.clone());
        }

        metadata.status = RunStatus::Discarded;
        metadata.updated_at = now_timestamp();
        self.write_metadata(&metadata)?;

        let report_path = run_dir.join(REPORT_FILE);
        let report = match fs::read_to_string(&report_path) {
            Ok(existing_report) => format!(
                "{existing_report}\n\n## Discard\n\n- Status: `discarded`\n- Worktree removed: `{}`\n- Branch cleanup: `{}` (`{}`)\n{}- Run history preserved at: `{}`\n- Next action: use `keel rerun {run_id}` to create a fresh candidate from the same task.\n",
                if worktree_removed { "yes" } else { "already absent" },
                branch_cleanup.result,
                branch_cleanup.branch,
                branch_cleanup
                    .warning
                    .as_deref()
                    .map_or_else(String::new, |warning| format!("- Warning: {warning}\n")),
                metadata.run_dir
            ),
            Err(_) => render_report(
                &metadata,
                &[],
                "",
                Some("prior report was missing during discard; run history may be incomplete"),
                "",
                "",
            ),
        };
        fs::write(&report_path, report)
            .with_context(|| format!("failed to update {}", report_path.display()))?;
        log.push(format!("run {run_id} marked discarded"));
        log.write_to(&log_path)?;

        Ok(metadata)
    }

    fn cleanup_candidate_branch(
        &self,
        run_id: &str,
        branch: &str,
        log: &mut RunLog,
    ) -> Result<BranchCleanup> {
        let expected_branch = expected_run_branch(run_id)?;
        if branch != expected_branch {
            let warning = format!(
                "candidate branch cleanup skipped: metadata branch `{branch}` did not match expected `{expected_branch}`"
            );
            log.push(&warning);
            return Ok(BranchCleanup {
                branch: branch.to_string(),
                result: BranchCleanupResult::SkippedInvalidMetadata,
                warning: Some(warning),
            });
        }

        let ref_name = format!("refs/heads/{branch}");
        let exists_args = vec![
            "show-ref".to_string(),
            "--verify".to_string(),
            "--quiet".to_string(),
            ref_name,
        ];
        let exists_capture = run_command(&self.root, "git", &exists_args)?;
        log.push_command(
            &self.root,
            &format_command("git", &exists_args),
            &exists_capture,
        );
        if !exists_capture.status.success() {
            log.push(format!("candidate branch {branch} already absent"));
            return Ok(BranchCleanup {
                branch: branch.to_string(),
                result: BranchCleanupResult::AlreadyAbsent,
                warning: None,
            });
        }

        let delete_args = vec!["branch".to_string(), "-D".to_string(), branch.to_string()];
        let delete_capture = run_command(&self.root, "git", &delete_args)?;
        log.push_command(
            &self.root,
            &format_command("git", &delete_args),
            &delete_capture,
        );
        if delete_capture.status.success() {
            return Ok(BranchCleanup {
                branch: branch.to_string(),
                result: BranchCleanupResult::Deleted,
                warning: None,
            });
        }

        let warning = format!(
            "candidate branch cleanup failed for `{branch}`: {}",
            delete_capture.stderr.trim()
        );
        log.push(&warning);
        Ok(BranchCleanup {
            branch: branch.to_string(),
            result: BranchCleanupResult::Failed,
            warning: Some(warning),
        })
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

    pub(crate) fn run_dir(&self, run_id: &str) -> PathBuf {
        self.runs_dir().join(run_id)
    }

    fn worktree_dir(&self, run_id: &str) -> PathBuf {
        self.worktrees_dir().join(run_id)
    }

    fn read_metadata(&self, run_id: &str) -> Result<RunMetadata> {
        read_json(&self.run_dir(run_id).join(METADATA_FILE))
    }

    pub(crate) fn write_metadata(&self, metadata: &RunMetadata) -> Result<()> {
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

fn next_actions_for_report(metadata: &RunMetadata) -> Vec<String> {
    let run_id = &metadata.run_id;
    let mut actions = vec![
        format!("keel diff {run_id}"),
        format!("keel rerun {run_id}"),
    ];
    if metadata.status != RunStatus::Discarded {
        actions.push(format!("keel discard {run_id}"));
    }
    actions
}

fn compare_runs_newest_first(left: &RunMetadata, right: &RunMetadata) -> Ordering {
    compare_created_at_newest_first(&left.created_at, &right.created_at)
        .then_with(|| right.run_id.cmp(&left.run_id))
}

fn compare_created_at_newest_first(left: &str, right: &str) -> Ordering {
    match (
        parse_created_at_millis(left),
        parse_created_at_millis(right),
    ) {
        (Some(left), Some(right)) => return right.cmp(&left),
        (None, None) => {}
        _ => {}
    }

    right.cmp(left)
}

fn parse_created_at_millis(value: &str) -> Option<i128> {
    parse_rfc3339_millis(value).or_else(|| value.parse::<i128>().ok())
}

fn parse_rfc3339_millis(value: &str) -> Option<i128> {
    let parsed = OffsetDateTime::parse(value, &Rfc3339).ok()?;
    Some(parsed.unix_timestamp_nanos() / 1_000_000)
}

#[cfg(test)]
pub(crate) fn compare_created_at_for_test(left: &str, right: &str) -> Ordering {
    compare_created_at_newest_first(left, right)
}
