mod agents;
mod checks;
mod command;
mod commit;
mod config;
mod constants;
mod doctor;
mod fsio;
mod git;
mod json;
mod ledger;
mod model;
mod pr;
mod project;
mod push;
mod report;
mod risk;
mod run;
mod time;

pub mod artifact_files {
    pub const METADATA: &str = crate::constants::METADATA_FILE;
    pub const LOG: &str = crate::constants::LOG_FILE;
    pub const DIFF: &str = crate::constants::DIFF_FILE;
    pub const CHECKS: &str = crate::constants::CHECKS_FILE;
    pub const REPORT: &str = crate::constants::REPORT_FILE;
    pub const COMMIT: &str = crate::constants::COMMIT_FILE;
    pub const PUSH: &str = crate::constants::PUSH_FILE;
    pub const PR: &str = crate::constants::PR_FILE;
}

pub use commit::{CommitArtifact, CommitOptions, CommitResult};
pub use config::{
    validate_config, AgentConfig, AgentsConfig, ChecksConfig, Config, ConfigValidationIssue,
    ConfigValidationReport, ConfigValidationSeverity, ConfigValidationSummary, ReadinessConfig,
    RiskConfig,
};
pub use constants::{artifact_keys, RunArtifactSpec, RUN_ARTIFACTS};
pub use doctor::{run_doctor, DoctorCheck, DoctorReport, DoctorStatus, DoctorSummary};
pub use json::{
    ledger_handoff_json, ledger_review_json, report_json, status_json, ArtifactJson,
    ArtifactSetJson, LedgerHandoffJson, LedgerReviewJson, ReportJson, RunSummaryJson,
};
pub use ledger::{
    ChangedFileGroup, LedgerCheckpoint, LedgerDecision, LedgerEvidence, LedgerEvidenceBrief,
    LedgerEvidenceEnv, LedgerEvidencePacket, LedgerEvidenceStatus, LedgerHandoff, LedgerNote,
    LedgerReview, LedgerReviewPacket, LedgerStatus, LedgerSummary, LedgerTask, LedgerTaskReport,
    LedgerTaskStatus, LedgerTaskSummary, LedgerWorkspaceContextKind, WorkspaceContext,
};
pub use model::{
    ArtifactInfo, CheckResult, CheckStatus, DiffInfo, FailureReason, InitResult, LogInfo,
    ReportInfo, RunArtifacts, RunMetadata, RunStatus,
};
pub use pr::{infer_provider, PrArtifact, PrOptions, PrPlan, PrProvider, PrResult};
pub use project::KeelProject;
pub use push::{PushArtifact, PushOptions, PushResult};
pub use report::{
    primary_next_action, suggested_next_actions, ReviewNextAction, ReviewNextActionKind,
};
pub use risk::{RiskWarning, RiskWarningKind};

#[cfg(test)]
mod tests;
