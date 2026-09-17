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
   an action Output, and whether the State changed.
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
- Hover、Focus、打开菜单、本地确认计时器等临时 UI 状态，除非确有其他页面消费，
  否则不得进入持久 Store。

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
- Drop a subscription and verify it receives no later commits.
- Run Store crate tests, the feature's persistence tests, full library tests,
  and the affected route-switch matrix.

- 多次 Action 后再订阅，确认首次回调立即得到最新 State 与 revision。
- 分别 dispatch 有变化/无变化 Action，只有前者递增并通知。
- 并发 dispatch 后确认订阅者最终停在最大 revision，不被旧状态覆盖。
- 在路由 A 修改共享数据，再创建路由 B，确认 B 的首个可见帧已包含最新 selector
  状态。
- 每类持久 Action 后重启，确认最终 State 正确恢复。
- 对 deferred store，确认 hydration 不写盘，随后第一个变更 Action 会持久化且 `flush`
  能观察到该写入。
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
pre-shutdown joins the worker after high-priority feature workers. Explicit
credential/provider saves retain their existing save feedback and write gate.
Tests cover burst coalescing, final preference, write failure/recovery and close.

`LatestDelivery` 最多保留一个待投递状态和一个排队回调。`app_runtime/latest_ui.rs`
在 Slint 回调运行时取最新值，适用于 Inbox/未读与 AI Review 快照投影；不得合并逐条
增量、用户动作、审计或事件记录。回归覆盖投递繁忙时 1,000 次提交、旧版本与关闭。

Settings 延迟偏好使用单个 Worker，每次请求重置 100ms 截止时间。Flush 跳过去抖并
等待已接受请求；应用 pre-shutdown 在高优先级 Feature Worker 后 join。显式凭据/
Provider 保存保留反馈及写锁。测试覆盖突发合并、最终偏好、写失败/恢复和关闭。
