use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct InitResult {
    pub root: PathBuf,
    pub keel_dir: PathBuf,
    pub config_path: PathBuf,
    pub runs_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ReportInfo {
    pub metadata: RunMetadata,
    pub path: PathBuf,
    pub summary: String,
    pub artifacts: Vec<ArtifactInfo>,
    pub next_actions: Vec<String>,
    pub is_discarded: bool,
}

#[derive(Debug, Clone)]
pub struct ArtifactInfo {
    pub label: &'static str,
    pub path: PathBuf,
    pub exists: bool,
}

#[derive(Debug, Clone)]
pub struct DiffInfo {
    pub path: PathBuf,
    pub content: String,
    pub is_empty: bool,
}

#[derive(Debug, Clone)]
pub struct LogInfo {
    pub path: PathBuf,
    pub content: String,
    pub is_empty: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Created,
    Running,
    Ready,
    NotReady,
    Discarded,
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Created => "created",
            Self::Running => "running",
            Self::Ready => "ready",
            Self::NotReady => "not_ready",
            Self::Discarded => "discarded",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMetadata {
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<String>,
    pub task: String,
    pub agent: String,
    pub status: RunStatus,
    pub created_at: String,
    pub updated_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub duration_ms: Option<u128>,
    pub worktree_path: String,
    pub run_dir: String,
    pub branch: String,
    pub base_commit: String,
    #[serde(default)]
    pub agent_command: Vec<String>,
    pub exit_code: Option<i32>,
    pub failure_reason: Option<FailureReason>,
    #[serde(default)]
    pub readiness_reason: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailureReason {
    MissingCli,
    NonzeroExit,
    Timeout,
    CheckFailed,
    AdapterError,
    EmptyDiff,
}

impl std::fmt::Display for FailureReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::MissingCli => "missing_cli",
            Self::NonzeroExit => "nonzero_exit",
            Self::Timeout => "timeout",
            Self::CheckFailed => "check_failed",
            Self::AdapterError => "adapter_error",
            Self::EmptyDiff => "empty_diff",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Passed,
    Failed,
    Skipped,
}

impl std::fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        };
        f.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub name: String,
    pub command: String,
    pub status: CheckStatus,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}
