# HTTP Pipeline Specification / HTTP 管线规格

## Scope / 范围

This spec covers application-owned HTTP requests. It does not replace reqwest and does not govern network calls hidden inside third-party libraries such as `self_update` release discovery.

本文覆盖应用直接发起的 HTTP 请求。它不替代 reqwest，也不约束 `self_update` 等第三方库内部隐藏的网络调用。

## Canonical implementation / 主实现

- `src/http/mod.rs`
- Initialization: `setup_http` in `src/lib.rs`, after Toast and Notification setup.
- Async execution: shared Tokio runtime from `src/app_runtime.rs`.

## Invariants / 不变量

1. Application code must not construct a reusable reqwest client outside `src/http/mod.rs`.
2. Every application-owned call uses the single module-owned async client with
   one user agent and a 10-second connect timeout. `RequestOptions.timeout` owns the total
   request deadline: 30 seconds by default, any custom duration when supplied,
   and no total deadline when set to zero. `RequestOptions.read_idle_timeout`
   independently limits the interval without body progress: 30 seconds by
   default, any custom duration when supplied, and disabled when set to zero.
   The request-scoped progress policy defaults to any received chunk. Buffered
   JSON callers may require non-whitespace bytes, so blank transport heartbeats
   do not keep a request alive. Policies describe response semantics and never
   select behavior by provider host or model name.
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
12. Transient retry is opt-in per request and defaults to GET/HEAD only. The
    standard provider policy makes at most three attempts, retries connect and
    timeout failures plus 429, 5xx, and rate-limit-exhausted 403 responses, and
    uses capped exponential backoff with jitter. Intermediate failures remain
    silent; only the terminal result enters normal error feedback.
13. Retry delay honors numeric or HTTP-date `Retry-After` and GitHub-style
    `X-RateLimit-Reset`, capped at 30 seconds. Rate-limit headers are diagnostic
    signals only and must never expose credentials or unbounded response data.
14. Provider fan-out must be bounded. Independent fixed reads may use
    `join!`/`try_join!`; item hydration uses bounded unordered streams. GitLab
    Closed pagination incrementally caches complete closed/merged prefixes for
    30 seconds, at most 32 filter keys, and invalidates the project after merge.

1. 应用代码不得在 `src/http/mod.rs` 外创建可复用 reqwest client。
2. 所有应用自有请求使用模块内唯一的异步 client，并统一 User-Agent 与 10 秒连接超时。
   总请求截止时间由 `RequestOptions.timeout` 管理：默认 30 秒，可传入任意自定义时长，
   传零则不设置总截止时间。`RequestOptions.read_idle_timeout` 独立限制未产生正文进度的
   等待：默认 30 秒，可自定义，传零则关闭空闲检测。请求级进度策略默认把任意 chunk
   视为进度；缓冲 JSON 调用方可要求非空白字节，避免空白传输心跳让请求续期。策略只描述
   响应语义，禁止按 Provider host 或模型名称选择行为。
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
12. 瞬态重试必须由请求显式启用，默认只允许 GET/HEAD。Provider 标准策略最多尝试三次，
    重试连接/超时错误、429、5xx 及 rate-limit 已耗尽的 403，并使用带 jitter 的有上限
    指数退避；中间失败保持静默，只有终态进入统一错误反馈。
13. 重试延迟识别数字或 HTTP-date 形式的 `Retry-After` 以及 GitHub 风格
    `X-RateLimit-Reset`，上限为 30 秒。Rate-limit header 只作诊断信号，不得泄露凭据或
    无限正文。
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
Every buffered body format uses the same chunk loop and a request-scoped
progress policy. The default resets the idle timer after each chunk; callers
expecting JSON may require non-whitespace bytes. An idle interval becomes
`Timeout`, transport errors retain reqwest's classification, and only invalid
buffered JSON becomes `Decode`. A stalled non-2xx error body follows the same
rule instead of hiding the timeout. Streaming callers retain the default
chunk-progress semantics and own their higher-level protocol heartbeats.

`HttpErrorKind` 区分构建、超时、连接、请求、状态码、解码和拦截器错误。业务分支应匹配 kind/status，不应继续依赖错误字符串包含关系。

响应头成功后，读取 body 仍可能失败或停止前进。所有缓冲响应格式使用同一 chunk 循环和
请求级进度策略：默认每个 chunk 后重置空闲计时，预期 JSON 的调用方可要求非空白字节。
空闲超限归类为 `Timeout`，transport 错误保留 reqwest 分类，只有缓冲后的 JSON 内容无效
才归类为 `Decode`。非 2xx 的错误响应体卡住时也遵循同一规则；流式调用保留默认 chunk
进度语义，并自行负责更高层协议心跳。

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
- Opted-in GET/HEAD transient requests retry at most three times; POST and other
  writes remain single-attempt unless a caller explicitly supplies a custom policy.
- Numeric/date `Retry-After`, provider reset instants, timeout/connect failures,
  5xx, 429, and exhausted-rate-limit 403 produce bounded retry decisions.
- GitLab Closed page N reuses the cached closed/merged prefixes from earlier
  pages and extends only missing provider pages; expiry and merge invalidate it.
- A 5xx response includes status but does not expose an unbounded body.
- Timeout and connect failures remain distinct.
- Request timeout configuration maps the 30-second default and custom durations
  onto the reqwest request; zero maps to no total deadline without removing the
  connect timeout.
- Body-idle timeout configuration defaults to 30 seconds, uses any received
  chunk as progress by default, supports a request-scoped non-whitespace mode,
  supports custom values, and treats zero as disabled.
- Whitespace-only heartbeats cannot extend a non-whitespace progress deadline;
  ordinary and streaming requests retain the default any-chunk behavior.
- A stalled JSON, text, bytes, streamed, or non-2xx error body becomes `Timeout`
  instead of waiting forever or being overwritten as `Decode`/`Status`.
- Silent background calls do not create Toast/Notification.
- User feedback invoked from a worker thread is scheduled onto the Slint event loop.
- Update streaming still reports progress and supports cancellation.
- Provider fallback does not show an error for the first failed endpoint when the fallback succeeds.
