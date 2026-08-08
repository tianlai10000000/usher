# GitHub/Primer UI Design Specification / GitHub/Primer UI 设计规范

## 1. Scope / 范围

This specification is the stable visual and interaction contract for every
user-facing Slint page and application-owned reusable component under `ui/`.
It covers naming, tokens, layout, controls, overlays, feedback, accessibility,
responsive behavior, icons, light/dark themes, and validation.

本文是 `ui/` 下所有 Slint 页面和应用自有复用组件的稳定视觉与交互契约，
覆盖命名、令牌、布局、控件、浮层、反馈、无障碍、响应式行为、图标、
明暗主题及验证要求。

The staged implementation checklist is maintained separately in
[`docs/ui-github-migration-plan.md`](../ui-github-migration-plan.md). A batch
may change this specification only when the reference contract changes; it
must not weaken the contract to fit unfinished migration work.

分批实施清单单独维护在
[`docs/ui-github-migration-plan.md`](../ui-github-migration-plan.md)。只有参考
契约发生变化时，批次才可修改本规范；不得为了迁就尚未完成的迁移而降低要求。

## 2. Source of truth / 事实来源

Use the following evidence in order:

1. The matching interaction in GitHub's current product UI, checked in light
   and dark mode when it is accessible.
2. Current [Primer Product UI](https://primer.style/product/) foundations,
   components, and patterns.
3. Official implementation sources:
   [Primer React](https://github.com/primer/react),
   [Primer Primitives](https://github.com/primer/primitives), and
   [Octicons](https://github.com/primer/octicons).
4. If no matching reference exists or authoritative references conflict,
   record the question and obtain a product decision. Do not invent a hybrid.

查证顺序如下：

1. GitHub 当前产品界面中的对应交互；条件允许时同时核对明暗主题。
2. 当前 [Primer Product UI](https://primer.style/product/) 的基础、组件和模式。
3. 官方实现源码：[Primer React](https://github.com/primer/react)、
   [Primer Primitives](https://github.com/primer/primitives) 和
   [Octicons](https://github.com/primer/octicons)。
4. 没有对应参考或权威资料冲突时，记录问题并取得产品决策，禁止自行拼接近似样式。

## 3. Naming and ownership / 命名与归属

- Every application-owned component anywhere under `ui/` that implements a
  GitHub/Primer visual pattern must use the `GitHub` prefix in its exported
  and local component name, for example `GitHubButton`, `GitHubDialog`, and
  `GitHubTooltip`. The audit boundary is the complete application-owned UI
  tree, not only `ui/components/`.
- Existing GitHub-style components without this prefix are migration debt,
  not naming exceptions. Rename them and every import/usage in the designated
  migration batch.
- Every visual control, row, card, menu, tab, panel, dialog, banner, field, or
  other component-library declaration belongs in `ui/components/<feature>/`.
  Page composition belongs in `ui/views/`; a view file may declare only its
  route page entry, named `GitHub*Page`, and must import visual building blocks
  from `ui/components/`. Pages must not declare local visual components or
  recreate an existing shared pattern.
- Shared design globals are `GitHubTheme` and `GitHubMetrics`, defined only in
  `ui/theme/github_theme.slint`.
- Slint callback/property names remain feature-oriented unless they expose a
  reusable visual component contract; the prefix rule does not apply to Rust
  business types or route names.
- A domain component does not receive the prefix merely because it consumes a
  `GitHubTheme` token. Use the prefix only when the component itself implements
  a documented GitHub/Primer visual pattern; non-GitHub visual components keep
  domain names but still live under `ui/components/`.

- `ui/` 下所有实现 GitHub/Primer 视觉模式的应用自有组件，无论导出还是局部
  声明，都必须使用 `GitHub` 前缀，例如 `GitHubButton`、
  `GitHubDialog`、`GitHubTooltip`；审计范围是完整应用 UI 树，不能只检查
  `ui/components/`。
- 现有 GitHub 风格组件若没有该前缀，属于待迁移问题，不是命名例外；必须在指定
  批次中同步重命名组件及全部导入和用法。
- Control、Row、Card、Menu、Tab、Panel、Dialog、Banner、Field 等所有视觉
  组件声明统一放在 `ui/components/<feature>/`。`ui/views/` 只保留路由页面
  编排，每个 view 文件只能声明一个命名为 `GitHub*Page` 的页面入口，并从
  `ui/components/` 引入视觉积木；页面不得声明局部视觉组件或重做已有模式。
- 共享设计 global 固定命名为 `GitHubTheme` 和 `GitHubMetrics`，且只在
  `ui/theme/github_theme.slint` 定义。
- Slint callback/property 仍按功能语义命名；除非它们属于复用视觉组件的公开契约，
  `GitHub` 前缀不扩展到 Rust 业务类型或路由名。
- 组件仅仅消费 `GitHubTheme` 令牌，并不构成加前缀的理由。只有组件自身实现了
  文档化的 GitHub/Primer 视觉模式时才使用 `GitHub` 前缀；非 GitHub 视觉组件
  保持领域命名，但仍统一放在 `ui/components/`。

## 4. Tokens / 设计令牌

Product UI consumes semantic roles from `GitHubTheme`; it must not choose a raw
hex color or branch on `AppState.effective-dark-mode`. Raw functional colors
are owned by `ui/theme/github_theme.slint`.

产品 UI 必须使用 `GitHubTheme` 的语义角色，禁止自行选择十六进制颜色或基于
`AppState.effective-dark-mode` 写明暗分支。功能色原始值统一归
`ui/theme/github_theme.slint` 管理。

Required semantic families:

- canvas, foreground, muted/subtle foreground, border, focus, selection;
- accent, success, attention, danger, done, open, and closed states;
- default, primary, danger, transparent, disabled, hover, active, selected,
  and inverse controls;
- overlay, backdrop, shadow, tooltip, scrollbar, and code-review/diff roles.

`GitHubMetrics` is the only shared source for Primer geometry:

- base sizes: 2, 4, 6, 8, 12, 16, 20, 24, 28, 32, 36, 40, 44, 48, 64, 80,
  and 96 px;
- radius: 3, 6, and 12 px; full radius only for pills/circles;
- controls: 24, 28, 32, 40, and 48 px;
- focus outline: 2 px;
- overlay widths: 192, 320, 480, 640, and 960 px;
- breakpoints: 320, 544, 768, 1012, 1280, and 1400 px.

Repeated geometry must reference `GitHubMetrics`. A one-off product layout
value may remain local only when its purpose is named and it is not pretending
to be a Primer primitive.

重复几何值必须引用 `GitHubMetrics`。只有具有明确产品布局含义、且不冒充 Primer
基础令牌的一次性数值才可留在局部。

## 5. Typography and icons / 字体与图标

- Normal UI inherits the platform CJK-capable family selected by
  `MainWindow`: the native `.AppleSystemUIFont` on macOS,
  `Microsoft YaHei UI` on Windows, and `Noto Sans` elsewhere.
- Terminal content keeps its terminal font. Diff source and file-tree code may
  keep their documented monospace/platform fonts; other PR-review UI inherits
  the application family.
- PR shell, header, navigation, and interaction fixes must not implicitly
  restyle the Files changed source or file tree. Their colors and fonts are an
  independent review-content contract and may change only through an explicit
  diff/file-tree design decision with dedicated visual regression coverage.
- Default body text is 14 px. Use 12 px for secondary labels and 16 px or the
  documented heading tokens for hierarchy. Control labels normally use weight
  600; body text uses 400.
- Use the matching official Octicon from `ui/assets/icons/`. Do not substitute
  Unicode glyphs, emoji, Material icons, or hand-drawn approximations. Register
  new upstream assets in `ui/assets/icons/OCTICONS.md`.
- The Codex-style application sidebar is the documented exception: it uses
  the official round-stroke Lucide assets registered in
  `ui/assets/icons/LUCIDE.md`, because filled Octicons have a visibly heavier
  optical weight than the desktop reference at this size.
- Repository-host provider marks registered in `ui/assets/icons/PROVIDERS.md`
  are identity assets rather than command icons. They may only identify the
  corresponding provider and do not create a general non-Octicon exception.
- `ui/components/title_bar.slint` may retain `settings.svg` and `about.svg` as
  the two explicit native-title-bar product exceptions. No other component may
  introduce a non-Octicon command icon without a documented product decision.

## 6. Layout and responsive behavior / 布局与响应式行为

- Build page hierarchy with Primer `PageLayout`, `Stack`, `Box`, `PageHeader`,
  `UnderlineNav`, and `ActionList` patterns where the product concept maps.
- Use an 8 px condensed, 16 px normal, or 24 px spacious rhythm. Preserve the
  reference component's internal 4/6/8/12 px geometry where specified.
- Content should adapt at Primer breakpoints. Do not rely on one fixed desktop
  width; verify the application minimum width and at least one wide layout.
- Project grid columns are derived from the constrained content width, not the
  outer window width. A card keeps at least 280 px of readable width before a
  new column is introduced. Project mounts only the active list or grid visual
  representation; switching modes may replace that visual tree once, but native
  resize must not keep evaluating both representations. Provider corner ribbons
  keep their band, 10 px identity mark, and caption vertically centered in the
  45-degree geometry, but ship that fixed identity as an application-owned SVG
  and display it without a runtime rotation. Small live `Text` must not be placed
  below a rotated transform with the FemtoVG renderer because rotation switches
  glyphs from the hinted atlas to low-DPI outline rendering.
- Chat history pagination is part of the message flow after the virtual top
  spacer. It reserves layout height and must never float over the oldest
  mounted message.
- PR review exposes one visible state source in the summary row. The header
  action strip must not repeat the same Open/Closed/Merged readiness label.
  Its three counted tabs follow the shared Primer UnderlineNav geometry at every
  width: content-sized items, left alignment, a 16 px inset on both sides, and
  explicit 4 px inter-item gaps. They never stretch to divide the row equally;
  the full-width baseline remains edge-to-edge, and nonessential trailing diff
  statistics yield before tabs need overflow.
- Text must follow the reference wrapping/truncation rule. Lists and scroll
  views must not rebuild large models while scrolling or resizing.
- Keep hover hit targets mounted. Change opacity/state instead of
  conditionally creating an interactive node.
- Mount full-window overlays at the root layer in `ui/main.slint`.

## 7. Controls and interaction states / 控件与交互状态

Every interactive component must implement the applicable reference states:
default, hover, pressed, focus-visible, selected, disabled, loading, error,
expanded/collapsed, and success confirmation.

每个交互组件必须按参考实现适用状态：默认、Hover、按下、键盘可见焦点、选中、
禁用、加载、错误、展开/收起及成功确认。

- Use Primer control sizes and a 6 px default control radius unless the
  reference specifies another token.
- Use semantic focus color with a 2 px outline. Show the outline for keyboard
  focus (`focus-visible` behavior), not as a persistent pointer-click
  selection border. Focus must not rely on color change alone and must not be
  clipped.
- Icon-only controls require an accessible action label and a persistent hit
  target. They show that label in a tooltip except for the sidebar About
  action and the familiar Dialog, Notification, and Toast dismiss controls,
  which keep the accessible label without showing a visual tooltip.
- Destructive actions use danger semantics and require confirmation when the
  consequence is difficult to reverse.
- Loading prevents duplicate starts and preserves the control's dimensions.
- Disabled state must prevent input, not only lower opacity.
- Shared controls expose Slint accessibility roles, enabled/checked/value
  state, and a label derived from visible text or the required IconButton
  tooltip. Buttons, links, tabs, disclosure rows, and choices support
  Enter/Space; overlays support Escape when dismissible.

## 8. Shared component contracts / 共享组件契约

- `GitHubButton`: Primer default/primary/danger action variants, medium 32 px
  control geometry by default, semibold single-line label, stable loading
  dimensions (width measured with the visible weight-600 label plus a reserved
  leading spinner slot), duplicate-start protection, Enter/Space activation,
  and a visible focus outline.
- `GitHubIconButton`: the shared basis for every icon-only action; centered
  Octicon, default/primary/danger/invisible states, small/medium/large control
  geometry, loading support, Enter/Space activation, focus outline, and an
  accessible action label. It shows the label as a tooltip on hover or focus
  unless the caller selects the documented About/dismiss exception.
- A Codex-style title-bar toggle built on `GitHubIconButton` must keep hover,
  selected, and selected-plus-hover visually distinct. In light mode the
  sampled reference values are: hover uses `#f5f5f5` background with
  `#9a9b9c` foreground; selected uses `#f5f5f6` background with `#212529`
  foreground; selected-plus-hover uses `#ededed` background with `#212529`
  foreground. State resolution is selected-plus-hover, then selected, then
  hover, then rest. Dark mode uses semantic counterparts with the same
  contrast ordering; it must not collapse the three states into one active
  color.
- 基于 `GitHubIconButton` 的 Codex 风格标题栏切换按钮必须区分 hover、激活、
  激活并 hover 三种视觉状态。浅色模式采用参考界面取样值：hover 背景为
  `#f5f5f5`、前景为 `#9a9b9c`；激活背景为 `#f5f5f6`、前景为
  `#212529`；激活并 hover 背景为 `#ededed`、前景保持 `#212529`。
  状态解析优先级固定为“激活并 hover > 激活 > hover > 默认”。暗色模式使用
  保持相同对比层级的语义色，不得把三种状态合并为单一 active 颜色。
- `GitHubNavigationSidebar`: one 240 px desktop application sidebar with full
  labels, selected-route state, pinned Settings/About actions, and a persisted
  docked mode. Before the first rendered frame, the persisted docked state is
  restored by snapping the toggle, spring progress, and route width to one
  consistent final geometry; startup must never run the interactive sidebar
  transition. When no persisted value exists, the default is expanded. Its menu
  rows follow the Codex/macOS SwiftUI sidebar hierarchy:
  compact 30 px rows, 8 px inline inset, 10 px selection radius, 16 px Lucide
  icons centered on a 24 px window-edge centerline (16 px keeps the Image on
  whole logical pixels so FemtoVG does not bilinear-blur the SVG bitmap). On
  macOS, AppKit's default traffic-light cluster (and the sidebar/back/forward
  accessories that follow it) is shifted 8 pt right in `src/platform/macos.rs`
  so the close button shares that same centerline; the product title aligns to
  the icon left edges. Each 31 px painted row owns a continuous 32 px hit band,
  so the trailing 1 px remains a visual gap while still retaining that row's
  pointer, hover, and click behavior during vertical traversal. Row background
  states switch immediately without a color transition, keeping hover attached
  to the pointer during rapid traversal. 13 px native system body typography, a neutral
  low-contrast selection surface, and no accent outline. The footer uses a
  sidebar-specific translucent hairline. The right edge is owned by the root
  shell as a content-boundary decoration: a visible 1 px stroke plus
  Codex-compatible 20 px ambient and 8 px near elevation lobes project only
  toward the sidebar. One interruptible frame-paced spring moves the sidebar,
  content surface, title-bar divider, and the status bar's painted leading edge;
  ordinary routes adopt the target width once when motion begins instead of
  reflowing on every animation frame. The transparent status host and persistent
  right cluster remain in window coordinates. Native live resize snaps sidebar
  motion to its target, freezes mounted route width, and commits the exact final
  width at drag end. PR Review also preserves its stable internal diff geometry.
  Native title-bar actions retarget the motion directly on the owning event-loop
  thread, so click bursts cannot queue stale callbacks.
  Settings uses the same regular label weight as navigation rows, with a 1 px
  optical baseline correction against its 16 px icon.
  The sidebar body itself must not paint an independent container shadow.
  The docked state uses the root-mounted visible border plus restrained inward
  elevation. The temporary hover state uses an opaque white (`#ffffff` light
  mode) border-only overlay with a restrained 10 px rounded trailing edge, no
  right-edge shadow, and Codex's neutral `#dfdfe0` light-mode hairline. Both remain above ordinary route
  surfaces; modal backdrops remain above them.
  Native live resize settles shared shell geometry to the current docked target
  instead of exposing an intermediate sidebar animation position.
  Route content must never cover the sidebar, this edge, or an escaping
  sidebar Tooltip during resize or route-specific composition.
  On macOS the docked sidebar uses a width-fixed 240 px native effect view with
  Menu material, BehindWindow blending, and height autoresizing. A full-window
  opaque native canvas matching the theme sits below Slint, so a late GPU resize
  cannot expose wallpaper tint in the content region. Native vibrancy adopts
  the final full-size transparent-title-bar style before inserting underlays,
  attaches them while the window is still opaque, and only then opens the
  transparent window path with a near-clear background rather than
  `clearColor`. The startup window stays alpha-gated until this native chrome
  is confirmed installed plus a short settle so the effect's first unblurred
  behind-window sample commits while invisible; a deadline still guarantees the
  window appears. Startup therefore never reveals desktop content or a staged
  transition through the translucent Slint sidebar. Until native
  installation reports ready, both the Slint window root and docked sidebar use
  opaque theme fallbacks. The effect frame is derived from its full-size native
  parent bounds, not the pre-full-size render-view frame, and is refreshed after
  style changes and native resize so it covers the title-bar strip. The docked
  sidebar paints Codex's final renderer surface above the native effect instead
  of exposing the raw Menu material: while key it uses `#ffffffa6` in light mode
  or `#2b2b2ba3` in dark mode, preserving blur while neutralizing wallpaper
  saturation. On resign-key, the effect is disabled, the native window keeps
  Codex's `#f9f9f9` light or `#000000` dark fallback, and the rendered sidebar
  becomes opaque `#fdfdfd` light or `#212121` dark. The temporary hover sidebar
  continues to use its independent opaque overlay surface.
  Undocked mode removes it from page layout; an 8 px left-edge hover target
  opens the same component as a rounded, border-only root-layer overlay with a
  short pointer-exit delay. macOS places Toggle/Back/Forward in a native
  left title-bar accessory. Windows and Linux expose equivalent controls in
  the sidebar header and collapsed-state control strip. Back/Forward enabled
  state always comes from Router history; the visual shell must not invent or
  mutate route history.
  On macOS, the route breadcrumb fills the remaining 46 px native title-bar
  inset instead of creating a second row above page content. Its blank area
  must remain free of hit targets for native window dragging, and its leading
  edge must stay clear of traffic lights and native accessories throughout the
  sidebar transition. A stable trailing action slot is excluded from the
  breadcrumb's clipping width; title-bar actions use shared IconButtons and
  must never overlay the route hierarchy or native drag region. One root-owned
  hairline spans the native left region, breadcrumb, and trailing action slot
  without seams. Compact title-bar actions use 28 px controls, 14 px glyphs,
  and the subtle foreground role. On macOS the accessory SF Symbols are
  configured at a whole 14 pt size with `NSImageScaleNone` so AppKit does not
  soft-rasterize a half-point symbol inside the bezel. Native control frames
  must remain wholly inside their title-bar accessory container so hover,
  focus, and pressed chrome cannot be clipped. The initial Slint window title is empty so
  AppKit cannot flash the standard product label before accessory installation;
  Rust restores the metadata title only after hiding native title presentation.
  macOS must not render substitute Slint title-bar controls while the native
  accessory is pending; only the final AppKit controls may appear there.
- `GitHubNavigationSidebar` 是统一的 240px 桌面应用侧栏，展示完整标签、当前
  路由选中态和固定在底部的 Settings/About，并持久化固定状态。首帧渲染前必须把
  持久化开关、弹簧进度和路由宽度一次性吸附到同一个最终几何，启动恢复不得播放用户
  交互使用的侧栏动画；没有持久化值时默认展开。取消固定时侧栏不
  占页面布局，窗口左侧 8px Hover 热区在根层以圆角、仅边框浮层打开同一组件，指针离开
  后短延迟关闭。菜单行高 30px、左右内缩 8px、选中圆角 10px，16px Lucide 线性图标中心落在距窗口左缘
  24px 的中线上（用 16px 保证 Image 落在整逻辑像素上，避免 FemtoVG 对 SVG 位图做半像素双线性模糊）。
  macOS 上由 `src/platform/macos.rs` 把 AppKit 默认交通灯簇（及跟随其后的侧栏/后退/前进附件）
  整体右移 8pt，使关闭按钮与该中线重合；产品标题与图标左缘对齐。正文使用 13px
  原生系统字体。每个 31px 绘制行拥有连续的 32px 命中带，因此末尾 1px 仍表现为视觉
  间距，但垂直穿越时继续保留该行的手型光标、Hover 与点击行为。行背景状态不做颜色
  过渡而是立即切换，快速移动时 Hover 始终紧随指针。底部使用侧栏专用半透明 Hairline。右边界
  由根 Shell 作为内容边界装饰拥有：可见的 1px 描边加 20px 环境阴影和 8px 近端阴影
  只向侧栏方向投射。
  固定侧栏使用同一个可中断、按显示帧推进的弹簧移动侧栏、内容表面、标题栏分割线和
  状态栏绘制起点；普通路由在运动开始时只采用一次目标宽度，不逐帧重排。状态栏透明
  宿主与右侧常驻工具组保持窗口坐标。原生 Live Resize 会把侧栏运动吸附到目标，冻结
  已挂载路由宽度，并在结束时提交精确最终宽度；PR Review 同时保持稳定内部 Diff 几何。
  Settings 与普通导航行使用相同的常规字重，并针对 16px 图标做 1px 视觉基线校正。
  侧栏主体自身不得绘制独立的右侧容器阴影；固定态使用根层清晰边框与克制的内向
  Elevation，临时 Hover 态使用不透明白色（浅色模式 `#ffffff`）、只有边框、尾部上下
  10px 圆角且没有右边界阴影的浮层；浅色边框使用 Codex 的中性 `#dfdfe0`。
  两者均位于普通路由表面之上、Modal 遮罩之下。原生 Live Resize 期间，共享 Shell
  几何必须收敛到当前固定目标，不得暴露侧栏动画中间位置。窗口 Resize 或路由专属组合期间，页面内容
  不得覆盖侧栏、右边界或越过侧栏边界显示的 Tooltip。
  macOS 固定侧栏使用固定 240px 宽、仅随高度缩放的原生 Effect View，材质为 Menu、
  混合为 BehindWindow；Slint 下方保留与主题一致的全窗口不透明原生 Canvas，GPU
  Resize 晚一帧时右侧也不会露出壁纸色。原生 Vibrancy 先切换到最终的全尺寸透明
  标题栏样式，再在窗口仍不透明时插入底层视图，最后才打开透明窗口路径并使用近透明
  背景而不是 `clearColor`；启动窗口保持 alpha 门控，直到确认原生 Chrome 安装完成，
  并再等待短暂 settle，让 Effect 第一次未模糊的 Behind-Window 采样在不可见期间提交；
  兜底时限保证窗口一定出现。启动阶段不得透出桌面内容或出现分阶段过渡。原生安装报告 Ready
  前，Slint 窗口根层与固定侧栏都必须使用主题对应的不透明 fallback。Effect 几何取自
  全尺寸原生父视图 bounds，而不是启用 FullSizeContentView 前的 Render View frame；
  应用样式与原生 resize 后都要重新同步，确保覆盖标题栏带。固定侧栏不会直接
  露出原始 Menu 材质，而是在原生 Effect 上绘制 Codex 最终的渲染表面：Key Window
  在浅色模式使用 `#ffffffa6`，深色模式使用 `#2b2b2ba3`，保留模糊并压低壁纸饱和度。
  失去 Key 状态时关闭 Effect，原生窗口继续使用 Codex 的浅色 `#f9f9f9` 或深色
  `#000000` fallback，渲染侧栏则切换为浅色 `#fdfdfd` 或深色 `#212121` 不透明表面。
  临时 Hover
  侧栏继续使用自己独立的不透明浮层表面。
  macOS 使用原生左侧标题栏附件，Windows/Linux 使用侧栏页头和收起态
  控制条；macOS 侧栏按钮在收起态使用轮廓 `sidebar.left`，展开态使用左侧
  三分之一填充图标，两种状态不得共用同一视觉；Back/Forward 可用状态只能来自
  Router 历史。Slint 窗口初始标题必须为空，避免原生附件安装前由 AppKit 闪现标准
  产品标题；Rust 只有在隐藏原生标题展示后才恢复系统元数据中的 `Usher`。原生附件
  尚未就绪时，macOS 不得绘制替代的 Slint 标题栏控件；该区域只允许最终 AppKit
  控件出现。macOS 附件 SF Symbols 使用整点号 14pt，并以 `NSImageScaleNone` 禁止
  Bezel 对半点号符号再次双线性缩放。
- `GitHubLink`: default, inline, and muted text-link states with visible
  underline and keyboard focus behavior.
- `GitHubTooltip`: TooltipV2 pattern; tooltip canvas/foreground tokens, 12 px
  text in an 18 px line box (so descenders are not clipped), 4 px vertical and
  8 px horizontal padding, 6 px radius, and a 4 px host gap. Call sites publish
  only the complete anchor rectangle plus a preferred side/alignment; they must
  never calculate floating-surface `x`/`y`. `GitHubTooltipOverlayHost` applies
  the Floating UI sequence `offset -> flip(bestFit) -> shift(padding) -> size`:
  keep the preferred side when it fits, try its opposite, otherwise choose the
  least-overflowing side, clamp both axes to an 8 px visible-viewport inset,
  and wrap long labels to the remaining width. Every Slint global graph owns
  one root portal, including the independently compiled Settings and Messages
  pages, so scroll/list/card/menu `clip` cannot truncate the surface. A generic
  portal remounts its measurement surface for each new claim token so a
  ComponentFactory cannot reuse the preceding label's preferred width. Locators
  stay mounted and clear text while inactive; action activation synchronously
  dismisses the active claim before callbacks that may unmount the anchor, with
  token/lease expiry only as a model-removal fallback. Messages row actions use
  an enum-only factory-local state and separate literal surfaces; no label
  string or shared dynamic tip-label property may cross a list delegate. Local
  Tooltip rectangles, pointer-relative built-in
  `Tooltip`, and local `z` escalation are not valid alternatives. /
  Tooltip 调用点只发布完整锚点矩形及首选方向/对齐，禁止自行计算浮层 `x`/`y`。
  根层按 `offset -> flip(bestFit) -> shift(padding) -> size` 计算：能容纳时保留
  首选方向，否则尝试反向，再选择溢出最少的一侧；两轴约束在可视视口 8px 内边距
  内，长文案按剩余宽度换行。每个 Slint global 图各自挂一个根门户，包括独立编译的
  Settings 与 Messages，因此滚动区、列表、卡片和菜单的 `clip` 不得裁剪 Tooltip。
  通用 portal 每次收到新的 claim token 都重新创建文本测量 surface，禁止独立 factory
  复用上一条文案的 preferred-width。定位器常驻并用空文案撤销；操作触发业务回调前
  同步关闭当前 claim，token/lease 仅作为模型移除兜底。Messages 行动作使用只传枚举的
  factory-local state 和相互独立的字面量 surface，任何标签字符串或共享动态 tip-label
  属性都不得穿过列表 delegate。局部 Tooltip Rectangle、指针相对的内置 `Tooltip` 和提高局部
  `z` 均不是允许的替代方案。
- `GitHubCounterLabel`: the typed Primer CounterLabel counterpart for counts
  adjacent to navigation text or actions. It provides Default, Primary, and
  Secondary schemes, optional upper-limit formatting, a compact overlay mode,
  and explicit zero hiding. The component remains content-sized in layouts.
  Icon-only hosts must include count meaning in their accessible label and
  Tooltip. Its property matrix lives under `Labels and counters` in UI
  Components.
- `GitHubCounterLabel`：导航文字或操作旁计数统一使用的强类型 Primer CounterLabel，
  提供 Default、Primary、Secondary Scheme、可选上限格式、紧凑叠加模式与明确的
  零值隐藏，并在 Layout 中保持内容宽度。仅图标宿主必须在无障碍名称和 Tooltip 中
  包含计数语义；其属性矩阵归入 UI Components 的 `Labels and counters` 分类。
- `GitHubStatusBar`: one 28 px transparent, full-window root-shell host whose
  painted surface starts at the live main-content edge, leaving no status-bar
  surface below an open sidebar. Its persistent keyboard, bell, network, and
  version cluster is anchored 8 px from the window-right edge with 4 px gaps,
  outside both the moving painted surface and any width-distributing layout.
  The bell is icon-only and overlays a Primer primary-style numeric unread
  counter only when count is positive (`99+` cap); zero mounts no counter.
  Green is reserved for ready/online state, unread notification uses accent,
  and version remains muted. Right-edge tooltips align to their host's trailing
  edge and expand leftward. The bell navigates to `Route.Messages`; remaining
  shared popups use one status-bar parent coordinate space and must leave the version item visible. The complete behavior contract
  is [status-bar.md](status-bar.md).
- `GitHubMessageListItem`: GitHub Inbox-style 84 px row (76 px compact) with
  stable unread dot, semantic 16 px leading visual, title/body/source-time
  hierarchy, and always-mounted read/unread and delete IconButtons. Unread state
  uses a neutral subtle background, dot, and title weight; semantic color is
  confined to the leading state icon and color is never the only signal. Its
  read/unread property matrix lives under `Notification inbox rows`
  in UI Components. The separate `Message center` section publishes real test
  events for each built-in category and for default, always-native, and in-app-only
  delivery policies; it is not a static component preview.
- Floating application notifications use a neutral `canvas-overlay` card with
  neutral border/shadow; severity is limited to the leading icon and a 3 px
  accent. The overlay is not rendered on the Notifications route because the
  corresponding Inbox row already supplies durable feedback.
- `GitHubMessageListItem`：GitHub Inbox 风格 84px 行（紧凑态 76px），包含稳定未读
  圆点、16px 语义前导视觉、标题/正文/来源时间层级，以及常驻的已读切换和删除
  IconButton。未读状态使用中性浅底、圆点与标题字重，语义色仅用于前导状态图标，
  不能只依赖颜色；其属性矩阵
  登记在 UI Components 的 `Notification inbox rows`。独立的 `Message center` 分类会
  发布各内置分类以及默认、强制原生、仅应用内投递策略的真实测试事件，并非静态预览。
- 浮动应用通知使用中性的 `canvas-overlay` 卡片及中性边框/阴影，语义色仅用于前导图标
  和 3px 强调线；Notifications 路由已有对应 Inbox 行，因此不再渲染该浮层。
- `GitHubStatusBar`：根 Shell 唯一的 28px 全窗口透明宿主；可见表面从实时主内容
  边缘开始，侧栏展开后其正下方不得出现状态栏表面。常驻键盘、铃铛、网络与版本
  工具组距窗口右边缘 8px、条目间距 4px，位于移动可见表面之外，并禁止参与会重新
  分配宽度的 Layout。铃铛只显示
  图标；未读数大于 0 时叠加 Primer Primary 风格数字 Counter（上限显示 `99+`），
  为 0 时不挂载 Counter。绿色只表示就绪/在线，未读通知使用强调色，版本保持弱化色。
  右侧 Tooltip 与宿主尾边对齐并向左展开；铃铛导航到 `Route.Messages`，其余共享浮层
  只使用一套状态栏父坐标系，且不得遮挡版本项。完整契约见 [status-bar.md](status-bar.md)。
- `GitHubCopyButton`: 32×32 invisible icon button, centered 16 px Copy
  Octicon, persistent tooltip, and Copy→Check confirmation only after the
  clipboard callback returns success.
- `GitHubActionMenu`: ActionList inside an AnchoredOverlay; 12 px overlay
  radius, semantic border/shadow, 4 px anchor gap, viewport collision handling,
  bounded height, internal scrolling, focus ownership, Escape dismissal, and
  reusable `GitHubActionListItem` rows. Medium rows follow Primer's 32 px
  geometry (6 px block padding around a 20 px label); block descriptions use
  48 px rows with a 16 px secondary line. Single selections use a leading
  Check Octicon; selected state must not be represented by persistent hover
  fill or heavier label weight. Row dividers are opt-in and remain disabled for
  the borderless ActionMenu/ActionList variant. Outside dismissal is split on purpose: the menu owns Escape and chrome
  `dismiss-revision` (native title-bar / docked-shell); opening also
  `claim-exclusive` so peer ActionMenu/Diff-settings popovers close on the same
  click without page-local dismiss wiring. Each host still mounts a sibling
  blank outside-click TouchArea under the menu—and under chrome that must keep
  receiving clicks. ActionMenu itself must not paint a full-viewport hit target
  at its overlay z, or filter/popover switching would only close instead of
  retargeting. A click in the native title bar advances `dismiss-revision` so
  an open ActionMenu closes without consuming the native title-bar action.
  Triggers that live in that chrome dismiss zone must use
  `GitHubActionMenuTriggerArea` (press-state open/close) so the same click
  cannot dismiss then toggle-reopen; `dismiss-requested` must close, never
  toggle.
- `GitHubFilterMenuButton`, `GitHubFilterMenuFrame`, and
  `GitHubFilterMenuItem` are the generic single-select filter/sort ActionMenu
  composition. They own trigger, header/close, checked-row, keyboard, and
  accessibility behavior. Toolbar triggers use the 14 px body-text token;
  compact option labels use the 12 px small-text token while retaining the
  shared 32 px ActionList row geometry. The GitHub PR filter variant matches
  `.SelectMenu-item`: each row has a muted 1 px bottom divider; the selected
  label uses weight 500 and the Check plus label use the default foreground,
  with no persistent selected fill. Feature menus provide only labels, values,
  grouping, and anchored state; overlays anchor to the measured trigger edge
  rather than typography-specific offsets. UI Components demonstrates both the
  bordered SelectMenu and borderless ActionMenu variants with provider-neutral
  examples.
- `GitHubPageHeader`: the shared Primer PageHeader foundation for page title,
  description, leading visual, optional divider, and trailing actions.
- `GitHubUnderlineNavItem`: the shared UnderlineNav/UnderlinePanels tab item
  for route and in-page navigation. It owns optional leading Octicon,
  descriptive label, selected underline, hover, disabled, Enter/Space, and
  keyboard-visible focus states; page owners retain selected-panel semantics.
  `GitHubUnderlineNav` owns the full-width baseline and accessible tab-list,
  keeps the first and last items inside a 16 px horizontal safe area, and uses
  an explicit 4 px gap between items. Pages must not recreate that baseline or
  pin shared tab items directly to a container edge.
- `GitHubSegmentedControl`, `GitHubSegmentedControlItem`, and
  `GitHubSegmentedControlDivider`: the shared compact, mutually exclusive
  choice group. The shell width hugs the segment row with no trailing empty
  chrome. Item labels fill the complete control height, remain vertically
  centered, and apply the shared 1 px optical correction for compact text.
  Items own selected, hover, disabled, Enter/Space, and
  keyboard-visible focus states, and always report `clicked` (callers may
  ignore a re-selection). Live numeric suffixes use the item's integer `count`
  input. The item renders `label`, `" ("`, the integer, and `")"` as separate
  `Text` nodes — never as one concatenated string property. Call sites must not
  concatenate changing numbers into `label` either; a finished `"All (N)"`
  string property also freezes after the first paint on independently compiled
  ComponentFactory pages. Groups expose an accessible contextual label and
  contain 2–5 concise non-wrapping labels. Selection may change text weight or
  color, but measuring always reserves the selected weight so divider positions
  stay stable for a given label/count.
- `GitHubScrollView`: the application-owned mounted viewport and semantic
  scrollbar foundation. It must preserve models while scrolling, support
  track and thumb pointer interaction, and never depend on Material styling.
- `GitHubProgressBar`: the shared determinate/indeterminate semantic progress
  track; callers provide the visible operation label and status.
- `GitHubSpinner`: the only indeterminate spinner implementation; callers
  retain a visible loading label/status and must not shift layout when it
  appears. It paints a Primer 2px track (25% opacity) plus a rotating head
  with the caller-supplied theme foreground so the indicator stays visible in
  both light and dark mode. The track geometry remains static and animation
  updates only the cached head image's rotation transform; it must not rebuild
  Path coordinates per frame and must remain evenly paced at 60 Hz.
  `GitHubCenteredLoadingState` is the shared compact
  spinner+13px muted label panel used by PR list and PR review
  Conversation/Commits/Files changed loading.
- `GitHubActionsJob`: the shared GitHub Actions-style Job/Step presenter for
  ordered workflow state, semantic lifecycle/conclusion, bounded disclosure
  logs, duration cells, and running animation. It remains presentation-only;
  feature controllers own planning, execution, cancellation, and timing.
- `GitHubJobRunSurface`: the application-level GitHub Actions-style run layout.
  It combines generic run metadata, a Jobs region, and the selected
  `GitHubActionsJob`; it must not read a Workflow/Tag property or own execution.
  `Route.JobRun` can be entered from any route and relies on Router history for
  source-aware Back navigation.
- `GitHubBlankslate`: the shared Primer Blankslate foundation for empty,
  filtered-empty, loading, and recoverable error states. It owns optional
  leading visual, heading, concise description, primary action, secondary
  link, bordered/narrow/spacious variants, and stable top/bottom padding.
  Actions must explain the next useful step; loading and error variants must
  remain descriptive without relying on animation or color alone.
- `GitHubTextField` and `GitHubCompactTextField`: visible label when used as a
  form control, 32 px medium input, focus outline, disabled/loading handling,
  caption, and descriptive validation message. Error color alone is
  insufficient. A transformed loading visual must be mounted above the field as
  a sibling of the clipped TextInput surface, not inside its clip subtree, so
  continuous animation cannot destabilize text antialiasing on non-Retina
  60 Hz displays.
- `GitHubToggle`: immediate-save boolean setting only; permanently visible
  external label/caption supplied by its owner, 24 px minimum target, loading
  and disabled protection, Enter/Space activation, and keyboard-visible focus.
- `GitHubSegment`: one of 2–5 mutually exclusive, instantly applied choices;
  labels do not wrap and the group has a visible or contextual label.
- `GitHubDialog`: root-layer modal/non-modal overlay with owned backdrop input,
  stable header/body/footer safe areas, visible close control, close/Esc
  semantics, initial focus ownership, consistent action ordering, and a
  full-window narrow layout at the small breakpoint. Root Dialog hosts paint
  above application chrome, so a modal backdrop covers the complete window,
  including the native-title-bar inset and any persistent status bar. A route
  host that occupies only the main content column supplies the complete window
  viewport in host-local coordinates; both the surface and backdrop use that
  viewport, so docked navigation cannot shift the Dialog center or create a
  backdrop seam. On
  macOS, the standard close/minimize/zoom traffic lights remain above the
  backdrop and clickable; application-owned sidebar/back/forward title-bar
  accessories remain below it and must expose no hover, Tooltip, or click
  interaction while the backdrop is visible. Native-title-bar clicks also
  participate in the shared outside-dismiss contract: a Dialog closes only if
  its ordinary backdrop click would close it; hard/modal forms keep their
  existing non-dismissible behavior.
- `GitHubToast`: compact transient feedback using the current Primer Banner
  anatomy and semantic status icon/background/border, never the retired
  rail-style Toast pattern. A single-line message, leading icon/spinner, and
  dismiss glyph share one visual centerline. When a message wraps, the leading
  icon/spinner and dismiss glyph align with its first line; the message block
  must never be centered against its complete multi-line height.
- `GitHubNotificationBanner`: queued persistent feedback with at most two
  shared GitHub actions plus a dismiss control. Render only the newest queue
  head so one Banner is visible at a time; dismissing it reveals the next older
  item. The transient Banner queue is capped at three (newest-first; older
  tails drop from the overlay). Its in-card footer distinguishes
  the retained `queue-count` from the durable unread total: overflow uses copy
  such as **3 shown · 9 more**, never an ambiguous **3 notifications**.
  The footer contains only that status plus a vertical overflow trigger; a
  shared ActionMenu owns **View all notifications** and **Dismiss all**, keeping
  those batch operations out of the Banner's existing action-button row. A
  hairline separates the footer, while the semantic rail spans the complete
  left edge inside the clipped card radius. The footer appears for a multi-item
  queue or hidden unread messages. UI Components must demo an Action Banner in
  the overflow state plus the live stack.
- Toast/notification feedback must not duplicate an existing feature-owned
  error surface and must never be the only way to communicate critical
  information.
- PR/MR collections use one compact DataTable/Box-like surface: state controls
  and filters share the header row, rows preserve a stable title/meta
  hierarchy, an empty filtered result uses `GitHubBlankslate`, and pagination
  remains outside the bordered collection. Small known option sets use
  `GitHubActionMenu`; searchable provider/user lists use a SelectPanel-style
  composition.
- Review navigation uses `GitHubPullRequestTabButton` as the counted
  UnderlineNav variant. The Files changed hierarchy uses TreeView semantics
  (indent guides, expandable directories, current file), while icon-only file
  actions reuse `GitHubIconButton`. Diff source remains virtualized and
  monospace; all surrounding prose and controls use application typography.
- Settings uses SplitPage/NavList at regular widths and a
  `GitHubSettingsSectionMenuButton` at compact widths. Both paths expose the
  same sections and keyboard contract; form controls remain shared components
  under `ui/components/settings/`.
- Chat uses an unframed work surface, bounded virtual history,
  `GitHubScrollView`, compact bordered message surfaces, and a responsive
  bordered composer. Suggestions are standard GitHub buttons and sending is a
  medium IconButton, not a floating circular action. User-facing Chat failures
  use one application language and concise recovery text; technical HTTP
  details remain in bounded logs rather than message bubbles.
- Terminal chrome reuses shared GitHub controls, but terminal cells retain
  their dedicated monospace/ANSI rendering contract. Product theme changes
  must not recolor or reflow PTY-provided cell content.
- `Route.UiComponents` is the mandatory interactive component playground for
  development builds and is compiled only with the non-default
  `ui-components` Cargo feature. Default and production builds omit its menu
  entry and page implementation; live-preview UI work must enable both
  `live-preview` and `ui-components`.
  Every application-owned GitHub/Primer component must be registered there
  when it is introduced or materially changed. Reusable foundations must show
  every applicable public-property contrast, including intent, selected,
  disabled, loading, error, size, placement, overlay, and dismissal behavior.
  Feature composites that require project/provider/session models must link to
  their owning live route and enumerate the states tested there; the playground
  must not create a second business-state implementation.
- The playground uses the real shared components and root-layer managers.
  Static lookalikes, screenshots, or gallery-only variants do not satisfy
  component validation. Notification, Toast, and Dialog behavior is exercised
  from this route; they do not own standalone demo routes.

- `Route.UiComponents` 是开发构建中强制维护的交互式组件演练场，仅在启用非默认
  Cargo feature `ui-components` 时编译。默认和生产构建不显示菜单入口，也不编译
  页面实现；使用 live preview 开发 UI 时必须同时启用 `live-preview` 和
  `ui-components`。每个应用自有
  GitHub/Primer 组件在新增或发生实质变更时，都必须同步登记。复用基础组件必须
  对比展示所有适用的公开属性，包括意图、选中、禁用、加载、错误、尺寸、位置、
  遮罩和关闭行为。
- 依赖 Project、Provider 或 Session model 的功能组合组件必须链接到其真实业务
  路由，并列明在那里验证的状态；演练场不得复制第二套业务状态实现。
- 演练场必须使用真实共享组件和根层 Manager。静态仿制、截图或只在 Gallery 中
  存在的变体不能视为组件验证。Notification、Toast、Dialog 统一在此路由演练，
  不再拥有独立 demo 路由。

All GitHub/Primer visual component symbols across `ui/` use the `GitHub`
prefix. `MainWindow`, non-visual structs, enums, globals, routes, and callbacks
remain domain-oriented. Legacy/demo route pages that have not yet been
migrated must remain explicitly listed as debt; they must not receive the
prefix until their implementation satisfies this specification.

整个 `ui/` 中已实现 GitHub/Primer 规范的视觉组件符号全部使用 `GitHub`
前缀。`MainWindow` 以及非视觉 struct、enum、global、路由和 callback 继续
使用领域命名。尚未迁移的 legacy/demo 路由页面必须明确登记为欠债，在实现满足
本规范之前不得仅靠加前缀伪装成合规组件。

补充强制约束：Project 网格右上角 Provider 斜角标保持 10px 身份图标和文字在
45 度色带内部垂直居中，但固定身份内容必须预合成为应用自有 SVG，并以无运行时旋转的
`Image` 显示。FemtoVG 会把旋转变换下的小字号实时 `Text` 从带 hinting 的字形图集切换为
低 DPI 下偏软的轮廓渲染，因此禁止在该斜角标中恢复旋转 `Text`。通用单选筛选/排序弹层统一归入 ActionMenu，由
`GitHubFilterMenuButton`、`GitHubFilterMenuFrame` 和
`GitHubFilterMenuItem` 负责触发器、标题/关闭、选中标记、键盘及无障碍行为；
工具栏触发器统一使用 14px body-text 令牌；紧凑选项标签统一使用 12px small-text
令牌，同时保持共享 ActionList 的 32px 行高；弹层按触发器实际边界定位，不得依赖
特定字号下的固定偏移；PR 筛选变体匹配 GitHub `.SelectMenu-item`：每行底部绘制
1px muted 分隔线，选中标签使用 500 字重，Check 与标签使用默认前景色且不绘制持续
选中背景。无逐行边框的 ActionMenu/ActionList 是独立变体，UI Components 必须以
中性示例同时演示这两种菜单，禁止混用；
业务层只提供中性的 label、value、分组与锚点状态，UI Components 页面必须在
ActionMenu 区域使用无 Provider/Workflow/PR 字样的通用数据演示。Project 网格
必须按受约束内容区计算列数，新增一列前保证每张卡片至少 280px 可读宽度。Chat
历史分页入口必须位于 virtual top spacer 之后的消息流内并占据真实高度，禁止覆盖
最早一条已挂载消息；用户可见错误统一使用应用语言，HTTP 技术详情仅写有界日志。
PR 审核摘要是 Open/Closed/Merged 的唯一可见状态来源，页头操作条不得重复状态；
三个带计数 Tab 在所有宽度下都按共享 Primer UnderlineNav 规范左对齐并按内容定宽，
保留 16px 容器内边距与 4px Tab 间距，禁止拉伸均分；尾部 diff 统计必须先让位。PR 页头、导航和交互
调整不得顺带修改 Files changed 代码区或文件树的字体与颜色；两者属于独立的审核
内容契约，只有明确提出并带专门视觉回归的 diff/文件树设计变更才可调整。
原生标题栏点击必须进入应用级 Chrome 关闭通道（`dismiss-revision`）：所有共享
ActionMenu 和其他本来可通过页面空白关闭的临时弹层都应关闭，但不得吞掉原生标
题栏操作。弹层互斥由组件在打开时 `claim-exclusive` 完成，业务页不得再为切换
首击散落调用 dismiss-all。页面内容区的空白点击关闭由各宿主自行挂载在菜单之
下的 TouchArea 承担，不能依赖 ActionMenu 在 overlay z 上铺满视口。Dialog 仅在
普通遮罩点击本就允许关闭时响应，硬模态表单仍保持不可消隐语义。

## 9. Material dependency boundary / Material 依赖边界

Material may be used only as a replaceable low-level Slint implementation
library:

- import public controls through `@material`;
- never import a file from `material-1.7.1-fixed/` directly;
- never edit vendored Material source;
- do not consume Material palette, typography, or style metrics as product
  design tokens;
- implement GitHub/Primer visual behavior in application-owned `ui/` files.
- treat the complete vendored directory as read-only in every migration batch
  and verify it has no working-tree changes before handoff.
- the application allowlist is limited to `MaterialWindowAdapter` in
  `ui/main.slint` and public `ListView`/`ScrollView` virtualization in
  `ui/views/workflow/pr_review_workspace.slint`;
- run `./scripts/check-ui-policy.sh` to enforce the Material and icon boundary.

Material 只能作为可替换的底层 Slint 实现库：必须通过 `@material` 导入公开控件，
禁止直接导入或修改 `material-1.7.1-fixed/`，禁止把 Material palette、
typography 或 style metrics 当作产品设计令牌，GitHub/Primer 视觉行为必须在
项目自有 `ui/` 文件中实现。
当前应用白名单仅包含 `ui/main.slint` 的 `MaterialWindowAdapter`，以及
`ui/views/workflow/pr_review_workspace.slint` 用于大型 model 虚拟化的公开
`ListView`/`ScrollView`。`./scripts/check-ui-policy.sh` 自动检查 Material 与
图标边界。

## 10. Validation and batch gate / 验证与批次门禁

For every UI batch:

1. Inspect the exact GitHub/Primer references before implementation.
2. Search for a matching shared component and reuse it.
3. Run `./scripts/check-ui-policy.sh`, then compile the complete root Slint
   tree with `cargo check --locked`.
4. Run `cargo fmt --all`, `cargo test --lib --locked`, and
   `git diff --check`.
5. Audit direct vendored imports, deprecated theme globals, raw functional
   colors, unprefixed GitHub-style components across the complete `ui/` tree,
   and component declarations incorrectly placed outside `ui/components/`
   (route page entries are the only exception).
6. Manually inspect affected routes at `800x600`, a representative working
   size, and `1280x900`, in light and dark mode, including keyboard focus and
   applicable interaction states.
7. Record discovered defects in `bug-history.md` and update the migration
   checklist.
8. Stop after the batch handoff. Do not start the next batch until the user
   explicitly accepts the current batch, unless the user has explicitly
   authorized a named consecutive batch run. Stop after the final authorized
   batch for acceptance.

每一批都必须查证参考、优先复用共享组件、执行完整 Slint 编译及规定检查，审计本批
涉及的私有导入/旧主题/裸色/未加前缀组件，并在明暗主题、最小和宽窗口下检查适用
交互状态。所有缺陷写入 `bug-history.md`，同步改造清单；交付后必须停止，只有用户
明确验收通过才可进入下一批。若用户明确授权连续执行若干指定批次，则可连续实施，
但必须在最后一个获授权批次完成后停下等待验收。
