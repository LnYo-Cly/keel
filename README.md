# Keel

Keel is a local-first control layer for AI-generated code.

Keel runs coding agents in isolated git worktrees, captures their logs, diffs,
exit status, checks, and reports, then leaves the final decision to the human
developer. Agent output is treated as a candidate change, not as something to
merge automatically.

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
keel rerun <run-id>
keel discard <run-id>
```

Useful review commands:

```bash
keel doctor
keel status --agent noop
keel status --status ready
keel status --limit 5
keel status --json
keel report <run-id> --json
```

`keel doctor` checks git, Keel's local `.keel/` layout, and optional agent
CLIs. It is read-only: it does not initialize, fix, install, merge, or push.

## Supported Agents

- `noop`: local smoke-test adapter that writes a sample candidate file.
- `codex`: runs Codex CLI in a candidate worktree.
- `claude`: runs Claude Code in non-interactive print mode.
- `opencode`: runs OpenCode in a candidate worktree.

Real agent runs depend on the corresponding CLI being installed and available
on `PATH`.

## Safety Model

- Every run executes in its own isolated git worktree under `.keel/worktrees/`.
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

Discarding a run removes only the candidate worktree and keeps these artifacts
for later review.

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
```

Timed-out or failed agent runs are marked `not_ready`; Keel still writes
metadata, logs, diff, checks, and report artifacts when possible.

## Roadmap

- v0.4: doctor, config validation, and risk path warnings.
- v0.5: draft GitHub PR creation.
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
