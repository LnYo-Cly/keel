use crate::command::run_command;
use crate::commit::default_commit_message;
use crate::model::{RunMetadata, RunStatus};
use crate::push::PushArtifact;
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
    pub manual_steps: Vec<String>,
    pub would_create_request: bool,
    pub would_write_artifact: bool,
    pub would_push: bool,
    pub would_merge: bool,
}

pub(crate) fn plan_pr(root: &Path, metadata: &RunMetadata, options: PrOptions) -> Result<PrPlan> {
    validate_pr_mode(&options)?;
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
        .filter(|target| !target.trim().is_empty())
        .unwrap_or_else(|| default_target_branch(root));
    let title = options
        .title
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| default_commit_message(metadata));
    let body = default_body(metadata, &push, &target_branch);
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
        manual: true,
        dry_run: true,
        remote: push.remote,
        remote_url: push.remote_url,
        repository_url,
        web_url,
        source_branch: push.branch,
        target_branch,
        commit_sha,
        title,
        body,
        manual_steps,
        would_create_request: false,
        would_write_artifact: false,
        would_push: false,
        would_merge: false,
    })
}

fn validate_pr_mode(options: &PrOptions) -> Result<()> {
    if !options.manual || !options.dry_run {
        bail!("this Keel version only supports `keel pr <run-id> --manual --dry-run`");
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

fn default_body(metadata: &RunMetadata, push: &PushArtifact, target_branch: &str) -> String {
    format!(
        "Keel run: {}\nAgent: {}\nSource branch: {}\nTarget branch: {}\nCommit: {}\n\nHuman review required before merge.",
        metadata.run_id, metadata.agent, push.branch, target_branch, push.commit_sha
    )
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
