use crate::constants::{CONFIG_FILE, KEEL_DIR, RUNS_DIR, WORKTREES_DIR};
use serde::Serialize;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct DoctorReport {
    pub ok: bool,
    pub summary: DoctorSummary,
    pub checks: Vec<DoctorCheck>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorSummary {
    pub ok: usize,
    pub warnings: usize,
    pub errors: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorCheck {
    pub id: String,
    pub group: String,
    pub label: String,
    pub status: DoctorStatus,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DoctorStatus {
    Ok,
    Warning,
    Error,
}

impl std::fmt::Display for DoctorStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Ok => "ok",
            Self::Warning => "warning",
            Self::Error => "error",
        };
        f.write_str(value)
    }
}

pub fn run_doctor(project_root: &Path) -> DoctorReport {
    let mut checks = Vec::new();

    let git_path = which::which("git").ok();
    checks.push(match &git_path {
        Some(path) => DoctorCheck::ok(
            "repository.git_command",
            "Repository",
            "git command",
            "git found",
            Some(path.display().to_string()),
        ),
        None => DoctorCheck::error(
            "repository.git_command",
            "Repository",
            "git command",
            "git not found in PATH",
            None,
        ),
    });

    let inside_git_repo = git_success(
        project_root,
        git_path.as_deref(),
        &["rev-parse", "--is-inside-work-tree"],
    );
    checks.push(if inside_git_repo {
        DoctorCheck::ok(
            "repository.git_repo",
            "Repository",
            "git repository",
            "git repository detected",
            None,
        )
    } else {
        DoctorCheck::error(
            "repository.git_repo",
            "Repository",
            "git repository",
            "current directory is not inside a git repository",
            None,
        )
    });

    checks.push(
        if git_success(project_root, git_path.as_deref(), &["worktree", "list"]) {
            DoctorCheck::ok(
                "repository.git_worktree",
                "Repository",
                "git worktree",
                "git worktree available",
                None,
            )
        } else {
            DoctorCheck::error(
                "repository.git_worktree",
                "Repository",
                "git worktree",
                "git worktree command failed",
                None,
            )
        },
    );

    checks.push(if inside_git_repo {
        let status = git_stdout(
            project_root,
            git_path.as_deref(),
            &["status", "--porcelain"],
        );
        match status {
            Some(output) if output.trim().is_empty() => DoctorCheck::ok(
                "repository.working_tree_clean",
                "Repository",
                "working tree",
                "working tree is clean",
                None,
            ),
            Some(_) => DoctorCheck::warning(
                "repository.working_tree_clean",
                "Repository",
                "working tree",
                "working tree has uncommitted changes",
                None,
            ),
            None => DoctorCheck::error(
                "repository.working_tree_clean",
                "Repository",
                "working tree",
                "failed to inspect working tree",
                None,
            ),
        }
    } else {
        DoctorCheck::error(
            "repository.working_tree_clean",
            "Repository",
            "working tree",
            "cannot inspect working tree outside a git repository",
            None,
        )
    });

    checks.extend(keel_checks(project_root));
    checks.extend(agent_checks());

    DoctorReport::from_checks(checks)
}

fn keel_checks(project_root: &Path) -> Vec<DoctorCheck> {
    [
        (
            "keel.directory",
            ".keel directory",
            project_root.join(KEEL_DIR),
            ".keel directory found",
            ".keel directory is missing; run `keel init` first",
            true,
        ),
        (
            "keel.config",
            ".keel/config.toml",
            project_root.join(KEEL_DIR).join(CONFIG_FILE),
            ".keel/config.toml found",
            ".keel/config.toml is missing; run `keel init` first",
            false,
        ),
        (
            "keel.runs",
            ".keel/runs directory",
            project_root.join(KEEL_DIR).join(RUNS_DIR),
            ".keel/runs directory found",
            ".keel/runs directory is missing; run `keel init` first",
            true,
        ),
        (
            "keel.worktrees",
            ".keel/worktrees directory",
            project_root.join(KEEL_DIR).join(WORKTREES_DIR),
            ".keel/worktrees directory found",
            ".keel/worktrees directory is missing; run `keel init` first",
            true,
        ),
    ]
    .into_iter()
    .map(
        |(id, label, path, ok_message, error_message, should_be_dir)| {
            let exists = if should_be_dir {
                path.is_dir()
            } else {
                path.is_file()
            };
            if exists {
                DoctorCheck::ok(
                    id,
                    "Keel",
                    label,
                    ok_message,
                    Some(path.display().to_string()),
                )
            } else {
                DoctorCheck::error(
                    id,
                    "Keel",
                    label,
                    error_message,
                    Some(path.display().to_string()),
                )
            }
        },
    )
    .collect()
}

fn agent_checks() -> Vec<DoctorCheck> {
    ["codex", "claude", "opencode"]
        .into_iter()
        .map(|agent| match which::which(agent) {
            Ok(path) => DoctorCheck::ok(
                format!("agents.{agent}"),
                "Agents",
                format!("{agent} CLI"),
                format!("{agent} found"),
                Some(path.display().to_string()),
            ),
            Err(_) => DoctorCheck::warning(
                format!("agents.{agent}"),
                "Agents",
                format!("{agent} CLI"),
                format!("{agent} not found in PATH"),
                None,
            ),
        })
        .collect()
}

fn git_success(project_root: &Path, git_path: Option<&Path>, args: &[&str]) -> bool {
    let Some(git_path) = git_path else {
        return false;
    };
    Command::new(git_path)
        .args(args)
        .current_dir(project_root)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn git_stdout(project_root: &Path, git_path: Option<&Path>, args: &[&str]) -> Option<String> {
    let git_path = git_path?;
    let output = Command::new(git_path)
        .args(args)
        .current_dir(project_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

impl DoctorReport {
    fn from_checks(checks: Vec<DoctorCheck>) -> Self {
        let summary = DoctorSummary {
            ok: checks
                .iter()
                .filter(|check| check.status == DoctorStatus::Ok)
                .count(),
            warnings: checks
                .iter()
                .filter(|check| check.status == DoctorStatus::Warning)
                .count(),
            errors: checks
                .iter()
                .filter(|check| check.status == DoctorStatus::Error)
                .count(),
        };
        Self {
            ok: summary.errors == 0,
            summary,
            checks,
        }
    }
}

impl DoctorCheck {
    fn ok(
        id: impl Into<String>,
        group: impl Into<String>,
        label: impl Into<String>,
        message: impl Into<String>,
        details: Option<String>,
    ) -> Self {
        Self::new(id, group, label, DoctorStatus::Ok, message, details)
    }

    fn warning(
        id: impl Into<String>,
        group: impl Into<String>,
        label: impl Into<String>,
        message: impl Into<String>,
        details: Option<String>,
    ) -> Self {
        Self::new(id, group, label, DoctorStatus::Warning, message, details)
    }

    fn error(
        id: impl Into<String>,
        group: impl Into<String>,
        label: impl Into<String>,
        message: impl Into<String>,
        details: Option<String>,
    ) -> Self {
        Self::new(id, group, label, DoctorStatus::Error, message, details)
    }

    fn new(
        id: impl Into<String>,
        group: impl Into<String>,
        label: impl Into<String>,
        status: DoctorStatus,
        message: impl Into<String>,
        details: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            group: group.into(),
            label: label.into(),
            status,
            message: message.into(),
            details,
        }
    }
}
