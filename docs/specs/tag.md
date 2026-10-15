# Tag workflow / Tag 工作流

## Scope / 范围

This specification covers the project-scoped Tag workflow, semantic-version selection, local
Git command plans, progress reporting, and cancellation.

本文覆盖项目级 Tag 工作流、语义版本选择、本地 Git 命令计划、进度回写与取消。

## Change navigation / 修改导航

| Change / 改动 | Rust owner / Rust 入口 | Slint/test boundary / Slint 与测试边界 |
| --- | --- | --- |
| Version parsing and bump / 版本解析及递增 | `src/workflow/tag/version.rs` | `cargo test --lib --locked workflow::tag::version` |
| Git argv and queued plan / Git argv 与 queued 计划 | `src/workflow/tag/commands.rs` | `cargo test --lib --locked workflow::tag::commands` |
| Streaming/cancellation/errors / 流式输出、取消及错误 | `src/workflow/tag/command_output.rs`, `controller.rs` | `cargo test --lib --locked workflow::tag`; [Security boundaries](security-boundaries.md) |
| Init/start/Back/Job publication / 初始化、启动、Back 及 Job 发布 | `src/workflow/tag/controller.rs`, `src/workflow/state.rs`, `bindings.rs` | `ui/main.slint` callbacks; `ui/views/workflow/workflow.slint`, `ui/components/workflow/github_tag_{flow,add_panel,remove_panel}.slint`; `cargo test --lib --locked workflow::state` plus manual matrix below / 加下方手工矩阵 |
| Step durations/log rendering / 步骤耗时及日志展示 | `src/jobs/` | [Reusable job runs](jobs.md) owns the shared presenter and acceptance matrix / 共享展示与验收矩阵归该规格 |

Focused tests support iteration; `AGENTS.md` handoff checks still apply. UI
changes require native inspection in the source locale and one packaged
non-source locale. Remote provider mode is unavailable; the implemented Local
workflow still performs Git network operations.

定向测试用于迭代，交付仍按 `AGENTS.md` 检查。UI 改动须原生检查并覆盖源语言及一种
内置非源语言。Remote Provider 模式尚未实现；已实现的 Local 工作流仍包含 Git 网络操作。

## Initialization and failure behavior / 初始化与失败行为

- Add initialization reads `git -C <repo> tag --list --sort=-version:refname`
  through the shared-runtime asynchronous process driver and selects the first tag accepted by the core parser.
  This is Git's version-ref ordering, not commit-date ordering or full SemVer
  prerelease precedence. No accepted tag yields `v0.0.0`; a Git failure is
  reported separately even when the displayed version fields fall back to that baseline.
- Remove initialization refreshes `origin` with `fetch origin --tags --force`,
  then reads `ls-remote --tags --refs origin`. Its parser compares numeric
  major/minor/patch cores and ignores prerelease/build qualifiers for that
  comparison. After successful deletion, the Job's refresh step repeats these
  operations before completion. The displayed refresh definition is a Job
  descriptor, not the complete executed argv; orchestration stays in the controller.
- The first failed command ends the run with its bounded log and failed step.
  Stop requests cancellation; already completed checkout/rebase/push/tag/delete
  effects are not rolled back automatically. A local deletion can therefore
  succeed before a remote deletion fails. Recovery must inspect actual repository
  state before a retry.

- Add 初始化在共享 Runtime 的异步进程驱动执行本地 `git tag --list --sort=-version:refname`，
  取核心版本 parser 接受的首项。这是 Git 版本引用排序，不是提交时间或完整 SemVer
  prerelease 排序；没有合法项回退 `v0.0.0`，Git 失败必须单独报告，即使展示版本也回退基线。
- Remove 初始化先 `fetch origin --tags --force`，再 `ls-remote --tags --refs origin`；
  parser 按数值 major/minor/patch 核心比较，忽略预发布/build。删除成功后 Job 的
  refresh 步骤再次执行这两项。展示的 refresh definition 是 Job 描述符，不是完整
  执行 argv；编排仍由 controller 负责。
- 第一个失败命令结束运行并保留失败步骤及有界日志。Stop 发出取消请求，已完成的
  checkout/rebase/push/tag/delete 不自动回滚；可能本地删除已成功、远端删除失败。
  重试前必须先核对实际仓库状态。

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
15. Local Tag initialization reports a specific, localized failure for a missing
    repository, unavailable Git executable, Apple developer-tools/license setup,
    repository ownership, repository validity, and permissions. Unclassified Git
    failures include a bounded diagnostic excerpt and exit status; the same semantic
    error is used by the configuration result and Toast.

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
15. Local Tag 初始化必须针对仓库缺失、Git 可执行文件不可用、Apple 开发者工具/许可、
    仓库所有权、仓库合法性和权限分别显示明确且已本地化的错误。未分类的 Git 失败必须
    包含有界诊断摘要和退出状态；配置结果与 Toast 使用同一语义错误。

## Ownership / 职责

- `src/workflow/tag.rs`: stable binding/refresh façade.
- `src/workflow/tag/controller.rs`: typed UI input capture and guarded effect execution.
- `src/workflow/tag/state.rs`: pure Tag lifecycle/configuration Actions and reducer.
- `src/workflow/tag/execution.rs`: Git plans, step timing and cancellation-aware orchestration.
- `src/workflow/tag/initialization.rs`, `initialization_error.rs`: bounded inspection and semantic errors.
- `src/workflow/tag/activity.rs`, `progress.rs`: cancellation ownership and terminal progress drain.
- `src/workflow/state/facade.rs`, `reducer.rs`, `equality.rs`: ordered admission, subscriptions and semantic commit identity.
- `src/workflow/tag/version.rs`: typed semantic-version parsing, bump rules, and the Slint string boundary.
- `src/workflow/tag/commands.rs`: typed create/remove Git argv plans and their immutable Job step projection.
- `src/workflow/tag/command_output.rs`: bounded streaming transport, tail retention and reader lifecycle.
- `src/jobs/`: reusable immutable step definitions, runtime snapshots, and Job Run projection.
- `ui/components/jobs/job.slint`: shared `GitHubActionsJob` presenter.
- `ui/views/job_run.slint`: shared application-level execution route.
- `src/workflow/state.rs`: project-scoped in-memory workflow view state (not disk persistence),
  including one `JobSnapshot` for Tag.

- `src/workflow/tag.rs`：稳定的绑定/刷新 façade。
- `src/workflow/tag/controller.rs`：强类型 UI 输入与有保护的 effect 执行。
- `src/workflow/tag/state.rs`：纯 Tag 生命周期/配置 Action 与 reducer。
- `src/workflow/tag/execution.rs`：Git 计划、步骤计时及可取消编排。
- `src/workflow/tag/initialization.rs`、`initialization_error.rs`：有界检查与语义错误。
- `src/workflow/tag/activity.rs`、`progress.rs`：取消所有权与终态进度排空。
- `src/workflow/state/facade.rs`、`reducer.rs`、`equality.rs`：有序准入、自动订阅和语义 commit 身份。
- `src/workflow/tag/version.rs`：强类型 SemVer 解析、递增规则及 Slint 字符串边界。
- `src/workflow/tag/commands.rs`：强类型创建/删除 Git argv 计划及其不可变 Job Step 投影。
- `src/workflow/tag/command_output.rs`：有界流式传输、日志尾部保留和 Reader 生命周期。
- `src/jobs/`：可复用的不可变 Step 定义、运行时快照与 Job Run 投影。
- `ui/components/jobs/job.slint`：共享的 `GitHubActionsJob` 展示组件。
- `ui/views/job_run.slint`：应用级共享执行路由。
- `src/workflow/state.rs`：按项目隔离的 workflow view 内存状态，并为 Tag 持有一个
  `JobSnapshot`，不跨应用重启持久化。

## State, draft and process lifecycle / 状态、草稿与进程生命周期

`WorkflowAction` and the feature-owned `SharedWorkflowState` facade own all
Branch/CD/Tag mutations. UI callbacks capture the complete typed input and the
admission-time project/generation identity, then use a nonblocking FIFO. One
worker dispatches Actions on the shared blocking pool in accepted order. State
is an immutable, revisioned `usher-state-store` snapshot; unchanged Actions do
not increment its revision, and queue rejection never dispatches. A domain
validation failure that changes the displayed result is a changed commit.
Changed commits automatically feed one
replaceable UI delivery slot; no worker touches Slint.

Admission permits bound 64 complete operations, including accepted queue items,
cancelling process tasks and completed Toast/navigation callbacks awaiting the
UI. Each operation retains its permit through its derived background progress,
terminal Action and UI effects. A full/closing facade rejects new UI work with a
localized error. Background publications await queue space. Replaceable progress
has one latest mailbox and one forwarding task, and a terminal result waits for
that task to drain before entering the FIFO. A project selection increments its
identity even for A → B → A; old project/generation publications cannot recreate
or mutate the new project's state. Project opening hydrates the committed
snapshot before navigating; rejected selections stay on Project.

The complete Tag draft (next version, removal target and description) is
project-scoped in-memory state. The actual `edited` event forwards all three
fields atomically; programmatic projection does not create another edit. The
facade overlays a newer accepted draft until its Action commits, so an older
projection cannot replace current typing. Add/Remove remounting and route Back
preserve the description; application restart does not persist these drafts.

Initialization uses the same bounded process transport as execution: 4 KiB
reads, a 16-chunk queue, 32 KiB retained logs and an 8 KiB maximum partial tag
reference line. Each initialization command has a 30-second deadline; Remove
runs fetch and listing as two separately bounded commands. Switching
project/mode/tab, replacing initialization, starting a run or closing cancels
the previous initialization. Cancellation/deadline kills the owned process
tree, reaps the direct child, and aborts/joins pipe readers before completion.
Unix starts a separate process group; Windows attaches the direct child to an
OS job. Native Windows acceptance remains required, including the interval
between Git spawn and job assignment (unlike the Hook bootstrap, Git has no
stdin handshake preventing helpers from starting in that interval).

Run navigation verifies project identity, run generation, cancellation and the
running latch when its UI callback executes. Stop followed by a configuration
reset invalidates the old run's terminal result. Shutdown closes UI admission,
drains accepted Actions, cancels and joins owned tasks, then drains their final
Actions/effects before closing the Store bridge. Concurrent/repeated shutdown
waits for the same barrier; the event loop stays available during this drain.

`WorkflowAction` 与 Feature 自有 `SharedWorkflowState` 门面统一拥有 Branch/CD/Tag
变更。UI 在准入时捕获完整强类型输入及项目/代际身份，通过有界、非阻塞 FIFO
交给单个 Worker；Worker 按接受顺序在共享 blocking pool 归约。未变化 Action
不增加 revision；队列拒绝不 dispatch，领域验证若改变展示结果仍为有效 commit。
有效 commit 自动进入一个可替换 UI 投递槽，后台不操作 Slint。
64 个操作许可覆盖排队输入、尚在取消的进程任务，以及已完成但 UI 尚未消费的
Toast/导航回调；许可持续经过派生进度、终态 Action 和 UI effect。后台发布等待队列
空间，进度只保留一个最新 mailbox 和一个转发任务，终态先排空进度再入队。
A → B → A 仍使用不同项目代际，旧发布不得重建或污染新项目状态。项目选择真实提交、
hydrate 后才进入 Workflow，拒绝时留在 Project。

Tag 的 next version、remove target、description 作为完整草稿按项目保留在内存。
只有实际 `edited` 回调原子提交三项输入，投影不产生编辑回声；较新的已接受草稿会覆盖
尚未追上的旧投影，直到对应 Action 提交。切换 Add/Remove 或路由 Back 不丢描述，
应用重启不持久化草稿。初始化复用执行管线，单条命令最多 30 秒，Remove 的 fetch/list
分别计时；读取块/队列/日志分别为 4 KiB、16 块和 32 KiB，Tag 引用半行上限 8 KiB。
项目/模式/页签变化、替换初始化、开始运行或退出会取消旧初始化；取消/超时终止所拥有
进程树，等待直接子进程结束并 abort/join pipe reader。Unix 使用独立进程组，Windows
使用 OS job；Git spawn 到 job assignment 间的 helper 逃逸窗口仍须 Windows 原生验收，
不能套用 Hook bootstrap 的 stdin 握手保证。延迟运行导航还核对 run 代际、token 与
running 标记；Stop 后重置配置会拒绝旧终态。退出先关 UI 准入，排空已接受动作，再取消/
等待任务并排空其终态，最后关闭 Store bridge；并发/重复退出共用同一完成屏障。

## Regression checklist / 回归清单

- Run `cargo test --lib --locked workflow::tag` and `cargo test --lib --locked workflow::state`.
- Exercise the real child/descendant timeout and cancellation fixtures, legitimate local/bare-origin
  Git controls, FIFO/no-op revision tests, full-owner rejection, concurrent close, stale initialization
  and A → B → A publications. The ignored helpers are invoked by their parent tests.
- Keep the Unix direct-child-only negative control: its descendant must still write after the
  direct child is reaped; the production cancellation/deadline cases must prevent that write.
- The compiled Project open fixture covers English/Chinese and grid/list: rejection remains on
  Project; a blocked B commit followed by C opens only hydrated C. Native route/Back checks below
  remain separate acceptance.
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
- 保留 Unix 仅终止直接子进程的负面对照：直接子进程回收后后代仍能写标记；生产
  取消/超时用例必须阻止同一后代写入。真实本地/裸仓库 Git 作为正常用途对照。
- 验证 major/minor/patch 结果与非法输入回退。
- 验证包含空格的路径和描述仍是单个 argv 参数。
- 验证创建/删除命令顺序。
- 手工验证项目切换、防重复启动、取消、有界进度日志、Remote 模式明确不可用状态，
  以及 Settings → Back 后的状态恢复。
- 验证合法 Create/Remove 打开 Job Run、非法输入留在 Tag、Back 恢复配置，以及
  View latest run 可重新打开共享路由。
- 创建失败并手动展开步骤后，切换 Patch/Minor/Major 或 Add/Remove 时，目标表面必须
  显示完全折叠的 queued 计划。
