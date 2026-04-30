use crate::command::run_command_with_timeout;
use crate::constants::{DEFAULT_AGENT_TIMEOUT_SECS, NOOP_OUTPUT_FILE};
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use std::time::Duration;

pub(crate) trait AgentAdapter {
    fn name(&self) -> &'static str;
    fn command(&self, context: &AgentRunContext<'_>) -> Vec<String>;
    fn run(&self, context: &AgentRunContext<'_>) -> Result<AgentExecution>;

    fn requires_non_empty_diff(&self) -> bool {
        false
    }
}

pub(crate) struct AgentRunContext<'a> {
    pub(crate) run_id: &'a str,
    pub(crate) task: &'a str,
    pub(crate) worktree: &'a Path,
    pub(crate) agent_timeout_secs: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct AgentExecution {
    pub(crate) command: Vec<String>,
    pub(crate) exit_code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) timed_out: bool,
}

impl AgentExecution {
    pub(crate) fn command_line(&self) -> String {
        self.command.join(" ")
    }
}

pub(crate) struct NoopAgent;

impl AgentAdapter for NoopAgent {
    fn name(&self) -> &'static str {
        "noop"
    }

    fn command(&self, _context: &AgentRunContext<'_>) -> Vec<String> {
        vec!["noop".to_string()]
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
            command: self.command(context),
            exit_code: Some(0),
            stdout: format!("wrote {NOOP_OUTPUT_FILE}\n"),
            stderr: String::new(),
            timed_out: false,
        })
    }

    fn requires_non_empty_diff(&self) -> bool {
        true
    }
}

pub(crate) struct CodexAgent {
    program: String,
}

impl CodexAgent {
    pub(crate) fn new() -> Self {
        Self {
            program: "codex".to_string(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_program(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
        }
    }

    fn build_command(&self, context: &AgentRunContext<'_>) -> Vec<String> {
        vec![
            self.program.clone(),
            "--ask-for-approval".to_string(),
            "on-request".to_string(),
            "exec".to_string(),
            "--cd".to_string(),
            context.worktree.to_string_lossy().to_string(),
            "--sandbox".to_string(),
            "workspace-write".to_string(),
            context.task.to_string(),
        ]
    }
}

impl AgentAdapter for CodexAgent {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn command(&self, context: &AgentRunContext<'_>) -> Vec<String> {
        self.build_command(context)
    }

    fn run(&self, context: &AgentRunContext<'_>) -> Result<AgentExecution> {
        let command = self.build_command(context);
        let capture = run_command_with_timeout(
            context.worktree,
            &self.program,
            &command[1..],
            Duration::from_secs(context.agent_timeout_secs),
        )?;
        Ok(AgentExecution {
            command,
            exit_code: capture.exit_code,
            stdout: capture.stdout,
            stderr: capture.stderr,
            timed_out: capture.timed_out,
        })
    }
}

pub(crate) fn default_agent_timeout_secs() -> u64 {
    DEFAULT_AGENT_TIMEOUT_SECS
}
