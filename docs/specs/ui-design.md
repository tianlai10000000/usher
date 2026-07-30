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
- A reusable pattern belongs in `ui/components/`; page composition belongs in `ui/views/`. A page must reuse the shared pattern rather than locally recreating a visually similar variant. Page and section tabs must follow the [UnderlineNav tabs](#underlinenav-tabs--underlinenav-页签) contract; do not invent alternate tab chrome.
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
- 复用模式放入 `ui/components/`，页面装配放入 `ui/views/`；页面不得重复实现已有共享模式的近似变体。页面级与分区级 Tab 必须遵循 [UnderlineNav 页签](#underlinenav-tabs--underlinenav-页签) 契约，禁止另起一套 Tab 外观。
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

## Notify reference / Notify 参考

The shared floating Notify follows Primer Banner's compact structure and messaging guidance while using the neutral canvas treatment appropriate for a transient overlay. Notifications form a top-right stack with a 16 px viewport inset, 8 px item gap, bounded scrolling, and a 360 px content width. Each notification uses an 8 px outer padding, 1 px neutral border, 6 px radius, 16 px semantic Octicon, 14 px text, and a 32 px dismiss target. Long text wraps. Standard and actionable notifications use the same renderer; actionable notifications expose no more than two actions below the message. The action group follows `GitHubDialog` footer alignment: it is pinned to the trailing edge, with secondary actions first and the primary action last. Only the recommended final action in a multi-action notification is primary; a single dismissive action remains secondary. Light and dark modes use their neutral canvas and border tokens, with status communicated by the Octicon color rather than a tinted card.

共享浮层 Notify 遵循 Primer Banner 的紧凑结构和消息规范，同时采用更适合临时浮层的中性画布。通知以右上角队列展示，距视口 16px、条目间距 8px、超高时在限定区域滚动，内容宽度为 360px。每条通知使用 8px 外层内边距、1px 中性边框、6px 圆角、16px 语义 Octicon、14px 文本和 32px 关闭热区；长文本自动换行。普通通知和带操作通知使用同一渲染器；带操作通知最多展示两个位于正文下方的操作。操作组与 `GitHubDialog` 底部一致，整体靠右，次操作在前、主操作在后。只有多操作通知中最后一个推荐操作使用主按钮，单个取消类操作保持次按钮。明暗主题均采用中性画布和边框，状态只由 Octicon 颜色表达，不再给整张卡片染色。

Settings-triggered update checks report every terminal result through Notify: current version, available update, or failure. Update availability may offer `Later` and `Update now`; failure messages remain concise and never expose raw transport errors.

设置页主动检查更新的所有终态都通过 Notify 反馈：已是最新版、发现新版本或检查失败。发现新版本时可以提供 `Later` 和 `Update now`；失败消息保持简洁，不向用户暴露原始网络错误。

References: [Primer Banner](https://primer.style/product/components/banner/), [Banner guidelines](https://primer.style/product/components/banner/guidelines/), [notification messaging](https://primer.style/product/ui-patterns/notification-messaging/), and the [Primer React Banner implementation](https://github.com/primer/react/tree/main/packages/react/src/Banner).

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

Manual Settings saves report completion only after the settings file has been
persisted. Saving code-hosting tokens, enterprise API configuration, or AI
Agent settings shows one bottom success Toast; persistence failures show one
bottom error Toast. AI Agent connection-test success and failure use the same
bottom placement.

设置页手工保存操作仅在配置文件实际持久化后报告完成。保存代码托管 Token、
企业 API 配置或 AI Agent 配置成功时显示一个底部成功 Toast；持久化失败时
显示一个底部错误 Toast。AI Agent 连接测试的成功与失败也使用相同的底部
位置。

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
a large text action. The bar may use a translucent canvas so scrolled page
content shows through, but it must mount a full-bar TouchArea shield so clicks
cannot reach controls underneath; crumb and Back hit targets remain interactive.

全局路由面包屑采用 Primer Breadcrumbs 的 normal 变体，并放入紧凑的
PageHeader 上下文行。父级使用 14px 强调色链接，Hover 时显示下划线；
当前页使用默认前景色，不使用强调色；每个路由保留一个 16px 前置导航
图标，便于桌面端快速识别。项目之间采用 Primer 的紧凑斜线分隔符。顶栏
使用普通画布背景和 1px 默认底边框；左侧由 16px
PageHeader 内边距叠加 Shell 的 4px 内容边距，保证与页面标题对齐；总高度
为 40px，内部 32px 内容须严格垂直居中。路由历史返回操作保留为右侧
32px ArrowLeft IconButton 并提供 Tooltip，不得再以大号文字操作与面包屑
争夺视觉层级。顶栏可使用半透明画布以透出滚动内容，但必须挂载整栏
TouchArea 屏蔽层，阻止点击穿透到下方控件；面包屑项与 Back 仍可交互。

References: [Primer Breadcrumbs](https://primer.style/product/components/breadcrumbs/),
[Breadcrumbs guidelines](https://primer.style/product/components/breadcrumbs/guidelines/),
[Primer PageHeader](https://primer.style/product/components/page-header/), and
[Primer layout foundations](https://primer.style/product/getting-started/foundations/layout/).

## Application navigation and utility status / 应用导航与状态栏

The persistent application rail follows Primer NavList and ActionList semantics
in an icon-only desktop form. It is 48 px wide on a subtle canvas with a
one-pixel trailing default border. Every route uses a consistent 20 px leading
visual inside a mounted 32 px, 6 px-radius target. Items are separated by 4 px;
default, hover, pressed, and current-route states use semantic muted foreground
and transparent-control colors in both themes. The current route is identified
by the selected control surface rather than an unrelated brand color. Tooltips
remain available for every icon-only destination.

固定应用导航栏以仅图标桌面形态遵循 Primer NavList 和 ActionList 语义。
导航栏宽 48px，使用 subtle 画布并在尾侧绘制 1px 默认边框。所有路由统一
使用 20px leading visual，放在常驻的 32px、6px 圆角命中面内；项目间距
为 4px。默认、Hover、按下和当前路由状态在明暗主题下均使用语义化的弱化
前景色与透明控件状态色；当前路由通过 selected control surface 表达，
不再使用无关的品牌色。每个仅图标入口必须保留 Tooltip。

GitHub's web App Footer is not a match for Usher's persistent desktop runtime
status. The bottom surface is therefore a compact Primer utility bar: 28 px
high, subtle canvas, one-pixel default top border, 12 px muted labels, and
mounted hover/pressed targets. Transient update and notification state stays
left-aligned; connection and version metadata stay right-aligned. Popovers use
the default canvas, default border, 6 px radius, and Primer floating shadow.
Status color is reserved for connection or error meaning.

GitHub 网页 App Footer 不对应 Usher 常驻的桌面运行状态，因此底部采用紧凑
Primer utility bar：高度 28px、subtle 画布、1px 默认顶边框、12px 弱化
标签及常驻 Hover/按下命中区。更新与通知等临时状态左对齐，网络连接和版本
元信息右对齐；Popover 使用默认画布、默认边框、6px 圆角和 Primer floating
阴影。状态色仅用于表达连接或错误语义。

References: [Primer navigation patterns](https://primer.style/product/ui-patterns/navigation/),
[Primer NavList guidelines](https://primer.style/product/components/nav-list/guidlines/),
[Primer ActionList](https://primer.style/product/components/action-list/),
[Primer layout foundations](https://primer.style/product/getting-started/foundations/layout/),
and [Primer color usage](https://primer.style/product/getting-started/foundations/color-usage/).

## UnderlineNav tabs / UnderlineNav 页签

Every page or section tab strip in Usher must reproduce GitHub Primer
**UnderlineNav** (repository page tabs such as Actions / Projects). Do not
invent square full-height hover washes, Material tabs, or SegmentedControl
substitutes for primary view switching. Scope toggles that are not page tabs
(for example Tag Local/Remote) keep using SegmentedControl.

项目内所有页面级或分区级 Tab 条都必须复刻 GitHub Primer **UnderlineNav**
（仓库页 Actions / Projects 一类页签）。禁止自创铺满整高的方形 Hover 底、
Material Tabs，或用 SegmentedControl 代替主视图切换。非页签的范围切换
（如 Tag 的 Local/Remote）继续使用 SegmentedControl。

### Canonical geometry / 标准几何

| Element | Value | Notes |
| --- | --- | --- |
| Row height | 48 px | 8 px top inset + 32 px item + 8 px gap above the baseline |
| Item control height | 32 px | Primer `--base-size-32` / `--control` medium |
| Item border-radius | 6 px | Primer medium radius on the hover wash only |
| Item gap | 8 px | Primer condensed stack gap between items |
| Label | 14 px | Semibold (600) when selected, regular (400) otherwise |
| Leading Octicon | 16 px | Optional; muted foreground when present |
| Shared baseline | 1 px | Full-width muted border under the row |
| Selected underline | 2 px coral | Light `#fd8c73` / dark `#f78166`; spans the item width and sits on the baseline |

Hover fills only the 32×N rounded control with neutral-muted wash (light
`#eaedf0`, dark `#656c7633`). The wash must never cover or interrupt the
shared 1 px baseline. Selected state keeps a transparent control surface and
shows only the coral underline on the baseline (bold label). Idle remains
transparent.

Hover 只填在 32×N 圆角控件上，使用中性弱化底色（浅色 `#eaedf0`，深色
`#656c7633`），不得遮挡或打断整行 1px 基线。选中态控件背景仍透明，仅在
基线上显示珊瑚色下划线，标签加粗。默认态保持透明。

### Row composition / 行结构

```text
[ 8px top ]
[ 32px UnderlineItem ][ 8px gap ][ 32px UnderlineItem ] …
[ 8px gap above baseline ]
[ 1px shared border; selected 2px coral underline overlays it ]
```

Mount the shared border in the row first (or ensure items leave the bottom
8 px clear). Place the selected underline on the baseline (`y = rowHeight - 2`
for a 48 px row host, or `y = controlHeight + 6` when the control itself is
only 32 px tall inside that row).

先绘制整行共享底边（或保证 item 底部留出 8px）。选中下划线落在基线上：
48px 行宿主使用 `y = rowHeight - 2`；若控件本身只有 32px 高，则使用
`y = controlHeight + 6`。

### Ownership / 现有实现

| Surface | Component |
| --- | --- |
| Workflow repository nav (Pull requests / Tags / …) | `WorkflowWideTab` in `ui/views/workflow/workflow.slint` |
| Tag Add / Remove | `TagTabChip` in `ui/views/workflow/tag.slint` |
| PR review Conversation / Commits / Files changed | `PrTabButton` in `ui/components/pr_review/shell.slint` |

New tab strips must reuse or extract a shared UnderlineNav item rather than
forking another hover/underline variant. Record Primer-derived numbers in a
short source comment; keep the durable contract in this section.

新建 Tab 条必须复用或抽成共享 UnderlineNav item，不得再分叉另一套
Hover/下划线变体。Primer 派生数值用简短源码注释记录；稳定契约以本节为准。

### Regression checklist / 回归清单

- Selected: bold label + coral underline on the shared baseline; no hover-sized gray fill required.
- Hover (unselected): 6 px rounded wash, 32 px tall, baseline fully visible underneath.
- Neighboring tabs do not clip or paint over the continuous 1 px baseline.
- Light and dark tokens match the table above.
- Icon + label (+ optional counter) stay vertically centered in the 32 px control.

- 选中：加粗标签 + 基线上的珊瑚色下划线；不要求出现 Hover 尺寸的灰底。
- Hover（未选中）：6px 圆角、32px 高的弱化底，基线完整可见。
- 相邻 Tab 不得裁切或盖住连续的 1px 基线。
- 明暗色值与上表一致。
- 图标 + 标签（及可选计数）在 32px 控件内垂直居中。

References: [Primer UnderlineNav](https://primer.style/product/components/underline-nav/),
[primer/react `UnderlineTabbedInterface.module.css`](https://github.com/primer/react/blob/main/packages/react/src/internal/components/UnderlineTabbedInterface.module.css),
and github.com repository navigation (Actions / Projects tabs).

## Workflow repository navigation / Workflow 仓库导航

The Workflow repository identity keeps project name, optional alias, provider,
and repository ID on the first line in that order. At 720 px and above, remote
and local paths share a plain-text metadata row with copy/reveal actions. Below
720 px, matching GitHub compact repository navigation, paths disappear from the
header and move into the trailing kebab overlay.

Workflow 仓库身份区第一行按“项目名称、可选别名、Provider、仓库 ID”的
顺序展示。720px 及以上时，远程与本地路径在同一纯文本元数据行显示复制/
打开操作；低于 720px 时对齐 GitHub 紧凑仓库导航，从页头隐藏路径并收进
尾部 kebab 浮层。

Workflow repository view switching uses the shared UnderlineNav contract in
[UnderlineNav tabs](#underlinenav-tabs--underlinenav-页签) at every supported
width (content-sized items with optional leading Octicons).

Workflow 仓库视图切换在所有支持宽度下统一使用
[UnderlineNav 页签](#underlinenav-tabs--underlinenav-页签) 契约（内容宽度
item，可选 leading Octicon）。

## Workflow execution steps / Workflow 执行步骤

Workflow execution steps reproduce GitHub Actions job logs as one disclosure
list below the job heading, muted result summary, and divider. The surface uses
GitHub/Primer default, subtle, border, and foreground tokens for the active
light or dark application theme. Each collapsed row is 40 px high and
uses, from left to right, a 16 px ChevronRight/ChevronDown Octicon, a 20 px
status slot, one 14 px step name, and a right-aligned 12 px duration. Pending
steps show an empty duration cell, running steps show an ellipsis, and completed, failed,
or cancelled steps retain their measured elapsed time. Completed success marks
follow GitHub Actions emphasis: muted CheckCircleFill when collapsed, default
foreground when expanded. Failed steps use the matching 16 px XCircleFill
(danger colorize), not a larger painted disc behind a plain X. Running, paused,
and cancelled states keep their semantic Octicons and colors. Rows never become independent
cards, never show numbered badges, and never add a trailing `Logs` text link.
The full row is the disclosure target. Collapsed idle rows stay on the bare log
canvas (no header wash). Hover on an expandable collapsed row shows a lighter
inset 6 px-radius wash than the expanded header
(light hover `#f3f4f6` / expanded `#eaeef2`; dark hover `#161b22` / expanded
`#21262d`). Expanded output stays directly under its
step and reproduces GitHub's muted line-number column on the shared canvas.
Each logical log line owns one row and one source line number. Long text wraps
within that row; continuation lines stay aligned under the message and do not
repeat the source line number. The nominal line box is 28 px with 12 px
monospace text measured-box centered in that wash (not Text
`vertical-alignment: center`, which reads optically high); wrapped rows grow
with 7 px vertical padding. No invented gutter fill or divider separates
the columns. Command output
uses GitHub's continuous `Run …` log format without invented stdout/stderr
headings or blank sections. Each disclosure is independent, so opening one step
does not close another; failed steps start expanded and may still be collapsed
by the user. Pending pre-run plans remain visible in Usher because their
steps are deterministic, but rows without logs do not expose disclosure
chevrons. The outer Workflow
page height follows the disclosure list's measured height so the page
ScrollView can always reach the last step above the persistent status bar.
This presentation is owned by `ui/components/jobs/job.slint`; workflow views
must pass normalized Job snapshots instead of copying the rows.

Workflow 执行步骤严格复刻 GitHub Actions Job logs，在当前应用明暗主题
对应的 Primer default/subtle 画布上显示 Job 标题、弱化结果摘要、分隔线和
统一 disclosure list。折叠行高固定为 40px，从左到右依次为 16px
ChevronRight/ChevronDown Octicon、20px 状态位、一行 14px 步骤名称以及
右对齐的 12px 耗时；pending 耗时单元格为空、running 显示省略号，完成、失败或
取消后保留实际测量的执行耗时。成功状态与 GitHub Actions 一致：折叠为 muted
CheckCircleFill，展开为 default 前景色；失败使用同尺寸的 16px XCircleFill
（danger 着色），不得用更大的实心底圆叠白 X；运行、暂停和取消状态
使用对应语义 Octicon 与颜色。步骤不得拆成独立卡片，不得使用数字徽标，也
不得在尾部添加 `Logs` 文字链接。整行都是展开命中区。折叠且未 Hover 的行
保持日志画布底色、无 header 底；可展开折叠行 Hover 时使用比展开头更浅的
6px 圆角内缩底（浅色 Hover `#f3f4f6` / 展开 `#eaeef2`；深色 Hover `#161b22` /
展开 `#21262d`）。展开内容直接接在对应
步骤下方，使用 GitHub 式共享画布上的弱化行号列。每个逻辑日志行只占一个源行号；
长文本在本行内换行，续行继续对齐消息列且不重复行号。名义行盒为 28px、12px
等宽字按实测文字盒在 Hover 底内垂直居中（不用 Text `vertical-alignment:
center`，否则会光学偏上）；换行时上下各留 7px。行号列不得自行添加底色或分隔线。命令输出采用 GitHub 连续的
`Run …` 格式，不自行添加
stdout/stderr 标题或空白区。每个 disclosure 独立展开，打开一个步骤不得关闭
其他步骤；失败步骤默认展开，但用户仍可将其折叠。Usher 的执行前计划
因为步骤确定而保持可见，但没有日志的 pending 行不显示展开箭头。Workflow
外层高度必须
跟随 disclosure list 的实测高度，使页面 ScrollView 始终能把最后一步滚到
常驻状态栏上方。
该展示统一归 `ui/components/jobs/job.slint` 所有；Workflow 页面必须传入
归一化 Job 快照，不得复制 Step 行。

References: [GitHub Actions workflow-run logs](https://docs.github.com/en/actions/how-tos/monitor-workflows/use-workflow-run-logs),
[GitHub Actions quickstart job-log screenshot](https://docs.github.com/assets/cb-95207/images/help/repository/actions-quickstart-logs.png),
[GitHub Actions expanded job-log screenshot](https://docs.github.com/assets/cb-53977/images/help/repository/actions-quickstart-log-detail.png),
and [Primer Octicons](https://primer.style/product/foundations/icons/).

## Workflow pull request list / Workflow PR 列表

Workflow PR filter controls reproduce GitHub's responsive list header. Open /
Closed and filters stay on one 48 px row. At 620 px and above, Reviews,
Assignee, and Sort use content-sized buttons; below it, Assignee remains visible
beside a borderless kebab IconButton (invisible variant: muted hover/selected
fill only). The kebab ActionMenu is a compact 200 px panel that groups review
filters and sort choices with checkmarks and separators. Individual filter
menus use the same 200 px width and 32 px ActionList rows. The dropdown
TriangleDown Octicon is optically centered with the text. Menus are mutually
exclusive and switching opens the target on the first click. Pagination is a sibling
footer below the list card (16px gap), not inside the card border; PrFlow height
is pinned to list + footer so a taller Workflow panel cannot stretch the card
over the pager. PR rows
keep the default canvas with no `Box-row--hover-gray` fill; the title is a
Primer accent link (`#0969da` / `#4493f8`) that underlines on title hover.
Relative-time tooltip hover stays on the metadata hit target only.
Each separator is anchored to the top edge of the following 72 px row, never
through the title or metadata. Open/Closed state labels follow Primer
degraded-experience rules: when a count is still loading or the count request
failed, hide the number and keep the label (optional “Count unavailable”
tooltip); never leave a perpetual `…` placeholder.

Workflow PR 筛选严格复刻 GitHub 响应式列表页头：Open/Closed 与筛选保持
在同一个 48px 行。620px 及以上使用 Reviews、Assignee、Sort 内容宽度按钮；
低于该宽度时只直接保留 Assignee，旁边使用无边框 kebab IconButton（invisible
变体，仅 muted hover/选中底色）。kebab ActionMenu 为紧凑的 200px 面板，
以勾选项和分隔线分组展示 Review 筛选与排序；独立筛选菜单同样使用 200px
宽度与 32px ActionList 行。TriangleDown Octicon 与文字光学居中；菜单互斥，
第一次点击即可切换目标。分页是列表
卡片下方的兄弟页脚（间隔 16px），不得落在卡片边框内；PrFlow 高度钉在
列表+页脚，避免更高的 Workflow 面板把卡片撑过分页条。PR 行保持
默认画布，不使用 `Box-row--hover-gray` 整行底色；标题为 Primer 强调色链接
（`#0969da` / `#4493f8`），仅在标题 hover 时显示下划线。相对时间 Tooltip
仅绑定元数据命中区域。每条分隔线固定在后一个 72px 行的顶部，不得穿过标题
或元信息。Open/Closed 状态标签遵循 Primer 降级体验：计数仍在加载或请求
失败时隐藏数字、保留标签（可选 “Count unavailable” Tooltip），不得长期
停留在 `…` 占位。

References: [GitHub pull request list](https://github.com/openai/openai-python/pulls),
[Primer CSS Box component](https://primer.style/css/components/box), and
[Primer degraded experiences](https://primer.style/product/ui-patterns/degraded-experiences/).

## Project collection and dialogs / 项目集合与弹窗

The Project page uses Primer's full-page `PageHeader`, a medium search input,
and two Primer icon-only controls for switching between list and grid views.
The controls are 32 px square with an 8 px product-specified gap and a 16 px
gap before Reload. The list is one bordered collection of repository rows.
The responsive grid uses 1/2/3/4 columns at 480/720/1000 px and flat
bordered Primer Cards (condensed repo-card density); windows at or above
1000 px stay on four columns inside the 1120 px content cap.
Both views present a distinct project heading, an optional alias Label capped
at 168 px with ellipsis when longer, remote and local repository paths (elided),
optional topic Labels, pull-request counts, and permanently mounted
terminal/edit/delete hit targets. Rows and cards use the default canvas
with a subtle hover canvas; they do not use gradients, elevated Material cards,
or a synthetic “add card”. The content is centered, capped at 1120 px, and the
primary `New project` action remains in the page header.

项目页面采用 Primer 全页 `PageHeader`、中号搜索输入框及两个图标型视图按钮，
用于切换列表与网格视图。按钮为 32px 方形，按照产品决定彼此间隔 8px，并在
刷新按钮前保留 16px。列表是一个带边框的仓库行集合；响应式网格按
480/720/1000px 切换 1/2/3/4 列；达到 1000px 后保持四列，并在 1120px
内容上限内采用扁平带边框的 Primer Card（紧凑仓库卡片密度）。两种视图都包
含明确的项目标题、可选别名 Label（最长 168px，超出省略）、远程与本地仓库
路径（过长省略）、可选 Topic Label、Pull Request 计数，以及常驻的终端/
编辑/删除命中区域。列表和卡片使用默认画布与轻量 Hover 画布；禁止渐变、
Material Elevated Card 或虚构的“新增卡片”。内容居中且最大宽度为 1120px，
主操作 `New project` 固定在页头。

New and Edit use Primer Dialog's 480 px large width, 16 px header/body/footer
safe area, 32 px medium controls, 6 px field radius, and a single scrolling
form column. Delete uses the dedicated 320 px ConfirmationDialog composition:
name the affected project, explain that only the Usher record is removed, and
label the danger action `Delete project`. Do not use generic wording such as
“Are you sure?” or `Confirm`.

Every application-owned modal uses the shared
`ui/components/dialog/github_dialog.slint` shell. `DialogManager` is only an
imperative state/action adapter; project forms, PR review/merge/revert and
commit-range content, update prompts, and download progress must not recreate
backdrop, surface, header, close button, footer, or action-state chrome locally.

新增和编辑采用 Primer Dialog 的 480px large 宽度、Header/Body/Footer 的
16px 安全边距、32px 中号控件、6px 输入框圆角及单列滚动表单。删除采用
320px 的专用 ConfirmationDialog 结构：明确项目名称、说明仅删除 Usher
记录，并将危险操作命名为 `Delete project`；禁止使用“Are you sure?”或
`Confirm` 等含糊文案。

所有应用自有 Modal 都必须使用共享
`ui/components/dialog/github_dialog.slint` 外壳。`DialogManager` 只负责
命令式状态与操作适配；项目表单、PR Review/Merge/Revert、Commit Range、
更新提示和下载进度不得在页面内重复实现遮罩、Surface、Header、关闭按钮、
Footer 或操作状态样式。

References: [Primer Card](https://primer.style/product/components/card/),
[Primer SegmentedControl](https://primer.style/product/components/segmented-control/),
[Primer Dialog guidelines](https://primer.style/product/components/dialog/guidelines/),
[Primer ConfirmationDialog](https://primer.style/product/components/confirmation-dialog/),
and [Primer delete pattern](https://primer.style/product/scenario-patterns/delete/).

## Global terminal drawer / 全局终端抽屉

Page-corner scroll and terminal actions use Primer medium IconButtons (32×32,
6 px radius, 16 px Octicons), not Material circular FABs. Back to top is the
default variant with `octicon-arrow-up-16`; the terminal control is primary
(green) with `octicon-terminal-16`. Both use Primer tooltip labels and a small
floating shadow because they sit above scrolling content.

页面右下角的返回顶部与终端入口采用 Primer medium IconButton（32×32、
6px 圆角、16px Octicon），禁止 Material 圆形 FAB。返回顶部为 default 变体
并使用 `octicon-arrow-up-16`；终端入口为 primary（绿色）并使用
`octicon-terminal-16`。二者均带 Primer Tooltip；因悬浮于滚动内容之上，
使用小尺寸 floating 阴影。

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
