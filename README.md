# Keel

Keel is a local-first control layer for AI-generated code.

Keel is Git-native, not GitHub-native.

Keel runs coding agents in isolated git worktrees, captures their logs, diffs,
exit status, checks, risk warnings, and reports, then leaves the final decision
to the human developer. Agent output is treated as a candidate change, not as
something to merge automatically.

## What Keel Is Not

- Not a coding agent replacement.
- Not a desktop app, Web UI, or TUI.
- Not a cloud service.
- Not an automatic merge or push tool.
- Not tied to one specific agent or harness.

## Quickstart

Run Keel inside an existing git repository with at least one commit.

```bash
keel init
keel run "fix login bug" --agent noop
keel status
keel report <run-id>
keel diff <run-id>
keel log <run-id>
keel commit <run-id> --dry-run
keel commit <run-id>
keel rerun <run-id>
keel discard <run-id>
```

Useful review commands:

```bash
keel doctor
keel config validate
keel config validate --json
keel status --agent noop
keel status --status ready
keel status --limit 5
keel status --json
keel report <run-id> --json
keel commit <run-id> --json
```

`keel doctor` checks git, Keel's local `.keel/` layout, and optional agent
CLIs. It is read-only: it does not initialize, fix, install, merge, or push.

`keel config validate` checks `.keel/config.toml` for presence, parseability,
and basic value sanity, including risk warning settings. It does not rewrite the
file.

## Supported Agents

- `noop`: local smoke-test adapter that writes a sample candidate file.
- `codex`: runs Codex CLI in a candidate worktree.
- `claude`: runs Claude Code in non-interactive print mode.
- `opencode`: runs OpenCode in a candidate worktree.

Real agent runs depend on the corresponding CLI being installed and available
on `PATH`.

## Safety Model

- Every run executes in its own isolated git worktree under `.keel/worktrees/`.
- `keel commit <run-id>` commits only inside the candidate worktree on the
  candidate branch.
- Local commit does not require a remote, GitHub, GitLab, or Gitee.
- Keel does not auto merge.
- Keel does not auto push.
- Keel preserves run history under `.keel/runs/`.
- A human developer is always the final merge decision maker.

## Artifacts

Each run stores review artifacts under `.keel/runs/<run-id>/`:

- `metadata.json`
- `log.txt`
- `diff.patch`
- `checks.json`
- `report.md`
- `commit.json` after `keel commit <run-id>` succeeds

Discarding a run removes only the candidate worktree and keeps these artifacts
for later review.

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
- v0.5: local commit, future publish, and future request workflows.
  - `keel publish`: push a candidate branch to any Git remote.
  - `keel request`: create a PR/MR on GitHub, GitLab, or Gitee.
- v0.6: TUI for reviewing candidate runs.

## Development Smoke Tests

Default regression does not require real agent CLIs:

```bash
cargo test --workspace
```

Real Codex smoke tests are opt-in because they depend on local Codex
installation, authentication, network access, and external model behavior:

```bash
KEEL_REAL_CODEX_SMOKE=1 cargo test -p keel-core real_codex_rerun_smoke_is_opt_in -- --nocapture
powershell -ExecutionPolicy Bypass -File scripts/real-codex-rerun-smoke.ps1
```
