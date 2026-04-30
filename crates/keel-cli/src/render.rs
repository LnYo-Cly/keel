use anyhow::Result;
use keel_core::{
    CommitResult, ConfigValidationReport, ConfigValidationSeverity, DiffInfo, DoctorReport,
    DoctorStatus, LogInfo, PrPlan, PrResult, PushResult, ReportInfo, RunMetadata,
};
use serde::Serialize;
use std::process::ExitCode;

pub(crate) fn print_run_created(label: &str, metadata: &RunMetadata) {
    println!("{label}: {}", metadata.run_id);
    println!("Status: {}", metadata.status);
    println!("Worktree: {}", metadata.worktree_path);
    println!("Report: {}/report.md", metadata.run_dir);
}

pub(crate) fn print_status(runs: &[RunMetadata], filtered: bool) {
    if runs.is_empty() {
        if filtered {
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

pub(crate) fn print_report(report: ReportInfo) {
    println!("Report: {}", report.path.display());
    println!("{}", report.summary);
    if let Some(commit) = &report.metadata.commit {
        println!("Commit:");
        println!("- SHA: {}", commit.commit_sha);
        println!("- Branch: {}", commit.branch);
        println!("- Message: {}", commit.commit_message);
        println!("- Committed at: {}", commit.committed_at);
    } else if report.metadata.committed {
        println!("Commit:");
        println!(
            "- SHA: {}",
            report.metadata.commit_sha.as_deref().unwrap_or("unknown")
        );
        println!(
            "- Message: {}",
            report
                .metadata
                .commit_message
                .as_deref()
                .unwrap_or("unknown")
        );
    }
    if let Some(push) = &report.metadata.push {
        println!("Push:");
        println!("- Remote: {}", push.remote);
        println!("- Remote URL: {}", push.remote_url);
        println!("- Branch: {}", push.branch);
        println!("- Commit: {}", push.commit_sha);
        println!("- Pushed at: {}", push.pushed_at);
    } else if report.metadata.pushed {
        println!("Push:");
        println!(
            "- Remote: {}",
            report.metadata.push_remote.as_deref().unwrap_or("unknown")
        );
        println!(
            "- Branch: {}",
            report
                .metadata
                .pushed_branch
                .as_deref()
                .unwrap_or("unknown")
        );
    }
    if let Some(pr) = &report.metadata.pr {
        println!("PR/MR:");
        println!("- Provider: {}", pr.provider_name);
        println!("- URL: {}", pr.url);
        println!("- Source branch: {}", pr.source_branch);
        println!("- Target branch: {}", pr.target_branch);
        println!("- Created at: {}", pr.created_at);
    } else if report.metadata.pr_created {
        println!("PR/MR:");
        println!(
            "- Provider: {}",
            report.metadata.pr_provider.as_deref().unwrap_or("unknown")
        );
        println!(
            "- URL: {}",
            report.metadata.pr_url.as_deref().unwrap_or("unknown")
        );
    }
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

pub(crate) fn print_commit_result(result: &CommitResult) {
    if result.already_committed {
        println!(
            "This run is already committed: {}",
            result.commit_sha.as_deref().unwrap_or("unknown")
        );
        println!("Branch: {}", result.branch);
        println!("Worktree: {}", result.worktree);
        return;
    }

    if result.dry_run {
        println!("Commit dry-run plan");
        println!("Run: {}", result.run_id);
        println!("Worktree: {}", result.worktree);
        println!("Branch: {}", result.branch);
        println!("Status: ready");
        println!("Commit message: {}", result.commit_message);
        println!("Would run: git add -A");
        println!("Would run: git commit -m {:?}", result.commit_message);
        print_warning_summary(&result.warnings);
        return;
    }

    println!(
        "Committed run {}: {}",
        result.run_id,
        result.commit_sha.as_deref().unwrap_or("unknown")
    );
    println!("Branch: {}", result.branch);
    println!("Worktree: {}", result.worktree);
    println!("Message: {}", result.commit_message);
    if let Some(commit_path) = &result.commit_path {
        println!("Commit artifact: {commit_path}");
    }
    println!("Keel did not push or merge anything.");
    print_warning_summary(&result.warnings);
}

pub(crate) fn print_push_result(result: &PushResult) {
    if result.already_pushed {
        println!(
            "This run is already pushed: {}/{}",
            result.remote, result.branch
        );
        println!("Remote URL: {}", result.remote_url);
        println!("Commit: {}", result.commit_sha);
        return;
    }

    if result.dry_run {
        println!("Push dry-run plan");
        println!("Run: {}", result.run_id);
        println!("Remote: {}", result.remote);
        println!("Remote URL: {}", result.remote_url);
        println!("Branch: {}", result.branch);
        println!("Commit: {}", result.commit_sha);
        println!("Would run: git push -u {} {}", result.remote, result.branch);
        return;
    }

    println!(
        "Pushed run {}: {}/{}",
        result.run_id, result.remote, result.branch
    );
    println!("Remote URL: {}", result.remote_url);
    println!("Commit: {}", result.commit_sha);
    if let Some(push_path) = &result.push_path {
        println!("Push artifact: {push_path}");
    }
    println!("Keel did not create a PR/MR.");
    println!("Keel did not merge anything.");
}

pub(crate) fn print_pr_plan(plan: &PrPlan) {
    println!("PR/MR manual dry-run plan");
    println!("Run: {}", plan.run_id);
    println!("Provider: {}", plan.provider_name);
    println!("Request kind: {}", plan.request_kind);
    println!("Remote: {}", plan.remote);
    println!("Remote URL: {}", plan.remote_url);
    if let Some(repository_url) = &plan.repository_url {
        println!("Repository URL: {repository_url}");
    }
    if let Some(web_url) = &plan.web_url {
        println!("Web URL: {web_url}");
    }
    println!("Source branch: {}", plan.source_branch);
    println!("Target branch: {}", plan.target_branch);
    println!("Commit: {}", plan.commit_sha);
    println!("Title: {}", plan.title);
    println!("Body:");
    println!("{}", plan.body);
    println!("Manual next steps:");
    for step in &plan.manual_steps {
        println!("- {step}");
    }
    println!("Keel did not create a PR/MR.");
    println!("Keel did not write pr.json.");
    println!("Keel did not merge anything.");
}

pub(crate) fn print_pr_result(result: &PrResult) {
    if result.already_created {
        println!(
            "This run already has a PR/MR: {}",
            result.url.as_deref().unwrap_or("unknown")
        );
        println!("Provider: {}", result.provider_name);
        println!("Source branch: {}", result.source_branch);
        println!("Target branch: {}", result.target_branch);
        return;
    }

    if result.dry_run {
        println!("PR/MR provider dry-run plan");
        println!("Run: {}", result.run_id);
        println!("Provider: {}", result.provider_name);
        println!("Request kind: {}", result.request_kind);
        println!("Remote: {}", result.remote);
        println!("Remote URL: {}", result.remote_url);
        if let Some(repository_url) = &result.repository_url {
            println!("Repository URL: {repository_url}");
        }
        if let Some(url) = &result.url {
            println!("Web URL: {url}");
        }
        println!("Source branch: {}", result.source_branch);
        println!("Target branch: {}", result.target_branch);
        println!("Commit: {}", result.commit_sha);
        println!("Title: {}", result.title);
        println!("Would run: {}", result.provider_command.join(" "));
        println!("Keel would create a PR/MR through the provider CLI.");
        println!("Keel would not write pr.json during dry-run.");
        println!("Keel would not merge anything.");
        return;
    }

    println!("Created {} for run {}", result.request_kind, result.run_id);
    println!("Provider: {}", result.provider_name);
    if let Some(url) = &result.url {
        println!("URL: {url}");
    }
    println!("Source branch: {}", result.source_branch);
    println!("Target branch: {}", result.target_branch);
    println!("Commit: {}", result.commit_sha);
    if let Some(pr_path) = &result.pr_path {
        println!("PR/MR artifact: {pr_path}");
    }
    println!("Keel did not merge anything.");
}

pub(crate) fn print_diff(run_id: &str, diff: &DiffInfo) {
    println!("Diff: {}", diff.path.display());
    print_text_artifact(&diff.content, diff.is_empty, || {
        println!("Diff for run `{run_id}` is empty.");
    });
}

pub(crate) fn print_log(run_id: &str, log: &LogInfo) {
    println!("Log: {}", log.path.display());
    print_text_artifact(&log.content, log.is_empty, || {
        println!("Log for run `{run_id}` is empty.");
    });
}

pub(crate) fn print_discarded_run(metadata: &RunMetadata) {
    println!("Discarded run: {}", metadata.run_id);
    println!("Status: {}", metadata.status);
    println!("History preserved at: {}", metadata.run_dir);
}

pub(crate) fn print_warning_summary(warnings: &[String]) {
    if warnings.is_empty() {
        println!("Warnings: none");
        return;
    }

    println!("Warnings:");
    for warning in warnings {
        println!("- {warning}");
    }
}

fn print_text_artifact<F>(content: &str, is_empty: bool, print_empty: F)
where
    F: FnOnce(),
{
    if is_empty {
        print_empty();
        return;
    }

    print!("{content}");
    if !content.ends_with('\n') {
        println!();
    }
}

pub(crate) fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

pub(crate) fn print_doctor(report: &DoctorReport) {
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

pub(crate) fn print_config_validation(report: &ConfigValidationReport) {
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

pub(crate) fn exit_code_for_config_report(report: &ConfigValidationReport) -> ExitCode {
    if report.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

pub(crate) fn exit_code_for_report(report: &DoctorReport) -> ExitCode {
    if report.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
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
