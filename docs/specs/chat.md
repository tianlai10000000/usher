# Chat Behavior Specification / Chat 行为规格

## Scope / 范围

This is the stable contract for the shared AI/RAG Chat presentation, isolated persisted histories, history rendering, and inline request failures.

本文约束 AI/RAG 共用 Chat 展示层、隔离历史持久化、历史渲染和请求失败的内联反馈。

## Ownership / 所有权

- Persistence and session state: `src/chat/persistence.rs`, `src/chat/session.rs`.
- Width-independent UI-row projection and bounded projection cache:
  `src/chat/renderer.rs`, `src/chat/markdown.rs`.
- Async request and route interaction: `src/chat/interactive.rs`.
- Production bounded height estimation, prefix cache, and virtual-window selection:
  `src/chat/virtual_layout.rs`.
- Presentation: `ui/views/chat.slint`, bridged through `ui/main.slint`.

- 持久化和双 Session 状态：`src/chat/persistence.rs`、`src/chat/session.rs`。
- 宽度无关 UI 行投影及有界投影缓存：`src/chat/renderer.rs`、`src/chat/markdown.rs`。
- 异步请求与路由交互：`src/chat/interactive.rs`。
- 生产使用的有界高度估算、prefix cache 与虚拟窗口选择：`src/chat/virtual_layout.rs`。
- 展示：`ui/views/chat.slint`，通过 `ui/main.slint` 桥接。

## Invariants / 不变量

1. Chat history remains JSONL and old entries without a `status` field remain readable as normal messages.
2. Application-generated error/success state is stored separately from message text and rendered with vector icons. Do not add fixed Emoji prefixes that force a color-font fallback.
3. Legacy assistant entries beginning with `❌` or `✅` are recognized at render time and shown without the prefix; the history file is not destructively rewritten.
4. Request failures remain inline assistant messages and do not duplicate centralized HTTP feedback.
5. Production projects the currently visible bounded history window into one
   stable Slint row model. Window shifts mutate only non-overlapping prefix and
   suffix rows; scrolling and resize must not replace the model. History loads
   a 20-message startup tail and explicit or automatic 10-message pages.
6. Chat's width-independent projection cache remains bounded by both entry count
   and approximate bytes and is cleared when leaving Chat. Full Markdown block
   parsing and formatted presentation belong to the artifact-preview framework.
7. History I/O and HTTP requests never block the Slint event loop; UI updates are scheduled back onto that loop.
8. The Chat component and its Slint message model are route-scoped. Leaving Chat destroys the component and clears its UI model/cache; the Rust session remains the source of truth and reconstructs the currently loaded model on re-entry.
   When that session contains history, re-entry shows a centered history-loading
   state until the loaded model is committed. The welcome view is visible only
   after restoration completes and the session is confirmed empty.
9. A history-page or assistant response that completes after leaving Chat updates the Rust session/persistence only and must not remount or repopulate the hidden Chat UI.
10. Chat presentation is application-owned: native layouts plus
    `GitHubScrollView`, `GitHubSpinner`, and `GitHubIconButton`; Material
    visuals are not part of the product contract.
11. Message surfaces use compact semantic borders and six-pixel corners.
    Suggestions and send/load actions support keyboard activation and
    keyboard-visible focus.
12. The composer adapts to available width, retains resize behavior, and keeps
    its model mounted while sending. Loading does not duplicate or reorder the
    currently loaded message model.
13. The history pagination action is mounted in the message flow immediately
    after the virtual top spacer. It reserves its own height and never overlays
    the oldest mounted message.
14. New user-facing request errors use concise English recovery text. Provider,
    HTTP status, and decode details remain in logs and are not exposed verbatim
    in message bubbles.
15. An assistant row retains a readable width-independent plain-text projection.
    To bound Slint text layout, a long inline projection is capped at 1,200
    Unicode scalar values and exposes an action that opens the complete response
    in the shared artifact preview. Width-only resize must not replace
    `chat-messages` or rerun the projection.
16. Production enables estimated-height virtual windows by default, with at
    most 32 mounted rows and real top/bottom spacers. The explicit benchmark
    baseline disables virtualization for controlled comparison; resize may
    rebuild the width-keyed prefix but must never replace the production model.
17. A message delegate instantiates only its active User or Assistant subtree.
    Hidden role trees must not remain mounted in the optimized product path.
18. During macOS native live resize, Chat keeps the complete message ScrollView
    at its last stable width and clips it through the current route host. The
    route surface and composer continue following the window, and releasing the
    edge applies one final message layout at the new width.
19. `USHER_CHAT_PERF_VARIANT=baseline|optimized` is a benchmark-only A/B seam.
    Baseline preserves the previous 56-row, dual-role-tree, non-virtualized
    presentation; normal launches use optimized single-role delegates and
    production virtualization.
20. Production virtual-window movement may occur only when an actual viewport-position
    change crosses the safe overscan threshold. No delayed idle task may replace
    the message model or reposition the viewport after scrolling stops. Normal
    optimized shifts never write an anchor correction to the viewport; Flickable
    remains the sole wheel/touchpad position owner. Same-event-loop requests are
    coalesced and commit from the latest viewport position. Ordinary wheel/touchpad
    overlapping shifts introduce at most two new rows unless the actual mounted-edge
    guard requires one uncapped convergence to keep the viewport out of a spacer.
    Direct scrollbar drags are random access and must commit the exact visible
    destination window without that cap or the ordinary 2,400 px overscan. Exact
    stable-ID correction is reserved for history prepend and settled resize.
21. A plain wrapped `Text` is the sole layout and paint owner for each message
    body. Selection uses a Chat-owned pointer and shaped-geometry overlay that
    never replaces that `Text`, participates in preferred size, or mounts a
    `TextInput`. Only the active message produces coalesced visual-line
    rectangles, and the platform copy shortcut copies its Unicode-safe range.
    Chat does not mount a per-message copy action.
22. Pointer drags inside the message viewport belong to text selection, not
    Flickable drag-scrolling. Wheel/touchpad scrolling and the explicit
    scrollbar remain available.
23. Reaching the top automatically starts one deduplicated history-page load.
    Before prepended rows are committed, Chat captures the first fully visible
    message's stable ID and actual content position; after layout, it applies
    the exact position delta so that message stays at the same viewport pixel.
    The explicit in-flow pagination action remains as a retry/accessibility path.
24. New persisted messages store a complete RFC 3339 instant with the local
    offset. Message rows derive a compact local-time label from calendar dates:
    `HH:mm` today, `昨天 HH:mm` yesterday, `前天 HH:mm` two calendar days ago,
    `星期X HH:mm` for three through six days ago, `M月D日 HH:mm` earlier in the current year, and
    `YYYY年M月D日 HH:mm` across years. Legacy `HH:mm` values remain unchanged
    because their original date cannot be recovered.
25. Production optimized virtual extents are computed before scrolling from the same bounded
    1,200-character projection and Parley shaping model used by message text,
    including bubble padding, preview/status/timestamp rows, role minimums,
    spacing, and Slint line-box rounding. The resulting width-keyed prefix and
    document extent remain stable for that layout width. A settled width change
    rebuilds the prefix once; a height-only change updates viewport coverage.
    The mounted-content edge guard replaces the window before the viewport can
    enter a spacer.
26. The production Chat route owns one stable `VecModel` containing only the
    bounded mounted window. Virtual-window shifts preserve overlapping stable message IDs and mutate only
    non-overlapping prefix/suffix rows; replacing the complete `ModelRc` is limited
    to the benchmark baseline. The interaction A/B fails when any optimized run
    exceeds a 50 ms commit-adjacent frame gap, exposes more than 1 px of stable
    virtual blank, commits within 800 ms after motion stops, mounts over 32 rows,
    or records more than 1 px of wheel/scrollbar reverse movement.
27. Programmatic bottom navigation pins the exact Flickable bottom for at most
    800 ms while initial message measurements settle. Wheel/touchpad or scrollbar
    input cancels the pin immediately. A resize that begins at the bottom remains
    at the bottom; a resize elsewhere preserves its visible stable-ID anchor. The
    interaction A/B fails if startup size settlement exposes more than 1 px of
    bottom distance after the initial bottom positioning.
28. A production direct scrollbar drag freezes the scroll maximum, thumb height/range, and
    pointer grab offset at pointer-down. The frozen thumb owns its display
    position until pointer-up, and ordinary anchor writes are forbidden during
    the drag. Every coalesced drag commit converges directly on the latest random
    access destination, and pointer-up performs one final uncapped convergence at
    the exact release position while retaining the visible-only drag window until
    the next ordinary scroll movement. Flickable's `flicked()` callback observes
    wheel/touchpad direction across the complete viewport without adding a second
    position-writing chain.
29. One Primer segmented control switches the shared page between AI Chat and
    RAG. AI uses the OpenAI-compatible provider configured under `[ai.agent]`;
    RAG continues to use the repository-knowledge endpoint in `src/chat/chat.rs`.
30. AI and RAG persist to separate JSONL files: `im/ai_chat_history.jsonl` and
    the legacy `im/chat_history.jsonl`. Pagination, next IDs, loaded tails, and
    sending state are isolated by mode.
31. AI requests include at most the latest 20 loaded AI messages plus one
    feature-owned system instruction. Switching mode while a response is in
    flight must not cancel it, append it to the other history, clear the other
    mode's sending state, or project it into the other mode.
32. A late response may update its original Rust session and persistence while
    hidden. It updates the Slint model only when Chat is visible and that same
    mode is active.
33. Every Chat model projection is bound to an explicit mode. Delayed route or
    mode mounts carry a monotonically increasing revision and may commit only
    when both their mode and revision still match the visible Chat state.
34. Incremental message-row identity includes both mode and persisted message
    ID. Switching to an empty mode still commits an empty render signature and
    removes every row mounted by the previous mode.
35. Every submitted AI or RAG turn starts one real elapsed timer. The pending
    assistant row updates once per second, and the terminal assistant or error
    row persists the measured `elapsed_ms` and renders a compact frozen label.
    Legacy JSONL rows without `elapsed_ms` remain readable and show no duration.
36. RAG requests resolve their endpoint from `[ai.chat].rag_api_url`, then
    `USHER_RAG_API_URL`. The endpoint must be HTTPS; no deployment address is
    compiled into the application, and logs record only message length, never content.
37. Sending prepares the next message while holding the session lock, appends
    JSONL without that lock, then commits the in-memory message. A persistence
    failure must not advance the message ID or send the network request; the UI
    restores the user's draft and exposes a retryable error.
38. Older-page loading prepares an immutable store request while holding the
    session lock, reads JSONL after releasing that lock, then reacquires it only
    to validate the oldest message ID and commit the page. A stale page cannot
    replace a newer prepend.
39. AI and RAG keep separate composer drafts. Switching mode preserves the
    current text and restores the destination mode's text. If user-message
    persistence fails after a mode switch, the original text is restored to the
    originating mode and the failure remains visible.
40. The AI/RAG message scrollbar is bounded by the message viewport. The Chat
    page already starts below the native title bar, so its scrollbar must not
    apply the window title-bar inset a second time; at document offset zero the
    thumb starts at the top of the message viewport track.

1. Chat 历史继续使用 JSONL；缺少 `status` 字段的旧记录按普通消息兼容读取。
2. 应用生成的错误/成功状态与正文分开保存，并使用矢量图标展示；禁止重新加入会触发彩色字体回退的固定 Emoji 前缀。
3. 以 `❌` 或 `✅` 开头的旧 assistant 记录在渲染时识别并去除前缀，不破坏性改写历史文件。
4. 请求失败继续使用内联 assistant 消息，不与统一 HTTP 反馈重复。
5. 生产路径把当前可见的有界历史窗口投影到一个稳定 Slint 行 model；窗口移动只增删
   非重叠的头尾行，滚动和 resize 不得替换该 model。启动加载最近 20 条，显式或自动
   分页每次增加 10 条。
6. Chat 的宽度无关文本投影缓存同时受条目数和估算字节数限制，离开 Chat 时清理；
   完整 Markdown block 解析和格式化展示归文件预览框架所有。
7. 历史 I/O 和 HTTP 请求不得阻塞 Slint event loop；UI 更新必须调度回该线程。
8. Chat 组件及其 Slint 消息 model 按路由管理；离开 Chat 时销毁组件并清空 UI model/cache，Rust session 继续作为事实来源，重新进入时重建当前已加载的 model。
   会话中存在历史时，重新进入必须显示居中的历史加载状态，直到已加载 model 提交完成；
   只有恢复完成且确认会话为空后才能显示欢迎引导。
9. 离开 Chat 后才完成的历史分页或 assistant 响应只更新 Rust session/持久化，不得重新挂载或填充隐藏的 Chat UI。
10. Chat 展示层由应用自有 native layout、`GitHubScrollView`、
    `GitHubSpinner` 和 `GitHubIconButton` 组成，Material 视觉不属于产品契约。
11. 消息表面使用紧凑语义边框和 6px 圆角；建议、发送和历史加载操作必须支持
    键盘激活与仅键盘可见的焦点状态。
12. 编辑器必须随可用宽度自适应，保留拖拽调高能力，发送期间保持 model 常驻；
    loading 不得复制或重排当前已加载的消息 model。
13. 历史分页操作必须挂在 virtual top spacer 之后的消息流内并占据自身高度，
    不得覆盖最早一条已挂载消息。
14. 新产生的用户可见请求错误统一使用简洁英文恢复文案；Provider、HTTP 状态和
    解码技术详情只写日志，不得原样暴露在消息气泡中。
15. Assistant 行保留宽度无关的可读纯文本投影。为限制 Slint 文本布局开销，长内联投影
    最多保留 1,200 个 Unicode scalar value，并提供在通用文件预览中打开完整回复的入口；
    仅宽度变化不得替换 `chat-messages` 或重新执行投影。
16. 生产路径默认启用估算行高虚拟窗口，最多挂载 32 行，并使用真实上下 spacer。
    显式 benchmark baseline 关闭虚拟化用于受控比较；resize 可以重建 width-keyed prefix，
    但不得替换生产 model。
17. 消息 delegate 只实例化当前 User 或 Assistant 角色子树；optimized 产品路径不得
    保留隐藏的另一套角色树。
18. macOS 原生 live resize 期间，Chat 将完整消息 ScrollView 保持在上一个稳定宽度，
    并由当前路由 host 裁剪；路由底面与编辑器继续跟随窗口，松开边缘后只按最终宽度
    执行一次消息布局。
19. `USHER_CHAT_PERF_VARIANT=baseline|optimized` 仅作为性能基准 A/B 接缝；baseline
    保留此前 56 行、双角色子树与非虚拟化展示；普通启动使用 optimized 单角色 delegate
    与生产虚拟化。
20. 只有生产路径中的实际 viewport 位置变化越过安全 overscan 阈值时才能移动虚拟窗口；滚动停止后
    不得有延迟任务替换消息 model 或重新定位 viewport。普通 optimized 窗口移动禁止向
    viewport 写入锚点补偿，Flickable 是滚轮/触控板位置的唯一 owner。同一 event-loop
    周期内的请求必须合并并按最新 viewport 位置提交。普通滚轮/触控板的重叠移动每次最多
    新增两行；仅当真实 mounted-edge 保护需要避免 viewport 进入 spacer 时，允许一次无上限
    收敛。直接拖拽滚动条属于随机访问，必须不受该限制及普通 2,400 px overscan 影响，只提交
    精确覆盖目标 viewport 的窗口。只有历史 prepend 与稳定 resize 保留按稳定 ID 的精确补偿。
21. 每条消息正文只由常驻的自动换行普通 `Text` 负责布局和绘制。文本选择使用 Chat
    自有指针处理与字形几何覆盖层，不替换该 `Text`、不参与 preferred size，也不挂载
    `TextInput`。只有当前选择的消息生成按视觉行合并后的选区矩形，平台复制快捷键按
    Unicode 安全范围复制。Chat 不挂载逐消息复制按钮。
22. 消息 viewport 内的鼠标拖拽归文本选择所有，不得被 Flickable 接管为拖拽滚动；
    滚轮、触控板和显式滚动条继续可用。
23. 滚动到顶部会自动启动一次去重的历史分页加载。提交 prepend 行之前，Chat 记录首个
    完整可见消息的稳定 ID 与真实内容坐标；布局后只补偿该消息的精确坐标差，使它保持
    在同一 viewport 像素。消息流内的显式分页操作继续作为重试和无障碍入口。
24. 新持久化消息使用带本地时区偏移的完整 RFC 3339 时间。消息行按本地自然日生成紧凑
    标签：今天为 `HH:mm`，昨天为“昨天 `HH:mm`”，前天为“前天 `HH:mm`”，前 3 至 6
    个自然日为“星期X `HH:mm`”，同年更早为“`M月D日 HH:mm`”，跨年为
    “`YYYY年M月D日 HH:mm`”。旧版
    `HH:mm` 记录因无法恢复原始日期，继续原样显示。
25. 生产 optimized 虚拟 extent 在滚动前按同一份有界 1,200 字符投影和消息文本使用的 Parley
    shaping 计算，并计入气泡 padding、预览/状态/时间行、角色最小高度、消息间距与 Slint
    line-box rounding。得到的 width-keyed prefix 与 document extent 在该稳定宽度内不得变化。
    稳定宽度变化只重建一次 prefix；仅高度变化只更新 viewport 覆盖。mounted 内容边缘保护
    必须在 viewport 进入 spacer 前替换窗口。
26. 生产 Chat 路由在其生命周期内只拥有一个包含有界挂载窗口的稳定 `VecModel`。虚拟窗口移动时保留
    稳定消息 ID 重叠区，只增删不重叠的头尾行；整批替换 `ModelRc` 仅保留给 benchmark
    baseline。任一 optimized A/B 轮次出现提交相邻帧超过 50 ms、稳定虚拟空白超过 1 px、
    停止移动后 800 ms 内仍提交、挂载超过 32 行，或滚轮/滚动条逆向移动超过 1 px 时，
    基准必须失败。
27. 程序主动滚到底部后最多固定 Flickable 精确底部 800 ms，等待初始消息测量稳定；滚轮、
    触控板或滚动条输入必须立即取消固定。resize 从底部开始时必须继续贴底，其他位置则保持
    可见稳定 ID 锚点。首次自动贴底后，启动尺寸稳定期间离底距离超过 1 px 时交互 A/B 必须失败。
28. 生产路径中的直接滚动条拖拽在 pointer-down 时必须冻结 scroll maximum、thumb 高度/范围及指针抓取
    偏移；冻结的 thumb 在 pointer-up 前独占显示位置，拖拽期间禁止普通锚点写入。完整 viewport
    的每次合并提交都必须直接收敛到最新随机访问目标，pointer-up 还必须按精确松手位置执行
    一次最终无上限收敛，并将仅覆盖可见区的 drag 窗口保留到下一次普通滚动。滚轮/触控板
    方向由 Flickable `flicked()` 观测，不得增加第二条位置写入链。
29. 同一个页面使用 Primer segmented control 在 AI Chat 与 RAG 间切换。AI 使用
    `[ai.agent]` 配置的 OpenAI-compatible Provider；RAG 继续使用
    `src/chat/chat.rs` 中的仓库知识 endpoint。
30. AI 与 RAG 分别持久化到 `im/ai_chat_history.jsonl` 和旧有
    `im/chat_history.jsonl`；分页、next ID、加载尾部和发送状态按模式隔离。
31. AI 请求最多携带最近 20 条已加载 AI 消息和一条 Feature 自有 system 指令。
    响应进行中切换模式不得取消请求、写入另一历史、清除另一模式 sending 状态，或把
    响应投影到另一模式。
32. 晚到响应可以更新原模式 Rust Session 与持久化；只有 Chat 可见且原模式仍为活动模式时，
    才更新 Slint model。
33. 每次 Chat model 投影都必须绑定明确的模式。延迟执行的路由或模式挂载携带单调递增的
    revision，只有其模式和 revision 都仍与可见 Chat 状态一致时才能提交。
34. 增量消息行身份必须同时包含模式和持久化消息 ID。切换到空历史模式时仍必须提交空的
    render signature，并删除上一模式挂载的全部消息行。
35. 每次提交 AI 或 RAG 对话都会启动一个真实耗时计时器；等待中的 assistant 行每秒更新，
    完成或失败的 assistant 行持久化实测 `elapsed_ms` 并显示紧凑的冻结标签。缺少
    `elapsed_ms` 的旧 JSONL 记录继续兼容读取且不显示耗时。
36. RAG 请求先从 `[ai.chat].rag_api_url` 解析 endpoint，再回退到
    `USHER_RAG_API_URL`。Endpoint 必须使用 HTTPS；应用不得编译内置部署地址，日志只记录
    消息长度，禁止记录消息正文。
37. 发送消息时只在 Session 锁内准备下一条记录，释放锁后追加 JSONL，再回锁提交内存状态。
    持久化失败不得推进消息 ID 或发起网络请求；UI 必须恢复用户草稿并显示可重试错误。
38. 加载更早分页时，只在 Session 锁内准备不可变 Store 请求；释放锁后读取 JSONL，
    再回锁校验最早消息 ID 并提交。过期分页不得覆盖更新的 prepend 结果。
39. AI 与 RAG 分别保存 composer 草稿。切换模式时保存当前文字并恢复目标模式文字；
    若用户消息持久化失败时已经切换模式，原文字仍恢复到发起模式且失败提示保持可见。
40. AI/RAG 消息滚动条以消息 viewport 为边界。Chat 页面本身已经位于原生标题栏下方，
    因此滚动条不得再次应用窗口标题栏 inset；文档偏移为零时，thumb 必须位于消息 viewport
    轨道顶部。

## Regression checklist / 回归检查

- Load a pre-status JSONL history and verify it remains visible.
- Verify a legacy `❌ message` renders as an error icon plus selectable `message`
  text without the Emoji prefix.
- Trigger a new request error, restart, and verify its status and plain text persist.
- Open enough history to exceed the virtual window and verify scrolling loads the expected rows without linear UI-model growth.
- Verify the history action occupies a row above the oldest message at minimum
  width and while loading; it must not overlap message content.
- Navigate Chat -> Home -> Chat and verify the previous scroll state and messages remain correct.
- With persisted history, navigate Chat -> Home -> Chat and verify the centered
  loading state replaces the welcome view until messages appear. With an empty
  history, verify the welcome view appears without a restoration loading state.
- While a request or history page is loading, leave Chat and verify the response persists without recreating the Chat component/model; re-enter and verify the completed content appears.
- Switch AI Chat -> RAG while each mode has an in-flight request; verify responses,
  loading states, pagination, and persisted histories remain isolated.
- Type distinct unsent drafts in AI and RAG and switch repeatedly; verify both
  survive. Fail user-message persistence, switch mode before completion, then
  return and verify the originating draft is restored with visible feedback.
- Resize the window on a non-Chat route with existing Chat history and verify Chat delegates/text layout do not appear in the resize profile.
- Resize repeatedly on Chat and verify the model-commit and Markdown-projection
  counters remain unchanged throughout width-only interaction.
- Open a Markdown title and verify the formatted document uses the globally
  selected Docked/Detached host while the readable inline projection remains.
- Scroll through consecutive responses longer than 1,200 characters and verify
  the inline projection remains bounded, the complete response opens from its
  title action, and crossing each row does not pad the window to its 32-row cap.
- Stop wheel, touchpad, and scrollbar-thumb scrolling between messages and verify
  the viewport does not perform a delayed model replacement or position jump.
- Scroll through consecutive large responses at minimum, intermediate, and wide
  window sizes. Resize both width and height between passes and verify the
  viewport never exposes a blank virtual spacer before the real history end.
- At the minimum supported width, drag-select user and assistant message text,
  copy it with the platform shortcut, and verify no row action is clipped or
  covered because no per-message action is mounted.
- Drag vertically across multiple wrapped lines in one long message and verify
  the selection continues without moving the page; then verify wheel/touchpad
  scrolling and scrollbar dragging still move the message viewport.
- Capture message and viewport geometry before pointer-down, during selection,
  and after pointer-up. The same plain `Text` must remain mounted and neither
  the bubble height nor scroll position may change because selection started.
- Scroll to the top and stop. Verify exactly one history page loads automatically
  and the first visible message remains on the same viewport pixel when the load
  completes. Scroll away and back to verify the next page can load without a
  duplicate-request loop; verify the explicit pagination action still works.
- Verify timestamps at today/yesterday/six-day/seven-day/year boundaries, and
  verify a legacy time-only history row remains visible unchanged.
- Send successful and failing messages in both modes. Verify the pending timer
  advances, the final duration freezes, survives restart, and does not leak
  between concurrent AI/RAG requests.
- Make the history path unwritable and verify Send restores the draft, does not
  issue the network request, and reuses the same next message ID after recovery.
- Verify an unset or non-HTTPS RAG endpoint fails inline without a network
  request, while a configured HTTPS endpoint preserves the existing payload.
- Run `scripts/benchmark-chat-interaction.sh --runs 3` against one release binary;
  verify both phases exist in every trace, optimized mounted rows never exceed
  32, commit-adjacent p95 and every mutation-to-render latency are at most 50 ms,
  startup bottom distance remains at most 1 px, wheel direction has at least ten samples, drag/wheel
  reverse movement remains at most 1 px, and compare p50/p95/max rather than
  relying on an average.
- Verify both 24-step fast scrollbar drags (bottom-to-top and top-to-bottom)
  produce at least five rendered drag samples and report at most 1 px of blank
  exposure during drag and after release.
- In both AI Chat and RAG, scroll to document offset zero and verify the
  scrollbar thumb reaches the top of the message viewport track without a
  native-title-bar-sized gap. Repeat in the AI Review conversation and PR
  Conversation surfaces to confirm their scrollbar origins remain unchanged.
- Run `cargo test --lib --locked` and compile the root Slint UI.
