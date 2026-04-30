use crate::command::{exit_code_text, format_command_line};
use crate::constants::{
    CHECKS_FILE, COMMIT_FILE, DIFF_FILE, LOG_FILE, METADATA_FILE, PUBLISH_FILE, REPORT_FILE,
    REPORT_OUTPUT_LIMIT,
};
use crate::model::{CheckResult, RunMetadata, RunStatus};

pub(crate) fn render_report(
    metadata: &RunMetadata,
    checks: &[CheckResult],
    diff: &str,
    failure: Option<&str>,
    agent_stdout: &str,
    agent_stderr: &str,
) -> String {
    let failure_reason = metadata
        .failure_reason
        .as_ref()
        .map_or_else(|| "none".to_string(), ToString::to_string);
    let parent_run_id = metadata.parent_run_id.as_deref().unwrap_or("none");
    let duration = metadata
        .duration_ms
        .map_or_else(|| "n/a".to_string(), |duration| duration.to_string());
    let agent_command = format_command_line(&metadata.agent_command);
    let agent_stdout = summarize_report_output(agent_stdout);
    let agent_stderr = summarize_report_output(agent_stderr);

    format!(
        "# Keel Run Report\n\n\
         ## Summary\n\n\
          - Run ID: `{}`\n\
          - Parent Run ID: `{}`\n\
          - Task: {}\n\
          - Agent: `{}`\n\
          - Status: `{}`\n\
         - Created At: `{}`\n\
         - Updated At: `{}`\n\
         - Worktree: `{}`\n\
         - Branch: `{}`\n\
         - Base Commit: `{}`\n\
         - Agent Command: `{}`\n\
         - Agent Exit Code: `{}`\n\
         - Failure Reason: `{}`\n\
         - Duration Ms: `{}`\n\n\
         ## Readiness\n\n\
         - {}\n\n\
         ## Warnings\n\n\
         {}\
         {}\
         {}\
         {}\
         ## Agent Output\n\n\
         ### Stdout\n\n\
         ```text\n{}\
         ```\n\n\
         ### Stderr\n\n\
         ```text\n{}\
         ```\n\n\
         ## Checks\n\n\
         | Name | Status | Exit | Command |\n\
          | --- | --- | --- | --- |\n\
          {}\
          ## Artifacts\n\n\
          {}\
          ## Suggested Next Actions\n\n\
          {}\
          ## Diff\n\n\
          ```diff\n{}\
          ```\n",
        metadata.run_id,
        parent_run_id,
        metadata.task,
        metadata.agent,
        metadata.status,
        metadata.created_at,
        metadata.updated_at,
        metadata.worktree_path,
        metadata.branch,
        metadata.base_commit,
        agent_command,
        exit_code_text(metadata.exit_code),
        failure_reason,
        duration,
        metadata.readiness_reason,
        render_warnings(&metadata.warnings),
        render_commit_section(metadata),
        render_publish_section(metadata),
        render_failure_section(failure),
        agent_stdout,
        agent_stderr,
        render_checks_table(checks),
        render_artifacts(),
        render_suggested_next_actions(metadata),
        diff
    )
}

fn summarize_report_output(output: &str) -> String {
    if output.trim().is_empty() {
        return "(empty)\n".to_string();
    }

    if output.len() <= REPORT_OUTPUT_LIMIT {
        return output.trim_end().to_string() + "\n";
    }

    let mut summary = output.chars().take(REPORT_OUTPUT_LIMIT).collect::<String>();
    summary.push_str("\n... output truncated ...\n");
    summary
}

fn render_warnings(warnings: &[String]) -> String {
    if warnings.is_empty() {
        return "- none\n".to_string();
    }

    warnings
        .iter()
        .map(|warning| format!("- {warning}\n"))
        .collect()
}

fn render_failure_section(failure: Option<&str>) -> String {
    failure.map_or_else(String::new, |message| {
        format!("## Failure\n\n- {message}\n\n")
    })
}

fn render_checks_table(checks: &[CheckResult]) -> String {
    checks
        .iter()
        .map(|check| {
            format!(
                "| {} | {} | {} | `{}` |\n",
                check.name,
                check.status,
                exit_code_text(check.exit_code),
                check.command
            )
        })
        .collect()
}

fn render_artifacts() -> String {
    [
        ("Metadata", METADATA_FILE),
        ("Log", LOG_FILE),
        ("Diff", DIFF_FILE),
        ("Checks", CHECKS_FILE),
        ("Report", REPORT_FILE),
        ("Commit", COMMIT_FILE),
        ("Publish", PUBLISH_FILE),
    ]
    .iter()
    .map(|(label, file)| format!("- {label}: `{file}`\n"))
    .collect()
}

pub(crate) fn render_commit_section(metadata: &RunMetadata) -> String {
    if !metadata.committed {
        return String::new();
    }

    let commit_sha = metadata.commit_sha.as_deref().unwrap_or("unknown");
    let commit_message = metadata.commit_message.as_deref().unwrap_or("unknown");
    let committed_at = metadata.committed_at.as_deref().unwrap_or("unknown");
    let warnings = if metadata.warnings.is_empty() {
        "- none\n".to_string()
    } else {
        metadata
            .warnings
            .iter()
            .map(|warning| format!("- {warning}\n"))
            .collect()
    };

    format!(
        "## Commit\n\n\
         - Commit: `{commit_sha}`\n\
         - Branch: `{}`\n\
         - Message: `{commit_message}`\n\
         - Committed at: `{committed_at}`\n\n\
         ### Warnings\n\n\
         {}\
         ### Next\n\n\
         - You can publish this branch later with future `keel publish`.\n\
         - Keel did not push or merge anything.\n\n",
        metadata.branch, warnings
    )
}

pub(crate) fn render_publish_section(metadata: &RunMetadata) -> String {
    if !metadata.published {
        return String::new();
    }

    let remote = metadata.publish_remote.as_deref().unwrap_or("unknown");
    let remote_url = metadata.publish_remote_url.as_deref().unwrap_or("unknown");
    let branch = metadata
        .published_branch
        .as_deref()
        .unwrap_or(&metadata.branch);
    let commit_sha = metadata.commit_sha.as_deref().unwrap_or("unknown");
    let published_at = metadata.published_at.as_deref().unwrap_or("unknown");

    format!(
        "## Publish\n\n\
         - Remote: `{remote}`\n\
         - Remote URL: `{remote_url}`\n\
         - Branch: `{branch}`\n\
         - Commit: `{commit_sha}`\n\
         - Published at: `{published_at}`\n\n\
         ### Next\n\n\
         - Open a pull/merge request on your Git hosting provider.\n\
         - Keel did not create a PR/MR.\n\
         - Keel did not merge anything.\n\n"
    )
}

fn render_suggested_next_actions(metadata: &RunMetadata) -> String {
    match metadata.status {
        RunStatus::Ready => format!(
            "- Review `{}` and `{}` before making any merge decision.\n- Use `keel discard {}` to remove the candidate worktree and preserve history.\n- Use `keel rerun {}` to try the same task again in a fresh worktree.\n\n",
            DIFF_FILE, REPORT_FILE, metadata.run_id, metadata.run_id
        ),
        RunStatus::NotReady => format!(
            "- Inspect `{}` and `{}` to understand why the candidate is not ready.\n- Use `keel rerun {}` after fixing environment or task issues.\n- Use `keel discard {}` if the candidate worktree is no longer useful.\n\n",
            LOG_FILE, CHECKS_FILE, metadata.run_id, metadata.run_id
        ),
        RunStatus::Discarded => format!(
            "- Run history is preserved under `{}`.\n- Use `keel rerun {}` to create a fresh candidate from the same task.\n\n",
            metadata.run_dir, metadata.run_id
        ),
        RunStatus::Created | RunStatus::Running => {
            "- Wait for the run to finish, then check `keel status` and inspect this report again.\n\n"
                .to_string()
        }
    }
}
