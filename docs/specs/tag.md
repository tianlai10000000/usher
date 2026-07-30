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
   The project `tag_running` flag stays latched from accepted start until the terminal
   success/failure callback or an explicit stop; progress ticks must not clear it from
   transient step statuses (that would flash the Stop control between steps).
7. Step logs are bounded. Secrets must not be added to commands or logs.
8. Remote mode remains explicitly unavailable in the current product and must report that
   state instead of silently executing a local plan.
9. Because the local create and remove plans are deterministic, their complete step lists are
   visible before execution with pending status. This intentionally differs from GitHub Actions,
   which does not render job-log steps until a workflow run exists.
10. Every rendered step reserves a duration column. Command and refresh steps use measured
    monotonic elapsed time. Pending steps render an empty duration cell; a running ellipsis is
    transient state and must not be presented as a measured result.
11. Navigating from Workflow to a utility route such as Settings and returning through Back
    preserves the selected Add/Remove tab, summary, step statuses, durations, and bounded logs.
    Reconstructing the Slint component tree must not invoke a fresh Tag initialization.
12. Add/Remove switching uses the shared UnderlineNav tab contract in
    [GitHub/Primer UI design](ui-design.md#underlinenav-tabs--underlinenav-页签)
    (`TagTabChip`); do not invent alternate tab chrome.
13. Tag owns only its Git plans and orchestration. Step definitions, runtime
    lifecycle, job conclusion, total duration, and the visual Job surface use
    the shared [Reusable workflow jobs](jobs.md) contract.
14. Switching Add/Remove, Patch/Minor/Major, or Local/Remote while idle must
    restore a queued plan for the destination surface and clear user disclosure
    overrides. A prior run's expanded or failed steps must not remain visible.

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
   必须丢弃。项目的 `tag_running` 从接受启动起保持闩锁，直到终态成功/失败回调或
   显式 Stop；进度回写不得根据瞬时步骤 status 清除它（否则步骤间隙会闪烁 Stop）。
7. 步骤日志必须有界，不得把密钥加入命令或日志。
8. 当前产品的 Remote 模式仍明确不可用，必须展示该状态，不得静默执行 Local 计划。
9. 本地创建和删除计划是确定的，因此执行前必须以 pending 状态展示完整步骤列表。
   这有意区别于 GitHub Actions；后者在 workflow run 尚未创建时不会渲染 Job-log
   步骤。
10. 每个可见步骤都必须保留耗时列。命令与刷新步骤使用单调时钟测得的实际耗时；
    pending 步骤的耗时单元格保持为空；running 的省略号只是瞬时状态，不得伪装成
    已测量结果。
11. 从 Workflow 进入 Settings 等工具路由，再通过 Back 返回时，必须保留 Add/Remove
    选中页、摘要、步骤状态、耗时和有界日志；重建组件树不得触发新的 Tag 初始化。
12. Add/Remove 切换遵循 [GitHub/Primer UI 设计](ui-design.md#underlinenav-tabs--underlinenav-页签)
    的 UnderlineNav 契约（`TagTabChip`），不得另起 Tab 外观。
13. Tag 只拥有自身 Git 计划和领域编排；Step 定义、运行生命周期、Job conclusion、
    总耗时和可视 Job 表面统一使用[可复用 Workflow Job](jobs.md)契约。
14. 空闲时切换 Add/Remove、Patch/Minor/Major 或 Local/Remote，必须恢复目标表面的
    queued 计划并清除用户展开覆盖；不得继续显示上一轮运行的展开或失败步骤。

## Ownership / 职责

- `src/workflow/tag.rs`: stable binding/refresh façade.
- `src/workflow/tag/controller.rs`: UI callbacks, project state, sequencing, progress,
  cancellation, and command execution.
- `src/workflow/tag/version.rs`: pure semantic-version parsing and bump rules.
- `src/workflow/tag/commands.rs`: pure create/remove Git argv plans.
- `src/workflow/jobs/`: reusable immutable step definitions and runtime snapshots.
- `ui/components/jobs/job.slint`: shared GitHub Actions-style Job presenter.
- `src/workflow/state.rs`: project-scoped persisted-in-memory workflow view state,
  including one `JobSnapshot` for Tag.

- `src/workflow/tag.rs`：稳定的绑定/刷新 façade。
- `src/workflow/tag/controller.rs`：UI callback、项目状态、序列、进度、取消和命令执行。
- `src/workflow/tag/version.rs`：纯 SemVer 解析和递增规则。
- `src/workflow/tag/commands.rs`：纯创建/删除 Git argv 计划。
- `src/workflow/jobs/`：可复用的不可变 Step 定义和运行时快照。
- `ui/components/jobs/job.slint`：共享的 GitHub Actions 风格 Job 展示组件。
- `src/workflow/state.rs`：按项目隔离的 workflow view 内存状态，并为 Tag 持有一个
  `JobSnapshot`。

## Regression checklist / 回归清单

- Run `cargo test --lib --locked workflow::tag`.
- Verify major/minor/patch results and invalid-input fallback.
- Verify paths and descriptions containing spaces remain one argv item.
- Verify create/remove command ordering.
- Manually verify project switching, duplicate-start protection, cancellation, bounded progress
  logs, the explicit Remote-mode unavailable state, and Settings → Back state restoration.
- After a failed create with manually expanded steps, switching Patch/Minor/Major or
  Add/Remove must show a fully collapsed queued plan for the destination surface.

- 运行 `cargo test --lib --locked workflow::tag`。
- 验证 major/minor/patch 结果与非法输入回退。
- 验证包含空格的路径和描述仍是单个 argv 参数。
- 验证创建/删除命令顺序。
- 手工验证项目切换、防重复启动、取消、有界进度日志、Remote 模式明确不可用状态，
  以及 Settings → Back 后的状态恢复。
- 创建失败并手动展开步骤后，切换 Patch/Minor/Major 或 Add/Remove 时，目标表面必须
  显示完全折叠的 queued 计划。
