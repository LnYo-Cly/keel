use crate::agents::AgentExecution;
use crate::artifact_files;
use crate::checks::RunClassification;
use crate::command::{
    exit_code_text, failure_reason_from_error, format_command_line, CommandCapture,
};
use crate::fsio::write_text;
use crate::json::write_json_pretty;
use crate::model::{CheckResult, FailureReason, RunMetadata, RunStatus};
use crate::project::KeelProject;
use crate::report::render_report;
use crate::risk::RiskWarning;
use crate::time::{generate_run_id, now_timestamp, unix_millis};
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub(crate) struct RunLog {
    pub(crate) lines: Vec<String>,
}

impl RunLog {
    pub(crate) fn push(&mut self, message: impl AsRef<str>) {
        self.lines
            .push(format!("[{}] {}", now_timestamp(), message.as_ref()));
    }

    pub(crate) fn push_command(&mut self, cwd: &Path, command: &str, capture: &CommandCapture) {
        self.push(format!("$ ({}) {}", cwd.display(), command));
        self.push(format!("exit: {}", exit_code_text(capture.status.code())));
        if !capture.stdout.trim().is_empty() {
            self.push(format!("stdout:\n{}", capture.stdout.trim_end()));
        }
        if !capture.stderr.trim().is_empty() {
            self.push(format!("stderr:\n{}", capture.stderr.trim_end()));
        }
    }

    pub(crate) fn write_to(&self, path: &Path) -> Result<()> {
        write_text(path, self.lines.join("\n") + "\n")
            .with_context(|| format!("failed to write log {}", path.display()))
    }
}

#[derive(Debug)]
pub(crate) struct RunSession {
    pub(crate) run_id: String,
    pub(crate) run_dir: PathBuf,
    pub(crate) metadata: RunMetadata,
    pub(crate) log: RunLog,
    pub(crate) checks: Vec<CheckResult>,
    pub(crate) diff: Option<String>,
    failure: Option<String>,
    agent_stdout: String,
    agent_stderr: String,
}

impl RunSession {
    pub(crate) fn start(
        project: &KeelProject,
        task: &str,
        agent: &str,
        parent_run_id: Option<String>,
    ) -> Result<Self> {
        let run_id = generate_run_id();
        let run_dir = project.run_dir(&run_id);
        fs::create_dir_all(&run_dir)
            .with_context(|| format!("failed to create run directory {}", run_dir.display()))?;

        let created_at = now_timestamp();
        let metadata =
            RunMetadata::new(run_id.clone(), task, agent, RunStatus::Created, created_at)
                .with_parent_run_id(parent_run_id)
                .with_readiness_reason("run has not started");
        let session = Self {
            run_id,
            run_dir,
            metadata,
            log: RunLog::default(),
            checks: Vec::new(),
            diff: None,
            failure: None,
            agent_stdout: String::new(),
            agent_stderr: String::new(),
        };
        session.persist_metadata()?;
        Ok(session)
    }

    pub(crate) fn mark_running(&mut self, base_commit: String) -> Result<()> {
        let started_at = unix_millis();
        self.metadata.started_at = Some(started_at.to_string());
        self.metadata.base_commit = base_commit;
        self.metadata.status = RunStatus::Running;
        self.metadata.readiness_reason = "run is in progress".to_string();
        self.metadata.updated_at = started_at.to_string();
        self.persist_metadata()
    }

    pub(crate) fn record_agent_plan(&mut self, command: Vec<String>) -> Result<()> {
        self.metadata.agent_command = command;
        self.persist_metadata()?;
        self.log.push(format!(
            "agent command: {}",
            format_command_line(&self.metadata.agent_command)
        ));
        Ok(())
    }

    pub(crate) fn record_agent_execution(
        &mut self,
        execution: AgentExecution,
        timeout_secs: u64,
    ) -> (Option<i32>, bool) {
        let command_line = execution.command_line();
        let exit_code = execution.exit_code;
        let timed_out = execution.timed_out;

        self.log.push(format!("agent command: {command_line}"));
        self.log
            .push(format!("agent exit code: {}", exit_code_text(exit_code)));
        if timed_out {
            self.log
                .push(format!("agent timed out after {timeout_secs} seconds"));
        }
        if !execution.stdout.trim().is_empty() {
            self.log
                .push(format!("agent stdout:\n{}", execution.stdout.trim_end()));
        }
        if !execution.stderr.trim().is_empty() {
            self.log
                .push(format!("agent stderr:\n{}", execution.stderr.trim_end()));
        }

        self.metadata.agent_command = execution.command;
        self.metadata.exit_code = exit_code;
        self.agent_stdout = execution.stdout;
        self.agent_stderr = execution.stderr;

        (exit_code, timed_out)
    }

    pub(crate) fn record_diff(
        &mut self,
        diff: String,
        requires_non_empty_diff: bool,
    ) -> Result<()> {
        if requires_non_empty_diff && diff.trim().is_empty() {
            self.metadata.failure_reason = Some(FailureReason::EmptyDiff);
            self.metadata.readiness_reason = "required candidate diff was empty".to_string();
            bail!("agent run produced an empty diff; refusing to mark candidate ready");
        }
        self.diff = Some(diff);
        Ok(())
    }

    pub(crate) fn apply_outcome(
        &mut self,
        warnings: Vec<String>,
        risk_warnings: Vec<RiskWarning>,
        classification: RunClassification,
    ) {
        self.metadata.warnings = warnings;
        self.metadata.risk_warnings = risk_warnings;
        self.metadata.status = classification.status;
        self.metadata.failure_reason = classification.failure_reason;
        self.metadata.readiness_reason = classification.readiness_reason;
        self.metadata.updated_at = now_timestamp();
    }

    pub(crate) fn finalize_success(&mut self) -> Result<()> {
        self.mark_finished();
        self.persist_metadata()?;
        self.persist_checks()?;
        self.persist_diff()?;
        self.persist_report()?;
        self.log.push("report generated");
        self.persist_log()
    }

    pub(crate) fn finalize_failure(&mut self, error: &anyhow::Error) -> Result<()> {
        self.failure = Some(error.to_string());
        self.metadata.status = RunStatus::NotReady;
        if self.metadata.failure_reason.is_none() {
            self.metadata.failure_reason = Some(failure_reason_from_error(error));
        }
        if self.metadata.readiness_reason.trim().is_empty()
            || self.metadata.readiness_reason == "run has not started"
        {
            self.metadata.readiness_reason = format!("run failed: {error}");
        }
        self.mark_finished();
        self.log.push(format!("run failed: {error}"));
        self.persist_metadata()?;
        self.persist_checks()?;
        self.persist_diff()?;
        self.persist_report()?;
        self.persist_log()
    }

    fn persist_metadata(&self) -> Result<()> {
        write_json_pretty(&self.run_dir.join(artifact_files::METADATA), &self.metadata)
    }

    fn persist_checks(&self) -> Result<()> {
        write_json_pretty(&self.run_dir.join(artifact_files::CHECKS), &self.checks)
    }

    fn persist_diff(&self) -> Result<()> {
        write_text(
            &self.run_dir.join(artifact_files::DIFF),
            self.diff.as_deref().unwrap_or(""),
        )
        .with_context(|| {
            format!(
                "failed to write {}",
                self.run_dir.join(artifact_files::DIFF).display()
            )
        })
    }

    fn persist_report(&self) -> Result<()> {
        write_text(
            &self.run_dir.join(artifact_files::REPORT),
            render_report(
                &self.metadata,
                &self.checks,
                self.diff.as_deref().unwrap_or(""),
                self.failure.as_deref(),
                &self.agent_stdout,
                &self.agent_stderr,
            ),
        )
        .with_context(|| {
            format!(
                "failed to write {}",
                self.run_dir.join(artifact_files::REPORT).display()
            )
        })
    }

    fn persist_log(&self) -> Result<()> {
        self.log.write_to(&self.run_dir.join(artifact_files::LOG))
    }

    fn mark_finished(&mut self) {
        let finished_at = unix_millis();
        self.metadata.finished_at = Some(finished_at.to_string());
        self.metadata.updated_at = finished_at.to_string();
        self.metadata.duration_ms = self
            .metadata
            .started_at
            .as_deref()
            .and_then(|started_at| started_at.parse::<u128>().ok())
            .map(|started_at| finished_at.saturating_sub(started_at));
    }
}
