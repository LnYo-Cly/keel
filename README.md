# Keel

Keel is a local-first control layer for AI-generated code.

## Commands

```bash
keel init
keel run "test noop run" --agent noop
keel run "implement a small change" --agent codex
keel status
keel report <run-id>
keel discard <run-id>
```

Keel creates a candidate worktree, captures logs, diffs, checks, and a report,
then leaves the merge decision to the human developer. The `noop` agent is a
local smoke-test adapter; the `codex` adapter shells out to `codex exec` without
automatic merge, push, or dangerous approval bypass flags.
