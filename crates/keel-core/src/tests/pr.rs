use super::*;

#[test]
fn pr_manual_dry_run_rejects_invalid_run_states() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();

    let missing = project
        .pr_plan("run-does-not-exist", pr_options(None))
        .unwrap_err()
        .to_string();
    assert!(missing.contains("run `run-does-not-exist` does not exist"));

    let uncommitted = project.run("pr uncommitted", "noop").unwrap();
    let error = project
        .pr_plan(&uncommitted.run_id, pr_options(None))
        .unwrap_err()
        .to_string();
    assert!(error.contains("is not committed"));

    project
        .commit(
            &uncommitted.run_id,
            CommitOptions {
                dry_run: false,
                message: None,
            },
        )
        .unwrap();
    let error = project
        .pr_plan(&uncommitted.run_id, pr_options(None))
        .unwrap_err()
        .to_string();
    assert!(error.contains("is not pushed"));

    let discarded = project.discard(&uncommitted.run_id).unwrap();
    let error = project
        .pr_plan(&discarded.run_id, pr_options(None))
        .unwrap_err()
        .to_string();
    assert!(error.contains("only ready runs can create a PR/MR plan"));
}

#[test]
fn pr_manual_dry_run_builds_plan_from_pushed_metadata() {
    let temp = git_repo();
    let remote = bare_git_repo();
    add_origin(&temp, &remote);
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("manual pr plan", "noop").unwrap();
    let commit = project
        .commit(
            &metadata.run_id,
            CommitOptions {
                dry_run: false,
                message: None,
            },
        )
        .unwrap();
    project.push(&metadata.run_id, push_options(false)).unwrap();
    let before_metadata = read_run_file(&temp, &metadata.run_id, METADATA_FILE);
    let before_report = read_run_file(&temp, &metadata.run_id, REPORT_FILE);

    let plan = project
        .pr_plan(&metadata.run_id, pr_options(Some(PrProvider::Github)))
        .unwrap();

    assert_eq!(plan.provider, PrProvider::Github);
    assert_eq!(plan.request_kind, "pull_request");
    assert_eq!(plan.source_branch, metadata.branch);
    assert_eq!(
        plan.target_branch,
        git_stdout(temp.path(), &["branch", "--show-current"])
    );
    assert_eq!(plan.commit_sha, commit.commit_sha.unwrap());
    assert_eq!(plan.title, "keel: manual pr plan");
    assert!(plan.body.contains(&metadata.run_id));
    assert!(plan.body.contains("## Keel Candidate Change"));
    assert!(plan.body.contains("## Warnings"));
    assert!(plan.body.contains("None"));
    assert!(plan.body.contains("## Artifacts"));
    assert!(plan.body.contains(METADATA_FILE));
    assert!(plan.body.contains(LOG_FILE));
    assert!(plan.body.contains(DIFF_FILE));
    assert!(plan.body.contains(CHECKS_FILE));
    assert!(plan.body.contains(REPORT_FILE));
    assert!(plan.body.contains(COMMIT_FILE));
    assert!(plan.body.contains(PUSH_FILE));
    assert!(plan
        .body
        .contains("Keel did not merge this candidate change"));
    assert!(plan.copyable_summary.contains(&metadata.run_id));
    assert!(plan.copyable_summary.contains("manual pr plan"));
    assert!(plan.artifacts.metadata.ends_with(METADATA_FILE));
    assert!(plan
        .artifacts
        .commit
        .as_deref()
        .unwrap()
        .ends_with(COMMIT_FILE));
    assert!(plan.artifacts.push.as_deref().unwrap().ends_with(PUSH_FILE));
    assert!(plan.web_url.is_none());
    assert!(!plan.would_create_request);
    assert!(!plan.would_write_artifact);
    assert!(!plan.would_push);
    assert!(!plan.would_merge);
    assert!(!run_dir(&temp, &metadata.run_id).join("pr.json").exists());
    assert_eq!(
        read_run_file(&temp, &metadata.run_id, METADATA_FILE),
        before_metadata
    );
    assert_eq!(
        read_run_file(&temp, &metadata.run_id, REPORT_FILE),
        before_report
    );
}

#[test]
fn pr_manual_dry_run_infers_provider_from_remote_url() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("provider inference", "noop").unwrap();
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
    pushed.push_remote_url = Some("git@gitlab.com:owner/repo.git".to_string());
    pushed.pushed_branch = Some(pushed.branch.clone());
    pushed.push = None;
    project.write_metadata(&pushed).unwrap();

    let plan = project.pr_plan(&metadata.run_id, pr_options(None)).unwrap();

    assert_eq!(plan.provider, PrProvider::Gitlab);
    assert_eq!(plan.request_kind, "merge_request");
    assert_eq!(
        plan.repository_url.as_deref(),
        Some("https://gitlab.com/owner/repo")
    );
    assert!(plan
        .web_url
        .as_deref()
        .unwrap()
        .starts_with("https://gitlab.com/owner/repo/-/merge_requests/new?"));
    assert!(plan
        .web_url
        .as_deref()
        .unwrap()
        .contains("merge_request[source_branch]=keel%2Frun%2F"));
    assert!(plan
        .manual_steps
        .iter()
        .any(|step| step.contains("https://gitlab.com/owner/repo/-/merge_requests/new")));
}

#[test]
fn pr_manual_dry_run_builds_github_web_url_with_overrides() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("github web url", "noop").unwrap();
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
    pushed.push = None;
    project.write_metadata(&pushed).unwrap();

    let plan = project
        .pr_plan(
            &metadata.run_id,
            PrOptions {
                manual: true,
                dry_run: true,
                draft: true,
                provider: None,
                base: Some("release/v1".to_string()),
                head: None,
                target: Some("release/v1".to_string()),
                title: Some("custom title".to_string()),
            },
        )
        .unwrap();

    let web_url = plan.web_url.as_deref().unwrap();
    assert_eq!(plan.provider, PrProvider::Github);
    assert!(web_url.starts_with("https://github.com/owner/repo/compare/"));
    assert!(web_url.contains("release%2Fv1...keel%2Frun%2F"));
    assert!(web_url.contains("title=custom%20title"));
    assert!(web_url.contains("body=%23%23%20Keel%20Candidate%20Change"));
}

#[test]
fn pr_manual_dry_run_body_includes_warning_summary() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    write_config(
        &temp,
        r#"version = 1
runs_dir = "runs"
worktrees_dir = "worktrees"

[[checks]]
name = "git status"
command = ["git", "status", "--short"]

[risk]
paths = ["keel-noop-output.txt"]
"#,
    );
    let metadata = project.run("warning pr body", "noop").unwrap();
    assert!(!metadata.warnings.is_empty());
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
    pushed.push = None;
    project.write_metadata(&pushed).unwrap();

    let plan = project
        .pr_plan(&metadata.run_id, pr_options(Some(PrProvider::Github)))
        .unwrap();

    assert!(plan.body.contains("touched risk path"));
    assert!(!plan.body.contains("\nNone\n"));
}

#[test]
fn pr_manual_dry_run_rejects_unknown_provider_without_override() {
    let temp = git_repo();
    let remote = bare_git_repo();
    add_origin(&temp, &remote);
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("unknown provider", "noop").unwrap();
    project
        .commit(
            &metadata.run_id,
            CommitOptions {
                dry_run: false,
                message: None,
            },
        )
        .unwrap();
    project.push(&metadata.run_id, push_options(false)).unwrap();

    let error = project
        .pr_plan(&metadata.run_id, pr_options(None))
        .unwrap_err()
        .to_string();

    assert!(error.contains("could not infer PR provider"));
}

#[test]
fn pr_manual_dry_run_requires_manual_and_dry_run_flags() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("pr mode", "noop").unwrap();

    let error = project
        .pr_plan(
            &metadata.run_id,
            PrOptions {
                manual: false,
                dry_run: true,
                draft: true,
                provider: Some(PrProvider::Github),
                base: None,
                head: None,
                target: None,
                title: None,
            },
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("--manual --dry-run"));
}

#[test]
fn pr_provider_dry_run_builds_creation_plan_without_writing_artifact() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("provider pr dry run", "noop").unwrap();
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
    pushed.push = None;
    project.write_metadata(&pushed).unwrap();

    let result = project
        .pr(
            &metadata.run_id,
            PrOptions {
                manual: false,
                dry_run: true,
                draft: true,
                provider: Some(PrProvider::Github),
                base: Some("main".to_string()),
                head: None,
                target: None,
                title: Some("custom pr title".to_string()),
            },
        )
        .unwrap();

    assert!(!result.created);
    assert!(result.dry_run);
    assert_eq!(result.provider, PrProvider::Github);
    assert_eq!(result.provider_command[0], "gh");
    assert!(result.provider_command.iter().any(|arg| arg == "--draft"));
    assert!(result
        .provider_command
        .windows(2)
        .any(|pair| pair == ["--repo", "owner/repo"]));
    let body_index = result
        .provider_command
        .iter()
        .position(|arg| arg == "--body")
        .unwrap()
        + 1;
    assert!(result.provider_command[body_index].contains("## Keel Candidate Change"));
    assert!(result.provider_command[body_index].contains("\n## Artifacts\n"));
    assert!(result
        .provider_command_display
        .contains("<generated PR body>"));
    assert!(!result
        .provider_command_display
        .contains("Keel Candidate Change"));
    assert!(result.would_create_request);
    assert!(!result.would_write_artifact);
    assert!(!result.would_push);
    assert!(!result.would_merge);
    assert!(!run_dir(&temp, &metadata.run_id).join("pr.json").exists());
}

#[test]
fn pr_provider_rejects_unsupported_gitee_creation() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("unsupported provider pr", "noop").unwrap();
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
    pushed.push_remote_url = Some("git@gitee.com:owner/repo.git".to_string());
    pushed.pushed_branch = Some(pushed.branch.clone());
    pushed.push = None;
    project.write_metadata(&pushed).unwrap();

    let error = project
        .pr(
            &metadata.run_id,
            PrOptions {
                manual: false,
                dry_run: true,
                draft: true,
                provider: Some(PrProvider::Gitee),
                base: None,
                head: None,
                target: None,
                title: None,
            },
        )
        .unwrap_err()
        .to_string();

    assert!(error.contains("provider-backed PR/MR creation for Gitee is not implemented in v0.5c"));
}

#[test]
fn pr_legacy_metadata_is_used_by_report_and_json_views() {
    let temp = git_repo();
    let project = KeelProject::discover(temp.path()).unwrap();
    project.init().unwrap();
    let metadata = project.run("legacy pr metadata", "noop").unwrap();
    project
        .commit(
            &metadata.run_id,
            CommitOptions {
                dry_run: false,
                message: None,
            },
        )
        .unwrap();

    let mut legacy = read_metadata(&temp, &metadata.run_id);
    legacy.pushed = true;
    legacy.pushed_at = Some("2026-04-30T00:00:00Z".to_string());
    legacy.push_remote = Some("origin".to_string());
    legacy.push_remote_url = Some("git@github.com:owner/repo.git".to_string());
    legacy.pushed_branch = Some(legacy.branch.clone());
    legacy.push = None;
    legacy.pr_created = true;
    legacy.pr_created_at = Some("2026-04-30T01:00:00Z".to_string());
    legacy.pr_provider = Some("github".to_string());
    legacy.pr_url = Some("https://github.com/owner/repo/pull/1".to_string());
    legacy.pr_target_branch = Some("main".to_string());
    legacy.pr_source_branch = Some(legacy.branch.clone());
    legacy.pr = None;
    project.write_metadata(&legacy).unwrap();

    let report = project.report(&metadata.run_id).unwrap();
    let json = serde_json::to_value(report_json(&report)).unwrap();

    assert_eq!(json["pr"]["provider"], "github");
    assert_eq!(
        json["pr"]["repository_url"],
        "https://github.com/owner/repo"
    );
    assert_eq!(json["pr"]["url"], "https://github.com/owner/repo/pull/1");
    assert!(json["artifacts"]["pr"]["path"]
        .as_str()
        .unwrap()
        .ends_with(PR_FILE));
}
