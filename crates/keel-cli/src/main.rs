use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use keel_core::{KeelProject, RunMetadata, RunStatus};

#[derive(Debug, Parser)]
#[command(name = "keel")]
#[command(about = "Local-first control layer for AI-generated code")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
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
    },
    /// Show the report path and summary for a run.
    Report {
        /// Run id.
        run_id: String,
    },
    /// Print the saved diff for a run.
    Diff {
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    let project = KeelProject::discover_from_current_dir()?;

    match cli.command {
        Commands::Init => {
            let result = project.init()?;
            println!("Initialized Keel at {}", result.keel_dir.display());
            println!("Config: {}", result.config_path.display());
            println!("Runs: {}", result.runs_dir.display());
        }
        Commands::Run { task, agent } => {
            let metadata = project.run(&task, &agent)?;
            print_run_created("Run created", &metadata);
        }
        Commands::Status { agent, status } => {
            let runs = filtered_runs(project.list_runs()?, agent.as_deref(), status);
            print_status(&runs, agent.as_deref(), status);
        }
        Commands::Report { run_id } => {
            let report = project.report(&run_id)?;
            println!("Report: {}", report.path.display());
            println!("{}", report.summary);
            println!("Artifacts:");
            for artifact in report.artifacts {
                let state = if artifact.exists {
                    "present"
                } else {
                    "missing"
                };
                println!(
                    "- {}: {} ({})",
                    artifact.label,
                    artifact.path.display(),
                    state
                );
            }
            println!("Suggested next actions:");
            for action in report.next_actions {
                println!("- {action}");
            }
            if report.is_discarded {
                println!("Run is already discarded.");
            }
        }
        Commands::Diff { run_id } => {
            let diff = project.diff(&run_id)?;
            println!("Diff: {}", diff.path.display());
            if diff.is_empty {
                println!("Diff for run `{run_id}` is empty.");
            } else {
                print!("{}", diff.content);
                if !diff.content.ends_with('\n') {
                    println!();
                }
            }
        }
        Commands::Rerun { run_id } => {
            let metadata = project.rerun(&run_id)?;
            print_run_created("Rerun created", &metadata);
            println!(
                "Parent: {}",
                metadata.parent_run_id.as_deref().unwrap_or("none")
            );
        }
        Commands::Discard { run_id } => {
            let metadata = project.discard(&run_id)?;
            println!("Discarded run: {}", metadata.run_id);
            println!("Status: {}", metadata.status);
            println!("History preserved at: {}", metadata.run_dir);
        }
    }

    Ok(())
}

fn filtered_runs(
    runs: Vec<RunMetadata>,
    agent: Option<&str>,
    status: Option<StatusFilter>,
) -> Vec<RunMetadata> {
    runs.into_iter()
        .filter(|run| agent.is_none_or(|agent| run.agent == agent))
        .filter(|run| status.is_none_or(|status| status.matches(&run.status)))
        .collect()
}

fn print_run_created(label: &str, metadata: &RunMetadata) {
    println!("{label}: {}", metadata.run_id);
    println!("Status: {}", metadata.status);
    println!("Worktree: {}", metadata.worktree_path);
    println!("Report: {}/report.md", metadata.run_dir);
}

fn print_status(runs: &[RunMetadata], agent: Option<&str>, status: Option<StatusFilter>) {
    if runs.is_empty() {
        if agent.is_some() || status.is_some() {
            println!("No runs matched the provided filters.");
        } else {
            println!("No runs found.");
        }
        return;
    }

    println!(
        "{:<24} {:<10} {:<11} {:<16} {:<18} Worktree",
        "Run ID", "Agent", "Status", "Created At", "Task"
    );
    for run in runs {
        println!(
            "{:<24} {:<10} {:<11} {:<16} {:<18} {}",
            run.run_id,
            run.agent,
            run.status,
            run.created_at,
            truncate(&run.task, 18),
            run.worktree_path
        );
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let mut output = value
        .chars()
        .take(max.saturating_sub(1))
        .collect::<String>();
    output.push('…');
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filtered_runs_filters_by_agent() {
        let runs = sample_runs();

        let filtered = filtered_runs(runs, Some("codex"), None);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].agent, "codex");
    }

    #[test]
    fn filtered_runs_filters_by_status() {
        let runs = sample_runs();

        let filtered = filtered_runs(runs, None, Some(StatusFilter::NotReady));

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].status, RunStatus::NotReady);
    }

    #[test]
    fn status_filter_accepts_snake_case_values() {
        let cli = Cli::parse_from(["keel", "status", "--status", "not_ready"]);

        match cli.command {
            Commands::Status { status, .. } => {
                assert!(matches!(status, Some(StatusFilter::NotReady)));
            }
            _ => panic!("expected status command"),
        }
    }

    fn sample_runs() -> Vec<RunMetadata> {
        vec![
            sample_run("run-1", "noop", RunStatus::Ready),
            sample_run("run-2", "codex", RunStatus::NotReady),
            sample_run("run-3", "claude", RunStatus::Discarded),
        ]
    }

    fn sample_run(run_id: &str, agent: &str, status: RunStatus) -> RunMetadata {
        RunMetadata {
            run_id: run_id.to_string(),
            parent_run_id: None,
            task: "task".to_string(),
            agent: agent.to_string(),
            status,
            created_at: "1".to_string(),
            updated_at: "1".to_string(),
            started_at: None,
            finished_at: None,
            duration_ms: None,
            worktree_path: format!(".keel/worktrees/{run_id}"),
            run_dir: format!(".keel/runs/{run_id}"),
            branch: format!("keel/run/{run_id}"),
            base_commit: String::new(),
            agent_command: Vec::new(),
            exit_code: None,
            failure_reason: None,
            readiness_reason: String::new(),
            warnings: Vec::new(),
        }
    }
}
