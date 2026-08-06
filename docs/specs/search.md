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
   available in the explicit navigation command. They must not attach delegate
   geometry callbacks, viewport observers, or frame timers that continue Find
   positioning during later scrolling. Approximation error from wrapped virtual
   rows is tracked separately and must never be fixed by degrading scrolling.
7. `usher-search` may expose pure measured-convergence primitives for features
   that can complete measurement inside one editor transaction. A UI adapter
   must not turn those primitives into a persistent event-loop reconciliation
   process.
8. Virtualized diff content paints inactive matches with Blink yellow `#ffff00`
   and the active match with orange `#ff9632`. Row-local match models and paint
   repeaters exist only while Find is open, contain only hits for that mounted
   row/pane, and never participate in viewport positioning.
9. Reveal is command-scoped, following CodeMirror and VS Code: only Enter,
   Shift+Enter, or the Find previous/next controls perform one calculation and
   at most one viewport write. Query changes, opening Find, focus changes,
   resize, virtual-list measurement, wheel/touch scrolling, and scrollbar-thumb
   dragging never position the viewport. These events do not clear the query,
   result set, active index, or active highlight; closing Find does.
10. The normal scroll path must contain no Find positioning timer, viewport
    observer, scroll callback, or recycled-row geometry report. Paint-only match
    projection must remain bounded to mounted rows and must be absent when Find
    is closed.
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
6. 虚拟化或可变行高 Feature 只使用显式导航命令执行时可取得的几何计算揭示；禁止挂接
   delegate 几何 callback、viewport observer 或跨帧 timer，使 Find 定位在后续滚动中
   继续执行。软换行虚拟行的近似误差单独记录，绝不能以破坏滚动性能为代价修正。
7. `usher-search` 可以为能在单个编辑器事务内完成测量的 Feature 提供纯测量收敛原语，
   UI adapter 禁止把这些原语变成长驻 event-loop 收敛过程。
8. 虚拟化 Diff 使用 Blink 黄色 `#ffff00` 绘制非活动命中，使用橙色 `#ff9632`
   绘制当前命中。行级命中 model 与绘制 repeater 仅在 Find 打开时存在，只包含当前已挂载
   行/侧的命中，并且绝不参与 viewport 定位。
9. 揭示按命令隔离，与 CodeMirror、VS Code 一致：只有 Enter、Shift+Enter 或 Find
   上一个/下一个控件执行一次计算并至多写一次 viewport。修改查询、打开 Find、焦点
   变化、窗口 resize、虚拟列表测量、滚轮/触控滚动和拖拽滚动条滑块均不得定位 viewport。
   这些事件不清除查询、结果集、当前索引或当前高亮；只有关闭 Find 才清空。
10. 普通滚动路径中禁止存在 Find 定位 timer、viewport observer、scroll callback 或回收
   行几何上报。仅绘制用途的命中投影必须限制在已挂载行，并在 Find 关闭时完全不存在。
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

The library's optional pure convergence calculation was informed by TanStack
Virtual and CodeMirror's separated measure/update loop. The PR adapter does not
run their frame-reconciliation lifecycle because Slint's virtual ListView and
Material scrollbar do not expose the same cancellable transaction boundary:
<https://tanstack.com/virtual/latest/docs/api/virtualizer>
<https://github.com/TanStack/virtual/blob/main/packages/virtual-core/src/index.ts>
<https://codemirror.net/docs/ref/#view.EditorView.requestMeasure>

## Regression Checklist / 回归清单

- Cover case-sensitive and insensitive matching, CJK text, Unicode lowercase
  expansion, repeated occurrences on one line, empty results, and wraparound.
- Verify a fully visible match does not change scroll offset.
- Verify partially visible, fully hidden, first, and last matches center or
  clamp correctly.
- In virtualized/wrapped feature UIs, verify the command-time adapter resolves
  the active visual subline. Recycled rows must have no per-row match model or
  match repeater.
- Exercise Find closed/open, fixed width, width changed before/during Find,
  height changed before/during Find, Unified/Split, wrapped/unwrapped content,
  and first/middle/final results. Resize alone must never move the viewport.
- Verify wheel/touch scrolling and direct scrollbar-thumb dragging remain under
  user control after navigating to a match, and a stale virtual content bound
  cannot hide the final match. Exact reveal placement still has the known
  deferred limitation recorded as UI-079.
- Move focus directly from the Find input to the scrollbar or code surface,
  then start a wheel/touch or thumb-drag scroll. Verify there is no later Find
  viewport write. The active highlight, count, and active result remain unchanged.
- After manual scrolling, verify virtual-list measurement and width/height
  reflow do not restart reveal; only Enter/Shift+Enter or the Find navigation
  controls may perform another one-shot viewport calculation.
- With Find open and closed, profile ordinary code scrolling to confirm there
  is no Find timer, viewport observer, scroll callback, recycled-row geometry
  report, row-local match model, or match repeater in the scrolling path.

- 覆盖大小写敏感/不敏感、CJK、Unicode 小写展开、同一行重复命中、空结果与首尾循环。
- 验证完整可见命中不改变滚动 offset。
- 验证部分可见、完全不可见、首个和末尾命中正确居中或贴边夹紧。
- 在虚拟化/换行页面验证命令执行时的 adapter 使用活动视觉子行；回收行不得拥有逐行
  匹配 model 或命中 repeater。
- 覆盖 Find 关闭/打开、固定宽度、Find 前/过程中改变宽度、Find 前/过程中改变高度、
  Unified/Split、换行/不换行、首个/中间/最后命中；仅 resize 不得移动 viewport。
- 导航到命中后分别验证滚轮/触控滚动和直接拖拽滚动条滑块始终由用户控制，并验证
  滞后的虚拟内容边界不会隐藏最后一个命中。精确定位仍存在 UI-079 记录的延期限制。
- 从 Find 输入框直接点击滚动条或代码区，再开始滚轮/触控或滑块拖动；验证后续不存在
  Find viewport 写入，同时保留当前高亮、计数与当前结果。
- 手动滚动后，验证虚拟列表测量与宽高重排都不会重新启动揭示；只有
  Enter/Shift+Enter 或 Find 导航控件可以再次执行一次性 viewport 计算。
- 在 Find 打开和关闭时检查普通代码滚动，确认滚动路径中不存在 Find timer、viewport
  observer、scroll callback、回收行几何上报、行级匹配 model 或命中 repeater。
