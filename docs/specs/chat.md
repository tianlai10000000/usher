# Chat Behavior Specification / Chat 行为规格

## Scope / 范围

This is the stable contract for persisted Chat history, message presentation, virtualized history rendering, and inline request failures.

本文约束 Chat 历史持久化、消息展示、虚拟化历史渲染和请求失败的内联反馈。

## Ownership / 所有权

- Persistence and session state: `src/chat/persistence.rs`, `src/chat/session.rs`.
- UI payload rendering and bounded markdown cache: `src/chat/renderer.rs`.
- Async request and route interaction: `src/chat/interactive.rs`.
- Bounded height estimation, prefix cache, and virtual-window selection:
  `src/chat/virtual_layout.rs`.
- Presentation: `ui/views/chat.slint`, bridged through `ui/main.slint`.

- 异步请求与路由交互：`src/chat/interactive.rs`。
- 有界高度估算、prefix cache 与虚拟窗口选择：`src/chat/virtual_layout.rs`。

## Invariants / 不变量

1. Chat history remains JSONL and old entries without a `status` field remain readable as normal messages.
2. Application-generated error/success state is stored separately from message text and rendered with vector icons. Do not add fixed Emoji prefixes that force a color-font fallback.
3. Legacy assistant entries beginning with `❌` or `✅` are recognized at render time and shown without the prefix; the history file is not destructively rewritten.
4. Request failures remain inline assistant messages and do not duplicate centralized HTTP feedback.
5. Only a bounded virtual window is converted into Slint message rows; scrolling and resize must not rebuild the complete history model.
6. Markdown render caching remains bounded by both entry count and approximate bytes and is cleared when leaving Chat.
7. History I/O and HTTP requests never block the Slint event loop; UI updates are scheduled back onto that loop.
8. The Chat component and its Slint message model are route-scoped. Leaving Chat destroys the component and clears its UI model/cache; the Rust session remains the source of truth and reconstructs the bounded model on re-entry.
9. A history-page or assistant response that completes after leaving Chat updates the Rust session/persistence only and must not remount or repopulate the hidden Chat UI.
10. Chat presentation is application-owned: native layouts plus
    `GitHubScrollView`, `GitHubSpinner`, `GitHubCopyButton`, and
    `GitHubIconButton`; Material visuals are not part of the product contract.
11. Message surfaces use compact semantic borders and six-pixel corners.
    Suggestions and send/load actions support keyboard activation and
    keyboard-visible focus.
12. The composer adapts to available width, retains resize behavior, and keeps
    its model mounted while sending. Loading does not duplicate or reorder the
    bounded message window.
13. The history pagination action is mounted in the message flow immediately
    after the virtual top spacer. It reserves its own height and never overlays
    the oldest mounted message.
14. New user-facing request errors use concise English recovery text. Provider,
    HTTP status, and decode details remain in logs and are not exposed verbatim
    in message bubbles.

1. Chat 历史继续使用 JSONL；缺少 `status` 字段的旧记录按普通消息兼容读取。
2. 应用生成的错误/成功状态与正文分开保存，并使用矢量图标展示；禁止重新加入会触发彩色字体回退的固定 Emoji 前缀。
3. 以 `❌` 或 `✅` 开头的旧 assistant 记录在渲染时识别并去除前缀，不破坏性改写历史文件。
4. 请求失败继续使用内联 assistant 消息，不与统一 HTTP 反馈重复。
5. 只有有界虚拟窗口转换为 Slint 消息行；滚动和 resize 不得重建完整历史模型。
6. Markdown 渲染缓存同时受条目数和估算字节数限制，离开 Chat 时清理。
7. 历史 I/O 和 HTTP 请求不得阻塞 Slint event loop；UI 更新必须调度回该线程。
8. Chat 组件及其 Slint 消息 model 按路由管理；离开 Chat 时销毁组件并清空 UI model/cache，Rust session 继续作为事实来源，重新进入时重建有界 model。
9. 离开 Chat 后才完成的历史分页或 assistant 响应只更新 Rust session/持久化，不得重新挂载或填充隐藏的 Chat UI。
10. Chat 展示层由应用自有 native layout、`GitHubScrollView`、
    `GitHubSpinner`、`GitHubCopyButton` 和 `GitHubIconButton` 组成，
    Material 视觉不属于产品契约。
11. 消息表面使用紧凑语义边框和 6px 圆角；建议、发送和历史加载操作必须支持
    键盘激活与仅键盘可见的焦点状态。
12. 编辑器必须随可用宽度自适应，保留拖拽调高能力，发送期间保持 model 常驻；
    loading 不得复制或重排有界消息窗口。
13. 历史分页操作必须挂在 virtual top spacer 之后的消息流内并占据自身高度，
    不得覆盖最早一条已挂载消息。
14. 新产生的用户可见请求错误统一使用简洁英文恢复文案；Provider、HTTP 状态和
    解码技术详情只写日志，不得原样暴露在消息气泡中。

## Regression checklist / 回归检查

- Load a pre-status JSONL history and verify it remains visible.
- Verify a legacy `❌ message` renders as an error icon plus `message`, and copying returns text without the Emoji prefix.
- Trigger a new request error, restart, and verify its status and plain text persist.
- Open enough history to exceed the virtual window and verify scrolling loads the expected rows without linear UI-model growth.
- Verify the history action occupies a row above the oldest message at minimum
  width and while loading; it must not overlap message content.
- Navigate Chat -> Home -> Chat and verify the previous scroll state and messages remain correct.
- While a request or history page is loading, leave Chat and verify the response persists without recreating the Chat component/model; re-enter and verify the completed content appears.
- Resize the window on a non-Chat route with existing Chat history and verify Chat delegates/text layout do not appear in the resize profile.
- Run `cargo test --lib --locked` and compile the root Slint UI.
