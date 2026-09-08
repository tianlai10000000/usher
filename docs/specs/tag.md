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
7. Step logs including truncation markers are capped at 32 KiB. The streaming
   driver uses 4 KiB reads and a 16-block queue; it drains concurrently with readers
   and aborts/joins readers on cancellation. Secrets must not enter commands/logs.
8. Remote mode remains explicitly unavailable in the current product and must report that
   state instead of silently executing a local plan.
9. The Tag configuration surface shows inputs and start actions only. After a
   valid create/remove start is accepted, the complete queued plan is published
   to the application Job Run route before execution begins.
10. Every rendered step reserves a duration column. Command and refresh steps use measured
    monotonic elapsed time. Pending steps render an empty duration cell; a running ellipsis is
    transient state and must not be presented as a measured result.
11. Navigating from Tag to Job Run, Settings, or another route and returning
    through Back preserves the selected Add/Remove tab, inputs, summary, step
    statuses, durations, and bounded logs. Reconstructing the Slint component
    tree must not invoke a fresh Tag initialization.
12. Add/Remove switching uses the shared UnderlineNav tab contract in
    [GitHub/Primer UI design](ui-design.md)
    (`GitHubUnderlineNavItem`); do not invent alternate tab chrome. Local/Remote
    and Patch/Minor/Major use the shared `GitHubSegmentedControl*` contract.
13. Tag owns only its Git plans and orchestration. Step definitions, runtime
    lifecycle, job conclusion, total duration, and the visual Job surface use
    the shared [Reusable job runs](jobs.md) contract.
14. Switching Add/Remove, Patch/Minor/Major, or Local/Remote while idle resets
    the pending plan and current Job Run descriptor. A prior run can be reopened
    only while the configuration still exposes its current/latest run action.

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
7. 步骤日志含截断标记最多 32 KiB。流式驱动每次读取 4 KiB，队列最多 16 块；
   Reader 运行期间并发排空，取消时 abort/join。不得把密钥加入命令或日志。
8. 当前产品的 Remote 模式仍明确不可用，必须展示该状态，不得静默执行 Local 计划。
9. Tag 配置面只展示输入与启动操作。合法创建/删除启动被接受后，必须先把完整的
   queued 计划发布到应用级 Job Run 路由，再开始执行。
10. 每个可见步骤都必须保留耗时列。命令与刷新步骤使用单调时钟测得的实际耗时；
    pending 步骤的耗时单元格保持为空；running 的省略号只是瞬时状态，不得伪装成
    已测量结果。
11. 从 Tag 进入 Job Run、Settings 或其他路由，再通过 Back 返回时，必须保留
    Add/Remove 选中页、输入、摘要、步骤状态、耗时和有界日志；重建组件树不得触发
    新的 Tag 初始化。
12. Add/Remove 切换遵循 [GitHub/Primer UI 设计](ui-design.md)
    的 UnderlineNav 契约（`GitHubUnderlineNavItem`），不得另起 Tab 外观；
    Local/Remote 与 Patch/Minor/Major 统一使用 `GitHubSegmentedControl*`。
13. Tag 只拥有自身 Git 计划和领域编排；Step 定义、运行生命周期、Job conclusion、
    总耗时和可视 Job 表面统一使用[可复用 Job Run](jobs.md)契约。
14. 空闲时切换 Add/Remove、Patch/Minor/Major 或 Local/Remote，必须重置待执行计划
    和当前 Job Run 描述符；只有配置面仍展示当前/最近运行操作时才能重新打开该运行。

## Ownership / 职责

- `src/workflow/tag.rs`: stable binding/refresh façade.
- `src/workflow/tag/controller.rs`: UI callbacks, project state, sequencing, progress,
  cancellation, and command execution.
- `src/workflow/tag/version.rs`: typed semantic-version parsing, bump rules, and the Slint string boundary.
- `src/workflow/tag/commands.rs`: typed create/remove Git argv plans and their immutable Job step projection.
- `src/workflow/tag/command_output.rs`: bounded streaming transport, tail retention and reader lifecycle.
- `src/jobs/`: reusable immutable step definitions, runtime snapshots, and Job Run projection.
- `ui/components/jobs/job.slint`: shared `GitHubActionsJob` presenter.
- `ui/views/job_run.slint`: shared application-level execution route.
- `src/workflow/state.rs`: project-scoped persisted-in-memory workflow view state,
  including one `JobSnapshot` for Tag.

- `src/workflow/tag.rs`：稳定的绑定/刷新 façade。
- `src/workflow/tag/controller.rs`：UI callback、项目状态、序列、进度、取消和命令执行。
- `src/workflow/tag/version.rs`：强类型 SemVer 解析、递增规则及 Slint 字符串边界。
- `src/workflow/tag/commands.rs`：强类型创建/删除 Git argv 计划及其不可变 Job Step 投影。
- `src/jobs/`：可复用的不可变 Step 定义、运行时快照与 Job Run 投影。
- `ui/components/jobs/job.slint`：共享的 `GitHubActionsJob` 展示组件。
- `ui/views/job_run.slint`：应用级共享执行路由。
- `src/workflow/state.rs`：按项目隔离的 workflow view 内存状态，并为 Tag 持有一个
  `JobSnapshot`。

## Regression checklist / 回归清单

- Run `cargo test --lib --locked workflow::tag`.
- Verify major/minor/patch results and invalid-input fallback.
- Verify paths and descriptions containing spaces remain one argv item.
- Verify create/remove command ordering.
- Manually verify project switching, duplicate-start protection, cancellation, bounded progress
  logs, the explicit Remote-mode unavailable state, and Settings → Back state restoration.
- Verify valid Create/Remove opens Job Run, invalid input stays on Tag, Back
  restores the configuration, and View latest run reopens the shared route.
- After a failed create with manually expanded steps, switching Patch/Minor/Major or
  Add/Remove must show a fully collapsed queued plan for the destination surface.

- 运行 `cargo test --lib --locked workflow::tag`。
- 验证 major/minor/patch 结果与非法输入回退。
- 验证包含空格的路径和描述仍是单个 argv 参数。
- 验证创建/删除命令顺序。
- 手工验证项目切换、防重复启动、取消、有界进度日志、Remote 模式明确不可用状态，
  以及 Settings → Back 后的状态恢复。
- 验证合法 Create/Remove 打开 Job Run、非法输入留在 Tag、Back 恢复配置，以及
  View latest run 可重新打开共享路由。
- 创建失败并手动展开步骤后，切换 Patch/Minor/Major 或 Add/Remove 时，目标表面必须
  显示完全折叠的 queued 计划。
