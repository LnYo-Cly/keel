use super::*;

#[test]
fn discover_requires_git_repo() {
    let temp = TempDir::new().unwrap();
    let error = KeelProject::discover(temp.path()).unwrap_err().to_string();
    assert!(error.contains("git repository"));
}

#[test]
fn init_creates_keel_layout() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();

    let result = project.init().unwrap();

    assert!(result.config_path.exists());
    assert!(result.runs_dir.exists());
    assert!(result.keel_dir.join(WORKTREES_DIR).exists());
    let config = fs::read_to_string(result.config_path).unwrap();
    assert!(config.contains("agent_timeout_secs"));
}
