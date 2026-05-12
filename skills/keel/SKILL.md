---
name: keel
description: Use Keel as the local-first workflow ledger and candidate-change boundary for coding-agent work. Trigger in Keel-enabled repositories, when asked to self-dogfood Keel, when managing long Codex/Claude/OpenCode sessions, when recording checkpoints, notes, evidence, review, verify, handoff, or when deciding the next safe command with `keel`.
---

# Keel

Use Keel to keep long coding-agent work recoverable, reviewable, and
evidence-backed. Keel does not replace Codex, Claude Code, or OpenCode; it
records what the current agent session did and keeps AI-generated code as a
candidate change until a human decides what to merge.

## Operating Loop

When working in a git repository that should use Keel:

1. If `.keel/config.toml` is missing, run `keel init`.
2. Start non-trivial coding, debugging, review, or refactor work with:

```bash
keel task start "<short task title>"
```

3. Record meaningful progress:

```bash
keel checkpoint "implemented parser changes"
keel note "risk: CLI output changed; update integration tests"
```

4. Record real verification as evidence. Prefer the configured check runner:

```bash
keel check
```

Use `keel evidence add --cmd "<command>"` only for one-off evidence that is not
covered by `.keel/config.toml`.

5. When unsure what to do next, run:

```bash
keel
```

Use `keel` as the daily-driver command. It combines the active ledger task with
the newest actionable candidate run and suggests the next useful command.
`keel next` is the explicit form of the same workflow guidance, and
`keel next --json` is the automation interface.

6. Before final response or handoff, run:

```bash
keel review
keel verify
```

Use `keel handoff` when the work may continue later.

7. If the work is complete, run:

```bash
keel task finish
```

## Evidence Rules

- Do not invent passing evidence.
- If evidence fails, fix the issue and record fresh passing evidence.
- `keel verify` can return ready after later evidence passes, even if older
  failed evidence remains in history.
- On Windows, prefer Keel's `--env` flag over shell-specific environment
  syntax when evidence needs isolated environment variables:

```bash
keel evidence add --env CARGO_TARGET_DIR=target/keel-evidence --cmd "cargo test --workspace"
```

## Candidate Runs

Use candidate-run commands when asking Keel to run or inspect an isolated agent
result:

```bash
keel run "<task>" --agent noop
keel run "<task>" --agent codex
keel run "<task>" --agent claude
keel run "<task>" --agent opencode
keel status
keel report <run-id>
keel diff <run-id>
keel log <run-id>
```

Empty, obsolete, or validation-only candidate runs should be discarded rather
than committed:

```bash
keel discard <run-id>
```

Only use Git workflow commands after reviewing the run and when the user intent
allows it:

```bash
keel commit <run-id> --dry-run
keel commit <run-id>
keel push <run-id> --dry-run
keel push <run-id>
keel pr <run-id> --manual --dry-run
keel pr <run-id> --provider github --dry-run
```

Do not push or create provider PRs unless the user explicitly scoped that work.

## Closeout

For substantial work, final answers should mention:

- what changed
- files changed
- tests or evidence run
- `keel verify` decision
- remaining risks

Use `keel review`, `keel verify`, and `keel` output to keep this grounded.

## Safety

- Do not use Keel to auto merge.
- Do not push unless the user explicitly requests push or publish work.
- Do not create PRs unless the user explicitly requests PR automation.
- Keep destructive actions limited to Keel-owned candidate worktrees.
- Treat `.keel/`, `AGENTS.md`, local harness docs, `target/`, and local output
  directories as local-only when the repo says they are ignored.
- Never hide failed checks behind a ready status; record fresh passing evidence
  after fixing failures.
