use crate::commit::CommitArtifact;
use crate::fsio::write_text;
use crate::ledger::{LedgerEvidenceBrief, LedgerHandoff, LedgerReview, LedgerTaskSummary};
use crate::model::{ArtifactInfo, ReportInfo, RunMetadata};
use crate::pr::{PrArtifact, PrProvider};
use crate::push::PushArtifact;
use crate::risk::RiskWarning;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

pub(crate) fn read_json<T>(path: &Path) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("failed to parse JSON {}", path.display()))
}

pub(crate) fn write_json_pretty<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    let content = serde_json::to_string_pretty(value)
        .with_context(|| format!("failed to serialize {}", path.display()))?;
    write_text(path, content + "\n")
}

pub fn status_json(runs: &[RunMetadata]) -> Vec<RunSummaryJson> {
    runs.iter().map(RunSummaryJson::from).collect()
}

pub fn report_json(report: &ReportInfo) -> ReportJson {
    ReportJson {
        run_id: report.metadata.run_id.clone(),
        parent_run_id: report.metadata.parent_run_id.clone(),
        task: report.metadata.task.clone(),
        agent: report.metadata.agent.clone(),
        status: report.metadata.status.to_string(),
        created_at: report.metadata.created_at.clone(),
        worktree: report.metadata.worktree_path.clone(),
        branch: report.metadata.branch.clone(),
        base_commit: report.metadata.base_commit.clone(),
        failure_reason: report
            .metadata
            .failure_reason
            .as_ref()
            .map(ToString::to_string),
        readiness_reason: report.metadata.readiness_reason.clone(),
        warnings: report.metadata.warnings.clone(),
        risk_warnings: report.metadata.risk_warnings.clone(),
        commit: report_commit_json(&report.metadata),
        push: report_push_json(&report.metadata),
        pr: report_pr_json(&report.metadata),
        artifacts: ArtifactSetJson::from_artifacts(&report.artifacts),
        next_actions: report.next_actions.clone(),
    }
}

pub fn ledger_review_json(review: &LedgerReview) -> LedgerReviewJson {
    LedgerReviewJson {
        task: LedgerTaskSummary::from_task(&review.task, Some(&review.task.task_id)),
        summary: review.summary.clone(),
        decision: review.decision.clone(),
        workspace: review.workspace.clone(),
        packet: review.packet.clone(),
        next_actions: review.next_actions.clone(),
    }
}

pub fn ledger_handoff_json(handoff: &LedgerHandoff) -> LedgerHandoffJson {
    LedgerHandoffJson {
        task: LedgerTaskSummary::from_task(&handoff.task, Some(&handoff.task.task_id)),
        summary: handoff.summary.clone(),
        workspace: handoff.workspace.clone(),
        packet: handoff.packet.clone(),
        last_checkpoint: handoff.last_checkpoint.clone(),
        recent_notes: handoff.recent_notes.clone(),
        recent_evidence: handoff.recent_evidence.iter().map(evidence_brief).collect(),
        next_actions: handoff.next_actions.clone(),
    }
}

#[derive(Debug, Serialize)]
pub struct RunSummaryJson {
    run_id: String,
    parent_run_id: Option<String>,
    task: String,
    agent: String,
    status: String,
    created_at: String,
    worktree: String,
    branch: String,
    failure_reason: Option<String>,
}

impl From<&RunMetadata> for RunSummaryJson {
    fn from(metadata: &RunMetadata) -> Self {
        Self {
            run_id: metadata.run_id.clone(),
            parent_run_id: metadata.parent_run_id.clone(),
            task: metadata.task.clone(),
            agent: metadata.agent.clone(),
            status: metadata.status.to_string(),
            created_at: metadata.created_at.clone(),
            worktree: metadata.worktree_path.clone(),
            branch: metadata.branch.clone(),
            failure_reason: metadata.failure_reason.as_ref().map(ToString::to_string),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ReportJson {
    run_id: String,
    parent_run_id: Option<String>,
    task: String,
    agent: String,
    status: String,
    created_at: String,
    worktree: String,
    branch: String,
    base_commit: String,
    failure_reason: Option<String>,
    readiness_reason: String,
    warnings: Vec<String>,
    risk_warnings: Vec<RiskWarning>,
    commit: Option<CommitArtifact>,
    push: Option<PushArtifact>,
    pr: Option<PrArtifact>,
    artifacts: ArtifactSetJson,
    next_actions: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ArtifactSetJson {
    metadata: ArtifactJson,
    log: ArtifactJson,
    diff: ArtifactJson,
    checks: ArtifactJson,
    report: ArtifactJson,
    commit: ArtifactJson,
    push: ArtifactJson,
    pr: ArtifactJson,
}

impl ArtifactSetJson {
    fn from_artifacts(artifacts: &[ArtifactInfo]) -> Self {
        Self {
            metadata: artifact_json(artifacts, "Metadata"),
            log: artifact_json(artifacts, "Log"),
            diff: artifact_json(artifacts, "Diff"),
            checks: artifact_json(artifacts, "Checks"),
            report: artifact_json(artifacts, "Report"),
            commit: artifact_json(artifacts, "Commit"),
            push: artifact_json(artifacts, "Push"),
            pr: artifact_json(artifacts, "PR/MR"),
        }
    }
}

fn report_commit_json(metadata: &RunMetadata) -> Option<CommitArtifact> {
    metadata.commit.clone().or_else(|| {
        Some(CommitArtifact {
            run_id: metadata.run_id.clone(),
            branch: metadata.branch.clone(),
            worktree: metadata.worktree_path.clone(),
            commit_sha: metadata.commit_sha.clone()?,
            commit_message: metadata.commit_message.clone()?,
            committed_at: metadata.committed_at.clone()?,
            had_uncommitted_changes: false,
            warnings: metadata.warnings.clone(),
            dry_run: false,
        })
    })
}

fn report_push_json(metadata: &RunMetadata) -> Option<PushArtifact> {
    metadata.push.clone().or_else(|| {
        Some(PushArtifact {
            run_id: metadata.run_id.clone(),
            remote: metadata.push_remote.clone()?,
            remote_url: metadata.push_remote_url.clone()?,
            branch: metadata.pushed_branch.clone()?,
            commit_sha: metadata.commit_sha.clone()?,
            pushed: true,
            pushed_at: metadata.pushed_at.clone()?,
            dry_run: false,
        })
    })
}

fn report_pr_json(metadata: &RunMetadata) -> Option<PrArtifact> {
    metadata.pr.clone().or_else(|| {
        let provider = metadata
            .pr_provider
            .as_deref()?
            .parse::<PrProvider>()
            .ok()?;
        Some(PrArtifact {
            run_id: metadata.run_id.clone(),
            provider,
            provider_name: provider.display_name().to_string(),
            request_kind: provider.request_kind().to_string(),
            remote: metadata
                .push_remote
                .clone()
                .unwrap_or_else(|| "unknown".to_string()),
            remote_url: metadata.push_remote_url.clone().unwrap_or_default(),
            repository_url: None,
            source_branch: metadata.pr_source_branch.clone()?,
            target_branch: metadata.pr_target_branch.clone()?,
            commit_sha: metadata.commit_sha.clone()?,
            title: metadata
                .commit_message
                .clone()
                .unwrap_or_else(|| format!("keel: {}", metadata.task)),
            url: metadata.pr_url.clone()?,
            created_at: metadata.pr_created_at.clone()?,
            draft: false,
            reused_existing: false,
            dry_run: false,
        })
    })
}

#[derive(Debug, Serialize)]
pub struct ArtifactJson {
    path: String,
    exists: bool,
    state: &'static str,
}

#[derive(Debug, Serialize)]
pub struct LedgerReviewJson {
    task: LedgerTaskSummary,
    summary: crate::ledger::LedgerSummary,
    decision: crate::ledger::LedgerDecision,
    workspace: crate::ledger::WorkspaceContext,
    packet: crate::ledger::LedgerReviewPacket,
    next_actions: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct LedgerHandoffJson {
    task: LedgerTaskSummary,
    summary: crate::ledger::LedgerSummary,
    workspace: crate::ledger::WorkspaceContext,
    packet: crate::ledger::LedgerReviewPacket,
    last_checkpoint: Option<crate::ledger::LedgerCheckpoint>,
    recent_notes: Vec<crate::ledger::LedgerNote>,
    recent_evidence: Vec<LedgerEvidenceBrief>,
    next_actions: Vec<String>,
}

fn evidence_brief(evidence: &crate::ledger::LedgerEvidence) -> LedgerEvidenceBrief {
    LedgerEvidenceBrief {
        command: evidence.command.clone(),
        status: evidence.status,
        exit_code: evidence.exit_code,
        started_at: evidence.started_at.clone(),
        env_keys: evidence
            .env
            .iter()
            .map(|variable| variable.key.clone())
            .collect(),
    }
}

fn artifact_json(artifacts: &[ArtifactInfo], label: &str) -> ArtifactJson {
    artifacts
        .iter()
        .find(|artifact| artifact.label == label)
        .map(|artifact| ArtifactJson {
            path: artifact.path.display().to_string(),
            exists: artifact.exists,
            state: if artifact.exists {
                "present"
            } else {
                "missing"
            },
        })
        .unwrap_or_else(|| ArtifactJson {
            path: String::new(),
            exists: false,
            state: "missing",
        })
}
