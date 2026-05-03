use super::{PrPlan, PrProvider};
use crate::command::{format_command, run_command};
use anyhow::{bail, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone)]
pub(super) struct ExistingProviderPr {
    pub url: String,
    pub title: Option<String>,
    pub draft: bool,
}

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

pub(super) fn provider_command_display(command: &[String]) -> String {
    format_provider_command(&provider_command_for_display(command))
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
            "{}",
            format_provider_failure(
                plan.provider,
                "create",
                command,
                &capture.stdout,
                &capture.stderr
            )
        );
    }

    provider_output_url(plan.provider, &capture.stdout)
        .or_else(|| plan.web_url.clone())
        .ok_or_else(|| {
            anyhow::anyhow!("gh command succeeded but no GitHub PR URL was found in stdout")
        })
}

pub(super) fn find_existing_provider_pr(
    root: &Path,
    plan: &PrPlan,
) -> Result<Option<ExistingProviderPr>> {
    if plan.provider != PrProvider::Github {
        return Ok(None);
    }

    let command = existing_provider_pr_command(plan)?;
    let (program, args) = command
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("provider command is empty"))?;
    let capture = run_command(root, program, args)?;
    if !capture.status.success() {
        bail!(
            "{}",
            format_provider_failure(
                plan.provider,
                "inspect existing",
                &command,
                &capture.stdout,
                &capture.stderr
            )
        );
    }

    Ok(parse_existing_provider_prs(&capture.stdout)?
        .into_iter()
        .next())
}

fn existing_provider_pr_command(plan: &PrPlan) -> Result<Vec<String>> {
    let repository = repository_selector(&plan.remote_url).ok_or_else(|| {
        anyhow::anyhow!(
            "could not derive GitHub repository selector from remote `{}`; use manual PR planning for this remote",
            plan.remote_url
        )
    })?;

    Ok(vec![
        "gh".to_string(),
        "pr".to_string(),
        "list".to_string(),
        "--repo".to_string(),
        repository,
        "--head".to_string(),
        plan.source_branch.clone(),
        "--base".to_string(),
        plan.target_branch.clone(),
        "--state".to_string(),
        "open".to_string(),
        "--json".to_string(),
        "url,title,isDraft".to_string(),
        "--limit".to_string(),
        "1".to_string(),
    ])
}

fn parse_existing_provider_prs(output: &str) -> Result<Vec<ExistingProviderPr>> {
    let items: Vec<GithubPrListItem> = serde_json::from_str(output.trim()).map_err(|error| {
        anyhow::anyhow!(
            "gh command succeeded but existing GitHub PR JSON could not be parsed: {error}"
        )
    })?;

    Ok(items
        .into_iter()
        .filter_map(|item| {
            let url = item.url?.trim().to_string();
            if url.is_empty() {
                return None;
            }
            Some(ExistingProviderPr {
                url,
                title: item
                    .title
                    .map(|title| title.trim().to_string())
                    .filter(|title| !title.is_empty()),
                draft: item.is_draft,
            })
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct GithubPrListItem {
    url: Option<String>,
    title: Option<String>,
    #[serde(default, rename = "isDraft")]
    is_draft: bool,
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

fn format_provider_failure(
    provider: PrProvider,
    action: &str,
    command: &[String],
    stdout: &str,
    stderr: &str,
) -> String {
    let command_text = provider_command_display(command);
    let combined = format!("{stdout}\n{stderr}").to_ascii_lowercase();
    let hint = if is_auth_failure(&combined) {
        Some("GitHub CLI is not authenticated; run `gh auth login` or set a valid GITHUB_TOKEN.")
    } else if is_permission_failure(&combined) {
        Some("GitHub CLI does not have permission to create or inspect this Pull Request; verify repository access and token scopes.")
    } else if is_not_found_failure(&combined) {
        Some("GitHub repository was not found or is inaccessible; verify the pushed remote and repository permissions.")
    } else {
        None
    };

    let mut message = format!(
        "failed to {action} {} with `{command_text}`",
        request_label(provider)
    );
    if let Some(hint) = hint {
        message.push('\n');
        message.push_str(hint);
    }
    message.push_str("\nstdout:\n");
    message.push_str(stdout.trim());
    message.push_str("\nstderr:\n");
    message.push_str(stderr.trim());
    message
}

fn format_provider_command(command: &[String]) -> String {
    command
        .split_first()
        .map(|(program, args)| format_command(program, args))
        .unwrap_or_else(|| "<empty provider command>".to_string())
}

fn provider_command_for_display(command: &[String]) -> Vec<String> {
    let mut display = Vec::with_capacity(command.len());
    let mut redact_next = false;
    for arg in command {
        if redact_next {
            display.push("<generated PR body>".to_string());
            redact_next = false;
            continue;
        }

        if arg == "--body" {
            display.push(arg.clone());
            redact_next = true;
        } else if arg.starts_with("--body=") {
            display.push("--body=<generated PR body>".to_string());
        } else {
            display.push(arg.clone());
        }
    }
    display
}

fn is_auth_failure(value: &str) -> bool {
    value.contains("not logged in")
        || value.contains("not authenticated")
        || value.contains("authentication required")
        || value.contains("gh auth login")
        || value.contains("bad credentials")
        || value.contains("requires authentication")
}

fn is_permission_failure(value: &str) -> bool {
    value.contains("http 403")
        || value.contains("resource not accessible")
        || value.contains("insufficient")
        || value.contains("forbidden")
        || value.contains("not authorized")
        || value.contains("permission")
}

fn is_not_found_failure(value: &str) -> bool {
    value.contains("http 404")
        || value.contains("could not resolve to a repository")
        || value.contains("repository not found")
        || value.contains("not found")
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
    fn parses_existing_github_pr_list_output() {
        let existing = parse_existing_provider_prs(
            r#"[{"url":"https://github.com/owner/repo/pull/99","title":"existing","isDraft":true}]"#,
        )
        .unwrap();

        assert_eq!(existing.len(), 1);
        assert_eq!(existing[0].url, "https://github.com/owner/repo/pull/99");
        assert_eq!(existing[0].title.as_deref(), Some("existing"));
        assert!(existing[0].draft);
        assert!(parse_existing_provider_prs("[]").unwrap().is_empty());
    }

    #[test]
    fn formats_common_gh_failures_with_actionable_hints() {
        let command = vec![
            "gh".to_string(),
            "pr".to_string(),
            "create".to_string(),
            "--body".to_string(),
            "sensitive generated body".to_string(),
        ];

        let auth = format_provider_failure(
            PrProvider::Github,
            "create",
            &command,
            "",
            "You are not logged into any GitHub hosts",
        );
        assert!(auth.contains("GitHub CLI is not authenticated"));
        assert!(auth.contains("<generated PR body>"));
        assert!(!auth.contains("sensitive generated body"));

        let permission = format_provider_failure(
            PrProvider::Github,
            "create",
            &command,
            "",
            "HTTP 403: Resource not accessible by integration",
        );
        assert!(permission.contains("does not have permission"));

        let not_found = format_provider_failure(
            PrProvider::Github,
            "create",
            &command,
            "",
            "HTTP 404: Repository not found",
        );
        assert!(not_found.contains("repository was not found or is inaccessible"));
    }

    #[test]
    fn provider_command_display_redacts_generated_body() {
        let command = vec![
            "gh".to_string(),
            "pr".to_string(),
            "create".to_string(),
            "--title".to_string(),
            "keel: test".to_string(),
            "--body".to_string(),
            "## Keel Candidate Change\n\nlocal paths and generated details".to_string(),
        ];

        let display = provider_command_display(&command);

        assert!(display.contains("gh pr create"));
        assert!(display.contains("<generated PR body>"));
        assert!(!display.contains("Keel Candidate Change"));
        assert!(!display.contains("local paths"));
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
