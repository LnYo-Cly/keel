use crate::command::{format_command, run_command};
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

#[derive(Debug, Clone)]
pub struct PrOptions {
    pub manual: bool,
    pub dry_run: bool,
    pub provider: Option<PrProvider>,
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
    pub manual: bool,
    pub dry_run: bool,
    pub created: bool,
    pub already_created: bool,
    pub would_create_request: bool,
    pub would_write_artifact: bool,
    pub would_push: bool,
    pub would_merge: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_path: Option<String>,
    pub provider_command: Vec<String>,
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
            manual: false,
            dry_run: true,
            created: false,
            already_created: false,
            would_create_request: true,
            would_write_artifact: false,
            would_push: false,
            would_merge: false,
            pr_path: None,
            provider_command,
        });
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
        dry_run: false,
    };

    crate::json::write_json_pretty(&pr_path, &artifact)?;
    metadata.pr_created = true;
    metadata.pr_created_at = Some(created_at);
    metadata.pr_provider = Some(plan.provider.to_string());
    metadata.pr_url = Some(url.clone());
    metadata.pr_target_branch = Some(plan.target_branch.clone());
    metadata.pr_source_branch = Some(plan.source_branch.clone());
    metadata.pr = Some(artifact);

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
        manual: false,
        dry_run: false,
        created: true,
        already_created: false,
        would_create_request: false,
        would_write_artifact: false,
        would_push: false,
        would_merge: false,
        pr_path: Some(pr_path.display().to_string()),
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
        .target
        .as_deref()
        .filter(|target| !target.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| default_target_branch(root));
    let title = options
        .title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| default_commit_message(metadata));
    let artifacts = artifact_paths(metadata);
    let body = default_body(metadata, &push, &target_branch, &commit_sha, &artifacts);
    let copyable_summary = copyable_summary(metadata, &push, &target_branch, &commit_sha);
    let repository_url = repository_web_url(&push.remote_url);
    let web_url = repository_url
        .as_deref()
        .map(|url| provider_pr_web_url(provider, url, &push.branch, &target_branch, &title, &body));
    let manual_steps = manual_steps(
        provider,
        &push,
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
        source_branch: push.branch,
        target_branch,
        commit_sha,
        title,
        body,
        copyable_summary,
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
        manual: false,
        dry_run,
        created: true,
        already_created: true,
        would_create_request: false,
        would_write_artifact: false,
        would_push: false,
        would_merge: false,
        pr_path: Some(pr_path.display().to_string()),
        provider_command: provider_command(plan).unwrap_or_default(),
    }
}

fn provider_command(plan: &PrPlan) -> Result<Vec<String>> {
    let repository = repository_selector(&plan.remote_url).ok_or_else(|| {
        anyhow::anyhow!(
            "could not derive repository selector from remote `{}`; use manual PR/MR planning for this remote",
            plan.remote_url
        )
    })?;
    match plan.provider {
        PrProvider::Github => Ok(vec![
            "gh".to_string(),
            "pr".to_string(),
            "create".to_string(),
            "--repo".to_string(),
            repository,
            "--base".to_string(),
            plan.target_branch.clone(),
            "--head".to_string(),
            plan.source_branch.clone(),
            "--title".to_string(),
            plan.title.clone(),
            "--body".to_string(),
            provider_body_arg(&plan.body),
            "--draft".to_string(),
        ]),
        PrProvider::Gitlab => Ok(vec![
            "glab".to_string(),
            "mr".to_string(),
            "create".to_string(),
            "--repo".to_string(),
            repository,
            "--source-branch".to_string(),
            plan.source_branch.clone(),
            "--target-branch".to_string(),
            plan.target_branch.clone(),
            "--title".to_string(),
            plan.title.clone(),
            "--description".to_string(),
            provider_body_arg(&plan.body),
            "--draft".to_string(),
            "--yes".to_string(),
        ]),
        PrProvider::Gitee | PrProvider::Gitea => bail!(
            "provider-backed PR/MR creation for {} is not implemented yet; use `keel pr <run-id> --manual --dry-run --provider {}`",
            plan.provider.display_name(),
            plan.provider
        ),
    }
}

fn run_provider_command(root: &Path, plan: &PrPlan, command: &[String]) -> Result<String> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("provider command is empty"))?;
    let capture = run_command(root, program, args)?;
    if !capture.status.success() {
        bail!(
            "failed to create {} with `{}`\nstdout:\n{}\nstderr:\n{}",
            request_label(plan.provider),
            format_command(program, args),
            capture.stdout.trim(),
            capture.stderr.trim()
        );
    }

    provider_output_url(plan.provider, &capture.stdout)
        .or_else(|| plan.web_url.clone())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{} command succeeded but no PR/MR URL was found in stdout",
                provider_program(plan.provider)
            )
        })
}

fn provider_output_url(provider: PrProvider, output: &str) -> Option<String> {
    output
        .split_whitespace()
        .find(|token| token_matches_provider_url(provider, token))
        .map(|token| token.trim_matches(|ch: char| ch == '"' || ch == '\'' || ch == '`'))
        .map(str::to_string)
}

fn token_matches_provider_url(provider: PrProvider, token: &str) -> bool {
    let token = token.trim_matches(|ch: char| ch == '"' || ch == '\'' || ch == '`');
    if !(token.starts_with("http://") || token.starts_with("https://")) {
        return false;
    }

    match provider {
        PrProvider::Github | PrProvider::Gitee | PrProvider::Gitea => token.contains("/pull/"),
        PrProvider::Gitlab => {
            token.contains("/merge_requests/") || token.contains("/-/merge_requests/")
        }
    }
}

fn repository_selector(remote_url: &str) -> Option<String> {
    let remote_url = remote_url.trim().trim_end_matches(".git");
    if remote_url.is_empty() {
        return None;
    }

    let (host, path) = if let Some(index) = remote_url.find("://") {
        let without_scheme = &remote_url[index + 3..];
        let host = without_scheme.split('/').next()?.rsplit('@').next()?.trim();
        let path = without_scheme
            .split_once('/')
            .map(|(_, path)| path.trim_matches('/'))
            .filter(|path| !path.is_empty())?;
        (host.to_ascii_lowercase(), path)
    } else if let Some(index) = remote_url.find('@') {
        let rest = &remote_url[index + 1..];
        let (host, path) = rest.split_once([':', '/'])?;
        (host.trim().to_ascii_lowercase(), path.trim_matches('/'))
    } else {
        return None;
    };

    if path.is_empty() {
        None
    } else if host == "github.com" {
        Some(path.to_string())
    } else {
        Some(format!("{host}/{path}"))
    }
}

fn provider_program(provider: PrProvider) -> &'static str {
    match provider {
        PrProvider::Github => "gh",
        PrProvider::Gitlab => "glab",
        PrProvider::Gitee => "gitee",
        PrProvider::Gitea => "gitea",
    }
}

fn request_label(provider: PrProvider) -> &'static str {
    match provider {
        PrProvider::Gitlab => "Merge Request",
        PrProvider::Github | PrProvider::Gitee | PrProvider::Gitea => "Pull Request",
    }
}

fn provider_body_arg(body: &str) -> String {
    one_line(body)
}

pub fn infer_provider(remote_url: &str) -> Option<PrProvider> {
    let host = remote_host(remote_url)?;
    match host.as_str() {
        "github.com" => Some(PrProvider::Github),
        "gitlab.com" => Some(PrProvider::Gitlab),
        "gitee.com" => Some(PrProvider::Gitee),
        "gitea.com" => Some(PrProvider::Gitea),
        _ => None,
    }
}

fn remote_host(remote_url: &str) -> Option<String> {
    let remote_url = remote_url.trim();
    if remote_url.is_empty() {
        return None;
    }

    let host = if let Some(index) = remote_url.find("://") {
        let without_scheme = &remote_url[index + 3..];
        host_from_authority(without_scheme)
    } else if let Some(index) = remote_url.find('@') {
        host_until_separator(&remote_url[index + 1..])
    } else {
        host_until_separator(remote_url)
    }?;

    Some(host.trim().trim_matches('/').to_ascii_lowercase())
}

fn host_from_authority(value: &str) -> Option<String> {
    let authority = value.split('/').next().unwrap_or(value);
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    host_until_separator(host_port)
}

fn host_until_separator(value: &str) -> Option<String> {
    let host = value.split([':', '/', '\\']).next().unwrap_or(value).trim();
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
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

fn repository_web_url(remote_url: &str) -> Option<String> {
    let remote_url = remote_url.trim().trim_end_matches(".git");
    if remote_url.starts_with("http://") || remote_url.starts_with("https://") {
        return Some(remote_url.to_string());
    }

    let host = remote_host(remote_url)?;
    let path = if let Some(index) = remote_url.find('@') {
        let rest = &remote_url[index + 1..];
        rest.split_once([':', '/'])
            .map(|(_, path)| path.trim_matches('/'))?
    } else {
        return None;
    };

    if path.is_empty() {
        None
    } else {
        Some(format!("https://{host}/{}", path.trim_end_matches(".git")))
    }
}

fn provider_pr_web_url(
    provider: PrProvider,
    repository_url: &str,
    source_branch: &str,
    target_branch: &str,
    title: &str,
    body: &str,
) -> String {
    match provider {
        PrProvider::Github | PrProvider::Gitee | PrProvider::Gitea => {
            compare_url(repository_url, source_branch, target_branch, title, body)
        }
        PrProvider::Gitlab => {
            merge_request_url(repository_url, source_branch, target_branch, title, body)
        }
    }
}

fn compare_url(
    repository_url: &str,
    source_branch: &str,
    target_branch: &str,
    title: &str,
    body: &str,
) -> String {
    format!(
        "{}/compare/{}...{}?title={}&body={}",
        repository_url.trim_end_matches('/'),
        encode_path_segment(target_branch),
        encode_path_segment(source_branch),
        encode_query_value(title),
        encode_query_value(body)
    )
}

fn merge_request_url(
    repository_url: &str,
    source_branch: &str,
    target_branch: &str,
    title: &str,
    body: &str,
) -> String {
    format!(
        "{}/-/merge_requests/new?merge_request[source_branch]={}&merge_request[target_branch]={}&merge_request[title]={}&merge_request[description]={}",
        repository_url.trim_end_matches('/'),
        encode_query_value(source_branch),
        encode_query_value(target_branch),
        encode_query_value(title),
        encode_query_value(body)
    )
}

fn encode_path_segment(value: &str) -> String {
    urlencoding::encode(value).into_owned()
}

fn encode_query_value(value: &str) -> String {
    urlencoding::encode(value).into_owned()
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
    push: &PushArtifact,
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
        push.branch,
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
    push: &PushArtifact,
    target_branch: &str,
    commit_sha: &str,
) -> String {
    format!(
        "Keel run {} by {}: {} ({} -> {}, commit {})",
        metadata.run_id,
        metadata.agent,
        one_line(&metadata.task),
        push.branch,
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
    push: &PushArtifact,
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
        push.branch
    ));
    instructions.push("Keel did not call any provider API.".to_string());
    instructions.push("Keel did not push or merge anything.".to_string());
    instructions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_common_provider_remote_urls() {
        for (remote, provider) in [
            ("git@github.com:owner/repo.git", PrProvider::Github),
            ("https://github.com/owner/repo.git", PrProvider::Github),
            ("ssh://git@gitlab.com/owner/repo.git", PrProvider::Gitlab),
            ("https://gitlab.com/owner/repo", PrProvider::Gitlab),
            ("git@gitee.com:owner/repo.git", PrProvider::Gitee),
            ("https://gitee.com/owner/repo.git", PrProvider::Gitee),
            ("git@gitea.com:owner/repo.git", PrProvider::Gitea),
        ] {
            assert_eq!(infer_provider(remote), Some(provider));
        }
    }

    #[test]
    fn leaves_unknown_provider_uninferred() {
        assert_eq!(infer_provider("/tmp/local-bare.git"), None);
        assert_eq!(infer_provider("git@example.internal:owner/repo.git"), None);
    }

    #[test]
    fn builds_repository_web_url_from_common_remotes() {
        assert_eq!(
            repository_web_url("git@github.com:owner/repo.git").as_deref(),
            Some("https://github.com/owner/repo")
        );
        assert_eq!(
            repository_web_url("ssh://git@gitlab.com/owner/repo.git").as_deref(),
            Some("https://gitlab.com/owner/repo")
        );
        assert_eq!(
            repository_web_url("https://gitee.com/owner/repo.git").as_deref(),
            Some("https://gitee.com/owner/repo")
        );
    }

    #[test]
    fn builds_provider_cli_repository_selectors() {
        assert_eq!(
            repository_selector("git@github.com:owner/repo.git").as_deref(),
            Some("owner/repo")
        );
        assert_eq!(
            repository_selector("https://github.com/owner/repo.git").as_deref(),
            Some("owner/repo")
        );
        assert_eq!(
            repository_selector("git@gitlab.com:group/project.git").as_deref(),
            Some("gitlab.com/group/project")
        );
        assert_eq!(
            repository_selector("ssh://git@gitlab.example.com/group/project.git").as_deref(),
            Some("gitlab.example.com/group/project")
        );
    }

    #[test]
    fn extracts_provider_output_urls() {
        assert_eq!(
            provider_output_url(
                PrProvider::Github,
                "Creating pull request...\nhttps://github.com/owner/repo/pull/42\n"
            )
            .as_deref(),
            Some("https://github.com/owner/repo/pull/42")
        );
        assert_eq!(
            provider_output_url(
                PrProvider::Gitlab,
                "Created merge request: https://gitlab.com/owner/repo/-/merge_requests/7"
            )
            .as_deref(),
            Some("https://gitlab.com/owner/repo/-/merge_requests/7")
        );
    }

    #[test]
    fn builds_provider_web_urls() {
        for provider in [PrProvider::Github, PrProvider::Gitee, PrProvider::Gitea] {
            let web_url = provider_pr_web_url(
                provider,
                "https://example.com/owner/repo",
                "keel/run/123",
                "release/v1",
                "keel: test",
                "body text",
            );
            assert_eq!(
                web_url,
                "https://example.com/owner/repo/compare/release%2Fv1...keel%2Frun%2F123?title=keel%3A%20test&body=body%20text"
            );
        }

        let web_url = provider_pr_web_url(
            PrProvider::Gitlab,
            "https://gitlab.com/owner/repo",
            "keel/run/123",
            "release/v1",
            "keel: test",
            "body text",
        );
        assert_eq!(
            web_url,
            "https://gitlab.com/owner/repo/-/merge_requests/new?merge_request[source_branch]=keel%2Frun%2F123&merge_request[target_branch]=release%2Fv1&merge_request[title]=keel%3A%20test&merge_request[description]=body%20text"
        );
    }
}
