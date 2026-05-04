use crate::command::{exit_code_text, format_command_line};
use crate::commit::CommitArtifact;
use crate::constants::{
    CHECKS_FILE, COMMIT_FILE, DIFF_FILE, LOG_FILE, METADATA_FILE, PR_FILE, PUSH_FILE, REPORT_FILE,
    REPORT_OUTPUT_LIMIT,
};
use crate::model::{CheckResult, RunMetadata, RunStatus};
use crate::pr::{infer_provider, PrArtifact, PrProvider};
use crate::push::PushArtifact;

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
        render_markdown_list(&metadata.warnings, "- none"),
        render_commit_section(metadata),
        render_push_section(metadata),
        render_pr_section(metadata),
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
        ("Push", PUSH_FILE),
        ("PR/MR", PR_FILE),
    ]
    .iter()
    .map(|(label, file)| format!("- {label}: `{file}`\n"))
    .collect()
}

pub(crate) fn render_commit_section(metadata: &RunMetadata) -> String {
    let Some(commit) = CommitArtifact::from_metadata(metadata) else {
        return String::new();
    };
    let warnings = render_markdown_list(&metadata.warnings, "- none");

    format!(
        "## Commit\n\n\
         - Commit: `{}`\n\
         - Branch: `{}`\n\
         - Message: `{}`\n\
         - Committed at: `{}`\n\n\
         ### Warnings\n\n\
         {}\
         ### Next\n\n\
         - Use `keel push {}` when you want to push this candidate branch.\n\
         - Keel did not push or merge anything.\n\n",
        commit.commit_sha,
        commit.branch,
        commit.commit_message,
        commit.committed_at,
        warnings,
        metadata.run_id
    )
}

pub(crate) fn render_markdown_list(items: &[String], empty: &str) -> String {
    if items.is_empty() {
        return format!("{empty}\n");
    }

    items.iter().map(|item| format!("- {item}\n")).collect()
}

pub(crate) fn render_push_section(metadata: &RunMetadata) -> String {
    let Some(push) = PushArtifact::from_metadata(metadata) else {
        return String::new();
    };

    format!(
        "## Push\n\n\
         - Remote: `{}`\n\
         - Remote URL: `{}`\n\
         - Branch: `{}`\n\
         - Commit: `{}`\n\
         - Pushed at: `{}`\n\n\
         ### Next\n\n\
         - Use `keel pr {} --manual --dry-run` to prepare a Pull Request or Merge Request.\n\
         - Keel did not create a PR/MR.\n\
         - Keel did not merge anything.\n\n",
        push.remote, push.remote_url, push.branch, push.commit_sha, push.pushed_at, metadata.run_id
    )
}

pub(crate) fn render_pr_section(metadata: &RunMetadata) -> String {
    let Some(pr) = PrArtifact::from_metadata(metadata).ok().flatten() else {
        return String::new();
    };
    let draft = if pr.draft { "yes" } else { "no" };
    let reused_existing = if pr.reused_existing { "yes" } else { "no" };

    format!(
        "## PR/MR\n\n\
         - Provider: `{}`\n\
         - URL: `{}`\n\
         - Source branch: `{}`\n\
         - Target branch: `{}`\n\
         - Commit: `{}`\n\
         - Draft: `{draft}`\n\
         - Reused existing: `{reused_existing}`\n\
         - Created at: `{}`\n\n\
         ### Next\n\n\
         - Review this request on the provider before merging.\n\
         - Keel did not merge anything.\n\n",
        pr.provider_name, pr.url, pr.source_branch, pr.target_branch, pr.commit_sha, pr.created_at
    )
}

fn render_suggested_next_actions(metadata: &RunMetadata) -> String {
    suggested_next_actions(metadata)
        .into_iter()
        .map(|action| format!("- {}\n", action.command))
        .collect::<String>()
        + "\n"
}

pub fn suggested_next_actions(metadata: &RunMetadata) -> Vec<ReviewNextAction> {
    let run_id = &metadata.run_id;
    match metadata.status {
        RunStatus::Ready => ready_next_actions(metadata),
        RunStatus::NotReady => vec![
            ReviewNextAction::new(format!("keel log {run_id}"), ReviewNextActionKind::Inspect),
            ReviewNextAction::new(format!("keel diff {run_id}"), ReviewNextActionKind::Inspect),
            ReviewNextAction::new(format!("keel rerun {run_id}"), ReviewNextActionKind::Rerun),
            ReviewNextAction::new(
                format!("keel discard {run_id}"),
                ReviewNextActionKind::Discard,
            ),
        ],
        RunStatus::Discarded => vec![
            ReviewNextAction::new(
                format!("keel report {run_id}"),
                ReviewNextActionKind::Inspect,
            ),
            ReviewNextAction::new(format!("keel diff {run_id}"), ReviewNextActionKind::Inspect),
            ReviewNextAction::new(format!("keel log {run_id}"), ReviewNextActionKind::Inspect),
            ReviewNextAction::new(format!("keel rerun {run_id}"), ReviewNextActionKind::Rerun),
        ],
        RunStatus::Created | RunStatus::Running => vec![
            ReviewNextAction::new("keel status", ReviewNextActionKind::Wait),
            ReviewNextAction::new(format!("keel log {run_id}"), ReviewNextActionKind::Inspect),
        ],
    }
}

pub fn primary_next_action(metadata: &RunMetadata) -> Option<ReviewNextAction> {
    let actions = suggested_next_actions(metadata);
    [
        ReviewNextActionKind::Commit,
        ReviewNextActionKind::Push,
        ReviewNextActionKind::ProviderPr,
        ReviewNextActionKind::ManualPr,
        ReviewNextActionKind::ReviewProvider,
        ReviewNextActionKind::Wait,
        ReviewNextActionKind::Inspect,
        ReviewNextActionKind::Rerun,
        ReviewNextActionKind::Discard,
    ]
    .into_iter()
    .find_map(|kind| actions.iter().find(|action| action.kind == kind).cloned())
}

fn ready_next_actions(metadata: &RunMetadata) -> Vec<ReviewNextAction> {
    let run_id = &metadata.run_id;
    let mut actions = vec![
        ReviewNextAction::new(format!("keel diff {run_id}"), ReviewNextActionKind::Inspect),
        ReviewNextAction::new(format!("keel log {run_id}"), ReviewNextActionKind::Inspect),
    ];

    if !metadata.committed {
        actions.push(ReviewNextAction::new(
            format!("keel commit {run_id} --dry-run"),
            ReviewNextActionKind::Commit,
        ));
        actions.push(ReviewNextAction::new(
            format!("keel commit {run_id}"),
            ReviewNextActionKind::Commit,
        ));
    } else if !metadata.pushed {
        actions.push(ReviewNextAction::new(
            format!("keel push {run_id} --dry-run"),
            ReviewNextActionKind::Push,
        ));
        actions.push(ReviewNextAction::new(
            format!("keel push {run_id}"),
            ReviewNextActionKind::Push,
        ));
    } else if !metadata.pr_created {
        actions.push(ReviewNextAction::new(
            format!("keel pr {run_id} --manual --dry-run"),
            ReviewNextActionKind::ManualPr,
        ));
        if pushed_to_github(metadata) {
            actions.push(ReviewNextAction::new(
                format!("keel pr {run_id} --provider github --dry-run"),
                ReviewNextActionKind::ProviderPr,
            ));
            actions.push(ReviewNextAction::new(
                format!("keel pr {run_id} --provider github"),
                ReviewNextActionKind::ProviderPr,
            ));
        }
    } else {
        actions.push(ReviewNextAction::new(
            metadata
                .pr_url
                .as_deref()
                .map(|url| format!("review PR/MR on provider: {url}"))
                .unwrap_or_else(|| "review PR/MR on provider before merging".to_string()),
            ReviewNextActionKind::ReviewProvider,
        ));
    }

    actions.push(ReviewNextAction::new(
        format!("keel rerun {run_id}"),
        ReviewNextActionKind::Rerun,
    ));
    actions.push(ReviewNextAction::new(
        format!("keel discard {run_id}"),
        ReviewNextActionKind::Discard,
    ));
    actions
}

fn pushed_to_github(metadata: &RunMetadata) -> bool {
    let Some(remote_url) = metadata.push_remote_url.as_deref() else {
        return false;
    };
    infer_provider(remote_url) == Some(PrProvider::Github)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewNextAction {
    pub command: String,
    pub kind: ReviewNextActionKind,
}

impl ReviewNextAction {
    fn new(command: impl Into<String>, kind: ReviewNextActionKind) -> Self {
        Self {
            command: command.into(),
            kind,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewNextActionKind {
    Inspect,
    Commit,
    Push,
    ManualPr,
    ProviderPr,
    ReviewProvider,
    Rerun,
    Discard,
    Wait,
}
