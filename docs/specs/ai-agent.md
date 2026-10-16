# AI Agent Configuration / AI Agent 配置

## Scope / 范围

This spec covers named OpenAI-compatible AI Agent profiles, including DeepSeek, their independent persistence and active selection, request attribution, connectivity checks, and the shared completion transport used by AI Chat and AI Code Review. The RAG transport remains separate.

本文覆盖命名 OpenAI-compatible（包括 DeepSeek）AI Agent Profile、独立持久化与当前使用选择、请求身份、连通性测试，以及 AI Chat 与 AI Code Review 共用的 completion transport；RAG transport 保持独立。

## Ownership and iteration entry points / 职责与迭代入口

| Change / 修改内容 | Code and validation / 代码与验证 |
| --- | --- |
| Connection, JSON/SSE, budgets / 连接、JSON/SSE、预算 | `src/ai/agent.rs`; `cargo test --lib --locked ai::agent` |
| Named profiles and legacy migration / 命名 Profile 与旧配置迁移 | `src/settings/ai_agents.rs` (`AiAgentProfile`, `AiAgentConfiguration`), `src/settings.rs`; `cargo test --lib --locked settings::ai_agents` |
| Saved configuration, ordered Create/Update/Delete/Activate / 持久配置、有序 Create/Update/Delete/Activate | `src/ai/settings_state.rs` (`AiAgentSettingsFacade`), `src/ai/settings_state_tests.rs`; `cargo test --lib --locked ai::settings_state` |
| Editor commands, tests and feedback / 弹窗命令、测试与反馈 | `src/ai/settings_ui.rs`, `src/ai/settings_ui/connection_test.rs`, `src/ai/settings_ui/delete_confirmation.rs`, `src/ai/settings_ui_tests.rs`; `cargo test --lib --locked ai::settings_ui` |
| Saved-list connectivity / 已保存列表连通性 | `src/ai/settings_ui/{connection_probe,list_connection_test}.rs`, `list_connection_test_tests.rs`, `src/settings_page/agent_list_probe_ui_tests.rs`; shared transport capture/cancellation, single-click identities, list Toast and locale lifecycle |
| Header editor modes / Header 编辑模式 | `src/settings_page/ai_agent_header_object.rs`, `ai_agent_header_drafts.rs`, their adjacent tests, `ui/components/settings/github_ai_agent_headers.slint`; `cargo test --lib --locked settings_page::ai_agent_header` |
| Header controls and fixed Tooltip / Header 控件与固定 Tooltip | `src/settings_page/agent_header_controls_tests.rs`, `header_tooltip_tests.rs`, `ui/components/settings/github_ai_agent_header_tooltip.slint`; `cargo test --lib --locked settings_page::agent_header_controls_tests` and `settings_page::header_tooltip_tests`, plus native locale/scroll checks |
| Editor geometry and feedback / 编辑器几何与反馈 | `src/settings_page/agent_editor_layout_tests.rs`; `cargo test --lib --locked settings_page::agent_editor_layout_tests` plus native source/non-source locale checks |
| Shared Select placement / 共享 Select 定位 | `ui/components/input/github_select.slint`, `tests/select_placement_ui.rs`, `src/settings_page/agent_select_popup_tests.rs`; `cargo test --locked --test select_placement_ui` and `cargo test --lib --locked settings_page::agent_select_popup_tests`, plus native footer-adjacent menu paint |
| Clipped Select activation / 裁切 Select 激活 | `ui/components/settings/github_settings_controls.slint`, `tests/select_placement_ui.rs`, `src/settings_page/agent_select_popup_tests.rs`; first visible-area pointer click without scroll/reveal, top/bottom clipping and real-scroll dismissal |
| Settings drafts, factory and callbacks / 设置草稿、工厂与 callback | `src/settings_page.rs`, `src/settings_page/ai_agent_drafts.rs`, `src/settings_page/ai_agent_projection.rs`, `src/settings_page/ai_agent_list.rs`, `src/settings_page/provider_tabs_tests.rs`, `ui/components/settings/github_ai_agent_list.slint`, `ui/components/settings/github_ai_agent_editor.slint`, `ui/components/settings/github_ai_agent_delete.slint`, `ui/state/app_state.slint`, `ui/views/settings.slint`; `cargo test --lib --locked settings_page` |
| Request attribution / 请求身份展示 | `src/ai/identity.rs`, `src/chat/request.rs`, `src/ai_review/request_capture.rs`, `src/ai_review/agent_identity.rs`; see [Chat](chat.md) and [AI Review](ai-code-review.md) |
| Prompt content and provenance / Prompt 内容与来源 | `src/ai/prompts.toml`, `src/ai/prompts.rs`; `cargo test --lib --locked ai::prompts` |

Transport changes also follow [HTTP](http.md) and [security boundaries](security-boundaries.md).
UI copy and draft projection follow [i18n](i18n.md). / Transport 改动同时遵循 HTTP 与安全边界；
UI 文案和草稿投影同时遵循国际化规范。以下回归清单是验收要求，不代表本机已经完成验收。

## Configuration / 配置项

Model-list JSON and buffered completions, including JSON returned for a streaming request, enforce a
16 MiB response-body budget through the shared HTTP pipeline before JSON parsing.
Actual SSE keeps its existing parser and completion limits. Oversized JSON fails
with the typed, non-transient HTTP budget error and the existing failure UI.
模型列表 JSON 与缓冲 completion（包括流式请求收到 JSON 的回退）在解析前通过共享 HTTP 管线执行
16 MiB 正文预算；真实 SSE 保留原 parser 与完成限制。JSON 超限返回强类型、非瞬态
HTTP 预算错误，沿用既有失败展示。

Settings manages an arbitrary list of `AiAgentProfile { id, name, connection }` values. Stable ids identify Agents; Provider is a connection choice (`custom`, `deepseek`, or `openai`), so several Agents may use the same Provider. `AiAgentConfiguration` owns the list plus an independent `active_id`. The list searches saved name/model/Provider and shows ten rows per page, newest-created first, with Pagination and no separate Total/Page/per-page summary row. Durable profiles stay append-ordered; editing and activation do not reorder them. Add opens a new draft; a successful Create clears the query and shows the new Agent at the top of page one. Clicking a row or Edit opens that saved Agent. Use activates a saved id. The active row's Delete action is disabled, and the reducer also rejects deleting the active id.

Settings 管理任意数量的 `AiAgentProfile { id, name, connection }` 列表；稳定 id 标识 Agent，Provider 仅是连接类型（`custom`、`deepseek`、`openai`），同一 Provider 可以有多份 Agent。`AiAgentConfiguration` 保存列表和独立的 `active_id`。列表按已保存名称/模型/Provider 搜索，每页十条，按新建顺序倒序显示；保留 Pagination，不显示独立 Total/Page/每页条数摘要。持久化仍按追加顺序保存，编辑和激活不重排。新增打开新草稿，创建成功后清空查询并把新记录放在第一页首项；点击行或编辑打开对应已保存 Agent；使用操作激活已保存 id。当前使用行的删除按钮禁用，reducer 同样拒绝删除当前使用 id。

The list uses a single Hooks-style frame with a 1 px border and 6 px radius, 72 px fixed rows, internal 1 px dividers and no row gaps. `github_ai_agent_list_row.slint` keeps name/model and the row edit hit target before the trailing 168 px action group plus 16 px right inset. A shared 48×24 Toggle, then 32 px Test Connection (`plug`), Edit (`pencil`) and Delete (`trash`) use 8 px gaps and one vertical centerline. Turning an inactive Toggle On activates that saved id; the current On Toggle keeps its green enabled appearance but rejects interaction, so switching to another Agent is required to turn it Off. Current deletion stays disabled. Add remains an icon-only primary button.

列表使用统一 Hooks 式 1px 外框与 6px 圆角，固定 72px 行高、内部 1px 分隔线、无行间空隙。`github_ai_agent_list_row.slint` 的名称/模型及行编辑命中区避开尾部 168px 操作组和 16px 内缩。共享 48×24 Toggle 后依次为 32px 连通性测试（plug）、编辑（pencil）及删除（trash）IconButton，间隔 8px、垂直中心一致。非当前 Toggle 只有切 On 才激活对应已保存 id；当前 On 保持绿色启用外观但拒绝交互，必须启用另一个 Agent 才能使它 Off，不能清空唯一当前项；当前项仍禁删。新增仍为纯图标 Primary 按钮。

Agent Tooltips follow the fixed factory contract: seven distinct Add/Activate/Current/TestConnection/Edit/Delete/DeleteDisabled publishers, literal semantic identities and seven permanently mounted I18n surfaces share one root transient host. TestConnection uses `I18n.common-test-connection` and the official Plug Octicon. Disabled Current/Delete use hover-only targets. Claims contain geometry, kind, token and lease rather than labels; list/query/page/scroll/layout/modal/route changes dismiss them, and stale releases or renewals cannot replace a newer claim. Generic IconButton tooltip publishing stays disabled; the dedicated feature portal provides the visual Tooltip.

Agent Tooltip 遵守固定 Factory 契约：Add/Activate/Current/TestConnection/Edit/Delete/DeleteDisabled 七种独立发布方持有固定语义身份，七个常驻 I18n Surface 共用一个根层 transient host；TestConnection 复用 `I18n.common-test-connection` 与官方 Plug Octicon，禁用的 Current/Delete 使用纯 hover 命中区。Claim 只传几何、kind、token、lease，不传标签；列表/查询/分页/滚动/布局/模态/路由变化均 dismiss，旧释放或续租不能影响新 claim。IconButton 的通用 Tooltip 发布保持关闭，专用功能 Portal 提供视觉提示。

List Test captures the full saved profile by stable id and reuses `agent::test_connection`, including Header-only authentication, origin/redirect checks, budgets and redacted diagnostics. It never opens a dialog, tests unsaved editor values, saves or activates an Agent. One list probe runs at a time; its row shows a Spinner and repeated/peer Test activation is blocked. Editor and list probes use separate instances of the shared ticket/AbortHandle lifecycle owner. Completion checks the ticket, active list surface and latest saved profile equality. Editing/deleting the tested profile suppresses stale feedback; activating a peer does not change the captured request. Query/page changes keep the probe; leaving the section/route or opening an editor/delete dialog cancels it.

列表 Test 按稳定 id 捕获完整已保存 Profile，复用 `agent::test_connection`，包括 Header-only 鉴权、origin/重定向、预算和脱敏诊断；不能打开弹窗、读取未保存编辑值、保存或激活 Agent。列表同时一个 probe，对应行显示 Spinner，重复/其他 Test 激活被拦截；编辑器与列表各自持有共享 ticket/AbortHandle 生命周期 owner 的独立实例。完成时核验 ticket、列表活跃及最新已保存 Profile 完全一致；被测试项编辑/删除抑制旧反馈，激活其他项不改变捕获请求。查询/分页保留 probe，离开分区/路由或打开编辑/删除弹窗会取消。

List feedback is a closable `GitHubToast` at the bottom of the current Settings content viewport, outside scrolling content. Semantic success, model-unlisted success and failure reuse the editor's catalog keys and refresh immediately with the locale. Closing or its 3-second timeout hides only the Toast, retaining the key/text; locale refresh does not reopen it. Only the event loop updates UI. List probes do not create a second HTTP client or duplicate HTTP error Toast.

列表反馈复用滚动内容之外、当前 Settings 内容视口底部可关闭的 `GitHubToast`；成功、模型未列出成功和失败复用编辑器语义 key，切语言即时重投影。关闭或 3 秒超时仅隐藏 Toast，保留 key/文案，语言刷新不得重开；只有 event loop 回写 UI，不能新增 HTTP client 或重复 HTTP 错误 Toast。

Add/Edit uses the shared root `GitHubDialog`: Agent name, Provider Select, Base URL, API key, Model, reply language, and a Postman-style Header table with enabled/name/value/add/delete controls. At most 32 Header rows, including disabled rows, are allowed. Ten fixed list leaves and a fixed-count Header editor keep delegate ownership bounded; Provider/language options have fixed leaves and root Select portals outside the scrolling dialog. Code hosting keeps its independent fixed-three Tabs. Test, Cancel and Save remain at the dialog bottom. Cancel or route exit invalidates the editor revision and clears unsaved credentials. Field/Header edits clear old test feedback; completion additionally checks the captured revision and entire draft.

新增/编辑复用根层 `GitHubDialog`，包含 Agent 名称、Provider Select、Base URL、API key、Model、回复语言及 Postman 式 Header 表格，支持启用/名称/值/添加/删除，最多 32 行（含禁用行）。列表使用十个固定叶子，Header 编辑器保持固定 delegate 数量；Provider/语言选项使用固定叶子，Select portal 挂在滚动弹窗之外的根层。Code hosting 保留独立固定三项 Tabs。测试连通、取消、保存位于弹窗底部。取消或路由退出使编辑器 revision 失效并清空未保存凭据；字段/Header 编辑清除旧测试反馈，完成回调还须核验捕获的 revision 与完整草稿。

Provider and reply-language menus use the shared Select placement rule within their existing caller-supplied window viewport: prefer whichever side has more available vertical space, even if a short menu fits on both sides; equal space opens below. The existing 264 px/content/available-space height limit and scrolling remain. Root portals stay outside the form's scroll clip at z=9000. Select has no Dialog knowledge, body-boundary API, registry or timer; a particular three-option menu fitting above the footer is an acceptance outcome, not a shared Dialog-body containment contract. One Escape closes the menu while retaining the editor and draft.

Provider 与回复语言菜单在既有调用方窗口视口内使用共享 Select 定位：优先上下可用空间更多的一侧，即使短菜单在两侧都能放下；相等时向下展开。现有 264px/内容/可用空间高度限制和滚动保持不变，根 Portal 仍在表单裁剪之外的 z=9000。Select 不感知 Dialog，不引入正文边界 API、注册表或 Timer；特定三选项菜单能在页脚上方完整显示是验收结果，不是共享的 Dialog 正文包含契约。一次 Escape 只关闭菜单，保留编辑器和草稿。

A pointer click on a Provider/reply-language trigger's visible portion must also work when the form clips its top or bottom. The shared trigger does not pre-focus itself before activation: opening transfers focus to the root ActionMenu without revealing or repositioning the trigger. Form scroll offset and trigger position remain unchanged through release and subsequent layout frames. Keyboard activation and close-time trigger focus remain; a real form scroll still dismisses the menu. The first click must preserve draft values and editor state.

Provider/回复语言触发框被表单上缘或下缘部分裁切时，点击可见区域仍须首次展开。共享触发器不在激活前聚焦自身，由根层 ActionMenu 在展开时获取焦点，不自动 reveal 或重定位触发框；正文滚动偏移和触发框位置在释放及后续布局帧保持不变。键盘激活和关闭时触发器焦点恢复保持不变，真实表单滚动仍关闭菜单；首次点击不得修改草稿或编辑会话。

The editor requests a 620 px width and `min(560px, max(320px, root.height - 140px))` height with `fullscreen-on-narrow: false`. Shared Dialog viewport clamping preserves a 16 px outer margin; an 800×600 viewport yields a 620×460 px surface. Only the form body scrolls; the header and Test/Cancel/Save footer retain their positions.

The body viewport spans the complete area below the 56 px header and above the 64 px footer. Its 16 px top/bottom padding belongs to the scrolling form, so the first field and final Header row keep that padding at the start/end without leaving fixed blank clipping bands while scrolling. This applies to both Add and Edit.

正文视口覆盖 56px 页头下缘到 64px 页脚上缘的完整区域。16px 上下留白归滚动表单所有，首字段和末尾 Header 行在起止位置保留留白，滚动中不得出现固定空白裁切带；新增与编辑遵循同一契约。

编辑器请求 620px 宽、高度为 `min(560px, max(320px, root.height - 140px))`，并设置 `fullscreen-on-narrow: false`。共享 Dialog 的视口钳制保留 16px 外边距；800×600 视口下弹窗为 620×460px。仅表单正文滚动，页头与测试/取消/保存页脚保持固定。

The form's real field borders leave at least 8 px before the 8 px scrollbar track. Show API key's switch and text share a vertical centerline. Idle Test has no old result icon; testing disables the button and shows a 16 px Spinner 8 px to its right. Success/failure uses that same trailing slot. Localized feedback reuses a closable `GitHubToast` at the bottom center of the body, above the footer and outside the scrolling form. Dismiss hides only that feedback; it preserves the semantic outcome, draft and committed registry. Locale refresh must not reopen dismissed feedback. Editing or starting another test retires old feedback.

表单实际字段边缘与 8px 滚动条轨道之间至少留 8px 空隙，Show API key 开关与文字垂直居中。空闲测试按钮不显示旧结果；测试中按钮禁用，右侧间隔 8px 显示 16px Spinner，成功/失败图标复用同一槽位。本地化反馈复用正文底部居中、固定页脚之上且在滚动表单之外的可关闭 `GitHubToast`。关闭只隐藏反馈，保留语义结果、草稿和已提交列表；切语言不能重开已关闭反馈。编辑或开始下一次测试清除旧反馈。

Headers have Key/Value and JS object modes over one dialog draft. Object mode accepts data-only quoted/ASCII-identifier keys, string values or nonempty string arrays, and optional trailing commas; it evaluates no code and rejects comments, expressions, numbers, booleans, null and nested objects. Input is capped at 65,536 bytes. Repeated values become enabled rows in append order. An unchanged mode round trip preserves original ordering and disabled rows exactly; valid edited objects replace enabled rows while retaining disabled drafts in their relative slots. Converted data must satisfy the 32-row cap and shared HTTP Header validation before it replaces rows.

Headers 的 Key/Value 与 JS object 模式共享弹窗草稿。对象模式只接收引号/ASCII identifier key、字符串值或非空字符串数组及可选尾逗号，不执行代码，拒绝注释、表达式、数字、布尔、null 与嵌套对象；输入最多 65,536 字节。重复值按追加顺序转换为启用行。未改动的模式往返精确保留行顺序和禁用行；有效对象编辑替换启用行，并在相对槽位保留禁用草稿。转换数据只有通过合计最多 32 行及共享 HTTP Header 校验后才替换表格。

Invalid object input may still switch to Key/Value, leaving the previous row draft unchanged and clearing object-only feedback from that view. Its exact raw source, including an empty string, remains parked; returning to JS object restores it and immediately revalidates it. Real row name/value/enabled changes, Add or Delete supersede that source, so the next Object visit formats the current rows. Same-value setters, reselecting the current mode and rejected busy/index/cap callbacks do not discard parked input.

无效对象仍允许切回 Key/Value，保留原表格草稿并清除该视图中的对象反馈。原文精确保留（含空字符串），回到 JS object 时恢复并立即重验。真实行名称/值/启用变化、新增或删除使暂存原文失效，下一次 Object 使用当前表格重新生成；同值 setter、重选当前模式和被 busy/索引/上限拒绝的 callback 不丢弃暂存输入。

Save and Test validate only the active input mode. Active invalid Object input cannot be saved or requested; a parked inactive object does not block valid Key/Value rows and is never persisted or sent. All mode changes and real Header edits cancel probes, advance Header-input revision and clear old feedback. Late Save/Test still checks that revision and the complete normalized profile. A callback-private `Cell<Option<i32>>` tracks parked identity without another UI property or raw-value copy. Cancel, route re-entry and opening Add/Edit reset raw text and revision ownership.

Save/Test 只验证当前输入模式：当前 Object 无效时不得保存或请求；非当前的暂存对象不阻止有效 Key/Value 表格，也不落盘或发送。模式变化与真实 Header 编辑都取消 probe、递增 Header 输入 revision 并清旧反馈，迟到 Save/Test 仍核验该 revision 和完整归一化 Profile。模式 callback 的私有 `Cell<Option<i32>>` 标识暂存输入，不增加 UI 属性或原始值副本；取消、路由重入和打开新增/编辑重置原文与 revision 归属。

Add Header belongs to Key/Value tab content, below its caption and horizontally centered in a 32 px toolbar above the rows, including an empty table. It reuses `GitHubButton` with the leading `octicon-plus-16.svg`; its existing localized label is unchanged. It is absent from JS object content. Each 40 px row ends with a shared 32×32 trash IconButton at y=4 px, using the same semantic danger state as Agent-list Delete. Its first pointer click removes exactly that slot from the current draft, including disabled rows, without changing peer Headers or saved Agents. At 32 rows or while busy Add is disabled; busy/mode/cap checks also remain in the Rust callbacks.

Add Header 属于 Key/Value Tab 内容，水平居中于说明文字下方、各行上方的 32px 工具栏，空表同样保留；复用带前导 `octicon-plus-16.svg` 的 `GitHubButton`，沿用现有本地化标签。JS object 内容中不存在该按钮。每个 40px 行末尾为 y=4px 的共享 32×32 垃圾桶 IconButton，采用与 Agent 列表删除相同的语义 danger 状态；首次单击精确删除当前草稿对应槽位（含禁用行），不得修改其他 Header 或已保存 Agent。达到 32 行或 busy 时禁用新增，Rust callback 仍须独立核验 busy/模式/上限。

Header Delete uses `github_ai_agent_header_tooltip.slint` rather than the dynamic generic Tooltip: one literal action, one fixed I18n Delete Surface and one transient portal sibling to the dialog frame. Placement is bounded to the actual scrolling body; no Header name/value crosses the portal. Revision/row changes, scroll, mode switch, close and route exit dismiss claims, with token/lease rejection of stale publishers. Busy/object mode disables this publisher. Native painted labels still require the [Tooltip acceptance matrix](tooltips.md).

Header 删除使用 `github_ai_agent_header_tooltip.slint` 的固定工厂路径：单个固定动作、独立常驻 I18n 删除 Surface，以及 dialog frame 同级的单个 transient Portal；定位边界为实际滚动正文，不跨 Portal 传递 Header 名称或值。revision/行变化、滚动、模式切换、关闭及离开路由撤销 claim，token/lease 拒绝旧发布方；busy/对象模式关闭发布。实际绘制文字仍须完成 [Tooltip 验收矩阵](tooltips.md)。

Save captures the dialog's complete id/name/connection/Header draft before queuing Create or Update. It preserves other Agents and does not activate an existing or additional Agent; creating the first Agent selects it as the initial active id. Delete first opens a separate confirmation naming the target, with Chinese copy `你确定要删除名为 [{name}] 的 AI Agent吗？`; only Confirm admits Delete. The reducer rechecks target existence and active ownership when every command executes. Create/Update/Delete/Activate share the ordered worker and report success after persistence flush. Editor Test captures the dialog connection; List Test captures the clicked saved profile. Neither saves nor activates it.

Save 在排队 Create/Update 前捕获弹窗完整 id/名称/connection/Header 草稿，保留其他 Agent，不激活已存在或后续新增 Agent；创建第一项时将其设为初始当前使用 id。删除先打开单独确认框，中文文案为 `你确定要删除名为 [{name}] 的 AI Agent吗？`，只有确认后才准入 Delete。每项命令执行时，reducer 再核验目标存在及当前使用归属。Create/Update/Delete/Activate 共用有序 Worker，在持久化 flush 后才提示成功；Editor Test 捕获弹窗 connection，List Test 捕获被点击的已保存 Profile，两者均不保存或激活。

The modern TOML schema stores `active_id` under `[ai.agent]` and one `[[ai.agent.profiles]]` record per Agent, with id/name and flattened connection fields; nested Header rows retain enabled/name/value. Legacy single-connection data migrates into one stable-id Agent. Legacy fixed `[ai.agent.profiles.custom|deepseek|openai]` tables migrate independently into list records with matching stable ids, retaining their selected active id and isolated credentials. Modern list ids remain stable after normalization rather than being regenerated; duplicate/empty ids, more than 32 Header rows, and malformed data fail validation. An unknown active id clears selection rather than choosing another credential implicitly. Empty configuration starts with an empty list.

现代 TOML 在 `[ai.agent]` 保存 `active_id`，每个 Agent 使用一条 `[[ai.agent.profiles]]`，包含 id/名称和平铺 connection 字段；嵌套 Header 行保留启用/名称/值。旧单连接配置迁移为一个稳定 id Agent；旧固定 `[ai.agent.profiles.custom|deepseek|openai]` 表分别迁移为对应稳定 id 的列表记录，保留原当前使用项与隔离凭据。现代列表 id 规范化后稳定保留，不重新生成；重复/空 id、超过 32 行 Header 或损坏数据导致验证失败。未知 active id 清空选择，不得隐式选用其他凭据。空配置使用空列表。

A usable request needs Base URL, Model, and either an API key or at least one enabled nonempty custom Header. The API key supplies default Bearer authentication; an enabled custom Authorization Header overrides it. Disabled Header rows are preserved but never sent or validated as active request data. Enabled names/values must be valid HTTP headers; CR/LF injection and transport-owned names such as Host, Content-Length, Connection and Proxy-Authorization fail before networking. Duplicate ordinary Headers retain append order; explicit Content-Type overrides the JSON default. Custom values are marked sensitive, use the shared same-origin redirect policy and never enter UI errors/logs. Each Agent owns `reply_language = "auto" | <locale-id>`; `auto` resolves at request assembly, while explicit locales remain independent. RAG is unaffected.

有效请求需要 Base URL、Model，以及 API key 或至少一条已启用且值非空的自定义 Header。API key 提供默认 Bearer 鉴权，已启用的自定义 Authorization 可覆盖它。禁用 Header 行保留但不发送，也不作为活动请求数据验证；启用行必须符合 HTTP Header 格式，CR/LF 注入及 Host、Content-Length、Connection、Proxy-Authorization 等 transport 专有名称在联网前失败。重复普通 Header 保留追加顺序，显式 Content-Type 覆盖 JSON 默认值。自定义值标记为敏感，遵守共享同 origin 重定向策略，不能进入 UI 错误或日志。各 Agent 独立保存 `reply_language = "auto" | <locale-id>`；`auto` 在组装请求时解析，显式 locale 保持独立，RAG 不受影响。

## Request identity / 请求身份

AI Chat, Review admission, and each Ask capture the saved active profile once through `src/ai/settings_state.rs::current_profile()`, before asynchronous work is queued. The same captured connection supplies transport and the raw non-secret `{ name, provider, model }` identity supplies the visible Agent name/model. Switching or renaming profiles later cannot relabel an in-flight request or historical result. Chat assistant JSONL records, Review task summaries, and Ask conversation records persist that optional raw identity. Old records without attribution remain readable without inventing the current Agent's identity. Only empty-name fallbacks and label formatting are localized; user names and model ids remain literal and re-project on a locale switch. URLs and keys are excluded from attribution. RAG remains independent.

AI Chat、Review 准入及每次 Ask 都在排队异步工作前，通过 `src/ai/settings_state.rs::current_profile()` 一次性捕获已保存的当前使用 Profile。同一份 connection 用于请求，原始非敏感 `{ name, provider, model }` 身份用于展示 Agent 名称和模型。之后切换或重命名 Profile 不得重标记在途请求或历史结果。Chat assistant JSONL、Review 任务摘要与 Ask 对话记录持久化可选的原始身份；缺少身份的旧记录继续兼容，不能补成当前 Agent。只有空名称回退和标签格式参与本地化，用户名称与模型 id 保持原文，切语言时重新投影。身份数据不包含 URL 或 Key；RAG 保持独立。

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
- AI Chat, AI Code Review, and Ask use their captured saved active profile. Editor Test captures the open dialog's connection; List Test captures the clicked saved profile. Neither activates it. RAG continues to use `src/chat/chat.rs` and its independently configured HTTPS endpoint.
- The Settings-page test uses `/models` through the same shared HTTP module. It verifies URL reachability and credentials without creating a billable completion. A provider may not list every model; this is reported as a non-fatal result.
- The dialog connection test publishes one localized body-bottom Toast and a persistent trailing result icon, guarded by editor/Header-input revisions and the complete normalized draft. Request feedback stays silent in the shared HTTP layer so failures are not duplicated.

- `src/ai/agent.rs` 负责 AI Agent Provider 连接，针对可包含 `/v1` 的配置地址规范化 `/models` 和 `/chat/completions`，并统一通过共享 HTTP 执行。
- AI Chat、AI Code Review 与 Ask 使用各自捕获的已保存当前使用 Profile；编辑器连通性测试捕获弹窗 connection，列表测试捕获被点击的已保存 Profile，均不激活它。RAG 继续使用 `src/chat/chat.rs` 及其独立配置的 HTTPS endpoint。
- 设置页测试通过同一 HTTP 模块调用 `/models`，验证地址可达和凭据有效，不产生生成费用。Provider 不列出全部模型时只作提示，不视为连接失败。
- Chat 的失败仍显示为内联 assistant 消息；弹窗测试显示经 editor/Header 输入 revision 和完整归一化草稿核验的单条正文底部 Toast 及保留的右侧结果图标，共享 HTTP 层保持静默，避免重复提示。

## Invariants / 不变量

1. No AI request may construct a reqwest client or bypass `src/http/mod.rs`.
2. API keys must only be attached to configured provider requests (connection tests and completions) and must not appear in logs, Toasts, notifications, or error bodies.
3. Slint state is updated only on the event loop; the connection test runs on the shared Tokio runtime.
4. A missing URL/model or both an absent key and absent enabled nonempty Header is a local validation error; invalid enabled Headers must also fail before networking.
5. Completion requests carry a bounded feature-owned message list. API keys, complete prompts, source payloads, and response bodies must not be logged.
6. Ordinary completions and connection tests capture the shared `[http].request_timeout_ms` when `RequestOptions` is created: the default is 30,000 ms for both total and body-idle timeouts, and `0` disables both. Runtime edits affect subsequently created requests. Explicitly cancellable AI Review and Review Ask completions use OpenAI-compatible SSE streaming through the same interceptor pipeline, disable only the HTTP request total deadline, retain the 10-second connect timeout, and use a 300-second response-body idle timeout. Review chunks and Review Ask conversations additionally have caller-owned 600-second wall-clock limits. An SSE response is successful only after `[DONE]` or a `finish_reason`; provider error events and cleanly truncated streams fail explicitly. A provider that answers a streaming request with one JSON response remains supported.
7. Prompt rendering must succeed before a provider request starts; invalid catalog/schema/template data becomes a bounded internal/configuration error.
8. AI Agent form edits remain dialog-local drafts. The no-argument Save callback captures the complete named/id/Header draft on the Slint event loop; the feature worker dispatches one profile Action and waits for durable persistence. Neither reduction nor persistence runs on the event loop. Every Settings route re-entry restores the complete committed configuration before paint; a late completion must preserve newer edits and a later route visit.
9. Editor selection, Save, Delete confirmation and Activate are separate operations. Updating or adding a peer preserves the active id and all other profiles; only the first Create initializes activation. Activate changes only the active id and never captures unsaved editor fields. Active deletion is rejected again by the reducer.

1. AI 请求不得创建 reqwest client，也不得绕过 `src/http/mod.rs`。
2. API Key 仅附加到配置的 Provider 请求，不得出现在日志、Toast、通知或错误正文中。
3. Slint 状态只在 event loop 更新；连通性测试运行在共享 Tokio runtime。
4. URL/模型缺失，或 Key 与已启用非空 Header 同时缺失时为本地校验错误；启用 Header 无效也必须在联网前失败。
5. Completion 请求只接收 Feature 自有的有界消息列表；日志不得包含 API Key、完整 Prompt、源码 payload 或响应正文。
6. 普通 completion 与连通性测试在创建 `RequestOptions` 时读取共享 `[http].request_timeout_ms`：默认总时限和响应体空闲时限均为 30,000 ms，`0` 关闭两者；运行时修改影响随后创建的请求。可显式取消的 AI Review 与 Review Ask completion 通过同一 interceptor 管线使用 OpenAI-compatible SSE streaming，只关闭 HTTP 请求总截止时间，仍保留 10 秒连接超时和 300 秒响应体空闲超时。Review chunk 与 Review Ask conversation 另有调用方管理的 600 秒墙钟上限。SSE 只有收到 `[DONE]` 或 `finish_reason` 才成功；Provider error event 与干净断流的截断响应必须显式失败。若 Provider 对 streaming 请求返回一次性 JSON，仍须兼容。
7. Provider 请求开始前必须成功渲染 Prompt；catalog/schema/template 无效时只暴露有界的内部/配置错误。
8. AI Agent 表单编辑必须保留为弹窗局部草稿；无参 Save callback 在 Slint event loop 捕获完整名称/id/Header 草稿，Feature Worker 再 dispatch 单个 Profile Action 并等待持久化。Reducer 与持久化均禁止在 event loop 执行。每次重新进入 Settings 路由时，必须在绘制前恢复完整已提交配置；迟到完成不得覆盖新编辑或后续路由访问。
9. 编辑器选择、Save、删除确认与 Activate 是独立操作。更新或新增其他 Agent 不改变当前使用 id 或任何其他 Profile，只有首个 Create 初始化当前使用项；Activate 只修改当前使用 id，绝不捕获未保存编辑字段。Reducer 再次拒绝删除当前 Agent。

## Save admission, completion and shutdown / 保存准入、完成与关闭

`SettingsPageBridge` binds no-argument editor commands and captures the complete
page-local profile in `src/ai/settings_ui.rs::bind`. The callback captures the complete draft, request
identity and current factory visit before scheduling work; it never later reads
UI fields or the current Store as the input of an already accepted Save.
`AiAgentSettingsFacade` Create/Update/Delete/Activate commands share nonblocking FIFO
admission with at most 8 accepted commands, including the active durable command. Only an identical last
in-flight request is deduplicated; `A → B → A` retains the later revert. Full or
closed admission returns the command's localized failure feedback and leaves
already accepted requests intact.

`SettingsPageBridge` 为 `settings_ui::bind` 绑定无参编辑器命令并捕获页面完整 Profile。Callback 在调度前捕获
完整草稿、请求身份及当前 factory 访问；已接受 Save 不得延后读取 UI 或当前 Store
来替换原输入。Create/Update/Delete/Activate 共用非阻塞 FIFO，准入最多 8 项
（包含活动命令）；只合并末个
相同的在途请求，`A → B → A` 必须保留最后的回退。队列满或关闭时沿用本地化保存
或激活失败反馈，已接受请求不受影响。

The UI binding has a separate 8-owner budget spanning receipt waits and queued
Slint completion callbacks. Its RAII permit survives worker completion until the
UI callback runs or delivery is dropped; failure to enqueue onto the event loop
also releases it. Identical latest pending UI requests do not reserve another
owner. Thus draining the reducer/persistence queue cannot create an unbounded
queue of complete drafts waiting for the UI.

UI 绑定另有独立的 8 项完成 owner 预算，覆盖 receipt 等待及已排队的 Slint 完成回调。
RAII 许可在 Worker 完成后仍由 UI 请求持有，直到回调执行或投递被丢弃；event loop
投递失败同样归还。末个相同的待完成 UI 请求不重复占位。Worker 排空不会放开无限
份完整草稿在 UI 队列等待。

One actor on the shared `app_runtime` processes admitted Create/Update/Delete/Activate commands in order. Each
blocking worker operation performs `dispatch → flush` before the next command can
reduce. The Store commit automatically owns persistence. An unchanged explicit
Save calls `retry_failed()` before its checked flush, allowing the same failed
configuration to recover without a new business revision; a changed Save does
not implicitly retry its own failed automatic write. Every receipt carries that
request's committed `StoreSnapshot` and persistence result. A failed write stays
observable and dirty for a later explicit retry; no success is reported merely
because admission or reduction succeeded.

共享 `app_runtime` 上的单个 actor 顺序处理 Create/Update/Delete/Activate 命令，每项阻塞 Worker 操作完成
`dispatch → flush` 后才允许下一项 reduce；持久化仍由 Store commit 自动触发。
同值显式 Save 在 checked flush 前调用 `retry_failed()`，不增加业务 revision 即可
恢复失败配置；有变化的 Save 不隐式重试自己的自动写入失败。Receipt 保留对应请求
的已提交 `StoreSnapshot` 与持久化结果。写失败继续可观测并保持 dirty，等待后续
显式重试；准入或 reducer 成功不能作为保存成功。

Completion uses a weak window and `slint::invoke_from_event_loop`. Each receipt
refreshes list/runtime state from the facade's latest authoritative snapshot,
so out-of-order receipt delivery cannot regress committed state. Only the
captured editor revision owns its busy/error state. Successful Create/Update
closes the dialog only if its Header-input revision and complete currently
validated profile, including Header order/values, still match; later raw edits
or another editor session remain intact. Applied but
not durably persisted Create becomes an Update retry of the same stable id.
Only the latest command attempt owns Toast feedback. Probe results additionally
require the current probe ticket, open editor, matching revision and full draft;
Cancel/revision changes/route exit abort the probe. Error/test-result semantic
keys re-project in the active locale, without displaying raw transport errors.

完成通过 weak window 与 `invoke_from_event_loop` 返回 UI。每项 receipt 从 facade
最新权威快照刷新列表/运行状态，乱序投递不得回退已提交状态。只有捕获的 editor
revision 拥有其 busy/error；Create/Update 成功时还须 Header 输入 revision 与当前
有效完整 Profile（含 Header 顺序/值）保持一致，才关闭弹窗，之后原文编辑或新弹窗
不受影响。Create 已应用但持久化失败时，
以同一稳定 id 转为 Update 重试。只有最新命令 attempt 拥有 Toast。Probe 结果还须
核验当前 probe ticket、弹窗仍打开、revision 和完整草稿一致；取消、revision 变化
及路由退出都会中止 probe。错误/测试结果保留语义 key，随当前 locale 重投影，不显示
原始传输错误。

Application async drain calls `shutdown_async()`: close admission first, drain
every accepted Create/Update/Delete/Activate command, then perform a checked final flush and shut
down the Store worker off the event loop. Concurrent/repeated close waits for the same result.
Ordinary close does not retry a failed write; its failure is returned and logged.
The later synchronous cleanup is an idempotent fallback.

应用 async drain 调用 `shutdown_async()`：先关闭准入，再排空全部已接受 Create/Update/Delete/Activate 命令，最后
在 event loop 外执行 checked flush 并关闭 Store Worker。并发/重复关闭等待相同结果；
普通关闭不自动重试失败写入，失败必须返回并记录。后续同步 cleanup 作为幂等兜底。

## Regression checklist / 回归检查

Focused root filters are `ai::settings_state`, `ai::settings_ui` and
`settings_page` with `cargo test --lib --locked <filter>`. Worker tests exercise
real dispatch/persistence and the completion tests invoke the production
presentation function on its owning test UI thread. The no-event-loop testing
backend does not prove native event-loop scheduling or live visual acceptance.

专项过滤器为 `ai::settings_state`、`ai::settings_ui` 和 `settings_page`。Worker 回归使用
真实 dispatch/持久化；完成回归在所属测试 UI 线程调用生产展示入口。无 event loop 的
测试 backend 不能证明原生事件调度或视觉验收。

- Block the real worker; verify callbacks still return, reducers/persistence run
  off the caller, complete drafts and receipts keep FIFO identity, adjacent
  duplicates consume no extra owner, `A → B → A` survives, and the ninth active
  request is rejected without disturbing the first eight.
- Finish durable saves without consuming UI callbacks; the independent UI budget
  must stay at 8. Verify callback completion and failed event-loop delivery return
  permits, and an identical latest pending draft does not occupy another slot.
- Inject a write failure, explicitly save the same configuration and verify
  recovery without a new revision. Repeated failure and shutdown must report
  failure without hidden retries or false success feedback.
- Deliver older/newer receipts out of order, edit any field after Save, and leave
  then re-enter Settings before completion. Verify committed host state never
  regresses, newer drafts/visits remain intact and old feedback cannot overwrite the latest attempt. Check source and non-source locales.
- Close while a write is blocked; reject later input immediately, await every
  accepted receipt and durable write, and verify repeated close returns the same
  checked result.

- 阻塞真实 Worker，确认 callback 仍立即返回、reducer/持久化不在调用线程、完整草稿
  与 receipt 保持 FIFO 身份，相邻重复不多占 owner、`A → B → A` 保留，第九个活动
  请求被拒绝且前八个不丢失。
- Worker 完成而 UI 不消费回调时，独立 UI owner 仍不得超过 8；回调完成或 event loop
  投递失败必须归还许可，末个相同待完成草稿不重复占位。
- 注入写失败并显式保存同值，确认 revision 不变且恢复成功；重复失败及关闭不得隐藏
  重试或错误报告成功。
- 逆序投递 receipt、Save 后改任一字段、完成前离开并重新进入 Settings，确认 Host
  配置不回退、新草稿/访问不被覆盖、旧反馈不能干扰最新 attempt；覆盖源语言
  与一种非源语言。
- 写入阻塞时关闭，后续准入立即失败，关闭等待全部已接受 receipt 与真实落盘完成；
  重复关闭返回相同 checked 结果。

- Create at least 12 Agents including several with the same Provider, distinct names/models and credentials. Verify ten rows per page, case-insensitive name/model/Provider search, total matches, bounded pagination and clamped pages after deletion. Save/restart must preserve stable ids and active selection.
- Click a previously unfocused row's name/model area and each trailing action once. Edit must open that Agent; Use must activate only its saved id. Add/Edit/Cancel must not mutate peers or activation. The first Create may initialize activation, but subsequent saves must not select another Agent.
- Click each saved row's Test once from an unfocused state in EN/non-source locales and narrow/wide/sidebar layouts. Verify Toggle/Test/Edit/Delete order, exact stable id, spinner/duplicate guard, no editor or activation, and a bottom-content Toast. Cover listed/unlisted/auth failure, Header/redirect controls, saved-profile update/delete and stale completion, route/section/dialog cancellation, query/page retention, peer activation, locale refresh without reopening dismissed feedback, and fixed TestConnection Tooltip paint/lease bounds.
- Verify active Delete is disabled. For a different Agent, open the named confirmation, Cancel it without mutation, then Confirm. Queue activation before deletion and verify the reducer still protects the now-active target.
- Add, rename, disable, re-enable and remove Header rows; reject row 33. Disabled invalid values remain editable and unsent. Enabled invalid names, CR/LF values and transport-owned headers fail locally. Test Header-only authentication, explicit Authorization override and ordinary Bearer fallback without leaking synthetic secret markers.
- Switch syntax/security-invalid Object input, including empty text, to rows and back. Rows remain unchanged, parked text restores with semantic errors, and Save/Test validate only the selected mode. Same-value callbacks retain raw text; real name/value/enabled edits, Add or Delete supersede it. Close/reopen another Agent and verify raw secrets and old probe feedback do not return.
- In English and a non-source locale, verify empty/multiple-row Key/Value Add is centered with a leading Plus icon, no Add in JS object, 32×32 row-right Delete and exact-row removal on its first click. Inspect Delete Tooltip paint, body bounds, scroll/mode/close dismissal. Add/Edit body scroll must meet the header/footer edges while keeping 16 px content padding at both scroll endpoints.
- Scroll Provider/reply-language triggers in the actual Settings factory at compact/wide widths and with either sidebar state. Verify trigger-to-anchor identity, placement toward more available viewport space, option-to-value identity, repeated reopen/selection and one Escape retaining the editor. For the footer-adjacent three-option reply-language case, verify upward placement and its visible fit above the footer; do not infer a general body-boundary rule. Inspect native painted menus after source/non-source locale and route changes.
- Partially clip a trigger at either scroll edge and click its visible area once from an unfocused state. Verify no opening-induced viewport jump, menu remains open after pointer release and layout frames, selection preserves the draft, and subsequent real form scrolling closes it. Compare fully visible triggers and retain keyboard activation/close-focus checks.
- Test a dialog draft, edit before completion, Cancel/reopen another Agent, and switch locale while results/errors are visible. Stale completions must not enter the new revision, all feedback must re-project, unsaved credentials must clear on close, and Provider/locale controls must retain correct native painted labels.
- Load both legacy single connections and fixed provider tables. Verify independent stable-id migration and modern list round trips; reject duplicate/empty ids, invalid modern payloads and oversized Headers without cross-Agent credential fallback.
- Start AI Chat, queue a Review, and send Ask with one saved Agent; switch/rename the active Agent before completion. Verify the actual request and displayed/persisted name/model retain their captured identity, while the next request uses the new active profile. Reopen history and switch locale; literal names/models must remain intact and only unnamed fallbacks may translate.
- Test valid and invalid dialog credentials and verify one localized body-bottom Toast with a retained trailing result icon, without shared-HTTP Toast duplication.
- Verify a RAG Chat message still reaches its configured RAG endpoint after AI Agent settings are saved.
- Switch Chat between AI Chat and RAG; verify AI uses the configured model and each mode retains a separate history.
- Select each reply-language option in an Agent dialog, Cancel and reopen that Agent, and verify its saved choice is restored. Save each choice, reopen/restart, and verify it survives. Only the active saved Agent supplies AI Chat/Review/Ask reply language; RAG is unchanged and `auto` follows runtime interface language.
- Inspect one completed Review Session and one Review Ask assistant message; verify provenance contains only prompt metadata/context sizes and no source or credentials.
- Verify no logs contain the API key and no new direct reqwest client exists outside `src/http/mod.rs`.

- 创建至少 12 个 Agent，包含同 Provider 多项及各自名称/模型/凭据。验证每页十条、按名称/模型/Provider 忽略大小写搜索、匹配总数、有界页码及删除后页码收敛；保存/重启保留稳定 id 和当前使用项。
- 对未聚焦行的名称/模型区和尾部操作分别只点击一次；编辑打开对应 Agent，使用只激活已保存 id。新增/编辑/取消不得改变其他 Agent 或当前使用项；只有首个 Create 可初始化激活，后续保存不得切 Agent。
- 在英文/非源语言和窄/宽/侧栏布局中，对已保存行的 Test 从未聚焦状态只点一次；核对 Toggle/Test/Edit/Delete 顺序、稳定 id、Spinner/防重复、不打开编辑或激活、内容底部 Toast。覆盖列出/未列出模型/鉴权失败、Header/重定向对照、被测试项更新/删除的旧完成、路由/分区/弹窗取消、查询/分页保留、peer 激活、切语言不重开已关闭反馈，以及固定 TestConnection Tooltip 的实际绘制、lease 与边界。
- 当前 Agent 删除按钮禁用；其他 Agent 先打开带名称确认框，取消无变化，再确认删除。在删除前排队激活该目标，reducer 仍必须保护此时已激活的 Agent。
- 新增、修改、禁用、重启用、删除 Header，拒绝第 33 行。禁用的无效值仍可编辑但不发送；启用的无效名称、CR/LF 值及 transport 专有 Header 在本地失败。覆盖 Header-only 鉴权、显式 Authorization 覆盖和普通 Bearer 对照，禁止泄漏合成秘密标记。
- 将语法/安全无效对象（含空文本）切回表格再返回，表格保持不变、原文恢复并显示语义错误，Save/Test 只验证当前模式。同值 callback 保留原文，真实名称/值/启用变化或新增/删除使原文失效；关闭后编辑另一 Agent，原文凭据和旧 probe 反馈不得回来。
- 在英文与非源语言下检查空/多行 Key/Value 的 Add 居中且带前导 Plus 图标、JS object 无 Add、行末 32×32 Delete 首次点击精确删除；检查删除 Tooltip 实际绘制、正文边界及滚动/模式/关闭撤销。新增/编辑正文滚动视口直抵页头/页脚边缘，起止位置仍保留 16px 内容留白。
- 在实际 Settings factory 的窄/宽布局及侧栏开/关状态下滚动 Provider/回复语言触发框，检查触发框/锚点身份、朝视口可用空间更多的一侧展开、选项和值一一对应、重复展开/选择及一次 Escape 保留编辑器。靠近页脚的三选项回复语言场景须向上展开并完整显示在页脚上方，不得推导为通用正文边界规则；切语言和路由后检查原生实际绘制。
- 将触发框部分裁切于滚动区上缘/下缘，从未聚焦状态单击可见区域；不得因展开而跳动视口，鼠标释放及多个布局帧后菜单仍展开、选择不改其他草稿，随后真实表单滚动仍关闭。以完整可见触发框对照，保留键盘激活/关闭焦点检查。
- 测试草稿后在完成前编辑、取消并打开另一 Agent、在反馈可见时切语言；旧完成不得进入新 revision，反馈即时重译，关闭清空未保存凭据，Provider/语言控件实际绘制文字正确。
- 分别迁移旧单连接和固定 Provider 表，验证稳定 id 隔离及现代列表往返；拒绝重复/空 id、无效现代数据与超额 Header，不得跨 Agent 回退凭据。
- 用一份已保存 Agent 发起 AI Chat、排队 Review 和发送 Ask，在完成前切换/重命名当前 Agent。实际请求及展示/持久化名称和模型必须保持捕获时身份，下一请求才使用新当前 Profile。重开历史并切换语言，用户名称/模型保持原文，只有空名称回退可翻译。
- 用有效/无效弹窗鉴权测试，确认只显示单个本地化正文底部 Toast 并保留右侧结果图标，共享 HTTP 不重复 Toast。
- 保存 AI Agent 设置后，确认 RAG Chat 仍请求其独立配置的 RAG endpoint。
- 在 Chat 中切换 AI Chat 与 RAG，确认 AI 使用配置模型且两种模式保留独立历史。
- 在 Agent 弹窗逐个选择回复语言，取消再编辑同一 Agent，确认恢复已保存项；保存后重开/重启仍须保留。只有已保存当前 Agent 决定 AI Chat/Review/Ask 回复语言，RAG 不变，`auto` 跟随运行时界面语言。
- 检查一个已完成 Review Session 和一条 Review Ask assistant 消息，确认 provenance 只有 Prompt 元数据/上下文大小，不含源码或凭据。
- 确认日志不包含 API Key，并且 `src/http/mod.rs` 之外没有新增直接 reqwest client。
