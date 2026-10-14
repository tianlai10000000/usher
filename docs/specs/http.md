# HTTP Pipeline Specification / HTTP 管线规格

## Scope / 范围

This spec covers application-owned HTTP requests. It does not replace reqwest and does not govern network calls hidden inside third-party libraries such as `self_update` release discovery.

本文覆盖应用直接发起的 HTTP 请求。它不替代 reqwest，也不约束 `self_update` 等第三方库内部隐藏的网络调用。

## Canonical implementation / 主实现

- `src/http/mod.rs`
- Initialization: `setup_http` in `src/lib.rs`, after Toast and Notification setup.
- Async execution: shared Tokio runtime from `src/app_runtime.rs`.

## Response budgets / 响应预算

`RequestOptions::response_byte_limit` defaults to no total byte cap. Opted-in
requests count actual chunks before returning them or extending any body buffer;
Content-Length is not trusted as the enforcement boundary. Zero permits only an
empty body. Exceeding the limit returns non-transient `ResponseTooLarge`, never a
partial successful body; repeated reads retain the error and report it once.
After header inspection, `HttpResponse::response_byte_limit` may only tighten the
limit and still counts bytes read earlier. Idle/total deadlines and silent feedback
remain independent. Body failures do not restart a successful-header request.

`RequestOptions::response_byte_limit` 默认不设总字节限制；显式开启时，在返回 chunk
或扩展正文缓冲前累计真实字节，不依赖 Content-Length 执行限制。零只允许空正文。
超限返回非瞬态 `ResponseTooLarge`，不能返回成功的部分正文；继续读取仍失败，
且只反馈一次。header 后设置的 `HttpResponse::response_byte_limit` 只能收紧，
此前读过的字节仍计入。空闲/总截止时间与静默反馈策略独立；成功 header 后的
正文失败不会重新发起请求。

| Caller / 调用方 | Payload limit / 正文限制 |
| --- | --- |
| Remote Logo / 远程 Logo | 8 MiB, shared with local source input / 与本地源输入相同 |
| AI model list and buffered completion, including stream-to-JSON fallback / AI 模型列表、缓冲完成及 JSON 回退 | 16 MiB |
| GitHub/GitLab/Gitee public and enterprise ordinary metadata JSON / 普通元数据 JSON | 16 MiB per consumed response / 每个实际读取的响应 |
| Provider PR files, diff and commit-comparison JSON / PR 文件、diff 与 commit 比较 JSON | 64 MiB per consumed response / 每个实际读取的响应 |
| GitLab raw repository source / 原始源码 | 8 MiB |
| GitHub/Gitee source JSON / 源码 JSON | 12 MiB envelope; decoded bytes <=8 MiB / 包装 12 MiB，解码 <=8 MiB |

Provider limits are a finite accepted-input contract, not a promise to accept
all provider-legal PRs. `src/git/response_budget.rs` owns the ordinary metadata
and diff constants; repository source and conflict reads retain their separate
limits. They do not bound all pages in one operation or normalized/cache memory.
A headers-only or ignored-success-body caller does not begin reading that body
merely because its request declares a limit. Mutation replies keep their existing
single-attempt and response-consumption behavior. Essential list/detail/diff
failures stay errors; optional metadata probes retain their existing fallback.
The PR large-diff explicit-load gate remains a post-download highlighting policy
and does not bypass the transport limit.

Provider 限制是有限输入契约，不承诺接受所有 Provider 合法 PR。
`src/git/response_budget.rs` 统一定义普通元数据与 diff 常量；源码与冲突读取
保留独立上限。每响应限制不等于整次分页或归一化/缓存内存预算。只读 header
或忽略成功正文的调用不会因声明预算而开始读取正文；变更请求仍保持原有单次
尝试和正文消费方式。关键列表/详情/diff 超限保持错误，选填元数据探测保留
现有回退。大 diff 显式加载仍是下载后的高亮策略，不绕过传输限制。

SSE and update downloads retain their streaming policies. These limits do not cap
transport-internal buffers, JSON objects, lossy UTF-8 expansion or process RSS.
Non-2xx excerpts collect at most 8,000 bytes before the existing 2,000-character
truncation; the final received transport chunk may be larger than that prefix.
No response content is included in budget errors. Existing semantic error copy is
reused; background feedback remains silent.

SSE 与更新包沿用流式策略；这些限制不涵盖传输内部缓冲、JSON 对象、UTF-8 有损
转换膨胀或进程 RSS。非 2xx 摘要最多收集 8,000 字节，再沿用 2,000 字符截断；
最后收到的传输 chunk 可以大于前缀。预算错误不携带正文，复用既有语义错误文案，
后台仍静默。

Regression: exact/zero limits, missing length and split chunks, all body readers,
sticky single-report failure, header-based tightening, idle timeout, JSON fallback,
SSE success, bounded error excerpts, Base64 padding/UTF-8, and successful small
downloads after a rejected oversized response.
回归：精确/零边界、缺少长度与分块、全部读取接口、连续失败只反馈一次、header 后
收紧、idle 超时、JSON 回退、SSE 成功、错误摘要、Base64 padding/UTF-8，以及超限后
的小文件成功请求。

## Invariants / 不变量

1. Application code must not construct a reusable reqwest client outside `src/http/mod.rs`.
2. Every application-owned call uses the single module-owned async client with
   one user agent and a 10-second connect timeout. Settings General owns the
   application default timeout in decimal milliseconds (30,000 initially).
   Each new `RequestOptions` reads the latest committed value for both its
   total operation budget and body-idle budget; requests already created retain
   their snapshot. Zero disables both default deadlines, matching
   [Axios's `timeout: 0` behavior](https://github.com/axios/axios-docs/blob/master/posts/en/req_config.md).
   Explicit `RequestOptions.timeout` and `RequestOptions.read_idle_timeout`
   overrides remain independent. The idle budget limits the interval without
   body progress and may independently be disabled by an explicit zero.
   Arbitrarily large validated decimal values remain accepted in Settings;
   conversion saturates at `Duration::MAX`, and budgets beyond the platform's
   representable `Instant` range are effectively unbounded without overflowing
   or introducing a shorter substitute deadline. The fixed connect timeout
   and feature-owned protocol limits remain independent of the global setting.
   Every received transport chunk resets the body-idle deadline for buffered
   and streaming callers. Protocol-specific wall-clock limits and heartbeat
   semantics remain the caller's responsibility and never depend on provider
   host or model name.
   The General input restores `30000` when an empty edit loses focus. Save
   performs the same empty-value normalization and validates the live draft
   before submitting; blur alone does not persist. Zero and other nonempty
   values remain unchanged, and invalid nonempty input prevents submission.
3. Every call has a non-empty operation label.
4. Non-2xx responses fail by default and include only a bounded response-body excerpt.
5. Request, response, and error interceptors execute in registration order.
6. Request interceptors may mutate headers. They must not block or access Slint directly.
7. UI feedback is selected per request and defaults to silent.
8. Background probes, prefetch, retry, and provider fallback requests remain silent.
9. Secrets and authorization headers are never logged.
10. `send` returns the project-owned `HttpResponse`. JSON, text, bytes, error
    excerpts, and streaming chunks must all be read through it so body-idle
    timeout and normalized errors cannot be bypassed. Streaming callers retain
    ownership of progress and cancellation.
11. Detailed transfer diagnostics are request-scoped and opt-in. They may record
    body sizes, safe response-framing headers, chunk timing, and whitespace/progress
    classification, but never request/response content or authorization data.
12. GET and HEAD use the standard transient retry policy by default; callers
    may explicitly opt out when they own retry timing. POST and other writes
    remain single-attempt unless a caller supplies a custom safe policy. The
    standard policy makes at most three attempts, retries connect and timeout
    failures plus 429 and 5xx responses, and uses capped exponential backoff
    with jitter. Intermediate failures remain silent; only the terminal result
    enters normal error feedback.
    Only an attempt with a clonable later retry is capped to 10 seconds; the
    final attempt receives the complete remaining operation budget. Streaming
    downloads that continue reading a successful response for longer than that
    cap explicitly disable retry and retain their feature-owned deadline.
13. All attempts, per-attempt timeouts, and retry sleeps share the one
    `RequestOptions.timeout` operation budget. Retry delay honors numeric or
    HTTP-date `Retry-After` and GitHub-style `X-RateLimit-Reset` without
    shortening the server window. Exhausted-primary-quota 403 responses retry
    only when the advertised wait fits the remaining operation budget.
14. Provider fan-out must be bounded. Independent fixed reads may use
    `join!`/`try_join!`; item hydration uses bounded unordered streams. GitLab
    Closed pagination incrementally caches complete closed/merged prefixes for
    30 seconds, at most 32 filter keys, and invalidates the project after merge.

1. 应用代码不得在 `src/http/mod.rs` 外创建可复用 reqwest client。
2. 所有应用自有请求使用模块内唯一的异步 client，并统一 User-Agent 与 10 秒连接超时。
   General 设置持有应用默认超时，以十进制毫秒保存，初值为 30,000。每次创建
   `RequestOptions` 都读取最新已提交设置，同时作为总操作预算与正文空闲预算；
   已创建的请求保留自己的快照。零同时关闭这两个默认截止时间，遵循
   [Axios 的 `timeout: 0` 行为](https://github.com/axios/axios-docs/blob/master/posts/en/req_config.md)。
   显式 `RequestOptions.timeout` 与 `RequestOptions.read_idle_timeout` 覆盖仍相互独立；
   空闲预算限制未产生正文进度的等待，可通过显式零单独关闭。设置接受经过校验的任意
   大十进制值；转换最多饱和到 `Duration::MAX`，超出平台 `Instant` 可表示范围的预算
   视为实际上不可到达，不溢出，也不替换成更短的截止时间。固定连接超时及 Feature 自有
   协议上限仍独立于全局设置。缓冲与流式调用中收到的每个传输
   chunk 都会重置正文空闲截止时间；协议级墙钟上限与心跳语义仍由调用方负责，且禁止按
   Provider host 或模型名称选择行为。
   General 输入框空值失焦时回填 `30000`。Save 使用同一空值归一化并校验即时草稿
   后再提交；单纯失焦不持久化。0 和其他非空值保持原样，非法非空值阻止提交。
3. 每个请求必须提供非空 operation 名称。
4. 非 2xx 默认失败，响应正文只保留受限摘要。
5. Request/Response/Error interceptor 按注册顺序执行。
6. Request interceptor 可修改 header，但不得阻塞或直接访问 Slint。
7. UI 反馈按请求选择，默认静默。
8. 后台探测、预取、重试和 Provider fallback 必须静默。
9. 禁止记录 secret 和 authorization header。
10. `send` 返回项目自有 `HttpResponse`；JSON、文本、字节、错误摘要和流式 chunk 都必须
    通过它读取，避免绕过响应体空闲超时和统一错误。流式调用方仍自行管理进度和取消。
11. 详细传输诊断必须按请求显式启用；可以记录 body 大小、安全的响应 framing header、
    chunk 时序及空白/进度分类，但绝不记录请求/响应内容或鉴权数据。
12. GET/HEAD 默认使用标准瞬态重试策略；由调用方自行管理重试节奏时可以显式关闭。
    POST 和其他写请求仍保持单次尝试，除非调用方提供确认可安全重复的自定义策略。
    标准策略最多尝试三次，重试连接/超时错误、429 与 5xx，并使用带 jitter 的有上限
    指数退避；中间失败保持静默，只有终态进入统一错误反馈。
    只有仍存在可克隆后续重试的 attempt 才限制为 10 秒；末次 attempt 使用完整剩余操作
    预算。成功响应后还会持续读取正文的流式下载必须显式关闭 retry，并保留 Feature 自有
    截止时间。
13. 所有尝试、单次尝试超时与重试等待共享同一个 `RequestOptions.timeout` 操作预算。
    重试延迟识别数字或 HTTP-date 形式的 `Retry-After` 以及 GitHub 风格
    `X-RateLimit-Reset`，不得擅自缩短服务端等待窗口。主配额耗尽的 403 只有在服务端
    给出的等待可放入剩余操作预算时才重试。
14. Provider fan-out 必须有界。固定且互相独立的读取可用 `join!`/`try_join!`；逐项补全
    使用限并发 unordered stream。GitLab Closed 分页对完整 closed/merged 前缀做 30 秒、
    最多 32 个筛选 key 的增量缓存，并在合并成功后使该项目缓存失效。

## Feedback policy / 反馈策略

| Policy | Intended use |
| --- | --- |
| `Silent` | Polling, network probes, prefetch, retries, existing feature-owned error UI. |
| `Toast` | Short user-triggered operation where immediate transient feedback is sufficient. |
| `Notification` | Important failure that should remain visible longer or join the notification queue. |
| `ToastAndNotification` | Rare critical user action; avoid using it as a default. |

Do not emit HTTP feedback when a feature already renders the same error. Chat uses an inline assistant message; updater owns its notification flow.

当 feature 已展示同一错误时，不要再由 HTTP 层提示。Chat 使用内联 assistant 错误消息；Updater 自己管理 Notification。

## Interceptor contract / 拦截器契约

```rust
impl HttpInterceptor for MyInterceptor {
    fn on_request(&self, context: &mut RequestContext<'_>) -> HttpResult<()> {
        context.headers.insert("x-example", "value".parse().unwrap());
        Ok(())
    }

    fn on_response(&self, context: &ResponseContext<'_>) {
        // Observe status, headers, and elapsed time.
    }

    fn on_error(&self, error: &HttpError) {
        // Metrics/logging only; UI feedback is handled by RequestOptions.
    }
}
```

Interceptors must be thread-safe, fast, and deterministic. Authentication interceptors should scope credentials by host and must never attach one provider's token to another host.

Interceptor 必须线程安全、快速、确定性。鉴权拦截器必须按 host 限定 Token，禁止把某个平台凭据附加到其他 host。

## Error model / 错误模型

`HttpErrorKind` distinguishes build, timeout, connect, request, status, decode, and interceptor errors. Callers should match the kind/status when behavior differs; user messages must come from the normalized error rather than fragile substring matching.

Response-body reading may fail or stop making progress after successful headers.
Every buffered body format uses the same chunk loop, and each received transport
chunk resets the idle timer. An idle interval becomes `Timeout`, transport
errors retain reqwest's classification, and only invalid buffered JSON becomes
`Decode`. A stalled non-2xx error body follows the same rule instead of hiding
the timeout. Streaming callers use the same transport progress semantics and
own higher-level protocol heartbeats and wall-clock limits.

`HttpErrorKind` 区分构建、超时、连接、请求、状态码、解码和拦截器错误。业务分支应匹配 kind/status，不应继续依赖错误字符串包含关系。

响应头成功后，读取 body 仍可能失败或停止前进。所有缓冲响应格式使用同一 chunk 循环，
收到的每个传输 chunk 都会重置空闲计时。空闲超限归类为 `Timeout`，transport 错误保留
reqwest 分类，只有缓冲后的 JSON 内容无效才归类为 `Decode`。非 2xx 的错误响应体卡住时
也遵循同一规则；流式调用使用相同的传输进度语义，并自行负责更高层协议心跳和墙钟上限。

## Migration status / 迁移状态

The following direct calls are migrated:

- Chat JSON requests.
- Connectivity probes.
- Streamed update asset downloads.
- Remote project-logo downloads.
- GitHub/GitLab/Gitee PR/MR statistics.

Search guard:

```sh
rg -n "reqwest::Client::new|reqwest::Client::builder|reqwest::blocking|\.send\(\)" src --glob '*.rs'
```

Only the shared HTTP module should contain client creation or raw execution.

## Regression checklist / 回归检查

- A successful JSON request decodes normally.
- A 401/403 maps to a credential/permission message.
- A 429 maps to rate-limit feedback.
- GET/HEAD transient requests retry at most three times by default and support
  explicit opt-out; POST and other writes remain single-attempt unless a caller
  explicitly supplies a custom policy.
- Retryable non-final attempts use the 10-second cap, while the final or
  unclonable attempt uses the complete remaining operation budget. Update,
  avatar, and other long streaming downloads opt out of retries.
- Numeric/date `Retry-After`, provider reset instants, timeout/connect failures,
  5xx, 429, and exhausted-rate-limit 403 produce decisions bounded by one total
  operation deadline.
- GitLab Closed page N reuses the cached closed/merged prefixes from earlier
  pages and extends only missing provider pages; expiry and merge invalidate it.
- A 5xx response includes status but does not expose an unbounded body.
- Timeout and connect failures remain distinct.
- The General timeout defaults to 30,000 ms; changing it applies to the total
  and idle budgets of newly created requests. Existing requests retain their
  snapshot, and feature overrides continue to take precedence independently.
  Zero disables both default deadlines without removing the 10-second connect
  timeout. Values above the platform clock's range cannot overflow or produce
  a shorter substitute deadline.
- Body-idle timeout uses every received transport chunk as progress, supports
  custom values, and treats an explicit zero as disabled.
- Buffered and streaming responses share the same any-chunk idle semantics;
  protocol-specific wall-clock limits remain caller-owned.
- A stalled JSON, text, bytes, streamed, or non-2xx error body becomes `Timeout`
  instead of waiting forever or being overwritten as `Decode`/`Status`.
- Silent background calls do not create Toast/Notification.
- User feedback invoked from a worker thread is scheduled onto the Slint event loop.
- Update streaming still reports progress and supports cancellation.
- Provider fallback does not show an error for the first failed endpoint when the fallback succeeds.
