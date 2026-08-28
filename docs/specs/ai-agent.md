# AI Agent Configuration / AI Agent 配置

## Scope / 范围

This spec covers the Settings-page connection to an OpenAI-compatible AI Agent provider, including DeepSeek. It governs local persistence, connectivity checks, and the shared completion transport used by AI Chat and AI Code Review. The RAG transport remains separate.

本文覆盖设置页中 OpenAI-compatible（包括 DeepSeek）AI Agent 服务的连接配置、本地持久化、连通性测试，以及 AI Chat 与 AI Code Review 共用的 completion transport；RAG transport 保持独立。

## Configuration / 配置项

The minimum usable configuration has three values:

1. **OpenAI Base URL** — provider origin, with or without `/v1`.
2. **OpenAI API Key** — sent only as a Bearer authorization header.
3. **Model Name** — the exact model identifier to request.

最小可用配置有三项：服务地址（可带或不带 `/v1`）、API Key、模型名称。仅有前两项只能定位并认证服务，不能确定实际调用哪个模型。

Values are stored in the local platform `settings.toml` under `[ai.agent]`, including the selected provider preset, then loaded during startup and mirrored to `AppState` for the Settings view. Keys must never be logged or committed.

配置（包括选中的 Provider 预设）保存到本机平台目录下 `settings.toml` 的 `[ai.agent]`，启动时读取并同步到 `AppState`。API Key 不得写入日志或提交到仓库。

## Request behavior / 请求行为

- `src/ai/agent.rs` owns the AI Agent provider connection, normalizes `/models` and `/chat/completions` against a configured URL that may already include `/v1`, and executes both through shared HTTP.
- AI Chat and AI Code Review explicitly load this configuration. RAG continues to use `src/chat/chat.rs` and its independently configured HTTPS endpoint.
- The Settings-page test uses `/models` through the same shared HTTP module. It verifies URL reachability and credentials without creating a billable completion. A provider may not list every model; this is reported as a non-fatal result.
- The user-triggered connection test owns one bottom Toast for every terminal result. Request feedback stays silent in the shared HTTP layer so failures are not duplicated.

- `src/ai/agent.rs` 负责 AI Agent Provider 连接，针对可包含 `/v1` 的配置地址规范化 `/models` 和 `/chat/completions`，并统一通过共享 HTTP 执行。
- AI Chat 和 AI Code Review 显式读取本配置；RAG 继续使用 `src/chat/chat.rs` 及其独立配置的 HTTPS endpoint。
- 设置页测试通过同一 HTTP 模块调用 `/models`，验证地址可达和凭据有效，不产生生成费用。Provider 不列出全部模型时只作提示，不视为连接失败。
- Chat 的失败仍显示为内联 assistant 消息；用户主动测试时由 AI Agent 功能层对成功或失败统一显示一个底部 Toast，共享 HTTP 层保持静默以避免重复提示。

## Invariants / 不变量

1. No AI request may construct a reqwest client or bypass `src/http/mod.rs`.
2. API keys must only be attached to the configured provider test request and must not appear in logs, Toasts, notifications, or error bodies.
3. Slint state is updated only on the event loop; the connection test runs on the shared Tokio runtime.
4. A missing URL, key, or model is a local validation error and must not create a network request.
5. Completion requests carry a bounded feature-owned message list. API keys, complete prompts, source payloads, and response bodies must not be logged.
6. Ordinary completions keep the shared 30-second total and body-idle timeouts. Explicitly cancellable AI Review and Review Ask completions disable only the total deadline; they retain the 10-second connect timeout and use a 90-second response-body idle timeout through the same interceptor pipeline.

1. AI 请求不得创建 reqwest client，也不得绕过 `src/http/mod.rs`。
2. API Key 仅附加到配置的 Provider 请求，不得出现在日志、Toast、通知或错误正文中。
3. Slint 状态只在 event loop 更新；连通性测试运行在共享 Tokio runtime。
4. URL、Key、模型任一缺失时为本地校验错误，不得发起网络请求。
5. Completion 请求只接收 Feature 自有的有界消息列表；日志不得包含 API Key、完整 Prompt、源码 payload 或响应正文。
6. 普通 completion 使用共享的 30 秒总时限和响应体空闲时限；可显式取消的 AI Review 与 Review Ask completion 只关闭总截止时间，仍保留 10 秒连接超时，并通过同一 interceptor 管线使用 90 秒响应体空闲超时。

## Regression checklist / 回归检查

- Save a DeepSeek preset, restart the app, and verify all three fields are restored (the key remains masked).
- Cycle `Custom -> DeepSeek -> OpenAI -> DeepSeek`. The selected segment, Base URL, and Model must always describe the same provider: DeepSeek uses `https://api.deepseek.com` / `deepseek-v4-flash`, while OpenAI uses `https://api.openai.com/v1` / `gpt-4o-mini`. Editing Base URL or Model must select Custom.
- Navigate away from Settings so its route-conditional factory product is destroyed, reopen it, and repeat the provider cycle. The saved preset must be restored without stale segment or field bindings; do not retain the hidden Settings tree to pass this check.
- Test a valid provider/key and confirm a bottom success Toast; test invalid credentials and confirm one bottom error Toast.
- Verify a RAG Chat message still reaches its configured RAG endpoint after AI Agent settings are saved.
- Switch Chat between AI Chat and RAG; verify AI uses the configured model and each mode retains a separate history.
- Verify no logs contain the API key and no new direct reqwest client exists outside `src/http/mod.rs`.

- 保存 DeepSeek 预设并重启应用，确认三个字段都被恢复，API Key 仍保持遮罩。
- 按 `Custom -> DeepSeek -> OpenAI -> DeepSeek` 循环切换；选中项、Base URL 和 Model 必须始终属于同一 Provider。DeepSeek 对应 `https://api.deepseek.com` / `deepseek-v4-flash`，OpenAI 对应 `https://api.openai.com/v1` / `gpt-4o-mini`；手工编辑 Base URL 或 Model 后必须切到 Custom。
- 离开 Settings 使按路由创建的 factory 产品销毁，重新进入后再次循环切换。已保存预设必须正确恢复，不能残留旧选中态或字段绑定；禁止通过常驻隐藏 Settings 树来通过此检查。
- 使用有效 Provider/Key 测试时确认底部只出现一个成功 Toast；无效凭据只出现一个错误 Toast。
- 保存 AI Agent 设置后，确认 RAG Chat 仍请求其独立配置的 RAG endpoint。
- 在 Chat 中切换 AI Chat 与 RAG，确认 AI 使用配置模型且两种模式保留独立历史。
- 确认日志不包含 API Key，并且 `src/http/mod.rs` 之外没有新增直接 reqwest client。
