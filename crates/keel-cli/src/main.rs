use anyhow::Result;
use clap::{Parser, Subcommand};
use keel_core::{KeelProject, RunMetadata};

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
        /// Agent adapter to use. Supported: noop, codex.
        #[arg(long, default_value = "noop")]
        agent: String,
    },
    /// List known runs.
    Status,
    /// Show the report path and summary for a run.
    Report {
        /// Run id.
        run_id: String,
    },
    /// Discard a candidate run by removing its worktree and preserving history.
    Discard {
        /// Run id.
        run_id: String,
    },
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
            println!("Run created: {}", metadata.run_id);
            println!("Status: {}", metadata.status);
            println!("Worktree: {}", metadata.worktree_path);
            println!("Report: {}/report.md", metadata.run_dir);
        }
        Commands::Status => {
            let runs = project.list_runs()?;
            print_status(&runs);
        }
        Commands::Report { run_id } => {
            let report = project.report(&run_id)?;
            println!("Report: {}", report.path.display());
            println!("{}", report.summary);
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

fn print_status(runs: &[RunMetadata]) {
    if runs.is_empty() {
        println!("No runs found.");
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
