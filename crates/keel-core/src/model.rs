use crate::commit::CommitArtifact;
use crate::constants::{artifact_keys, KEEL_DIR, RUNS_DIR, WORKTREES_DIR};
use crate::pr::PrArtifact;
use crate::push::PushArtifact;
use crate::risk::RiskWarning;
use crate::RunArtifactSpec;
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
    pub commit: Option<CommitArtifact>,
    pub push: Option<PushArtifact>,
    pub pr: Option<PrArtifact>,
    pub next_actions: Vec<String>,
    pub is_discarded: bool,
}

impl ReportInfo {
    pub fn new(
        metadata: RunMetadata,
        path: PathBuf,
        summary: impl Into<String>,
        artifacts: Vec<ArtifactInfo>,
        next_actions: Vec<String>,
    ) -> Self {
        let commit = CommitArtifact::from_metadata(&metadata);
        let push = PushArtifact::from_metadata(&metadata);
        let pr = PrArtifact::from_metadata(&metadata).ok().flatten();
        let is_discarded = metadata.status == RunStatus::Discarded;

        Self {
            metadata,
            path,
            summary: summary.into(),
            artifacts,
            commit,
            push,
            pr,
            next_actions,
            is_discarded,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ArtifactInfo {
    pub key: &'static str,
    pub label: &'static str,
    pub path: PathBuf,
    pub exists: bool,
    pub required: bool,
}

impl ArtifactInfo {
    pub fn new(
        key: &'static str,
        label: &'static str,
        path: PathBuf,
        exists: bool,
        required: bool,
    ) -> Self {
        Self {
            key,
            label,
            path,
            exists,
            required,
        }
    }

    pub fn from_spec(spec: &RunArtifactSpec, path: PathBuf, exists: bool) -> Self {
        Self::new(spec.key, spec.label, path, exists, spec.required)
    }

    pub fn required(key: &'static str, label: &'static str, path: PathBuf, exists: bool) -> Self {
        Self::new(key, label, path, exists, true)
    }

    pub fn optional(key: &'static str, label: &'static str, path: PathBuf, exists: bool) -> Self {
        Self::new(key, label, path, exists, false)
    }

    pub fn state(&self) -> &'static str {
        if self.exists {
            "present"
        } else {
            "missing"
        }
    }
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

#[derive(Debug, Clone)]
pub struct RunArtifacts {
    pub report: ReportInfo,
    pub report_content: Option<String>,
    pub diff: Option<DiffInfo>,
    pub log: Option<LogInfo>,
    pub checks: Option<Vec<CheckResult>>,
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
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub risk_warnings: Vec<RiskWarning>,
    #[serde(default)]
    pub committed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub committed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<CommitArtifact>,
    #[serde(default, alias = "published")]
    pub pushed: bool,
    #[serde(
        default,
        alias = "published_at",
        skip_serializing_if = "Option::is_none"
    )]
    pub pushed_at: Option<String>,
    #[serde(
        default,
        alias = "publish_remote",
        skip_serializing_if = "Option::is_none"
    )]
    pub push_remote: Option<String>,
    #[serde(
        default,
        alias = "publish_remote_url",
        skip_serializing_if = "Option::is_none"
    )]
    pub push_remote_url: Option<String>,
    #[serde(
        default,
        alias = "published_branch",
        skip_serializing_if = "Option::is_none"
    )]
    pub pushed_branch: Option<String>,
    #[serde(default, alias = "publish", skip_serializing_if = "Option::is_none")]
    pub push: Option<PushArtifact>,
    #[serde(default)]
    pub pr_created: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_target_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_source_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr: Option<PrArtifact>,
}

impl RunMetadata {
    pub fn new(
        run_id: impl Into<String>,
        task: impl Into<String>,
        agent: impl Into<String>,
        status: RunStatus,
        created_at: impl Into<String>,
    ) -> Self {
        let run_id = run_id.into();
        let created_at = created_at.into();

        Self {
            parent_run_id: None,
            task: task.into(),
            agent: agent.into(),
            status,
            created_at: created_at.clone(),
            updated_at: created_at,
            worktree_path: format!("{KEEL_DIR}/{WORKTREES_DIR}/{run_id}"),
            run_dir: format!("{KEEL_DIR}/{RUNS_DIR}/{run_id}"),
            branch: format!("keel/run/{run_id}"),
            base_commit: String::new(),
            run_id,
            started_at: None,
            finished_at: None,
            duration_ms: None,
            agent_command: Vec::new(),
            exit_code: None,
            failure_reason: None,
            readiness_reason: String::new(),
            warnings: Vec::new(),
            risk_warnings: Vec::new(),
            committed: false,
            commit_sha: None,
            commit_message: None,
            committed_at: None,
            commit: None,
            pushed: false,
            pushed_at: None,
            push_remote: None,
            push_remote_url: None,
            pushed_branch: None,
            push: None,
            pr_created: false,
            pr_created_at: None,
            pr_provider: None,
            pr_url: None,
            pr_target_branch: None,
            pr_source_branch: None,
            pr: None,
        }
    }

    pub fn with_parent_run_id(mut self, parent_run_id: Option<String>) -> Self {
        self.parent_run_id = parent_run_id;
        self
    }

    pub fn with_base_commit(mut self, base_commit: impl Into<String>) -> Self {
        self.base_commit = base_commit.into();
        self
    }

    pub fn with_readiness_reason(mut self, readiness_reason: impl Into<String>) -> Self {
        self.readiness_reason = readiness_reason.into();
        self
    }

    pub fn recorded_commit_artifact(&self) -> Option<CommitArtifact> {
        CommitArtifact::from_metadata(self)
    }

    pub fn recorded_push_artifact(&self) -> Option<PushArtifact> {
        PushArtifact::from_metadata(self)
    }

    pub fn recorded_pr_artifact(&self) -> Option<PrArtifact> {
        PrArtifact::from_metadata(self).ok().flatten()
    }

    pub fn recorded_commit_sha(&self) -> Option<&str> {
        non_empty(self.commit_sha.as_deref()).or_else(|| {
            self.commit
                .as_ref()
                .map(|commit| commit.commit_sha.as_str())
                .and_then(|commit_sha| non_empty(Some(commit_sha)))
        })
    }

    pub fn recorded_push_remote(&self) -> Option<&str> {
        self.push
            .as_ref()
            .map(|push| push.remote.as_str())
            .and_then(|remote| non_empty(Some(remote)))
            .or_else(|| non_empty(self.push_remote.as_deref()))
    }

    pub fn recorded_push_remote_url(&self) -> Option<&str> {
        self.push
            .as_ref()
            .map(|push| push.remote_url.as_str())
            .and_then(|remote_url| non_empty(Some(remote_url)))
            .or_else(|| non_empty(self.push_remote_url.as_deref()))
    }

    pub fn recorded_pushed_branch(&self) -> Option<&str> {
        self.push
            .as_ref()
            .map(|push| push.branch.as_str())
            .and_then(|branch| non_empty(Some(branch)))
            .or_else(|| non_empty(self.pushed_branch.as_deref()))
    }

    pub fn recorded_pr_provider(&self) -> Option<String> {
        self.pr
            .as_ref()
            .map(|pr| pr.provider.to_string())
            .filter(|provider| !provider.trim().is_empty())
            .or_else(|| non_empty(self.pr_provider.as_deref()).map(str::to_string))
    }

    pub fn recorded_pr_url(&self) -> Option<&str> {
        self.pr
            .as_ref()
            .map(|pr| pr.url.as_str())
            .and_then(|url| non_empty(Some(url)))
            .or_else(|| non_empty(self.pr_url.as_deref()))
    }

    pub fn review_search_terms(&self) -> Vec<String> {
        let mut terms = vec![
            self.run_id.clone(),
            self.task.clone(),
            self.agent.clone(),
            self.status.to_string(),
            self.branch.clone(),
            self.base_commit.clone(),
            self.failure_reason
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
            self.readiness_reason.clone(),
        ];

        terms.extend(
            [
                self.recorded_commit_sha(),
                self.recorded_push_remote(),
                self.recorded_push_remote_url(),
                self.recorded_pr_url(),
            ]
            .into_iter()
            .flatten()
            .map(str::to_string),
        );
        if let Some(provider) = self.recorded_pr_provider() {
            terms.push(provider);
        }

        terms
    }

    pub fn has_commit_record(&self) -> bool {
        self.committed || self.recorded_commit_sha().is_some() || self.commit.is_some()
    }

    pub fn has_push_record(&self) -> bool {
        self.pushed || self.push.is_some()
    }

    pub fn has_pr_record(&self) -> bool {
        self.pr_created || self.pr.is_some()
    }

    pub fn run_artifact_recorded(&self, artifact_key: &str) -> bool {
        match artifact_key {
            artifact_keys::COMMIT => self.has_commit_record(),
            artifact_keys::PUSH => self.has_push_record(),
            artifact_keys::PR => self.has_pr_record(),
            _ => true,
        }
    }

    pub fn record_commit(&mut self, artifact: CommitArtifact) {
        self.committed = true;
        self.commit_sha = Some(artifact.commit_sha.clone());
        self.commit_message = Some(artifact.commit_message.clone());
        self.committed_at = Some(artifact.committed_at.clone());
        self.commit = Some(artifact);
    }

    pub fn record_push(&mut self, artifact: PushArtifact) {
        self.pushed = true;
        self.pushed_at = Some(artifact.pushed_at.clone());
        self.push_remote = Some(artifact.remote.clone());
        self.push_remote_url = Some(artifact.remote_url.clone());
        self.pushed_branch = Some(artifact.branch.clone());
        self.push = Some(artifact);
    }

    pub fn record_pr(&mut self, artifact: PrArtifact) {
        self.pr_created = true;
        self.pr_created_at = Some(artifact.created_at.clone());
        self.pr_provider = Some(artifact.provider.to_string());
        self.pr_url = Some(artifact.url.clone());
        self.pr_target_branch = Some(artifact.target_branch.clone());
        self.pr_source_branch = Some(artifact.source_branch.clone());
        self.pr = Some(artifact);
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.trim().is_empty())
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
