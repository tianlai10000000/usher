# UI Components playground / 组件演练场

## Scope and entry points / 范围与入口

The development-only `ui-components` feature exposes the gallery route and local
Preview fixtures. Default builds and release scripts exclude it. This contract
covers compilation, composition and host integration; component behavior remains
owned by [UI design](ui-design.md), [Tooltips](tooltips.md) and [i18n](i18n.md).

`ui-components` 只用于开发，包含演练场路由与本地 Preview 夹具。默认构建与发布脚本
不启用它。组件行为继续遵守对应 UI、Tooltip 与国际化契约。

| Owner / 职责 | Entry / 入口 |
| --- | --- |
| Static compilation / 静态编译 | `crates/usher-ui-components/`, `ui/views/ui_components_factory.slint` |
| Host geometry and lifetime / 宿主几何与生命周期 | `ui/features/ui_components_bridge.slint`, `ui/main.slint` |
| Event-loop adapters / Event-loop 桥接 | `src/ui_components/`, existing Toast/Notification/Dialog controllers |
| Gallery composition / 演练场编排 | `ui/views/ui_components.slint`, `ui/components/showcase/` |
| Feature selection / Feature 选择 | `Cargo.toml`, `crates/usher-ui/build.rs`, `ui/features/ui_components_*.slint` |

The navigation section compares the real `GitHubUnderlineNav` `inset` and `flush`
variants. Both rows share localized items but keep independent owners and
selection state; changing one example must not select an item in the other.

导航分类使用真实 `GitHubUnderlineNav` 对照展示 `inset` 与 `flush`。两行共享
本地化 Item model，但 owner 与选中态独立，操作其中一行不得改变另一行的选择。

## Compilation boundary / 编译边界

1. Ordinary `cargo run --features ui-components` compiles the gallery into the
   optional `usher-ui-components` package. MainWindow imports the lightweight
   host contract, never the static gallery implementation. Editing only a
   showcase file must keep `usher-ui`'s library and generated Rust unchanged.
   MainWindow's transitive Slint inputs must contain no `ui/components/showcase/`
   source. Reusable Tooltip geometry lives in
   `ui/components/github_fixed_tooltip_anchor.slint`; PR-specific literal action
   anchors live in `ui/components/workflow/github_pr_factory_tooltip_anchors.slint`.
   Production widgets must not import those helpers through the showcase tree.
2. Shared widgets remain real dependencies of every importing root. A shared
   PageView edit must invalidate MainWindow and the playground; an optimization
   must not suppress this rebuild. Rust-only i18n inputs must not invalidate any
   UI package, including this optional package.
3. `live-preview,ui-components` keeps the integrated interpreter graph so gallery
   edits reload inside the running process. `integrated-preview` makes the
   optional package inert rather than compiling an unused static gallery.
   The Cargo feature combination selects this path; setting
   `SLINT_LIVE_PREVIEW` alone does not move the static factory into that graph.
4. Every static root receives the complete Slint-visible registry and locale
   inputs from `usher-i18n-build`, with identical language ordering. Generated
   message arrays and their indexes remain module-local. A smaller factory-local
   source catalog is invalid.

普通静态命令隔离演练场编译；共享组件改动仍正常传播。实时预览保留集成图，不得因
隔离静态编译而丢失演练场热重载。新 factory 同样不得依赖 Rust-only catalog。

## Composition and lifecycle / 编排与生命周期

- One transparent full-window factory contains the document, its single scroll
  viewport, Select/filter/notification/PR/Tab overlays and Tooltip portals. Page
  and overlay must share the same local global identities. Do not split them
  into separate factories or insert a variable-height factory into the shell's
  Flickable.
- `UiComponentsHost` publishes actual window bounds, content viewport and top
  padding through the feature-owned `UiComponentsBridge`. Rust binds that global
  directly. The product copies the application's font family and weight 445,
  and mirrors current theme on creation and subsequent theme changes.
- Mount the product lazily on the first visit and retain that cross-crate root.
  `src/ui_components/mod.rs::create_page` must create it with the actual host
  `WindowAdapter`, so child-tree destruction releases caches in the renderer
  that paints those children. In the same event-loop turn, save MainWindow's
  item-tree identity, call the shared-window constructor, and immediately
  restore the saved shell root before handling the result or synchronizing and
  binding application callbacks. The constructor temporarily installs its own
  root; restoration must also run when it returns an error. Do not yield or
  dispatch application work while that temporary root is installed.
  Route exit closes local transients and releases its route document and menu
  overlay children; Tooltip portals remain mounted. Re-entry creates a fresh
  gallery document at scroll position zero.
- Keep the existing content insets, section-directory absolute-to-viewport
  conversion, return-to-top links and bottom scroll reachability. The shell's
  page viewport must be hidden while the independent gallery is active.
- Host dismissal, text-input popover dismissal and post-dispatch pointer release
  must reach the factory. Root notification, Dialog and modal chrome retain
  their existing ordering and application ownership.
  [`src/transient_input.rs`](../../src/transient_input.rs) owns classification
  through the actual post-dispatch window hook on every platform. A scrollable
  Flickable can still delay child press delivery beyond that hook. Classify each
  MainWindow left press immediately; only a miss retains its original logical
  position and route. After its actual left release dispatches, consume that
  pending press and classify the original point once more only if the route is
  unchanged. A successful initial classification must not repeat. Pointer exit
  or window deactivation cancels pending work; other adapters/buttons do not
  consume it. Do not classify release coordinates or add timers/polling.
  Installation follows `Window::show()` and filters by actual MainWindow adapter
  identity. macOS's native monitor forwards chrome/release events rather than
  classifying inputs; its production hook avoids duplicate pointer forwarding.
  The public test entry still forwards both pointer phases.
- Exclusive popover claims cross both global graphs through
  `src/ui_components/popover.rs` and `UiComponentsBridge`. A local claim closes
  host peers and stamps the expected next host generation before claiming, so
  synchronous observers recognize its ownership; the matching echoed
  generation must not dismiss its local opener. An external host claim closes
  factory popovers through `dismiss(false)`, without issuing another exclusive
  claim or dismissing Dialogs/Toasts. Inactive factory claims do not affect the
  host. Shared [`GitHubActionMenu`](../../ui/components/github_action_menu.slint)
  must claim on initial `open: true` as well as later openings, through one
  idempotent initialization/change path that also applies scroll reveal and
  focus. Stamping precedes the claim; duplicate init/change notifications must
  not claim twice or close the new opener.
- `src/ui_components/keyboard.rs` forwards ordinary application shortcuts from
  the factory's capture scope. Tab/Backtab bubbles first so an open Select can
  close, then uses `src/focus_navigation.rs` for mounted-host traversal shared
  with Messages. Only this static factory enables its local
  `GitHubSelectState.restore-before-tab`; ordinary MainWindow/Settings and the
  integrated playground keep the default `false` policy. With that opt-in the
  Select requests opener restoration on Tab/Backtab, and the host owner flushes queued
  close/focus handlers so the opener is restored before reading the current
  item and choosing its next/previous peer. Closing must neither restart at the
  directory nor stop on the opener. When a shell control owns focus,
  MainWindow's capture scope
  instead calls `UiComponentsBridge.key-pressed` to enter that same traversal.
  Preserve both entry points so Tab/Shift+Tab and standalone Backtab can cross
  from the factory to the shell and back. A shared renderer alone does not
  create the shell's parent FocusScope chain.
- Fixed factory icon actions use distinct literal-kind leaves and one fixed
  Tooltip portal with separate mounted measure/paint surfaces. Dynamic PR
  metadata retains the generic portal. The static root enables its local
  `FactoryTooltipState`; the integrated main tree retains ordinary Tooltip
  publishing. Apply the full Tooltip acceptance matrix to the static product.

页面、滚动和浮层属于同一个全窗口 factory；首次访问后保留工厂根，离开路由释放
文档并清理临时状态。宿主几何、字体、主题、dismiss 与 pointer release 都必须同步。
工厂必须使用实际负责绘制的宿主 `WindowAdapter`；首次创建在同一 event-loop turn
保存并立即恢复 MainWindow 根，随后才处理构造结果、同步状态和绑定应用 callback。
返回错误也须先恢复，临时根安装期间不得让出执行或调度应用工作。
固定操作的 Tooltip 遵守工厂隔离规则，不能仅靠通用字符串 Portal 交付。
Tab 桥接须保留工厂冒泡与宿主 capture 两侧入口，支持 Tab/Shift+Tab 与独立 Backtab
双向跨越 factory 边界。Select 先请求恢复触发器，宿主遍历在读取当前焦点前完成
排队的关闭/焦点变更，再从触发器继续，不能跳回目录或仅停在触发器。
该恢复策略仅由静态演练场在本地 global 显式开启；MainWindow、Settings 与集成
演练场默认保持 `restore-before-tab: false`，不得向其它宿主传播此 opt-in。

## Application feedback / 应用反馈

`src/ui_components/feedback.rs` maps generated DTOs and enums explicitly, then
invokes the existing MainWindow managers on the event loop. Toast options,
notification category/source/tag/actions/native-delivery, and Dialog dimensions
and modality must survive the boundary. A notification still enters the real
Inbox once. The factory must not own another notification queue controller,
persistence worker, reducer, HTTP client or runtime. Floating placement forwards
to the installed host adapter; no placement algorithm is copied.

## Acceptance / 验收

- Use the same warmed dev-profile command before/after, three semantic gallery
  edits, unchanged runs and bin-only Rust edits. Exclude initialization and source
  restore builds. Capture raw timings, package freshness and generated/library
  hashes and MainWindow's watched Slint inputs. Require the repository
  build-iteration acceptance threshold; a comment edit alone cannot establish a
  semantic compilation boundary, and unchanged output alone cannot prove that
  Cargo preserved a compilation unit.
- Run a shared-component positive control, the integrated live-preview check,
  default/full static checks, feature-specific bridge/locale tests and repository
  validation. Keep native acceptance distinct from testing-backend assertions.
  Integration fixtures share `tests/support/ui_components.rs`: install the
  static factory explicitly, select its real document (the integrated root for
  live-preview), and reveal mounted controls through real wheel events. Use
  `Window::set_size` for backend resize fixtures so the adapter's size and the
  dispatched geometry agree before shared-window construction.
- Assert that the factory and MainWindow use the same actual adapter and that
  MainWindow retains its original item-tree identity after creation, route
  cycles and factory drop. Keep a standalone `new()` negative control whose
  adapter differs from the host. Inspect painted text and icons after locale/model
  updates and repeated route re-entry. Correct catalog values alone do not
  establish correct native rendering; do not change PO entries to mask a cache
  ownership defect.
- Keep the Preview close regression in `src/ui_components/tests.rs`: click the
  real root-owned close action before creation, after an unmounted constructor
  and drop, and after Gallery/Home/Messages transitions. Its Preview dismissal
  callback and the native window close callback must remain installed. Native
  acceptance must also repeat shell route transitions in a host-only control
  that has never opened the gallery; distinguish native results from backend
  callback and pointer-routing assertions.
  Preview 验收同时覆盖从未打开演练场的宿主对照与 factory 生命周期，保留真实关闭
  点击及两类关闭 callback；testing backend 通过不能代替原生验收。
- On macOS inspect the compiled factory in source and packaged non-source locale:
  directory/return links, scrolling and bottom reachability, narrow/wide layout,
  Select/PR/notification menus, input dismissal, Toast, durable Inbox and Dialog,
  theme/locale switches, Tooltip A→B→A and short→long→short, route exit/re-entry.
  Repeat route cycles to check cleanup. Record unexecuted platforms/scenarios.
  Traverse Tab/Shift+Tab and standalone Backtab across the factory/shell boundary
  in both directions, including closing an open Select before advancing focus.
  With a menu open, one click into an unfocused input must focus it and dismiss
  the popover; dragging a selection from inside the field and releasing outside
  must preserve text focus and selection.
  Include both short clicks released before Flickable forwarding and held
  presses delivered internally before release, plus route changes, pointer
  exit/deactivation, repeated releases, other buttons and other window adapters.
  Exercise both directions of exclusivity: opening root Version/Network closes
  an old factory menu, and opening a factory menu closes an old root popup while
  keeping its own opener open. Include a newly mounted `open: true` peer and
  close/reopen. Check no echo loop and no inactive-route claim.
- Dated raw evidence and adopt/reject decisions stay local under ignored
  `docs/validation/` or verified `.local-archives/` packages; they are not project
  regression tests or submission inputs. Recovery is documented in
  [local archives](../local-archives.md#validation-experiments-20261016).
  This spec does not contain timing diaries or promise a cold-build speedup.
  带日期的原始证据与采用/否决记录仅在本地保存，不作为项目回归测试提交；归档恢复
  方式见上述索引。
