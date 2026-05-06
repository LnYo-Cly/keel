use crate::artifact_files;
use crate::command::{format_command, run_command};
use crate::commit::CommitArtifact;
use crate::constants::LEGACY_PUBLISH_FILE;
use crate::git::{ensure_safe_run_id, expected_run_branch};
use crate::json::read_json;
use crate::model::{RunMetadata, RunStatus};
use crate::time::now_timestamp;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PushOptions {
    pub remote: String,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushArtifact {
    pub run_id: String,
    pub remote: String,
    pub remote_url: String,
    pub branch: String,
    pub commit_sha: String,
    #[serde(alias = "published")]
    pub pushed: bool,
    #[serde(alias = "published_at")]
    pub pushed_at: String,
    pub dry_run: bool,
}

impl PushArtifact {
    pub fn from_metadata(metadata: &RunMetadata) -> Option<Self> {
        metadata
            .push
            .clone()
            .or_else(|| Self::from_legacy_metadata(metadata))
    }

    pub(crate) fn from_legacy_metadata(metadata: &RunMetadata) -> Option<Self> {
        if !metadata.pushed {
            return None;
        }

        let pushed_at = metadata.pushed_at.clone()?;
        let remote = metadata.recorded_push_remote()?.to_string();
        let remote_url = metadata.recorded_push_remote_url()?.to_string();
        let branch = metadata.recorded_pushed_branch()?.to_string();
        let commit_sha = metadata.recorded_commit_sha()?.to_string();

        Some(Self {
            run_id: metadata.run_id.clone(),
            remote,
            remote_url,
            branch,
            commit_sha,
            pushed: true,
            pushed_at,
            dry_run: false,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PushResult {
    pub run_id: String,
    pub remote: String,
    pub remote_url: String,
    pub branch: String,
    pub commit_sha: String,
    pub pushed: bool,
    pub dry_run: bool,
    pub already_pushed: bool,
    pub would_push: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub push_path: Option<String>,
}

pub(crate) fn push_run(
    root: &Path,
    run_dir: &Path,
    metadata: &mut RunMetadata,
    options: PushOptions,
) -> Result<PushResult> {
    let push_path = run_dir.join(artifact_files::PUSH);
    ensure_safe_run_id(&metadata.run_id)?;
    validate_remote_name(&options.remote)?;
    validate_push_identity(metadata)?;

    if let Some(existing) = existing_push(metadata, run_dir, &push_path)? {
        return Ok(already_pushed_result(
            metadata,
            &existing.artifact,
            options.dry_run,
            &existing.path,
        ));
    }

    validate_push_preconditions(root, run_dir, metadata)?;
    let commit_sha = committed_sha(metadata, &run_dir.join(artifact_files::COMMIT))?;
    let remote_url = remote_url(root, &options.remote)?;
    validate_branch_head(root, &metadata.branch, &commit_sha)?;

    if options.dry_run {
        return Ok(PushResult {
            run_id: metadata.run_id.clone(),
            remote: options.remote,
            remote_url,
            branch: metadata.branch.clone(),
            commit_sha,
            pushed: false,
            dry_run: true,
            already_pushed: false,
            would_push: true,
            push_path: None,
        });
    }

    git_push(root, &options.remote, &metadata.branch)?;
    let pushed_at = now_timestamp();
    let artifact = PushArtifact {
        run_id: metadata.run_id.clone(),
        remote: options.remote.clone(),
        remote_url: remote_url.clone(),
        branch: metadata.branch.clone(),
        commit_sha: commit_sha.clone(),
        pushed: true,
        pushed_at: pushed_at.clone(),
        dry_run: false,
    };

    metadata.record_push(artifact);

    Ok(PushResult {
        run_id: metadata.run_id.clone(),
        remote: options.remote,
        remote_url,
        branch: metadata.branch.clone(),
        commit_sha,
        pushed: true,
        dry_run: false,
        already_pushed: false,
        would_push: false,
        push_path: Some(push_path.display().to_string()),
    })
}

pub(crate) fn write_push_artifact(run_dir: &Path, artifact: &PushArtifact) -> Result<()> {
    crate::json::write_json_pretty(&run_dir.join(artifact_files::PUSH), artifact)
}

fn validate_push_preconditions(root: &Path, run_dir: &Path, metadata: &RunMetadata) -> Result<()> {
    validate_push_identity(metadata)?;

    let _commit_sha =
        committed_sha(metadata, &run_dir.join(artifact_files::COMMIT)).with_context(|| {
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
            "candidate branch `{}` does not exist; cannot push run `{}`",
            metadata.branch,
            metadata.run_id
        );
    }

    Ok(())
}

fn validate_push_identity(metadata: &RunMetadata) -> Result<()> {
    if metadata.status != RunStatus::Ready {
        bail!(
            "run `{}` has status `{}`; only ready runs can be pushed",
            metadata.run_id,
            metadata.status
        );
    }

    if metadata.branch != expected_run_branch(&metadata.run_id)? {
        bail!(
            "refusing to push unexpected branch `{}` for run `{}`",
            metadata.branch,
            metadata.run_id
        );
    }
    Ok(())
}

fn committed_sha(metadata: &RunMetadata, commit_path: &Path) -> Result<String> {
    if let Some(commit_sha) = CommitArtifact::commit_sha_from_metadata(metadata) {
        return Ok(commit_sha);
    }

    if commit_path.is_file() {
        let commit: CommitArtifact = read_json(commit_path)?;
        if !commit.commit_sha.trim().is_empty() {
            return Ok(commit.commit_sha);
        }
    }

    bail!("missing committed candidate metadata")
}

fn remote_url(root: &Path, remote: &str) -> Result<String> {
    if let Some(configured_url) = configured_remote_url(root, remote)? {
        return Ok(configured_url);
    }

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

fn configured_remote_url(root: &Path, remote: &str) -> Result<Option<String>> {
    let capture = run_command(
        root,
        "git",
        &[
            "config".to_string(),
            "--get".to_string(),
            format!("remote.{remote}.url"),
        ],
    )?;
    if !capture.status.success() {
        return Ok(None);
    }

    let url = capture.stdout.trim().to_string();
    if url.is_empty() {
        Ok(None)
    } else {
        Ok(Some(url))
    }
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
            "failed to push candidate branch with `{}`\nstdout:\n{}\nstderr:\n{}",
            format_command("git", &args),
            capture.stdout.trim(),
            capture.stderr.trim()
        );
    }
    Ok(())
}

#[derive(Debug)]
struct ExistingPush {
    artifact: PushArtifact,
    path: PathBuf,
}

fn existing_push(
    metadata: &RunMetadata,
    run_dir: &Path,
    push_path: &Path,
) -> Result<Option<ExistingPush>> {
    if let Some(push) = PushArtifact::from_metadata(metadata) {
        return Ok(Some(ExistingPush {
            artifact: push,
            path: existing_push_artifact_path(run_dir, push_path),
        }));
    }

    if push_path.is_file() {
        return Ok(Some(ExistingPush {
            artifact: read_json(push_path)?,
            path: push_path.to_path_buf(),
        }));
    }

    let legacy_path = run_dir.join(LEGACY_PUBLISH_FILE);
    if legacy_path.is_file() {
        return Ok(Some(ExistingPush {
            artifact: read_json(&legacy_path)?,
            path: legacy_path,
        }));
    }

    Ok(None)
}

fn existing_push_artifact_path(run_dir: &Path, push_path: &Path) -> PathBuf {
    if push_path.is_file() {
        return push_path.to_path_buf();
    }

    let legacy_path = run_dir.join(LEGACY_PUBLISH_FILE);
    if legacy_path.is_file() {
        return legacy_path;
    }

    push_path.to_path_buf()
}

fn already_pushed_result(
    metadata: &RunMetadata,
    artifact: &PushArtifact,
    dry_run: bool,
    push_path: &Path,
) -> PushResult {
    PushResult {
        run_id: metadata.run_id.clone(),
        remote: artifact.remote.clone(),
        remote_url: artifact.remote_url.clone(),
        branch: artifact.branch.clone(),
        commit_sha: artifact.commit_sha.clone(),
        pushed: true,
        dry_run,
        already_pushed: true,
        would_push: false,
        push_path: Some(push_path.display().to_string()),
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
