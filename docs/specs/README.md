# Specifications / 规格索引

Specifications describe stable behavior and invariants. Architecture documents explain ownership and flow; historical notes explain how a problem was investigated. Do not mix those three roles.

Spec 描述稳定行为和不变量；架构文档解释职责与数据流；历史记录描述问题排查过程。三者不要混写。

| Spec | Status | Code owners |
| --- | --- | --- |
| [Exact virtual scrolling](virtual-scrolling.md) | Shared contract for variable-height documents and stable thumb geometry / 可变行高文档与稳定滑块几何通用契约 | `crates/usher-virtual-list/`, `src/app_runtime/latest_compute.rs`, `ui/components/github_scroll_view.slint`, feature layout adapters |
| [HTTP pipeline](http.md) | Explicit response budgets and bounded error reads / 显式响应预算与有界错误读取 | `src/http/mod.rs`, all HTTP callers |
| [Cross-page state store](state-store.md) | Current: bounded latest snapshots and revision barriers / 当前：有界最新快照与版本屏障 | `crates/usher-state-store/`, all cross-page state owners and UI bridges |
| [Shared search navigation](search.md) | Current / 当前 | `crates/usher-search/`, feature Find adapters, PR `find_bridge.rs`, `controller/diff_bindings.rs`, `PrDiffBridge` |
| [Internationalization](i18n.md) | Implementation current; full-route visual QA pending / 实现当前，全路由视觉验收待完成 | `i18n/`, `crates/usher-i18n/`, `src/i18n.rs`, all user-facing copy |
| [AI Agent configuration](ai-agent.md) | Current / 当前 | `src/settings.rs`, `src/ai/agent.rs`, `ui/views/settings.slint` |
| [AI Code Review](ai-code-review.md) | Current: bounded task persistence and UI delivery / 当前：有界任务持久化与 UI 投递 | `crates/usher-ai-review/`, `src/ai_review/` (controller, sessions, task persistence), `src/agent_response/`, PR list/detail and Settings bridges |
| [Diagnostic logging](diagnostics.md) | Current: grouped settings and legacy-key migration / 当前：配置分类与旧键迁移 | `src/settings.rs`, `src/diagnostics.rs`, `src/logger/`, `src/memory_diagnostics.rs`, `src/terminal/`, Settings UI and bridges |
| [Chat behavior](chat.md) | Current: history boundary anchoring, bidirectional viewport coverage, and mode-switch tail mounting / 当前：历史边界锚定、双向视口覆盖与模式切换尾部挂载 | `src/chat/` (interactive, viewport policy, message model), `ui/views/chat.slint`, `ui/components/chat/`, `ui/main.slint` |
| [Artifact preview framework](artifact-preview.md) | Current: cancellable exact worker layout and virtual code lines / 当前：可取消的后台精确布局与代码行虚拟化 | `crates/usher-preview/`, `src/artifact_preview/`, `ui/components/artifact_preview/`, Settings and all preview callers |
| [Project repository metadata](project-repository.md) | Current: explicit Hook opt-in with disabled-row restoration, bounded Logo retries and projection / 当前：Hook 显式选择与禁用行恢复、Logo 有界重试与投影 | `src/project/mod.rs`, `src/git/`, `src/codereview/api/` |
| [Pull request workflow](pr-review.md) | Current / 当前 | `src/workflow/pr_list.rs`, `src/workflow/pr_review.rs`, `src/workflow/pr_review/`, `src/git/`, `ui/features/pr_review/`, `ui/views/workflow/`, `ui/components/workflow/` |
| [Reusable job runs](jobs.md) | Current: typed lifecycle/durations, bounded output, virtualized logs, safe blank rows and fixed run pages with output-only scrolling / 当前：类型化生命周期/耗时、有界输出与日志虚拟化、空行安全处理及固定运行页/仅输出区滚动 | `src/jobs/`, `ui/views/job_run.slint`, `ui/components/jobs/`, feature controllers |
| [Application Hooks](hooks.md) | Current: bounded syntax text and native cursor/selection; macOS cursor visual QA passed / 当前：有界语法文本与原生光标/选区，macOS 光标视觉验收通过 | `src/hooks/`, `src/project/`, Settings, PR/AI completion sources, notifications and shutdown drains |
| [Tag workflow](tag.md) | Current / 当前 | `src/workflow/tag.rs`, `src/workflow/tag/`, `src/workflow/state.rs`, `ui/views/workflow/` |
| [Terminal behavior](terminal.md) | Bounded I/O and asynchronous session shutdown barrier / 有界 IO 与异步会话退出屏障 | `src/terminal/`, `ui/components/drawer/terminal_drawer.slint`, `ui/main.slint` |
| [Application keyboard shortcuts](shortcuts.md) | Current / 当前 | `src/shortcuts/`, `ui/main.slint`, `src/platform/` |
| [Application status bar](status-bar.md) | Current / 当前 | `ui/components/status_bar.slint`, `ui/main.slint`, shortcut/notification/network/update state owners |
| [Application message center](messages.md) | Current: 20-record pages, filter scroll preservation and row-bounded corner clipping / 当前：每页 20 条、筛选保留滚动位置与行级圆角裁剪 | `crates/usher-messages/`, `src/message_center.rs`, `src/notification.rs`, message/status UI |
| [GitHub/Primer UI design](ui-design.md) | Current / 当前 | `ui/theme/github_theme.slint`, `ui/`, `ui/assets/icons/`, all UI contributors |
| [Tooltip implementation contract](tooltips.md) | Mandatory: factory isolation, centered text/anchor and viewport avoidance / 强制阅读：工厂隔离、文字与锚点居中、视口避让 | Shared Tooltip/Float/IconButton, factory-owned actions and root portals |

The UI design specification includes the shared small Primer Label geometry,
transparent neutral variants and Hook status usage. / UI 设计规范包含共享 small
Primer Label 的几何、透明中性变体及 Hook 状态用法。

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
Dialog/borderless ActionMenu/bordered SelectMenu/form Select popup (shared Tooltip
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
UnderlineNav/带滑动选中态的 SegmentedControl/Actions Job 基础、Dialog/无边框 ActionMenu/带边框 SelectMenu/Banner
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
