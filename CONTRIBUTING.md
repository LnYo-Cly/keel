# Contributing

Thanks for helping improve Keel.

Keel is a local-first control layer for AI-generated code. Contributions should
keep the core guarantees intact: isolated candidate worktrees, durable review
artifacts, no automatic merge, and no automatic push unless a user explicitly
requests a push workflow.

## Development Setup

```bash
git clone https://github.com/LnYo-Cly/keel.git
cd keel
cargo test --workspace
```

Useful local commands:

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p keel-cli -- --help
```

## Recommended Keel Workflow

When working on Keel itself, use Keel as the local workflow ledger:

```bash
keel
keel task start "short task title"
keel checkpoint "meaningful milestone"
keel check
keel review
keel verify
keel task finish
```

Use candidate-run commands only when you intentionally want Keel to run an
isolated agent result:

```bash
keel run "task" --agent noop
keel report <run-id>
keel diff <run-id>
keel log <run-id>
```

## Pull Requests

Before opening a PR:

- Keep the change focused.
- Update README or changelog entries when user-facing behavior changes.
- Add or update tests for behavior changes.
- Run the local validation commands above.
- Mention any risk warnings or review-sensitive paths in the PR body.

Do not include local-only files such as `.keel/`, `AGENTS.md`, `docs/`,
`output/`, or `target/`.

## Safety Boundaries

- Do not add automatic merge behavior.
- Do not add automatic push behavior outside explicit push commands.
- Do not make core flows depend on GitHub, GitLab, Gitee, or any hosted
  provider.
- Keep destructive actions restricted to Keel-owned candidate worktrees.
