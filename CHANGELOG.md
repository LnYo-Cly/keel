# Changelog

All notable changes to Keel will be documented in this file.

The format is inspired by Keep a Changelog, and this project follows semantic
versioning once public release tags are cut.

## [Unreleased]

### Planned

- Provider-backed PR/MR creation beyond GitHub.
- Packaged binary distribution for Windows, macOS, and Linux.
- Crates.io publication after the CLI surface stabilizes further.

## [0.1.0] - 2026-05-17

### Added

- Local-first Rust workspace with `keel-core`, `keel-cli`, and `keel-tui`.
- Candidate run lifecycle using isolated git worktrees.
- Agent adapters for `noop`, Codex, Claude Code, and OpenCode.
- Review artifacts: `metadata.json`, `log.txt`, `diff.patch`, `checks.json`,
  and `report.md`.
- Workspace ledger commands for long-running coding-agent sessions:
  `task`, `checkpoint`, `note`, `check`, `evidence`, `next`, `review`,
  `verify`, and `handoff`.
- Human and JSON output for status, report, doctor, config validation, checks,
  review, verify, handoff, commit, push, and PR workflows.
- Risk warnings for configured risk paths, dependency manifests, lockfiles,
  deleted files, and large diffs.
- Local commit workflow with `commit.json`.
- Generic Git push workflow with `push.json`.
- Manual PR/MR planning and GitHub PR creation through the local `gh` CLI.
- Read-only terminal review UI built with Ratatui and Crossterm.
- Bundled Codex skill under `skills/keel`.
- English and Chinese README documentation.

### Safety

- Keel does not auto merge.
- Keel does not auto push.
- Keel treats agent output as candidate changes until a human reviews them.
- Destructive candidate cleanup is limited to Keel-owned worktrees.
