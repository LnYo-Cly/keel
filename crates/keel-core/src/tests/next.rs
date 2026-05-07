use super::*;

#[test]
fn next_summarizes_active_ledger_and_latest_candidate() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let run = project.run("candidate next action", "noop").unwrap();
    project.start_ledger_task("self dogfood next").unwrap();
    project.checkpoint("implemented next model").unwrap();
    project.evidence("git --version", Vec::new()).unwrap();

    let next = project.next().unwrap();

    assert!(next.ledger.active);
    assert_eq!(next.ledger.title.as_deref(), Some("self dogfood next"));
    assert_eq!(
        next.ledger.decision.as_ref().map(|decision| decision.ready),
        Some(true)
    );
    assert_eq!(next.candidate.as_ref().unwrap().run_id, run.run_id);
    assert_eq!(
        next.candidate.as_ref().unwrap().primary_action.as_deref(),
        Some(format!("keel commit {} --dry-run", run.run_id).as_str())
    );
    assert!(next
        .recommended_actions
        .contains(&format!("keel commit {} --dry-run", run.run_id)));
    assert_eq!(next.ledger.primary_action, "keel review");
}

#[test]
fn next_handles_missing_ledger_and_missing_runs() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let next = project.next().unwrap();

    assert!(!next.ledger.active);
    assert_eq!(next.ledger.primary_action, "keel task start \"...\"");
    assert!(next.candidate.is_none());
    assert!(next
        .recommended_actions
        .contains(&"keel run \"...\" --agent noop".to_string()));
}

#[test]
fn next_prioritizes_check_when_ledger_has_no_or_failed_evidence() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    project.start_ledger_task("needs checks").unwrap();

    let missing = project.next().unwrap();
    assert_eq!(missing.ledger.primary_action, "keel check");
    assert_eq!(missing.recommended_actions[0], "keel check");

    project
        .evidence("definitely-not-a-keel-next-test-command", Vec::new())
        .unwrap();
    let failed = project.next().unwrap();
    assert_eq!(failed.ledger.primary_action, "keel check");
    assert_eq!(failed.recommended_actions[0], "keel check");
}

#[test]
fn next_prioritizes_handoff_when_passing_ledger_workspace_is_clean() {
    let temp = git_repo();
    fs::write(
        temp.path().join(".git").join("info").join("exclude"),
        ".keel/\n",
    )
    .unwrap();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    project.start_ledger_task("clean handoff").unwrap();
    project.evidence("git --version", Vec::new()).unwrap();

    let next = project.next().unwrap();

    assert_eq!(next.ledger.workspace_dirty, Some(false));
    assert_eq!(next.ledger.primary_action, "keel handoff");
    assert_eq!(next.recommended_actions[0], "keel handoff");
}
