use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use keel_core::{PrProvider, RunStatus};
use std::process::ExitCode;

mod commands;
mod render;

#[derive(Debug, Parser)]
#[command(name = "keel")]
#[command(about = "Local-first control layer for AI-generated code")]
struct Cli {
    /// Open the default review UI focused on this run id.
    #[arg(long)]
    run: Option<String>,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Diagnose whether the current repository is ready to run Keel.
    Doctor {
        /// Print machine-readable JSON instead of human output.
        #[arg(long)]
        json: bool,
    },
    /// Inspect and validate Keel configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
    /// Initialize local Keel state in the current git repository.
    Init,
    /// Manage the current workspace task ledger.
    Task {
        #[command(subcommand)]
        command: TaskCommands,
    },
    /// Record a checkpoint on the active workspace task.
    Checkpoint {
        /// Checkpoint message.
        message: String,
        /// Print machine-readable JSON instead of human output.
        #[arg(long)]
        json: bool,
    },
    /// Record a note on the active workspace task.
    Note {
        /// Note message.
        message: String,
        /// Print machine-readable JSON instead of human output.
        #[arg(long)]
        json: bool,
    },
    /// Record command evidence on the active workspace task.
    Evidence {
        #[command(subcommand)]
        command: EvidenceCommands,
    },
    /// Verify whether the active workspace task has passing evidence.
    Verify {
        /// Print machine-readable JSON instead of human output.
        #[arg(long)]
        json: bool,
    },
    /// Generate a handoff packet for the active workspace task.
    Handoff {
        /// Print machine-readable JSON instead of human output.
        #[arg(long)]
        json: bool,
    },
    /// Generate a review packet for the active workspace task.
    Review {
        /// Print machine-readable JSON instead of human output.
        #[arg(long)]
        json: bool,
    },
    /// Open the read-only terminal review UI.
    Tui {
        /// Focus the review UI on a specific run id.
        #[arg(long)]
        run: Option<String>,
        /// Start with a fuzzy text filter applied.
        #[arg(long)]
        filter: Option<String>,
        /// Start with an exact agent filter applied.
        #[arg(long)]
        agent: Option<String>,
        /// Start with an exact run status filter applied.
        #[arg(long)]
        status: Option<StatusFilter>,
    },
    /// Run a coding task with an agent.
    Run {
        /// Task prompt to pass to the agent.
        task: String,
        /// Agent adapter to use. Supported: noop, codex, claude, opencode.
        #[arg(long, default_value = "noop")]
        agent: String,
    },
    /// List known runs.
    Status {
        /// Filter by agent.
        #[arg(long)]
        agent: Option<String>,
        /// Filter by run status.
        #[arg(long)]
        status: Option<StatusFilter>,
        /// Limit the number of runs after filtering.
        #[arg(long, value_parser = parse_positive_usize)]
        limit: Option<usize>,
        /// Print machine-readable JSON instead of the human table.
        #[arg(long)]
        json: bool,
    },
    /// Show the report path and summary for a run.
    Report {
        /// Run id.
        run_id: String,
        /// Print machine-readable JSON instead of human output.
        #[arg(long)]
        json: bool,
    },
    /// Commit a ready candidate run on its local worktree branch.
    Commit {
        /// Run id.
        run_id: String,
        /// Do all prechecks and print the plan without creating a commit.
        #[arg(long)]
        dry_run: bool,
        /// Print machine-readable JSON instead of human output.
        #[arg(long)]
        json: bool,
        /// Custom local commit message.
        #[arg(long)]
        message: Option<String>,
    },
    /// Push a committed candidate branch to a generic Git remote.
    Push {
        /// Run id.
        run_id: String,
        /// Git remote name to push to.
        #[arg(long, default_value = "origin")]
        remote: String,
        /// Do all prechecks and print the plan without pushing.
        #[arg(long)]
        dry_run: bool,
        /// Print machine-readable JSON instead of human output.
        #[arg(long)]
        json: bool,
    },
    /// Plan or create a PR/MR for a pushed candidate branch.
    Pr {
        /// Run id.
        run_id: String,
        /// Generate manual instructions without provider API calls.
        #[arg(long)]
        manual: bool,
        /// Print the plan without writing artifacts or creating provider requests.
        #[arg(long)]
        dry_run: bool,
        /// Print machine-readable JSON instead of human output.
        #[arg(long)]
        json: bool,
        /// Create a draft PR when using a provider-backed flow.
        #[arg(long)]
        draft: bool,
        /// Override provider inference. Automated creation is GitHub-only; other providers support manual planning.
        #[arg(long, value_parser = parse_pr_provider)]
        provider: Option<PrProvider>,
        /// Target branch for the PR/MR.
        #[arg(long)]
        base: Option<String>,
        /// Source branch for the PR/MR. Defaults to the pushed candidate branch.
        #[arg(long)]
        head: Option<String>,
        /// Deprecated alias for --base.
        #[arg(long, hide = true)]
        target: Option<String>,
        /// Title for the PR/MR.
        #[arg(long)]
        title: Option<String>,
    },
    /// Print the saved diff for a run.
    Diff {
        /// Run id.
        run_id: String,
    },
    /// Print the saved log for a run.
    Log {
        /// Run id.
        run_id: String,
    },
    /// Rerun an existing task in a fresh candidate worktree.
    Rerun {
        /// Source run id.
        run_id: String,
    },
    /// Discard a candidate run by removing its worktree and preserving history.
    Discard {
        /// Run id.
        run_id: String,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommands {
    /// Validate .keel/config.toml without modifying it.
    Validate {
        /// Print machine-readable JSON instead of human output.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum TaskCommands {
    /// Start or replace the active workspace task ledger.
    Start {
        /// Task title.
        title: String,
        /// Print machine-readable JSON instead of human output.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum EvidenceCommands {
    /// Execute a command and record its output as evidence.
    Add {
        /// Command to execute from the repository root.
        #[arg(long)]
        cmd: String,
        /// Environment variable to set for the command, formatted as KEY=VALUE.
        #[arg(long = "env", value_parser = parse_key_value)]
        env: Vec<(String, String)>,
        /// Print machine-readable JSON instead of human output.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
#[value(rename_all = "snake_case")]
enum StatusFilter {
    Created,
    Running,
    Ready,
    NotReady,
    Discarded,
}

impl std::fmt::Display for StatusFilter {
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

impl StatusFilter {
    fn matches(self, status: &RunStatus) -> bool {
        matches!(
            (self, status),
            (Self::Created, RunStatus::Created)
                | (Self::Running, RunStatus::Running)
                | (Self::Ready, RunStatus::Ready)
                | (Self::NotReady, RunStatus::NotReady)
                | (Self::Discarded, RunStatus::Discarded)
        )
    }

    fn to_run_status(self) -> RunStatus {
        match self {
            Self::Created => RunStatus::Created,
            Self::Running => RunStatus::Running,
            Self::Ready => RunStatus::Ready,
            Self::NotReady => RunStatus::NotReady,
            Self::Discarded => RunStatus::Discarded,
        }
    }
}

fn main() -> Result<ExitCode> {
    commands::run(Cli::parse())
}

fn parse_positive_usize(value: &str) -> std::result::Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("invalid positive integer `{value}`"))?;
    if parsed == 0 {
        return Err("limit must be greater than 0".to_string());
    }
    Ok(parsed)
}

fn parse_pr_provider(value: &str) -> std::result::Result<PrProvider, String> {
    value
        .parse::<PrProvider>()
        .map_err(|error: anyhow::Error| error.to_string())
}

fn parse_key_value(value: &str) -> std::result::Result<(String, String), String> {
    let (key, value) = value
        .split_once('=')
        .ok_or_else(|| "expected KEY=VALUE".to_string())?;
    if key.trim().is_empty() {
        return Err("environment variable key cannot be empty".to_string());
    }
    Ok((key.trim().to_string(), value.to_string()))
}
