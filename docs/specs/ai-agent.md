# AI Agent Configuration / AI Agent 配置

## Scope / 范围

This spec covers the Settings-page connection to an OpenAI-compatible AI Agent provider, including DeepSeek. It governs local persistence and connectivity checks only; the existing RAG chat transport is outside this module.

本文覆盖设置页中 OpenAI-compatible（包括 DeepSeek）AI Agent 服务的连接配置，以及本地持久化和连通性测试；既有 RAG Chat 请求不属于本模块。

## Configuration / 配置项

The minimum usable configuration has three values:

1. **OpenAI Base URL** — provider origin, with or without `/v1`.
2. **OpenAI API Key** — sent only as a Bearer authorization header.
3. **Model Name** — the exact model identifier to request.

最小可用配置有三项：服务地址（可带或不带 `/v1`）、API Key、模型名称。仅有前两项只能定位并认证服务，不能确定实际调用哪个模型。

Values are stored in the local platform `settings.toml` under `[ai.agent]`, including the selected provider preset, then loaded during startup and mirrored to `AppState` for the Settings view. Keys must never be logged or committed.

配置（包括选中的 Provider 预设）保存到本机平台目录下 `settings.toml` 的 `[ai.agent]`，启动时读取并同步到 `AppState`。API Key 不得写入日志或提交到仓库。

## Request behavior / 请求行为

- `src/ai/agent.rs` owns the AI Agent provider connection and appends `/models` to the configured provider URL (which may already include `/v1`).
- The existing RAG Chat endpoint and `src/chat/chat.rs` are unchanged. A future AI Agent feature must use this configuration explicitly rather than repurposing RAG Chat.
- The Settings-page test uses `/models` through the same shared HTTP module. It verifies URL reachability and credentials without creating a billable completion. A provider may not list every model; this is reported as a non-fatal result.
- The user-triggered connection test owns one bottom Toast for every terminal result. Request feedback stays silent in the shared HTTP layer so failures are not duplicated.

- `src/ai/agent.rs` 负责 AI Agent Provider 的连接，在配置的服务地址后追加 `/models`（地址可自行包含 `/v1`）。
- 既有 RAG Chat endpoint 和 `src/chat/chat.rs` 保持不变；未来 AI Agent 功能必须显式使用本配置，不能复用或替换 RAG Chat。
- 设置页测试通过同一 HTTP 模块调用 `/models`，验证地址可达和凭据有效，不产生生成费用。Provider 不列出全部模型时只作提示，不视为连接失败。
- Chat 的失败仍显示为内联 assistant 消息；用户主动测试时由 AI Agent 功能层对成功或失败统一显示一个底部 Toast，共享 HTTP 层保持静默以避免重复提示。

## Invariants / 不变量

1. No AI request may construct a reqwest client or bypass `src/http/mod.rs`.
2. API keys must only be attached to the configured provider test request and must not appear in logs, Toasts, notifications, or error bodies.
3. Slint state is updated only on the event loop; the connection test runs on the shared Tokio runtime.
4. A missing URL, key, or model is a local validation error and must not create a network request.

1. AI 请求不得创建 reqwest client，也不得绕过 `src/http/mod.rs`。
2. API Key 仅附加到配置的 Provider 请求，不得出现在日志、Toast、通知或错误正文中。
3. Slint 状态只在 event loop 更新；连通性测试运行在共享 Tokio runtime。
4. URL、Key、模型任一缺失时为本地校验错误，不得发起网络请求。

## Regression checklist / 回归检查

- Save a DeepSeek preset, restart the app, and verify all three fields are restored (the key remains masked).
- Test a valid provider/key and confirm a bottom success Toast; test invalid credentials and confirm one bottom error Toast.
- Verify a RAG Chat message still reaches the original RAG endpoint after AI Agent settings are saved.
- Verify no logs contain the API key and no new direct reqwest client exists outside `src/http/mod.rs`.
