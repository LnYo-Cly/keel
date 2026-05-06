use crate::constants::{artifact_keys, run_artifact_spec};
use crate::fsio::write_text;
use crate::ledger::{LedgerEvidenceBrief, LedgerHandoff, LedgerReview, LedgerTaskSummary};
use crate::model::{ArtifactInfo, ReportInfo, RunMetadata};
use crate::risk::RiskWarning;
use crate::{CommitArtifact, PrArtifact, PushArtifact};
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
    ReportJson::from(report)
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

impl From<&ReportInfo> for ReportJson {
    fn from(report: &ReportInfo) -> Self {
        let metadata = &report.metadata;
        Self {
            run_id: metadata.run_id.clone(),
            parent_run_id: metadata.parent_run_id.clone(),
            task: metadata.task.clone(),
            agent: metadata.agent.clone(),
            status: metadata.status.to_string(),
            created_at: metadata.created_at.clone(),
            worktree: metadata.worktree_path.clone(),
            branch: metadata.branch.clone(),
            base_commit: metadata.base_commit.clone(),
            failure_reason: metadata.failure_reason.as_ref().map(ToString::to_string),
            readiness_reason: metadata.readiness_reason.clone(),
            warnings: metadata.warnings.clone(),
            risk_warnings: metadata.risk_warnings.clone(),
            commit: report.commit.clone(),
            push: report.push.clone(),
            pr: report.pr.clone(),
            artifacts: ArtifactSetJson::from(report.artifacts.as_slice()),
            next_actions: report.next_actions.clone(),
        }
    }
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

impl From<&[ArtifactInfo]> for ArtifactSetJson {
    fn from(artifacts: &[ArtifactInfo]) -> Self {
        Self {
            metadata: artifact_json_by_key(artifacts, artifact_keys::METADATA),
            log: artifact_json_by_key(artifacts, artifact_keys::LOG),
            diff: artifact_json_by_key(artifacts, artifact_keys::DIFF),
            checks: artifact_json_by_key(artifacts, artifact_keys::CHECKS),
            report: artifact_json_by_key(artifacts, artifact_keys::REPORT),
            commit: artifact_json_by_key(artifacts, artifact_keys::COMMIT),
            push: artifact_json_by_key(artifacts, artifact_keys::PUSH),
            pr: artifact_json_by_key(artifacts, artifact_keys::PR),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ArtifactJson {
    key: &'static str,
    label: &'static str,
    path: String,
    exists: bool,
    state: &'static str,
    required: bool,
}

#[derive(Debug, Serialize)]
pub struct LedgerReviewJson {
    task: LedgerTaskSummary,
    summary: crate::ledger::LedgerSummary,
    decision: crate::ledger::LedgerDecision,
    workspace: Option<crate::ledger::WorkspaceContext>,
    packet: crate::ledger::LedgerReviewPacket,
    next_actions: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct LedgerHandoffJson {
    task: LedgerTaskSummary,
    summary: crate::ledger::LedgerSummary,
    workspace: Option<crate::ledger::WorkspaceContext>,
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

fn artifact_json(artifacts: &[ArtifactInfo], spec: &crate::RunArtifactSpec) -> ArtifactJson {
    artifacts
        .iter()
        .find(|artifact| artifact.key == spec.key)
        .map(|artifact| ArtifactJson {
            key: artifact.key,
            label: artifact.label,
            path: artifact.path.display().to_string(),
            exists: artifact.exists,
            state: artifact.state(),
            required: artifact.required,
        })
        .unwrap_or_else(|| ArtifactJson {
            key: spec.key,
            label: spec.label,
            path: String::new(),
            exists: false,
            state: "missing",
            required: spec.required,
        })
}

fn artifact_json_by_key(artifacts: &[ArtifactInfo], key: &'static str) -> ArtifactJson {
    let Some(spec) = run_artifact_spec(key) else {
        return ArtifactJson {
            key,
            label: key,
            path: String::new(),
            exists: false,
            state: "missing",
            required: false,
        };
    };

    artifact_json(artifacts, spec)
}
