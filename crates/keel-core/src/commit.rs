use crate::artifact_files;
use crate::command::{format_command, run_command};
use crate::git::{ensure_safe_run_id, ensure_safe_worktree_target};
use crate::model::{RunMetadata, RunStatus};
use crate::time::now_timestamp;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct CommitOptions {
    pub dry_run: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitArtifact {
    pub run_id: String,
    pub branch: String,
    pub worktree: String,
    pub commit_sha: String,
    pub commit_message: String,
    pub committed_at: String,
    pub had_uncommitted_changes: bool,
    pub warnings: Vec<String>,
    pub dry_run: bool,
}

impl CommitArtifact {
    pub fn from_metadata(metadata: &RunMetadata) -> Option<Self> {
        metadata
            .commit
            .clone()
            .or_else(|| Self::from_legacy_metadata(metadata))
    }

    pub(crate) fn commit_sha_from_metadata(metadata: &RunMetadata) -> Option<String> {
        metadata.recorded_commit_sha().map(str::to_string)
    }

    pub(crate) fn from_legacy_metadata(metadata: &RunMetadata) -> Option<Self> {
        if !metadata.committed {
            return None;
        }

        let commit_sha = Self::commit_sha_from_metadata(metadata)?;
        let commit_message = metadata.commit_message.clone()?;
        let committed_at = metadata.committed_at.clone()?;

        Some(Self {
            run_id: metadata.run_id.clone(),
            branch: metadata.branch.clone(),
            worktree: metadata.worktree_path.clone(),
            commit_sha,
            commit_message,
            committed_at,
            had_uncommitted_changes: false,
            warnings: metadata.warnings.clone(),
            dry_run: false,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CommitResult {
    pub run_id: String,
    pub branch: String,
    pub worktree: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
    pub commit_message: String,
    pub committed: bool,
    pub dry_run: bool,
    pub already_committed: bool,
    pub would_git_add: bool,
    pub would_git_commit: bool,
    pub had_uncommitted_changes: bool,
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_path: Option<String>,
}

pub(crate) fn commit_run(
    root: &Path,
    run_dir: &Path,
    worktree: &Path,
    metadata: &mut RunMetadata,
    options: CommitOptions,
) -> Result<CommitResult> {
    let commit_path = run_dir.join(artifact_files::COMMIT);
    ensure_safe_run_id(&metadata.run_id)?;

    if let Some(existing) = existing_commit(metadata, &commit_path)? {
        return Ok(already_committed_result(
            metadata,
            existing,
            options.dry_run,
            &commit_path,
        ));
    }

    validate_commit_preconditions(root, run_dir, worktree, metadata)?;
    let message = options
        .message
        .clone()
        .unwrap_or_else(|| default_commit_message(metadata));
    let had_uncommitted_changes = has_uncommitted_changes(worktree)?;
    if !had_uncommitted_changes {
        bail!(
            "run `{}` has no candidate changes to commit",
            metadata.run_id
        );
    }

    if options.dry_run {
        return Ok(CommitResult {
            run_id: metadata.run_id.clone(),
            branch: metadata.branch.clone(),
            worktree: metadata.worktree_path.clone(),
            commit_sha: None,
            commit_message: message,
            committed: false,
            dry_run: true,
            already_committed: false,
            would_git_add: true,
            would_git_commit: true,
            had_uncommitted_changes,
            warnings: metadata.warnings.clone(),
            commit_path: None,
        });
    }

    git_add_all(worktree)?;
    git_commit(worktree, &message)?;
    let commit_sha = git_stdout(worktree, &["rev-parse".to_string(), "HEAD".to_string()])
        .context("failed to read committed HEAD")?;
    let committed_at = now_timestamp();
    let artifact = CommitArtifact {
        run_id: metadata.run_id.clone(),
        branch: metadata.branch.clone(),
        worktree: metadata.worktree_path.clone(),
        commit_sha: commit_sha.clone(),
        commit_message: message.clone(),
        committed_at: committed_at.clone(),
        had_uncommitted_changes,
        warnings: metadata.warnings.clone(),
        dry_run: false,
    };

    metadata.record_commit(artifact);

    Ok(CommitResult {
        run_id: metadata.run_id.clone(),
        branch: metadata.branch.clone(),
        worktree: metadata.worktree_path.clone(),
        commit_sha: Some(commit_sha),
        commit_message: message,
        committed: true,
        dry_run: false,
        already_committed: false,
        would_git_add: false,
        would_git_commit: false,
        had_uncommitted_changes,
        warnings: metadata.warnings.clone(),
        commit_path: Some(commit_path.display().to_string()),
    })
}

pub(crate) fn write_commit_artifact(run_dir: &Path, artifact: &CommitArtifact) -> Result<()> {
    crate::json::write_json_pretty(&run_dir.join(artifact_files::COMMIT), artifact)
}

pub(crate) fn default_commit_message(metadata: &RunMetadata) -> String {
    let task = metadata
        .task
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if task.is_empty() {
        return format!("keel: candidate change {}", metadata.run_id);
    }

    let subject = format!("keel: {task}");
    truncate_subject(&subject, 72)
}

fn validate_commit_preconditions(
    root: &Path,
    run_dir: &Path,
    worktree: &Path,
    metadata: &RunMetadata,
) -> Result<()> {
    if metadata.status != RunStatus::Ready {
        bail!(
            "run `{}` has status `{}`; only ready runs can be committed",
            metadata.run_id,
            metadata.status
        );
    }

    ensure_safe_worktree_target(root, &metadata.run_id, worktree)?;
    if !worktree.is_dir() {
        bail!(
            "run `{}` has no candidate worktree at {}; cannot commit",
            metadata.run_id,
            worktree.display()
        );
    }

    let diff_path = run_dir.join(artifact_files::DIFF);
    if !diff_path.is_file() {
        bail!(
            "diff for run `{}` does not exist at {}; cannot commit",
            metadata.run_id,
            diff_path.display()
        );
    }
    let diff = fs::read_to_string(&diff_path)
        .with_context(|| format!("failed to read {}", diff_path.display()))?;
    if diff.trim().is_empty() {
        bail!("diff for run `{}` is empty; cannot commit", metadata.run_id);
    }

    let current_branch = git_stdout(
        worktree,
        &[
            "rev-parse".to_string(),
            "--abbrev-ref".to_string(),
            "HEAD".to_string(),
        ],
    )
    .context("failed to inspect candidate worktree branch")?;
    if current_branch != metadata.branch {
        bail!(
            "candidate worktree for run `{}` is on branch `{}` but metadata expects `{}`",
            metadata.run_id,
            current_branch,
            metadata.branch
        );
    }

    Ok(())
}

fn existing_commit(metadata: &RunMetadata, commit_path: &Path) -> Result<Option<CommitArtifact>> {
    if let Some(artifact) = CommitArtifact::from_metadata(metadata) {
        return Ok(Some(artifact));
    }

    if commit_path.is_file() {
        return Ok(Some(crate::json::read_json(commit_path)?));
    }

    Ok(None)
}

fn already_committed_result(
    metadata: &RunMetadata,
    artifact: CommitArtifact,
    dry_run: bool,
    commit_path: &Path,
) -> CommitResult {
    CommitResult {
        run_id: metadata.run_id.clone(),
        branch: artifact.branch,
        worktree: artifact.worktree,
        commit_sha: Some(artifact.commit_sha),
        commit_message: artifact.commit_message,
        committed: true,
        dry_run,
        already_committed: true,
        would_git_add: false,
        would_git_commit: false,
        had_uncommitted_changes: false,
        warnings: artifact.warnings,
        commit_path: Some(commit_path.display().to_string()),
    }
}

fn has_uncommitted_changes(worktree: &Path) -> Result<bool> {
    let status = git_stdout(worktree, &["status".to_string(), "--porcelain".to_string()])
        .context("failed to inspect candidate worktree status")?;
    Ok(!status.trim().is_empty())
}

fn git_add_all(worktree: &Path) -> Result<()> {
    let args = vec!["add".to_string(), "-A".to_string()];
    let capture = run_command(worktree, "git", &args)?;
    if !capture.status.success() {
        bail!(
            "failed to stage candidate changes with `{}`\n{}",
            format_command("git", &args),
            capture.stderr.trim()
        );
    }
    Ok(())
}

fn git_commit(worktree: &Path, message: &str) -> Result<()> {
    let args = vec!["commit".to_string(), "-m".to_string(), message.to_string()];
    let capture = run_command(worktree, "git", &args)?;
    if !capture.status.success() {
        bail!(
            "failed to create local commit with `{}`\n{}",
            format_command("git", &args),
            capture.stderr.trim()
        );
    }
    Ok(())
}

fn git_stdout(worktree: &Path, args: &[String]) -> Result<String> {
    let capture = run_command(worktree, "git", args)?;
    if !capture.status.success() {
        bail!("{}", capture.stderr.trim());
    }
    Ok(capture.stdout.trim().to_string())
}

fn truncate_subject(subject: &str, max_chars: usize) -> String {
    if subject.chars().count() <= max_chars {
        return subject.to_string();
    }
    subject.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_commit_message_normalizes_and_truncates_task() {
        let metadata = metadata_with_task("fix\nlogin   bug with a task that is deliberately much longer than seventy two characters");

        let message = default_commit_message(&metadata);

        assert!(message.starts_with("keel: fix login bug"));
        assert!(message.chars().count() <= 72);
        assert!(!message.contains('\n'));
    }

    #[test]
    fn default_commit_message_handles_empty_task() {
        let metadata = metadata_with_task("   ");

        assert_eq!(
            default_commit_message(&metadata),
            "keel: candidate change run-test"
        );
    }

    fn metadata_with_task(task: &str) -> RunMetadata {
        let mut metadata = RunMetadata::new("run-test", task, "noop", RunStatus::Ready, "1");
        metadata.exit_code = Some(0);
        metadata
    }
}
