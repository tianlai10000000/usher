# Cross-page state store / 跨页面状态 Store

## Scope / 范围

This specification defines the mandatory state-management contract for data
shared by two or more routes, pages, independently compiled UI crates, status
surfaces, or background publishers. It applies to new features and to existing
features when their shared state is materially changed.

本文定义两个或更多路由、页面、独立编译 UI crate、状态展示或后台发布方共享数据时
必须遵循的状态管理契约。该契约适用于新功能，也适用于现有功能发生实质性共享状态
改动时。

## Ownership and dependency boundary / 归属与依赖边界

- `crates/usher-state-store/` owns the framework-neutral `StateStore` port,
  revisioned `StoreSnapshot`, reducer contract, automatic subscriptions, RAII
  unsubscription, selector access, the built-in `LocalStateStore` engine, and
  the reusable `PersistentStore` facade for worker-owned durable commits.
- Feature/domain crates own their State, Action, Output, reducer, validation,
  persistence schema, and side-effect adapters. They depend on the `StateStore`
  trait and must not expose `LocalStateStore` or a third-party engine in their
  public API.
- UI bridges own conversion from a domain snapshot/selector into Slint models.
  They never own authoritative cross-page state.
- Slint globals remain presentation state. A global in one generated crate must
  never be treated as shared with a global of the same name in another crate.

- `crates/usher-state-store/` 持有与框架无关的 `StateStore` 接口、带 revision 的
  `StoreSnapshot`、reducer 契约、自动订阅、RAII 退订、selector 读取、内置
  `LocalStateStore` 引擎，以及负责后台持久化 commit 的通用 `PersistentStore` 门面。
- Feature/domain crate 持有自己的 State、Action、Output、reducer、校验、持久化
  Schema 与副作用适配器。Feature 只能依赖 `StateStore` trait，公共 API 禁止暴露
  `LocalStateStore` 或第三方引擎类型。
- UI bridge 只负责把领域快照/selector 转换成 Slint model，不持有跨页面权威状态。
- Slint global 仍是展示状态；不同生成 crate 中同名 global 绝不能当作自动共享。

## Required state flow / 强制状态流

1. Every shared mutation enters through one typed `dispatch(Action)` path.
   Pages and publishers must not mutate the stored State directly.
2. Reducers are synchronous and side-effect free. They return the next State,
   an action Output, and whether the State changed. Synchronous describes the
   engine contract, not permission to run reduction on the Slint event loop.
   UI-originated mutations use the feature-worker admission path below.
3. A changed commit atomically installs the next immutable State and increments
   its monotonically increasing revision. An unchanged action does neither.
4. New subscribers immediately receive the current snapshot. Changed commits
   automatically notify subscribers; subscribers reject older/equal revisions,
   so concurrent delivery cannot overwrite newer UI state.
5. Cross-page consumers use selectors or a feature facade. They do not depend
   on the concrete Store engine and do not manually poll to maintain correctness.
6. A UI bridge schedules Slint writes with `slint::invoke_from_event_loop` and a
   weak component/window handle. No Store callback touches Slint from a worker.
7. A route created after actions occurred hydrates from the current Store
   snapshot before its visible tree is shown. Data published on another route
   must therefore appear on first paint without an extra refresh action.

1. 所有共享状态变更统一经过强类型 `dispatch(Action)`；页面和发布方禁止直接修改
   Store 内 State。
2. Reducer 必须同步且无副作用，只返回 next State、Action Output 和是否发生变化。
   同步指引擎契约，不代表可以在 Slint event loop reduce；UI 发起的变更必须经过
   下述 Feature Worker 准入路径。
3. 有效 commit 原子安装新的不可变 State，并递增单调 revision；无变化 Action
   不递增、不广播。
4. 新订阅者立即收到当前快照；有效 commit 自动通知订阅者。订阅者拒绝旧 revision
   和相同 revision，避免并发乱序把较新的 UI 状态覆盖掉。
5. 跨页面消费者只能使用 selector 或 Feature 门面，不依赖具体 Store 引擎，也不能
   依靠手工轮询保证正确性。
6. UI bridge 必须用 weak handle + `slint::invoke_from_event_loop` 回写 Slint；Store
   Worker 回调禁止直接操作 Slint。
7. 页面在其他 Action 已提交后才创建时，必须在可见树显示前从当前 Store 快照完成
   hydration。因此其他页面发布的数据应在首次绘制出现，无需额外刷新操作。

## UI admission and feature workers / UI 准入与 Feature Worker

`StateStore::dispatch` and `PersistentStore::dispatch` reduce on their caller;
`PersistentStore` schedules persistence, not reduction. A UI callback therefore
captures the complete typed input and its request/project/route identity before
returning, then submits it through a bounded, nonblocking feature-owned queue.
The worker dispatches accepted commands in order. Starting one unrelated task
per callback without preserving admission order is insufficient. Background
publishers already running on a worker may use their feature facade directly.
No new Tokio runtime belongs in a feature; use `app_runtime` for async work.

`StateStore::dispatch` 与 `PersistentStore::dispatch` 都在调用线程执行 reducer；
后者只调度持久化，不负责迁移 reducer。UI callback 因此必须在返回前捕获完整强类型
输入及请求/项目/路由身份，通过 Feature 自有的有界非阻塞队列准入，由 Worker 按
接受顺序 dispatch。每次 callback 各自启动无序任务不能保证此契约。已经位于 Worker
的后台发布方可直接使用 Feature 门面；异步工作复用 `app_runtime`，不新建 Runtime。

Queue exhaustion and closing reject new work explicitly without disturbing
accepted commands. Features define whether an adjacent identical request may be
deduplicated; do not coalesce distinct ordered actions such as `A → B → A` into
one latest snapshot. Persistence and replaceable UI *state* delivery may still
coalesce according to their own contracts. A durable command receipt owns its
accepted input/result and waits for its real flush on a worker; it must never
substitute a later UI or Store value for the accepted request or report durable
success from admission alone.

队列耗尽或关闭必须显式拒绝新请求，同时保留已接受动作。Feature 可定义相邻同值
请求的去重规则，禁止把 `A → B → A` 等不同顺序动作合成最新快照；持久化和可替换的
UI 状态投递仍可按各自契约合并。持久命令 receipt 保留已接受输入/结果，在 Worker
等待真实 flush；不得用之后的 UI/Store 值替换原请求，也不得仅凭准入成功报告落盘。

Worker-queue capacity does not bound completed payloads waiting for the UI.
Every bridge that retains a complete request, model, image or completion must
also bound those owners until its callback runs or is dropped. A UI completion
uses weak handles and `invoke_from_event_loop`, rejects stale request/revision
identity and preserves later local edits or route visits. Only the current
attempt owns transient success/failure indicators and reset timers. Shutdown
closes feature admission before draining accepted commands and checked durable
barriers off the event loop; stop background publishers before closing their
shared Store. Concurrent/repeated close must wait for the same final result.

Worker 队列容量不能限制已经完成、正在等待 UI 的 payload；bridge 持有的完整请求、
model、图片或完成 owner 也必须有界，直到回调执行或丢弃。UI 完成使用 weak handle
与 `invoke_from_event_loop`，拒绝迟到身份/revision，保留较新的局部编辑及路由访问。
短时成功/失败指示及复位计时器只归当前 attempt。关闭时先关闭 Feature 准入，再在
事件循环外排空已接受动作及 checked 持久化屏障；共享 Store 的后台发布方须先停止。
并发/重复关闭必须等待相同最终结果。

| Feature / 功能 | Command and UI owners / 命令与 UI 职责 |
| --- | --- |
| AI Agent Save / AI Agent 保存 | `src/ai/settings_state.rs` owns the 8-request active-inclusive FIFO, typed dispatch, per-request snapshot/flush receipt and checked drain; `src/ai/settings_ui.rs` owns a separate 8-owner pending-UI budget, adjacent duplicate handling and request/attempt guards. Draft/factory guards stay in `src/settings_page.rs`. See [AI Agent](ai-agent.md). / 门面与 UI 完成各自最多 8 项，草稿工厂护栏由 Settings bridge 持有。 |
| Inbox mutations and UI notifications / Inbox 变更与 UI 通知 | `src/message_center/commands.rs` serializes UI publication, read/delete/clear and deferred hydration; `src/message_center.rs`, `src/notification.rs` and `src/notification/ingress.rs` project results. Background publishers retain the `MessageCenter` facade. See [Messages](messages.md). / UI 命令有序入 Worker，后台发布方继续使用领域门面。 |
| Workflow / Tag / Branch / CD | `src/workflow/state.rs`, `state/facade.rs` and `state/reducer.rs` own project/generation identity and typed Actions. The 64-operation owner budget spans admission, effects, tasks and UI completion; latest projection is separate. Project navigation follows its committed selection; editor callbacks submit complete raw drafts. See [Tag](tag.md). / 项目与代际隔离，64 owner 覆盖完整操作，草稿/导航不得越过 commit 边界。 |
| Explicit request preferences / 显式请求偏好 | `src/settings/request_preferences.rs` owns FIFO admission, typed dispatch and flush receipts; `request_preferences_ui.rs` schedules presentation. / HTTP 超时与默认合并策略的命令和展示各归其门面/bridge。 |

## Persistence / 持久化

- Durable shared State uses `PersistentStore` or an equivalent feature facade.
  Every changed revision is offered to one owned worker with one active write
  and one replaceable pending snapshot. Intermediate State may be superseded; action
  creators must not call save manually.
- A feature may load and validate persisted data before constructing revision
  zero, or create a deferred store and enable persistence immediately after a
  typed hydration action. Deferred hydration must not redundantly write the
  loaded snapshot back to disk. Corrupt data follows the feature's documented
  recovery policy.
- File/network/OS side effects never run in a reducer or on the Slint event
  loop. Persistence preserves commit order, writes through a temporary file when
  replacing JSON, and participates in the application shutdown flush sequence.
- `flush()` captures the accepted revision and waits for that or a newer
  successful write; write errors return failure and still invoke the error handler.
  `shutdown()` rejects new submissions, drains the latest accepted state and joins
  the worker. Repeated/concurrent shutdown is safe. Control waiters use revision
  predicates rather than an unbounded command queue.
- A persistence failure is observable through the facade callback and bounded
  logs/diagnostics. The feature must document retry, rollback, or memory-only
  degradation and must not silently block UI dispatch.
- A failed latest snapshot remains dirty until superseded or successfully
  retried. `retry_failed()` explicitly schedules one write attempt without an
  Action or a business-revision increment; concurrent callers share an active
  attempt. Flush captures both the accepted revision and scheduled attempt so
  an old failure cannot satisfy a new retry barrier. Ordinary flush/shutdown
  never create retries or busy loops; retry after closing returns failure.
  Hook configuration saves/toggles request this recovery before their durable
  flush. An unchanged explicit AI Agent Save requests it before its flush; a
  changed AI Agent Save waits for its automatic write without implicitly
  retrying that same attempt. The worker retains at most one
  active and one latest pending/dirty unique snapshot.
- Feature workers may skip serialization/writes when a separate durable revision
  is unchanged. Output-only commits still notify live subscribers and satisfy
  the flush barrier because the durable projection has already been written.
  Advance the durable revision for every configuration/summary transition,
  especially terminal state; failed writes must not advance the saved revision.
  / Feature Worker 可按独立持久版本跳过序列化/写盘；纯输出 commit 仍通知活跃订阅者，
  因持久投影已落盘可完成 flush 屏障。所有配置/摘要变化尤其终态必须推进持久版本，
  写入失败不得推进已保存版本。
- Ephemeral UI state such as hover, focus, an open menu, or a local confirmation
  timer does not enter the durable Store unless another page genuinely consumes it.

- 需要持久化的共享 State 必须使用 `PersistentStore` 或等价 Feature 门面。每个有效
  revision 自动交给单一自有 Worker，最多一个正在写入的快照和一个可替换的待写快照；
  中间状态可以被取代，各 Action creator 禁止手工调用 save。
- Feature 可以在构造 revision 0 前读取并校验持久化数据，也可以先创建 deferred store，
  通过强类型 hydration Action 提交数据后立即启用持久化。deferred hydration 不得把刚
  读取的快照冗余写回磁盘；损坏数据按 Feature 文档规定恢复。
- 文件、网络和 OS 副作用不得进入 reducer，也不得在 Slint event loop 执行。
  持久化必须保持 commit 顺序，替换 JSON 时使用临时文件，并接入应用关闭 flush。
- `flush()` 捕获已接受 revision，等待该版本或更新版本成功写入；写入错误必须返回
  失败并继续调用错误回调。`shutdown()` 拒绝新提交，排空最新已接受状态并 join，
  并发/重复关闭安全。控制等待通过版本条件实现，不使用无界控制消息队列。
- 持久化失败必须通过门面回调及有界日志/诊断可观测；Feature 应明确重试、回滚或降级
  为仅内存状态，禁止静默阻塞 UI dispatch。
- 失败的最新快照保持 dirty，直到被新状态替代或重试成功。`retry_failed()` 显式安排
  一次写入，不 dispatch Action、不增加业务 revision；并发调用共享在途尝试。Flush
  同时捕获已接受 revision 与已安排的尝试，旧失败不能提前结束新重试的屏障。普通
  flush/shutdown 不自动重试、不忙循环；关闭后重试返回失败。Hook 配置保存/开关在
  持久化 flush 前请求恢复；AI Agent 同值显式 Save 在 flush 前请求恢复，有变化的
  Save 则等待自动写入，不隐式重试自身失败。Worker 最多保留一个活动
  快照和一个最新 pending/dirty 独占快照。
- Hover、Focus、打开菜单、本地确认计时器等临时 UI 状态，除非确有其他页面消费，
  否则不得进入持久 Store。

## Implementation boundaries / 实现边界

- `crates/usher-state-store/src/lib.rs` owns `StateStore`, `LocalStateStore`,
  subscriptions and `PersistentStore`; `crates/usher-state-store/src/latest.rs` owns
  `LatestPersistence` and `LatestDelivery`.
- `subscribe` hydrates synchronously. Each listener serializes callback delivery
  and replaces its pending snapshot with the newest revision while busy. It
  guarantees current state, not one callback per Action. Ordered events/audit
  records need a separate event channel. Commit observers run inside the serial
  dispatch gate; keep them nonblocking and never dispatch recursively from them.
  Ordinary snapshot subscribers run outside that gate and may reenter dispatch.
- `LatestPersistence::{flush,shutdown}` return `Result<(), String>`;
  `PersistentStore::shutdown` currently returns `()` and discards the worker's
  shutdown result after removing its commit observer. It does not close the
  underlying Store's Action admission. Features that require checked close and
  admission barriers must own those in their lifecycle facade, as
  `MessageCenter::shutdown_checked` does. Do not assume all facades expose the
  same checked API. AI Agent Settings closes its own command admission, drains
  accepted commands and checks `flush()` before calling the unchecked Store
  shutdown; `shutdown_async()` returns that durable result.
- `PersistentStore`'s deferred mode does not validate feature hydration itself.
  A feature must prevent a successful durable result for accepted business
  changes that were never submitted to persistence.

- 引擎/订阅/`PersistentStore` 位于 crate 的 `src/lib.rs`，latest Worker/投递位于
  `crates/usher-state-store/src/latest.rs`。普通订阅同步 hydration；忙碌 listener 只保留最新待交付快照，
  不保证每个 Action 一次回调。顺序事件与审计记录另用事件通道。
- Commit observer 在串行 dispatch gate 内运行，只能做快速提交，不能递归 dispatch；
  普通快照 subscriber 在 gate 外运行，可重新 dispatch。
- `LatestPersistence` 的 flush/shutdown 返回 Result，而当前
  `PersistentStore::shutdown` 返回 `()`、丢弃 Worker 关闭结果，也不关闭 Store Action
  准入。需要 checked 关闭与准入屏障的 Feature 必须由生命周期门面提供，不能误用
  同名 API；`MessageCenter::shutdown_checked` 是对应参考。AI Agent Settings 自行
  关闭命令准入、排空已接受动作，在未返回结果的 Store shutdown 前检查 `flush()`，
  `shutdown_async()` 返回该持久化结果。
- Deferred 模式不替 Feature 核验 hydration；尚未提交持久化的业务变化不得被 Feature
  报告为 durable 成功。

### Deferred hydration boundary / 延迟 hydration 边界

Message-center hydration demonstrates the no-redundant-write boundary: dispatch
the typed merge before installing persistence, register the automatic observer,
then compare the current state with the loaded durable baseline. A differing
state needs an initial offer; equal disk-only or empty revision-zero state does
not. Installing the observer before the final snapshot read ensures concurrent
business changes cannot fall between the comparison and automatic persistence.
Hydration is one successful initialization, not a repeated disk-refresh action.
See [Messages](messages.md) and its concurrency/recovery regressions.

消息中心 hydration 的边界是：强类型合并后安装自动持久化 observer，再将当前状态
与已读取的持久基线比较。只有差异状态需要补交首次写入；纯磁盘恢复与空 revision 0
不提交。最终读取快照前先安装 observer，保证并发业务变更不会落入比较与自动持久化
之间的空窗。hydration 是单次成功初始化，不是重复磁盘刷新；入口、竞争及恢复回归见
[Messages](messages.md)。

## Engine replacement / 引擎替换

`LocalStateStore` is the current built-in engine, not an application API.
Replacing it with a community Redux/Pinia-like library requires an adapter that
implements `StateStore` and passes the same revision, immediate hydration,
automatic subscription, selector, threading, and persistence tests. Feature
facades, Actions, persisted schemas, Slint bridges, and publishers must remain
unchanged. A library is not accepted merely for Redux naming: maintenance,
documentation, runtime ownership, event-loop compatibility, and compile cost
must be measured.

`LocalStateStore` 是当前内置引擎，不是应用 API。未来换成社区 Redux/Pinia 类库时，
必须通过 adapter 实现 `StateStore`，并通过相同的 revision、立即 hydration、自动
订阅、selector、线程与持久化测试；Feature 门面、Action、持久化 Schema、Slint
bridge 与发布方保持不变。仅有 Redux 命名不足以引入依赖，必须评估维护状态、文档、
runtime 归属、event-loop 兼容性和编译成本。

## Reference implementation / 参考实现

The message center and Project registry are reference implementations:

- `MessageCenter` is the stable feature facade.
- private `MessageAction` values are reduced through `StateStore`;
- the status badge and Inbox bridge subscribe automatically;
- Inbox creation and route re-entry read the newest Store snapshot, including
  messages published from other routes;
- each changed revision automatically queues `messages/messages.json` through
  the owned persistence worker.
- `ProjectStateFacade` owns typed add/update/delete, save/logo/overview
  de-duplication, and repository metadata selectors;
- Project creates `PersistentStore` before the first frame is available, hydrates
  after that frame, and then enables automatic persistence;
- Project reports success only after a real durable write and rolls back a failed
  optimistic registry only while that exact failed snapshot remains current.

消息中心与 Project 注册表是参考实现：`MessageCenter` 是稳定 Feature 门面；私有
`MessageAction` 通过 `StateStore` reduce；状态徽标与 Inbox bridge 自动订阅；Inbox
创建/重新进入读取最新 Store 快照（包含其他页面期间发布的消息）；每个有效 revision
自动通过自有 Worker 排队写入 `messages/messages.json`。`ProjectStateFacade` 持有强类型
新增/更新/删除、保存/Logo/概览去重和仓库元数据 selector；Project 在首帧前创建
`PersistentStore`，首帧后 hydration 再启用自动持久化；只有真实写盘成功才提示成功，
失败的乐观注册表也只在对应失败快照仍为当前版本时回滚。

## Regression checklist / 回归清单

Focused automated checks are `cargo test -p usher-state-store --locked`,
`cargo test -p usher-messages --locked`, and the root feature filters
`project::`, `settings::save_worker::`, `ai::settings_state`,
`ai::settings_ui`, and `message_center::` with
`cargo test --lib --locked <filter>`. Store-only tests do not prove route first
paint or application shutdown ownership; inspect the feature bridge and
`src/lib.rs` lifecycle registrations together.

专项单测覆盖引擎与 Feature 门面，路由首帧与应用关闭归属仍须配套核对 bridge 和
`src/lib.rs` 的生命周期注册。

- Block a production feature reducer/persistence observer; the UI callback must
  still return immediately and later admitted commands must preserve their
  captured identities/order. Cover queue exhaustion, adjacent duplicates,
  `A → B → A`, close during a blocked command and post-close rejection.
- Complete worker operations while holding UI delivery. Verify its separate
  pending-owner budget, release on callback/drop/enqueue failure, stale-result
  rejection and preservation of a newly edited draft or route visit.
- Subscribe after several actions and verify the first callback contains the
  latest State and revision.
- Dispatch changed and unchanged actions; only changed actions increment and
  notify.
- Dispatch concurrently and verify every subscriber ends at the greatest
  revision without stale overwrite.
- Publish/change shared data on route A, then create route B and verify its first
  visible frame contains the current selected State.
- Restart after each durable action class and verify the final State reloads.
- For deferred stores, verify hydration causes no write, then verify the first
  changed post-hydration action persists and `flush` observes it.
- Fail a durable write, restore the target and explicitly save the same value;
  verify successful reload without a business revision increment. Interleave an
  old failure/new state, repeated failures, concurrent retry/flush/shutdown and
  closing; verify bounded owners, no automatic retry and the correct barrier.
- Drop a subscription and verify it receives no later commits.
- Run Store crate tests, the feature's persistence tests, full library tests,
  and the affected route-switch matrix.

- 阻塞生产 Feature reducer/持久化 observer，确认 UI callback 仍立即返回，后续
  准入动作保留捕获的身份/顺序；覆盖队列满、相邻重复、`A → B → A`、活动动作期间
  关闭及关闭后拒绝。
- Worker 完成但暂缓 UI 投递时，核验独立 pending owner 预算、完成/drop/投递失败
  的释放、旧结果拦截，以及新编辑草稿/路由访问的保留。
- 多次 Action 后再订阅，确认首次回调立即得到最新 State 与 revision。
- 分别 dispatch 有变化/无变化 Action，只有前者递增并通知。
- 并发 dispatch 后确认订阅者最终停在最大 revision，不被旧状态覆盖。
- 在路由 A 修改共享数据，再创建路由 B，确认 B 的首个可见帧已包含最新 selector
  状态。
- 每类持久 Action 后重启，确认最终 State 正确恢复。
- 对 deferred store，确认 hydration 不写盘，随后第一个变更 Action 会持久化且 `flush`
  能观察到该写入。
- 注入写失败、恢复目标并显式保存同值；确认重载成功且业务 revision 不变。交错旧
  失败/新状态、重复失败、并发 retry/flush/shutdown 与关闭；确认 owner 有界、不自动
  重试且屏障结果正确。
- Drop subscription 后不得再收到 commit。
- 运行 Store crate 测试、Feature 持久化测试、完整 lib 测试和受影响路由切换矩阵。

## Latest UI delivery and preferences / 最新 UI 投递与偏好

`LatestDelivery` retains one pending state and one queued delivery. The Slint
helper in `app_runtime/latest_ui.rs` takes the latest value when its callback
runs; it is used by Inbox/unread and AI Review snapshot projections. It must
not coalesce ordered deltas, user actions or audit/event records. Regression
coverage includes 1,000 offers while delivery is busy, stale revisions and close.

Delayed Settings preferences use one worker with a resettable 100 ms deadline.
Flush bypasses debounce and waits for the accepted request; application
background shutdown joins the worker after high-priority feature workers. Explicit
credential/provider saves retain their existing save feedback and write gate.
Tests cover burst coalescing, final preference, write failure/recovery and close.

`LatestDelivery` 最多保留一个待投递状态和一个排队回调。`app_runtime/latest_ui.rs`
在 Slint 回调运行时取最新值，适用于 Inbox/未读与 AI Review 快照投影；不得合并逐条
增量、用户动作、审计或事件记录。回归覆盖投递繁忙时 1,000 次提交、旧版本与关闭。

Settings 延迟偏好使用单个 Worker，每次请求重置 100ms 截止时间。Flush 跳过去抖并
等待已接受请求；应用后台关闭阶段在高优先级 Feature Worker 后 join。显式凭据/
Provider 保存保留反馈及写锁。测试覆盖突发合并、最终偏好、写失败/恢复和关闭。
