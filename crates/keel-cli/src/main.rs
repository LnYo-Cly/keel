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
    #[command(subcommand)]
    command: Commands,
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
