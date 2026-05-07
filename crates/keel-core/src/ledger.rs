use crate::constants::{KEEL_DIR, REPORT_OUTPUT_LIMIT};
use crate::json::{read_json, write_json_pretty};
use crate::time::{now_timestamp, unix_millis};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

const LEDGER_DIR: &str = "ledger";
const LEDGER_TASKS_DIR: &str = "tasks";
const ACTIVE_TASK_FILE: &str = "active_task.json";
const LEDGER_LOCK_FILE: &str = "ledger.lock";
const LEDGER_LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const LEDGER_LOCK_RETRY: Duration = Duration::from_millis(25);

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
    Superseded,
    Finished,
}

impl std::fmt::Display for LedgerTaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => f.write_str("active"),
            Self::Superseded => f.write_str("superseded"),
            Self::Finished => f.write_str("finished"),
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
pub struct LedgerStatus {
    pub active_task: Option<LedgerTaskSummary>,
    pub recent_tasks: Vec<LedgerTaskSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LedgerTaskReport {
    pub task: LedgerTask,
    pub summary: LedgerSummary,
    pub decision: LedgerDecision,
}

#[derive(Debug, Clone, Serialize)]
pub struct LedgerTaskSummary {
    pub task_id: String,
    pub title: String,
    pub status: LedgerTaskStatus,
    pub created_at: String,
    pub updated_at: String,
    pub checkpoints: usize,
    pub notes: usize,
    pub evidence: usize,
    pub evidence_passed: usize,
    pub evidence_failed: usize,
    pub current_evidence_failed: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct LedgerReview {
    pub task: LedgerTask,
    pub summary: LedgerSummary,
    pub decision: LedgerDecision,
    pub workspace: Option<WorkspaceContext>,
    pub packet: LedgerReviewPacket,
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
    pub workspace: Option<WorkspaceContext>,
    pub packet: LedgerReviewPacket,
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

#[derive(Debug, Clone, Serialize)]
pub struct LedgerReviewPacket {
    pub headline: String,
    pub changed_file_groups: Vec<ChangedFileGroup>,
    pub evidence: LedgerEvidencePacket,
    pub suggested_commands: Vec<String>,
    pub workspace_context: LedgerWorkspaceContextKind,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LedgerWorkspaceContextKind {
    Current,
    ArchivedTask,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChangedFileGroup {
    pub name: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LedgerEvidencePacket {
    pub latest: Option<LedgerEvidenceBrief>,
    pub current_window: Vec<LedgerEvidenceBrief>,
    pub failed: Vec<LedgerEvidenceBrief>,
    pub recovered_after_failure: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LedgerEvidenceBrief {
    pub command: String,
    pub status: LedgerEvidenceStatus,
    pub exit_code: Option<i32>,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_keys: Vec<String>,
}

pub(crate) fn start_task(root: &Path, title: &str) -> Result<LedgerTask> {
    let title = title.trim();
    if title.is_empty() {
        bail!("task title cannot be empty");
    }

    let _lock = acquire_ledger_lock(root)?;
    fs::create_dir_all(tasks_dir(root))
        .with_context(|| format!("failed to create {}", tasks_dir(root).display()))?;

    let timestamp = now_timestamp();
    supersede_active_task(root, &timestamp, None)?;

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
    write_active_task(root, &task.task_id)?;
    Ok(task)
}

pub(crate) fn status(root: &Path) -> Result<LedgerStatus> {
    let active_task = read_active_task(root).ok();
    let active_task_id = active_task.as_ref().map(|task| task.task_id.as_str());
    let active_task_summary = active_task
        .as_ref()
        .map(|task| LedgerTaskSummary::from_task(task, active_task_id));
    let mut recent_tasks = list_tasks(root)?
        .into_iter()
        .map(|task| LedgerTaskSummary::from_task(&task, active_task_id))
        .collect::<Vec<_>>();
    recent_tasks.sort_by(|left, right| {
        right
            .updated_at
            .cmp(&left.updated_at)
            .then_with(|| right.created_at.cmp(&left.created_at))
            .then_with(|| left.task_id.cmp(&right.task_id))
    });
    Ok(LedgerStatus {
        active_task: active_task_summary,
        recent_tasks,
    })
}

pub(crate) fn task_report(root: &Path, task_id: &str) -> Result<LedgerTaskReport> {
    let task = read_task(root, task_id)?;
    Ok(report_for_task(task))
}

pub(crate) fn reopen_task(root: &Path, task_id: &str) -> Result<LedgerTask> {
    let _lock = acquire_ledger_lock(root)?;
    let mut task = read_task(root, task_id)?;
    let timestamp = now_timestamp();
    supersede_active_task(root, &timestamp, Some(&task.task_id))?;
    task.status = LedgerTaskStatus::Active;
    task.updated_at = timestamp;
    write_task(root, &task)?;
    write_active_task(root, &task.task_id)?;
    Ok(task)
}

pub(crate) fn finish_task(root: &Path) -> Result<LedgerTask> {
    let _lock = acquire_ledger_lock(root)?;
    let mut task = read_active_task(root)?;
    task.status = LedgerTaskStatus::Finished;
    task.updated_at = now_timestamp();
    write_task(root, &task)?;
    let active_path = active_task_path(root);
    if active_path.exists() {
        fs::remove_file(&active_path)
            .with_context(|| format!("failed to remove {}", active_path.display()))?;
    }
    Ok(task)
}

pub(crate) fn add_checkpoint(root: &Path, message: &str) -> Result<LedgerTask> {
    let message = normalized_message(message, "checkpoint message")?;
    let _lock = acquire_ledger_lock(root)?;
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
    let _lock = acquire_ledger_lock(root)?;
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
    let active_task_id = {
        let _lock = acquire_ledger_lock(root)?;
        read_active_task(root)?.task_id
    };

    let evidence = run_evidence_command(root, &command, env)?;

    let _lock = acquire_ledger_lock(root)?;
    let mut task = read_active_task(root)?;
    if task.task_id != active_task_id {
        bail!(
            "active Keel task changed while evidence command was running; evidence was not recorded"
        );
    }
    let evidence = evidence.into_evidence(task.evidence.len() + 1);
    task.evidence.push(evidence);
    touch_and_write(root, task)
}

pub(crate) fn review(root: &Path) -> Result<LedgerReview> {
    let task = read_active_task(root)?;
    review_for_task(root, task, None)
}

pub(crate) fn review_task(root: &Path, task_id: &str) -> Result<LedgerReview> {
    let task = read_task(root, task_id)?;
    review_for_task(root, task, Some(task_id))
}

pub(crate) fn handoff(root: &Path) -> Result<LedgerHandoff> {
    let task = read_active_task(root)?;
    handoff_for_task(root, task, None)
}

pub(crate) fn handoff_task(root: &Path, task_id: &str) -> Result<LedgerHandoff> {
    let task = read_task(root, task_id)?;
    handoff_for_task(root, task, Some(task_id))
}

fn review_for_task(root: &Path, task: LedgerTask, task_id: Option<&str>) -> Result<LedgerReview> {
    let summary = summarize_task(&task);
    let decision = decision_for_summary(&summary);
    let workspace = workspace_context_for_review(root, task_id.is_some());
    let packet = review_packet(&task, &summary, &decision, &workspace);
    Ok(LedgerReview {
        task,
        summary,
        decision,
        workspace,
        packet,
        next_actions: review_next_actions(task_id),
    })
}

fn handoff_for_task(root: &Path, task: LedgerTask, task_id: Option<&str>) -> Result<LedgerHandoff> {
    let summary = summarize_task(&task);
    let decision = decision_for_summary(&summary);
    let workspace = workspace_context_for_review(root, task_id.is_some());
    let packet = review_packet(&task, &summary, &decision, &workspace);
    let last_checkpoint = task.checkpoints.last().cloned();
    let recent_notes = tail_items(&task.notes, 5);
    let recent_evidence = tail_items(&task.evidence, 5);
    Ok(LedgerHandoff {
        task,
        summary,
        workspace,
        packet,
        last_checkpoint,
        recent_notes,
        recent_evidence,
        next_actions: handoff_next_actions(task_id),
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
    ensure_safe_task_id(task_id)?;
    read_json(&task_path(root, task_id)).with_context(|| format!("task `{task_id}` does not exist"))
}

fn list_tasks(root: &Path) -> Result<Vec<LedgerTask>> {
    let tasks_dir = tasks_dir(root);
    if !tasks_dir.exists() {
        return Ok(Vec::new());
    }

    let mut tasks = Vec::new();
    for entry in fs::read_dir(&tasks_dir)
        .with_context(|| format!("failed to read {}", tasks_dir.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let task_path = entry.path().join("task.json");
        if task_path.exists() {
            tasks.push(read_json(&task_path)?);
        }
    }
    Ok(tasks)
}

fn write_task(root: &Path, task: &LedgerTask) -> Result<()> {
    write_json_pretty(&task_path(root, &task.task_id), task)
}

fn write_active_task(root: &Path, task_id: &str) -> Result<()> {
    write_json_pretty(
        &active_task_path(root),
        &ActiveLedgerTask {
            task_id: task_id.to_string(),
        },
    )
}

struct LedgerLock {
    file: File,
}

impl Drop for LedgerLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

fn acquire_ledger_lock(root: &Path) -> Result<LedgerLock> {
    let lock_path = ledger_lock_path(root);
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let start = Instant::now();
    loop {
        match open_ledger_lock_file(&lock_path).and_then(|file| {
            file.try_lock().map(|()| file).map_err(|error| match error {
                TryLockError::WouldBlock => std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "ledger lock is already held",
                ),
                TryLockError::Error(error) => error,
            })
        }) {
            Ok(mut file) => {
                let path_for_error = lock_path.display().to_string();
                file.set_len(0)
                    .with_context(|| format!("failed to clear {path_for_error}"))?;
                writeln!(
                    file,
                    "pid={}\ncreated_at={}",
                    std::process::id(),
                    now_timestamp()
                )
                .with_context(|| format!("failed to write {path_for_error}"))?;
                return Ok(LedgerLock { file });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if start.elapsed() >= LEDGER_LOCK_TIMEOUT {
                    bail!(
                        "timed out waiting for Keel ledger lock at {}; another ledger command may still be running",
                        lock_path.display()
                    );
                }
                thread::sleep(LEDGER_LOCK_RETRY);
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to create {}", lock_path.display()));
            }
        }
    }
}

fn open_ledger_lock_file(lock_path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
}

fn supersede_active_task(root: &Path, timestamp: &str, except_task_id: Option<&str>) -> Result<()> {
    let Ok(mut previous) = read_active_task(root) else {
        return Ok(());
    };
    if Some(previous.task_id.as_str()) == except_task_id {
        return Ok(());
    }

    previous.status = LedgerTaskStatus::Superseded;
    previous.updated_at = timestamp.to_string();
    write_task(root, &previous)
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
) -> Result<LedgerEvidenceCapture> {
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
    Ok(LedgerEvidenceCapture {
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

struct LedgerEvidenceCapture {
    command: String,
    status: LedgerEvidenceStatus,
    exit_code: Option<i32>,
    started_at: String,
    finished_at: String,
    duration_ms: u128,
    stdout: String,
    stderr: String,
    stdout_truncated: bool,
    stderr_truncated: bool,
    env: Vec<LedgerEvidenceEnv>,
}

impl LedgerEvidenceCapture {
    fn into_evidence(self, sequence: usize) -> LedgerEvidence {
        LedgerEvidence {
            evidence_id: generate_event_id("evidence", sequence),
            command: self.command,
            status: self.status,
            exit_code: self.exit_code,
            started_at: self.started_at,
            finished_at: self.finished_at,
            duration_ms: self.duration_ms,
            stdout: self.stdout,
            stderr: self.stderr,
            stdout_truncated: self.stdout_truncated,
            stderr_truncated: self.stderr_truncated,
            env: self.env,
        }
    }
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

fn report_for_task(task: LedgerTask) -> LedgerTaskReport {
    let summary = summarize_task(&task);
    let decision = decision_for_summary(&summary);
    LedgerTaskReport {
        task,
        summary,
        decision,
    }
}

impl LedgerTaskSummary {
    pub(crate) fn from_task(task: &LedgerTask, active_task_id: Option<&str>) -> Self {
        let summary = summarize_task(task);
        Self {
            task_id: task.task_id.clone(),
            title: task.title.clone(),
            status: task_summary_status(task, active_task_id),
            created_at: task.created_at.clone(),
            updated_at: task.updated_at.clone(),
            checkpoints: summary.checkpoints,
            notes: summary.notes,
            evidence: summary.evidence,
            evidence_passed: summary.evidence_passed,
            evidence_failed: summary.evidence_failed,
            current_evidence_failed: summary.current_evidence_failed,
        }
    }
}

fn task_summary_status(task: &LedgerTask, active_task_id: Option<&str>) -> LedgerTaskStatus {
    if task.status == LedgerTaskStatus::Active && Some(task.task_id.as_str()) != active_task_id {
        return LedgerTaskStatus::Superseded;
    }
    task.status
}

fn workspace_context_for_review(root: &Path, archived_task: bool) -> Option<WorkspaceContext> {
    if archived_task {
        return None;
    }
    Some(workspace_context(root))
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

fn review_packet(
    task: &LedgerTask,
    summary: &LedgerSummary,
    decision: &LedgerDecision,
    workspace: &Option<WorkspaceContext>,
) -> LedgerReviewPacket {
    LedgerReviewPacket {
        headline: review_headline(decision, workspace),
        changed_file_groups: workspace
            .as_ref()
            .map(|workspace| group_changed_files(&workspace.changed_files))
            .unwrap_or_default(),
        evidence: evidence_packet(task, summary),
        suggested_commands: suggested_packet_commands(decision, workspace),
        workspace_context: workspace
            .as_ref()
            .map_or(LedgerWorkspaceContextKind::ArchivedTask, |_| {
                LedgerWorkspaceContextKind::Current
            }),
    }
}

fn review_headline(decision: &LedgerDecision, workspace: &Option<WorkspaceContext>) -> String {
    let verdict = if decision.ready { "ready" } else { "not ready" };
    let workspace = match workspace {
        Some(workspace) if workspace.dirty => "workspace has changes",
        Some(_) => "workspace is clean",
        None => "archived task; current workspace context not included",
    };
    format!("{verdict}: {workspace}; {}", decision.reason)
}

fn group_changed_files(files: &[String]) -> Vec<ChangedFileGroup> {
    let mut groups = [
        ChangedFileGroup {
            name: "source".to_string(),
            files: Vec::new(),
        },
        ChangedFileGroup {
            name: "tests".to_string(),
            files: Vec::new(),
        },
        ChangedFileGroup {
            name: "docs".to_string(),
            files: Vec::new(),
        },
        ChangedFileGroup {
            name: "config".to_string(),
            files: Vec::new(),
        },
        ChangedFileGroup {
            name: "other".to_string(),
            files: Vec::new(),
        },
    ];

    for file in files {
        let index = changed_file_group_index(file);
        groups[index].files.push(file.clone());
    }

    groups
        .into_iter()
        .filter(|group| !group.files.is_empty())
        .collect()
}

fn changed_file_group_index(path: &str) -> usize {
    let normalized = path.replace('\\', "/");
    let lower = normalized.to_ascii_lowercase();
    if lower.contains("/tests/")
        || lower.ends_with("_test.rs")
        || lower.ends_with("_tests.rs")
        || lower.contains("/snapshots/")
        || lower.starts_with("tests/")
    {
        return 1;
    }
    if lower == "readme.md" || lower.starts_with("docs/") || lower.ends_with(".md") {
        return 2;
    }
    if lower == "cargo.toml"
        || lower == "cargo.lock"
        || lower.ends_with(".toml")
        || lower.starts_with(".github/")
        || lower.starts_with("scripts/")
    {
        return 3;
    }
    if lower.contains("/src/") || lower.starts_with("src/") || lower.ends_with(".rs") {
        return 0;
    }
    4
}

fn evidence_packet(task: &LedgerTask, summary: &LedgerSummary) -> LedgerEvidencePacket {
    let current_start = task.evidence.len().saturating_sub(summary.current_evidence);
    LedgerEvidencePacket {
        latest: task.evidence.last().map(evidence_brief),
        current_window: task.evidence[current_start..]
            .iter()
            .map(evidence_brief)
            .collect(),
        failed: task
            .evidence
            .iter()
            .filter(|evidence| evidence.status == LedgerEvidenceStatus::Failed)
            .map(evidence_brief)
            .collect(),
        recovered_after_failure: summary.evidence_failed > 0
            && summary.current_evidence_failed == 0,
    }
}

fn evidence_brief(evidence: &LedgerEvidence) -> LedgerEvidenceBrief {
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

fn suggested_packet_commands(
    decision: &LedgerDecision,
    workspace: &Option<WorkspaceContext>,
) -> Vec<String> {
    let mut commands = Vec::new();
    if !decision.ready {
        commands.push("fix the failed or missing evidence".to_string());
        commands.push("keel check".to_string());
        commands.push("keel verify".to_string());
        return commands;
    }
    let Some(workspace) = workspace else {
        commands.push("keel task reopen <task-id>".to_string());
        commands.push("keel task show <task-id>".to_string());
        return commands;
    };
    if workspace.dirty {
        commands.push("git diff --stat".to_string());
        commands.push("git diff --check".to_string());
        commands.push("keel check".to_string());
    } else {
        commands.push("keel handoff".to_string());
    }
    commands.push("keel review".to_string());
    commands
}

fn current_evidence_window(evidence: &[LedgerEvidence]) -> &[LedgerEvidence] {
    let start = evidence
        .iter()
        .rposition(|evidence| evidence.status == LedgerEvidenceStatus::Failed)
        .map_or(0, |index| index + 1);
    &evidence[start..]
}

fn review_next_actions(task_id: Option<&str>) -> Vec<String> {
    match task_id {
        Some(task_id) => vec![
            format!("keel task reopen {task_id}"),
            format!("keel task show {task_id}"),
            "keel task status".to_string(),
        ],
        None => vec![
            "keel checkpoint \"...\"".to_string(),
            "keel check".to_string(),
            "keel handoff".to_string(),
        ],
    }
}

fn handoff_next_actions(task_id: Option<&str>) -> Vec<String> {
    match task_id {
        Some(task_id) => vec![
            format!("keel task reopen {task_id}"),
            format!("keel task show {task_id}"),
            "keel task status".to_string(),
        ],
        None => vec![
            "continue from the last checkpoint".to_string(),
            "rerun or add evidence for any changed behavior".to_string(),
            "finish with `keel review` before committing".to_string(),
        ],
    }
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

fn ensure_safe_task_id(task_id: &str) -> Result<()> {
    if task_id.is_empty()
        || task_id == "."
        || task_id == ".."
        || !task_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        bail!("invalid task id `{task_id}`");
    }
    Ok(())
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

fn ledger_lock_path(root: &Path) -> PathBuf {
    ledger_dir(root).join(LEDGER_LOCK_FILE)
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
    fn concurrent_task_mutations_preserve_all_events() {
        let temp = TempDir::new().unwrap();
        start_task(temp.path(), "concurrent ledger").unwrap();

        thread::scope(|scope| {
            for index in 0..8 {
                let root = temp.path().to_path_buf();
                scope.spawn(move || {
                    add_note(&root, &format!("note {index}")).unwrap();
                });
            }
            for index in 0..8 {
                let root = temp.path().to_path_buf();
                scope.spawn(move || {
                    add_checkpoint(&root, &format!("checkpoint {index}")).unwrap();
                });
            }
        });

        let task = read_active_task(temp.path()).unwrap();
        assert_eq!(task.notes.len(), 8);
        assert_eq!(task.checkpoints.len(), 8);
        for index in 0..8 {
            assert!(task
                .notes
                .iter()
                .any(|note| note.message == format!("note {index}")));
            assert!(task
                .checkpoints
                .iter()
                .any(|checkpoint| checkpoint.message == format!("checkpoint {index}")));
        }
    }

    #[test]
    fn concurrent_evidence_commands_preserve_all_results() {
        let temp = TempDir::new().unwrap();
        start_task(temp.path(), "concurrent evidence").unwrap();

        thread::scope(|scope| {
            for _ in 0..4 {
                let root = temp.path().to_path_buf();
                scope.spawn(move || {
                    add_evidence(&root, "git --version", Vec::new()).unwrap();
                });
            }
        });

        let task = read_active_task(temp.path()).unwrap();
        assert_eq!(task.evidence.len(), 4);
        assert!(task
            .evidence
            .iter()
            .all(|evidence| evidence.status == LedgerEvidenceStatus::Passed));

        let mut evidence_ids = task
            .evidence
            .iter()
            .map(|evidence| evidence.evidence_id.clone())
            .collect::<Vec<_>>();
        evidence_ids.sort();
        evidence_ids.dedup();
        assert_eq!(evidence_ids.len(), task.evidence.len());
    }

    #[test]
    fn evidence_is_not_recorded_if_active_task_changes_while_command_runs() {
        let temp = TempDir::new().unwrap();
        let first = start_task(temp.path(), "first evidence target").unwrap();
        let root = temp.path().to_path_buf();

        let evidence = thread::spawn(move || {
            add_evidence(&root, slow_success_command(), Vec::new())
                .unwrap_err()
                .to_string()
        });
        thread::sleep(Duration::from_millis(100));
        let second = start_task(temp.path(), "second evidence target").unwrap();

        let error = evidence.join().unwrap();
        assert!(error.contains("active Keel task changed while evidence command was running"));
        assert!(read_task(temp.path(), &first.task_id)
            .unwrap()
            .evidence
            .is_empty());
        assert!(read_task(temp.path(), &second.task_id)
            .unwrap()
            .evidence
            .is_empty());
    }

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
    fn starting_new_task_marks_previous_active_task_superseded() {
        let temp = TempDir::new().unwrap();

        let first = start_task(temp.path(), "first task").unwrap();
        let second = start_task(temp.path(), "second task").unwrap();
        let status = status(temp.path()).unwrap();

        assert_eq!(status.active_task.unwrap().task_id, second.task_id);
        assert!(status.recent_tasks.iter().any(
            |task| task.task_id == first.task_id && task.status == LedgerTaskStatus::Superseded
        ));
    }

    #[test]
    fn finishing_task_clears_active_task_and_preserves_history() {
        let temp = TempDir::new().unwrap();

        let task = start_task(temp.path(), "finish me").unwrap();
        let finished = finish_task(temp.path()).unwrap();
        let status = status(temp.path()).unwrap();

        assert_eq!(finished.task_id, task.task_id);
        assert_eq!(finished.status, LedgerTaskStatus::Finished);
        assert!(status.active_task.is_none());
        assert!(status
            .recent_tasks
            .iter()
            .any(|task| task.task_id == finished.task_id
                && task.status == LedgerTaskStatus::Finished));
    }

    #[test]
    fn task_report_reads_preserved_finished_task() {
        let temp = TempDir::new().unwrap();

        let task = start_task(temp.path(), "show me").unwrap();
        add_evidence(temp.path(), "git --version", Vec::new()).unwrap();
        finish_task(temp.path()).unwrap();
        let report = task_report(temp.path(), &task.task_id).unwrap();

        assert_eq!(report.task.task_id, task.task_id);
        assert_eq!(report.task.status, LedgerTaskStatus::Finished);
        assert!(report.decision.ready);
    }

    #[test]
    fn reopen_task_restores_preserved_task_as_active() {
        let temp = TempDir::new().unwrap();

        let first = start_task(temp.path(), "first").unwrap();
        finish_task(temp.path()).unwrap();
        let second = start_task(temp.path(), "second").unwrap();
        let reopened = reopen_task(temp.path(), &first.task_id).unwrap();
        let status = status(temp.path()).unwrap();

        assert_eq!(reopened.task_id, first.task_id);
        assert_eq!(status.active_task.unwrap().task_id, first.task_id);
        assert!(status
            .recent_tasks
            .iter()
            .any(|task| task.task_id == second.task_id
                && task.status == LedgerTaskStatus::Superseded));
    }

    #[test]
    fn task_ids_reject_path_traversal() {
        let temp = TempDir::new().unwrap();

        let error = task_report(temp.path(), "../bad").unwrap_err().to_string();

        assert!(error.contains("invalid task id"));
    }

    #[test]
    fn preserved_task_review_omits_current_workspace_context() {
        let temp = TempDir::new().unwrap();
        let task = start_task(temp.path(), "archived review").unwrap();
        add_evidence(temp.path(), "git --version", Vec::new()).unwrap();
        finish_task(temp.path()).unwrap();
        fs::write(temp.path().join("README.md"), "changed\n").unwrap();

        let active_error = review(temp.path()).unwrap_err().to_string();
        assert!(active_error.contains("no active Keel task found"));
        let archived = review_task(temp.path(), &task.task_id).unwrap();

        assert!(archived.workspace.is_none());
        assert_eq!(
            archived.packet.workspace_context,
            LedgerWorkspaceContextKind::ArchivedTask
        );
        assert!(archived.packet.changed_file_groups.is_empty());
        assert!(archived
            .packet
            .headline
            .contains("archived task; current workspace context not included"));
        assert!(archived
            .packet
            .suggested_commands
            .contains(&"keel task reopen <task-id>".to_string()));
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

    fn slow_success_command() -> &'static str {
        if cfg!(windows) {
            "ping -n 2 127.0.0.1 >NUL"
        } else {
            "sleep 1"
        }
    }

    #[test]
    fn review_packet_groups_changed_files_for_closeout() {
        let groups = group_changed_files(&[
            "README.md".to_string(),
            "crates/keel-core/src/ledger.rs".to_string(),
            "crates/keel-cli/tests/cli_workflow.rs".to_string(),
            "Cargo.toml".to_string(),
        ]);

        assert!(groups
            .iter()
            .any(|group| group.name == "docs" && group.files == ["README.md"]));
        assert!(groups.iter().any(
            |group| group.name == "source" && group.files == ["crates/keel-core/src/ledger.rs"]
        ));
        assert!(groups.iter().any(|group| group.name == "tests"
            && group.files == ["crates/keel-cli/tests/cli_workflow.rs"]));
        assert!(groups
            .iter()
            .any(|group| group.name == "config" && group.files == ["Cargo.toml"]));
    }
}
