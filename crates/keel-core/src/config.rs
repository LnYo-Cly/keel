use crate::agents::default_agent_timeout_secs;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct KeelConfig {
    #[serde(default = "default_agent_timeout_secs")]
    pub(crate) agent_timeout_secs: u64,
    #[serde(default = "default_checks")]
    pub(crate) checks: Vec<ConfiguredCheck>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ConfiguredCheck {
    pub(crate) name: String,
    pub(crate) command: Vec<String>,
    #[serde(default)]
    pub(crate) run_if_path_exists: Option<String>,
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
        },
        ConfiguredCheck {
            name: "cargo test".to_string(),
            command: vec!["cargo".to_string(), "test".to_string()],
            run_if_path_exists: Some("Cargo.toml".to_string()),
        },
    ]
}
