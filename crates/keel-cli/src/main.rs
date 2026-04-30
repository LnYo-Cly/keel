use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use keel_core::{
    report_json, run_doctor, status_json, validate_config, CommitOptions, KeelProject, PrOptions,
    PrProvider, PushOptions, RunMetadata, RunStatus,
};
use std::process::ExitCode;

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
    /// Build a manual PR/MR plan for a pushed candidate branch.
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
        /// Override provider inference. Supported: github, gitlab, gitee, gitea.
        #[arg(long, value_parser = parse_pr_provider)]
        provider: Option<PrProvider>,
        /// Target branch for the future PR/MR.
        #[arg(long)]
        target: Option<String>,
        /// Title for the future PR/MR.
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
    let cli = Cli::parse();

    if let Commands::Doctor { json } = cli.command {
        let cwd = std::env::current_dir()?;
        let report = run_doctor(&cwd);
        if json {
            render::print_json(&report)?;
        } else {
            render::print_doctor(&report);
        }
        return Ok(render::exit_code_for_report(&report));
    }

    let project = KeelProject::discover_from_current_dir()?;

    match cli.command {
        Commands::Doctor { .. } => unreachable!("doctor is handled before project discovery"),
        Commands::Config {
            command: ConfigCommands::Validate { json },
        } => {
            let report = validate_config(project.root());
            if json {
                render::print_json(&report)?;
            } else {
                render::print_config_validation(&report);
            }
            return Ok(render::exit_code_for_config_report(&report));
        }
        Commands::Init => {
            let result = project.init()?;
            println!("Initialized Keel at {}", result.keel_dir.display());
            println!("Config: {}", result.config_path.display());
            println!("Runs: {}", result.runs_dir.display());
        }
        Commands::Run { task, agent } => {
            let metadata = project.run(&task, &agent)?;
            render::print_run_created("Run created", &metadata);
        }
        Commands::Status {
            agent,
            status,
            limit,
            json,
        } => {
            let runs = filtered_runs(project.list_runs()?, agent.as_deref(), status, limit);
            if json {
                render::print_json(&status_json(&runs))?;
            } else {
                render::print_status(&runs, agent.as_deref().is_some() || status.is_some());
            }
        }
        Commands::Report { run_id, json } => {
            let report = project.report(&run_id)?;
            if json {
                render::print_json(&report_json(&report))?;
            } else {
                render::print_report(report);
            }
        }
        Commands::Commit {
            run_id,
            dry_run,
            json,
            message,
        } => {
            let result = project.commit(&run_id, CommitOptions { dry_run, message })?;
            if json {
                render::print_json(&result)?;
            } else {
                render::print_commit_result(&result);
            }
        }
        Commands::Push {
            run_id,
            remote,
            dry_run,
            json,
        } => {
            let result = project.push(&run_id, PushOptions { remote, dry_run })?;
            if json {
                render::print_json(&result)?;
            } else {
                render::print_push_result(&result);
            }
        }
        Commands::Pr {
            run_id,
            manual,
            dry_run,
            json,
            provider,
            target,
            title,
        } => {
            let options = PrOptions {
                manual,
                dry_run,
                provider,
                target,
                title,
            };
            if manual {
                let plan = project.pr_plan(&run_id, options)?;
                if json {
                    render::print_json(&plan)?;
                } else {
                    render::print_pr_plan(&plan);
                }
            } else {
                let result = project.pr(&run_id, options)?;
                if json {
                    render::print_json(&result)?;
                } else {
                    render::print_pr_result(&result);
                }
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
        Commands::Log { run_id } => {
            let log = project.log(&run_id)?;
            println!("Log: {}", log.path.display());
            if log.is_empty {
                println!("Log for run `{run_id}` is empty.");
            } else {
                print!("{}", log.content);
                if !log.content.ends_with('\n') {
                    println!();
                }
            }
        }
        Commands::Rerun { run_id } => {
            let metadata = project.rerun(&run_id)?;
            render::print_run_created("Rerun created", &metadata);
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

    Ok(ExitCode::SUCCESS)
}

fn filtered_runs(
    runs: Vec<RunMetadata>,
    agent: Option<&str>,
    status: Option<StatusFilter>,
    limit: Option<usize>,
) -> Vec<RunMetadata> {
    let mut runs = runs
        .into_iter()
        .filter(|run| agent.is_none_or(|agent| run.agent == agent))
        .filter(|run| status.is_none_or(|status| status.matches(&run.status)))
        .collect::<Vec<_>>();
    if let Some(limit) = limit {
        runs.truncate(limit);
    }
    runs
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
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filtered_runs_filters_by_agent() {
        let runs = sample_runs();

        let filtered = filtered_runs(runs, Some("codex"), None, None);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].agent, "codex");
    }

    #[test]
    fn filtered_runs_filters_by_status() {
        let runs = sample_runs();

        let filtered = filtered_runs(runs, None, Some(StatusFilter::NotReady), None);

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

    #[test]
    fn filtered_runs_applies_limit_after_filters() {
        let runs = sample_runs();

        let filtered = filtered_runs(runs, Some("noop"), None, Some(1));

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].run_id, "run-1");
    }

    #[test]
    fn status_limit_rejects_zero() {
        let result = Cli::try_parse_from(["keel", "status", "--limit", "0"]);

        assert!(result.is_err());
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
            risk_warnings: Vec::new(),
            committed: false,
            commit_sha: None,
            commit_message: None,
            committed_at: None,
            commit: None,
            pushed: false,
            pushed_at: None,
            push_remote: None,
            push_remote_url: None,
            pushed_branch: None,
            push: None,
            pr_created: false,
            pr_created_at: None,
            pr_provider: None,
            pr_url: None,
            pr_target_branch: None,
            pr_source_branch: None,
            pr: None,
        }
    }
}
