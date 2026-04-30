use crate::command::{format_command, run_command};
use crate::constants::{KEEL_DIR, NOOP_OUTPUT_FILE, WORKTREES_DIR};
use crate::run::RunLog;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

pub(crate) fn prepare_untracked_for_diff(worktree: &Path, log: &mut RunLog) -> Result<()> {
    let noop_path = worktree.join(NOOP_OUTPUT_FILE);
    if noop_path.exists() {
        intent_to_add(worktree, &[NOOP_OUTPUT_FILE.to_string()], true, log)
            .context("failed to add noop output to candidate diff")?;
    }

    let ls_args = vec![
        "ls-files".to_string(),
        "--others".to_string(),
        "--exclude-standard".to_string(),
        "-z".to_string(),
    ];
    let ls_capture = run_command(worktree, "git", &ls_args)?;
    log.push_command(worktree, &format_command("git", &ls_args), &ls_capture);
    if !ls_capture.status.success() {
        bail!(
            "failed to list untracked files for diff\n{}",
            ls_capture.stderr.trim()
        );
    }

    let paths = ls_capture
        .stdout
        .split('\0')
        .filter(|path| !path.is_empty() && *path != NOOP_OUTPUT_FILE)
        .map(str::to_string)
        .collect::<Vec<_>>();
    if !paths.is_empty() {
        intent_to_add(worktree, &paths, false, log)?;
    }
    Ok(())
}

pub(crate) fn changed_paths(worktree: &Path) -> Result<Vec<String>> {
    let args = vec!["status".to_string(), "--short".to_string()];
    let capture = run_command(worktree, "git", &args)?;
    if !capture.status.success() {
        bail!("failed to inspect changed paths\n{}", capture.stderr.trim());
    }

    let mut paths = Vec::new();
    for line in capture.stdout.lines() {
        if line.len() < 4 {
            continue;
        }
        let path = line[3..].trim();
        if let Some((_, right)) = path.split_once(" -> ") {
            paths.push(right.to_string());
        } else {
            paths.push(path.to_string());
        }
    }
    Ok(paths)
}

pub(crate) fn collect_warnings(
    diff: &str,
    requires_non_empty_diff: bool,
    changed_paths: &[String],
) -> Vec<String> {
    let mut warnings = Vec::new();
    if diff.trim().is_empty() && !requires_non_empty_diff {
        warnings.push("candidate diff is empty".to_string());
    }
    warnings.extend(warnings_for_paths(changed_paths));
    warnings
}

pub(crate) fn ensure_safe_run_id(run_id: &str) -> Result<()> {
    if run_id.is_empty()
        || !run_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        bail!("invalid run id `{run_id}`");
    }
    Ok(())
}

pub(crate) fn expected_run_branch(run_id: &str) -> Result<String> {
    ensure_safe_run_id(run_id)?;
    Ok(format!("keel/run/{run_id}"))
}

pub(crate) fn ensure_safe_worktree_target(root: &Path, run_id: &str, target: &Path) -> Result<()> {
    ensure_safe_run_id(run_id)?;
    let expected = root.join(KEEL_DIR).join(WORKTREES_DIR).join(run_id);
    let expected_abs = absolutize(&expected)?;
    let target_abs = absolutize(target)?;
    if target_abs != expected_abs {
        bail!(
            "refusing to operate on unexpected worktree path {}; expected {}",
            target_abs.display(),
            expected_abs.display()
        );
    }
    Ok(())
}

fn intent_to_add(worktree: &Path, paths: &[String], force: bool, log: &mut RunLog) -> Result<()> {
    let mut args = vec!["add".to_string(), "--intent-to-add".to_string()];
    if force {
        args.push("--force".to_string());
    }
    args.push("--".to_string());
    args.extend(paths.iter().cloned());

    let capture = run_command(worktree, "git", &args)?;
    log.push_command(worktree, &format_command("git", &args), &capture);
    if !capture.status.success() {
        bail!("{}", capture.stderr.trim());
    }
    Ok(())
}

fn warnings_for_paths(paths: &[String]) -> Vec<String> {
    let mut warnings = Vec::new();
    for path in paths {
        let normalized = path.replace('\\', "/");
        let is_high_risk = normalized == "AGENTS.md"
            || normalized == "CLAUDE.md"
            || normalized == "COPILOT.md"
            || normalized.starts_with(".git")
            || normalized.starts_with(".keel")
            || normalized.starts_with(".github")
            || normalized.contains("/.git")
            || normalized.contains("/.keel");
        if is_high_risk {
            warnings.push(format!("high-risk path changed: {path}"));
        }
    }
    warnings
}

fn absolutize(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .context("failed to read current directory")?
            .join(path))
    }
}
