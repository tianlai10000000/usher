# AI Agent Configuration / AI Agent 配置

## Scope / 范围

This spec covers the Settings-page connection to an OpenAI-compatible AI Agent provider, including DeepSeek. It governs local persistence, connectivity checks, and the shared completion transport used by AI Chat and AI Code Review. The RAG transport remains separate.

本文覆盖设置页中 OpenAI-compatible（包括 DeepSeek）AI Agent 服务的连接配置、本地持久化、连通性测试，以及 AI Chat 与 AI Code Review 共用的 completion transport；RAG transport 保持独立。

## Configuration / 配置项

The minimum usable connection configuration has three values:

1. **OpenAI Base URL** — provider origin, with or without `/v1`.
2. **OpenAI API Key** — sent only as a Bearer authorization header.
3. **Model Name** — the exact model identifier to request.

最小可用连接配置有三项：服务地址（可带或不带 `/v1`）、API Key、模型名称。仅有前两项只能定位并认证服务，不能确定实际调用哪个模型。

The same section stores `reply_language = "auto" | <locale-id>`. `auto` resolves to the active interface locale when each request is built; an explicit locale remains independent from later interface-language changes. It applies to AI Chat, AI Code Review, and Review Ask, but never to RAG. Unknown legacy values normalize to `auto`.

同一区块保存 `reply_language = "auto" | <locale-id>`。`auto` 在每次构建请求时解析为当前界面 locale；显式 locale 不随之后的界面语言切换而改变。该设置应用于 AI Chat、AI Code Review 和 Review Ask，不影响 RAG；未知旧值统一归一化为 `auto`。

The five AI Agent fields form one page-local draft while Settings is open. Selecting DeepSeek or OpenAI updates Provider, Base URL, and Model atomically in that draft. Only Save dispatches the complete draft as one typed Store Action; the changed commit is automatically persisted on the Store worker and Save waits for its flush before reporting success. The Settings factory product stays mounted after its first visit to avoid Slint 1.17.1 remount corruption. On every re-entry, the bridge overwrites its draft from the last committed Store snapshot before paint, so leaving without Save still discards every edit and the selected segment cannot diverge from the displayed fields.

Settings 打开期间，这五项 AI Agent 字段构成一份页面局部草稿。选择 DeepSeek 或 OpenAI 时，Provider、Base URL 与 Model 必须在草稿中原子更新。只有点击 Save 才把完整草稿作为一个强类型 Store Action 提交；有效 commit 由 Store worker 自动持久化，Save 等待 flush 后再提示成功。Settings factory 产品首次访问后保持挂载，以规避 Slint 1.17.1 重挂载损坏；每次重新进入时，bridge 都在绘制前用最近提交的 Store 快照覆盖草稿，因此未点击 Save 的所有编辑仍会丢弃，选中按钮与显示字段也不得出现分裂。

配置（包括选中的 Provider 预设）保存到本机平台目录下 `settings.toml` 的 `[ai.agent]`，启动时读取并同步到 `AppState`。API Key 不得写入日志或提交到仓库。

## Prompt catalog and context / Prompt catalog 与上下文

`src/ai/prompts.toml` is the sole source of built-in AI Agent instructions. Every entry has a stable `id` and numeric `version`; `src/ai/prompts.rs` parses the embedded catalog once, renders it with MiniJinja strict-undefined behavior, assembles typed messages, and applies bounds. Do not add built-in system/user prompt literals at call sites.

`src/ai/prompts.toml` 是所有内置 AI Agent 指令的唯一来源。每项都有稳定 `id` 和数字 `version`；`src/ai/prompts.rs` 只解析一次内嵌 catalog，以 MiniJinja strict-undefined 模式渲染，组装强类型消息并执行边界限制。调用点不得再增加内置 system/user Prompt 字面量。

| Operation / 操作 | Prompt id and message order / Prompt id 与消息顺序 | Dynamic context sent to the model / 发送给模型的动态上下文 |
| --- | --- | --- |
| AI Chat | `general_chat`: system, then the latest 20 AI Chat history messages | User/assistant message content only. No RAG context. / 仅用户与 assistant 消息正文；无 RAG 上下文。 |
| AI Code Review | `review_chunk`: system, then one rendered user context per diff chunk | Project name; PR number/title; source/target branch; base/head SHA; selected rule names and contents; one-off requirements; current diff chunk and chunk index/count. / 项目名、PR 编号/标题、源/目标 Branch、base/head SHA、所选规则名称与正文、一次性要求、当前 diff chunk 及序号/总数。 |
| Review Ask | `review_conversation`: system, one rendered PR-context user message, then the latest 20 review conversation messages | The same approved PR metadata, a UTF-8-safe diff excerpt capped at 48 KiB, optional completed review JSON, and conversation content. / 同一组获准 PR 元数据、UTF-8 安全且最多 48 KiB 的 diff 摘要、可选已完成 Review JSON 与对话正文。 |

Local absolute repository/rule paths, remote repository/provider URLs, PR author, PR update time, API credentials, and model connection settings are never prompt context. Repository metadata, rule text, diffs, and conversation content are explicitly treated as untrusted data in the system contract.

本地仓库/规则绝对路径、远端仓库/Provider URL、PR 作者、PR 更新时间、API 凭据和模型连接设置绝不作为 Prompt 上下文。System contract 明确把仓库元数据、规则正文、diff 与对话内容视为不可信数据。

Every system prompt receives exactly one rendered language fragment: `请用<语言原生名称>回复结果。` General replies preserve code, identifiers, and file paths; structured review replies additionally keep JSON keys, `severity` values, paths, and code identifiers unchanged.

每条 system Prompt 都恰好注入一次语言片段：`请用<语言原生名称>回复结果。` 普通回复保留代码、标识符和文件路径；结构化 Review 回复还必须保持 JSON key、`severity` 值、路径和代码标识符不变。

Diagnostics log metadata only: operation, prompt id/version, effective locale, message count, dynamic-context field count/bytes, and truncation count. Completed Review Sessions persist aggregate prompt provenance and request count; Review Ask assistant messages persist per-request provenance. Neither path logs or duplicates raw prompt, source, rules, history, credentials, or response content.

诊断只记录元数据：操作、Prompt id/version、实际 locale、消息数、动态上下文字段数/字节数及截断数。已完成 Review Session 持久化聚合后的 Prompt 来源和请求数；Review Ask assistant 消息持久化单次请求来源。两条路径都不记录或复制原始 Prompt、源码、规则、历史、凭据或回复正文。

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
6. Ordinary completions keep the shared 30-second total and body-idle timeouts. Explicitly cancellable AI Review and Review Ask completions use OpenAI-compatible SSE streaming through the same interceptor pipeline, disable only the HTTP request total deadline, retain the 10-second connect timeout, and use a 300-second response-body idle timeout. Review chunks and Review Ask conversations additionally have caller-owned 600-second wall-clock limits. An SSE response is successful only after `[DONE]` or a `finish_reason`; provider error events and cleanly truncated streams fail explicitly. A provider that answers a streaming request with one JSON response remains supported.
7. Prompt rendering must succeed before a provider request starts; invalid catalog/schema/template data becomes a bounded internal/configuration error.
8. AI Agent form edits remain page-local drafts. Save is the only operation that dispatches the complete settings as a typed Store Action. Persistence runs on the Store worker, never on the Slint event loop, and every Settings route re-entry restores one complete committed snapshot into the stable factory page before paint.

1. AI 请求不得创建 reqwest client，也不得绕过 `src/http/mod.rs`。
2. API Key 仅附加到配置的 Provider 请求，不得出现在日志、Toast、通知或错误正文中。
3. Slint 状态只在 event loop 更新；连通性测试运行在共享 Tokio runtime。
4. URL、Key、模型任一缺失时为本地校验错误，不得发起网络请求。
5. Completion 请求只接收 Feature 自有的有界消息列表；日志不得包含 API Key、完整 Prompt、源码 payload 或响应正文。
6. 普通 completion 使用共享的 30 秒总时限和响应体空闲时限；可显式取消的 AI Review 与 Review Ask completion 通过同一 interceptor 管线使用 OpenAI-compatible SSE streaming，只关闭 HTTP 请求总截止时间，仍保留 10 秒连接超时和 300 秒响应体空闲超时。Review chunk 与 Review Ask conversation 另有调用方管理的 600 秒墙钟上限。SSE 只有收到 `[DONE]` 或 `finish_reason` 才成功；Provider error event 与干净断流的截断响应必须显式失败。若 Provider 对 streaming 请求返回一次性 JSON，仍须兼容。
7. Provider 请求开始前必须成功渲染 Prompt；catalog/schema/template 无效时只暴露有界的内部/配置错误。
8. AI Agent 表单编辑必须保留为页面局部草稿；只有 Save 才把完整配置 dispatch 为强类型 Store Action。持久化只能运行在 Store worker，禁止在 Slint event loop 执行；每次重新进入 Settings 路由时，必须在绘制前向稳定 factory 页面恢复一份完整的已提交快照。

## Regression checklist / 回归检查

- Save a DeepSeek preset and reply language, restart the app, and verify all four values are restored (the key remains masked).
- Cycle `Custom -> DeepSeek -> OpenAI -> DeepSeek`. The selected segment, Base URL, and Model must always describe the same provider: DeepSeek uses `https://api.deepseek.com` / `deepseek-v4-flash`, while OpenAI uses `https://api.openai.com/v1` / `gpt-4o-mini`. Editing Base URL or Model must select Custom.
- Change each AI Agent field without saving, navigate away, and reopen Settings. The same factory product must remain mounted while hidden, and the last committed snapshot must be restored before it becomes visible without stale segment, placeholder, or field bindings. Save a complete draft and repeat; the saved draft must then be restored.
- Test a valid provider/key and confirm a bottom success Toast; test invalid credentials and confirm one bottom error Toast.
- Verify a RAG Chat message still reaches its configured RAG endpoint after AI Agent settings are saved.
- Switch Chat between AI Chat and RAG; verify AI uses the configured model and each mode retains a separate history.
- Select each reply-language option without saving and verify navigation away/back restores the committed choice. Save each choice, then navigate away/back and restart; verify the saved selection survives and AI Chat, AI Code Review, and Review Ask use it while RAG is unchanged. Verify `auto` follows a runtime interface-language switch.
- Inspect one completed Review Session and one Review Ask assistant message; verify provenance contains only prompt metadata/context sizes and no source or credentials.
- Verify no logs contain the API key and no new direct reqwest client exists outside `src/http/mod.rs`.

- 保存 DeepSeek 预设与回复语言并重启应用，确认四项值都被恢复，API Key 仍保持遮罩。
- 按 `Custom -> DeepSeek -> OpenAI -> DeepSeek` 循环切换；选中项、Base URL 和 Model 必须始终属于同一 Provider。DeepSeek 对应 `https://api.deepseek.com` / `deepseek-v4-flash`，OpenAI 对应 `https://api.openai.com/v1` / `gpt-4o-mini`；手工编辑 Base URL 或 Model 后必须切到 Custom。
- 不保存地修改每个 AI Agent 字段，离开再重新进入 Settings。隐藏期间必须保留同一个 factory 产品，并在再次显示前恢复最近提交的快照，不能残留旧选中态、placeholder 或字段绑定。保存完整草稿后再重复一次，此时必须恢复已保存草稿。
- 使用有效 Provider/Key 测试时确认底部只出现一个成功 Toast；无效凭据只出现一个错误 Toast。
- 保存 AI Agent 设置后，确认 RAG Chat 仍请求其独立配置的 RAG endpoint。
- 在 Chat 中切换 AI Chat 与 RAG，确认 AI 使用配置模型且两种模式保留独立历史。
- 不保存地逐个选择回复语言，确认离开再返回 Settings 后恢复已提交选项。保存各选项后再离开返回并重启，确认已保存选择在两种情况下都保留，AI Chat、AI Code Review 与 Review Ask 使用该语言而 RAG 不变；运行时切换界面语言后，确认 `auto` 跟随新语言。
- 检查一个已完成 Review Session 和一条 Review Ask assistant 消息，确认 provenance 只有 Prompt 元数据/上下文大小，不含源码或凭据。
- 确认日志不包含 API Key，并且 `src/http/mod.rs` 之外没有新增直接 reqwest client。
