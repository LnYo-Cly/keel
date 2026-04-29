# Keel

Keel is a local-first control layer for AI-generated code.

## Commands

```bash
keel init
keel run "test noop run" --agent noop
keel run "implement a small change" --agent codex
keel status
keel report <run-id>
keel rerun <run-id>
keel discard <run-id>
```

Keel creates a candidate worktree, captures logs, diffs, checks, and a report,
then leaves the merge decision to the human developer. The `noop` agent is a
local smoke-test adapter; the `codex` adapter shells out to `codex exec` without
automatic merge, push, or dangerous approval bypass flags.

`keel rerun <run-id>` creates a fresh candidate run with the same task and
agent. It preserves the source run history and does not reuse the old worktree.

## Local config

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

Timed-out agent runs are marked `not_ready`; Keel still writes metadata, logs,
diff, checks, and report artifacts.

## Smoke tests

Default regression does not require a real Codex installation:

```bash
cargo test --workspace
```

Real Codex smoke tests are opt-in because they depend on local Codex
installation, authentication, network access, and external model behavior:

```bash
KEEL_REAL_CODEX_SMOKE=1 cargo test -p keel-core real_codex_rerun_smoke_is_opt_in -- --nocapture
powershell -ExecutionPolicy Bypass -File scripts/real-codex-rerun-smoke.ps1
```
