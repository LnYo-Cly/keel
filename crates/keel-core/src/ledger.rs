use crate::constants::{KEEL_DIR, REPORT_OUTPUT_LIMIT};
use crate::json::{read_json, write_json_pretty};
use crate::time::{now_timestamp, unix_millis};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

const LEDGER_DIR: &str = "ledger";
const LEDGER_TASKS_DIR: &str = "tasks";
const ACTIVE_TASK_FILE: &str = "active_task.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerTask {
    pub task_id: String,
    pub title: String,
    pub status: LedgerTaskStatus,
    pub created_at: String,
    pub updated_at: String,
    pub root: String,
    #[serde(default)]
    pub checkpoints: Vec<LedgerCheckpoint>,
    #[serde(default)]
    pub notes: Vec<LedgerNote>,
    #[serde(default)]
    pub evidence: Vec<LedgerEvidence>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LedgerTaskStatus {
    Active,
}

impl std::fmt::Display for LedgerTaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => f.write_str("active"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerCheckpoint {
    pub checkpoint_id: String,
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerNote {
    pub note_id: String,
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEvidence {
    pub evidence_id: String,
    pub command: String,
    pub status: LedgerEvidenceStatus,
    pub exit_code: Option<i32>,
    pub started_at: String,
    pub finished_at: String,
    pub duration_ms: u128,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<LedgerEvidenceEnv>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEvidenceEnv {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LedgerEvidenceStatus {
    Passed,
    Failed,
}

impl std::fmt::Display for LedgerEvidenceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Passed => f.write_str("passed"),
            Self::Failed => f.write_str("failed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveLedgerTask {
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LedgerReview {
    pub task: LedgerTask,
    pub summary: LedgerSummary,
    pub decision: LedgerDecision,
    pub workspace: WorkspaceContext,
    pub next_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LedgerSummary {
    pub checkpoints: usize,
    pub notes: usize,
    pub evidence: usize,
    pub evidence_passed: usize,
    pub evidence_failed: usize,
    pub current_evidence: usize,
    pub current_evidence_passed: usize,
    pub current_evidence_failed: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct LedgerDecision {
    pub ready: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LedgerHandoff {
    pub task: LedgerTask,
    pub summary: LedgerSummary,
    pub workspace: WorkspaceContext,
    pub last_checkpoint: Option<LedgerCheckpoint>,
    pub recent_notes: Vec<LedgerNote>,
    pub recent_evidence: Vec<LedgerEvidence>,
    pub next_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceContext {
    pub dirty: bool,
    pub changed_files: Vec<String>,
    pub git_status_short: String,
    pub git_diff_stat: String,
    pub git_status_error: Option<String>,
    pub git_diff_stat_error: Option<String>,
}

pub(crate) fn start_task(root: &Path, title: &str) -> Result<LedgerTask> {
    let title = title.trim();
    if title.is_empty() {
        bail!("task title cannot be empty");
    }

    fs::create_dir_all(tasks_dir(root))
        .with_context(|| format!("failed to create {}", tasks_dir(root).display()))?;

    let timestamp = now_timestamp();
    let task = LedgerTask {
        task_id: generate_task_id(),
        title: title.to_string(),
        status: LedgerTaskStatus::Active,
        created_at: timestamp.clone(),
        updated_at: timestamp,
        root: root.display().to_string(),
        checkpoints: Vec::new(),
        notes: Vec::new(),
        evidence: Vec::new(),
    };
    write_task(root, &task)?;
    write_json_pretty(
        &active_task_path(root),
        &ActiveLedgerTask {
            task_id: task.task_id.clone(),
        },
    )?;
    Ok(task)
}

pub(crate) fn add_checkpoint(root: &Path, message: &str) -> Result<LedgerTask> {
    let message = normalized_message(message, "checkpoint message")?;
    let mut task = read_active_task(root)?;
    task.checkpoints.push(LedgerCheckpoint {
        checkpoint_id: generate_event_id("checkpoint", task.checkpoints.len() + 1),
        message,
        created_at: now_timestamp(),
    });
    touch_and_write(root, task)
}

pub(crate) fn add_note(root: &Path, message: &str) -> Result<LedgerTask> {
    let message = normalized_message(message, "note message")?;
    let mut task = read_active_task(root)?;
    task.notes.push(LedgerNote {
        note_id: generate_event_id("note", task.notes.len() + 1),
        message,
        created_at: now_timestamp(),
    });
    touch_and_write(root, task)
}

pub(crate) fn add_evidence(
    root: &Path,
    command: &str,
    env: Vec<LedgerEvidenceEnv>,
) -> Result<LedgerTask> {
    let command = normalized_message(command, "evidence command")?;
    let mut task = read_active_task(root)?;
    let evidence = run_evidence_command(root, &command, env, task.evidence.len() + 1)?;
    task.evidence.push(evidence);
    touch_and_write(root, task)
}

pub(crate) fn review(root: &Path) -> Result<LedgerReview> {
    let task = read_active_task(root)?;
    let summary = summarize_task(&task);
    let decision = decision_for_summary(&summary);
    Ok(LedgerReview {
        task,
        summary,
        decision,
        workspace: workspace_context(root),
        next_actions: review_next_actions(),
    })
}

pub(crate) fn handoff(root: &Path) -> Result<LedgerHandoff> {
    let task = read_active_task(root)?;
    let summary = summarize_task(&task);
    let last_checkpoint = task.checkpoints.last().cloned();
    let recent_notes = tail_items(&task.notes, 5);
    let recent_evidence = tail_items(&task.evidence, 5);
    Ok(LedgerHandoff {
        task,
        summary,
        workspace: workspace_context(root),
        last_checkpoint,
        recent_notes,
        recent_evidence,
        next_actions: handoff_next_actions(),
    })
}

fn read_active_task(root: &Path) -> Result<LedgerTask> {
    let active: ActiveLedgerTask = read_json(&active_task_path(root)).with_context(|| {
        format!(
            "no active Keel task found at {}; run `keel task start <title>` first",
            active_task_path(root).display()
        )
    })?;
    read_task(root, &active.task_id)
}

fn read_task(root: &Path, task_id: &str) -> Result<LedgerTask> {
    read_json(&task_path(root, task_id)).with_context(|| format!("task `{task_id}` does not exist"))
}

fn write_task(root: &Path, task: &LedgerTask) -> Result<()> {
    write_json_pretty(&task_path(root, &task.task_id), task)
}

fn touch_and_write(root: &Path, mut task: LedgerTask) -> Result<LedgerTask> {
    task.updated_at = now_timestamp();
    write_task(root, &task)?;
    Ok(task)
}

fn run_evidence_command(
    root: &Path,
    command: &str,
    env: Vec<LedgerEvidenceEnv>,
    sequence: usize,
) -> Result<LedgerEvidence> {
    let started_at = now_timestamp();
    let start = Instant::now();
    let mut shell = shell_command(command);
    shell.current_dir(root);
    for variable in &env {
        shell.env(&variable.key, &variable.value);
    }
    let output = shell
        .output()
        .with_context(|| format!("failed to execute evidence command `{command}`"))?;
    let duration_ms = start.elapsed().as_millis();
    let finished_at = now_timestamp();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let (stdout, stdout_truncated) = truncate_output(&stdout);
    let (stderr, stderr_truncated) = truncate_output(&stderr);
    Ok(LedgerEvidence {
        evidence_id: generate_event_id("evidence", sequence),
        command: command.to_string(),
        status: if output.status.success() {
            LedgerEvidenceStatus::Passed
        } else {
            LedgerEvidenceStatus::Failed
        },
        exit_code: output.status.code(),
        started_at,
        finished_at,
        duration_ms,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
        env,
    })
}

fn shell_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut shell = Command::new("cmd");
        shell.args(["/C", command]);
        shell
    }

    #[cfg(not(windows))]
    {
        let mut shell = Command::new("sh");
        shell.args(["-c", command]);
        shell
    }
}

fn summarize_task(task: &LedgerTask) -> LedgerSummary {
    let evidence_passed = task
        .evidence
        .iter()
        .filter(|evidence| evidence.status == LedgerEvidenceStatus::Passed)
        .count();
    let evidence_failed = task.evidence.len().saturating_sub(evidence_passed);
    let current_evidence = current_evidence_window(&task.evidence);
    let current_evidence_passed = current_evidence
        .iter()
        .filter(|evidence| evidence.status == LedgerEvidenceStatus::Passed)
        .count();
    let current_evidence_failed = current_evidence
        .len()
        .saturating_sub(current_evidence_passed);
    LedgerSummary {
        checkpoints: task.checkpoints.len(),
        notes: task.notes.len(),
        evidence: task.evidence.len(),
        evidence_passed,
        evidence_failed,
        current_evidence: current_evidence.len(),
        current_evidence_passed,
        current_evidence_failed,
    }
}

fn workspace_context(root: &Path) -> WorkspaceContext {
    let status = capture_git(root, ["status", "--short"]);
    let diff_stat = capture_git(root, ["diff", "--stat"]);
    let git_status_short = status.output.unwrap_or_default();
    let changed_files = parse_changed_files(&git_status_short);
    WorkspaceContext {
        dirty: !git_status_short.trim().is_empty(),
        changed_files,
        git_status_short,
        git_diff_stat: diff_stat.output.unwrap_or_default(),
        git_status_error: status.error,
        git_diff_stat_error: diff_stat.error,
    }
}

struct GitCapture {
    output: Option<String>,
    error: Option<String>,
}

fn capture_git<const N: usize>(root: &Path, args: [&str; N]) -> GitCapture {
    match Command::new("git").args(args).current_dir(root).output() {
        Ok(output) if output.status.success() => GitCapture {
            output: Some(String::from_utf8_lossy(&output.stdout).to_string()),
            error: None,
        },
        Ok(output) => GitCapture {
            output: None,
            error: Some(String::from_utf8_lossy(&output.stderr).trim().to_string()),
        },
        Err(error) => GitCapture {
            output: None,
            error: Some(error.to_string()),
        },
    }
}

fn parse_changed_files(status_short: &str) -> Vec<String> {
    status_short.lines().filter_map(parse_status_path).collect()
}

fn parse_status_path(line: &str) -> Option<String> {
    let path = line.get(3..)?.trim();
    if path.is_empty() {
        return None;
    }
    path.rsplit(" -> ")
        .next()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_string)
}

fn decision_for_summary(summary: &LedgerSummary) -> LedgerDecision {
    if summary.evidence == 0 {
        return LedgerDecision {
            ready: false,
            reason: "no evidence has been recorded".to_string(),
        };
    }
    if summary.current_evidence == 0 || summary.current_evidence_failed > 0 {
        return LedgerDecision {
            ready: false,
            reason: "latest evidence command failed".to_string(),
        };
    }
    if summary.evidence_failed > 0 {
        return LedgerDecision {
            ready: true,
            reason: "all evidence since the most recent failure passed".to_string(),
        };
    }
    LedgerDecision {
        ready: true,
        reason: "all recorded evidence passed".to_string(),
    }
}

fn current_evidence_window(evidence: &[LedgerEvidence]) -> &[LedgerEvidence] {
    let start = evidence
        .iter()
        .rposition(|evidence| evidence.status == LedgerEvidenceStatus::Failed)
        .map_or(0, |index| index + 1);
    &evidence[start..]
}

fn review_next_actions() -> Vec<String> {
    vec![
        "keel checkpoint \"...\"".to_string(),
        "keel evidence add --cmd \"cargo test --workspace\"".to_string(),
        "keel handoff".to_string(),
    ]
}

fn handoff_next_actions() -> Vec<String> {
    vec![
        "continue from the last checkpoint".to_string(),
        "rerun or add evidence for any changed behavior".to_string(),
        "finish with `keel review` before committing".to_string(),
    ]
}

fn tail_items<T: Clone>(items: &[T], limit: usize) -> Vec<T> {
    items
        .iter()
        .skip(items.len().saturating_sub(limit))
        .cloned()
        .collect()
}

fn truncate_output(output: &str) -> (String, bool) {
    if output.len() <= REPORT_OUTPUT_LIMIT {
        return (output.to_string(), false);
    }

    let mut start = output.len().saturating_sub(REPORT_OUTPUT_LIMIT);
    while !output.is_char_boundary(start) {
        start += 1;
    }
    (output[start..].to_string(), true)
}

fn normalized_message(message: &str, label: &str) -> Result<String> {
    let message = message.trim();
    if message.is_empty() {
        bail!("{label} cannot be empty");
    }
    Ok(message.to_string())
}

fn generate_task_id() -> String {
    format!("task-{}-{}", unix_millis(), std::process::id())
}

fn generate_event_id(prefix: &str, sequence: usize) -> String {
    format!("{prefix}-{}-{sequence}", unix_millis())
}

fn active_task_path(root: &Path) -> PathBuf {
    ledger_dir(root).join(ACTIVE_TASK_FILE)
}

fn task_path(root: &Path, task_id: &str) -> PathBuf {
    tasks_dir(root).join(task_id).join("task.json")
}

fn tasks_dir(root: &Path) -> PathBuf {
    ledger_dir(root).join(LEDGER_TASKS_DIR)
}

fn ledger_dir(root: &Path) -> PathBuf {
    root.join(KEEL_DIR).join(LEDGER_DIR)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn review_requires_evidence_before_ready() {
        let temp = TempDir::new().unwrap();

        start_task(temp.path(), "self dogfood").unwrap();
        let review = review(temp.path()).unwrap();

        assert!(!review.decision.ready);
        assert_eq!(review.decision.reason, "no evidence has been recorded");
    }

    #[test]
    fn evidence_failure_blocks_review_ready() {
        let temp = TempDir::new().unwrap();

        start_task(temp.path(), "self dogfood").unwrap();
        add_evidence(
            temp.path(),
            "definitely-not-a-keel-test-command",
            Vec::new(),
        )
        .unwrap();
        let review = review(temp.path()).unwrap();

        assert!(!review.decision.ready);
        assert_eq!(review.summary.evidence_failed, 1);
    }

    #[test]
    fn passing_evidence_after_failure_restores_ready_decision() {
        let temp = TempDir::new().unwrap();

        start_task(temp.path(), "self dogfood").unwrap();
        add_evidence(
            temp.path(),
            "definitely-not-a-keel-test-command",
            Vec::new(),
        )
        .unwrap();
        add_evidence(temp.path(), "git --version", Vec::new()).unwrap();
        let review = review(temp.path()).unwrap();

        assert!(review.decision.ready);
        assert_eq!(review.summary.evidence_failed, 1);
        assert_eq!(review.summary.current_evidence, 1);
        assert_eq!(
            review.decision.reason,
            "all evidence since the most recent failure passed"
        );
    }

    #[test]
    fn workspace_context_extracts_changed_files_from_git_status() {
        let changed = parse_changed_files(
            " M README.md\nA  crates/keel-core/src/ledger.rs\nR  old.rs -> new.rs\n",
        );

        assert_eq!(
            changed,
            vec![
                "README.md".to_string(),
                "crates/keel-core/src/ledger.rs".to_string(),
                "new.rs".to_string()
            ]
        );
    }
}
