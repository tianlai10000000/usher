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
  `MainWindow`: `Hiragino Sans GB` on macOS, `Microsoft YaHei UI` on Windows,
  and `Noto Sans` elsewhere.
- Terminal content keeps its terminal font. Diff source and file-tree code may
  keep their documented monospace/platform fonts; other PR-review UI inherits
  the application family.
- Default body text is 14 px. Use 12 px for secondary labels and 16 px or the
  documented heading tokens for hierarchy. Control labels normally use weight
  600; body text uses 400.
- Use the matching official Octicon from `ui/assets/icons/`. Do not substitute
  Unicode glyphs, emoji, Material icons, or hand-drawn approximations. Register
  new upstream assets in `ui/assets/icons/OCTICONS.md`.
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
- Icon-only controls require an accessible tooltip and a persistent hit target.
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
  dimensions, duplicate-start protection, Enter/Space activation, and a
  visible focus outline.
- `GitHubIconButton`: the shared basis for every icon-only action; centered
  Octicon, default/primary/danger/invisible states, small/medium/large control
  geometry, loading support, Enter/Space activation, focus outline, and an
  action-label tooltip on hover or focus.
- `GitHubLink`: default, inline, and muted text-link states with visible
  underline and keyboard focus behavior.
- `GitHubTooltip`: TooltipV2 pattern; tooltip canvas/foreground tokens, 12 px
  text, 4 px vertical and 8 px horizontal padding, 6 px radius, and a 4 px
  host gap.
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
  fill or heavier label weight.
- `GitHubPageHeader`: the shared Primer PageHeader foundation for page title,
  description, leading visual, optional divider, and trailing actions.
- `GitHubUnderlineNavItem`: the shared UnderlineNav/UnderlinePanels tab item
  for route and in-page navigation. It owns optional leading Octicon,
  descriptive label, selected underline, hover, disabled, Enter/Space, and
  keyboard-visible focus states; page owners retain selected-panel semantics.
- `GitHubSegmentedControl`, `GitHubSegmentedControlItem`, and
  `GitHubSegmentedControlDivider`: the shared compact, mutually exclusive
  choice group. Items own selected, hover, disabled, Enter/Space, and
  keyboard-visible focus states; activating the selected item is a no-op.
  Groups expose an accessible contextual label and contain 2–5 concise
  non-wrapping labels. Selection may change text weight or color, but it must
  never change an item's measured width, divider positions, or the group's
  outer geometry.
- `GitHubScrollView`: the application-owned mounted viewport and semantic
  scrollbar foundation. It must preserve models while scrolling, support
  track and thumb pointer interaction, and never depend on Material styling.
- `GitHubProgressBar`: the shared determinate/indeterminate semantic progress
  track; callers provide the visible operation label and status.
- `GitHubSpinner`: the only indeterminate spinner implementation; callers
  retain a visible loading label/status and must not shift layout when it
  appears.
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
  insufficient.
- `GitHubToggle`: immediate-save boolean setting only; permanently visible
  external label/caption supplied by its owner, 24 px minimum target, loading
  and disabled protection, Enter/Space activation, and keyboard-visible focus.
- `GitHubSegment`: one of 2–5 mutually exclusive, instantly applied choices;
  labels do not wrap and the group has a visible or contextual label.
- `GitHubDialog`: root-layer modal/non-modal overlay with owned backdrop input,
  stable header/body/footer safe areas, visible close control, close/Esc
  semantics, initial focus ownership, consistent action ordering, and a
  full-window narrow layout at the small breakpoint. When a persistent status
  bar sits above the overlay, regular dialogs use the visible content viewport
  so compact-height top and bottom gutters remain equal.
- `GitHubToast`: compact transient feedback using the current Primer Banner
  anatomy and semantic status icon/background/border, never the retired
  rail-style Toast pattern.
- `GitHubNotificationBanner`: queued persistent feedback with at most two
  shared GitHub actions plus a dismiss control. Render only the queue head so
  one Banner is visible at a time; dismissing it reveals the next item.
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
  medium IconButton, not a floating circular action.
- Terminal chrome reuses shared GitHub controls, but terminal cells retain
  their dedicated monospace/ANSI rendering contract. Product theme changes
  must not recolor or reflow PTY-provided cell content.
- `Route.UiComponents` is the mandatory interactive component playground.
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

- `Route.UiComponents` 是强制性的交互式组件演练场。每个应用自有
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
6. Manually inspect affected routes at `640x600`, a representative working
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
