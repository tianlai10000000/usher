# Specifications / 规格索引

Hooks requires typed host/validation errors and semantic notification metadata,
with immediate locale refresh of mounted history, feedback and Job/Banner copy.
Hooks 约束类型化宿主/校验错误、语义通知元数据及已挂载文案的即时语言切换。

The [P6 regression protocol](../p6-regression.md) defines opt-in repeated-load
validation, paired and absolute gates, cancellation and raw evidence requirements.
P6 回归协议统一真实重复负载、成对/绝对门禁、取消与原始证据要求。

Specifications describe stable behavior and invariants. Architecture documents explain ownership and flow; historical notes explain how a problem was investigated. Do not mix those three roles.

PR Review requires guarded highlight completion writes. Hooks defines serialized
journal budgets and bounded, cancellable replay. / PR Review 要求校验高亮完成回写；
Hooks 定义序列化日志预算和有界、可取消回放。
PR Review additionally requires detached syntax-resource leases and guarded
source/expand loading writes; derived-presentation release remains an offline
candidate pending native Release A/B. / PR Review 另要求分离的语法资源 lease 和
源码/展开 loading 回写校验；派生展示释放仍为等待原生 Release A/B 的离线候选。
The unadopted syntax-only background-release experiment has been removed from
application code. Normal hiding retains the current review; PR replacement
must keep owners bounded and isolate old worker leases. Memory-policy changes
still require native repeated-growth and reentry checks, not just drop tests.
/ 未采用的仅后台释放语法资源实验已从应用代码移除。正常隐藏保留当前审核；
PR 替换须保持 owner 有界并隔离旧 Worker lease。内存策略变更仍须原生重复增长
与重入验收，不得以对象析构单测代替。
Hooks also requires shared configuration snapshots, durable-only automatic
writes and coalesced incremental Job projection; Jobs bounds shaping offsets.
/ Hooks 还要求共享配置快照、只随持久变化自动写盘及合并后的增量 Job 投影；Jobs 约束排版偏移范围。
Jobs also requires shared immutable measurement snapshots and UTF-8 wrap ranges,
with visual text materialized only for visible source rows, mounted viewport
leases pinned, and unmounted cache owners governed by soft entry/retained-byte
eviction targets. / Jobs 还要求共享不可变测量快照并保存 UTF-8 换行区间，
仅对可见源行物化视觉文本；挂载中的视口 lease 必须 pin，未挂载缓存 owner
遵循条目数与 retained bytes 软淘汰目标。

Spec 描述稳定行为和不变量；架构文档解释职责与数据流；历史记录描述问题排查过程。三者不要混写。

| Spec | Status | Code owners |
| --- | --- | --- |
| [Multiline text layout](text-layout.md) | Mandatory native line-height, documented fallback and measurement/extent synchronization / 强制原生行高、例外记录与测量/extent 同步契约 | `ui/`, feature text-layout adapters, worker measurement and exact virtual extents |
| [Exact virtual scrolling](virtual-scrolling.md) | Shared contract for variable-height documents, stable thumb geometry and soft retained-owner eviction targets / 可变行高文档、稳定滑块几何与 retained-owner 软淘汰目标通用契约 | `crates/usher-virtual-list/`, `src/app_runtime/latest_compute.rs`, `ui/components/github_scroll_view.slint`, feature layout adapters |
| [HTTP pipeline](http.md) | Explicit provider metadata/diff response budgets and bounded error reads / 显式 Provider 元数据/diff 响应预算与有界错误读取 | `src/http/mod.rs`, all HTTP callers |
| [Cross-page state store](state-store.md) | Current: bounded latest/dirty snapshots, explicit same-state retry and revision/attempt barriers / 当前：有界 latest/dirty 快照、同值显式重试与 revision/attempt 屏障 | `crates/usher-state-store/`, all cross-page state owners and UI bridges |
| [Shared search navigation](search.md) | Current / 当前 | `crates/usher-search/`, feature Find adapters, PR `find_bridge.rs`, `controller/diff_bindings.rs`, `PrDiffBridge` |
| [Internationalization](i18n.md) | Implementation current; full-route visual QA pending / 实现当前，全路由视觉验收待完成 | `i18n/`, `crates/usher-i18n/`, `src/i18n.rs`, all user-facing copy |
| [AI Agent configuration](ai-agent.md) | Current / 当前 | `src/settings.rs`, `src/ai/agent.rs`, `ui/views/settings.slint` |
| [AI Code Review](ai-code-review.md) | Current: Session transactions, real cancellation receipts, checked reconciliation and guarded UI delivery / 当前：Session 事务、取消真实 receipt、checked 对账与受保护的 UI 投递 | `crates/usher-ai-review/`, `src/ai_review/` (controller, session transactions, sessions, task persistence), `src/agent_response/`, PR list/detail and Settings bridges |
| [Diagnostic logging](diagnostics.md) | Current: grouped settings and legacy-key migration / 当前：配置分类与旧键迁移 | `src/settings.rs`, `src/diagnostics.rs`, `src/logger/`, `src/memory_diagnostics.rs`, `src/terminal/`, Settings UI and bridges |
| [Chat behavior](chat.md) | Current: history boundary anchoring, bidirectional viewport coverage, and mode-switch tail mounting / 当前：历史边界锚定、双向视口覆盖与模式切换尾部挂载 | `src/chat/` (interactive, viewport policy, message model), `ui/views/chat.slint`, `ui/components/chat/`, `ui/main.slint` |
| [Artifact preview framework](artifact-preview.md) | Current: split/floating/detached modes, placement persistence, full divider capture and stable detached first frames / 当前：三栏/浮动/独立窗口、模式持久化、完整分割线捕获与稳定的独立窗口首帧 | `crates/usher-preview/`, `src/artifact_preview/`, `src/platform/`, `ui/components/artifact_preview/`, Settings and all preview callers |
| [Project repository metadata](project-repository.md) | Current: explicit Hook opt-in with disabled-row restoration, bounded Logo retries and projection / 当前：Hook 显式选择与禁用行恢复、Logo 有界重试与投影 | `src/project/mod.rs`, `src/git/`, `src/codereview/api/` |
| [Pull request workflow](pr-review.md) | Approval/lifecycle separation, review-mutation list refresh, finite provider inputs and explicit diff presentation / 批准与生命周期独立、评审变更刷新列表、有限 Provider 输入与显式 diff 展示 | `src/workflow/pr_data.rs`, `src/workflow/pr_list.rs`, `src/workflow/pr_review.rs`, `src/workflow/pr_review/`, `src/git/`, `ui/features/pr_review/`, `ui/views/workflow/`, `ui/components/workflow/` |
| [PR conflict browsing](pr-conflicts.md) | Read-only; latest pending enrichment, four shared job permits, scope-fenced observations and real legacy drain / 只读；最新待处理 enrichment、四个共享逻辑任务许可、scope 观测隔离与真实旧关闭 drain | `src/git/conflicts/`, `src/workflow/pr_conflict_status.rs`, `src/workflow/pr_review/conflicts/`, `ui/views/workflow/pr_review_conflicts.slint` |
| [Reusable job runs](jobs.md) | Current: typed lifecycle/durations, bounded output, virtualized logs with a soft-budgeted unmounted cache, safe blank rows and fixed run pages with output-only scrolling / 当前：类型化生命周期/耗时、有界输出、带软预算未挂载缓存的日志虚拟化、空行安全处理及固定运行页/仅输出区滚动 | `src/jobs/`, `ui/views/job_run.slint`, `ui/components/jobs/`, feature controllers |
| [Application Hooks](hooks.md) | Current: bounded syntax text, native cursor/selection, and route/session-owned editor rendering / 当前：有界语法文本、原生光标/选区及由路由/会话共同拥有的编辑器渲染树 | `src/hooks/`, `src/project/`, Settings, PR/AI completion sources, notifications and shutdown drains |
| [Tag workflow](tag.md) | Current / 当前 | `src/workflow/tag.rs`, `src/workflow/tag/`, `src/workflow/state.rs`, `ui/views/workflow/` |
| [Terminal behavior](terminal.md) | Bounded I/O and asynchronous session shutdown barrier / 有界 IO 与异步会话退出屏障 | `src/terminal/`, `ui/components/drawer/terminal_drawer.slint`, `ui/main.slint` |
| [Application keyboard shortcuts](shortcuts.md) | Current / 当前 | `src/shortcuts/`, `ui/main.slint`, `src/platform/` |
| [Application status bar](status-bar.md) | Current / 当前 | `ui/components/status_bar.slint`, `ui/main.slint`, shortcut/notification/network/update state owners |
| [Application message center](messages.md) | Current: latest durable/dirty state, real flush errors and bounded native FIFO; existing 20-record pages and lifecycle tooltip host / 当前：latest durable/dirty 状态、真实 flush 错误与有界原生 FIFO；保留每页 20 条和生命周期 Tooltip host | `crates/usher-messages/`, `src/message_center.rs`, `src/notification.rs`, message/status UI |
| [GitHub/Primer UI design](ui-design.md) | Current / 当前 | `ui/theme/github_theme.slint`, `ui/`, `ui/assets/icons/`, all UI contributors |
| [Tooltip implementation contract](tooltips.md) | Mandatory: factory isolation, centered text/anchor, viewport avoidance and lifecycle-owned single-host portals / 强制阅读：工厂隔离、文字与锚点居中、视口避让及生命周期托管的单宿主 Portal | Shared Tooltip/Float/IconButton, factory-owned actions and root portals |

The UI design specification includes the shared small Primer Label geometry,
transparent neutral variants and Hook status usage. / UI 设计规范包含共享 small
Primer Label 的几何、透明中性变体及 Hook 状态用法。
Shared UnderlineNav uses measured item widths, a root-owned More menu and
99+ counters across PR/Workflow/Tag navigation. / 公共 Tab 按实测宽度收纳至根层
“更多”菜单，PR/Workflow/Tag 复用同一组件，计数超过 99 显示 99+。
Composite Tab rows own one full-width baseline and disable the nested nav border.
/ 带尾部内容的 Tab 组合行只绘制一条通栏底线，并关闭内部导航底线。

The UI design specification also covers window-wide text-input outside-click blur,
embedded page coordinates, viewport clipping and selection-drag focus preservation.
/ UI 设计规范也定义全窗口输入框外部点击失焦、嵌入页面坐标、视口裁剪和拖选焦点保留。

The UI design specification also defines the shared wide split-page navigation
contract: sticky navigation and section-top alignment on section switches,
including short content. Settings implements this contract, with
unchanged compact in-flow navigation. / UI 设计规范统一定义宽屏双栏页导航吸顶、
切换分组时内容顶部对齐吸顶留白（含短内容）；设置页遵循该规则，窄屏保持原有滚动行为。

The UI design specification defines `GitHub*` component naming,
the localized playground category directory and reusable-component-before-scenario order / 本地化演练场目录及基础组件先于场景的排序，
content-sized Select menus with upward placement at bottom edges, isolated
selector models, owner-scoped option focus, verified grouped-trigger anchors,
and repeat-trigger dismissal / 按内容定高、底部向上展开、选项焦点按归属隔离、
分组触发器锚点验证、
选项模型隔离、再次点击触发器收起的 Select 浮层，
`GitHubTheme`/`GitHubMetrics` ownership, Primer layout and interaction
contracts, shared Button/IconButton/Link/Tooltip/CounterLabel/Copy/Spinner/form-control
foundations, shared shell/title-bar breadcrumb/PageHeader/ScrollView/Progress foundations,
UnderlineNav/animated SegmentedControl/Actions Job foundations,
fixed-source Dialog ownership, borderless ActionMenu/bordered SelectMenu/form Select popup (shared Tooltip
placement and non-blocking window-wide dismissal)/Banner overlay contracts,
compact PR-list branch chips, the Material dependency boundary,
the mandatory UI Components playground/property-matrix contract,
repository-wide component placement (`ui/components/<feature>/`) and route-view
composition (`ui/views/`), and the per-batch acceptance gate. The
implementation sequence and current status live in
[`docs/ui-github-migration-plan.md`](../ui-github-migration-plan.md); defects
live in [`bug-history.md`](../../bug-history.md). / UI 设计规范定义
`GitHub*` 组件命名、`GitHubTheme`/`GitHubMetrics` 归属、Primer 布局与交互
契约、CounterLabel、Shell/标题栏面包屑/PageHeader/ScrollView/Progress、
UnderlineNav/带滑动选中态的 SegmentedControl/Actions Job 基础、固定源 Dialog 所有权/无边框 ActionMenu/带边框 SelectMenu/Banner
浮层契约、通用 Filter ActionMenu、Project 可读卡片宽度、Chat 历史流内操作、
PR 单一状态/窄屏 Tab、列表响应式标题/紧凑分支胶囊/行操作及 diff/文件树字体颜色保护约束、强制 UI Components 演练场/属性矩阵契约、
Material 依赖边界、仓库级组件目录
（`ui/components/<feature>/`）与路由页面编排（`ui/views/`）边界，以及逐批
验收门禁；实施顺序和当前状态见
[`docs/ui-github-migration-plan.md`](../ui-github-migration-plan.md)，缺陷记录见
[`bug-history.md`](../../bug-history.md)。

The PR workflow spec includes live filter-trigger anchoring and collision
handling against the visible scroll viewport. / PR 工作流规格包含筛选按钮实时
锚点与可见滚动视口边界避让。
PR menus use inset ActionList rows and complete scroll extents; UI Components
reuses the production toolbar with isolated short/long rule fixtures. / PR 菜单
使用内缩 ActionList 和完整滚动高度，UI Components 以隔离的短/长规则样例复用生产工具栏。
Bordered and borderless Select Menu demos are adjacent in Selection; bordered
menus use the window root and shared adaptive placement without growing layout.
/ 带边框与无边框 Select Menu 示例在“选择”中相邻展示；带边框菜单使用窗口根浮层
与共享自适应定位，展开不增高布局。
PR menus also use a window-root portal in Workflow and the showcase; visible
anchors track scrolling, offscreen anchors dismiss, and internal menu scrolling
stays open. / Workflow 与演练场的 PR 菜单也使用窗口根浮层；按钮可见时跟随滚动，
离屏时关闭，菜单内部滚动保持展开。

The message spec also requires inline notification demos to mount their shared
queue menu at the window root, above section backgrounds and outside scroll clips.
/ 消息规格也要求行内通知示例把共享队列菜单挂到窗口根层，避开章节背景遮挡与滚动裁剪。
Notification queue menus anchor both downward and upward placement to the footer
button, with a 6 px gap, using the shared Floating UI adapter. / 通知队列菜单上下
展开均以底栏按钮为锚点、间隔 6px，复用共享 Floating UI 适配器。

Documents that are not specs:

- `docs/architecture.md`, `docs/architecture.zh.md`: canonical architecture.
- `docs/architecture-structure-audit.md`: dated structure audit and refactoring roadmap.
- `docs/ARCHITECTURE_FOR_AI.md`: compatibility entry point for older agent prompts.
- `docs/settings-tooltip-memory-20260924.md`: dated Settings Tooltip Release A/B,
  repository-wide host audit, prevention gate, and pending Messages A/B.
- `docs/hook-editor-memory-20260926.md`: Hook Dialog shadow attribution,
  rejected lifecycle-only candidate, repository-wide renderer-surface audit,
  source-built Release evidence, and prevention gate.
- `docs/memory_optimization_backlog.md`, `docs/memory_optimization_backlog.zh.md`: measured memory baseline and phased optimization backlog.
- `docs/artifact-preview-performance.md`: dated Release measurements for Chat resize and artifact-preview hosts.
- `docs/ui-github-migration-plan.md`: staged UI implementation checklist and
  user-acceptance gate.
- `bug-history.md`: append-only migration defect ledger.
- `docs/terminal_viewport_iteration_notes.md`: historical terminal investigation log.
- `docs/ai-review-response-timeout-investigation.md`: historical AI Review
  non-streaming response-timeout evidence and threshold-adjustment playbook.
- `docs/workflow-optimization-rollback.md`: rejected Workflow compile-boundary
  experiment, UI regression history, measurements, and rollback scope.

When adding a spec, include scope, invariants, ownership, failure behavior, and a regression checklist. Update this index in the same change.
