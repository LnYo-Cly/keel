use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use keel_core::{
    run_doctor, validate_config, ArtifactInfo, ConfigValidationReport, ConfigValidationSeverity,
    DoctorReport, DoctorStatus, KeelProject, ReportInfo, RiskWarning, RunMetadata, RunStatus,
};
use serde::Serialize;
use std::path::Path;
use std::process::ExitCode;

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
            print_json(&report)?;
        } else {
            print_doctor(&report);
        }
        return Ok(exit_code_for_report(&report));
    }

    let project = KeelProject::discover_from_current_dir()?;

    match cli.command {
        Commands::Doctor { .. } => unreachable!("doctor is handled before project discovery"),
        Commands::Config {
            command: ConfigCommands::Validate { json },
        } => {
            let report = validate_config(project.root());
            if json {
                print_json(&report)?;
            } else {
                print_config_validation(&report);
            }
            return Ok(exit_code_for_config_report(&report));
        }
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
        Commands::Status {
            agent,
            status,
            limit,
            json,
        } => {
            let runs = filtered_runs(project.list_runs()?, agent.as_deref(), status, limit);
            if json {
                print_json(&status_json(&runs))?;
            } else {
                print_status(&runs, agent.as_deref(), status);
            }
        }
        Commands::Report { run_id, json } => {
            let report = project.report(&run_id)?;
            if json {
                print_json(&report_json(&report))?;
            } else {
                print_report(report);
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

fn print_report(report: ReportInfo) {
    println!("Report: {}", report.path.display());
    println!("{}", report.summary);
    if !report.metadata.warnings.is_empty() {
        println!("Warnings:");
        for warning in &report.metadata.warnings {
            println!("- {warning}");
        }
    }
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

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn print_doctor(report: &DoctorReport) {
    println!("Keel doctor");
    for group in ["Repository", "Keel", "Agents"] {
        println!();
        println!("{group}");
        for check in report.checks.iter().filter(|check| check.group == group) {
            let details = check
                .details
                .as_deref()
                .map(|details| format!(": {details}"))
                .unwrap_or_default();
            println!(
                "  {} {}{}",
                doctor_status_marker(check.status),
                check.message,
                details
            );
        }
    }

    println!();
    println!("Summary");
    println!(
        "  {} ok, {} warnings, {} errors",
        report.summary.ok, report.summary.warnings, report.summary.errors
    );
}

fn print_config_validation(report: &ConfigValidationReport) {
    println!("Keel config validation");
    println!();
    println!("Config");
    for issue in &report.issues {
        let details = issue
            .details
            .as_deref()
            .map(|details| format!(": {details}"))
            .unwrap_or_default();
        println!(
            "  {} {}{}",
            config_status_marker(issue.severity),
            issue.message,
            details
        );
    }

    println!();
    println!("Summary");
    println!(
        "  {} ok, {} warnings, {} errors",
        report.summary.ok, report.summary.warnings, report.summary.errors
    );
}

fn config_status_marker(status: ConfigValidationSeverity) -> &'static str {
    match status {
        ConfigValidationSeverity::Ok => "✅",
        ConfigValidationSeverity::Warning => "⚠️",
        ConfigValidationSeverity::Error => "❌",
    }
}

fn doctor_status_marker(status: DoctorStatus) -> &'static str {
    match status {
        DoctorStatus::Ok => "✅",
        DoctorStatus::Warning => "⚠️",
        DoctorStatus::Error => "❌",
    }
}

fn exit_code_for_config_report(report: &ConfigValidationReport) -> ExitCode {
    if report.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn exit_code_for_report(report: &DoctorReport) -> ExitCode {
    if report.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn status_json(runs: &[RunMetadata]) -> Vec<RunSummaryJson> {
    runs.iter().map(RunSummaryJson::from).collect()
}

fn report_json(report: &ReportInfo) -> ReportJson {
    ReportJson {
        run_id: report.metadata.run_id.clone(),
        parent_run_id: report.metadata.parent_run_id.clone(),
        task: report.metadata.task.clone(),
        agent: report.metadata.agent.clone(),
        status: report.metadata.status.to_string(),
        created_at: report.metadata.created_at.clone(),
        worktree: report.metadata.worktree_path.clone(),
        branch: report.metadata.branch.clone(),
        base_commit: report.metadata.base_commit.clone(),
        failure_reason: report
            .metadata
            .failure_reason
            .as_ref()
            .map(ToString::to_string),
        readiness_reason: report.metadata.readiness_reason.clone(),
        warnings: report.metadata.warnings.clone(),
        risk_warnings: report.metadata.risk_warnings.clone(),
        artifacts: ArtifactSetJson::from_artifacts(&report.artifacts),
        next_actions: report.next_actions.clone(),
    }
}

#[derive(Debug, Serialize)]
struct RunSummaryJson {
    run_id: String,
    parent_run_id: Option<String>,
    task: String,
    agent: String,
    status: String,
    created_at: String,
    worktree: String,
    branch: String,
    failure_reason: Option<String>,
}

impl From<&RunMetadata> for RunSummaryJson {
    fn from(metadata: &RunMetadata) -> Self {
        Self {
            run_id: metadata.run_id.clone(),
            parent_run_id: metadata.parent_run_id.clone(),
            task: metadata.task.clone(),
            agent: metadata.agent.clone(),
            status: metadata.status.to_string(),
            created_at: metadata.created_at.clone(),
            worktree: metadata.worktree_path.clone(),
            branch: metadata.branch.clone(),
            failure_reason: metadata.failure_reason.as_ref().map(ToString::to_string),
        }
    }
}

#[derive(Debug, Serialize)]
struct ReportJson {
    run_id: String,
    parent_run_id: Option<String>,
    task: String,
    agent: String,
    status: String,
    created_at: String,
    worktree: String,
    branch: String,
    base_commit: String,
    failure_reason: Option<String>,
    readiness_reason: String,
    warnings: Vec<String>,
    risk_warnings: Vec<RiskWarning>,
    artifacts: ArtifactSetJson,
    next_actions: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ArtifactSetJson {
    metadata: ArtifactJson,
    log: ArtifactJson,
    diff: ArtifactJson,
    checks: ArtifactJson,
    report: ArtifactJson,
}

impl ArtifactSetJson {
    fn from_artifacts(artifacts: &[ArtifactInfo]) -> Self {
        Self {
            metadata: artifact_json(artifacts, "Metadata"),
            log: artifact_json(artifacts, "Log"),
            diff: artifact_json(artifacts, "Diff"),
            checks: artifact_json(artifacts, "Checks"),
            report: artifact_json(artifacts, "Report"),
        }
    }
}

#[derive(Debug, Serialize)]
struct ArtifactJson {
    path: String,
    exists: bool,
    state: &'static str,
}

fn artifact_json(artifacts: &[ArtifactInfo], label: &str) -> ArtifactJson {
    artifacts
        .iter()
        .find(|artifact| artifact.label == label)
        .map(|artifact| ArtifactJson {
            path: path_string(&artifact.path),
            exists: artifact.exists,
            state: if artifact.exists {
                "present"
            } else {
                "missing"
            },
        })
        .unwrap_or_else(|| ArtifactJson {
            path: String::new(),
            exists: false,
            state: "missing",
        })
}

fn path_string(path: &Path) -> String {
    path.display().to_string()
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
        }
    }
}
