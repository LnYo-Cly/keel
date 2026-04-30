use crate::command::{format_command, run_command};
use crate::constants::{COMMIT_FILE, PUBLISH_FILE};
use crate::git::{ensure_safe_run_id, expected_run_branch};
use crate::json::{read_json, write_json_pretty};
use crate::model::{RunMetadata, RunStatus};
use crate::time::now_timestamp;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct PublishOptions {
    pub remote: String,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishArtifact {
    pub run_id: String,
    pub remote: String,
    pub remote_url: String,
    pub branch: String,
    pub commit_sha: String,
    pub pushed: bool,
    pub published_at: String,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublishResult {
    pub run_id: String,
    pub remote: String,
    pub remote_url: String,
    pub branch: String,
    pub commit_sha: String,
    pub pushed: bool,
    pub dry_run: bool,
    pub already_published: bool,
    pub would_push: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publish_path: Option<String>,
}

pub(crate) fn publish_run(
    root: &Path,
    run_dir: &Path,
    metadata: &mut RunMetadata,
    options: PublishOptions,
) -> Result<PublishResult> {
    let publish_path = run_dir.join(PUBLISH_FILE);
    ensure_safe_run_id(&metadata.run_id)?;
    validate_remote_name(&options.remote)?;
    validate_publish_identity(metadata)?;

    if let Some(existing) = existing_publish(metadata, &publish_path)? {
        return Ok(already_published_result(
            metadata,
            existing,
            options.dry_run,
            &publish_path,
        ));
    }

    validate_publish_preconditions(root, run_dir, metadata)?;
    let commit_sha = committed_sha(metadata, &run_dir.join(COMMIT_FILE))?;
    let remote_url = remote_url(root, &options.remote)?;
    validate_branch_head(root, &metadata.branch, &commit_sha)?;

    if options.dry_run {
        return Ok(PublishResult {
            run_id: metadata.run_id.clone(),
            remote: options.remote,
            remote_url,
            branch: metadata.branch.clone(),
            commit_sha,
            pushed: false,
            dry_run: true,
            already_published: false,
            would_push: true,
            publish_path: None,
        });
    }

    git_push(root, &options.remote, &metadata.branch)?;
    let published_at = now_timestamp();
    let artifact = PublishArtifact {
        run_id: metadata.run_id.clone(),
        remote: options.remote.clone(),
        remote_url: remote_url.clone(),
        branch: metadata.branch.clone(),
        commit_sha: commit_sha.clone(),
        pushed: true,
        published_at: published_at.clone(),
        dry_run: false,
    };

    write_json_pretty(&publish_path, &artifact)?;
    metadata.published = true;
    metadata.published_at = Some(published_at);
    metadata.publish_remote = Some(options.remote.clone());
    metadata.publish_remote_url = Some(remote_url.clone());
    metadata.published_branch = Some(metadata.branch.clone());
    metadata.publish = Some(artifact);

    Ok(PublishResult {
        run_id: metadata.run_id.clone(),
        remote: options.remote,
        remote_url,
        branch: metadata.branch.clone(),
        commit_sha,
        pushed: true,
        dry_run: false,
        already_published: false,
        would_push: false,
        publish_path: Some(publish_path.display().to_string()),
    })
}

fn validate_publish_preconditions(
    root: &Path,
    run_dir: &Path,
    metadata: &RunMetadata,
) -> Result<()> {
    validate_publish_identity(metadata)?;

    let _commit_sha = committed_sha(metadata, &run_dir.join(COMMIT_FILE)).with_context(|| {
        format!(
            "run `{}` is not committed; run `keel commit {}` first",
            metadata.run_id, metadata.run_id
        )
    })?;

    let branch_ref = format!("refs/heads/{}", metadata.branch);
    let capture = run_command(
        root,
        "git",
        &[
            "show-ref".to_string(),
            "--verify".to_string(),
            "--quiet".to_string(),
            branch_ref.clone(),
        ],
    )?;
    if !capture.status.success() {
        bail!(
            "candidate branch `{}` does not exist; cannot publish run `{}`",
            metadata.branch,
            metadata.run_id
        );
    }

    Ok(())
}

fn validate_publish_identity(metadata: &RunMetadata) -> Result<()> {
    if metadata.status != RunStatus::Ready {
        bail!(
            "run `{}` has status `{}`; only ready runs can be published",
            metadata.run_id,
            metadata.status
        );
    }

    if metadata.branch != expected_run_branch(&metadata.run_id)? {
        bail!(
            "refusing to publish unexpected branch `{}` for run `{}`",
            metadata.branch,
            metadata.run_id
        );
    }
    Ok(())
}

fn committed_sha(metadata: &RunMetadata, commit_path: &Path) -> Result<String> {
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

    if commit_path.is_file() {
        let commit: crate::commit::CommitArtifact = read_json(commit_path)?;
        if !commit.commit_sha.trim().is_empty() {
            return Ok(commit.commit_sha);
        }
    }

    bail!("missing committed candidate metadata")
}

fn remote_url(root: &Path, remote: &str) -> Result<String> {
    let capture = run_command(
        root,
        "git",
        &[
            "remote".to_string(),
            "get-url".to_string(),
            remote.to_string(),
        ],
    )?;
    if !capture.status.success() {
        bail!("git remote `{remote}` does not exist; add a remote or pass `--remote <remote>`");
    }

    let url = capture.stdout.trim().to_string();
    if url.is_empty() {
        bail!("git remote `{remote}` did not return a URL");
    }
    Ok(url)
}

fn validate_branch_head(root: &Path, branch: &str, commit_sha: &str) -> Result<()> {
    let capture = run_command(
        root,
        "git",
        &["rev-parse".to_string(), format!("refs/heads/{branch}")],
    )?;
    if !capture.status.success() {
        bail!(
            "failed to resolve candidate branch `{branch}`\n{}",
            capture.stderr.trim()
        );
    }

    let branch_head = capture.stdout.trim();
    if branch_head != commit_sha {
        bail!(
            "candidate branch `{branch}` HEAD `{branch_head}` does not match committed run SHA `{commit_sha}`"
        );
    }
    Ok(())
}

fn git_push(root: &Path, remote: &str, branch: &str) -> Result<()> {
    let args = vec![
        "push".to_string(),
        "-u".to_string(),
        remote.to_string(),
        branch.to_string(),
    ];
    let capture = run_command(root, "git", &args)?;
    if !capture.status.success() {
        bail!(
            "failed to publish candidate branch with `{}`\nstdout:\n{}\nstderr:\n{}",
            format_command("git", &args),
            capture.stdout.trim(),
            capture.stderr.trim()
        );
    }
    Ok(())
}

fn existing_publish(
    metadata: &RunMetadata,
    publish_path: &Path,
) -> Result<Option<PublishArtifact>> {
    if let Some(publish) = &metadata.publish {
        return Ok(Some(publish.clone()));
    }

    if metadata.published {
        if let (
            Some(published_at),
            Some(remote),
            Some(remote_url),
            Some(branch),
            Some(commit_sha),
        ) = (
            metadata.published_at.clone(),
            metadata.publish_remote.clone(),
            metadata.publish_remote_url.clone(),
            metadata.published_branch.clone(),
            metadata.commit_sha.clone(),
        ) {
            return Ok(Some(PublishArtifact {
                run_id: metadata.run_id.clone(),
                remote,
                remote_url,
                branch,
                commit_sha,
                pushed: true,
                published_at,
                dry_run: false,
            }));
        }
    }

    if publish_path.is_file() {
        return Ok(Some(read_json(publish_path)?));
    }

    Ok(None)
}

fn already_published_result(
    metadata: &RunMetadata,
    artifact: PublishArtifact,
    dry_run: bool,
    publish_path: &Path,
) -> PublishResult {
    PublishResult {
        run_id: metadata.run_id.clone(),
        remote: artifact.remote,
        remote_url: artifact.remote_url,
        branch: artifact.branch,
        commit_sha: artifact.commit_sha,
        pushed: true,
        dry_run,
        already_published: true,
        would_push: false,
        publish_path: Some(publish_path.display().to_string()),
    }
}

fn validate_remote_name(remote: &str) -> Result<()> {
    if remote.trim().is_empty() {
        bail!("remote name cannot be empty");
    }
    if remote.starts_with('-') || remote.chars().any(char::is_whitespace) {
        bail!("invalid remote name `{remote}`");
    }
    Ok(())
}
