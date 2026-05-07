use crate::config::{load_workspace_checks, WorkspaceCheckConfig};
use crate::ledger::{add_evidence, status, LedgerEvidenceStatus};
use anyhow::{bail, Result};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct WorkspaceCheckOptions {
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceCheckRun {
    pub ok: bool,
    pub dry_run: bool,
    pub task_id: String,
    pub summary: WorkspaceCheckSummary,
    pub commands: Vec<WorkspaceCheckCommand>,
    pub next_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceCheckSummary {
    pub planned: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceCheckCommand {
    pub name: String,
    pub command: String,
    pub status: WorkspaceCheckStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceCheckStatus {
    Planned,
    Passed,
    Failed,
    Skipped,
}

pub(crate) fn run_workspace_checks(
    root: &Path,
    options: WorkspaceCheckOptions,
) -> Result<WorkspaceCheckRun> {
    let ledger_status = status(root)?;
    let task_id = ledger_status
        .active_task
        .as_ref()
        .map(|task| task.task_id.clone())
        .ok_or_else(|| {
            anyhow::anyhow!("no active Keel task found; run `keel task start <title>` first")
        })?;
    let checks = load_workspace_checks(root)?;
    if checks.is_empty() {
        bail!("no workspace checks configured; add commands under [checks].commands");
    }

    let commands = checks
        .into_iter()
        .map(|check| run_one_check(root, check, options.dry_run))
        .collect::<Result<Vec<_>>>()?;
    let summary = WorkspaceCheckSummary::from_commands(&commands);
    let ok = summary.failed == 0;
    let next_actions = if ok {
        vec!["keel review".to_string(), "keel verify".to_string()]
    } else {
        vec![
            "fix the failed checks".to_string(),
            "keel check".to_string(),
            "keel verify".to_string(),
        ]
    };

    Ok(WorkspaceCheckRun {
        ok,
        dry_run: options.dry_run,
        task_id,
        summary,
        commands,
        next_actions,
    })
}

fn run_one_check(
    root: &Path,
    check: WorkspaceCheckConfig,
    dry_run: bool,
) -> Result<WorkspaceCheckCommand> {
    if let Some(path) = &check.run_if_path_exists {
        if !root.join(path).exists() {
            return Ok(WorkspaceCheckCommand {
                name: check.name,
                command: check.command,
                status: WorkspaceCheckStatus::Skipped,
                evidence_id: None,
                exit_code: None,
                skipped_reason: Some(format!("{path} does not exist")),
            });
        }
    }

    if dry_run {
        return Ok(WorkspaceCheckCommand {
            name: check.name,
            command: check.command,
            status: WorkspaceCheckStatus::Planned,
            evidence_id: None,
            exit_code: None,
            skipped_reason: None,
        });
    }

    let task = add_evidence(root, &check.command, Vec::new())?;
    let evidence = task
        .evidence
        .last()
        .ok_or_else(|| anyhow::anyhow!("workspace check did not record evidence"))?;
    Ok(WorkspaceCheckCommand {
        name: check.name,
        command: check.command,
        status: match evidence.status {
            LedgerEvidenceStatus::Passed => WorkspaceCheckStatus::Passed,
            LedgerEvidenceStatus::Failed => WorkspaceCheckStatus::Failed,
        },
        evidence_id: Some(evidence.evidence_id.clone()),
        exit_code: evidence.exit_code,
        skipped_reason: None,
    })
}

impl WorkspaceCheckSummary {
    fn from_commands(commands: &[WorkspaceCheckCommand]) -> Self {
        Self {
            planned: commands
                .iter()
                .filter(|command| command.status == WorkspaceCheckStatus::Planned)
                .count(),
            passed: commands
                .iter()
                .filter(|command| command.status == WorkspaceCheckStatus::Passed)
                .count(),
            failed: commands
                .iter()
                .filter(|command| command.status == WorkspaceCheckStatus::Failed)
                .count(),
            skipped: commands
                .iter()
                .filter(|command| command.status == WorkspaceCheckStatus::Skipped)
                .count(),
        }
    }
}
