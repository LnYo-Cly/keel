use super::{PrPlan, PrProvider};
use crate::command::{format_command, run_command};
use anyhow::{bail, Result};
use std::path::Path;

pub(super) fn provider_command(plan: &PrPlan) -> Result<Vec<String>> {
    match plan.provider {
        PrProvider::Github => {
            let repository = repository_selector(&plan.remote_url).ok_or_else(|| {
                anyhow::anyhow!(
                    "could not derive GitHub repository selector from remote `{}`; use manual PR planning for this remote",
                    plan.remote_url
                )
            })?;
            let mut command = vec![
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
            ];
            if plan.draft {
                command.push("--draft".to_string());
            }
            Ok(command)
        }
        PrProvider::Gitlab | PrProvider::Gitee | PrProvider::Gitea => bail!(
            "provider-backed PR/MR creation for {} is not implemented in v0.5c; use `keel pr <run-id> --manual --dry-run --provider {}`",
            plan.provider.display_name(),
            plan.provider
        ),
    }
}

pub(super) fn run_provider_command(
    root: &Path,
    plan: &PrPlan,
    command: &[String],
) -> Result<String> {
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
            anyhow::anyhow!("gh command succeeded but no GitHub PR URL was found in stdout")
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

fn request_label(provider: PrProvider) -> &'static str {
    match provider {
        PrProvider::Gitlab => "Merge Request",
        PrProvider::Github | PrProvider::Gitee | PrProvider::Gitea => "Pull Request",
    }
}

fn provider_body_arg(body: &str) -> String {
    collapse_whitespace(body)
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
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

pub(super) fn repository_web_url(remote_url: &str) -> Option<String> {
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

pub(super) fn provider_pr_web_url(
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
