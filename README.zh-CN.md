# Keel

[English README](README.md)

Keel 是一个 **local-first 的 AI 代码变更控制层**。

Keel 是 Git-native，不是 GitHub-native。

它不会替代 Codex、Claude Code 或 OpenCode。它的作用是把 coding agent
的工作过程、候选代码变更、日志、diff、检查结果、风险提示和收口报告记录下来，
让长程 agent 工作可以恢复、验证、审查、提交、推送，最后仍由人决定是否合并。

## Keel 解决什么问题

- 长程 agent session 中断后，可以知道做到哪一步。
- 不再只依赖 “AI 说完成了”，而是有命令证据和检查结果。
- agent 产出的代码先进入 candidate worktree，不直接污染主工作区。
- 可以保留 report、diff、log、checks、metadata 作为审查资料。
- 可以在本地完成 commit，也可以选择 push 到任意 Git remote。
- GitHub PR 只是可选发布层，不是核心依赖。

## Keel 不是什么

- 不是 Codex / Claude Code 的替代品。
- 不是聊天界面。
- 不是 TUI-first 产品；TUI 只是只读审查视图。
- 不是云服务。
- 不自动 merge。
- 不自动 push。
- 不强依赖 GitHub / GitLab / Gitee。
- 不要求每个任务都跑多个 agent 比较。

## 安装 Keel CLI

目前 Keel 从源码安装。

直接从 GitHub 安装：

```bash
cargo install --git https://github.com/LnYo-Cly/keel.git --package keel-cli --bin keel --locked
keel --help
```

从本地 clone 安装：

```bash
git clone https://github.com/LnYo-Cly/keel.git
cd keel
cargo install --path crates/keel-cli --bin keel --locked
keel --help
```

如果只是本地开发，不安装也可以：

```bash
cargo run -p keel-cli -- <command>
cargo run -p keel-cli -- doctor
cargo run -p keel-cli -- check
```

依赖：

- Rust stable toolchain
- Git，并且支持 `git worktree`
- 可选 agent CLI：`codex`、`claude`、`opencode`
- 可选 GitHub PR 自动化：已登录的 `gh` CLI

## 安装 Keel Skill

仓库里带了 Codex skill：

```text
skills/keel/SKILL.md
```

### npx 快速安装

```bash
npx --yes skills add https://github.com/LnYo-Cly/keel/tree/master/skills/keel --agent codex --global --yes
```

这条命令适合快速把仓库里的 Keel skill 安装到 Codex 可识别的位置。
如果你想自己控制安装位置或保持和仓库同步，再用下面的复制、Junction
或软链接方式。

安装后，Codex 在 Keel 项目或长程 coding-agent 任务里会更自然地调用：

```bash
keel
keel task start "..."
keel checkpoint "..."
keel check
keel review
keel verify
keel handoff
```

### Windows PowerShell：复制安装

```powershell
git clone https://github.com/LnYo-Cly/keel.git
New-Item -ItemType Directory -Force "$env:USERPROFILE\.codex\skills" | Out-Null
Copy-Item -Recurse -Force ".\keel\skills\keel" "$env:USERPROFILE\.codex\skills\keel"
```

### Windows PowerShell：Junction 安装

适合你想持续改 skill，并让 Codex 直接使用仓库里的版本。

```powershell
git clone https://github.com/LnYo-Cly/keel.git
New-Item -ItemType Directory -Force "$env:USERPROFILE\.codex\skills" | Out-Null
New-Item -ItemType Junction `
  -Path "$env:USERPROFILE\.codex\skills\keel" `
  -Target "$PWD\keel\skills\keel"
```

### macOS / Linux：复制安装

```bash
git clone https://github.com/LnYo-Cly/keel.git
mkdir -p ~/.codex/skills
cp -R keel/skills/keel ~/.codex/skills/keel
```

### macOS / Linux：软链接安装

```bash
git clone https://github.com/LnYo-Cly/keel.git
mkdir -p ~/.codex/skills
ln -s "$PWD/keel/skills/keel" ~/.codex/skills/keel
```

安装或更新 skill 后，重启 Codex 或打开一个新的 Codex session，让 skill
metadata 重新加载。

## 快速开始

在已有 Git 仓库中使用 Keel。仓库至少需要有一次 commit。

```bash
keel init
keel
```

`keel` 是默认日常入口。它会告诉你：

- 当前有没有 active task
- 当前是否缺少 evidence
- 工作区是否 dirty
- 最新可行动 candidate run 是什么
- 下一步最应该执行什么命令

## Agent Operating Protocol

这是推荐给 Codex / Claude Code 的默认工作协议。

1. 先运行：

```bash
keel
```

2. 非 trivial 任务启动 ledger：

```bash
keel task start "implement review workflow"
```

3. 过程中记录进展和决策：

```bash
keel checkpoint "planned CLI changes"
keel note "risk: CLI output changed"
```

4. 记录真实验证证据：

```bash
keel check
```

如果只是一次性命令，不在 `.keel/config.toml` 中：

```bash
keel evidence add --cmd "cargo test --workspace"
keel evidence add --env CARGO_TARGET_DIR=target/keel-evidence --cmd "cargo test --workspace"
```

5. 每个阶段后重新看下一步：

```bash
keel
keel next
keel next --json
```

6. 收口：

```bash
keel review
keel verify
keel handoff
keel task finish
```

如果 evidence 失败，不能只口头说修好了。必须修复后重新记录 passing evidence。

## Candidate Run 工作流

让 Keel 在隔离 worktree 里运行 agent：

```bash
keel run "fix login bug" --agent noop
keel run "fix login bug" --agent codex
keel run "fix login bug" --agent claude
keel run "fix login bug" --agent opencode
```

查看 candidate：

```bash
keel status
keel status --agent codex
keel status --status ready
keel status --limit 5
keel status --json
```

审查 candidate：

```bash
keel report <run-id>
keel report <run-id> --json
keel diff <run-id>
keel log <run-id>
```

如果 candidate 是空 diff、过期验证残留、noop 输出残留，应该 discard，而不是 commit：

```bash
keel discard <run-id>
```

如果要重新跑：

```bash
keel rerun <run-id>
```

## 本地 Commit 工作流

candidate ready 后，先 dry-run：

```bash
keel commit <run-id> --dry-run
```

确认后本地 commit：

```bash
keel commit <run-id>
keel commit <run-id> --message "keel: fix login validation"
keel commit <run-id> --json
```

行为边界：

- 只在 `.keel/worktrees/<run-id>` 的 candidate branch 上 commit。
- 不需要 remote。
- 不需要 GitHub / GitLab / Gitee。
- 不 push。
- 不 merge。
- risk warnings 不阻止 commit，只作为审查提示。

## Generic Git Push 工作流

已经 commit 的 candidate 可以 push 到任意 Git remote：

```bash
keel push <run-id> --dry-run
keel push <run-id>
keel push <run-id> --remote origin
keel push <run-id> --json
```

行为边界：

- 只 push candidate branch。
- 不 push `main` / `master`。
- 不 push tags。
- 不创建 PR/MR。
- 不 merge。
- remote 可以是 GitHub、GitLab、Gitee、Gitea、自建 Git、bare Git repo。

## PR / MR 工作流

手动计划，不调用 provider：

```bash
keel pr <run-id> --manual --dry-run --provider github
keel pr <run-id> --manual --dry-run --provider gitlab
keel pr <run-id> --manual --dry-run --json
```

GitHub 自动创建 PR，需要已安装并登录 `gh`：

```bash
keel pr <run-id> --provider github --dry-run
keel pr <run-id> --provider github
keel pr <run-id> --provider github --draft
keel pr <run-id> --provider github --base main
keel pr <run-id> --provider github --json
```

Keel 不存储 GitHub token。认证交给 `gh`。

## Doctor / Config / Risk

检查仓库是否适合运行 Keel：

```bash
keel doctor
keel doctor --json
```

校验配置：

```bash
keel config validate
keel config validate --json
```

`.keel/config.toml` 示例：

```toml
[checks]
commands = [
  "cargo fmt --all --check",
  "cargo test --workspace",
  "cargo clippy --workspace --all-targets -- -D warnings",
]

[risk]
paths = ["src/auth/**", "migrations/**"]
large_diff_file_threshold = 20

[readiness]
require_non_empty_diff = true
require_checks_pass = true
```

risk warnings 只提醒，不会自动阻止 ready、commit、push。

## TUI

TUI 是只读审查界面，不是主入口。

```bash
keel tui
keel tui --run <run-id>
keel tui --agent codex --status ready
```

快捷键：

| 快捷键 | 作用 |
| --- | --- |
| `j` / `Down` | 下一个 run |
| `k` / `Up` | 上一个 run |
| `1` / `2` / `3` / `4` | report / diff / log / artifacts |
| `PgUp` / `PgDn` | 滚动当前面板 |
| `/` | 过滤 |
| `r` | 刷新 |
| `?` | 帮助 |
| `q` / `Esc` | 退出 |

## Artifacts

每个 run 保存在：

```text
.keel/runs/<run-id>/
```

核心 artifacts：

- `metadata.json`
- `log.txt`
- `diff.patch`
- `checks.json`
- `report.md`
- `commit.json`
- `push.json`
- `pr.json`

`discard` 只删除 candidate worktree，不删除历史 artifacts。

## 安全模型

- 每个 run 使用独立 git worktree。
- agent 输出只是 candidate change。
- Keel 不自动 merge。
- Keel 不自动 push。
- Keel 不默认创建 PR。
- 人类始终是最终 merge decision maker。
- destructive 操作限制在 Keel-owned worktree 内。
- `.keel/` 是本地运行状态，不应该提交到 GitHub。

## 常见日常用法

### 只把 Keel 当 agent ledger

```bash
keel task start "refactor parser"
keel checkpoint "split parser module"
keel check
keel review
keel verify
keel handoff
keel task finish
```

### 让 Keel 管理一个 candidate

```bash
keel run "fix login bug" --agent codex
keel status
keel report <run-id>
keel diff <run-id>
keel commit <run-id> --dry-run
keel commit <run-id>
```

### 完整 Git 发布链路

```bash
keel commit <run-id>
keel push <run-id> --dry-run
keel push <run-id>
keel pr <run-id> --manual --dry-run --provider github
keel pr <run-id> --provider github --dry-run
keel pr <run-id> --provider github
```

### 本地项目完全不配置 remote

可以。Keel 的核心能力不需要 remote：

```bash
keel init
keel task start "local only work"
keel check
keel review
keel verify
keel commit <run-id>
```

只有 `keel push` 和 provider PR 需要 remote。
