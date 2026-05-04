# Keel

Keel is a local-first control layer for AI-generated code.

Keel is Git-native, not GitHub-native.

Keel runs coding agents in isolated git worktrees, captures their logs, diffs,
exit status, checks, risk warnings, and reports, then leaves the final decision
to the human developer. Agent output is treated as a candidate change, not as
something to merge automatically.

## What Keel Is Not

- Not a coding agent replacement.
- Not a desktop app or Web UI.
- Not a TUI-first product; the TUI is a read-only review surface over local
  artifacts.
- Not a cloud service.
- Not an automatic merge or push tool.
- Not tied to one specific agent or harness.

## Quickstart

Run Keel inside an existing git repository with at least one commit.

```bash
keel init
keel task start "implement review workflow"
keel task status
keel task show <task-id>
keel checkpoint "planned CLI changes"
keel evidence add --cmd "cargo test --workspace"
keel review
keel handoff
keel task finish
keel task reopen <task-id>
keel run "fix login bug" --agent noop
keel status
keel report <run-id>
keel diff <run-id>
keel log <run-id>
keel
keel commit <run-id> --dry-run
keel commit <run-id>
keel push <run-id> --dry-run
keel push <run-id>
keel pr <run-id> --manual --dry-run --provider github
keel pr <run-id> --provider github --dry-run
keel pr <run-id> --provider github
keel rerun <run-id>
keel discard <run-id>
```

Useful review commands:

```bash
keel doctor
keel config validate
keel config validate --json
keel verify
keel review --json
keel handoff --json
keel status --agent noop
keel status --status ready
keel status --limit 5
keel status --json
keel report <run-id> --json
keel
keel --run <run-id>
keel tui
keel tui --run <run-id>
keel tui --agent noop --status ready
keel commit <run-id> --json
keel push <run-id> --json
keel pr <run-id> --manual --dry-run --provider github --json
keel pr <run-id> --provider github --json
```

`keel doctor` checks git, Keel's local `.keel/` layout, and optional agent
CLIs. It is read-only: it does not initialize, fix, install, merge, or push.

`keel config validate` checks `.keel/config.toml` for presence, parseability,
and basic value sanity, including risk warning settings. It does not rewrite the
file.

`keel task start`, `keel checkpoint`, `keel note`, `keel evidence add`,
`keel verify`, `keel review`, `keel handoff`, `keel task status`,
`keel task show`, `keel task reopen`, and `keel task finish` provide a
lightweight workspace ledger for long-running agent sessions. This mode does not
start a new agent and does not create a worktree; it lets the current Codex or
Claude Code session record checkpoints, evidence, handoff state, and review
readiness while working in the current repository.

`keel` opens a read-only terminal review UI for browsing runs and artifacts.
`keel tui` is the explicit form of the same UI. It does not commit, push,
discard, create PRs, merge, or modify run history.

## Supported Agents

- `noop`: local smoke-test adapter that writes a sample candidate file.
- `codex`: runs Codex CLI in a candidate worktree.
- `claude`: runs Claude Code in non-interactive print mode.
- `opencode`: runs OpenCode in a candidate worktree.

Real agent runs depend on the corresponding CLI being installed and available
on `PATH`.

## Safety Model

- Every run executes in its own isolated git worktree under `.keel/worktrees/`.
- The workspace ledger records current-session task progress under
  `.keel/ledger/` and does not modify source files.
- `keel commit <run-id>` commits only inside the candidate worktree on the
  candidate branch.
- Local commit does not require a remote, GitHub, GitLab, or Gitee.
- `keel push <run-id>` pushes only the candidate branch to the selected Git
  remote.
- Push is generic Git push, not provider-specific PR/MR creation.
- `keel pr <run-id> --manual --dry-run` only prints a manual PR/MR plan.
- `keel pr <run-id> --provider github` creates a GitHub Pull Request through
  the installed `gh` CLI after the run has already been pushed.
- Keel does not auto merge.
- Keel does not auto push.
- Keel preserves run history under `.keel/runs/`.
- The TUI is read-only and renders existing `.keel/runs/<run-id>/` artifacts.
- A human developer is always the final merge decision maker.

`keel report <run-id>` is the command-line review hub. Its suggested next
actions follow the candidate lifecycle:

- ready but uncommitted: inspect `diff`/`log`, then `keel commit --dry-run` and
  `keel commit`
- committed: `keel push --dry-run` and `keel push`
- pushed: `keel pr --manual --dry-run`; GitHub remotes also show
  `keel pr --provider github --dry-run` and `keel pr --provider github`
- PR/MR created: review the provider request; Keel still does not merge
- not ready or discarded: inspect artifacts, rerun, or preserve history

## Artifacts

Each run stores review artifacts under `.keel/runs/<run-id>/`:

- `metadata.json`
- `log.txt`
- `diff.patch`
- `checks.json`
- `report.md`
- `commit.json` after `keel commit <run-id>` succeeds
- `push.json` after `keel push <run-id>` succeeds
- `pr.json` after provider-backed `keel pr <run-id>` succeeds

Discarding a run removes only the candidate worktree and keeps these artifacts
for later review.

Workspace ledger tasks store current-session records under
`.keel/ledger/tasks/<task-id>/task.json`:

- task title and status
- checkpoints
- notes
- evidence command results with exit code and captured output
- review and handoff summaries generated from the task ledger
- workspace context from `git status --short` and `git diff --stat`

The ledger is intended for agent self-management. A long-running Codex or Claude
Code session can call these commands while it works:

```bash
keel task start "implement Keel self-dogfood ledger"
keel task status
keel task show <task-id>
keel checkpoint "core model added"
keel note "risk: CLI output changed"
keel evidence add --cmd "cargo fmt --all --check"
keel evidence add --env CARGO_TARGET_DIR=target/keel-evidence --cmd "cargo test --workspace"
keel verify
keel review
keel handoff
keel review <task-id>
keel verify <task-id>
keel task finish
keel task reopen <task-id>
```

`keel evidence add --env KEY=VALUE --cmd "<command>"` sets environment variables
only for that evidence command. This is useful for isolated Rust target
directories, temporary caches, or other deterministic verification settings.

`keel verify` exits non-zero if the active task has no evidence or if the latest
evidence window is still failing. Historical failed evidence stays in the ledger,
but later passing evidence can restore readiness after a fix. `keel review` and
`keel handoff` also include a review packet with a readiness headline, changed
files grouped by source/tests/docs/config/other, latest and failed evidence, and
suggested closeout commands. These commands do not merge, push, or mutate source
files. After a task is finished, `keel review <task-id>`, `keel verify
<task-id>`, and `keel handoff <task-id>` can read the preserved task directly
without reopening it. Preserved-task review intentionally omits the current
workspace `git status` / `git diff --stat` context, because those live files may
belong to unrelated later work; reopen the task if you want live workspace
context again.

`keel task status` shows the active ledger task and recent task summaries.
Its JSON output is intentionally compact, so evidence stdout/stderr stays out
of quick status checks. `keel review --json`, `keel verify --json`, and
`keel handoff --json` also return compact evidence summaries for automation,
and the human review/handoff output avoids replaying full evidence history.
`keel task show <task-id>` reads the full preserved task history, including
checkpoints, notes, and evidence, even after a task is finished or superseded.
Starting a new task marks the previously active task superseded. `keel task
finish` marks the active task finished and clears it as active; the task history
remains under `.keel/ledger/tasks/`. `keel task reopen <task-id>` makes a
preserved task active again.

## Terminal Review UI

`keel` provides a local read-only review view over existing run artifacts. The
explicit `keel tui` command opens the same UI. It is built with `ratatui` and
Crossterm.

```bash
keel
keel --run <run-id>
keel tui
keel tui --run <run-id>
keel tui --filter not_ready
keel tui --agent noop
keel tui --status not_ready
keel tui --agent codex --status ready
```

The TUI is review-only. Write actions stay in the CLI so the terminal UI cannot
commit, push, create a PR/MR, discard a run, merge, or rewrite artifacts.

Current TUI behavior:

- Lists runs newest first.
- Keeps the selected run visible when the run list is longer than the terminal.
- Shows run position in the list title, for example `Runs (3/12, newest first)`.
- Supports `--filter <text>`, `--agent <agent>`, and `--status <status>` at
  startup to open directly on matching runs. These filters can be combined.
- Shows status counts for ready, not ready, running, discarded, committed,
  pushed, and PR/MR-created runs.
- Uses a review queue layout with compact `Queue` and `Next` columns so the
  review state and next CLI action are visible before lower-level details.
- Shows the selected run's report summary, review progress, checks, warnings,
  suggested next CLI action, diff, log, and artifact paths.
- Separates review progress into commit, push, and PR/MR state so the TUI shows
  what has happened without executing write actions.
- Marks diff/log tabs as present, empty, or missing, and marks artifacts when
  required review files are missing.
- Colors git diff file headers, hunks, additions, deletions, and metadata lines.
- Shows scroll position in long detail panels, for example `Diff (16-30/120)`.

TUI shortcuts:

| Shortcut | Action |
| --- | --- |
| `j` / `Down` | Select next run |
| `k` / `Up` | Select previous run |
| `g` / `G` | Jump to first or last visible run |
| `1` / `2` / `3` / `4` | Open report, diff, log, or artifacts |
| `Tab` | Switch to the next detail tab |
| `Shift+Tab` | Switch to the previous detail tab |
| `PgUp` / `PgDn` | Scroll the current detail tab |
| `Home` / `End` | Jump to top or bottom of the current detail tab |
| `/` | Filter runs by id, task, agent, status, branch, warning, commit, push, or PR metadata |
| `r` | Refresh run list and selected artifacts |
| `?` | Show the read-only help overlay |
| `q` / `Esc` / `Ctrl-C` | Quit |

TUI safety boundary:

- Does not execute agents.
- Does not commit.
- Does not push.
- Does not create PRs/MRs.
- Does not discard.
- Does not merge.
- Does not rewrite `.keel/` artifacts.

## Artifact And JSON Contract

Keel writes new v0.5 runs with push/pr naming:

- `metadata.json` uses `pushed`, `pushed_at`, `push_remote`,
  `push_remote_url`, `pushed_branch`, and `push`.
- `push.json` is the push artifact written by `keel push <run-id>`.
- `pr.json` is written only after provider-backed `keel pr <run-id>` succeeds.
- `keel status --json` returns an array of run summaries.
- `keel report <run-id> --json` returns a review summary with `commit`,
  `push`, `pr`, `artifacts`, `warnings`, `risk_warnings`, and `next_actions`.
- Missing artifacts are represented as `state: "missing"` instead of causing
  report JSON rendering to fail.
- Artifact JSON includes `required: true` for core review artifacts
  (`metadata`, `log`, `diff`, `checks`, `report`) and `required: false` for
  optional Git workflow artifacts (`commit`, `push`, `pr`).

Compatibility policy:

- Keel no longer exposes `keel publish`.
- New runs do not write `publish.json` or `published*` metadata fields.
- Read paths still understand legacy `publish.json`, `published`,
  `published_at`, `publish_remote`, `publish_remote_url`,
  `published_branch`, and `publish` so older local run history remains
  reviewable.
- There is no migration command yet. Legacy artifacts are read in place and are
  not rewritten unless a future explicit migration command is added.

## Local Commit Workflow

`keel commit <run-id>` turns a ready candidate change into a local Git commit on
that run's candidate branch.

```bash
keel commit <run-id> --dry-run
keel commit <run-id>
keel commit <run-id> --message "keel: fix login validation"
keel commit <run-id> --json
```

Commit behavior:

- Requires the run status to be `ready`.
- Requires the candidate worktree and non-empty saved diff to exist.
- Runs `git add -A` and `git commit -m ...` only inside
  `.keel/worktrees/<run-id>`.
- Writes `.keel/runs/<run-id>/commit.json`.
- Updates `metadata.json` and `report.md` with the commit summary.
- Does not push.
- Does not merge.
- Does not require GitHub, GitLab, Gitee, or any remote.

Risk warnings do not block local commit. They remain advisory review signals and
are copied into `commit.json` and the report.

## Generic Git Push Workflow

`keel push <run-id>` pushes an already committed ready candidate branch to a
Git remote.

```bash
keel push <run-id> --dry-run
keel push <run-id>
keel push <run-id> --remote origin
keel push <run-id> --json
```

Push behavior:

- Requires the run status to be `ready`.
- Requires the run to have a local commit from `keel commit <run-id>`.
- Defaults to `origin`; use `--remote <remote>` for another Git remote.
- Runs `git push -u <remote> <candidate-branch>`.
- Writes `.keel/runs/<run-id>/push.json`.
- Updates `metadata.json` and `report.md` with the push summary.
- Does not create a PR or MR.
- Does not merge.
- Does not push `main`, `master`, tags, or all branches.
- Does not require GitHub, GitLab, Gitee, or any provider API.

The remote can be GitHub, GitLab, Gitee, Gitea, a self-hosted Git service, or a
bare Git repository. If a repository has no remote, Keel can still complete the
local commit workflow; push is optional.

## PR/MR Workflow

Keel uses `pr` as the generic command name for creating a code review / merge
request. On GitHub, Gitee, and Gitea it maps to Pull Request language; on
GitLab it maps to Merge Request language.

Manual mode is still available and read-only:

```bash
keel pr <run-id> --manual --dry-run --provider github
keel pr <run-id> --manual --dry-run --provider gitlab
keel pr <run-id> --manual --dry-run --json
```

Manual plan behavior:

- Requires the run to be ready, committed, and pushed.
- Infers the provider from common remote hosts when possible.
- Supports explicit providers: `github`, `gitlab`, `gitee`, and `gitea`.
- Prints source branch, target branch, commit, title, body, a provider web URL
  when derivable, and manual next steps.
- JSON output includes `web_url` and `manual_steps` for automation-friendly
  manual workflows.
- The generated body is copyable Markdown with run id, agent, task, source and
  target branches, commit SHA, readiness summary, warnings, artifact paths, and
  safety notes.
- JSON output also includes `copyable_summary` for compact handoff text.
- Does not call GitHub, GitLab, Gitee, or Gitea APIs.
- Does not call provider CLIs.
- Does not write `pr.json`.
- Does not merge anything.

The generated web URL is a best-effort browser link:

- GitHub, Gitee, and Gitea use a compare-style Pull Request URL.
- GitLab uses a new Merge Request URL with source and target branches filled.
- Local bare remotes and unknown self-hosted URLs may not produce a `web_url`;
  pass `--provider` and use the printed manual steps.

Provider-backed mode is available for GitHub through the local `gh` CLI:

```bash
keel pr <run-id> --provider github --dry-run
keel pr <run-id> --provider github
keel pr <run-id> --provider github --draft
keel pr <run-id> --provider github --base main
keel pr <run-id> --provider github --head owner:feature-branch
keel pr <run-id> --provider github --json
```

Provider-backed behavior:

- Requires the run to be ready, committed, and pushed.
- Uses `gh pr create` for GitHub.
- Checks for an existing open GitHub PR with the same head/base before
  creating a new one. If one exists, Keel reuses it, writes `pr.json`, and does
  not create a duplicate request.
- Supports `--draft` for creating a Draft PR.
- Passes generated title/body with `--title` and `--body`.
- Supports `--base <branch>` for the target branch.
- Supports `--head <branch>` for the source branch. This is useful for
  explicit GitHub head refs such as `owner:feature-branch`.
- Passes `--head` by default using Keel's pushed candidate branch, so the
  GitHub CLI does not prompt Keel to push or fork from the PR command.
- Writes `.keel/runs/<run-id>/pr.json` only after successful creation.
- Updates `metadata.json` and `report.md` with the PR/MR URL.
- Is idempotent when `pr.json` or PR metadata already exists.
- Normalizes common `gh` authentication, permission, and inaccessible
  repository errors into clearer CLI messages.
- Does not call `git push`.
- Does not call `git merge`.
- Does not auto merge.
- Does not support provider-backed GitLab, Gitee, or Gitea creation yet; use
  manual mode for those providers.

Future command shape for additional provider-backed creation:

```bash
keel pr <run-id> --provider gitlab
keel pr <run-id> --provider gitee
keel pr <run-id> --provider gitea
```

All provider-backed `keel pr` workflows require the run to be pushed first.
They do not auto merge.

## Risk Warnings

Keel analyzes each saved `diff.patch` and adds non-blocking warnings for changes
that deserve closer human review:

- Configured risk paths from `[risk].paths`
- Dependency manifests such as `Cargo.toml`, `package.json`, `pyproject.toml`,
  and `requirements.txt`
- Lockfiles such as `Cargo.lock`, `package-lock.json`, `pnpm-lock.yaml`,
  `yarn.lock`, and `uv.lock`
- Deleted files
- Large diffs whose changed file count exceeds
  `risk.large_diff_file_threshold`

Risk warnings are informational. They do not block `ready` when the agent exits
successfully, the diff is non-empty when required, and checks pass. Keel still
does not auto merge or auto push; warnings are there to help the human reviewer
focus.

## Local Config

`keel init` creates `.keel/config.toml`. The default config includes:

```toml
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
```

Validation currently accepts this legacy layout and also understands the
future-facing validation fields:

```toml
[checks]
commands = []

[agents.codex]
enabled = true
timeout_seconds = 900

[agents.claude]
enabled = true
timeout_seconds = 900

[agents.opencode]
enabled = true
timeout_seconds = 900

[readiness]
require_non_empty_diff = true
require_checks_pass = true

[risk]
paths = ["src/auth/**", ".github/**"]
large_diff_file_threshold = 20
```

Timed-out or failed agent runs are marked `not_ready`; Keel still writes
metadata, logs, diff, checks, and report artifacts when possible.

## Roadmap

- v0.4: doctor, config validation, and risk path warnings.
- v0.5: local commit, generic Git push, and future PR/MR workflow.
  - `keel push`: push a candidate branch to any Git remote.
  - `keel pr --manual --dry-run`: print a manual PR/MR plan without provider
    API calls.
  - `keel pr --provider github`: create a GitHub PR through `gh`.
  - Future provider-backed GitLab/Gitee/Gitea support.
- v0.6: self-dogfood ledger for long-running agent sessions.
  - `keel task start`: start a current-workspace task ledger.
  - `keel task status`: show the active task and recent ledger tasks.
  - `keel task show <task-id>`: inspect preserved task history.
  - `keel task reopen <task-id>`: make a preserved task active again.
  - `keel task finish`: finish the active task without deleting history.
  - `keel checkpoint`: record meaningful implementation milestones.
  - `keel note`: record decisions, risks, and unresolved context.
  - `keel evidence add --cmd`: run verification commands and capture evidence.
  - `keel verify`: fail when evidence is missing or any recorded evidence failed.
  - `keel review`: summarize current task readiness and evidence.
  - `keel handoff`: produce a recovery packet for future sessions.
- v0.6.x: read-only TUI for reviewing candidate runs.
  - Current stack: `ratatui` with the Crossterm backend.
  - Current slice: run list, report, diff, log, artifacts, filtering, and
    diff/log scrolling.
  - Deferred: agent execution controls, commit, push, PR creation, merge, and
    destructive actions.
  - The TUI consumes existing `keel-core` models instead of duplicating CLI
    parsing or artifact logic.

## Development Smoke Tests

Default regression does not require real agent CLIs:

```bash
cargo test --workspace
```

Provider-backed GitHub PR regression has two evidence levels:

- Default CLI tests use a fake `gh` shim to verify Keel's command boundary
  without network access.
- Real smoke tests are opt-in and must use the real GitHub CLI,
  authentication, network access, and a writable test repository.
- Keel does not store GitHub tokens. Authentication is delegated to `gh`.
- Do not paste Personal Access Tokens into chat, shell history, issue trackers,
  or test logs. If a token is exposed, revoke it and create a new one.

Run a real GitHub PR smoke with a disposable writable repository:

```bash
KEEL_REAL_GITHUB_PR_SMOKE=1 \
KEEL_REAL_GITHUB_REMOTE=git@github.com:owner/test-repo.git \
KEEL_REAL_GITHUB_TARGET=main \
cargo test -p keel-cli real_github_pr_smoke_is_opt_in -- --nocapture

powershell -ExecutionPolicy Bypass -File scripts/real-provider-pr-smoke.ps1 \
  -Provider github \
  -Remote git@github.com:owner/test-repo.git \
  -Base main \
  -CloseRequest
```

These real smoke tests intentionally create a candidate branch and a GitHub
Pull Request. Use only disposable test repositories. The PowerShell helper
leaves the request open by default; pass `-CloseRequest` to close the request
after verifying `pr.json`.

Real smoke authentication notes:

- For `keel pr --provider github`, the token or `gh` login needs enough
  permission to create a branch and PR in the test repository.
- If the smoke creates a disposable repository that should be deleted
  automatically afterward, the token must also include GitHub's `delete_repo`
  scope.
- A token with repository admin permission but without `delete_repo` can create
  and close the PR but cannot delete the repository.
- Prefer `gh auth login` or a short-lived environment variable over placing
  tokens directly in commands.

Real Codex smoke tests are opt-in because they depend on local Codex
installation, authentication, network access, and external model behavior:

```bash
KEEL_REAL_CODEX_SMOKE=1 cargo test -p keel-core real_codex_rerun_smoke_is_opt_in -- --nocapture
powershell -ExecutionPolicy Bypass -File scripts/real-codex-rerun-smoke.ps1
```
