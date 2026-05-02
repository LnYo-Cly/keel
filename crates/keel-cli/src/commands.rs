use anyhow::Result;
use keel_core::{
    report_json, run_doctor, status_json, validate_config, CommitOptions, KeelProject,
    LedgerEvidenceEnv, PrOptions, PushOptions, RunMetadata,
};
use std::process::ExitCode;

use super::{render, Cli, Commands, ConfigCommands, EvidenceCommands, StatusFilter, TaskCommands};

pub(crate) fn run(cli: Cli) -> Result<ExitCode> {
    if let Some(Commands::Doctor { json }) = cli.command.as_ref() {
        let cwd = std::env::current_dir()?;
        let report = run_doctor(&cwd);
        if *json {
            render::print_json(&report)?;
        } else {
            render::print_doctor(&report);
        }
        return Ok(render::exit_code_for_report(&report));
    }

    let project = KeelProject::discover_from_current_dir()?;

    match cli.command {
        None => {
            if let Some(run_id) = cli.run {
                keel_tui::run_tui_for_run(project, run_id)?;
            } else {
                keel_tui::run_tui(project)?;
            }
        }
        Some(Commands::Doctor { .. }) => {
            unreachable!("doctor is handled before project discovery")
        }
        Some(Commands::Config {
            command: ConfigCommands::Validate { json },
        }) => {
            let report = validate_config(project.root());
            if json {
                render::print_json(&report)?;
            } else {
                render::print_config_validation(&report);
            }
            return Ok(render::exit_code_for_config_report(&report));
        }
        Some(Commands::Init) => {
            let result = project.init()?;
            println!("Initialized Keel at {}", result.keel_dir.display());
            println!("Config: {}", result.config_path.display());
            println!("Runs: {}", result.runs_dir.display());
        }
        Some(Commands::Task {
            command: TaskCommands::Start { title, json },
        }) => {
            let task = project.start_ledger_task(&title)?;
            if json {
                render::print_json(&task)?;
            } else {
                render::print_ledger_task_started(&task);
            }
        }
        Some(Commands::Checkpoint { message, json }) => {
            let task = project.checkpoint(&message)?;
            if json {
                render::print_json(&task)?;
            } else {
                render::print_ledger_checkpoint(&task);
            }
        }
        Some(Commands::Note { message, json }) => {
            let task = project.note(&message)?;
            if json {
                render::print_json(&task)?;
            } else {
                render::print_ledger_note(&task);
            }
        }
        Some(Commands::Evidence {
            command: EvidenceCommands::Add { cmd, env, json },
        }) => {
            let env = env
                .into_iter()
                .map(|(key, value)| LedgerEvidenceEnv { key, value })
                .collect();
            let task = project.evidence(&cmd, env)?;
            if json {
                render::print_json(&task)?;
            } else {
                render::print_ledger_evidence(&task);
            }
        }
        Some(Commands::Verify { json }) => {
            let review = project.ledger_review()?;
            if json {
                render::print_json(&review)?;
            } else {
                render::print_ledger_verify(&review);
            }
            return Ok(if review.decision.ready {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            });
        }
        Some(Commands::Handoff { json }) => {
            let handoff = project.handoff()?;
            if json {
                render::print_json(&handoff)?;
            } else {
                render::print_ledger_handoff(&handoff);
            }
        }
        Some(Commands::Review { json }) => {
            let review = project.ledger_review()?;
            if json {
                render::print_json(&review)?;
            } else {
                render::print_ledger_review(&review);
            }
        }
        Some(Commands::Tui {
            run,
            filter,
            agent,
            status,
        }) => {
            keel_tui::run_tui_with_filters(
                project,
                keel_tui::TuiFilters {
                    text: filter.unwrap_or_default(),
                    agent,
                    status: status.map(StatusFilter::to_run_status),
                    run_id: run,
                },
            )?;
        }
        Some(Commands::Run { task, agent }) => {
            let metadata = project.run(&task, &agent)?;
            render::print_run_created("Run created", &metadata);
        }
        Some(Commands::Status {
            agent,
            status,
            limit,
            json,
        }) => {
            let runs = filtered_runs(project.list_runs()?, agent.as_deref(), status, limit);
            if json {
                render::print_json(&status_json(&runs))?;
            } else {
                render::print_status(&runs, agent.as_deref().is_some() || status.is_some());
            }
        }
        Some(Commands::Report { run_id, json }) => {
            let report = project.report(&run_id)?;
            if json {
                render::print_json(&report_json(&report))?;
            } else {
                render::print_report(report);
            }
        }
        Some(Commands::Commit {
            run_id,
            dry_run,
            json,
            message,
        }) => {
            let result = project.commit(&run_id, CommitOptions { dry_run, message })?;
            if json {
                render::print_json(&result)?;
            } else {
                render::print_commit_result(&result);
            }
        }
        Some(Commands::Push {
            run_id,
            remote,
            dry_run,
            json,
        }) => {
            let result = project.push(&run_id, PushOptions { remote, dry_run })?;
            if json {
                render::print_json(&result)?;
            } else {
                render::print_push_result(&result);
            }
        }
        Some(Commands::Pr {
            run_id,
            manual,
            dry_run,
            json,
            draft,
            provider,
            base,
            head,
            target,
            title,
        }) => {
            let options = PrOptions {
                manual,
                dry_run,
                draft,
                provider,
                base,
                head,
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
        Some(Commands::Diff { run_id }) => {
            let diff = project.diff(&run_id)?;
            render::print_diff(&run_id, &diff);
        }
        Some(Commands::Log { run_id }) => {
            let log = project.log(&run_id)?;
            render::print_log(&run_id, &log);
        }
        Some(Commands::Rerun { run_id }) => {
            let metadata = project.rerun(&run_id)?;
            render::print_run_created("Rerun created", &metadata);
            println!(
                "Parent: {}",
                metadata.parent_run_id.as_deref().unwrap_or("none")
            );
        }
        Some(Commands::Discard { run_id }) => {
            let metadata = project.discard(&run_id)?;
            render::print_discarded_run(&metadata);
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use keel_core::RunStatus;

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
            Some(Commands::Status { status, .. }) => {
                assert!(matches!(status, Some(StatusFilter::NotReady)));
            }
            _ => panic!("expected status command"),
        }
    }

    #[test]
    fn root_command_without_subcommand_defaults_to_tui() {
        let cli = Cli::parse_from(["keel"]);

        assert!(cli.command.is_none());
    }

    #[test]
    fn root_command_accepts_run_focus_for_default_tui() {
        let cli = Cli::parse_from(["keel", "--run", "run-123"]);

        assert!(cli.command.is_none());
        assert_eq!(cli.run.as_deref(), Some("run-123"));
    }

    #[test]
    fn tui_command_accepts_run_focus() {
        let cli = Cli::parse_from(["keel", "tui", "--run", "run-123"]);

        match cli.command {
            Some(Commands::Tui { run, .. }) => {
                assert_eq!(run.as_deref(), Some("run-123"));
            }
            _ => panic!("expected tui command"),
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
