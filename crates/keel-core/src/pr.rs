use crate::command::run_command;
use crate::commit::default_commit_message;
use crate::constants::{
    CHECKS_FILE, COMMIT_FILE, DIFF_FILE, LOG_FILE, METADATA_FILE, PR_FILE, PUSH_FILE, REPORT_FILE,
};
use crate::model::{RunMetadata, RunStatus};
use crate::push::PushArtifact;
use crate::time::now_timestamp;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::str::FromStr;

mod provider;
pub use provider::infer_provider;
use provider::{find_existing_provider_pr, ExistingProviderPr};
use provider::{
    provider_command, provider_command_display, provider_pr_web_url, repository_web_url,
    run_provider_command,
};

#[derive(Debug, Clone)]
pub struct PrOptions {
    pub manual: bool,
    pub dry_run: bool,
    pub draft: bool,
    pub provider: Option<PrProvider>,
    pub base: Option<String>,
    pub head: Option<String>,
    pub target: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrProvider {
    Github,
    Gitlab,
    Gitee,
    Gitea,
}

impl PrProvider {
    pub fn request_kind(self) -> &'static str {
        match self {
            Self::Github | Self::Gitee | Self::Gitea => "pull_request",
            Self::Gitlab => "merge_request",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Github => "GitHub",
            Self::Gitlab => "GitLab",
            Self::Gitee => "Gitee",
            Self::Gitea => "Gitea",
        }
    }
}

impl std::fmt::Display for PrProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Github => "github",
            Self::Gitlab => "gitlab",
            Self::Gitee => "gitee",
            Self::Gitea => "gitea",
        };
        f.write_str(value)
    }
}

impl FromStr for PrProvider {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "github" => Ok(Self::Github),
            "gitlab" => Ok(Self::Gitlab),
            "gitee" => Ok(Self::Gitee),
            "gitea" => Ok(Self::Gitea),
            other => bail!(
                "unsupported PR provider `{other}`; supported providers: github, gitlab, gitee, gitea"
            ),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PrPlan {
    pub run_id: String,
    pub provider: PrProvider,
    pub provider_name: &'static str,
    pub request_kind: &'static str,
    pub manual: bool,
    pub dry_run: bool,
    pub remote: String,
    pub remote_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_url: Option<String>,
    pub source_branch: String,
    pub target_branch: String,
    pub commit_sha: String,
    pub title: String,
    pub body: String,
    pub copyable_summary: String,
    pub draft: bool,
    pub artifacts: PrArtifactPaths,
    pub manual_steps: Vec<String>,
    pub would_create_request: bool,
    pub would_write_artifact: bool,
    pub would_push: bool,
    pub would_merge: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrArtifact {
    pub run_id: String,
    pub provider: PrProvider,
    pub provider_name: String,
    pub request_kind: String,
    pub remote: String,
    pub remote_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_url: Option<String>,
    pub source_branch: String,
    pub target_branch: String,
    pub commit_sha: String,
    pub title: String,
    pub url: String,
    pub created_at: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub reused_existing: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrResult {
    pub run_id: String,
    pub provider: PrProvider,
    pub provider_name: &'static str,
    pub request_kind: &'static str,
    pub remote: String,
    pub remote_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_url: Option<String>,
    pub source_branch: String,
    pub target_branch: String,
    pub commit_sha: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub draft: bool,
    pub manual: bool,
    pub dry_run: bool,
    pub created: bool,
    pub already_created: bool,
    pub reused_existing: bool,
    pub would_create_request: bool,
    pub would_write_artifact: bool,
    pub would_push: bool,
    pub would_merge: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_path: Option<String>,
    pub provider_command: Vec<String>,
    pub provider_command_display: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrArtifactPaths {
    pub metadata: String,
    pub log: String,
    pub diff: String,
    pub checks: String,
    pub report: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr: Option<String>,
}

pub(crate) fn plan_pr(root: &Path, metadata: &RunMetadata, options: PrOptions) -> Result<PrPlan> {
    validate_manual_plan_mode(&options)?;
    build_pr_plan(root, metadata, &options, true)
}

pub(crate) fn create_pr(
    root: &Path,
    run_dir: &Path,
    metadata: &mut RunMetadata,
    options: PrOptions,
) -> Result<PrResult> {
    validate_create_mode(&options)?;
    let pr_path = run_dir.join(PR_FILE);

    if let Some(existing) = existing_pr(metadata, &pr_path)? {
        let plan = build_pr_plan(root, metadata, &options, false)?;
        return Ok(already_created_result(
            &plan,
            existing,
            options.dry_run,
            &pr_path,
        ));
    }

    let plan = build_pr_plan(root, metadata, &options, false)?;
    let provider_command = provider_command(&plan)?;
    let provider_command_display = provider_command_display(&provider_command);

    if options.dry_run {
        return Ok(PrResult {
            run_id: plan.run_id,
            provider: plan.provider,
            provider_name: plan.provider_name,
            request_kind: plan.request_kind,
            remote: plan.remote,
            remote_url: plan.remote_url,
            repository_url: plan.repository_url,
            source_branch: plan.source_branch,
            target_branch: plan.target_branch,
            commit_sha: plan.commit_sha,
            title: plan.title,
            url: plan.web_url,
            draft: plan.draft,
            manual: false,
            dry_run: true,
            created: false,
            already_created: false,
            reused_existing: false,
            would_create_request: true,
            would_write_artifact: false,
            would_push: false,
            would_merge: false,
            provider_command_display,
            pr_path: None,
            provider_command,
        });
    }

    if let Some(existing) = find_existing_provider_pr(root, &plan)? {
        let (artifact, result) = reused_existing_result(
            &plan,
            existing,
            &created_at_now(),
            &pr_path,
            provider_command,
        );
        write_pr_artifact_and_metadata(&pr_path, metadata, &artifact)?;
        return Ok(result);
    }

    let url = run_provider_command(root, &plan, &provider_command)?;
    let created_at = now_timestamp();
    let artifact = PrArtifact {
        run_id: plan.run_id.clone(),
        provider: plan.provider,
        provider_name: plan.provider_name.to_string(),
        request_kind: plan.request_kind.to_string(),
        remote: plan.remote.clone(),
        remote_url: plan.remote_url.clone(),
        repository_url: plan.repository_url.clone(),
        source_branch: plan.source_branch.clone(),
        target_branch: plan.target_branch.clone(),
        commit_sha: plan.commit_sha.clone(),
        title: plan.title.clone(),
        url: url.clone(),
        created_at: created_at.clone(),
        draft: plan.draft,
        reused_existing: false,
        dry_run: false,
    };

    write_pr_artifact_and_metadata(&pr_path, metadata, &artifact)?;

    Ok(PrResult {
        run_id: plan.run_id,
        provider: plan.provider,
        provider_name: plan.provider_name,
        request_kind: plan.request_kind,
        remote: plan.remote,
        remote_url: plan.remote_url,
        repository_url: plan.repository_url,
        source_branch: plan.source_branch,
        target_branch: plan.target_branch,
        commit_sha: plan.commit_sha,
        title: plan.title,
        url: Some(url),
        draft: plan.draft,
        manual: false,
        dry_run: false,
        created: true,
        already_created: false,
        reused_existing: false,
        would_create_request: false,
        would_write_artifact: false,
        would_push: false,
        would_merge: false,
        pr_path: Some(pr_path.display().to_string()),
        provider_command_display,
        provider_command,
    })
}

fn build_pr_plan(
    root: &Path,
    metadata: &RunMetadata,
    options: &PrOptions,
    manual: bool,
) -> Result<PrPlan> {
    validate_pr_status(metadata)?;

    let commit_sha = committed_sha(metadata)?;
    let push = pushed_candidate(metadata)?;
    validate_push_matches_run(metadata, &commit_sha, &push)?;

    let provider = match options.provider {
        Some(provider) => provider,
        None => infer_provider(&push.remote_url).ok_or_else(|| {
            anyhow::anyhow!(
                "could not infer PR provider from remote URL `{}`; pass `--provider <provider>` or use a supported Git hosting remote",
                push.remote_url
            )
        })?,
    };
    let target_branch = options
        .base
        .as_deref()
        .or(options.target.as_deref())
        .filter(|branch| !branch.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| default_target_branch(root));
    let source_branch = options
        .head
        .as_deref()
        .filter(|branch| !branch.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| push.branch.clone());
    let title = options
        .title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| default_commit_message(metadata));
    let artifacts = artifact_paths(metadata);
    let body = default_body(
        metadata,
        &source_branch,
        &target_branch,
        &commit_sha,
        &artifacts,
    );
    let copyable_summary = copyable_summary(metadata, &source_branch, &target_branch, &commit_sha);
    let repository_url = repository_web_url(&push.remote_url);
    let web_url = repository_url.as_deref().map(|url| {
        provider_pr_web_url(provider, url, &source_branch, &target_branch, &title, &body)
    });
    let manual_steps = manual_steps(
        provider,
        &source_branch,
        &target_branch,
        repository_url.as_deref(),
        web_url.as_deref(),
    );

    Ok(PrPlan {
        run_id: metadata.run_id.clone(),
        provider,
        provider_name: provider.display_name(),
        request_kind: provider.request_kind(),
        manual,
        dry_run: options.dry_run,
        remote: push.remote,
        remote_url: push.remote_url,
        repository_url,
        web_url,
        source_branch,
        target_branch,
        commit_sha,
        title,
        body,
        copyable_summary,
        draft: options.draft,
        artifacts,
        manual_steps,
        would_create_request: !manual,
        would_write_artifact: !manual && !options.dry_run,
        would_push: false,
        would_merge: false,
    })
}

fn validate_manual_plan_mode(options: &PrOptions) -> Result<()> {
    if !options.manual || !options.dry_run {
        bail!("manual PR/MR planning requires `--manual --dry-run`");
    }
    Ok(())
}

fn validate_create_mode(options: &PrOptions) -> Result<()> {
    if options.manual && !options.dry_run {
        bail!("manual PR/MR planning requires `--manual --dry-run`");
    }
    Ok(())
}

fn validate_pr_status(metadata: &RunMetadata) -> Result<()> {
    if metadata.status != RunStatus::Ready {
        bail!(
            "run `{}` has status `{}`; only ready runs can create a PR/MR plan",
            metadata.run_id,
            metadata.status
        );
    }
    Ok(())
}

fn committed_sha(metadata: &RunMetadata) -> Result<String> {
    if let Some(commit_sha) = &metadata.commit_sha {
        if !commit_sha.trim().is_empty() {
            return Ok(commit_sha.clone());
        }
    }

    if let Some(commit) = &metadata.commit {
        if !commit.commit_sha.trim().is_empty() {
            return Ok(commit.commit_sha.clone());
        }
    }

    bail!(
        "run `{}` is not committed; run `keel commit {}` first",
        metadata.run_id,
        metadata.run_id
    )
}

fn pushed_candidate(metadata: &RunMetadata) -> Result<PushArtifact> {
    if let Some(push) = &metadata.push {
        return Ok(push.clone());
    }

    if metadata.pushed {
        if let (Some(pushed_at), Some(remote), Some(remote_url), Some(branch), Some(commit_sha)) = (
            metadata.pushed_at.clone(),
            metadata.push_remote.clone(),
            metadata.push_remote_url.clone(),
            metadata.pushed_branch.clone(),
            metadata.commit_sha.clone(),
        ) {
            return Ok(PushArtifact {
                run_id: metadata.run_id.clone(),
                remote,
                remote_url,
                branch,
                commit_sha,
                pushed: true,
                pushed_at,
                dry_run: false,
            });
        }
    }

    bail!(
        "run `{}` is not pushed; run `keel push {}` first",
        metadata.run_id,
        metadata.run_id
    )
}

fn validate_push_matches_run(
    metadata: &RunMetadata,
    commit_sha: &str,
    push: &PushArtifact,
) -> Result<()> {
    if push.branch != metadata.branch {
        bail!(
            "pushed branch `{}` does not match candidate branch `{}` for run `{}`",
            push.branch,
            metadata.branch,
            metadata.run_id
        );
    }

    if push.commit_sha != commit_sha {
        bail!(
            "pushed commit `{}` does not match committed run SHA `{}` for run `{}`",
            push.commit_sha,
            commit_sha,
            metadata.run_id
        );
    }

    Ok(())
}

fn existing_pr(metadata: &RunMetadata, pr_path: &Path) -> Result<Option<PrArtifact>> {
    if let Some(pr) = &metadata.pr {
        return Ok(Some(pr.clone()));
    }

    if metadata.pr_created {
        if let (
            Some(created_at),
            Some(provider),
            Some(url),
            Some(target_branch),
            Some(source_branch),
            Some(commit_sha),
        ) = (
            metadata.pr_created_at.clone(),
            metadata.pr_provider.clone(),
            metadata.pr_url.clone(),
            metadata.pr_target_branch.clone(),
            metadata.pr_source_branch.clone(),
            metadata.commit_sha.clone(),
        ) {
            let provider = provider.parse::<PrProvider>()?;
            return Ok(Some(PrArtifact {
                run_id: metadata.run_id.clone(),
                provider,
                provider_name: provider.display_name().to_string(),
                request_kind: provider.request_kind().to_string(),
                remote: metadata
                    .push_remote
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
                remote_url: metadata.push_remote_url.clone().unwrap_or_default(),
                repository_url: metadata
                    .push_remote_url
                    .as_deref()
                    .and_then(repository_web_url),
                source_branch,
                target_branch,
                commit_sha,
                title: metadata
                    .commit_message
                    .clone()
                    .unwrap_or_else(|| default_commit_message(metadata)),
                url,
                created_at,
                draft: false,
                reused_existing: false,
                dry_run: false,
            }));
        }
    }

    if pr_path.is_file() {
        return Ok(Some(crate::json::read_json(pr_path)?));
    }

    Ok(None)
}

fn already_created_result(
    plan: &PrPlan,
    artifact: PrArtifact,
    dry_run: bool,
    pr_path: &Path,
) -> PrResult {
    let provider_command = provider_command(plan).unwrap_or_default();
    PrResult {
        run_id: plan.run_id.clone(),
        provider: artifact.provider,
        provider_name: artifact.provider.display_name(),
        request_kind: artifact.provider.request_kind(),
        remote: artifact.remote,
        remote_url: artifact.remote_url,
        repository_url: artifact.repository_url,
        source_branch: artifact.source_branch,
        target_branch: artifact.target_branch,
        commit_sha: artifact.commit_sha,
        title: artifact.title,
        url: Some(artifact.url),
        draft: artifact.draft,
        manual: false,
        dry_run,
        created: true,
        already_created: true,
        reused_existing: false,
        would_create_request: false,
        would_write_artifact: false,
        would_push: false,
        would_merge: false,
        pr_path: Some(pr_path.display().to_string()),
        provider_command_display: provider_command_display(&provider_command),
        provider_command,
    }
}

fn reused_existing_result(
    plan: &PrPlan,
    existing: ExistingProviderPr,
    created_at: &str,
    pr_path: &Path,
    provider_command: Vec<String>,
) -> (PrArtifact, PrResult) {
    let title = existing.title.unwrap_or_else(|| plan.title.clone());
    let artifact = PrArtifact {
        run_id: plan.run_id.clone(),
        provider: plan.provider,
        provider_name: plan.provider_name.to_string(),
        request_kind: plan.request_kind.to_string(),
        remote: plan.remote.clone(),
        remote_url: plan.remote_url.clone(),
        repository_url: plan.repository_url.clone(),
        source_branch: plan.source_branch.clone(),
        target_branch: plan.target_branch.clone(),
        commit_sha: plan.commit_sha.clone(),
        title: title.clone(),
        url: existing.url.clone(),
        created_at: created_at.to_string(),
        draft: existing.draft,
        reused_existing: true,
        dry_run: false,
    };
    let result = PrResult {
        run_id: plan.run_id.clone(),
        provider: plan.provider,
        provider_name: plan.provider_name,
        request_kind: plan.request_kind,
        remote: plan.remote.clone(),
        remote_url: plan.remote_url.clone(),
        repository_url: plan.repository_url.clone(),
        source_branch: plan.source_branch.clone(),
        target_branch: plan.target_branch.clone(),
        commit_sha: plan.commit_sha.clone(),
        title,
        url: Some(existing.url),
        draft: existing.draft,
        manual: false,
        dry_run: false,
        created: true,
        already_created: false,
        reused_existing: true,
        would_create_request: false,
        would_write_artifact: true,
        would_push: false,
        would_merge: false,
        pr_path: Some(pr_path.display().to_string()),
        provider_command_display: provider_command_display(&provider_command),
        provider_command,
    };
    (artifact, result)
}

fn write_pr_artifact_and_metadata(
    pr_path: &Path,
    metadata: &mut RunMetadata,
    artifact: &PrArtifact,
) -> Result<()> {
    crate::json::write_json_pretty(pr_path, artifact)?;
    metadata.pr_created = true;
    metadata.pr_created_at = Some(artifact.created_at.clone());
    metadata.pr_provider = Some(artifact.provider.to_string());
    metadata.pr_url = Some(artifact.url.clone());
    metadata.pr_target_branch = Some(artifact.target_branch.clone());
    metadata.pr_source_branch = Some(artifact.source_branch.clone());
    metadata.pr = Some(artifact.clone());
    Ok(())
}

fn created_at_now() -> String {
    now_timestamp()
}

fn default_target_branch(root: &Path) -> String {
    git_stdout(root, &["branch", "--show-current"])
        .filter(|branch| !branch.trim().is_empty())
        .or_else(|| {
            git_stdout(
                root,
                &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
            )
            .and_then(|branch| branch.rsplit('/').next().map(str::to_string))
        })
        .unwrap_or_else(|| "main".to_string())
}

fn git_stdout(root: &Path, args: &[&str]) -> Option<String> {
    let args = args
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    let capture = run_command(root, "git", &args).ok()?;
    if !capture.status.success() {
        return None;
    }
    Some(capture.stdout.trim().to_string())
}

fn artifact_paths(metadata: &RunMetadata) -> PrArtifactPaths {
    let run_dir = metadata.run_dir.trim_end_matches(['/', '\\']);
    PrArtifactPaths {
        metadata: format!("{run_dir}/{METADATA_FILE}"),
        log: format!("{run_dir}/{LOG_FILE}"),
        diff: format!("{run_dir}/{DIFF_FILE}"),
        checks: format!("{run_dir}/{CHECKS_FILE}"),
        report: format!("{run_dir}/{REPORT_FILE}"),
        commit: metadata
            .committed
            .then(|| format!("{run_dir}/{COMMIT_FILE}")),
        push: metadata.pushed.then(|| format!("{run_dir}/{PUSH_FILE}")),
        pr: metadata.pr_created.then(|| format!("{run_dir}/{PR_FILE}")),
    }
}

fn default_body(
    metadata: &RunMetadata,
    source_branch: &str,
    target_branch: &str,
    commit_sha: &str,
    artifacts: &PrArtifactPaths,
) -> String {
    let warnings = warnings_markdown(metadata);
    format!(
        "\
## Keel Candidate Change

- Run: `{}`
- Agent: `{}`
- Task: {}
- Source branch: `{}`
- Target branch: `{}`
- Commit: `{}`
- Status: `{}`
- Readiness: {}

## Warnings

{}

## Artifacts

- Metadata: `{}`
- Log: `{}`
- Diff: `{}`
- Checks: `{}`
- Report: `{}`
- Commit: `{}`
- Push: `{}`
- PR/MR: `{}`

## Safety

- Keel did not merge this candidate change.
- Keel did not push anything from the PR command.
- Human review is required before merge.
",
        metadata.run_id,
        metadata.agent,
        markdown_inline(&metadata.task),
        source_branch,
        target_branch,
        commit_sha,
        metadata.status,
        markdown_inline(readiness_summary(metadata)),
        warnings,
        artifacts.metadata,
        artifacts.log,
        artifacts.diff,
        artifacts.checks,
        artifacts.report,
        artifacts.commit.as_deref().unwrap_or("not available"),
        artifacts.push.as_deref().unwrap_or("not available"),
        artifacts.pr.as_deref().unwrap_or("not created yet"),
    )
}

fn copyable_summary(
    metadata: &RunMetadata,
    source_branch: &str,
    target_branch: &str,
    commit_sha: &str,
) -> String {
    format!(
        "Keel run {} by {}: {} ({} -> {}, commit {})",
        metadata.run_id,
        metadata.agent,
        one_line(&metadata.task),
        source_branch,
        target_branch,
        commit_sha
    )
}

fn warnings_markdown(metadata: &RunMetadata) -> String {
    if metadata.warnings.is_empty() {
        return "None".to_string();
    }

    metadata
        .warnings
        .iter()
        .map(|warning| format!("- {}", markdown_inline(warning)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn readiness_summary(metadata: &RunMetadata) -> &str {
    if metadata.readiness_reason.trim().is_empty() {
        "ready"
    } else {
        metadata.readiness_reason.trim()
    }
}

fn markdown_inline(value: &str) -> String {
    one_line(value).replace('`', "\\`")
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn manual_steps(
    provider: PrProvider,
    source_branch: &str,
    target_branch: &str,
    repository_url: Option<&str>,
    web_url: Option<&str>,
) -> Vec<String> {
    let request_label = match provider {
        PrProvider::Gitlab => "Merge Request",
        PrProvider::Github | PrProvider::Gitee | PrProvider::Gitea => "Pull Request",
    };
    let mut instructions = Vec::new();
    if let Some(web_url) = web_url {
        instructions.push(format!("Open {web_url} in your browser."));
    } else if let Some(repository_url) = repository_url {
        instructions.push(format!("Open {repository_url} in your browser."));
    }
    instructions.push(format!(
        "Create a {request_label} from `{}` into `{target_branch}`.",
        source_branch
    ));
    instructions.push("Keel did not call any provider API.".to_string());
    instructions.push("Keel did not merge anything.".to_string());
    instructions
}
