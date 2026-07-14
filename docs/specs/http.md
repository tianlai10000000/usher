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
2. Both async and blocking calls use shared clients with consistent user agent, connect timeout, and default timeout.
3. Every call has a non-empty operation label.
4. Non-2xx responses fail by default and include only a bounded response-body excerpt.
5. Request, response, and error interceptors execute in registration order.
6. Request interceptors may mutate headers. They must not block or access Slint directly.
7. UI feedback is selected per request and defaults to silent.
8. Background probes, prefetch, retry, and provider fallback requests remain silent.
9. Secrets and authorization headers are never logged.
10. Streaming callers may set a longer per-request timeout and retain ownership of progress/cancellation.

1. 应用代码不得在 `src/http/mod.rs` 外创建可复用 reqwest client。
2. 异步和阻塞请求使用共享 client，并统一 User-Agent、连接超时和默认超时。
3. 每个请求必须提供非空 operation 名称。
4. 非 2xx 默认失败，响应正文只保留受限摘要。
5. Request/Response/Error interceptor 按注册顺序执行。
6. Request interceptor 可修改 header，但不得阻塞或直接访问 Slint。
7. UI 反馈按请求选择，默认静默。
8. 后台探测、预取、重试和 Provider fallback 必须静默。
9. 禁止记录 secret 和 authorization header。
10. 流式请求可设置更长的单次超时，并自行管理进度和取消。

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

`HttpErrorKind` 区分构建、超时、连接、请求、状态码、解码和拦截器错误。业务分支应匹配 kind/status，不应继续依赖错误字符串包含关系。

## Migration status / 迁移状态

The following direct calls are migrated:

- Chat JSON requests.
- Connectivity probes.
- Streamed update asset downloads.
- Remote project-logo downloads.
- GitHub/GitLab/Gitee PR/MR statistics.

Search guard:

```sh
rg -n "reqwest::Client::new|reqwest::blocking::get|\.send\(\)" src --glob '*.rs'
```

Only the shared HTTP module should contain client creation or raw execution.

## Regression checklist / 回归检查

- A successful JSON request decodes normally.
- A 401/403 maps to a credential/permission message.
- A 429 maps to rate-limit feedback.
- A 5xx response includes status but does not expose an unbounded body.
- Timeout and connect failures remain distinct.
- Silent background calls do not create Toast/Notification.
- User feedback invoked from a worker thread is scheduled onto the Slint event loop.
- Update streaming still reports progress and supports cancellation.
- Provider fallback does not show an error for the first failed endpoint when the fallback succeeds.
