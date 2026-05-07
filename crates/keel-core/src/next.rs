use crate::ledger::{LedgerDecision, LedgerReview, LedgerStatus, LedgerTaskStatus};
use crate::model::{ReportInfo, RunStatus};
use crate::report::primary_next_action;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowNext {
    pub ledger: LedgerNext,
    pub candidate: Option<CandidateNext>,
    pub recommended_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LedgerNext {
    pub active: bool,
    pub task_id: Option<String>,
    pub title: Option<String>,
    pub status: Option<LedgerTaskStatus>,
    pub decision: Option<LedgerDecision>,
    pub workspace_dirty: Option<bool>,
    pub headline: Option<String>,
    pub primary_action: String,
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CandidateNext {
    pub run_id: String,
    pub task: String,
    pub agent: String,
    pub status: RunStatus,
    pub branch: String,
    pub primary_action: Option<String>,
    pub actions: Vec<String>,
}

pub(crate) fn workflow_next(
    ledger_status: LedgerStatus,
    ledger_review: Option<LedgerReview>,
    latest_report: Option<ReportInfo>,
) -> WorkflowNext {
    let ledger = LedgerNext::from_status_and_review(ledger_status, ledger_review);
    let candidate = latest_report.map(CandidateNext::from_report);
    let recommended_actions = recommended_actions(&ledger, candidate.as_ref());

    WorkflowNext {
        ledger,
        candidate,
        recommended_actions,
    }
}

impl LedgerNext {
    fn from_status_and_review(status: LedgerStatus, review: Option<LedgerReview>) -> Self {
        let Some(active_task) = status.active_task else {
            let action = "keel task start \"...\"".to_string();
            return Self {
                active: false,
                task_id: None,
                title: None,
                status: None,
                decision: None,
                workspace_dirty: None,
                headline: None,
                primary_action: action.clone(),
                actions: vec![action, "keel task status".to_string()],
            };
        };

        let mut actions = Vec::new();
        let mut decision = None;
        let mut workspace_dirty = None;
        let mut headline = None;
        if let Some(review) = review {
            decision = Some(review.decision);
            workspace_dirty = review.workspace.as_ref().map(|workspace| workspace.dirty);
            headline = Some(review.packet.headline);
            extend_unique(&mut actions, review.packet.suggested_commands);
            extend_unique(&mut actions, review.next_actions);
        }
        if actions.is_empty() {
            actions.push("keel review".to_string());
        }

        let primary_action = primary_command(&actions)
            .cloned()
            .unwrap_or_else(|| actions[0].clone());

        Self {
            active: true,
            task_id: Some(active_task.task_id),
            title: Some(active_task.title),
            status: Some(active_task.status),
            decision,
            workspace_dirty,
            headline,
            primary_action,
            actions,
        }
    }
}

impl CandidateNext {
    fn from_report(report: ReportInfo) -> Self {
        let metadata = report.metadata;
        let primary_action = primary_next_action(&metadata).map(|action| action.command);
        Self {
            run_id: metadata.run_id,
            task: metadata.task,
            agent: metadata.agent,
            status: metadata.status,
            branch: metadata.branch,
            primary_action,
            actions: report.next_actions,
        }
    }
}

fn recommended_actions(ledger: &LedgerNext, candidate: Option<&CandidateNext>) -> Vec<String> {
    let mut actions = Vec::new();
    if ledger.active {
        push_unique(&mut actions, ledger.primary_action.clone());
    }
    if let Some(candidate) = candidate {
        if let Some(action) = &candidate.primary_action {
            push_unique(&mut actions, action.clone());
        }
    }
    if !ledger.active {
        push_unique(&mut actions, ledger.primary_action.clone());
    }
    if candidate.is_none() {
        push_unique(&mut actions, "keel run \"...\" --agent noop".to_string());
    }
    actions
}

fn primary_command(actions: &[String]) -> Option<&String> {
    actions
        .iter()
        .find(|action| action.starts_with("keel ") || action.starts_with("git "))
}

fn extend_unique(actions: &mut Vec<String>, new_actions: Vec<String>) {
    for action in new_actions {
        push_unique(actions, action);
    }
}

fn push_unique(actions: &mut Vec<String>, action: String) {
    if !actions.contains(&action) {
        actions.push(action);
    }
}
