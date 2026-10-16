# Tooltip Implementation Contract / Tooltip 实现规范

## Required reading / 强制阅读

Read this document **before adding, changing or migrating any Tooltip** in Usher.
This includes icon actions, list delegates, modal controls and independently
compiled ComponentFactory pages. `AGENTS.md` makes this a repository-wide gate.

项目内**新增、修改、迁移任何 Tooltip 前必须先阅读本文**，包括图标按钮、列表操作、
弹窗操作及独立 ComponentFactory 页面。此要求已接入根 `AGENTS.md`。

## Failure modes / 反复出现的原因

| 症状 / Symptom | 原因 / Cause | 正确处理 / Required treatment |
| --- | --- | --- |
| 编辑按钮显示删除或查看流程文案 | 独立 factory/复用 delegate 曾串用兄弟节点的动态字符串绑定 | 操作叶子发布固定 action kind，文字留在根层固定 Surface |
| 文案正确但被短框截断 | 测量节点保留上一个短文案的 preferred-width，绘制文案已更新 | 每种固定操作常驻独立测量/绘制节点，不能只加固定宽度 |
| 靠窗口边缘或滚动区被裁切 | Tooltip 在带 clip 的按钮、行、ScrollView 内绘制 | 根窗口或所属 factory 的全窗口 Portal，复用 flip/shift/size |
| 点击、关闭弹窗或离开页面后残留 | Anchor 消失，未释放所有权或失去续租 | 点击/关闭主动 dismiss，保留 lease 过期兜底 |
| 层级提高仍然被裁切 | z 只改变当前合成层，不能越过祖先 clip | 修复 Portal 挂载和视口坐标，不堆叠更大的 z |
| 首访持久 factory 页面后返回 Home 仍保留大块图形内存 | 每个固定动作各有一个全窗口定位宿主，且以 opacity 隐藏，仍可能分别形成离屏层 | 每个 Portal 只用一个 `GitHubTransientFloatingOverlayHost`；用 `active` 控制其内部定位树，按动作显示独立固定文案 Surface |

History: UI-033 (ancestor clipping), UI-103 (factory label/measurement aliasing),
UI-111 (literal leaf binding isolation), UI-124 (Hook recurrence), UI-140
(Settings full-window opacity layers), and UI-147 (the same structural risk in
Messages) in
[`bug-history.md`](../../bug-history.md). These are observed application/runtime
failures and confirmed source-level risks, not proof that every Slint string
binding is broken. Fixed factory
actions must use the isolated path below; do not assume the generic portal's
alternating nodes solve the factory case.

## Ownership / 组件选择

| 场景 / Context | 使用路径 / Implementation |
| --- | --- |
| 普通主窗口树中的标准图标按钮 | `GitHubIconButton` 的 `tooltip-text`，宿主 `GitHubTooltipOverlayHost` |
| 普通非按钮锚点 | `GitHubTooltip` 定位器 + 根层 `GitHubTooltipOverlayHost` |
| 独立 factory 或复用列表中的固定操作 | 独立固定操作叶子 + action-kind 状态 + 一个根层 `GitHubTransientFloatingOverlayHost` + 常驻固定文案 Surface |
| 键位提示 | 从 `src/shortcuts/` 注册表取标签，执行与展示共用来源 |

Checked implementation references:

- [`GitHubIconButton`](../../ui/components/button/github_icon_button.slint):
  mounted hit target, keyboard focus, click dismissal, `tooltip-active`.
- [`GitHubTooltip`](../../ui/components/github_tooltip.slint): locator, lease,
  shared themed surface and matching text measurement.
- [`GitHubTransientFloatingOverlayHost`](../../ui/components/github_floating_overlay.slint):
  the public lifecycle and placement boundary for persistent root portals. Its
  `active` input alone controls the private full-window geometry tree's
  visibility while retaining content-neutral offset/flip/shift and pixel snapping.
  Feature code must not instantiate that private geometry tree directly.
- [`GitHubTooltipOverlayHost`](../../ui/components/github_tooltip_overlay.slint):
  generic root portal; **not the fixed-action factory isolation recipe**.
- [`Message actions`](../../ui/components/messages/github_message_actions.slint),
  [`Message Tooltip portal`](../../ui/components/messages/github_message_tooltip.slint),
  [`Message state`](../../ui/state/message_tooltip_overlay_state.slint): literal
  action/enum-only portal with one transient host and three fixed surfaces.
- [`Hook actions and portal`](../../ui/components/settings/github_hook_tooltip.slint),
  [`Settings root`](../../ui/views/settings.slint): fixed action types, literal
  surfaces and one transient full-window positioning host inside the independent
  Settings factory. The six surfaces remain instantiated; `active` is false
  while idle.
- [`Agent actions and portal`](../../ui/components/settings/github_ai_agent_tooltip.slint)
  keep seven literal action kinds and mounted I18n surfaces inside one factory-root
  transient host. Activate/Current reuse the shared Toggle; Current and disabled
  Delete retain hover-only hit targets and never admit actions. Bounds intersect
  list and scroll viewport; query/page/rows/scroll/layout/modal/route changes
  dismiss the claim, and token/lease checks reject stale publishers.
  TestConnection has its own fixed Plug action, literal kind and mounted
  `I18n.common-test-connection` measure/paint Surface; it must not borrow Edit's
  claim or dynamic text. The 168px row group contains Toggle/Test/Edit/Delete.
  Agent 的 TestConnection 使用独立固定 Plug 叶子、固定 kind 和常驻
  `I18n.common-test-connection` 测量/绘制 Surface；七种动作共用单个根 Portal，
  不得复用 Edit 身份或动态文案。168px 行操作组依次为 Toggle/Test/Edit/Delete。
- [`Agent Header Delete`](../../ui/components/settings/github_ai_agent_header_tooltip.slint)
  owns a separate single literal action and fixed I18n Delete Surface, with one
  transient portal sibling to the editor frame. It is bounded to the actual
  scrolling body, not clipped inside a Header row; no Header text/value crosses
  the portal. Busy/object mode disables publishing. Header/session revisions,
  row changes, scroll, mode switch and close dismiss claims; tokens and leases
  reject stale publishers. The row reuses the shared 32×32 trash IconButton.
  Header 删除使用独立单动作固定叶子和 I18n Surface，单个 transient Portal 与编辑器
  frame 同级、以实际滚动正文为边界，不跨 Portal 传 Header 名称/值。busy/对象模式
  不发布，行/revision、滚动、模式和关闭均撤销 claim；不能复用 Agent 列表的 Delete
  身份或把提示挂在受裁切的行内。
- [`Fixed anchor geometry`](../../ui/components/github_fixed_tooltip_anchor.slint)
  owns only geometry, viewport eligibility and lease mechanics. Its callbacks
  carry no label or action kind; each semantic leaf publishes its own literal
  identity. Shared PR leaves belong to
  [`workflow/github_pr_factory_tooltip_anchors.slint`](../../ui/components/workflow/github_pr_factory_tooltip_anchors.slint),
  so production PR widgets do not depend on the development showcase tree.
- [`Playground action portal`](../../ui/components/showcase/github_showcase_tooltip.slint)
  and [`factory state`](../../ui/state/factory_tooltip_state.slint) own the static
  playground's literal actions and fixed I18n surfaces. Its full-window factory
  enables that local path; integrated live preview retains ordinary publishing.
  See the [playground contract](ui-components.md) for the two compilation roots.
  Its [`creation adapter`](../../src/ui_components/mod.rs) shares MainWindow's
  actual `WindowAdapter` and restores the shell root immediately after the
  shared-window constructor. This ensures destroyed Tooltip/model item trees
  release resources in the renderer that painted them. It supplements the
  literal-leaf recipe; it does not replace fixed action/measurement isolation.

## Factory recipe / 工厂内正确做法

1. Locate the actual compiled UI root and its clipping ancestors. Each independent
   factory owns its local state and full-window portal; MainWindow globals are
   not implicitly shared with that product.
2. Keep separate component types for semantically distinct actions. Each leaf
   directly publishes its literal kind, such as `HookTooltipKind.Edit`. Do not
   consolidate them into a delegate receiving a dynamic text/kind input.
3. Reuse button chrome and focus/hover state. Suppress its generic visual Tooltip
   with `show-tooltip: false`; do not leave two tooltip publishers active.
4. Publish only action identity and the complete anchor rectangle. Text remains
   an I18n binding at a fixed root Surface. Each action owns separate, permanently
   mounted measure and paint nodes. Every fixed-action Portal owns exactly one
   `GitHubTransientFloatingOverlayHost`; set `active` from whether an action kind
   is claimed and select the matching fixed Surface with `visible`. Do not apply
   opacity or `visible` to the transient host, create one host per action, remount
   text with `if kind`, or share a `kind ? label-a : label-b` measured Surface.
5. Use the transient host for viewport positioning. Pass the actual
   window bounds, root-relative anchor geometry, and matching measured surface
   dimensions. Labels must wrap within the viewport, including longer locales.
   Default to centered placement over the anchor and center text inside the
   surface. Only viewport collision handling should shift Hook tips sideways;
   do not use End alignment as a blanket workaround for edge clipping.
   默认提示框对准锚点中心，框内文字水平及垂直居中；Hook 提示仅在碰撞视口边界时
   自动偏移，不能把右对齐作为所有按钮防裁切的通用处理。
6. Hide on hover/focus departure and activation. Closing a modal or changing
   route must dismiss its tooltip; lease expiry handles detached publishers.
   An old anchor must not clear a newer anchor's claim.

中文执行要点：先确认 factory 根及 clip；不同动作使用不同固定叶子；复用按钮外观
但关闭通用 Tooltip；跨 delegate 只传动作身份与锚点几何；文案和测量在根层每动作
独立常驻；使用共享浮层定位；离开、点击、关闭和卸载均检查失效与续租。
每个固定动作 Portal 只保留一个 `GitHubTransientFloatingOverlayHost`；固定文案 Surface
分别常驻并按动作切换可见性，无提示时把 `active` 设为 false。Feature 禁止直接使用底层
全窗口几何宿主，也不得给 transient host 设置 opacity 或 visible。Settings 与 Messages
均遵守该结构，其他 factory Portal 仍须单独完成测量和验收。

## Prohibited fixes / 禁止的修法

- Do not fix swapped labels by changing translations or appending spaces.
- Do not use fixed pixel widths to hide stale text measurement.
- Do not mount Tooltip surfaces inside scrolling/clipped rows or dialogs.
- Do not change vendored Material sources.
- Do not recreate hover hit targets conditionally.
- Do not instantiate the private full-window geometry host from a feature, create
  one transient host per action, or override a transient host's opacity/visibility.
- Do not infer correctness from accessible labels alone: accessibility, painted
  text, measured width and action callback must agree.
- Do not report resolution based only on `cargo check` or one stationary hover.

禁止靠改翻译、补空格、写死宽度、堆 z 值掩盖串文案；禁止只看 AX 文案或编译通过。

## Acceptance matrix / 交付验收

For the affected root/factory, verify all applicable rows. State any unexecuted
checks at handoff; do not label them passed.

| Check / 检查 | Expected / 期望 |
| --- | --- |
| 相邻动作 A → B → A，至少三轮，跨列表行 | 文案和按钮动作一致，不残留上一动作 |
| 短文案 → 长文案 → 短文案；中文、英文 | 测量与绘制一致，无截字或不必要留白 |
| 无边界碰撞时悬浮图标按钮 | 提示框与按钮中心对齐，框内文字居中 |
| 窄 → 宽 → 窄，窗口四边/底边附近 | 全文在视口内，合理避让，不受滚动区裁切 |
| 滚动、切 Tab、离开路由再进入 | 锚点随实际位置更新，无悬空残留 |
| 打开/关闭/再次打开 Dialog，测试后返回列表 | 查看流程、停止、编辑、删除文案不相互污染 |
| Tab 聚焦、Enter/Space 点击、禁用/忙碌状态 | 状态与交互一致，点击后的提示正确撤销 |
| 多语言重新切换后重复 hover | 不保留旧语言或旧宽度 |
| 无提示、离开路由、重新进入并重复 20 次 | Portal 的 `active` 归零，内部全窗口定位树不可见；无逐轮持续增长 |

Add focused regressions for action identity, source ownership and bounds where
possible. Compile the full root UI, then inspect the **real compiled factory**
on the native app, using isolated test configuration. Testing backend assertions
cannot by themselves prove native ComponentFactory text painting is correct.
Run the repository checks from `AGENTS.md`, including `scripts/check-ui-policy.sh`.
That gate must reject feature-owned raw geometry hosts, per-host opacity, and
direct opacity/visibility overrides on the transient facade while accepting
opacity on a nested content-sized Surface. It must also pin the facade to exactly
one private geometry instance whose visibility is `root.active`, and constrain
the generic dynamic-tooltip exception to exactly two mutually exclusive claim
slots. Keep positive and negative fixtures in
`scripts/tests/test_quality_checks.py`, and record recurring defects in the bug
ledger with their regression coverage.

关键区别：单元测试负责身份、状态和几何，真实 native factory 截图负责最终绘制。
两者都要检查；验收时记录实际执行范围，未验证的平台或场景明确列出。
门禁还必须锁定 facade 内部只有一个私有几何实例且 `visible: root.active`，通用动态
Tooltip 的例外只能是两个按 claim 奇偶互斥的条件槽。
