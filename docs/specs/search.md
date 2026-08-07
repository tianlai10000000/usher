# Shared Search Navigation / 通用搜索导航

## Scope / 范围

`crates/usher-search/` owns framework-neutral text matching, adjacent-result
navigation, and viewport reveal policy for every application feature that
offers Find/Find next/Find previous. Feature modules remain responsible for
mapping their rows, panes, virtualized models, and visual-wrap geometry into
these primitives.

`crates/usher-search/` 统一负责所有页面 Find/下一个/上一个功能中的文本匹配、
相邻结果导航与视口揭示策略；Feature 模块只负责把自身的行、左右面板、虚拟 model
与视觉换行几何映射到这些通用原语。

## Invariants / 不变量

1. Matches are returned in source order and do not overlap. Each occurrence on
   the same logical line is an independent result.
2. Case-insensitive matching must preserve original UTF-8 byte and character
   boundaries even when Unicode lowercase expansion changes byte length.
3. Next/previous navigation wraps at the result-list boundaries. Empty result
   sets have no active index.
4. Match reveal follows Chromium/Blink `CenterIfNeeded`: if the active match
   rectangle is completely visible, preserve the viewport exactly; if any part
   is outside, center that rectangle and clamp to content bounds.
5. A feature with wrapped content must resolve the active occurrence to its
   visual subline before calling the shared viewport calculation. Moving among
   multiple visible matches on one visual line must not move the viewport.
6. Virtualized or variably sized features calculate reveal from the geometry
   available in the explicit navigation command. PR Diff uses the exact item
   start supplied by `usher-virtual-list`, adds only the target's intra-item
   visual offset, and applies `CenterIfNeeded`. Features must not scan preceding
   rows on every navigation or attach delegate geometry callbacks, viewport
   observers, or frame timers that continue Find positioning during scrolling.
7. `usher-search` may expose pure measured-convergence primitives for features
   that can complete measurement inside one editor transaction. A UI adapter
   must not turn those primitives into a persistent event-loop reconciliation
   process.
8. Virtualized diff content paints inactive matches with Blink yellow `#ffff00`
   and the active match with orange `#ff9632`. Row-local match models and paint
   repeaters exist only while Find is open, contain only hits for that mounted
   row/pane, and never participate in viewport positioning. A row projection
   must derive from the query passed to that invocation; it must not read a
   separately refreshed full-result cache whose update order can lag the input.
9. Reveal is command-scoped, following CodeMirror and VS Code: only Enter,
   Shift+Enter, or the Find previous/next controls perform one calculation and
   at most one viewport write. Query changes atomically replace the result set
   and immediately select/highlight its first result, but never position the
   viewport. Opening Find, focus changes, resize, virtual-list measurement,
   wheel/touch scrolling, and scrollbar-thumb dragging likewise never position
   the viewport. These events preserve the current query/result presentation;
   closing Find clears it.
10. The normal scroll path must contain no Find positioning timer or recycled-row
    geometry report. A virtual-list adapter may observe offset solely to update
    its bounded overscan render window; that path must not read query, active
    result, or match geometry. Paint-only match projection remains bounded to
    mounted rows and is absent when Find is closed.
10.1. Syntax-styled unwrapped content and soft-wrapped content resolve the
    active match through a
    feature-owned, replaceable shaped-text geometry facade using the same font
    collection, family, size, weight, italic state, scale factor, and rendered
    run width as visible text. The PR adapter uses bounded Parley cursor and
    selection geometry without WebView/editor embedding. A wrapped range returns
    one rectangle per intersected visual line, so CJK, punctuation, whitespace,
    and line breaks cannot be expanded through display-column estimates. Geometry
    is conditional on the active mounted row or a real run intersection; adapters
    must not create token-by-match cross-product repeaters or per-character UI models.
11. Feature code must not copy these algorithms. It may add a thin adapter for
   domain ordering, excluded rows, split panes, or scroll-coordinate signs.

1. 命中按源码顺序返回且互不重叠；同一逻辑行中的每次出现都是独立结果。
2. 不区分大小写搜索必须保留原始 UTF-8 字节与字符边界，即使 Unicode 小写展开改变
   字节长度也不得用折叠后的 offset 切原字符串。
3. 上一个/下一个在结果首尾循环；空结果没有活动索引。
4. 命中揭示严格采用 Chromium/Blink `CenterIfNeeded`：活动命中矩形完整可见时，
   viewport 完全不动；任一部分不可见时，将该矩形居中并按内容边界夹紧。
5. 存在软换行时，Feature 必须先把活动命中解析到视觉子行，再调用通用视口计算。
   同一可见视觉行内切换多个命中不得移动 viewport。
6. 虚拟化或可变行高 Feature 只使用显式导航命令执行时可取得的几何计算揭示。PR Diff
   使用 `usher-virtual-list` 提供的精确 item start，仅叠加目标在本行内的视觉偏移，再
   应用 `CenterIfNeeded`；禁止每次导航扫描前序行，也禁止挂接 delegate 几何 callback、
   viewport observer 或跨帧 timer，使 Find 定位在后续滚动中继续执行。
7. `usher-search` 可以为能在单个编辑器事务内完成测量的 Feature 提供纯测量收敛原语，
   UI adapter 禁止把这些原语变成长驻 event-loop 收敛过程。
8. 虚拟化 Diff 使用 Blink 黄色 `#ffff00` 绘制非活动命中，使用橙色 `#ff9632`
   绘制当前命中。行级命中 model 与绘制 repeater 仅在 Find 打开时存在，只包含当前已挂载
   行/侧的命中，并且绝不参与 viewport 定位。行级投影必须直接使用本次调用传入的 query，
   禁止读取另行刷新的全量结果缓存，以免输入与缓存写入顺序不同步。
9. 揭示按命令隔离，与 CodeMirror、VS Code 一致：只有 Enter、Shift+Enter 或 Find
   上一个/下一个控件执行一次计算并至多写一次 viewport。修改查询必须原子替换结果集并
   立即选中/高亮新结果的第 1 项，但不得定位 viewport。打开 Find、焦点变化、窗口 resize、
   虚拟列表测量、滚轮/触控滚动和拖拽滚动条滑块同样不得定位 viewport。这些事件保留当前
   查询/结果展示；只有关闭 Find 才清空。
10. 普通滚动路径中禁止存在 Find 定位 timer 或回收行几何上报。虚拟列表 adapter 可以
   观察 offset，但只能更新有界 overscan 渲染窗口；该路径不得读取查询、当前结果或
   命中几何。仅绘制用途的命中投影必须限制在已挂载行，并在 Find 关闭时不存在。
10.1. 对带语法样式且不换行或软换行的内容，当前命中必须通过 Feature 自有、可替换的 shaped-text
   几何门面计算，并与可见文字共用字体集合、字体族、字号、字重、斜体、scale factor 和
   实际 run 宽度。PR adapter 使用有界 Parley cursor 与 selection 几何，禁止嵌入 WebView/
   浏览器编辑器。换行范围按实际相交视觉行返回多个矩形，CJK、标点、空白和换行边界不得
   回退到显示列估算。只有当前已挂载行或确实相交的 run 才计算几何；禁止 token×match
   交叉 repeater 和逐字符 UI model。
11. Feature 禁止复制这些算法；只允许为领域顺序、排除行、Split 面板或滚动坐标正负
   增加薄适配层。

The reveal policy follows Blink's current Find-in-page implementation, which
passes both axes through `ScrollAlignment::CenterIfNeeded()`:
<https://chromium.googlesource.com/chromium/src/+/refs/heads/main/third_party/blink/renderer/core/editing/finder/text_finder.cc#144>

The command boundary follows mature editor implementations rather than a
continuous scroll observer: CodeMirror adds `scrollToMatch` only to its
`findNext`/`findPrevious` command transactions, and VS Code reveals from
`_setCurrentFindMatch`, which is called by explicit match navigation:
<https://github.com/codemirror/search/blob/main/src/search.ts#L398-L428>
<https://github.com/microsoft/vscode/blob/main/src/vs/editor/contrib/find/browser/findModel.ts#L236-L246>

The shared reveal policy follows TanStack Virtual's index/offset contract. The
PR adapter obtains exact prefix-sum geometry from `usher-virtual-list`, whose
private default adapter uses the community Rust `virtualizer` crate. It resolves
only the wrapped offset inside the target row, so navigation is logarithmic and
does not materialize preceding lazy rows. Pure convergence remains available
for adapters with a cancellable measurement transaction; PR Diff does not need
that lifecycle because its row extents are deterministic at the stable width:
<https://tanstack.com/virtual/latest/docs/api/virtualizer>
<https://github.com/TanStack/virtual/blob/main/packages/virtual-core/src/index.ts>
<https://docs.rs/virtualizer/0.4.0/virtualizer/>
<https://codemirror.net/docs/ref/#view.EditorView.requestMeasure>

## Regression Checklist / 回归清单

- Cover case-sensitive and insensitive matching, CJK text, Unicode lowercase
  expansion, repeated occurrences on one line, empty results, and wraparound.
- Verify a fully visible match does not change scroll offset.
- Edit a non-empty query without pressing Enter and verify the count, active
  index, and active/inactive highlight bounds switch atomically to the new
  result set while the viewport remains unchanged.
- Verify partially visible, fully hidden, first, and last matches center or
  clamp correctly.
- In virtualized/wrapped feature UIs, verify the command-time adapter resolves
  the active visual subline. Per-row paint models and repeaters may exist only
  for the bounded mounted window and must not read or write viewport state.
- Exercise Find closed/open, fixed width, width changed before/during Find,
  height changed before/during Find, Unified/Split, wrapped/unwrapped content,
  and first/middle/final results. Resize alone must never move the viewport.
- Verify wheel/touch scrolling and direct scrollbar-thumb dragging remain under
  user control after navigating to a match, and a stale virtual content bound
  cannot hide the final match. Verify navigation addresses the target model
  index after width-dependent wrapping, especially for the final results. Once
  a direct-thumb gesture is clamped at either track edge, repeated pointer
  events at that same mapped position must not restart virtual-list correction.
  Exact content extent and pointer-to-offset mapping must stay stable for the
  complete gesture. Dragging and release render the same wrapped rows; scrolling
  updates only a bounded overscan model, never the complete row model.
- Move focus directly from the Find input to the scrollbar or code surface,
  then start a wheel/touch or thumb-drag scroll. Verify there is no later Find
  viewport write. The active highlight, count, and active result remain unchanged.
- After manual scrolling, verify virtual-list measurement and width/height
  reflow do not restart reveal; only Enter/Shift+Enter or the Find navigation
  controls may perform another one-shot viewport calculation.
- With Find open and closed, confirm ordinary scrolling contains no Find timer
  or recycled-row geometry report. The virtual-window callback may update only
  bounded overscan rows and must perform no Find work.

- 覆盖大小写敏感/不敏感、CJK、Unicode 小写展开、同一行重复命中、空结果与首尾循环。
- 验证完整可见命中不改变滚动 offset。
- 不按 Enter 直接修改非空查询，验证计数、当前索引及活动/非活动高亮边界原子切换到
  新结果集，同时 viewport 保持不变。
- 验证部分可见、完全不可见、首个和末尾命中正确居中或贴边夹紧。
- 在虚拟化/换行页面验证命令执行时的 adapter 使用活动视觉子行；回收行不得拥有逐行
  匹配定位状态；绘制用 model/repeater 只能存在于有界的已挂载窗口，且不得读写 viewport。
- 覆盖 Find 关闭/打开、固定宽度、Find 前/过程中改变宽度、Find 前/过程中改变高度、
  Unified/Split、换行/不换行、首个/中间/最后命中；仅 resize 不得移动 viewport。
- 导航到命中后分别验证滚轮/触控滚动和直接拖拽滚动条滑块始终由用户控制，并验证
  滞后的虚拟内容边界不会隐藏最后一个命中；宽度相关换行发生后仍须按目标 model index
  寻址，尤其覆盖最后几个结果。滑块在轨道任一边缘夹紧后，同一映射位置的重复指针事件
  不得重新启动虚拟列表校正。一次手势中的精确 content extent 与指针到 offset 映射必须
  保持稳定；拖拽与释放渲染相同的换行行，滚动只更新有界 overscan model，不重建完整行 model。
- 从 Find 输入框直接点击滚动条或代码区，再开始滚轮/触控或滑块拖动；验证后续不存在
  Find viewport 写入，同时保留当前高亮、计数与当前结果。
- 手动滚动后，验证虚拟列表测量与宽高重排都不会重新启动揭示；只有
  Enter/Shift+Enter 或 Find 导航控件可以再次执行一次性 viewport 计算。
- 在 Find 打开和关闭时检查普通代码滚动，确认滚动路径中不存在 Find timer 或回收行
  几何上报；虚拟窗口 callback 只能更新有界 overscan 行，不得执行 Find 工作。
