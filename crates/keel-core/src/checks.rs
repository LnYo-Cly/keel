use crate::command::{format_command, run_command, CommandCapture};
use crate::config::ConfiguredCheck;
use crate::model::{CheckResult, CheckStatus, FailureReason, RunStatus};
use crate::run::RunLog;
use anyhow::{bail, Result};
use std::path::Path;

#[derive(Debug)]
pub(crate) struct RunClassification {
    pub(crate) status: RunStatus,
    pub(crate) failure_reason: Option<FailureReason>,
    pub(crate) readiness_reason: String,
}

pub(crate) fn run_checks(
    worktree: &Path,
    configured_checks: &[ConfiguredCheck],
    log: &mut RunLog,
) -> Result<Vec<CheckResult>> {
    let mut checks = Vec::new();

    for check in configured_checks {
        let Some((program, args)) = check.command.split_first() else {
            bail!("configured check `{}` has an empty command", check.name);
        };
        let command = format_command(program, args);

        if let Some(required_path) = &check.run_if_path_exists {
            if !worktree.join(required_path).exists() {
                let message = format!("skipped: {required_path} not found in candidate worktree");
                checks.push(CheckResult {
                    name: check.name.clone(),
                    command,
                    status: CheckStatus::Skipped,
                    exit_code: None,
                    stdout: message.clone(),
                    stderr: String::new(),
                });
                log.push(format!("skipped {}: {message}", check.name));
                continue;
            }
        }

        let capture = run_command(worktree, program, args)?;
        log.push_command(worktree, &command, &capture);
        checks.push(check_from_capture(&check.name, &command, capture));
    }

    Ok(checks)
}

pub(crate) fn classify_run(
    exit_code: Option<i32>,
    timed_out: bool,
    checks: &[CheckResult],
) -> RunClassification {
    if timed_out {
        return RunClassification {
            status: RunStatus::NotReady,
            failure_reason: Some(FailureReason::Timeout),
            readiness_reason: "agent command timed out".to_string(),
        };
    }

    if let Some(code) = exit_code {
        if code != 0 {
            return RunClassification {
                status: RunStatus::NotReady,
                failure_reason: Some(FailureReason::NonzeroExit),
                readiness_reason: format!("agent exited with nonzero status {code}"),
            };
        }
    } else {
        return RunClassification {
            status: RunStatus::NotReady,
            failure_reason: Some(FailureReason::AdapterError),
            readiness_reason: "agent did not report an exit code".to_string(),
        };
    }

    let failed_checks = checks
        .iter()
        .filter(|check| matches!(check.status, CheckStatus::Failed))
        .map(|check| check.name.as_str())
        .collect::<Vec<_>>();
    if !failed_checks.is_empty() {
        return RunClassification {
            status: RunStatus::NotReady,
            failure_reason: Some(FailureReason::CheckFailed),
            readiness_reason: format!("failed checks: {}", failed_checks.join(", ")),
        };
    }

    RunClassification {
        status: RunStatus::Ready,
        failure_reason: None,
        readiness_reason: "agent exited successfully and required checks did not fail".to_string(),
    }
}

fn check_from_capture(name: &str, command: &str, capture: CommandCapture) -> CheckResult {
    CheckResult {
        name: name.to_string(),
        command: command.to_string(),
        status: if capture.status.success() {
            CheckStatus::Passed
        } else {
            CheckStatus::Failed
        },
        exit_code: capture.status.code(),
        stdout: capture.stdout,
        stderr: capture.stderr,
    }
}
