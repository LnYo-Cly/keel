use crate::model::FailureReason;
use anyhow::{Context, Result};
use process_wrap::std::CommandWrap;
#[cfg(windows)]
use process_wrap::std::JobObject;
#[cfg(unix)]
use process_wrap::std::ProcessGroup;
use std::ffi::OsStr;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub(crate) struct CommandCapture {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

#[derive(Debug)]
pub(crate) struct AgentCommandCapture {
    pub(crate) exit_code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) timed_out: bool,
}

pub(crate) fn run_command(dir: &Path, program: &str, args: &[String]) -> Result<CommandCapture> {
    let executable = resolve_program(program);
    let output = Command::new(&executable)
        .args(args.iter().map(OsStr::new))
        .current_dir(dir)
        .output()
        .map_err(|error| command_error(program, args, error))?;

    Ok(CommandCapture {
        status: output.status,
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

pub(crate) fn run_command_with_timeout(
    dir: &Path,
    program: &str,
    args: &[String],
    timeout: Duration,
) -> Result<AgentCommandCapture> {
    let executable = resolve_program(program);
    let mut command = Command::new(&executable);
    command
        .args(args.iter().map(OsStr::new))
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut command = CommandWrap::from(command);
    #[cfg(unix)]
    command.wrap(ProcessGroup::leader());
    #[cfg(windows)]
    command.wrap(JobObject);

    let mut child = command
        .spawn()
        .map_err(|error| command_error(program, args, error))?;

    let stdout_handle = child
        .stdout()
        .take()
        .map(|stdout| thread::spawn(move || read_pipe(stdout)));
    let stderr_handle = child
        .stderr()
        .take()
        .map(|stderr| thread::spawn(move || read_pipe(stderr)));

    let start = Instant::now();
    let (status, timed_out) = loop {
        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("failed to wait for {}", format_command(program, args)))?
        {
            break (status, false);
        }

        if start.elapsed() >= timeout {
            let _ = child.start_kill();
            let status = child
                .wait()
                .with_context(|| format!("failed to stop {}", format_command(program, args)))?;
            break (status, true);
        }

        thread::sleep(Duration::from_millis(50));
    };

    let stdout = join_pipe_reader(stdout_handle)?;
    let mut stderr = join_pipe_reader(stderr_handle)?;
    if timed_out {
        if !stderr.ends_with('\n') && !stderr.is_empty() {
            stderr.push('\n');
        }
        stderr.push_str(&format!(
            "process timed out after {} seconds\n",
            timeout.as_secs()
        ));
    }

    Ok(AgentCommandCapture {
        exit_code: status.code(),
        stdout,
        stderr,
        timed_out,
    })
}

pub(crate) fn failure_reason_from_error(error: &anyhow::Error) -> FailureReason {
    let message = error.to_string();
    if message.contains("CLI not found") {
        FailureReason::MissingCli
    } else {
        FailureReason::AdapterError
    }
}

pub(crate) fn format_command_line(command: &[String]) -> String {
    if command.is_empty() {
        "n/a".to_string()
    } else {
        format_command(&command[0], &command[1..])
    }
}

pub(crate) fn format_command(program: &str, args: &[String]) -> String {
    std::iter::once(program.to_string())
        .chain(args.iter().map(|arg| {
            if arg.contains(' ') {
                format!("{arg:?}")
            } else {
                arg.to_string()
            }
        }))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn exit_code_text(code: Option<i32>) -> String {
    code.map_or_else(|| "n/a".to_string(), |code| code.to_string())
}

fn read_pipe<R: Read>(mut pipe: R) -> Vec<u8> {
    let mut bytes = Vec::new();
    let _ = pipe.read_to_end(&mut bytes);
    bytes
}

fn join_pipe_reader(handle: Option<thread::JoinHandle<Vec<u8>>>) -> Result<String> {
    let bytes = match handle {
        Some(handle) => handle
            .join()
            .map_err(|_| anyhow::anyhow!("failed to join process output reader"))?,
        None => Vec::new(),
    };
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

fn resolve_program(program: &str) -> PathBuf {
    let program_path = Path::new(program);
    if program_path.is_absolute() || program_path.components().count() > 1 {
        return program_path.to_path_buf();
    }

    #[cfg(windows)]
    {
        if let Some(path) = std::env::var_os("PATH") {
            let extensions = windows_path_extensions();
            let path_entries = std::env::split_paths(&path).collect::<Vec<_>>();
            if let Some(resolved) =
                resolve_windows_program_from_path(program, &path_entries, &extensions)
            {
                return resolved;
            }
        }
    }

    program_path.to_path_buf()
}

#[cfg(windows)]
fn windows_path_extensions() -> Vec<String> {
    std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
        .split(';')
        .filter(|extension| !extension.is_empty())
        .map(|extension| {
            if extension.starts_with('.') {
                extension.to_string()
            } else {
                format!(".{extension}")
            }
        })
        .collect()
}

#[cfg(windows)]
pub(crate) fn resolve_windows_program_from_path(
    program: &str,
    path_entries: &[PathBuf],
    extensions: &[String],
) -> Option<PathBuf> {
    let has_extension = Path::new(program).extension().is_some();

    for dir in path_entries {
        if has_extension {
            let candidate = dir.join(program);
            if candidate.is_file() {
                return Some(candidate);
            }
            continue;
        }

        for extension in extensions {
            let candidate = dir.join(format!("{program}{extension}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }

        let candidate = dir.join(program);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

fn command_error(program: &str, args: &[String], error: io::Error) -> anyhow::Error {
    if error.kind() == io::ErrorKind::NotFound {
        if let Some(message) = missing_cli_error(program) {
            return anyhow::anyhow!(message);
        }
    }
    anyhow::anyhow!(
        "failed to execute {}: {}",
        format_command(program, args),
        error
    )
}

fn missing_cli_error(program: &str) -> Option<String> {
    let cli_name = Path::new(program).file_stem().and_then(OsStr::to_str)?;
    let product_name = match cli_name.to_ascii_lowercase().as_str() {
        "codex" => "Codex",
        "claude" => "Claude Code",
        "gh" => "GitHub CLI",
        "glab" => "GitLab CLI",
        "opencode" => "OpenCode",
        _ => return None,
    };

    Some(format!(
        "{cli_name} CLI not found; install {product_name} CLI or ensure `{cli_name}` is on PATH"
    ))
}
