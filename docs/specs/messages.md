# Application message center / 应用消息中心

## Scope / 范围

This specification defines durable application messages, unread state, native
desktop delivery, the global status-bar count, and the GitHub-style notification
inbox. Transient Toast/Banner presentation remains a separate concern.

本文定义可持久化应用消息、未读状态、操作系统桌面通知、状态栏全局计数以及
GitHub 风格通知 Inbox。短时 Toast/Banner 仍是独立职责。

## Ownership / 归属

- `crates/usher-messages/` owns the message domain model, bounded JSON storage,
  subscriptions, background persistence worker, `notify-rust` adapter, and the
  independently generated `GitHubMessagesPage` API.
- `src/message_center.rs` is the only root-window bridge. It converts immutable
  snapshots to Slint models, mirrors theme/viewport state, pushes explicit
  total/unread/read filter counts, and forwards page commands to the domain
  service. Inbox row actions use dedicated hardcoded-glyph controls with local
  tips that match `GitHubIconButton`: below the action, trailing-aligned, and
  separated by `overlay-offset`. Rows keep `clip: false` so those tips are not
  cropped onto the button. The Inbox intentionally bypasses the shared tooltip
  portal.   Filter tab badges bind to integer `count` inputs on
  `GitHubSegmentedControlItem` (rendered as separate Text nodes, not one
  concatenated caption). The page header status sentence is a complete string
  pushed from Rust on each snapshot for the same ComponentFactory freeze reason.
- `src/notification.rs` continues to own transient queued Banners and mirrors
  every created application notification into the durable message center.
- `ui/components/messages/github_message_components.slint` owns reusable Inbox
  row visuals and is registered in the UI Components playground.
- `ui/components/status_bar.slint` presents the unread count supplied by the
  message service. It never derives durable count from the transient Banner queue.

- `crates/usher-messages/` 负责消息领域模型、有界 JSON 存储、订阅、后台持久化
  Worker、`notify-rust` 适配器和独立生成的 `GitHubMessagesPage` API。
- `src/message_center.rs` 是唯一主窗口桥接层，负责把不可变快照转换为 Slint model、
  同步主题/视口、下发 All/Unread/Read 整型计数，并把页面命令转给领域服务。Inbox
  行动作使用写死 SVG；tip 在按钮上本地绘制，与 `GitHubIconButton` 一致（下方、
  尾对齐、`overlay-offset` 间距）。行保持 `clip: false`，避免 tip 被裁进按钮。
  Inbox 特意绕过共享 tooltip 门户。筛选 Tab 徽标绑定
  `GitHubSegmentedControlItem` 的整型 `count`（控件内用独立 Text 节点渲染，
  禁止拼成单一字符串）。页头状态句由 Rust 在每次快照中下发完整字符串，原因
  同为 ComponentFactory 页面上动态字符串拼接会在首次绘制后冻结。
- `src/notification.rs` 继续负责短时排队 Banner，并把每条应用通知镜像到持久消息中心。
- `ui/components/messages/github_message_components.slint` 负责可复用 Inbox 行视觉，
  并登记到 UI Components 演练场。
- `ui/components/status_bar.slint` 只展示消息服务提供的未读数，不得从短时 Banner
  队列推导持久计数。

## Domain and extension contract / 领域与扩展契约

Each stored message has a stable string ID, title, body, source, ingress channel,
semantic category, severity, UTC creation time, read state, optional feature
target, optional external deduplication key, and small string metadata.
`channel` answers where an event entered (`application`, future `webhook`), while
`category` answers which business area it belongs to. Built-in category keys are
`general`, `webhook`, `software_update`, and `cd_workflow`; unknown non-empty keys
are valid so new feature packages extend the system without editing its core.
Publishers use `MessageDraft`; optional native delivery is a draft policy, not
presentation state.

A future Webhook receiver is an ingress adapter: it authenticates and normalizes
the request, then publishes the same draft as application features. It must
supply a stable provider delivery/event ID when available. Retries with the same
non-empty `(channel, deduplication key)` return the existing message and do not
increase the unread count. Raw payloads, credentials, signatures, and
authorization material must never enter message metadata. The HTTP receiver and
provider-specific schemas are intentionally not implemented by this feature.
New ingress adapters, business categories, or target resolvers extend adapter
boundaries without changing persistence consumers. Storage is capped at 1,000
newest messages.

每条消息包含稳定字符串 ID、标题、正文、来源、入口通道、业务分类、语义级别、UTC
创建时间、已读状态、可选功能目标、可选外部幂等键与小型字符串元数据。`channel`
表示事件从哪里进入（`application`、未来 `webhook`），`category` 表示它属于哪个业务
领域。内置分类键为 `general`、`webhook`、`software_update` 和 `cd_workflow`；未知的
非空键同样合法，因此新 Feature 包无需修改消息中心核心即可扩展分类。发布方使用
`MessageDraft`；是否投递系统通知属于 Draft 策略，不属于展示状态。

未来 Webhook 接收器属于入口适配器：它先完成鉴权与请求归一化，再像应用 Feature
一样发布 Draft；若提供方存在稳定的 Delivery/Event ID，适配器必须传入该 ID。相同
非空 `(channel, deduplication key)` 的重试直接返回已有消息，不增加未读数。原始
Payload、凭据、签名与鉴权材料禁止进入消息元数据。本功能明确不实现 HTTP 接收器及
提供方专属 Schema。新增入口适配器、业务分类或目标解析器应扩展适配器边界，不修改
持久化消费者。存储最多保留最新 1,000 条。

## Delivery, persistence, and threading / 投递、持久化与线程

1. Publishing updates the in-memory snapshot synchronously and returns the new
   message. It must not perform filesystem or OS-notification work on the Slint
   event loop.
2. One owned worker serializes JSON persistence and native delivery. JSON lives
   at `messages/messages.json` under the platform local-data directory.
3. Native delivery uses community crate `notify-rust`; application code must not
   add per-platform notification implementations beside this adapter.
4. `NativeDelivery` is an orthogonal per-draft policy: `WhenUnfocused` is the
   default, `Never` is Inbox/Banner only, and `Always` is reserved for explicit
   critical or diagnostic cases. Active-window delivery therefore produces an
   in-app Banner and durable Inbox row without duplicating it in the OS center.
5. The shell supplies window activity to the message service. The macOS adapter
   reports key-window changes; platforms without an activity adapter currently
   remain conservatively active, so `WhenUnfocused` suppresses native delivery
   while `Always` remains available.
6. Native delivery failure is logged and degrades to the durable in-app message;
   it must never roll back the message or block feature completion.
7. Snapshot subscribers receive immutable copies. Slint subscribers schedule UI
   changes through `slint::invoke_from_event_loop` with weak handles.
8. Shutdown flushes preceding worker commands and joins the owned worker through
   the normal high-priority pre-shutdown stage.

1. 发布操作同步更新内存快照并返回新消息；不得在 Slint event loop 执行文件系统或
   OS 通知工作。
2. 唯一 Worker 串行执行 JSON 持久化和原生投递；JSON 位于平台本地数据目录下的
   `messages/messages.json`。
3. 原生投递统一使用社区 `notify-rust`，应用代码不得在适配器之外新增各平台实现。
4. `NativeDelivery` 是独立的逐 Draft 策略：默认 `WhenUnfocused`，`Never` 只进入
   Inbox/Banner，`Always` 仅用于明确的严重事件或诊断。窗口激活时默认只显示应用内
   Banner 并写入 Inbox，避免与系统通知重复。
5. Shell 向消息服务提供窗口激活状态。macOS 适配器报告 Key Window 变化；尚无活动
   状态适配器的平台保守视为激活，因此 `WhenUnfocused` 暂不投递原生通知，仍可显式
   使用 `Always`。
6. 原生投递失败只记录日志并降级为应用内持久消息，不得回滚消息或阻塞功能完成。
7. 订阅者接收不可变快照；Slint 订阅者必须通过 weak handle 与
   `slint::invoke_from_event_loop` 更新 UI。
8. 关闭时通过高优先级 pre-shutdown 阶段处理已排队命令并 Join Worker。

## Inbox and unread behavior / Inbox 与未读行为

- The bell in both the sidebar and status bar opens `Route.Messages` from every
  ordinary route. The status badge shows unread count 1–99 and `99+`; zero hides it.
  Unread presence uses accent color and the badge; status-bar selected chrome is
  only for the Messages route being current.
- The Inbox provides All, Unread, and Read filters, individual read/unread toggles,
  individual delete, Mark all as read, and an explicit two-step Clear all action.
- Every row displays its category label. The category key remains in the row
  model so future category filters do not require a storage migration.
- A row activation marks an unread message read before forwarding an optional
  target. Target routing remains feature-owned and absent targets are valid.
- Read state and deletions persist. Closing a transient Banner does not mark its
  durable message read or delete it. Deleting or clearing an Inbox message removes
  any still-queued Banner linked by `message-id`; Clear all therefore clears the
  mirrored Banner queue as well.
- The transient top-right notification uses a neutral overlay canvas, neutral
  border, shadow, semantic icon, and narrow semantic accent. It is suppressed on
  `Route.Messages`, where the durable Inbox row is already visible, so one event
  never appears simultaneously as two competing blue surfaces.
- The row follows github.com/notifications unread hierarchy: unread rows rest on
  `canvas-subtle`, show an 8px accent unread indicator ahead of the leading type
  icon (slot always mounted), and use bold `fg-default` titles; read rows stay
  transparent with muted titles. Hover uses `accent-subtle`. Leading type icons
  keep Primer semantic color. Read-state changes never reorder the authoritative
  newest-first sequence.

- 侧栏和状态栏铃铛可从每个普通路由打开 `Route.Messages`。状态徽标显示 1–99，
  超过显示 `99+`，零值隐藏。未读只用强调色与徽标表达；状态栏选中底色仅在
  Messages 路由当前打开时出现。
- Inbox 提供 All/Unread/Read 筛选、单条已读/未读切换、单条删除、全部已读以及显式两步
  Clear all。
- 每行展示分类标签；原始分类键保留在行模型中，未来增加分类筛选无需迁移存储。
- 激活未读行时先标记已读，再转发可选目标；目标路由仍由 Feature 持有，无目标合法。
- 已读与删除状态都持久化；关闭短时 Banner 不得把对应持久消息标记已读或删除。
  删除或清空 Inbox 消息时，按 `message-id` 移除仍排队的对应 Banner；Clear all
  因此也会清掉已镜像的 Banner 队列。
- 右上角短时通知使用中性浮层表面、中性边框、阴影、语义图标与窄语义强调线；在
  `Route.Messages` 中不显示该浮层，因为持久 Inbox 行已经可见，避免同一事件同时表现
  为两块竞争注意力的蓝色表面。
- 行遵循 github.com/notifications 的未读层级：未读行静止态使用 `canvas-subtle`、
  在类型图标前显示 8px 强调色未读圆点（占位常驻），标题为加粗 `fg-default`；已读
  行透明底 + muted 标题。Hover 使用 `accent-subtle`。前导类型图标保留 Primer
  语义色。改变已读状态不得重排最新优先序列。

## Regression checklist / 回归清单

- Publish from a non-message route and verify Banner, OS notification (when
  permission is granted), durable Inbox row, and unread badge.
- Restart and verify messages/read state reload newest-first.
- Exercise All/Unread/Read, individual toggle/delete, Mark all, and two-step Clear all.
- Clear all, publish again from UI Components, reopen Messages, and verify tab
  badges plus the header status sentence match the new unread/total counts.
- From a non-Messages route, publish a permanent Banner, delete or Clear all its
  Inbox row, leave Messages, and verify the Banner is gone from the queue.
- Mark several non-adjacent rows read in All and verify their relative order and
  scroll position remain stable; filtered-out rows must leave no layout gaps.
- Verify 0, 1, 99, and 100 unread badge states at 800 px and wide widths.
- With the docked sidebar open, verify header actions reflow above the filters
  and that Blankslate text stays inside the content viewport.
- Deny OS notification permission and verify in-app delivery still succeeds.
- Verify software-update notifications and completed CD runs are stored under
  their explicit categories.
- In UI Components, use the dedicated Message center section to exercise General,
  Webhook, Software update, CD success, Warning, Critical + OS, and In-app only.
- Publish a future-adapter draft twice with the same channel/key and verify one
  durable row, one unread increment, and preserved category/metadata on restart.
- Run the isolated message-package check, full Slint compile, and standard tests.

- 从非消息页面发布，检查 Banner、系统通知（已授权时）、持久 Inbox 行和未读徽标。
- 重启后检查消息/已读状态按最新优先恢复。
- 检查 All/Unread/Read、单条切换/删除、全部已读和两步 Clear all。
- 清空后从 UI Components 再发布，重新打开 Messages，确认 Tab 徽标与页头状态句
  与新的未读/总数一致。
- 在非 Messages 路由发布一条常驻 Banner，删除或 Clear all 对应 Inbox 行后离开
  Messages，确认 Banner 已从队列消失。
- 在 All 中把多条不相邻消息标记为已读，确认相对顺序与滚动位置稳定，筛掉的行不留下空白。
- 在 800px 与宽屏检查未读数 0、1、99、100。
- 展开停靠侧栏后，确认页头操作换行到筛选器上方，且 Blankslate 文本不超出内容区。
- 拒绝系统通知权限后确认应用内消息仍成功。
- 确认软件更新通知与完成的 CD 运行分别写入明确分类。
- 在 UI Components 的独立 Message center 分类中测试 General、Webhook、Software
  update、CD success、Warning、Critical + OS 与 In-app only。
- 用相同通道/幂等键发布两次未来适配器 Draft，确认只产生一条持久行、一次未读增量，
  且重启后分类与元数据仍存在。
- 运行独立消息包检查、完整 Slint 编译和标准测试。
