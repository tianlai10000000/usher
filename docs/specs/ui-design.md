# GitHub/Primer UI Design / GitHub/Primer 页面设计规范

## Scope / 范围

This specification governs every user-facing page and reusable visual component under `ui/`, including light and dark themes, transient feedback, empty/error/loading states, overlays, and interaction states.

本文约束 `ui/` 下所有面向用户的页面与复用视觉组件，包括明暗主题、短时反馈、空/错误/加载状态、遮罩层及交互状态。

## Source of truth / 设计依据

Usher's product UI must reproduce GitHub's current product UI and its Primer design system. Implementations must not be invented from memory or approximated from generic Material, Fluent, or platform conventions.

Usher 的产品 UI 必须复刻 GitHub 当前产品界面及其 Primer 设计系统。不得凭记忆设计，也不得用通用 Material、Fluent 或平台惯例代替。

Use this evidence order:

查证依据按以下优先级使用：

1. The current matching interaction on `github.com`, inspected in both light and dark mode when accessible.
2. Current [Primer Product UI](https://primer.style/product/) component and foundation documentation.
3. Current official implementation source in GitHub's [`primer/react`](https://github.com/primer/react), [`primer/css`](https://github.com/primer/css), [`primer/view_components`](https://github.com/primer/view_components), [`primer/primitives`](https://github.com/primer/primitives), and [`primer/octicons`](https://github.com/primer/octicons) repositories.
4. If GitHub has no matching component or the references conflict, stop and obtain a product decision. Do not silently invent a hybrid.

When an older Primer component is intentionally reproduced because it exactly matches an existing Usher interaction, record that status in the component source and preserve its official geometry. For example, Usher's transient Toast follows Primer CSS's deprecated Toast visual structure because GitHub's current accessibility guidance discourages toast usage but does not define a replacement floating-toast visual.

如果因现有 Usher 交互与旧版 Primer 组件完全对应而有意复刻旧组件，必须在组件源码中记录其状态，并保留官方几何参数。例如，Usher 的短时 Toast 沿用 Primer CSS 已弃用 Toast 的视觉结构；原因是 GitHub 当前无障碍指南不推荐 Toast，却没有定义替代的悬浮 Toast 视觉。

## Visual invariants / 视觉硬性约束

- Use GitHub/Primer semantic color roles for canvas, foreground, muted foreground, border, accent, success, attention, danger, and done states. Every role needs verified light and dark values.
- Match the reference component's dimensions, spacing, border width, radius, typography, icon size, shadow, truncation/wrapping, and responsive breakpoint behavior. “Looks close” is not acceptance.
- Use the matching Octicon from `ui/assets/icons/`. Do not substitute Unicode glyphs, emoji, Material icons, or hand-drawn approximations. New SVG assets must come from the official Octicons source and be recorded in `ui/assets/icons/OCTICONS.md`.
- Implement all observable states present in the reference: default, hover, active/pressed, focus-visible, selected, disabled, loading, error, empty, and expanded/collapsed as applicable.
- Keep interactive hit targets mounted and change visual state instead of conditionally creating hover targets.
- Use GitHub wording, capitalization, density, information hierarchy, and content placement where the product concept maps directly. Do not copy GitHub trademarks or irrelevant product content.
- A reusable pattern belongs in `ui/components/`; page composition belongs in `ui/views/`. A page must reuse the shared pattern rather than locally recreating a visually similar variant.
- Full-window overlays remain mounted at the root layer in `ui/main.slint`.
- Never edit `material-1.7.1-fixed/` or any other vendored Slint Material UI source to achieve product behavior or visuals. Adapt or wrap the dependency only in application-owned `ui/` code so framework upgrades remain replaceable.
- Ordinary application UI inherits one platform-selected CJK-capable family
  from `MainWindow`: `Hiragino Sans GB` on macOS, `Microsoft YaHei UI` on
  Windows, and `Noto Sans` elsewhere. Do not add page-local font overrides.
  Terminal content keeps all existing font choices unchanged. In PR detail,
  only diff source code and file-tree content retain their documented
  platform/monospace families; all other review UI inherits the application
  family.

- 画布、前景、弱化前景、边框、强调、成功、注意、危险、完成等状态必须使用 GitHub/Primer 语义色；每种语义色都要核对明暗主题。
- 尺寸、间距、边框宽度、圆角、字体、图标尺寸、阴影、截断/换行以及响应式断点必须与参考组件一致。“看起来差不多”不算验收通过。
- 必须使用 `ui/assets/icons/` 中对应的 Octicon；禁止用 Unicode 字符、Emoji、Material 图标或手绘近似图标替代。新增 SVG 必须来自官方 Octicons，并登记到 `ui/assets/icons/OCTICONS.md`。
- 按参考实现覆盖所有可见状态：默认、Hover、按下、键盘焦点、选中、禁用、加载、错误、空状态及展开/折叠（如适用）。
- 交互命中区域应常驻，通过视觉状态变化实现 Hover 等反馈，不得条件创建命中节点。
- 产品概念一致时，应采用 GitHub 的文案风格、大小写、信息密度、层级和内容位置；不得复制 GitHub 商标或无关产品内容。
- 复用模式放入 `ui/components/`，页面装配放入 `ui/views/`；页面不得重复实现已有共享模式的近似变体。
- 全窗口遮罩继续挂载在 `ui/main.slint` 根层。
- 为实现产品行为或视觉，绝对禁止修改 `material-1.7.1-fixed/` 或任何其他 vendored Slint Material UI 源码。只能在项目自有 `ui/` 代码中适配或包装依赖，确保框架升级时可以直接替换。
- 普通应用 UI 统一继承 `MainWindow` 按平台选择的中日韩兼容字体：
  macOS 使用 `Hiragino Sans GB`，Windows 使用 `Microsoft YaHei UI`，
  其他平台使用 `Noto Sans`。禁止再增加页面级字体覆盖。Terminal 内容的
  既有字体选择完全不变；PR 详情中仅 Diff 源码和文件树内容保留文档约定
  的平台/等宽字体，其余审核界面继承应用字体。

## Evidence required before editing / 修改前证据要求

For every new component or material visual change:

每个新组件或实质性视觉改动都必须：

1. Identify the exact GitHub surface and interaction being reproduced.
2. Inspect the relevant Primer documentation and, when exact values are not documented, the official implementation source.
3. Record non-obvious source-derived values in concise code comments close to the component. Link the stable reference in this spec or the relevant feature spec instead of scattering URLs through Slint.
4. Check whether a matching shared component and Octicon already exist in the repository.
5. If the evidence is unavailable or ambiguous, do not guess; request a product decision before implementation.

## Validation / 验证

- Compile the complete root Slint tree with `cargo check --locked`.
- Run `cargo fmt --all`, `cargo test --lib --locked`, and `git diff --check`.
- Manually compare the affected surface with the cited GitHub/Primer reference at the same effective scale in both light and dark mode.
- Verify narrow-window behavior, text wrapping/truncation, hover, pressed, focus, disabled, loading, and error states that apply.
- Capture or inspect a rendered screenshot for material visual changes. Compilation alone is insufficient.
- Report any reference that could not be accessed and every state that was not manually checked.

- 使用 `cargo check --locked` 编译完整根 Slint 树。
- 运行 `cargo fmt --all`、`cargo test --lib --locked` 和 `git diff --check`。
- 在相同有效缩放比例下，将受影响界面与引用的 GitHub/Primer 参考逐项对照，并同时检查明暗主题。
- 检查窄窗口、文本换行/截断，以及适用的 Hover、按下、焦点、禁用、加载和错误状态。
- 实质性视觉改动必须截图或检查实际渲染结果；仅编译通过不算完成。
- 交付时说明无法访问的参考资料以及未手工检查的状态。

## Toast reference / Toast 参考

The shared Toast in `ui/components/toast/toast.slint` follows the official Primer CSS Toast geometry: a horizontally centered, content-sized card capped at 450 px, 16 px vertical viewport margin (8 px below the 544 px breakpoint), a 48 px semantic icon rail, 16 px content padding, neutral canvas, semantic rail color, border, and medium shadow. `top`, `middle`, and `bottom` select only the vertical position. Success uses the Check Octicon, error uses Alert, informational text uses Info, and loading uses a spinner.

共享 Toast `ui/components/toast/toast.slint` 采用 Primer CSS 官方 Toast 几何：卡片始终水平居中、内容自适应且最大 450px，纵向视口边距 16px（低于 544px 时为 8px），48px 语义图标栏，内容内边距 16px，中性画布、语义色图标栏、边框和中等阴影。`top`、`middle`、`bottom` 只控制纵向位置。成功使用 Check Octicon，错误使用 Alert，普通信息使用 Info，加载使用 Spinner。

Reference: [Primer CSS deprecated Toast documentation](https://primer.style/css/storybook/?path=/docs/deprecated-toast--docs) and [GitHub's current guidance on accessible notifications and toasts](https://primer.style/accessibility/toasts/).

## Settings and About references / 设置与关于页面参考

Settings uses Primer's Settings Page composition: a `SplitPageLayout` with a
start-side `NavList` at regular and wide viewports, one active content region,
and a single-column navigation disclosure below the 768 px regular breakpoint.
The content uses `PageHeader`, `FormControl`, medium `TextInput` and `Button`,
`ToggleSwitch`, `Label`, and section-divider geometry. Application concepts are
grouped as General, Code hosting, AI Agent, and Diagnostics; changing visual
structure must not remove or rename the existing Slint/Rust settings callbacks.
The content is centered within equal left and right gutters, capped at 1120 px,
and must shrink to the allocated viewport width without introducing a
horizontal scrollbar. Above 1000 px the Settings gutters grow to 48 px so the
form column keeps clearer trailing space; the start-side NavList pane is 180 px
wide for the short section labels.

设置页面采用 Primer 的 Settings Page 组合：常规和宽窗口使用左侧
`NavList` + 单一内容区的 `SplitPageLayout`，低于 768px 的 regular
断点时切换为单列导航展开器。内容区采用 `PageHeader`、`FormControl`、
中号 `TextInput`/`Button`、`ToggleSwitch`、`Label` 及分区分隔线的官方
几何。应用设置固定分为 General、Code hosting、AI Agent、Diagnostics；
调整视觉结构不得删除或重命名现有 Slint/Rust 设置回调。
页面内容须在等宽的左右边距内居中，最大宽度为 1120px，并随实际分配的
视口宽度收缩，不得产生横向滚动条。宽度超过 1000px 时 Settings 外边距
增至 48px，避免表单贴紧右侧；左侧 NavList 对短标签使用 180px 宽度。

About uses the same full-page `PageHeader` hierarchy, Primer metadata `Label`,
bordered `Box` rows, semantic muted text, and a maximum readable content width.
It is a product-information page rather than a Material profile card.

关于页面采用同一套全页 `PageHeader` 层级、Primer 元数据 `Label`、带边框
`Box` 信息行、语义弱化文本及受控的可读内容宽度；它是产品信息页，不是
Material 风格的个人资料卡。

References: [Primer SplitPageLayout settings example](https://primer.style/product/components/split-page-layout/),
[Primer PageLayout](https://primer.style/product/components/page-layout/),
[Primer layout foundations](https://primer.style/product/getting-started/foundations/layout/),
[Primer PageHeader](https://primer.style/product/components/page-header/),
and [GitHub Desktop settings](https://docs.github.com/en/desktop/configuring-and-customizing-github-desktop/configuring-basic-settings-in-github-desktop).

## Breadcrumb and context header / 面包屑与上下文顶栏

The global route breadcrumb uses Primer Breadcrumbs' normal variant inside a
compact PageHeader context row. Ancestors are 14 px accent links with an
underline on hover; the current page uses the default foreground without an
accent color. Each route keeps one 16 px leading navigation icon for desktop
scanability. Items are separated by Primer's compact slanted divider. The row
uses the normal canvas background, a one-pixel default bottom
border, the 16 px PageHeader inset plus the shell's 4 px content gutter on its
leading edge, and a compact 40 px total height with its 32 px content vertically
centered. Route-history navigation remains available as a 32 px trailing
ArrowLeft IconButton with a tooltip; it must not compete with the breadcrumb as
a large text action.

全局路由面包屑采用 Primer Breadcrumbs 的 normal 变体，并放入紧凑的
PageHeader 上下文行。父级使用 14px 强调色链接，Hover 时显示下划线；
当前页使用默认前景色，不使用强调色；每个路由保留一个 16px 前置导航
图标，便于桌面端快速识别。项目之间采用 Primer 的紧凑斜线分隔符。顶栏
使用普通画布背景和 1px 默认底边框；左侧由 16px
PageHeader 内边距叠加 Shell 的 4px 内容边距，保证与页面标题对齐；总高度
为 40px，内部 32px 内容须严格垂直居中。路由历史返回操作保留为右侧
32px ArrowLeft IconButton 并提供 Tooltip，不得再以大号文字操作与面包屑
争夺视觉层级。

References: [Primer Breadcrumbs](https://primer.style/product/components/breadcrumbs/),
[Breadcrumbs guidelines](https://primer.style/product/components/breadcrumbs/guidelines/),
[Primer PageHeader](https://primer.style/product/components/page-header/), and
[Primer layout foundations](https://primer.style/product/getting-started/foundations/layout/).

## Workflow repository navigation / Workflow 仓库导航

The Workflow repository identity keeps project name, optional alias, provider,
and repository ID on the first line in that order. The remote repository URL
uses plain text with a copy action on the second line; it must not resemble an
editable input with a filled background or border.

Workflow 仓库身份区第一行按“项目名称、可选别名、Provider、仓库 ID”的
顺序展示。第二行仓库地址使用纯文本和复制操作，不得使用类似可编辑输入框
的填充背景或边框。

Workflow uses one content-sized UnderlineNav at every supported width. The
selected underline spans the complete tab hit target. Hover uses a square muted
background that stops one pixel above the shared bottom divider, so neighboring
tabs never interrupt or cover the continuous navigation baseline.

Workflow 在所有支持的宽度下统一使用内容宽度型 UnderlineNav。选中下划线
覆盖完整 Tab 命中宽度；Hover 使用无圆角的 muted 背景，并在公共底部分隔线
上方保留 1px，避免相邻 Tab 遮挡或打断连续导航基线。

## Workflow pull request list / Workflow PR 列表

Workflow PR filter controls use GitHub's content-sized filter-button geometry.
The dropdown TriangleDown Octicon is optically centered with the text. PR rows
keep the default canvas with no `Box-row--hover-gray` fill; the title is a
Primer accent link (`#0969da` / `#4493f8`) that underlines on title hover.
Relative-time tooltip hover stays on the metadata hit target only.
Each separator is anchored to the top edge of the following 72 px row, never
through the title or metadata. Open/Closed state labels follow Primer
degraded-experience rules: when a count is still loading or the count request
failed, hide the number and keep the label (optional “Count unavailable”
tooltip); never leave a perpetual `…` placeholder.

Workflow PR 筛选控件采用 GitHub 的内容宽度按钮几何。下拉 TriangleDown
Octicon 必须与文字进行光学垂直居中。PR 行保持默认画布，不使用
`Box-row--hover-gray` 整行底色；标题为 Primer 强调色链接（`#0969da` /
`#4493f8`），仅在标题 hover 时显示下划线。相对时间 Tooltip 仅绑定元数据
命中区域。每条分隔线固定在后一个 72px 行的顶部，不得穿过标题或元信息。
Open/Closed 状态标签遵循 Primer 降级体验：计数仍在加载或请求失败时隐藏
数字、保留标签（可选 “Count unavailable” Tooltip），不得长期停留在 `…`
占位。

References: [GitHub pull request list](https://github.com/openai/openai-python/pulls),
[Primer CSS Box component](https://primer.style/css/components/box), and
[Primer degraded experiences](https://primer.style/product/ui-patterns/degraded-experiences/).

## Project collection and dialogs / 项目集合与弹窗

The Project page uses Primer's full-page `PageHeader`, a medium search input,
and two Primer icon-only controls for switching between list and grid views.
The controls are 32 px square with an 8 px product-specified gap and a 16 px
gap before Reload. The list is one bordered collection of repository rows.
The responsive grid uses 1/2/3/4 columns at 480/720/1000 px and flat
bordered Primer Cards; windows at or above 1000 px stay on four columns so
aliases and paths keep usable width inside the 1120 px content cap.
Both views present a distinct project
heading, remote and local repository metadata, optional Labels, pull-request
counts, and permanently mounted terminal/edit/delete hit targets. Rows and
cards use the default canvas with a subtle hover canvas; they do not use
gradients, elevated Material cards, or a synthetic “add card”. The content is
centered, capped at 1120 px, and the primary `New project` action remains in
the page header.

项目页面采用 Primer 全页 `PageHeader`、中号搜索输入框及两个图标型视图按钮，
用于切换列表与网格视图。按钮为 32px 方形，按照产品决定彼此间隔 8px，并在
刷新按钮前保留 16px。列表是一个带边框的仓库行集合；响应式网格按
480/720/1000px 切换 1/2/3/4 列；达到 1000px 后保持四列，以便在 1120px
内容上限内给别名和路径留下可用宽度，并采用
扁平带边框的 Primer Card。两种视图都包含明确的项目标题、远程与本地仓库元数据、
可选 Label、Pull Request 计数，以及常驻的终端/编辑/删除命中区域。列表和
卡片使用默认画布与轻量 Hover 画布；禁止渐变、Material Elevated Card 或
虚构的“新增卡片”。内容居中且最大宽度为 1120px，主操作 `New project`
固定在页头。

New and Edit use Primer Dialog's 480 px large width, 16 px header/body/footer
safe area, 32 px medium controls, 6 px field radius, and a single scrolling
form column. Delete uses the dedicated 320 px ConfirmationDialog composition:
name the affected project, explain that only the Usher record is removed, and
label the danger action `Delete project`. Do not use generic wording such as
“Are you sure?” or `Confirm`.

新增和编辑采用 Primer Dialog 的 480px large 宽度、Header/Body/Footer 的
16px 安全边距、32px 中号控件、6px 输入框圆角及单列滚动表单。删除采用
320px 的专用 ConfirmationDialog 结构：明确项目名称、说明仅删除 Usher
记录，并将危险操作命名为 `Delete project`；禁止使用“Are you sure?”或
`Confirm` 等含糊文案。

References: [Primer Card](https://primer.style/product/components/card/),
[Primer SegmentedControl](https://primer.style/product/components/segmented-control/),
[Primer Dialog guidelines](https://primer.style/product/components/dialog/guidelines/),
[Primer ConfirmationDialog](https://primer.style/product/components/confirmation-dialog/),
and [Primer delete pattern](https://primer.style/product/scenario-patterns/delete/).

## Global terminal drawer / 全局终端抽屉

The global Terminal drawer uses a Primer bordered surface: 6 px radius while
docked, 12 px radius plus medium shadow while floating, a 40 px subtle header,
semantic connection status, repository working-directory metadata, and 32 px
invisible/selected actions. Terminal, fullscreen, restore, close, and resize
controls use official 16 px Octicons. The terminal canvas follows Primer
default-canvas light/dark colors; terminal text remains monospace and ANSI span
colors remain owned by the Rust terminal renderer.

全局 Terminal Drawer 使用 Primer 带边框浮层：Docked 状态为 6px 圆角，
Floating 状态为 12px 圆角和中等阴影，Header 高 40px 并使用 subtle 画布，
显示语义化连接状态与仓库工作目录，操作采用 32px invisible/selected 几何。
Terminal、全屏、还原、关闭与 Resize 均使用官方 16px Octicon。终端画布
跟随 Primer 明暗 default canvas；文本保持等宽字体，ANSI span 颜色仍由
Rust Terminal Renderer 管理。

References: [Primer IconButton](https://primer.style/product/components/icon-button/),
[Primer color primitives](https://primer.style/product/primitives/color/), and
[Primer Octicons](https://primer.style/octicons/).
