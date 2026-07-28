# Tag workflow / Tag 工作流

## Scope / 范围

This specification covers the project-scoped Tag workflow, semantic-version selection, local
Git command plans, progress reporting, and cancellation.

本文覆盖项目级 Tag 工作流、语义版本选择、本地 Git 命令计划、进度回写与取消。

## Invariants / 稳定约束

1. Tag state is scoped by the selected project. Switching projects must not reuse another
   project's running flag, selected version, steps, result, or repository path.
2. Local mode resolves the newest valid semantic tag and falls back to `v0.0.0` when the
   repository contains none. Tags may use `v`/`V`; prerelease/build qualifiers do not change
   the parsed `major.minor.patch` core.
3. Major, minor, and patch selection derives the next tag without mutating repository state.
   Invalid current input falls back to the `v0.0.0` baseline.
4. A create plan validates the repository, updates `release`, rebases and pushes `main`,
   creates an annotated tag, then pushes that tag. A remove plan validates the repository,
   deletes the local tag, then deletes the remote tag.
5. Every command is executed as an argument vector, never through shell interpolation.
   Repository paths, tag names, and descriptions remain distinct arguments.
6. Long-running work uses the shared application runtime, rejects duplicate starts through
   workflow state, and observes a cancellation token between steps and while commands run.
   UI updates occur through the Slint event loop and stale initialization sequences are ignored.
7. Step logs are bounded. Secrets must not be added to commands or logs.
8. Remote mode remains explicitly unavailable in the current product and must report that
   state instead of silently executing a local plan.

1. Tag 状态按所选项目隔离；切换项目不得复用其他项目的 running 标记、版本、步骤、
   结果或仓库路径。
2. Local 模式解析最新合法语义 Tag；仓库没有语义 Tag 时回退到 `v0.0.0`。允许
   `v`/`V` 前缀；预发布和 build qualifier 不改变 `major.minor.patch` 核心。
3. major、minor、patch 选择只计算下一个 Tag，不修改仓库；当前输入非法时以
   `v0.0.0` 为基线。
4. 创建计划依次校验仓库、更新 `release`、rebase 并推送 `main`、创建 annotated
   Tag、推送 Tag。删除计划依次校验仓库、删除本地 Tag、删除远端 Tag。
5. 命令始终以 argv 执行，禁止 shell 插值；仓库路径、Tag 名称和描述保持独立参数。
6. 长任务使用应用共享 runtime，通过 workflow state 防止重复启动，并在步骤之间及
   命令执行期间检查取消 token。UI 只能通过 Slint event loop 回写，过期初始化序列
   必须丢弃。
7. 步骤日志必须有界，不得把密钥加入命令或日志。
8. 当前产品的 Remote 模式仍明确不可用，必须展示该状态，不得静默执行 Local 计划。

## Ownership / 职责

- `src/workflow/tag.rs`: stable binding/refresh façade.
- `src/workflow/tag/controller.rs`: UI callbacks, project state, sequencing, progress,
  cancellation, and command execution.
- `src/workflow/tag/version.rs`: pure semantic-version parsing and bump rules.
- `src/workflow/tag/commands.rs`: pure create/remove Git argv plans.
- `src/workflow/state.rs`: project-scoped persisted-in-memory workflow view state.

- `src/workflow/tag.rs`：稳定的绑定/刷新 façade。
- `src/workflow/tag/controller.rs`：UI callback、项目状态、序列、进度、取消和命令执行。
- `src/workflow/tag/version.rs`：纯 SemVer 解析和递增规则。
- `src/workflow/tag/commands.rs`：纯创建/删除 Git argv 计划。
- `src/workflow/state.rs`：按项目隔离的 workflow view 内存状态。

## Regression checklist / 回归清单

- Run `cargo test --lib --locked workflow::tag`.
- Verify major/minor/patch results and invalid-input fallback.
- Verify paths and descriptions containing spaces remain one argv item.
- Verify create/remove command ordering.
- Manually verify project switching, duplicate-start protection, cancellation, bounded progress
  logs, and the explicit Remote-mode unavailable state.

- 运行 `cargo test --lib --locked workflow::tag`。
- 验证 major/minor/patch 结果与非法输入回退。
- 验证包含空格的路径和描述仍是单个 argv 参数。
- 验证创建/删除命令顺序。
- 手工验证项目切换、防重复启动、取消、有界进度日志和 Remote 模式明确不可用状态。
