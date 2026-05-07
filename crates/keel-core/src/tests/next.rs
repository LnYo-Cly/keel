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
