# PR Review UI ownership / PR Review UI 职责

## Scope / 范围

This specification owns the compilation and host boundary of the complete PR
Review workspace: Conversation, Commits, Files changed, Diff/Find/selection,
read-only Conflicts and their dialogs. Provider operations, review-session state
and cleanup remain governed by [PR workflow](pr-review.md) and
[PR conflicts](pr-conflicts.md). Compilation isolation is an implementation
boundary; it is not evidence of a performance gain or completed native acceptance.

本文负责完整 PR Review 工作区的编译及宿主边界，包括 Conversation、Commits、
Files changed、Diff/Find/选区、只读 Conflicts 及所属弹窗。Provider 操作、审核会话状态
和清理继续遵守上方两份规格。编译隔离是实现边界，不代表性能已提升或原生验收已完成。

The split currently describes an **unaccepted working-tree candidate**. Its
[A/B record](../archive/maintenance/mainwindow-build-split-20261016.md) retains
failed B3 runtime gates. The [runtime follow-up](../archive/maintenance/pr-review-runtime-20261017.md)
has completed the controlled original-A/B5 comparison: candidate RSS and relative
gates pass, while the unchanged joint strict checker retains original-A failures.
Scoped macOS native file-switch/cache/reentry and live locale checks also pass
on a separate debug fixture. The subsequent [full P6 run](../archive/maintenance/b5-full-p6-20261017.md)
is complete: six paired fixtures and three other soaks pass, while the 15-minute
PR soak fails RSS growth and slope. B5 remains unaccepted; neither short-run
success nor an RSS peak below
300 MiB waives those gates. Do not promote B5 or the whole split to the next
baseline. Preserve original A and both historical results; do not pool absolute
numbers from the old 2× and new 1× display cohorts. The full old native matrix
and physical Cmd-F remain outside this follow-up.

当前拆分仍是**未经采用的工作区候选**。上方 A/B 记录保留 B3 运行时失败；
[运行时后续](../archive/maintenance/pr-review-runtime-20261017.md) 已完成原 A/B5
受控对照，候选 RSS 和相对门槛通过，未改动的联合 strict checker 仍保留原 A 失败。
独立 debug 夹具的限定 macOS 快速选择、缓存/重入及实时语言切换检查也通过。
后续[完整 P6](../archive/maintenance/b5-full-p6-20261017.md) 已完成，六组短对照和
另外三组长时检查通过，但 15 分钟 PR soak 因 RSS 增长和斜率失败。B5 仍未采用，
短对照或 RSS 峰值低于
300 MiB 不能豁免这些门槛；B5 和整批拆分均不晋升基线。保留原始 A 及两次历史
结果，不混合旧 2× 与新 1× 显示批次的绝对数字。旧完整原生矩阵未重跑，物理 Cmd-F
仍按用户要求延期。

The [B6 experiment](../archive/maintenance/b6-pr-soak-20261017.md) is rejected:
its sustained PR RSS growth/slope checks pass, but the three original-A/B6
90-second pairs add 8.73 MiB of final footprint, exceeding the unchanged 8 MiB
allowance. Full P6 was not executed for B6. The current unaccepted B7 candidate
keeps the factory/model boundary and bounded worker, while
`controller/highlight_cache.rs` owns intent and the current lazy grammar slot.
Normal open claims interest before async reads without loading; scheduled work
captures fixed cache/session slot leases. Real close/project reset coalesces one
weak event-loop idle check; only still-idle slots pass to one shared blocking
release drain. Reopen/reload can reclaim the graph before confirmation; old work
cannot initialize replacement slots. Home hiding and commit-tab Back checkpoints
remain distinct from actual close. No input text or per-file parser state enters
the graph; its mutable lazy internals make a graph count unsuitable as a byte
bound. The [completed B7 run](../archive/maintenance/b7-pr-idle-cache-20261017.md)
closes all 40 requested processes in a consistent 2× cohort. Six paired
generic/relative comparisons and all candidate PR gates pass, including the
900-second PR soak. Soaks 65 and 70 also pass; Hook soak 69 fails footprint slope
at 0.979011 MiB/min against 0.5. B7 and the whole split remain unaccepted. The raw
audit confirms evidence integrity and preserves joint strict failures separately:
original A 12, B7 0, global 0. The Hook generic failure remains unassigned; do not
infer that unchanged Hook source makes it unrelated. Original A stays the formal
baseline, all old failures/Release parameters/gates remain, and absolute 2×/1×
results must not be pooled.

[B6 实验](../archive/maintenance/b6-pr-soak-20261017.md) 已拒绝：PR 长时 RSS
增长/斜率通过，但原 A/B6 三对 90 秒的最终 footprint 增加 8.73 MiB，超过不变的
8 MiB 额度；B6 未执行完整 P6。当前尚未采用的 B7 候选保持 factory/model 边界
及有界 Worker，由 `controller/highlight_cache.rs` 持有意图及当前惰性语法槽。
正常 open 在异步读取前声明使用而不加载语法，入队任务捕获固定的缓存/会话槽 lease。
真正关闭/项目重置合并最多一个 weak event-loop 空闲检查，仍空闲的槽才交给单个
共享 blocking 释放 drain；重开/重载可在确认前复用，旧任务不能初始化替换槽。
Home 隐藏和 commit-tab Back 返回点仍不等于真正关闭。图内不保留输入文本或逐文件
解析历史，其惰性内部缓存可变，图数量不是字节上限。
[B7 完整运行](../archive/maintenance/b7-pr-idle-cache-20261017.md) 已在一致的 2×
显示批次关闭全部 40 次请求进程。六组通用/相对短对照及候选 PR 全部门槛通过，
包括 900 秒 PR soak；soak 65、70 也通过，但 Hook soak 69 的 footprint 斜率
0.979011 MiB/min 超过 0.5，B7 与整批拆分仍未采用。raw audit 确认证据完整性，
联合 strict 失败单独保留为原 A 12、B7 0、global 0。Hook 通用门禁失败尚未归因，
不能从 Hook 源码未变推断其与候选无关。正式基线仍为原始 A，全部历史失败、Release
参数和门槛保持不变，2× 与 1× 的绝对结果不得混合。

## Owners / 实现入口

| Owner / 入口 | Responsibility / 职责 |
| --- | --- |
| `crates/usher-ui-pr-review/` | Static `PrReviewFactory` generation, complete i18n bundle and renderer forwarding / 静态 factory、完整语言包与渲染器转发 |
| `ui/features/pr_review/bridge.slint` | Actual `PrReviewBridge` presentation properties and business commands / 真实展示状态及业务命令 |
| `diff_bridge.slint`, `conflicts_bridge.slint` | Feature-owned geometry/Find and conflict APIs / 功能自有几何、Find 与冲突 API |
| `host_bridge.slint`, `factory_host.slint` | Small shell context/summary and retained factory container / 小型主壳上下文、摘要及稳定容器 |
| `surface.slint`, `ui/views/workflow/pr_review_workspace.slint` | One complete route document and its owned children / 唯一完整路由文档及子树 |
| `src/workflow/pr_review/ui.rs`, `ui/{context,input}.rs` | Actual UI-root resolution, window ownership, scalar context and input adaptation / 实际 UI root 访问、窗口归属、标量上下文及输入适配 |
| `controller/presentation.rs`, `controller/actions/presentation.rs` | Synchronous model/header/layout and capability/readiness/locale projection / 同步 model、页头、布局及能力、就绪和语言投影 |
| `controller/actions/{reviews,remote}.rs` | Review commands and captured remote/Hook execution context / 评审命令及已捕获的远端、Hook 执行上下文 |

The short paths in this table are relative to `ui/features/pr_review/` or
`src/workflow/pr_review/`, as appropriate. The stable complete-page entry is
`@pr-review-page`; the shell imports the build-selected `@pr-review` host.

表中短路径分别相对于对应的 `ui/features/pr_review/` 或 `src/workflow/pr_review/`。
完整页面入口为 `@pr-review-page`；主壳通过构建选择的 `@pr-review` 导入宿主。

## Single presentation owner / 单一展示 owner

Rust consumers resolve `PrReviewUiExt::pr_review()` and bind directly to that
root's `PrReviewBridge`, `PrDiffBridge` and `PrConflictsBridge`. The static
application re-exports PR DTOs from `usher-ui-pr-review`, including nested
`WorkflowPrDiffVirtualRow` and token types. Integrated preview uses MainWindow's
corresponding generated types. Do not pass DTOs from a different generated root,
add per-property/callback MainWindow forwarding, or reconstruct a second large
model to cross this boundary.

MainWindow owns navigation, native chrome, the AI panel, status surfaces and
window geometry. `PrReviewHostBridge` exchanges context, review identity,
modal visibility and small header summaries, without PR row models. Its review
identity change closes the shell readiness popup. Feature globals contain UI
projections, not another domain Store. Existing project/request/file generations,
weak-MainWindow delivery and pre-scheduling Agent/provider identity capture stay
at their original event-loop boundaries.

Rust 使用方通过 `PrReviewUiExt::pr_review()` 取得实际 root，直接绑定其三个功能 global。
静态应用从新包重新导出 PR DTO，包含嵌套虚拟行和 token；实时预览使用 MainWindow 对应
生成类型。禁止混用另一生成 root 的同名 DTO、逐属性/回调增加 MainWindow 代理，或为
跨边界重建第二份大型 model。主壳只保留路由、原生 chrome、AI 面板、状态栏和窗口几何；
host bridge 交换小型上下文、审核身份、modal 状态及页头摘要，不持有 PR 行 model。
审核身份变更继续关闭主壳 readiness popup。功能 global 是 UI 投影，不是第二份领域
Store；项目/请求/文件代次、weak MainWindow 回写及入队前身份捕获时点保持原契约。

## Window and document lifecycle / 窗口与文档生命周期

The static composition installs one lightweight factory root against MainWindow's
actual `WindowAdapter`. Save the shell item-tree identity before
`new_with_existing_window`, then restore it immediately after the constructor,
including error results, before binding callbacks or synchronizing context.
Never substitute a standalone hidden window. The registry stores weak handles;
the host factory closure retains the product. Dropping MainWindow must release
that product when no external handle remains.

The transparent host remains visible and noninteractive. Its container mounts
after the first active visit and remains stable. `PrReviewSurface` alone gates
the heavy document with `context.active`; route exit destroys that document and
its transient children. A hidden ancestor must not defer the conditional's
destruction. On the static root's active-to-inactive transition, revoke the
retained generic Tooltip claim and dismiss its transient surfaces immediately;
do not wait for a destroyed metadata anchor's lease timer. Active context updates
must leave current interactions intact, and shell peers keep their own state.
Reentry reads the same feature globals and `ModelRc` identities.
Unloading visual children does not clear review-session caches or invalidate
remote mutations. Explicit review close, unreachable Workflow scope and project
replacement remain separate controller cleanup boundaries.

Keep the host at the existing PR layer: docked sidebar and AI content below it,
shell header/status surfaces above it. Existing root modal shields complete the
titlebar, status-bar and sidebar input boundary. Do not raise all PR content to
fix one dialog. Validate the actual backdrop and first-click behavior across
native chrome, sidebar, AI splitter/panel and status controls.

静态装配使用 MainWindow 实际 `WindowAdapter` 创建一个轻量 factory root；构造前保存
shell item-tree 身份，构造后立即恢复，错误结果也先恢复，再绑定 callback 或同步上下文。
禁止替换为独立隐藏窗口。注册表只保存 weak，host factory closure 保留产品；没有外部
handle 时，销毁 MainWindow 必须释放产品。透明宿主保持可见且不拦截交互，首次进入后
保留容器，由 Surface 的 `context.active` 条件单独销毁/重建重文档，不能因隐藏祖先而
延迟析构。重入继续使用同一个功能 global 和 `ModelRc`。视觉子树卸载不等于清空审核
会话、缓存或远端 mutation；会话关闭与项目、路由域清理仍由控制器分别管理。
静态 root 从 active 变为 inactive 时立即撤销保留的通用 Tooltip claim 并关闭所属
transient，不等待已销毁 metadata anchor 的租约计时器；active 上下文更新不应打断
当前交互，也不能顺带清除主壳其他浮层状态。
保留既有层级及根 modal 盾，不通过整体提高 PR 层级修补局部弹窗；验收必须覆盖真实
遮罩、原生 chrome、侧栏、AI 面板/分隔线和状态控件的首次点击。

## Input, overlays and locale / 输入、浮层及语言

The factory's full-window FocusScope does not focus on click or Tab. Non-Tab
application shortcuts cross the host callback during capture; Tab/Backtab bubble
after local controls handle dismissal, then use `src/focus_navigation.rs` to
traverse the mounted shell. Modal state blocks traversal into shell controls. Native shortcut monitors can
run before queued Slint `changed` handlers: the shared shortcut entry and PR Tab
adapters synchronously refresh only the PR modal boolean from the actual feature
root before reading MainWindow's combined modal guard. Opening and closing must
both take effect in that same call; other features' modal state remains intact.
This input check does not flush layout or copy models.
Local exclusive claims stamp the expected host generation before publishing;
peer host claims dismiss factory popovers without echoing the new opener's claim.
Placement adapts rectangles to the existing shared Floating UI bridge.

Fixed actions use 30 literal-kind leaves, `PrReviewTooltipState`, and one
factory-root `GitHubPrReviewTooltipOverlay` containing exactly one
`GitHubTransientFloatingOverlayHost`. Each kind owns independent, mounted I18n
measure/paint surfaces. Labels do not cross delegates. PR paths, timestamps and
other runtime metadata retain the generic two-slot portal exception; fixed
actions must not use that exception. Stale claims, route/modal changes, viewport
changes and lease expiry revoke tips. Follow the full [Tooltip matrix](tooltips.md).

The factory builds the complete shared i18n catalog in the same language order
as every other enabled root. Runtime locale changes refresh both Slint bindings
and feature-owned Rust projections without replacing models just to change copy.
See [i18n](i18n.md) for source/output watchers and shared-root regressions.

factory 外层 FocusScope 不通过点击或 Tab 获取焦点。非 Tab 快捷键在 capture 阶段转交
既有应用入口；Tab/Backtab 等局部控件关闭并恢复焦点后再 bubble，通过共享宿主遍历
跨越 factory 边界，modal 时禁止穿入主壳。原生快捷键可能先于 Slint changed 发布；
共享快捷键入口与 PR Tab adapter 在准入前从实际 owner 同步唯一 modal 布尔值，打开和
关闭均在同一调用生效，不能清除其他功能的 modal，也不推进布局或复制 model。互斥 claim 先登记预期宿主代次，避免回声关闭
刚打开的菜单；几何复用共享 Floating UI adapter。30 种固定 Tooltip 动作用 literal 叶子、
单个 transient Portal 和各自常驻 I18n 测量/绘制 Surface；路径、时间等动态 metadata
继续使用通用双槽例外，不能用于固定动作。完整语言包、语言顺序、即时刷新及不复制
model 的要求与国际化规格一致。

## Build modes / 构建模式

| Mode / 模式 | Compiled ownership / 编译归属 |
| --- | --- |
| Default, Release, static `ui-components` | `usher-ui` imports `factory_host.slint`; the complete visual graph is generated once by `usher-ui-pr-review` / 主壳仅 host，完整视觉图独立生成一次 |
| `live-preview` | `usher-ui` imports `integrated.slint`; `PrReviewUiRoot = MainWindow`, and `usher-ui-pr-review/integrated-preview` skips static generation / 使用集成图，不另编静态产品 |
| `dev-shell` | Both UI packages select the protocol-compatible empty page via `@pr-review-page`; navigation normalizes PrReview to Workflow / 两包选择同一空页面协议，路由回退 Workflow |

The root forwards its selected renderer to the new package. Its dev opt1/no-debug
and test no-debug overrides preserve the generated MainWindow package policy;
Release profile parameters remain unchanged. Slint library-module metadata alone
is not a replacement for the factory boundary or shared global-instance ownership.
Isolation applies to the page's independent visual inputs. Shared DTO definitions,
native-titlebar header components and shared widgets still belong to every root
that imports them; edits there may invalidate both MainWindow and the PR package.

根包把选定渲染器转发给新包；dev opt1/无 debug、test 无 debug 延续原生成 UI 策略，
Release 参数不变。library-module metadata 本身不等于视觉编译隔离或共享 global 实例。
隔离针对页面独立视觉输入；共享 DTO 定义、原生标题栏页头组件及共享控件仍由各使用
root 导入，修改这些输入仍可能使 MainWindow 与 PR 包同时失效。

## Regression requirements / 回归要求

- Run `pr_review_factory_ui`, existing PR core/header/status/list and AI response
  integration tests, the `workflow::pr_review` library tests, the
  `shortcuts::pr_review_modal_tests` native-entry regression and the shared-root
  i18n test. Preserve actual-adapter/shell identity, one model, multi-window
  isolation, heavy-document teardown, locale reentry, immediate generic/fixed
  Tooltip revocation, two-way real popover exclusivity, real keyboard/Tab crossing
  and weak-handle release controls.
- Run architecture/UI-policy positive and negative fixtures and the two i18n
  build-path Python suites. Inspect static, integrated-preview and dev-shell
  feature ownership, including nested DTO types and original-input watches.
- On the native app, repeat PR/Conflicts/Find/dialogs, scroll/resize, keyboard and
  adjacent Tooltip actions in English and a packaged non-source locale. Repeat
  route exit/reentry with the AI panel and sidebar. Record unexecuted cases.
- Record controlled baseline/candidate build A/B and the adopt/reject decision
  separately. Source size, passing compilation and testing-backend assertions
  do not prove compile-time gains, native painting or stable RSS.

以上分别覆盖定向回归、结构正反例、原生多语言交互与受控构建 A/B；未执行项须明确
记录，不能把源码体积、编译或 testing backend 通过当作性能、原生绘制或 RSS 结论。
