use super::*;

#[test]
fn list_runs_sorts_newest_first() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let first = project.run("first run", "noop").unwrap();
    let second = project.run("second run", "noop").unwrap();

    let runs = project.list_runs().unwrap();

    assert_eq!(runs[0].run_id, second.run_id);
    assert_eq!(runs[1].run_id, first.run_id);
}

#[test]
fn created_at_sort_prefers_parseable_values_with_string_fallback() {
    assert!(compare_created_at_for_test("2026-04-30T10:01:00Z", "2026-04-30T10:00:00Z").is_lt());
    assert!(
        compare_created_at_for_test("2026-04-30T18:00:00+08:00", "2026-04-30T09:00:00Z").is_lt()
    );
    assert!(compare_created_at_for_test("200", "100").is_lt());
    assert!(compare_created_at_for_test("z-legacy", "a-legacy").is_lt());
}

#[test]
fn list_runs_sorts_legacy_metadata_by_created_at_compatibly() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let older = project.run("older legacy run", "noop").unwrap();
    let newer = project.run("newer legacy run", "noop").unwrap();
    let mut older_metadata = read_metadata(&temp, &older.run_id);
    older_metadata.created_at = "2026-04-30T10:00:00Z".to_string();
    project.write_metadata(&older_metadata).unwrap();
    let mut newer_metadata = read_metadata(&temp, &newer.run_id);
    newer_metadata.created_at = "2026-04-30T10:01:00Z".to_string();
    project.write_metadata(&newer_metadata).unwrap();

    let runs = project.list_runs().unwrap();

    assert_eq!(runs[0].run_id, newer.run_id);
    assert_eq!(runs[1].run_id, older.run_id);
}

#[test]
fn report_includes_artifact_paths_and_next_actions() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("review this run", "noop").unwrap();

    let report = project.report(&metadata.run_id).unwrap();

    assert_eq!(report.metadata.run_id, metadata.run_id);
    assert_eq!(
        report.path,
        run_dir(&temp, &metadata.run_id).join(REPORT_FILE)
    );
    assert!(report.artifacts.iter().any(|artifact| {
        artifact.label == "Metadata"
            && artifact.path == run_dir(&temp, &metadata.run_id).join(METADATA_FILE)
            && artifact.exists
            && artifact.required
    }));
    assert!(report.artifacts.iter().any(|artifact| {
        artifact.label == "Log"
            && artifact.path == run_dir(&temp, &metadata.run_id).join(LOG_FILE)
            && artifact.exists
            && artifact.required
    }));
    assert!(report.artifacts.iter().any(|artifact| {
        artifact.label == "Diff"
            && artifact.path == run_dir(&temp, &metadata.run_id).join(DIFF_FILE)
            && artifact.exists
            && artifact.required
    }));
    assert!(report
        .artifacts
        .iter()
        .any(|artifact| artifact.label == "Commit" && !artifact.required));
    assert!(report
        .next_actions
        .contains(&format!("keel diff {}", metadata.run_id)));
    assert!(report
        .next_actions
        .contains(&format!("keel log {}", metadata.run_id)));
    assert!(report
        .next_actions
        .contains(&format!("keel commit {} --dry-run", metadata.run_id)));
    assert!(report
        .next_actions
        .contains(&format!("keel commit {}", metadata.run_id)));
    assert!(report
        .next_actions
        .contains(&format!("keel discard {}", metadata.run_id)));
}

#[test]
fn report_info_constructor_derives_review_state_from_metadata() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("constructor review state", "noop").unwrap();
    project
        .commit(
            &metadata.run_id,
            CommitOptions {
                dry_run: false,
                message: None,
            },
        )
        .unwrap();

    let mut metadata = read_metadata(&temp, &metadata.run_id);
    metadata.status = RunStatus::Discarded;
    metadata.pushed = true;
    metadata.pushed_at = Some("2026-04-30T00:00:00Z".to_string());
    metadata.push_remote = Some("origin".to_string());
    metadata.push_remote_url = Some("git@github.com:owner/repo.git".to_string());
    metadata.pushed_branch = Some(metadata.branch.clone());
    metadata.push = None;
    metadata.pr_created = true;
    metadata.pr_created_at = Some("2026-04-30T01:00:00Z".to_string());
    metadata.pr_provider = Some("github".to_string());
    metadata.pr_url = Some("https://github.com/owner/repo/pull/1".to_string());
    metadata.pr_target_branch = Some("main".to_string());
    metadata.pr_source_branch = Some(metadata.branch.clone());
    metadata.pr = None;

    let report = crate::model::ReportInfo::new(
        metadata,
        PathBuf::from("report.md"),
        "summary",
        Vec::new(),
        Vec::new(),
    );

    assert!(report.is_discarded);
    assert!(report.commit.is_some());
    assert!(report.push.is_some());
    assert!(report.pr.is_some());
}

#[test]
fn report_next_actions_follow_commit_push_pr_progress() {
    let temp = git_repo();
    let remote = bare_git_repo();
    add_origin(&temp, &remote);
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("progressive next actions", "noop").unwrap();

    let initial = project.report(&metadata.run_id).unwrap();
    assert!(initial
        .next_actions
        .contains(&format!("keel commit {} --dry-run", metadata.run_id)));
    assert!(!initial
        .next_actions
        .contains(&format!("keel push {} --dry-run", metadata.run_id)));

    project
        .commit(
            &metadata.run_id,
            CommitOptions {
                dry_run: false,
                message: None,
            },
        )
        .unwrap();
    let committed = project.report(&metadata.run_id).unwrap();
    assert!(committed
        .next_actions
        .contains(&format!("keel push {} --dry-run", metadata.run_id)));
    assert!(committed
        .next_actions
        .contains(&format!("keel push {}", metadata.run_id)));
    assert!(!committed
        .next_actions
        .contains(&format!("keel commit {}", metadata.run_id)));

    project.push(&metadata.run_id, push_options(false)).unwrap();
    let pushed = project.report(&metadata.run_id).unwrap();
    assert!(pushed
        .next_actions
        .contains(&format!("keel pr {} --manual --dry-run", metadata.run_id)));
    assert!(!pushed
        .next_actions
        .contains(&format!("keel pr {} --provider github", metadata.run_id)));
}

#[test]
fn report_next_actions_offer_github_provider_pr_after_github_push() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("github next action", "noop").unwrap();
    project
        .commit(
            &metadata.run_id,
            CommitOptions {
                dry_run: false,
                message: None,
            },
        )
        .unwrap();
    let mut pushed = read_metadata(&temp, &metadata.run_id);
    pushed.pushed = true;
    pushed.pushed_at = Some("2026-04-30T00:00:00Z".to_string());
    pushed.push_remote = Some("origin".to_string());
    pushed.push_remote_url = Some("git@github.com:owner/repo.git".to_string());
    pushed.pushed_branch = Some(pushed.branch.clone());
    project.write_metadata(&pushed).unwrap();

    let report = project.report(&metadata.run_id).unwrap();

    assert!(report.next_actions.contains(&format!(
        "keel pr {} --provider github --dry-run",
        metadata.run_id
    )));
    assert!(report
        .next_actions
        .contains(&format!("keel pr {} --provider github", metadata.run_id)));
}

#[test]
fn primary_next_action_follows_review_progress() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("primary next action", "noop").unwrap();

    assert_eq!(
        primary_next_action(&metadata).map(|action| action.command),
        Some(format!("keel commit {} --dry-run", metadata.run_id))
    );

    let mut committed = metadata.clone();
    committed.committed = true;
    assert_eq!(
        primary_next_action(&committed).map(|action| action.command),
        Some(format!("keel push {} --dry-run", metadata.run_id))
    );

    let mut pushed = committed.clone();
    pushed.pushed = true;
    pushed.push_remote_url = Some("git@github.com:owner/repo.git".to_string());
    assert_eq!(
        primary_next_action(&pushed).map(|action| action.command),
        Some(format!(
            "keel pr {} --provider github --dry-run",
            metadata.run_id
        ))
    );

    let mut not_ready = metadata.clone();
    not_ready.status = RunStatus::NotReady;
    assert_eq!(
        primary_next_action(&not_ready).map(|action| action.command),
        Some(format!("keel log {}", metadata.run_id))
    );

    let mut running = metadata.clone();
    running.status = RunStatus::Running;
    assert_eq!(
        primary_next_action(&running).map(|action| action.command),
        Some("keel status".to_string())
    );
}

#[test]
fn core_json_views_cover_status_and_report_shapes() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("json view run", "noop").unwrap();

    let status = serde_json::to_value(status_json(std::slice::from_ref(&metadata))).unwrap();
    assert_eq!(status[0]["run_id"], metadata.run_id);
    assert_eq!(status[0]["agent"], "noop");
    assert_eq!(status[0]["status"], "ready");
    assert_eq!(status[0]["branch"], metadata.branch);

    let report = project.report(&metadata.run_id).unwrap();
    let report = serde_json::to_value(report_json(&report)).unwrap();

    assert_eq!(report["run_id"], metadata.run_id);
    assert_eq!(report["agent"], "noop");
    assert_eq!(report["status"], "ready");
    for artifact in RUN_ARTIFACTS {
        assert_eq!(
            report["artifacts"][artifact.key]["key"], artifact.key,
            "artifact key mismatch for {}",
            artifact.key
        );
        assert_eq!(report["artifacts"][artifact.key]["label"], artifact.label);
        assert_eq!(
            report["artifacts"][artifact.key]["required"],
            artifact.required
        );
    }
    assert_eq!(report["artifacts"]["metadata"]["exists"], true);
    assert_eq!(report["artifacts"]["log"]["exists"], true);
    assert_eq!(report["artifacts"]["diff"]["exists"], true);
    assert!(report["next_actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action == &format!("keel diff {}", metadata.run_id)));
    assert!(report["next_actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action == &format!("keel commit {}", metadata.run_id)));
}

#[test]
fn report_json_artifacts_are_keyed_independent_of_input_order() {
    let temp = git_repo();
    let run_dir = run_dir(&temp, "run-keyed");
    let artifacts = vec![
        artifact_info(&run_dir, "push", false),
        artifact_info(&run_dir, "metadata", true),
        artifact_info(&run_dir, "pr", false),
        artifact_info(&run_dir, "log", true),
        artifact_info(&run_dir, "commit", false),
        artifact_info(&run_dir, "report", true),
        artifact_info(&run_dir, "checks", true),
        artifact_info(&run_dir, "diff", true),
    ];
    let report = crate::model::ReportInfo::new(
        RunMetadata::new(
            "run-keyed",
            "keyed artifacts",
            "noop",
            RunStatus::Ready,
            "1",
        ),
        run_dir.join(REPORT_FILE),
        "summary",
        artifacts,
        Vec::new(),
    );

    let json = serde_json::to_value(report_json(&report)).unwrap();

    for spec in RUN_ARTIFACTS {
        assert_eq!(json["artifacts"][spec.key]["key"], spec.key);
        assert_eq!(json["artifacts"][spec.key]["label"], spec.label);
        assert_eq!(json["artifacts"][spec.key]["required"], spec.required);
        assert_eq!(
            json["artifacts"][spec.key]["path"],
            run_dir.join(spec.file).display().to_string()
        );
    }
}

#[test]
fn report_marks_missing_artifacts_without_failing() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("missing artifact report", "noop").unwrap();
    fs::remove_file(run_dir(&temp, &metadata.run_id).join(LOG_FILE)).unwrap();

    let report = project.report(&metadata.run_id).unwrap();

    assert!(report
        .artifacts
        .iter()
        .any(|artifact| artifact.label == "Log" && !artifact.exists));
}

#[test]
fn log_reads_saved_log_and_empty_log() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("read log", "noop").unwrap();

    let log = project.log(&metadata.run_id).unwrap();

    assert_eq!(log.path, run_dir(&temp, &metadata.run_id).join(LOG_FILE));
    assert!(!log.is_empty);
    assert!(log.content.contains("created run"));

    fs::write(run_dir(&temp, &metadata.run_id).join(LOG_FILE), "").unwrap();
    let empty_log = project.log(&metadata.run_id).unwrap();
    assert!(empty_log.is_empty);
    assert!(empty_log.content.is_empty());
}

#[test]
fn log_errors_when_run_or_file_is_missing() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let missing_run = project.log("run-does-not-exist").unwrap_err().to_string();
    assert!(missing_run.contains("run `run-does-not-exist` does not exist"));

    let metadata = project.run("missing log", "noop").unwrap();
    fs::remove_file(run_dir(&temp, &metadata.run_id).join(LOG_FILE)).unwrap();

    let missing_log = project.log(&metadata.run_id).unwrap_err().to_string();
    assert!(missing_log.contains("log for run"));
}

#[test]
fn diff_reads_saved_patch() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("read diff", "noop").unwrap();

    let diff = project.diff(&metadata.run_id).unwrap();

    assert_eq!(diff.path, run_dir(&temp, &metadata.run_id).join(DIFF_FILE));
    assert!(!diff.is_empty);
    assert!(diff.content.contains(NOOP_OUTPUT_FILE));
}

#[test]
fn diff_reports_empty_patch() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("empty diff", "noop").unwrap();
    fs::write(run_dir(&temp, &metadata.run_id).join(DIFF_FILE), "").unwrap();

    let diff = project.diff(&metadata.run_id).unwrap();

    assert!(diff.is_empty);
    assert!(diff.content.is_empty());
}

#[test]
fn diff_errors_when_run_or_patch_is_missing() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let missing_run = project.diff("run-does-not-exist").unwrap_err().to_string();
    assert!(missing_run.contains("run `run-does-not-exist` does not exist"));

    let metadata = project.run("missing diff", "noop").unwrap();
    fs::remove_file(run_dir(&temp, &metadata.run_id).join(DIFF_FILE)).unwrap();

    let missing_diff = project.diff(&metadata.run_id).unwrap_err().to_string();
    assert!(missing_diff.contains("diff for run"));
}

fn artifact_info(run_dir: &Path, key: &'static str, exists: bool) -> crate::ArtifactInfo {
    let spec = RUN_ARTIFACTS
        .iter()
        .find(|artifact| artifact.key == key)
        .expect("test artifact spec should exist");
    crate::ArtifactInfo::from_spec(spec, run_dir.join(spec.file), exists)
}
