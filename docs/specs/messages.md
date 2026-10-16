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
  independently generated `GitHubMessagesPage` API. Its build script compiles
  the page in Slint library-module mode: the exported `Rectangle` remains an
  embeddable `ComponentFactory` product and must not be converted into a second
  application `Window`.
- `MessageCenter` is the stable feature Store facade. Its private typed Actions
  reduce through the framework-neutral `usher-state-store::StateStore` port.
  Subscribers hydrate immediately and follow changed revisions automatically;
  each changed revision automatically queues the complete bounded State to the
  JSON persistence worker. Concrete Store-engine types never cross this facade.
  Application startup may construct the facade empty and hydrate JSON after the
  first frame. That typed hydration Action merges messages published meanwhile,
  including `(channel, deduplication key)` identity, before persistence starts;
  it never replaces a newer in-memory snapshot with an older disk snapshot.
- `src/message_center.rs` is the only root-window bridge. It converts immutable
  snapshots to Slint models, mirrors theme/viewport state, pushes explicit
  total/unread/read filter counts, and forwards page commands to the domain
  service. Inbox row actions use dedicated fixed-glyph controls and direct
  semantic `I18n` bindings so dynamic list delegates cannot alias icon or label bindings.
  The independently compiled page owns a dedicated enum-only
  `MessageTooltipOverlayState` and root-layer
  `GitHubMessageTooltipOverlayHost`. Delegates publish only an action kind and
  anchor rectangle; the page Portal owns one `GitHubTransientFloatingOverlayHost`
  plus three permanently mounted factory-root surfaces for
  `I18n.messages-mark-as-read`, `I18n.messages-mark-as-unread`, and
  `I18n.common-delete`. This prevents
  both delegate string aliasing and stale preferred-width reuse while retaining
  the shared anchor-based flip/shift/size geometry without reaching into
  `MainWindow` globals. The action kind selects one Surface with `visible`; the
  transient host receives `active=false` for `None` and after route dismissal,
  which hides its private full-window geometry tree. The Portal never creates a
  host per action or hides a host with opacity. Rows remain clipped list content
  while only the page-owned surface escapes that clip. Filter tab badges bind to integer
  `count` inputs on
  `GitHubSegmentedControlItem` (rendered as separate Text nodes, not one
  concatenated caption). The page header status sentence is a complete string
  pushed from Rust on each snapshot for the same ComponentFactory freeze reason.
- `ui/main.slint` lazily creates one stable factory/page shell on the first
  Inbox visit. Route changes hide rather than remount that shell; while hidden,
  the bridge clears all message payloads from its 20 stable slots and skips page
  projection while the Store keeps accepting Actions and updating the global
  unread selector. Re-entry rehydrates the row model from the latest Store
  snapshot before making the page active. The stable visual tree remains
  mounted: a three-run release A/B found no material RSS saving from rebuilding
  it on each visit and observed mixed physical-footprint regressions.
- `src/message_center/commands.rs` owns bounded FIFO admission and the worker
  for UI-originated typed mutations; `src/notification/ingress.rs` owns pending
  UI payloads and ticketed completion. `src/notification.rs` owns transient Banners.
- `ui/components/messages/github_message_components.slint` owns reusable Inbox
  row visuals and is registered in the UI Components playground.
- `ui/components/status_bar.slint` presents the unread count supplied by the
  message service. It never derives durable count from the transient Banner queue.

- `crates/usher-messages/` 负责消息领域模型、有界 JSON 存储、订阅、后台持久化
  Worker、`notify-rust` 适配器和独立生成的 `GitHubMessagesPage` API。其 build
  script 使用 Slint library-module 模式编译页面：导出的 `Rectangle` 继续作为
  `ComponentFactory` 可嵌入产品，禁止为消除警告而改成第二个应用 `Window`。
- `MessageCenter` 是稳定的 Feature Store 门面；私有强类型 Action 通过与框架无关的
  `usher-state-store::StateStore` 接口 reduce。订阅者立即 hydration，并自动跟随
  有效 revision；每个有效 revision 自动把完整有界 State 排队到 JSON 持久化
  Worker。应用启动可先创建空门面，首帧后再 hydration JSON；该强类型 Action 会按消息
  ID 与 `(channel, deduplication key)` 合并期间新发布的消息，恢复持久化前不得用旧磁盘
  快照覆盖较新的内存 State。具体 Store 引擎类型不得越过该门面。
- `src/message_center.rs` 是唯一主窗口桥接层，负责把不可变快照转换为 Slint model、
  同步主题/视口、下发 All/Unread/Read 整型计数，并把页面命令转给领域服务。Inbox
  行动作使用固定 SVG 和直接绑定语义 `I18n` 的 Tooltip 文案，避免动态列表 delegate 串用图标或文案
  binding。独立编译页面拥有专用、只传枚举的
  `MessageTooltipOverlayState` 与根层 `GitHubMessageTooltipOverlayHost`。Delegate
  只发布动作类型和锚点矩形；页面 Portal 只拥有一个
  `GitHubTransientFloatingOverlayHost`，页面根部三个常驻独立 Surface 分别绑定
  `I18n.messages-mark-as-read`、`I18n.messages-mark-as-unread` 和 `I18n.common-delete`，
  既避免 delegate 字符串串用，也避免复用旧的
  preferred-width。定位仍复用共享的锚点式 flip/shift/size 算法，但不读取
  `MainWindow` global。动作类型通过 `visible` 选择一个 Surface；类型为 `None` 或离开
  路由时 transient host 收到 `active=false`，由它隐藏私有全窗口定位树。Portal 禁止为
  每个动作创建独立宿主，也禁止用 opacity 隐藏宿主。列表行继续裁剪内容，只有页面根层
  Surface 逃逸该裁剪。筛选
  Tab 徽标绑定
  `GitHubSegmentedControlItem` 的整型 `count`（控件内用独立 Text 节点渲染，
  禁止拼成单一字符串）。页头状态句由 Rust 在每次快照中下发完整字符串，原因
  同为 ComponentFactory 页面上动态字符串拼接会在首次绘制后冻结。
- `ui/main.slint` 在首次进入 Inbox 时才懒创建一个稳定 factory/page shell；路由
  切换只隐藏、不重新挂载该 shell。隐藏期间 bridge 清空 20 个稳定槽位的消息数据，
  并跳过页面投影；Store 仍继续接收 Action、更新全局未读 selector。重新进入时先从
  最新 Store 快照 hydration 行 model，再激活页面。视觉树保持稳定挂载：三轮 Release
  A/B 未测得逐次重建带来的实质 RSS 节省，且 physical footprint 结果出现混合回退。
- `src/message_center/commands.rs` 负责 UI 强类型变更的有界 FIFO 与 Worker；
  `src/notification/ingress.rs` 负责 UI pending payload 和 ticket 完成，
  `src/notification.rs` 负责短时 Banner。
- `ui/components/messages/github_message_components.slint` 负责可复用 Inbox 行视觉，
  并登记到 UI Components 演练场。
- `ui/components/status_bar.slint` 只展示消息服务提供的未读数，不得从短时 Banner
  队列推导持久计数。

## Implementation and focused checks / 实现与专项检查

| Change / 改动 | Inspect together / 配套核对 |
| --- | --- |
| Domain, durability, close / 领域、持久化、关闭 | `crates/usher-messages/src/{center,storage,center_tests}.rs`, `crates/usher-state-store/src/latest.rs` |
| Native delivery / 原生投递 | `crates/usher-messages/src/{model,desktop,native_worker}.rs`, application adapter in `src/message_center.rs` |
| Page/filter/locale projection / 页面、筛选与语言投影 | `src/message_center.rs`, `src/message_center/paging_tests.rs`, `ui/views/messages.slint` |
| Factory keyboard ownership / Factory 键盘所有权 | `src/message_center/focus.rs`, shared `src/focus_navigation.rs` (also used by the playground), shared pagination and the page capture scope |
| UI mutation admission / UI 变更准入 | `src/message_center/commands.rs`, `src/message_center/commands_tests.rs` |
| Row actions and Tooltip portal / 行动作与 Tooltip Portal | `ui/components/messages/github_message_{components,actions,tooltip}.slint`, `ui/state/message_tooltip_overlay_state.slint` |
| Banner mirroring/queue / Banner 镜像与队列 | `src/notification.rs`, `src/notification/ingress.rs`, `src/notification/ingress_tests.rs`, `ui/components/notify/`, root hosts in `ui/main.slint` |

Focused commands: `cargo test -p usher-messages --locked`,
`cargo test --lib --locked message_center::` and
`cargo test --lib --locked notification::`. Root tests include compiled Inbox
paging, host-focus and route-reentry cases. Their simulated input is separate
from the scoped [macOS native regression（归档：docs/validation/ui120-macos-focus-20261015/README.md）](../local-archives.md#validation-tracked-history-20261016)
below; neither establishes renderer memory behavior or acceptance on other platforms.

专项命令覆盖领域、真实编译 Inbox 分页、宿主焦点与重入；模拟输入与下文单独记录的
macOS 原生回归分开，不代表渲染内存或其它平台已验收。

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

1. The domain `MessageCenter::publish` reduces synchronously on its caller and
   returns the message. UI callbacks therefore use the command worker described
   below; they never call domain mutation methods on the Slint event loop.
2. Every changed Store revision automatically offers its immutable shared
   snapshot to `LatestPersistence`: one active write and one replaceable latest
   pending/dirty snapshot. Actions never call save manually. JSON lives at
   `messages/messages.json` under the platform local-data directory.
   `flush()` observes the accepted durable revision and actual write outcome,
   independently of native delivery; explicit `retry_failed()` retries the same
   failed revision without a new Action. A changed deferred state whose
   hydration has not enabled persistence returns failure, never a false success.
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
6. Native delivery has a separate FIFO event worker. Admission never waits for
   capacity: at most 64 entries / 1 MiB UTF-8 payload bytes, including the active
   call, and 128 KiB per entry. Fixed object/allocator overhead is excluded from
   payload bytes and bounded separately by the entry/metadata limits. Overload
   rejects only the new native event, counts/logs the rejection, and preserves
   successful Inbox publication. Admitted events are not coalesced. Ordinary OS
   delivery errors degrade to Inbox; notifier panics remain observable failures.
7. Snapshot subscribers receive immutable copies. Slint subscribers schedule UI
   changes through `slint::invoke_from_event_loop` with weak handles.
8. `shutdown_checked()` closes Action admission, waits for complete admitted
   operations and joins both workers. It retains persistence/native failure
   causes; repeated/concurrent calls observe the same completed barriers.
   Calling close from this center's synchronous Action/subscriber or native
   worker returns `ReentrantShutdown` before acquiring the shutdown lock, so it
   cannot self-wait. The compatibility `shutdown()` logs checked errors.
   The shell runs these barriers in the background cleanup phase after async
   producer drains and original-thread producer stops, before UI/AppKit cleanup.
   AI Review whole-operation receipts include terminal Message/Hook continuations;
   its drain precedes Hooks and final center close. Concurrent exit paths share
   the actual phase/tail barriers, including a cancelled waiter or natural loop
   return. The root window hides before async drains to stop new pointer input.
   Existing failure logging/exit policy remains; the historical rejected P7 #77
   implementation is not restored.

1. 领域 `MessageCenter::publish` 在调用线程同步 reduce 并返回消息；UI callback
   因此必须使用下述命令 Worker，禁止在 Slint event loop 调用领域变更方法。
2. 每个有效 Store revision 自动把共享不可变快照交给 `LatestPersistence`：最多
   一个活动写入和一个可替换的最新 pending/dirty 快照，Action 禁止手工 save。
   JSON 位于平台本地数据目录的 `messages/messages.json`。`flush()` 观察已接受
   持久版本与真实写入结果，不等待原生投递；`retry_failed()` 显式重试相同失败
   revision，不产生新 Action。deferred State 已变化但 hydration 尚未启用持久化
   时返回失败，不报告虚假成功。
3. 原生投递统一使用社区 `notify-rust`，应用代码不得在适配器之外新增各平台实现。
4. `NativeDelivery` 是独立的逐 Draft 策略：默认 `WhenUnfocused`，`Never` 只进入
   Inbox/Banner，`Always` 仅用于明确的严重事件或诊断。窗口激活时默认只显示应用内
   Banner 并写入 Inbox，避免与系统通知重复。
5. Shell 向消息服务提供窗口激活状态。macOS 适配器报告 Key Window 变化；尚无活动
   状态适配器的平台保守视为激活，因此 `WhenUnfocused` 暂不投递原生通知，仍可显式
   使用 `Always`。
6. 原生投递使用独立 FIFO 事件 Worker，准入不等待容量：包括在途调用在内，最多
   64 条/1 MiB UTF-8 payload bytes，单条最多 128 KiB。该 bytes 不含固定对象/
   allocator 开销，对象和 metadata 节点另由条目上限约束。满载只拒绝新原生事件，
   计数并记录日志，Inbox 发布仍成功；已准入事件不合并。普通 OS 错误降级到 Inbox，
   notifier panic 仍作为失败可观测。
7. 订阅者接收不可变快照；Slint 订阅者必须通过 weak handle 与
   `slint::invoke_from_event_loop` 更新 UI。
8. `shutdown_checked()` 关闭 Action 准入，等待已接收操作完整结束并 join 两个
   Worker；保留持久化/原生 Worker 的失败原因，并发/重复调用共享完成屏障。
   从本中心的同步 Action/subscriber 或原生 Worker 中调用关闭时，在获取关闭锁前
   返回 `ReentrantShutdown`，避免自等待。兼容 `shutdown()` 记录 checked 错误。
   Shell 在 async producer drain 与原线程停止回调后、UI/AppKit 清理前的后台阶段
   执行这些屏障。AI Review 完整操作 receipt 覆盖终态 Message/Hook continuation，
   drain 早于 Hooks 与最终中心 close。并发退出、等待方取消和自然 loop 返回共享
   实际阶段及尾部屏障。根窗口在 async drain 前隐藏以阻止新指针操作。保留既有
   失败日志/退出政策，不恢复历史已拒绝的 P7 #77 实现。

### UI command admission and completion / UI 命令准入与完成

Inbox set-read, mark-all-read, delete, clear, Banner mirroring and deferred
hydration enter one feature-owned FIFO. Slint callbacks capture IDs/drafts and
use nonblocking admission: at most 256 active-plus-pending business commands and
1 MiB accounted owned payload capacity (including envelopes). Overflow rejects
the new command and logs a bounded reason; accepted mutations execute in order,
without coalescing. Hydration has one reserved, coalesced control slot. Before
first-frame hydration, accepted UI commands stay buffered within the same budget;
hydration runs first so mark-all/clear also apply to stored history. The worker
performs domain reduction, and closes UI admission before draining and flushing
on the async shutdown stage. Closing before the first frame still hydrates and
merges disk history. Final center close runs on the background cleanup owner
after producers have drained/stopped, retaining HIGH priority and LIFO order
within that phase. Project/request-preference writer joins and the final NORMAL
settings flush also run there. UI/native detach remains on the original thread.

Notification UI payloads have an independent 256-entry / 1 MiB accounting budget
until UI completion takes them, including when the worker has already finished.
Accounting includes UTF-8 text, action slots/text and estimated RGBA icon bytes;
it is not an allocator/RSS ceiling. Actions are snapshotted into an owned model
at admission. `WhenUnfocused` is resolved using window activity at admission;
message ID and creation time are assigned at worker publication. Workers carry
only typed drafts and ticketed completion through weak event-loop handles.
Only the latest ticket for a caller-supplied Banner ID can mount, and mounted
Banner IDs stay unique. Completion rechecks that the durable row still exists,
so accepted delete/clear cannot leave an orphan Banner. Worker admission failures
retain the existing transient Banner fallback. UI-budget overload retains only
one bounded latest fallback (including bounded actions and default icon), one
timer and a logged rejection; it does not silently claim Inbox publication.
Cleanup drops pending UI payloads. This does not change direct background
publishers' domain API or native delivery's separate queue budget.

Inbox 已读/全部已读/删除/清空、Banner 镜像与 deferred hydration 共用 Feature FIFO。
Slint callback 捕获 ID/完整 draft 后非阻塞准入：业务命令含活动项最多 256 项，
自有 payload capacity（含 envelope）记账最多 1 MiB。超限拒绝新命令并记录原因；
已接受变更按序执行，不合并。Hydration 有一个保留且去重的控制槽；首帧前已接受
UI 命令仍计预算并缓冲，先 hydration 再执行，使全部已读/清空也作用于磁盘历史。异步关闭阶段
先关闭 UI 准入，再排空并 flush；首帧前关闭仍会 hydration 合并磁盘历史。最终
中心 close 在发布方排空/停止之后由后台 owner 执行，保留该阶段内的 HIGH 优先级
和 LIFO；Project/请求偏好 writer join 及最后 NORMAL Settings flush 也在后台，
UI/原生集成的卸载仍在原线程。

通知 UI payload 另有含活动项 256 项/1 MiB 记账预算，直到 UI 完成取走才释放，
不随 Worker 提前完成释放；统计 UTF-8、action 槽/文案与估算 RGBA 图像，不代表
allocator/RSS 上限。Action 在准入时形成独占 model；`WhenUnfocused` 使用准入时
焦点，消息 ID/时间在 Worker publish 时生成。Worker 只接收强类型 draft，完成经
ticket 与 weak handle 回到 event loop。同一调用方 Banner ID 只有最新 ticket 可
挂载，已挂载 ID 保持唯一；完成会重新检查 durable row，避免后续删除/清空产生孤立
Banner。Worker 准入失败保留原短时 Banner fallback；UI 预算超限只保留一个有界
最新 fallback（动作截断、默认图标）、一个 timer 和拒绝日志，不声称已进入 Inbox。
清理时释放 pending UI payload。后台直接 publish API 与原生投递队列预算保持独立。

## Failure behavior and implementation gaps / 失败行为与实现缺口

Invalid JSON/storage-schema data is logged and loads as an empty history. Without
a changed business state the original corrupt bytes remain available for recovery;
the next valid changed state atomically replaces them through normal persistence.
Other storage I/O errors are returned, leave accepted in-memory changes intact and
allow an explicit hydration retry; durability remains unavailable until it succeeds.
Deferred hydration merges disk and memory, installs the automatic commit observer,
then compares the current message snapshot with the normalized disk baseline. Only
a different snapshot needs an initial persistence offer. Disk-only loads and an
empty revision zero never trigger a write or create a missing JSON file. Concurrent
or subscriber-reentrant business changes before observer installation are included
in that comparison; later changed commits enqueue automatically. Concurrent hydration
calls share the first successful result; later calls do not reload or resurrect
deleted rows. A hydration subscriber cannot wait for its own hydration operation:
same-thread reentry returns `ReentrantHydration` without closing the center.
`crates/usher-messages/src/hydration_tests.rs` covers real writes, absence of redundant
writes, read/delete/clear races, retries, reentry and checked shutdown.
The center-close worker barrier is covered by `src/shutdown/pipeline/tests.rs`.
UI-120's native failing-before case and corrected pointer/keyboard behavior are
recorded below; package and event-loop fixtures do not replace the remaining
native window/notification acceptance.

无效 JSON/存储 Schema 记录日志后按空历史加载；没有业务变化时保留原损坏字节供恢复，
下一次合法的有效状态变更再通过普通持久化原子替换。其余存储 IO 错误向调用方返回，
保留已接受的内存变更并允许显式重试 hydration；成功前不得报告业务变更已持久化。
deferred hydration 先合并磁盘与内存、安装自动 commit observer，再将当前消息快照
与规范化磁盘基线比较，仅有差异时补交首次持久化。纯磁盘加载与空 revision 0 不写盘，
缺失 JSON 文件也不因此创建。observer 安装前的并发或 subscriber 重入业务变更进入
最终比较；后续有效 commit 自动提交。并发 hydration 共用首次成功结果，之后不再
重读磁盘或复活已删记录；同线程 subscriber 重入 hydration 返回
`ReentrantHydration`，避免等待自身，中心继续可用。
`crates/usher-messages/src/hydration_tests.rs` 覆盖真实写入/零冗余写入、已读/删除/清空
竞争、重试、重入及 checked 关闭。中心最终关闭的后台屏障由
`src/shutdown/pipeline/tests.rs` 覆盖。UI-120 的原生修复前失败及修复后鼠标/键盘
行为见下文记录；包测试和 event-loop 夹具不能替代尚未完成的原生窗口/通知验收。

## Inbox and unread behavior / Inbox 与未读行为

- The domain snapshot's `page` query filters before pagination and borrows at
  most 20 records in the existing newest-first order. Only these records are
  converted to Slint rows; the Store and persistence keep the complete bounded
  history. All/Unread/Read badges and the header's unread count are global,
  never counts of the current page. No history model is rebuilt while scrolling.
  The model retains 20 lightweight delegate slots across page/filter changes,
  empty results, and route exits. Update individual rows without resetting the
  model or recreating delegates, which can leave factory renderer text bindings
  stale. Unused slots have zero height and default empty payloads; route exit
  clears all message payloads. Unchanged rows receive no notification.
- Pagination follows [Primer Pagination](https://primer.style/product/components/pagination/):
  below the list, Previous/Next with chevrons, a highlighted current page,
  first/last and neighboring page links, and noninteractive ellipses for gaps.
  Disable unavailable directions; omit pagination for zero or one page. When
  the group cannot fit, retain Previous/Next without overflowing. Keyboard
  activation and visible focus are required. Changing filters resets to page
  one while preserving the current scroll offset, clamped only to the new
  content bounds. Pointer page changes reset scroll to the top and leave neutral
  page focus; keyboard activation keeps the pagination visible for continued
  navigation. Store updates preserve
  the page unless it is out of range, then clamp to the new last page. Mark all
  as read and confirmed Clear all still apply to the entire durable history.
  The pagination surface and its nine link slots stay mounted when hidden to
  avoid stale factory labels after single-page/multi-page transitions.
  The host-focus adapter fixes UI-120: pointer pagination clears stale sidebar
  focus, and keyboard pagination uses the actual host adapter. Native regression
  scope and remaining acceptance limits are recorded under Factory keyboard focus.
- The list keeps its continuous 1px outline and all four Primer medium (6px)
  corners, including hover/unread fills. Only the first and last rows use
  rounded clipping; middle rows use rectangular clipping. Never place the
  complete list inside a rounded clip that allocates a full-document GPU layer.

- 领域快照的 `page` 查询先筛选再分页，按既有最新优先顺序借用最多 20 条记录；
  只有这些记录转换为 Slint 行，Store 与持久化仍保留完整有界历史。All/Unread/Read
  徽标与页头未读数始终是全局计数，不是当前页计数；滚动不得重建历史 model。
  翻页、筛选、空结果和离开路由时保留 20 个轻量 delegate 槽位，逐行更新，避免
  重置 model 或重建 delegate 导致 factory renderer 沿用旧文字 binding。
  未使用槽位高度为零且数据为空；离开路由时清空全部消息数据，没有变化的行不发送通知。
- 分页遵循 [Primer Pagination](https://primer.style/product/components/pagination/)：
  位于列表下方，上一页/下一页带 Chevron，当前页高亮，保留首页/末页与相邻页码，
  省略区域为不可交互的省略号。边界方向禁用，空结果或单页隐藏分页；宽度不足时只
  显示上一页/下一页，不能溢出。支持键盘激活与可见焦点。切换筛选回到第一页，
  保留当前滚动位置，仅在超出新内容范围时校正；
  鼠标翻页回到顶部并保留中立页面焦点，键盘翻页保持分页控件可见以继续操作；
  Store 更新保留页码，越界则回退到新的末页。全部已读与确认后的
  清空全部仍作用于完整持久历史。
  隐藏时保留分页控件与九个页码槽位，避免单页/多页切换后 factory 页码文字残留。
  宿主焦点适配已修复 UI-120：鼠标分页清除旧侧栏焦点，键盘分页使用真实宿主 adapter。
  原生回归范围与剩余验收限制见下文 Factory 键盘焦点。
- 列表保留连续 1px 外框与四个 Primer medium（6px）圆角，包括 Hover/未读底色。
  只有首末行使用圆角裁剪，中间行采用矩形裁剪；禁止把完整列表放进会分配整表 GPU
  图层的圆角裁剪容器。

- The bell in both the sidebar and status bar opens `Route.Messages` from every
  ordinary route. The status badge shows unread count 1–99 and `99+`; zero hides it.
  Unread presence uses accent color and the badge; status-bar selected chrome is
  only for the Messages route being current.
- The Inbox provides All, Unread, and Read filters, individual read/unread toggles,
  individual delete, Mark all as read, and an explicit two-step Clear all action.
- Every row displays its category label. The category key remains in the row
  model so future category filters do not require a storage migration.
- Persisted application-authored diagnostic copy that exactly matches a known
  packaged catalog value is re-projected in the active locale at the Inbox
  presentation boundary. Recognition is additionally restricted by channel,
  source, and category; remote, user-authored, and unknown copy remains verbatim.
- A row activation marks an unread message read before forwarding an optional
  target. Target routing remains feature-owned and absent targets are valid.
- Read state and deletions persist. Closing a transient Banner does not mark its
  durable message read or delete it. Deleting or clearing an Inbox message removes
  any still-queued Banner linked by `message-id`; Clear all therefore clears the
  mirrored Banner queue as well.
- The transient top-right notification uses a neutral overlay canvas, neutral
  border, shadow, semantic icon, and narrow semantic accent. It is suppressed on
  `Route.Messages`, where the durable Inbox row is already visible, so one event
  never appears simultaneously as two competing blue surfaces. The Banner queue
  retains at most three recent slots in newest-first order (index 0 is the
  visible head; older tails drop from the overlay only). The head Banner's
  in-card footer distinguishes the retained queue count from the durable unread
  total. When unread messages exceed the queue cap it says, for example,
  **3 shown · 9 more** instead of implying that three is the total.
  The footer keeps only the count and a vertical overflow trigger, separated
  from the Banner body by a hairline. Its ActionMenu contains **View all
  notifications** (opens `Route.Messages`) and **Dismiss all** (clears only the
  Banner stack without deleting Inbox rows). Dismissal follows the root-mounted
  ActionMenu contract (same as readiness): Escape and chrome `dismiss-revision`
  on the menu, peer popovers close via `claim-exclusive` on the same click, and
  page blank outside-click is owned under page chrome through
  `TransientOverlayState.dismiss-all`—never a Notify-z full-window scrim that
  would steal the first click from other popups. The overflow-open flag belongs to
  the currently visible queue head: closing that Banner resets it immediately,
  and replacing `queue[0]` with another notification always starts with the
  ActionMenu closed. Clicking the non-interactive Banner surface also closes
  the menu, while action buttons, close, and the overflow trigger keep their
  own behavior.
  Banner rows remain in one stable incremental model. Auto-close and dismiss
  timers carry per-notification generations, so an older timer cannot close a
  newer row; Inbox reconciliation copies only the at-most-three mounted message
  IDs rather than the complete durable history.
  Window hosts must pass the real window size as `overlay-viewport-*` so collision clamping uses
  window-mapped coordinates. Inline showcase Banners opt into the external
  shared queue menu at the window root, publish live window anchors, and use
  Floating UI against the visible page viewport. Section backgrounds and scroll
  clips must never cover this menu; scrolling/resizing updates its placement,
  and moving its trigger out of view closes it. The menu is end-aligned with the overflow trigger horizontally and
  anchored to the actual 24 px footer button with a 6 px gap in either direction.
  Both hosts use shared Floating UI placement: prefer below, flip above the
  button when space is insufficient, and never use the full card as the anchor. This
  prevents a Banner's own action buttons from competing with a second
  text-button toolbar. The semantic rail spans the card's complete left edge
  and is clipped by its outer radius. The footer is shown for a multi-item
  queue or whenever additional unread messages exist.
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
- 持久化的应用诊断文案只有同时满足 channel/source/category 限定，并精确匹配
  内置 catalog 值时才在当前语言重投影；远端、用户及未知文案保持原样。
- 激活未读行时先标记已读，再转发可选目标；目标路由仍由 Feature 持有，无目标合法。
- 已读与删除状态都持久化；关闭短时 Banner 不得把对应持久消息标记已读或删除。
  删除或清空 Inbox 消息时，按 `message-id` 移除仍排队的对应 Banner；Clear all
  因此也会清掉已镜像的 Banner 队列。
- 右上角短时通知使用中性浮层表面、中性边框、阴影、语义图标与窄语义强调线；在
  `Route.Messages` 中不显示该浮层，因为持久 Inbox 行已经可见，避免同一事件同时表现
  为两块竞争注意力的蓝色表面。Banner 队列最多保留 3 条，且最新在前（index 0
  为可见队首；更早的从队尾离开浮层，Inbox 仍保留）。队首 Banner **同一张卡**
  底部会区分已保留队列数与持久未读总数；
  超过上限时显示如 **3 shown · 9 more**，不再暗示总共只有 3 条。
  底部与正文之间有 Hairline，只保留计数与竖向三点入口；ActionMenu 内提供
  **View all notifications**（进入 `Route.Messages`）与 **Dismiss all**（只清空
  Banner，不删除 Inbox）。关闭遵循根层 ActionMenu 约定（与 readiness 相同）：
  Esc / Chrome `dismiss-revision` 由菜单承接，其它弹层靠同一次点击的
  `claim-exclusive` 互斥关闭，页面空白点击由页面 chrome 之下通过
  `TransientOverlayState.dismiss-all` 关闭——禁止在 Notify 根 z 铺全窗口 scrim
  抢走其它弹层的首次点击。overflow 展开状态只属于当前可见队首：关闭当前 Banner
  时必须立即复位，`queue[0]` 替换为下一条通知时 ActionMenu 也必须默认关闭。
  Banner 行保存在同一个增量 model 中；自动关闭与 dismiss timer 携带逐通知世代号，
  旧 timer 不得关闭新行。Inbox 对账只复制最多三条已挂载消息 ID，不复制完整持久历史。
  点击 Banner 的非交互表面也必须关闭菜单；操作按钮、关闭按钮与三点入口保持各自行为。
  窗口宿主必须把真实窗口尺寸传入 `overlay-viewport-*`，
  碰撞夹紧才使用窗口坐标。演练场行内 Banner 选择外部共享队列菜单，将其挂到窗口
  根层并发布实时窗口锚点，使用 Floating UI 按可见页面视口避让。章节背景与滚动
  裁剪不得遮挡菜单；滚动/缩放更新定位，触发器离开视口时关闭。保证
  菜单横向与三点按钮尾对齐，纵向以实际 24px 底栏按钮为锚点，上下均留出 6px。
  两类宿主共用 Floating UI：优先向下，空间不足时翻转到按钮上方，禁止把整张卡片
  当成锚点。这样 Notify 自身已有操作按钮时，不会再叠加
  第二条文字按钮工具栏。左侧语义色条贯穿卡片全高，并由外层圆角裁剪。多条 Banner
  排队或存在额外未读消息时显示该底部行。
- 行遵循 github.com/notifications 的未读层级：未读行静止态使用 `canvas-subtle`、
  在类型图标前显示 8px 强调色未读圆点（占位常驻），标题为加粗 `fg-default`；已读
  行透明底 + muted 标题。Hover 使用 `accent-subtle`。前导类型图标保留 Primer
  语义色。改变已读状态不得重排最新优先序列。

## Factory keyboard focus / Factory 键盘焦点

`src/message_center/focus.rs` owns the Inbox's focus on the actual MainWindow
adapter. The independently compiled page's generated `.focus()` targets its
private adapter; leaving focus there does not route native host key events to
the page. A valid pointer pagination activation carries an explicit synchronous
signal, rather than inferring its origin from residual private focus. After the
page updates and scrolls to the top, its neutral capture scope takes host focus
and the private owner is cleared. Return/Space on that scope performs no business
action: it must not navigate Home or mark messages read.

Keyboard pagination retains the mounted link slot's focus and reveals the footer
within the new content bounds. A numbered slot's label can change when the
neighboring-page window is reprojected; focus does not promise to follow the same
semantic page number. Previous/Next retain their directional identity. Shared
links do not run generated `.focus()` again during
keyboard activation. Shared `src/focus_navigation.rs` traverses the mounted host's eligible
controls for Tab/Shift+Tab across the factory boundary. A standalone `Key.Backtab`
also requests reverse traversal without requiring the Shift modifier; unrelated
Control/Alt/Meta combinations remain outside this Tab handler. Disabled/hidden controls,
ellipsis slots and transparent/offscreen sidebar copies are excluded. Each chosen
target retains its actual mounted rectangle and enclosing Flickable viewport.
Before assigning Tab focus, the owner applies the minimum vertical movement to
reveal that target, clamped to the viewport's actual content bounds. This avoids
an offscreen row's own Clip rejecting reverse focus and triggering Slint's fixed
forward fallback. The [before trace（归档：docs/validation/ui120-macos-focus-20261015/backtab-trace-before.log）](../local-archives.md#validation-tracked-history-20261016)
records that local loop; the [compiled regression（归档：docs/validation/ui120-macos-focus-20261015/focused-reveal-after.log）](../local-archives.md#validation-tracked-history-20261016)
passes complete reverse traversal and exit. This uses live geometry without
estimated row heights, extent changes or rebuilding the 20-slot message model.

When keyboard paging disables the focused Previous/Next direction, the shared
link must clear its painted keyboard outline and ignore further activation. The
pinned Slint FocusScope can ignore FocusOut after disabling, so clearing only in
`focus-lost` is insufficient. A subsequent Tab must move to an eligible control
without leaving a second focus ring on the disabled direction. The compiled
regression inspects the actual border width, expecting zero after disabling;
the [recorded before case（归档：docs/validation/ui120-macos-focus-20261015/disabled-outline-before.log）](../local-archives.md#validation-tracked-history-20261016)
fails with 2px rather than zero. The final native build also passes the page
49 → Space on Next → page 50 → Tab to host sequence with only the host focus ring.

The page capture scope forwards application shortcuts through the existing root
callback and consumes them only when executed. Ordinary unmatched keys are
rejected. Modal or inactive page input is blocked from reaching background
controls. Route exit releases only host focus owned by this page. These rules
use the pinned Slint focus APIs; a toolkit upgrade must rerun compiled and native
focus acceptance before removing the adapter.

`src/message_center/focus.rs` 将 Inbox 焦点归真实 MainWindow adapter 所有。
独立编译页面的 `.focus()` 指向私有 adapter，不能单靠它接收宿主原生键盘事件。
有效鼠标分页激活通过显式同步信号标记来源，不反查残留私有焦点。更新页码并回到
顶部后，中立 capture scope 接管宿主焦点并清除私有 owner；其 Return/Space 不执行
业务动作，不能跳 Home 或把消息标为已读。

键盘翻页保留已挂载链接槽位的焦点，并按新内容边界显示分页栏。相邻页窗口重投影时，
数字槽位的标签可变化，不保证焦点一直跟随同一个语义页码；Previous/Next 的方向语义
保持不变。共享链接在键盘激活时不再次调用
生成的 `.focus()`。共享 `src/focus_navigation.rs` 按已挂载宿主顺序处理 Tab/Shift+Tab，
独立 `Key.Backtab` 也按反向处理，不要求同时带 Shift；带 Control/Alt/Meta 的其它
组合不进入该 Tab 处理。跨越 factory 边界时排除禁用/隐藏控件、省略号、透明或屏外
侧栏副本。选中目标保留真实挂载矩形及所属 Flickable 视口；赋予 Tab 焦点前按最小
垂直位移精确显示该目标，并夹紧到真实内容边界，避免离屏行自身 Clip 拒绝反向焦点后
触发 Slint 固定向前 fallback。[修复前 trace（归档：docs/validation/ui120-macos-focus-20261015/backtab-trace-before.log）](../local-archives.md#validation-tracked-history-20261016)
保留了该局部循环；[compiled 回归（归档：docs/validation/ui120-macos-focus-20261015/focused-reveal-after.log）](../local-archives.md#validation-tracked-history-20261016)
已通过完整反向遍历并离开 factory。该过程使用实时几何，不估算行高、不改变 extent，
也不重建 20 个消息槽位。

键盘翻页使当前 Previous/Next 方向禁用时，共享链接必须清除键盘焦点描边并拒绝继续
激活。固定版本的 Slint FocusScope 在禁用后可能忽略 FocusOut，仅靠 `focus-lost`
清理不足；后续 Tab 到可用控件时，禁用方向不能残留第二个焦点环。Compiled 回归检查
真实 border width，要求禁用后为零；保留的修复前用例实际得到 2px，因而失败。
最终原生构建也通过第 49 页聚焦 Next → Space 到 50 页 → Tab 到宿主的序列，
只保留宿主焦点环。

页面 capture scope 通过既有根 callback 转发应用快捷键，只有确实执行才接受；
活动页面的其余按键返回未处理。Modal 或非活动状态阻止按键进入背景控件；离开路由
只释放属于本页的宿主焦点。适配依赖固定 Slint 焦点 API，升级 toolkit 后须重新完成
compiled 与原生验收，再判断是否移除。

### Native regression scope / 原生回归范围

The [2026-10-15 macOS record（归档：docs/validation/ui120-macos-focus-20261015/README.md）](../local-archives.md#validation-tracked-history-20261016)
uses the complete application on macOS 27.0.1 arm64, Slint 1.18.1 and
Winit/FemtoVG, with an isolated store of 1,000 synthetic messages. CUA supplies
native input; the acceptance steps do not dispatch `WindowEvent` or invoke UI
callbacks. The baseline establishes the sidebar Home focus ring, clicks page 4,
then reproduces Return navigating Home. On the final build, the same English
sequence followed by Return and Space stays in Messages at first row 0940 with
all 1,000 messages unread. Runtime switching through Settings to Chinese keeps
the factory mounted; Home keyboard focus followed by page 1 and Return/Space
also stays in Messages at first row 1000 with unread state intact.

Native forward Tab reaches the footer, Return/Space changes pages while keeping
it visible, and Tab leaves the factory for host controls. At the last-page
boundary, Next becomes disabled without retaining a second focus ring after
Tab leaves it. The 800 × 600 logical window was zoomed to a 5120 × 2692 physical
window at scale 2 and restored to 1600 × 1200 physical without overflow. Command+B
toggles the sidebar in both directions. Home/bell reentry preserves the selected
page. Empty Read results hide pagination; after marking one message read, Read
contains one row and no footer, while All restores full-history paging. A final
pointer page change followed by Return/Space preserves the remaining 999 unread
messages. Numbered-slot label reprojection was observed and follows the slot
focus contract above. See the linked record for the exact sequences and binary.

This closes the reproduced UI-120 core failure; it is not a blanket keyboard
acceptance claim. Reverse traversal remains a native verification limit: the
current CUA Shift+Tab input also moved forward between ordinary host controls,
so it could not supply a trustworthy native reverse case. Compiled Shift+Tab
coverage remains separate. Windows and screen reader behavior have not been
inspected. The application minimum is 800 × 600px; forced 640px direction-only
coverage belongs to the compiled component fixture, not an available native
window mode. The linked record owns final build identities and executed checks.

[2026-10-15 macOS 记录（归档：docs/validation/ui120-macos-focus-20261015/README.md）](../local-archives.md#validation-tracked-history-20261016)使用完整
应用、macOS 27.0.1 arm64、Slint 1.18.1 和 Winit/FemtoVG，消息与设置均在隔离目录，
包含 1,000 条合成消息。实际输入由 CUA 提供，不通过 `WindowEvent` 或 UI callback
代替。修复前先建立侧栏 Home 的蓝色焦点环，再点第 4 页、按 Return，重现跳 Home；
最终构建执行相同英文前置条件后，Return/Space 仍停留消息页、首条为 0940，
1,000 条消息仍全部未读。从 Settings 运行时切到中文时保留 factory；重新建立 Home
键盘焦点，再点第 1 页、按 Return/Space，也停留消息页、首条为 1000，未读状态保持。

原生正向 Tab 可到达分页栏，Return/Space 翻页后分页栏继续可见，Tab 也能离开
factory 到宿主控件。末页边界 Next 禁用后，Tab 移走只保留宿主焦点环。原生窗口从
800 × 600 逻辑尺寸放大至 scale 2 下的 5120 × 2692 物理尺寸，再还原到 1600 × 1200
物理尺寸，未见溢出。Command+B 可来回切换侧栏，Home/铃铛重入保留所选页。
空 Read 隐藏分页；标记一条已读后，Read 仅一条且无分页，All 恢复完整历史分页；
最后鼠标翻页再按 Return/Space，剩余 999 条未读保持。数字槽位标签重投影也已观察，
遵守上文槽位焦点契约。精确操作序列与二进制身份见链接记录。

已复现的 UI-120 核心问题由此关闭，但不表示整个键盘矩阵已通过。原生反向遍历仍受
验证工具限制：当前 CUA 的 Shift+Tab 在普通宿主控件之间也表现为前进，不能作为可信
反向输入；compiled Shift+Tab 覆盖单独记录。Windows 和读屏行为未做原生检查。
应用窗口最小为 800 × 600px；强制 640px 的仅方向模式属于 compiled 组件边界夹具，
不是可到达的原生窗口模式。最终二进制身份与实际检查结果以链接记录为准。

## Regression checklist / 回归清单

- Establish real sidebar Home keyboard focus before clicking pagination; verify
  the first click changes page, scrolls to top, and Return/Space cannot activate
  Home or mutate read state. A pointer-selected Home without keyboard focus is
  not a sufficient before/after test.
- In source/non-source locales, verify keyboard pagination, visible focus,
  complete Tab/Shift+Tab and standalone Backtab entry/exit, disabled boundaries
  with no residual focus outline, ellipses, narrow
  direction-only mode, modal blocking, root shortcuts and route reentry.
- 先建立侧栏 Home 的真实键盘焦点，再单击分页；验证首次点击翻页并回顶部，随后
  Return/Space 不跳 Home、不改变已读状态。仅鼠标选择 Home 不足以证明修复。
- 源语言/非源语言下覆盖键盘翻页、可见焦点、完整 Tab/Shift+Tab 与独立 Backtab 出入、
  禁用边界无残留焦点环、
  省略号、窄屏仅方向、Modal 阻断、根快捷键和路由重入。

- Block the real persistence adapter, dispatch 1,000 changes and verify only
  active/latest shared owners remain; flush and restart must recover the final
  read/delete state. Inject failure and retry the same durable revision.
- Block the notifier and verify durable flush completes independently; test
  FIFO, active-inclusive entry/byte caps, overload Inbox durability, ordinary
  OS degradation, panic/dual-fault outcomes, admitted-operation shutdown and
  rejected lifecycle reentry. Unhydrated changed state must not report success.
- 阻塞真实持久适配器，提交 1,000 次变化并确认仅保留 active/latest 共享 owner；
  flush/重启必须恢复最终已读/删除状态；注入故障并重试相同持久 revision。
- 阻塞 notifier，验证 durable flush 独立完成；覆盖 FIFO、包含在途的条数/bytes
  上限、满载时 Inbox 持久化、普通 OS 降级、panic/双故障、已接收操作的关闭及
  生命周期重入拒绝；未 hydration 的变更状态不得报告成功。

- In English and Chinese, open the inline showcase queue menu and click its
  overlay actions. Assert a 6 px button gap both below and above; verify scrolling, bottom-edge avoidance,
  resizing, Escape, offscreen dismissal, and the production notification menu.
- 中英文下打开行内通知菜单，验证浮层操作可点击；验证上下展开均与按钮相距 6px、
  滚动跟随、底部避让、缩放、Escape、锚点离屏关闭，以及生产通知菜单。

- Check 0/1/20/21/78/1,000 records: each projected page has at most 20 rows,
  all pages cover the filtered history once, and badges retain global counts.
  Verify first/last direction disabling, numbered jumps, ellipses and narrow
  Previous/Next layout. Delete the last item on the last page and mark all read
  while viewing Unread; neither may leave an invalid empty page. Re-enter after
  off-page publications and verify the current query uses the latest snapshot.
- Inspect all four corners at the top/bottom, read/unread and hovered rows,
  one-row pages, light/dark themes, and minimum/wide window sizes. Compare
  footprint/RSS with the same synthetic data and renderer; corner layers must
  be row-sized, not sized to all stored messages.
- 检查 0/1/20/21/78/1,000 条：当前页最多 20 行，所有页恰好覆盖筛选后的历史，
  徽标仍是全局计数。验证首页/末页方向禁用、页码跳转、省略号与窄屏上一页/下一页。
  删除末页最后一条、在未读筛选中全部已读都不能留下越界空页。离页发布消息后重新
  进入，当前查询必须使用最新快照。
- 检查顶部/底部四角、已读/未读/Hover、单行页、明暗主题及最小/宽窗口；在相同
  合成数据与渲染器下对比 footprint/RSS，圆角图层必须限制在行大小而非完整历史。

- Publish from a non-message route and verify Banner, OS notification (when
  permission is granted), durable Inbox row, and unread badge.
- Restart and verify messages/read state reload newest-first.
- Exercise All/Unread/Read, individual toggle/delete, Mark all, and two-step Clear all.
- Clear all, publish again from UI Components, reopen Messages, and verify tab
  badges plus the header status sentence match the new unread/total counts.
- While Messages is closed, publish several messages from other routes, then
  open/reopen Messages and verify automatic Store hydration shows the newest
  rows and counts on the first visible frame.
- With memory diagnostics enabled, leave Messages and verify the lifecycle log
  reports `active=false projected_rows=0`; repeat enter/leave and verify the
  slots retain no message payloads while inactive and rehydrate before the
  first visible frame after re-entry.
- Alternate Mark read, Mark unread, and Delete tooltips across rows, then leave
  Messages and repeat Messages-to-Home 20 times. Verify one fixed Surface at a
  time, a false transient-host `active` state while idle, and no sustained
  per-cycle RSS/physical-footprint growth. Run the static UI policy positive and
  negative fixtures so raw or opacity-hidden full-window hosts remain blocked.
- From a non-Messages route, publish a permanent Banner, delete or Clear all its
  Inbox row, leave Messages, and verify the Banner is gone from the queue.
- Queue three-plus Banners off Messages, confirm only the head shows with a
  Dismiss all affordance, open the overflow menu and close it by blank page
  click plus Escape, then reopen and verify another page ActionMenu opens on
  the first click (peer exclusive-claim, no double-click), dismiss all, and
  verify Inbox rows remain.
- With at least two queued Banners, open the first head's overflow menu and
  close that Banner with its X button. Verify the next head appears with its
  overflow ActionMenu closed.
- Open the Banner overflow and verify the menu is end-aligned with the kebab
  and anchors to its 24 px button with a 6 px gap below, or above when flipped;
  it must not overlap the trigger.
- With the overflow open, click the Banner body or footer count and verify the
  menu closes; then verify action buttons, X, and the kebab still activate on
  their first click.
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
- Messages 关闭期间从其他页面发布多条消息，再打开/重新打开 Messages，确认 Store
  自动 hydration 在首个可见帧展示最新消息行与计数。
- 启用内存诊断后离开 Messages，确认生命周期日志显示
  `active=false projected_rows=0`；重复进入/离开，确认非活动期间所有槽位消息数据
  为空，并在重新进入后的首个可见帧前完成 hydration。
- 跨行交替悬浮已读、未读和删除动作，再离开 Messages 并执行 20 次 Messages→Home。
  确认同一时刻仅一个固定 Surface 可见、闲置时 transient host 的 `active=false`，且
  RSS/physical footprint 不随轮次持续增长。运行 UI policy 正反例，确保底层全窗口宿主
  或 opacity 隐藏宿主无法重新进入 Feature。
- 在非 Messages 路由发布一条常驻 Banner，删除或 Clear all 对应 Inbox 行后离开
  Messages，确认 Banner 已从队列消失。
- 在非 Messages 路由连续排队 3 条以上 Banner，确认只显示队首且出现 Dismiss all，
  打开 overflow 菜单后点击页面空白与按 Esc 均可关闭，再打开后点页面其它
  ActionMenu 入口应一次切换（互斥，无需点两次），一键关闭后 Inbox 行仍在。
- 至少排队两条 Banner，展开第一条队首的 overflow 菜单后直接点击 X 关闭，确认
  下一条队首出现时其 overflow ActionMenu 默认关闭。
- 展开 Banner overflow，确认菜单横向与三点按钮尾对齐、纵向以 24px 按钮为锚点，
  向下或翻转向上都保持 6px 间距，不能覆盖触发按钮。
- overflow 展开时点击 Banner 正文或底栏计数区域，确认菜单关闭；再确认操作按钮、
  X 与三点入口仍能在第一次点击时正常响应。
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
