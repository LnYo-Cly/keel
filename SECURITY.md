# Security Policy

Keel works with git worktrees, local command execution, agent logs, diffs, and
optional provider CLIs. Please report security issues privately.

## Reporting A Vulnerability

Use GitHub private vulnerability reporting for this repository:

https://github.com/LnYo-Cly/keel/security/advisories/new

If that is not available to you, contact the repository owner through GitHub
before opening a public issue.

Please include:

- affected Keel version or commit
- operating system
- command or workflow involved
- whether a git worktree, provider CLI, token, or local credential was involved
- reproduction steps, if safe to share privately

## Scope

Security-sensitive areas include:

- destructive filesystem operations
- git branch, remote, push, and worktree handling
- agent command invocation
- log and artifact persistence
- provider CLI integration such as GitHub `gh`
- handling of secrets, tokens, environment variables, and local credentials

Keel should not store provider tokens. GitHub authentication is delegated to the
local `gh` CLI.
