use super::*;

#[test]
fn config_validation_accepts_default_legacy_config() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let report = validate_config(temp.path());

    assert!(report.ok);
    assert_eq!(
        issue_severity(&report, "config.exists"),
        ConfigValidationSeverity::Ok
    );
    assert_eq!(
        issue_severity(&report, "config.parse"),
        ConfigValidationSeverity::Ok
    );
    assert_eq!(
        issue_severity(&report, "checks.commands"),
        ConfigValidationSeverity::Ok
    );
    assert_eq!(
        issue_severity(&report, "agents.codex.timeout_seconds"),
        ConfigValidationSeverity::Ok
    );
}

#[test]
fn config_validation_reports_missing_or_invalid_config() {
    let temp = git_repo();

    let missing = validate_config(temp.path());

    assert!(!missing.ok);
    assert_eq!(
        issue_severity(&missing, "config.exists"),
        ConfigValidationSeverity::Error
    );

    let config_dir = temp.path().join(KEEL_DIR);
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(config_dir.join(CONFIG_FILE), "not = [valid").unwrap();

    let invalid = validate_config(temp.path());

    assert!(!invalid.ok);
    assert_eq!(
        issue_severity(&invalid, "config.parse"),
        ConfigValidationSeverity::Error
    );
}

#[test]
fn config_validation_rejects_zero_timeout_and_empty_check_command() {
    let temp = git_repo();
    let config_dir = temp.path().join(KEEL_DIR);
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join(CONFIG_FILE),
        r#"
[checks]
commands = [""]

[agents.codex]
timeout_seconds = 0
"#,
    )
    .unwrap();

    let report = validate_config(temp.path());

    assert!(!report.ok);
    assert_eq!(
        issue_severity(&report, "checks.commands.empty"),
        ConfigValidationSeverity::Error
    );
    assert_eq!(
        issue_severity(&report, "agents.codex.timeout_seconds"),
        ConfigValidationSeverity::Error
    );
}

#[test]
fn config_validation_warns_for_empty_future_checks_commands() {
    let temp = git_repo();
    let config_dir = temp.path().join(KEEL_DIR);
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join(CONFIG_FILE),
        r#"
[checks]
commands = []
"#,
    )
    .unwrap();

    let report = validate_config(temp.path());

    assert!(report.ok);
    assert_eq!(
        issue_severity(&report, "checks.commands"),
        ConfigValidationSeverity::Warning
    );
}

#[test]
fn config_validation_accepts_default_risk_config() {
    let temp = git_repo();
    let config_dir = temp.path().join(KEEL_DIR);
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join(CONFIG_FILE),
        r#"
[checks]
commands = []
"#,
    )
    .unwrap();

    let report = validate_config(temp.path());

    assert!(report.ok);
    assert_eq!(
        issue_severity(&report, "risk.paths"),
        ConfigValidationSeverity::Ok
    );
    assert_eq!(
        issue_severity(&report, "risk.large_diff_file_threshold"),
        ConfigValidationSeverity::Ok
    );
}

#[test]
fn config_validation_rejects_invalid_risk_config() {
    let temp = git_repo();
    let config_dir = temp.path().join(KEEL_DIR);
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join(CONFIG_FILE),
        r#"
[risk]
paths = ["", "["]
large_diff_file_threshold = 0
"#,
    )
    .unwrap();

    let report = validate_config(temp.path());

    assert!(!report.ok);
    assert_eq!(
        issue_severity(&report, "risk.paths.empty"),
        ConfigValidationSeverity::Error
    );
    assert_eq!(
        issue_severity(&report, "risk.paths.glob"),
        ConfigValidationSeverity::Error
    );
    assert_eq!(
        issue_severity(&report, "risk.large_diff_file_threshold"),
        ConfigValidationSeverity::Error
    );
}

fn issue_severity(
    report: &crate::config::ConfigValidationReport,
    id: &str,
) -> ConfigValidationSeverity {
    report
        .issues
        .iter()
        .find(|issue| issue.id == id)
        .map(|issue| issue.severity)
        .unwrap()
}
