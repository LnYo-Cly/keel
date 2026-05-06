pub(crate) const KEEL_DIR: &str = ".keel";
pub(crate) const RUNS_DIR: &str = "runs";
pub(crate) const WORKTREES_DIR: &str = "worktrees";
pub(crate) const CONFIG_FILE: &str = "config.toml";
pub(crate) const METADATA_FILE: &str = "metadata.json";
pub(crate) const LOG_FILE: &str = "log.txt";
pub(crate) const DIFF_FILE: &str = "diff.patch";
pub(crate) const CHECKS_FILE: &str = "checks.json";
pub(crate) const REPORT_FILE: &str = "report.md";
pub(crate) const COMMIT_FILE: &str = "commit.json";
pub(crate) const PUSH_FILE: &str = "push.json";
pub(crate) const PR_FILE: &str = "pr.json";
pub(crate) const LEGACY_PUBLISH_FILE: &str = "publish.json";
pub(crate) const NOOP_OUTPUT_FILE: &str = "keel-noop-output.txt";
pub(crate) const DEFAULT_AGENT_TIMEOUT_SECS: u64 = 900;
pub(crate) const REPORT_OUTPUT_LIMIT: usize = 4000;

pub(crate) mod artifact_labels {
    pub(crate) const METADATA: &str = "Metadata";
    pub(crate) const LOG: &str = "Log";
    pub(crate) const DIFF: &str = "Diff";
    pub(crate) const CHECKS: &str = "Checks";
    pub(crate) const REPORT: &str = "Report";
    pub(crate) const COMMIT: &str = "Commit";
    pub(crate) const PUSH: &str = "Push";
    pub(crate) const PR: &str = "PR/MR";
}

pub mod artifact_keys {
    pub const METADATA: &str = "metadata";
    pub const LOG: &str = "log";
    pub const DIFF: &str = "diff";
    pub const CHECKS: &str = "checks";
    pub const REPORT: &str = "report";
    pub const COMMIT: &str = "commit";
    pub const PUSH: &str = "push";
    pub const PR: &str = "pr";
}

#[derive(Debug, Clone, Copy)]
pub struct RunArtifactSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub file: &'static str,
    pub required: bool,
}

pub const RUN_ARTIFACTS: &[RunArtifactSpec] = &[
    RunArtifactSpec {
        key: artifact_keys::METADATA,
        label: artifact_labels::METADATA,
        file: METADATA_FILE,
        required: true,
    },
    RunArtifactSpec {
        key: artifact_keys::LOG,
        label: artifact_labels::LOG,
        file: LOG_FILE,
        required: true,
    },
    RunArtifactSpec {
        key: artifact_keys::DIFF,
        label: artifact_labels::DIFF,
        file: DIFF_FILE,
        required: true,
    },
    RunArtifactSpec {
        key: artifact_keys::CHECKS,
        label: artifact_labels::CHECKS,
        file: CHECKS_FILE,
        required: true,
    },
    RunArtifactSpec {
        key: artifact_keys::REPORT,
        label: artifact_labels::REPORT,
        file: REPORT_FILE,
        required: true,
    },
    RunArtifactSpec {
        key: artifact_keys::COMMIT,
        label: artifact_labels::COMMIT,
        file: COMMIT_FILE,
        required: false,
    },
    RunArtifactSpec {
        key: artifact_keys::PUSH,
        label: artifact_labels::PUSH,
        file: PUSH_FILE,
        required: false,
    },
    RunArtifactSpec {
        key: artifact_keys::PR,
        label: artifact_labels::PR,
        file: PR_FILE,
        required: false,
    },
];

pub fn run_artifact_spec(key: &str) -> Option<&'static RunArtifactSpec> {
    RUN_ARTIFACTS.iter().find(|artifact| artifact.key == key)
}
