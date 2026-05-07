use crate::agents::default_agent_timeout_secs;
use crate::command::format_command_line;
use crate::constants::{CONFIG_FILE, KEEL_DIR};
use anyhow::{bail, Context, Result};
use globset::Glob;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct KeelConfig {
    #[serde(default = "default_agent_timeout_secs")]
    pub(crate) agent_timeout_secs: u64,
    #[serde(default)]
    pub(crate) checks: Vec<ConfiguredCheck>,
    #[serde(default)]
    pub(crate) risk: RiskConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ConfiguredCheck {
    pub(crate) name: String,
    pub(crate) command: Vec<String>,
    #[serde(default)]
    pub(crate) run_if_path_exists: Option<String>,
    #[serde(default)]
    pub(crate) shell: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceCheckConfig {
    pub(crate) name: String,
    pub(crate) command: String,
    pub(crate) run_if_path_exists: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub checks: ChecksConfig,
    #[serde(default)]
    pub agents: AgentsConfig,
    #[serde(default)]
    pub readiness: ReadinessConfig,
    #[serde(default)]
    pub risk: RiskConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ChecksConfig {
    #[serde(default)]
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentsConfig {
    #[serde(default = "default_agent_config")]
    pub codex: AgentConfig,
    #[serde(default = "default_agent_config")]
    pub claude: AgentConfig,
    #[serde(default = "default_agent_config")]
    pub opencode: AgentConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentConfig {
    #[serde(default = "default_agent_enabled")]
    pub enabled: bool,
    #[serde(default = "default_agent_timeout_secs")]
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReadinessConfig {
    #[serde(default = "default_require_non_empty_diff")]
    pub require_non_empty_diff: bool,
    #[serde(default = "default_require_checks_pass")]
    pub require_checks_pass: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RiskConfig {
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default = "default_large_diff_file_threshold")]
    pub large_diff_file_threshold: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigValidationReport {
    pub ok: bool,
    pub config_path: String,
    pub summary: ConfigValidationSummary,
    pub issues: Vec<ConfigValidationIssue>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigValidationSummary {
    pub ok: usize,
    pub warnings: usize,
    pub errors: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigValidationIssue {
    pub id: String,
    pub severity: ConfigValidationSeverity,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigValidationSeverity {
    Ok,
    Warning,
    Error,
}

impl std::fmt::Display for ConfigValidationSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Ok => "ok",
            Self::Warning => "warning",
            Self::Error => "error",
        };
        f.write_str(value)
    }
}

pub(crate) fn default_config_toml() -> &'static str {
    r#"version = 1
runs_dir = "runs"
worktrees_dir = "worktrees"
agent_timeout_secs = 900

[[checks]]
name = "git status"
command = ["git", "status", "--short"]

[[checks]]
name = "cargo test"
command = ["cargo", "test"]
run_if_path_exists = "Cargo.toml"

[risk]
paths = []
large_diff_file_threshold = 20
"#
}

pub(crate) fn default_checks() -> Vec<ConfiguredCheck> {
    vec![
        ConfiguredCheck {
            name: "git status".to_string(),
            command: vec![
                "git".to_string(),
                "status".to_string(),
                "--short".to_string(),
            ],
            run_if_path_exists: None,
            shell: false,
        },
        ConfiguredCheck {
            name: "cargo test".to_string(),
            command: vec!["cargo".to_string(), "test".to_string()],
            run_if_path_exists: Some("Cargo.toml".to_string()),
            shell: false,
        },
    ]
}

pub(crate) fn load_project_config(project_root: &Path) -> Result<KeelConfig> {
    let config_path = project_root.join(KEEL_DIR).join(CONFIG_FILE);
    let content = fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    let value = content
        .parse::<toml::Value>()
        .with_context(|| format!("failed to parse {}", config_path.display()))?;

    let agent_timeout_secs = value
        .get("agent_timeout_secs")
        .and_then(toml::Value::as_integer)
        .and_then(|value| u64::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or_else(default_agent_timeout_secs);
    let checks = load_configured_checks_from_value(&value)?;
    let risk = value
        .get("risk")
        .cloned()
        .map(toml::Value::try_into)
        .transpose()
        .context("failed to parse risk config")?
        .unwrap_or_default();

    Ok(KeelConfig {
        agent_timeout_secs,
        checks,
        risk,
    })
}

fn load_configured_checks_from_value(value: &toml::Value) -> Result<Vec<ConfiguredCheck>> {
    let Some(checks_value) = value.get("checks") else {
        return Ok(default_checks());
    };

    if let Some(table) = checks_value.as_table() {
        let Some(commands_value) = table.get("commands") else {
            return Ok(default_checks());
        };
        let Some(commands) = commands_value.as_array() else {
            bail!("checks.commands must be an array of strings");
        };
        if commands.is_empty() {
            return Ok(default_checks());
        }
        return commands
            .iter()
            .enumerate()
            .map(|(index, command)| configured_check_from_command_string(index, command))
            .collect();
    }

    if let Some(checks) = checks_value.as_array() {
        if checks.is_empty() {
            return Ok(default_checks());
        }
        return checks
            .iter()
            .enumerate()
            .map(configured_check_from_legacy_check)
            .collect();
    }

    bail!("checks must be either a table with commands or a legacy array of tables")
}

fn configured_check_from_command_string(
    index: usize,
    command: &toml::Value,
) -> Result<ConfiguredCheck> {
    let workspace_check = workspace_check_from_command_string(index, command)?;
    Ok(ConfiguredCheck {
        name: workspace_check.name,
        command: vec![workspace_check.command],
        run_if_path_exists: None,
        shell: true,
    })
}

fn configured_check_from_legacy_check(
    (index, check): (usize, &toml::Value),
) -> Result<ConfiguredCheck> {
    let Some(table) = check.as_table() else {
        bail!("legacy [[checks]] entry at index {index} must be a table");
    };
    let name = legacy_check_name(index, table);
    let command_value = table
        .get("command")
        .ok_or_else(|| anyhow::anyhow!("legacy [[checks]] entry `{name}` is missing command"))?;
    let command = legacy_command_parts(&name, command_value)?;
    let run_if_path_exists = table
        .get("run_if_path_exists")
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(ToString::to_string);
    Ok(ConfiguredCheck {
        name,
        command,
        run_if_path_exists,
        shell: false,
    })
}

pub(crate) fn load_workspace_checks(project_root: &Path) -> Result<Vec<WorkspaceCheckConfig>> {
    let config_path = project_root.join(KEEL_DIR).join(CONFIG_FILE);
    let content = fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    let value = content
        .parse::<toml::Value>()
        .with_context(|| format!("failed to parse {}", config_path.display()))?;

    let Some(checks_value) = value.get("checks") else {
        return Ok(Vec::new());
    };

    if let Some(table) = checks_value.as_table() {
        let Some(commands_value) = table.get("commands") else {
            return Ok(Vec::new());
        };
        let Some(commands) = commands_value.as_array() else {
            bail!("checks.commands must be an array of strings");
        };
        return commands
            .iter()
            .enumerate()
            .map(|(index, command)| workspace_check_from_command_string(index, command))
            .collect();
    }

    if let Some(checks) = checks_value.as_array() {
        return checks
            .iter()
            .enumerate()
            .map(|(index, check)| workspace_check_from_legacy_check(index, check))
            .collect();
    }

    bail!("checks must be either a table with commands or a legacy array of tables")
}

fn workspace_check_from_command_string(
    index: usize,
    command: &toml::Value,
) -> Result<WorkspaceCheckConfig> {
    let Some(command) = command.as_str() else {
        bail!("checks.commands entry at index {index} must be a string");
    };
    let command = command.trim();
    if command.is_empty() {
        bail!("checks.commands entry at index {index} cannot be empty");
    }
    Ok(WorkspaceCheckConfig {
        name: format!("check {}", index + 1),
        command: command.to_string(),
        run_if_path_exists: None,
    })
}

fn workspace_check_from_legacy_check(
    index: usize,
    check: &toml::Value,
) -> Result<WorkspaceCheckConfig> {
    let Some(table) = check.as_table() else {
        bail!("legacy [[checks]] entry at index {index} must be a table");
    };
    let name = legacy_check_name(index, table);
    let command_value = table
        .get("command")
        .ok_or_else(|| anyhow::anyhow!("legacy [[checks]] entry `{name}` is missing command"))?;
    let command_parts = legacy_command_parts(&name, command_value)?;
    let run_if_path_exists = table
        .get("run_if_path_exists")
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(ToString::to_string);

    Ok(WorkspaceCheckConfig {
        name,
        command: format_command_line(&command_parts),
        run_if_path_exists,
    })
}

fn legacy_check_name(index: usize, table: &toml::map::Map<String, toml::Value>) -> String {
    table
        .get("name")
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map_or_else(|| format!("check {}", index + 1), ToString::to_string)
}

fn legacy_command_parts(name: &str, command_value: &toml::Value) -> Result<Vec<String>> {
    let Some(command) = command_value.as_array() else {
        bail!("legacy [[checks]] entry `{name}` command must be an array of strings");
    };
    if command.is_empty() {
        bail!("legacy [[checks]] entry `{name}` command cannot be empty");
    }
    command
        .iter()
        .map(toml::Value::as_str)
        .map(|part| {
            part.map(str::trim)
                .filter(|part| !part.is_empty())
                .map(ToString::to_string)
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| {
            anyhow::anyhow!("legacy [[checks]] entry `{name}` command entries must be strings")
        })
}

pub fn validate_config(project_root: &Path) -> ConfigValidationReport {
    let config_path = project_root.join(KEEL_DIR).join(CONFIG_FILE);
    let mut issues = Vec::new();

    if !config_path.is_file() {
        issues.push(ConfigValidationIssue::error(
            "config.exists",
            ".keel/config.toml is missing; run `keel init` first",
            Some(config_path.display().to_string()),
        ));
        return ConfigValidationReport::from_issues(config_path.display().to_string(), issues);
    }

    issues.push(ConfigValidationIssue::ok(
        "config.exists",
        ".keel/config.toml found",
        Some(config_path.display().to_string()),
    ));

    let content = match fs::read_to_string(&config_path) {
        Ok(content) => content,
        Err(error) => {
            issues.push(ConfigValidationIssue::error(
                "config.read",
                "failed to read .keel/config.toml",
                Some(error.to_string()),
            ));
            return ConfigValidationReport::from_issues(config_path.display().to_string(), issues);
        }
    };

    let value = match content.parse::<toml::Value>() {
        Ok(value) => {
            issues.push(ConfigValidationIssue::ok(
                "config.parse",
                "config parsed",
                None,
            ));
            value
        }
        Err(error) => {
            issues.push(ConfigValidationIssue::error(
                "config.parse",
                "failed to parse .keel/config.toml",
                Some(error.to_string()),
            ));
            return ConfigValidationReport::from_issues(config_path.display().to_string(), issues);
        }
    };

    validate_checks(&value, &mut issues);
    validate_agent_timeouts(&value, &mut issues);
    validate_readiness(&value, &mut issues);
    validate_risk(&value, &mut issues);

    ConfigValidationReport::from_issues(config_path.display().to_string(), issues)
}

fn validate_checks(value: &toml::Value, issues: &mut Vec<ConfigValidationIssue>) {
    let Some(checks_value) = value.get("checks") else {
        issues.push(ConfigValidationIssue::warning(
            "checks.commands",
            "checks.commands is empty",
            Some("no configured check commands were found".to_string()),
        ));
        return;
    };

    if let Some(table) = checks_value.as_table() {
        match table.get("commands") {
            None => issues.push(ConfigValidationIssue::warning(
                "checks.commands",
                "checks.commands is empty",
                Some("no configured check commands were found".to_string()),
            )),
            Some(commands_value) => match commands_value.as_array() {
                Some(commands) => validate_check_command_strings(commands, issues),
                None => issues.push(ConfigValidationIssue::error(
                    "checks.commands.type",
                    "checks.commands must be an array of strings",
                    Some(commands_value.to_string()),
                )),
            },
        }
        return;
    }

    if let Some(checks) = checks_value.as_array() {
        validate_legacy_checks(checks, issues);
        return;
    }

    issues.push(ConfigValidationIssue::error(
        "checks.type",
        "checks must be either a table with commands or a legacy array of tables",
        Some(checks_value.to_string()),
    ));
}

fn validate_check_command_strings(
    commands: &[toml::Value],
    issues: &mut Vec<ConfigValidationIssue>,
) {
    if commands.is_empty() {
        issues.push(ConfigValidationIssue::warning(
            "checks.commands",
            "checks.commands is empty",
            None,
        ));
        return;
    }

    let mut commands_seen = CommandDuplicateTracker::default();
    for (index, command) in commands.iter().enumerate() {
        match command.as_str() {
            Some(command) if command.trim().is_empty() => {
                issues.push(ConfigValidationIssue::error(
                    "checks.commands.empty",
                    "checks.commands contains an empty command",
                    Some(format!("index {index}")),
                ));
            }
            Some(command) => {
                commands_seen.insert(command);
            }
            None => {
                issues.push(ConfigValidationIssue::error(
                    "checks.commands.type",
                    "checks.commands entries must be strings",
                    Some(format!("index {index}")),
                ));
            }
        }
    }

    commands_seen.push_duplicate_warning(issues);

    if !issues
        .iter()
        .any(|issue| issue.id.starts_with("checks.commands."))
    {
        issues.push(ConfigValidationIssue::ok(
            "checks.commands",
            "checks.commands is valid",
            Some(format!("{} command(s)", commands.len())),
        ));
    }
}

fn validate_legacy_checks(checks: &[toml::Value], issues: &mut Vec<ConfigValidationIssue>) {
    if checks.is_empty() {
        issues.push(ConfigValidationIssue::warning(
            "checks.commands",
            "checks.commands is empty",
            Some("legacy [[checks]] array is empty".to_string()),
        ));
        return;
    }

    let mut commands_seen = CommandDuplicateTracker::default();
    let mut has_error = false;
    for (index, check) in checks.iter().enumerate() {
        let Some(table) = check.as_table() else {
            has_error = true;
            issues.push(ConfigValidationIssue::error(
                "checks.commands.type",
                "legacy [[checks]] entries must be tables",
                Some(format!("index {index}")),
            ));
            continue;
        };

        let Some(command_value) = table.get("command") else {
            has_error = true;
            issues.push(ConfigValidationIssue::error(
                "checks.commands.missing",
                "legacy check is missing command",
                Some(format!("index {index}")),
            ));
            continue;
        };

        let Some(command) = command_value.as_array() else {
            has_error = true;
            issues.push(ConfigValidationIssue::error(
                "checks.commands.type",
                "legacy check command must be an array of strings",
                Some(format!("index {index}: {}", command_value)),
            ));
            continue;
        };

        if command.is_empty() {
            has_error = true;
            issues.push(ConfigValidationIssue::error(
                "checks.commands.empty",
                "legacy check command is empty",
                Some(format!("index {index}")),
            ));
            continue;
        }

        let command_parts = command
            .iter()
            .map(toml::Value::as_str)
            .collect::<Option<Vec<_>>>();
        let Some(command_parts) = command_parts else {
            has_error = true;
            issues.push(ConfigValidationIssue::error(
                "checks.commands.type",
                "legacy check command entries must be strings",
                Some(format!("index {index}")),
            ));
            continue;
        };

        if command_parts.iter().any(|part| part.trim().is_empty()) {
            has_error = true;
            issues.push(ConfigValidationIssue::error(
                "checks.commands.empty",
                "legacy check command contains an empty part",
                Some(format!("index {index}")),
            ));
        }

        commands_seen.insert(command_parts.join(" "));
    }

    commands_seen.push_duplicate_warning(issues);

    if !has_error {
        issues.push(ConfigValidationIssue::ok(
            "checks.commands",
            "checks.commands is valid",
            Some(format!("{} command(s)", checks.len())),
        ));
    }
}

#[derive(Default)]
struct CommandDuplicateTracker {
    seen: HashSet<String>,
    duplicate_found: bool,
}

impl CommandDuplicateTracker {
    fn insert(&mut self, command: impl Into<String>) {
        if !self.seen.insert(command.into()) {
            self.duplicate_found = true;
        }
    }

    fn push_duplicate_warning(&self, issues: &mut Vec<ConfigValidationIssue>) {
        if self.duplicate_found {
            issues.push(ConfigValidationIssue::warning(
                "checks.commands.duplicates",
                "checks.commands contains duplicate commands",
                None,
            ));
        }
    }
}

fn validate_agent_timeouts(value: &toml::Value, issues: &mut Vec<ConfigValidationIssue>) {
    for agent in ["codex", "claude", "opencode"] {
        match agent_timeout(value, agent) {
            TimeoutValue::Valid(timeout) => {
                issues.push(ConfigValidationIssue::ok(
                    format!("agents.{agent}.timeout_seconds"),
                    format!("{agent} timeout_seconds: {timeout}"),
                    None,
                ));
            }
            TimeoutValue::Invalid(value) => {
                issues.push(ConfigValidationIssue::error(
                    format!("agents.{agent}.timeout_seconds"),
                    format!("{agent} timeout_seconds must be greater than 0"),
                    Some(value),
                ));
            }
        }

        match agent_enabled(value, agent) {
            BoolValue::Valid(enabled) => issues.push(ConfigValidationIssue::ok(
                format!("agents.{agent}.enabled"),
                format!("{agent} enabled: {enabled}"),
                None,
            )),
            BoolValue::Invalid(value) => issues.push(ConfigValidationIssue::error(
                format!("agents.{agent}.enabled"),
                format!("{agent} enabled must be a boolean"),
                Some(value),
            )),
        }
    }
}

enum TimeoutValue {
    Valid(u64),
    Invalid(String),
}

fn agent_timeout(value: &toml::Value, agent: &str) -> TimeoutValue {
    if let Some(agents) = value.get("agents").and_then(toml::Value::as_table) {
        if let Some(agent_value) = agents.get(agent) {
            if let Some(agent_table) = agent_value.as_table() {
                if let Some(timeout_value) = agent_table.get("timeout_seconds") {
                    return timeout_from_value(timeout_value);
                }
            } else {
                return TimeoutValue::Invalid(agent_value.to_string());
            }
        }
    }

    if let Some(timeout_value) = value.get("agent_timeout_secs") {
        return timeout_from_value(timeout_value);
    }

    TimeoutValue::Valid(default_agent_timeout_secs())
}

fn timeout_from_value(timeout_value: &toml::Value) -> TimeoutValue {
    let Some(timeout) = timeout_value.as_integer() else {
        return TimeoutValue::Invalid(timeout_value.to_string());
    };
    positive_timeout(timeout)
}

fn positive_timeout(timeout: i64) -> TimeoutValue {
    match u64::try_from(timeout) {
        Ok(timeout) if timeout > 0 => TimeoutValue::Valid(timeout),
        _ => TimeoutValue::Invalid(timeout.to_string()),
    }
}

enum BoolValue {
    Valid(bool),
    Invalid(String),
}

fn agent_enabled(value: &toml::Value, agent: &str) -> BoolValue {
    let Some(agents) = value.get("agents").and_then(toml::Value::as_table) else {
        return BoolValue::Valid(default_agent_enabled());
    };
    let Some(agent_value) = agents.get(agent) else {
        return BoolValue::Valid(default_agent_enabled());
    };
    let Some(agent_table) = agent_value.as_table() else {
        return BoolValue::Invalid(agent_value.to_string());
    };
    let Some(enabled_value) = agent_table.get("enabled") else {
        return BoolValue::Valid(default_agent_enabled());
    };
    match enabled_value.as_bool() {
        Some(enabled) => BoolValue::Valid(enabled),
        None => BoolValue::Invalid(enabled_value.to_string()),
    }
}

fn validate_readiness(value: &toml::Value, issues: &mut Vec<ConfigValidationIssue>) {
    match readiness_bool(
        value,
        "require_non_empty_diff",
        default_require_non_empty_diff(),
    ) {
        BoolValue::Valid(require_non_empty_diff) => issues.push(ConfigValidationIssue::ok(
            "readiness.require_non_empty_diff",
            format!("readiness require_non_empty_diff: {require_non_empty_diff}"),
            None,
        )),
        BoolValue::Invalid(value) => issues.push(ConfigValidationIssue::error(
            "readiness.require_non_empty_diff",
            "readiness.require_non_empty_diff must be a boolean",
            Some(value),
        )),
    }

    match readiness_bool(value, "require_checks_pass", default_require_checks_pass()) {
        BoolValue::Valid(require_checks_pass) => issues.push(ConfigValidationIssue::ok(
            "readiness.require_checks_pass",
            format!("readiness require_checks_pass: {require_checks_pass}"),
            None,
        )),
        BoolValue::Invalid(value) => issues.push(ConfigValidationIssue::error(
            "readiness.require_checks_pass",
            "readiness.require_checks_pass must be a boolean",
            Some(value),
        )),
    }
}

fn validate_risk(value: &toml::Value, issues: &mut Vec<ConfigValidationIssue>) {
    let Some(risk_value) = value.get("risk") else {
        issues.push(ConfigValidationIssue::ok(
            "risk.paths",
            "risk paths are valid",
            Some("0 pattern(s); default []".to_string()),
        ));
        issues.push(ConfigValidationIssue::ok(
            "risk.large_diff_file_threshold",
            format!(
                "risk large_diff_file_threshold: {}",
                default_large_diff_file_threshold()
            ),
            Some("default".to_string()),
        ));
        return;
    };

    let Some(risk) = risk_value.as_table() else {
        issues.push(ConfigValidationIssue::error(
            "risk.type",
            "risk must be a table",
            Some(risk_value.to_string()),
        ));
        return;
    };

    validate_risk_paths(risk.get("paths"), issues);
    validate_large_diff_threshold(risk.get("large_diff_file_threshold"), issues);
}

fn validate_risk_paths(paths_value: Option<&toml::Value>, issues: &mut Vec<ConfigValidationIssue>) {
    let Some(paths_value) = paths_value else {
        issues.push(ConfigValidationIssue::ok(
            "risk.paths",
            "risk paths are valid",
            Some("0 pattern(s); default []".to_string()),
        ));
        return;
    };

    let Some(paths) = paths_value.as_array() else {
        issues.push(ConfigValidationIssue::error(
            "risk.paths.type",
            "risk.paths must be an array of glob strings",
            Some(paths_value.to_string()),
        ));
        return;
    };

    let mut has_error = false;
    for (index, pattern_value) in paths.iter().enumerate() {
        let Some(pattern) = pattern_value.as_str() else {
            has_error = true;
            issues.push(ConfigValidationIssue::error(
                "risk.paths.type",
                "risk.paths entries must be strings",
                Some(format!("index {index}")),
            ));
            continue;
        };

        if pattern.trim().is_empty() {
            has_error = true;
            issues.push(ConfigValidationIssue::error(
                "risk.paths.empty",
                "risk.paths contains an empty pattern",
                Some(format!("index {index}")),
            ));
            continue;
        }

        if let Err(error) = Glob::new(pattern) {
            has_error = true;
            issues.push(ConfigValidationIssue::error(
                "risk.paths.glob",
                "risk.paths contains an invalid glob pattern",
                Some(format!("{pattern}: {error}")),
            ));
        }
    }

    if !has_error {
        issues.push(ConfigValidationIssue::ok(
            "risk.paths",
            "risk paths are valid",
            Some(format!("{} pattern(s)", paths.len())),
        ));
    }
}

fn validate_large_diff_threshold(
    threshold_value: Option<&toml::Value>,
    issues: &mut Vec<ConfigValidationIssue>,
) {
    let Some(threshold_value) = threshold_value else {
        issues.push(ConfigValidationIssue::ok(
            "risk.large_diff_file_threshold",
            format!(
                "risk large_diff_file_threshold: {}",
                default_large_diff_file_threshold()
            ),
            Some("default".to_string()),
        ));
        return;
    };

    let Some(threshold) = threshold_value.as_integer() else {
        issues.push(ConfigValidationIssue::error(
            "risk.large_diff_file_threshold",
            "risk.large_diff_file_threshold must be a positive integer",
            Some(threshold_value.to_string()),
        ));
        return;
    };

    match usize::try_from(threshold) {
        Ok(threshold) if threshold > 0 => issues.push(ConfigValidationIssue::ok(
            "risk.large_diff_file_threshold",
            format!("risk large_diff_file_threshold: {threshold}"),
            None,
        )),
        _ => issues.push(ConfigValidationIssue::error(
            "risk.large_diff_file_threshold",
            "risk.large_diff_file_threshold must be greater than 0",
            Some(threshold.to_string()),
        )),
    }
}

fn readiness_bool(value: &toml::Value, key: &str, default: bool) -> BoolValue {
    let Some(readiness) = value.get("readiness").and_then(toml::Value::as_table) else {
        return BoolValue::Valid(default);
    };
    let Some(value) = readiness.get(key) else {
        return BoolValue::Valid(default);
    };
    match value.as_bool() {
        Some(boolean) => BoolValue::Valid(boolean),
        None => BoolValue::Invalid(value.to_string()),
    }
}

impl Default for AgentsConfig {
    fn default() -> Self {
        Self {
            codex: default_agent_config(),
            claude: default_agent_config(),
            opencode: default_agent_config(),
        }
    }
}

impl Default for ReadinessConfig {
    fn default() -> Self {
        Self {
            require_non_empty_diff: default_require_non_empty_diff(),
            require_checks_pass: default_require_checks_pass(),
        }
    }
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            large_diff_file_threshold: default_large_diff_file_threshold(),
        }
    }
}

fn default_agent_config() -> AgentConfig {
    AgentConfig {
        enabled: default_agent_enabled(),
        timeout_seconds: default_agent_timeout_secs(),
    }
}

fn default_agent_enabled() -> bool {
    true
}

fn default_require_non_empty_diff() -> bool {
    true
}

fn default_require_checks_pass() -> bool {
    true
}

pub fn default_large_diff_file_threshold() -> usize {
    20
}

impl ConfigValidationReport {
    fn from_issues(config_path: String, issues: Vec<ConfigValidationIssue>) -> Self {
        let summary = ConfigValidationSummary {
            ok: issues
                .iter()
                .filter(|issue| issue.severity == ConfigValidationSeverity::Ok)
                .count(),
            warnings: issues
                .iter()
                .filter(|issue| issue.severity == ConfigValidationSeverity::Warning)
                .count(),
            errors: issues
                .iter()
                .filter(|issue| issue.severity == ConfigValidationSeverity::Error)
                .count(),
        };
        Self {
            ok: summary.errors == 0,
            config_path,
            summary,
            issues,
        }
    }
}

impl ConfigValidationIssue {
    fn ok(id: impl Into<String>, message: impl Into<String>, details: Option<String>) -> Self {
        Self::new(id, ConfigValidationSeverity::Ok, message, details)
    }

    fn warning(id: impl Into<String>, message: impl Into<String>, details: Option<String>) -> Self {
        Self::new(id, ConfigValidationSeverity::Warning, message, details)
    }

    fn error(id: impl Into<String>, message: impl Into<String>, details: Option<String>) -> Self {
        Self::new(id, ConfigValidationSeverity::Error, message, details)
    }

    fn new(
        id: impl Into<String>,
        severity: ConfigValidationSeverity,
        message: impl Into<String>,
        details: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            severity,
            message: message.into(),
            details,
        }
    }
}
