# AI Code Review / AI Code Review

## Scope / 范围

This specification covers the optional, page-independent AI review task engine,
its Settings configuration, project rule binding, persistence, and the
GitHub-style result panel mounted beside the PR Files changed workspace. A
review task does not own a route or require a visible page.

本文约束可选且页面无关的 AI Review 任务引擎、Settings 配置、项目规则绑定、
持久化，以及挂在 PR Files changed 工作区右侧的 GitHub 风格结果面板。Review
任务不拥有独立路由，也不要求页面保持可见。

## Ownership / 职责

- `crates/usher-ai-review/`: framework-neutral inputs, rules, scheduler,
  reducers, prompt construction, diff chunking, result parsing, and sessions.
- `src/ai_review/config.rs`: `usher-state-store` facade plus automatic
  configuration persistence.
- `src/ai_review/controller.rs`: task orchestration, Git/provider diff loading,
  AI calls, cancellation, session persistence, notifications, and on-demand UI
  projection.
- `src/agent_response/`: width-independent, bounded projection of agent-authored
  Markdown, content-identity caching, and opaque repository-file actions. It
  performs no file I/O and owns no Preview host.
- `src/artifact_preview/`: worker-side resolution and reading of explicitly
  activated repository Markdown plus delivery through the shared Preview
  provider/host pipeline.
- `ui/features/ai_review/`: feature package contract and the optional PR-side
  panel. Primer finding rows and severity labels live in
  `ui/components/ai_review/`; compact response blocks live in
  `ui/components/agent_response/`. Both visual layers own no parsing or I/O.

- `crates/usher-ai-review/`：与框架无关的输入、规则、调度器、reducer、Prompt、
  diff 分块、结果解析和 Session。
- `src/ai_review/config.rs`：基于 `usher-state-store` 的门面和自动配置持久化。
- `src/ai_review/controller.rs`：任务编排、Git/Provider diff 获取、AI 调用、取消、
  Session 持久化、通知和按需 UI 投影。
- `src/agent_response/`：Agent Markdown 的宽度无关有界投影、内容身份缓存和不透明
  仓库文件 action；不执行文件 I/O，也不拥有 Preview Host。
- `src/artifact_preview/`：由 Worker 解析并读取用户显式激活的仓库 Markdown，再经
  共用 Preview Provider/Host 管线交付。
- `ui/features/ai_review/`：Feature 包协议和可选 PR 右侧面板；Primer finding row
  与严重级标签位于 `ui/components/ai_review/`，紧凑回复 block 位于
  `ui/components/agent_response/`；两层视觉代码都不承担解析或 I/O。

## Inputs and configuration / 输入与配置

Each task receives a selected AI model, the current PR commit diff, optional
local and remote repository locations, the resolved Markdown rule snapshot,
and optional one-off requirements. Rule files are limited to 512 KiB and are
read on a worker only when a review starts.

Settings stores a concurrency limit from 1 through 8, named local Markdown
rules, one default rule, and per-project bindings. A project may inherit the
default or explicitly bind one preset. Deleting a referenced rule changes every
explicit project binding to default inheritance and repairs the default rule.
Adding a preset validates, on a worker, that its path is absolute and identifies
an existing regular `.md` or `.markdown` file within the 512 KiB limit. Invalid
input remains in the form and receives field-level feedback; it is never stored.

每个任务接收选中的 AI 模型、当前 PR commit diff、可选本地/远程仓库地址、解析后的
Markdown 规则快照，以及一次性的附加要求。规则文件最大 512 KiB，只在任务开始时由
Worker 读取。

Settings 持久化 1 到 8 的并发上限、具名本地 Markdown 规则、一个默认规则和项目级
绑定。项目可继承默认规则或显式选择预设；删除被引用规则时，所有相关项目绑定自动改为
继承默认值，并修复默认规则。
新增预设时由 Worker 校验路径为绝对路径，并指向现有的普通 `.md` 或 `.markdown`
文件且不超过 512 KiB。无效输入保留在表单中并显示字段级错误，绝不写入配置。

## Lifecycle and concurrency / 生命周期与并发

1. Starting from the PR list queues a metadata-only task and leaves the list
   visible. Starting from PR detail may open the side panel immediately.
2. The scheduler starts at most the configured number of tasks. Raising the
   limit starts queued work immediately; lowering it never interrupts running
   work and applies as tasks finish.
3. One project/PR may have only one active task. Different PRs and projects may
   run concurrently.
4. Diff content, rule content, prompt chunks, and provider responses are loaded
   only after a task receives a scheduler slot. Large buffers are not retained
   in the durable task store.
5. Completion persists one session and publishes a Message Center result.
   Persisted task summaries are bounded to 128 terminal/active entries.
6. Opening a completed result loads its review session and agent-response
   projections on a worker. Opening a cancelled or failed task loads its
   lightweight PR chat session when one exists. Closing the panel immediately
   clears finding/conversation models, input text, the inline response
   projection, opaque-action map, and task repository binding. Leaving PR Review
   performs the same cleanup and cancels the current Ask so a late completion
   cannot enter a reopened mount. Independently scheduled Review tasks continue
   without a UI dependency.
7. Cancellation uses a task flag and queued-task removal. Shutdown stops the
   configuration and task persistence workers in the registered shutdown order.
   Cancelling a running task immediately marks it cancelled and aborts the
   in-flight provider completion before another diff chunk can start.
8. Panel close or PR-route exit also dismisses an active PR-context Preview only
   when its current host is Docked. An already Detached Preview remains open as
   an independent application window. Closing only the Docked Preview restores
   the intact AI conversation and does not execute panel cleanup.

1. 从 PR 列表启动时只入队元数据并留在列表；从 PR 详情启动时可立即打开右侧面板。
2. 调度器最多启动 Settings 配置的任务数；调高后立即调度队列，调低不打断运行任务，
   在任务完成后生效。
3. 同一项目/PR 只允许一个活动任务；不同项目和 PR 可以并发。
4. 只有获得调度槽后才读取 diff、规则、Prompt chunk 和 Provider 响应；持久任务 Store
   不保留大缓冲区。
5. 完成后持久化一个 Session 并发布 Message Center 结果；任务摘要最多保留 128 条。
6. 打开完成结果时由 Worker 加载 Review Session 和 Agent 回复投影；打开已取消或失败的
   任务时，如存在则加载轻量 PR Chat Session。关闭面板立即清空 finding/conversation
   model、输入、行内回复投影、不透明 action 映射和 task 仓库绑定。离开 PR Review
   执行同样清理并取消当前 Ask，避免迟到结果进入重新打开的 mount；独立调度的 Review
   task 不依赖 UI，继续运行。
7. 取消使用任务标记和队列移除；关闭应用时按已注册顺序停止配置与任务持久化 Worker。
   取消运行中任务时立即标记为已取消，并中止在途 Provider completion，后续 diff
   分段不得继续启动。
8. 关闭面板或离开 PR 路由时，仅在当前 PR 上下文 Preview 处于 Docked Host 时同时将其
   关闭；已经 Detached 的 Preview 作为独立应用窗口继续存在。只关闭 Docked Preview
   会恢复完整 AI 对话，不执行面板清理。

## Presentation / 展示

The PR detail Files changed view remains visible on the left. The right panel
follows GitHub Copilot Chat's PR-side information architecture: one compact
header with rule/model context, one continuous scroll containing review status,
summary, High/Medium/Low findings, and the review-scoped conversation, plus one
pinned composer. `Ask` and `Review again` reuse that composer through a Primer
segmented mode instead of stacking separate text fields or mounting a second
conversation header. A mode changes on the first pointer click even when the
text input previously held focus. The pinned canvas has no full-width rule
crossing behind the rounded input; the input border alone frames the composer.
Like GitHub Copilot Chat on a pull request, `Ask` remains available for queued,
running, completed, failed, and cancelled tasks. The current PR metadata and a
bounded diff excerpt are its primary context; a completed AI Review result is
optional additional context. Conversation history is stored locally and sent
to the Settings-configured OpenAI-compatible AI Agent. Ask only performs
read-only PR context loading and never submits a hosting-provider comment,
review, approval, or other write.
Submitting an Ask message immediately appends the user's message to the visible
conversation. While the response is pending, an Agent row shows bounded
operational phases such as PR-context preparation and thinking; these phases
never claim to expose hidden model reasoning. The composer action becomes the
same compact Stop control used by Review cancellation. Activating it aborts
context loading or the in-flight Agent completion without cancelling the AI
Review task, then marks the visible response as stopped.
The review status and every pending Ask response show one compact live elapsed
timer. Review duration starts when the task is enqueued and freezes on
completion, failure, or cancellation. Ask duration starts on submission;
successful assistant messages persist their final elapsed milliseconds, while
stopped and failed visible responses freeze the measured duration immediately.
Running reviews also show the exact current diff chunk and total chunk count.
The percentage is derived from completed chunks on a 96-percent processing
scale; it is workflow progress, not provider token or model-generation progress.
Legacy task summaries without chunk counts retain the percentage-only fallback.
While a review is queued or running, the `Review again` composer action becomes
an enabled inverse circular Stop button with a 20px visual surface, a square
glyph, and a 24px interaction target. It cancels that task. The PR list keeps
its Copilot action and adds a denser 16px-visual Stop button with the same 24px
target directly to its right. Conversation, Copilot, and Stop action centers
remain evenly spaced while that row's task is active.
Activating a finding selects its changed file and line in
the existing diff. The panel uses Primer density and repository-owned `GitHub*`
components; it is not a standalone review page. Its divider has a persistent resize hit target: dragging
retains a session width preference, clamps the panel between its readable
minimum and the remaining diff workspace, moves and clips the lightweight diff
viewport continuously, and commits the heavy diff layout once when the drag
ends. The live diff viewport exclusively owns the single-pixel resting divider;
the wider hit target paints only its hover/drag affordance and captures the first
pointer press after a Dialog closes. The Files changed toolbar opens the panel through the official 16 px
Copilot Octicon in a 32 px Primer IconButton with a 12 px trailing
inset; hover, pressed, selected, focus-visible, and loading states use the shared
semantic IconButton contract. PR Dialog backdrops and surfaces always paint
above the complete AI Review panel. Finding rows use 14 px GitHub body text,
8 px vertical spacing, and content-derived heights so wrapped explanations and
suggestions are never clipped at any allowed panel width.
Pointer presses anywhere in the visible AI Review panel, including formatted
`StyledText` prose and links, participate in the shared outside-dismiss channel
after the target receives the event. Input bridges publish only generic window
coordinates; the composition root routes those coordinates to the currently
mounted surface. On macOS the existing application-local `NSEvent` monitor
covers formatted text consumed by the Slint backend. The reusable AI Review
panel remains independent of AppKit, PR routes, and popover implementations.
PR and status-bar popovers therefore close without blocking the message action.

PR 详情左侧持续显示 Files changed diff；右侧面板采用 GitHub Copilot Chat 的 PR 侧栏
信息结构：一个显示规则/模型上下文的紧凑标题栏，一个连续承载 Review 状态、摘要、
High/Medium/Low finding 与 Review 专属对话的滚动流，以及一个固定在底部的 composer。
`Ask` 与 `Review again` 通过 Primer 分段模式复用同一个 composer，不再堆叠两个输入框
或挂载第二个对话标题栏；即使输入框此前持有焦点，第一次指针点击也必须完成模式切换。
固定 composer 的画布不绘制贯穿圆角输入框背后的横线，只由输入框自身边框界定区域。
与 GitHub Pull Request 中的 Copilot Chat 一致，`Ask` 在任务排队、运行、完成、失败和取消
状态下均可使用。当前 PR 元数据与有界 diff 摘要是主要上下文；已完成的 AI Review 结果
只是可选附加上下文。对话历史仅保存在本地，并发送给 Settings 中配置的
OpenAI-compatible AI Agent。Ask 只读取 PR 上下文，绝不向代码托管平台提交评论、Review、
Approval 或其他写操作。
提交 Ask 后，用户消息必须立即追加到可见对话。等待回复期间，Agent 行显示“准备 PR
上下文”“正在思考”等有界运行阶段；这些阶段不得声称展示模型隐藏推理。Composer
主操作切换为与 Review 取消一致的紧凑 Stop 控件；点击后中止上下文读取或在途 Agent
completion，但不取消 AI Review 任务，并在当前对话中标记回复已停止。
Review 状态和每条等待中的 Ask 回复都显示一个紧凑的实时耗时。Review 从任务入队开始
计时，并在完成、失败或取消时冻结；Ask 从提交开始计时，成功的 assistant 消息会持久化
最终毫秒耗时，停止或失败的可见回复立即冻结实测耗时。
运行中的 Review 同时显示当前 diff chunk 和总 chunk 数。百分比由已完成 chunk 按处理阶段
最高 96% 推导，表示工作流进度，不是 Provider token 或模型生成进度。缺少 chunk 计数的
旧任务摘要继续回退显示原有百分比。
Review 处于排队或运行状态时，`Review again` composer 的主操作切换为带方块图标的可点击
可视圆形为 20px、交互热区为 24px 的紧凑反色 Stop 按钮，并取消该任务。PR 列表
保留 Copilot 操作，并在对应行任务活跃时于其右侧显示可视圆形为 16px、交互
热区仍为 24px 的 Stop 按钮。Conversation、Copilot 与 Stop 三个操作的中心间距必须保持均等。
点击 finding 会在现有 diff 中选择对应文件与行。面板遵循
Primer 密度并复用仓库自有 `GitHub*` 组件，不提供独立 Review 页面。分隔线提供常驻拖拽命中区：拖动会保留本次会话的
宽度偏好，在面板可读最小宽度与剩余 Diff 工作区之间夹紧；轻量 Diff viewport 与裁剪
边界持续跟随拖动，仅在释放鼠标后提交一次重型 Diff 排版。静止态的单像素分隔线只由
实时 Diff viewport 绘制；更宽的命中区只绘制 Hover/拖动态并在 Dialog 关闭后的第一次
按下时直接捕获拖拽。Files changed 工具栏使用
32 px Primer IconButton 中的官方 16 px Copilot Octicon 打开面板，右侧留白为 12 px；
Hover、按下、选中、键盘可见焦点和加载态统一遵循共享 IconButton 语义契约。PR Dialog
的遮罩与内容始终覆盖完整 AI Review
面板。Finding 行使用 14 px GitHub 正文字号、8 px 垂直间距和内容派生高度，确保在
任何允许的面板宽度下，换行后的说明与建议都不会被裁切。
AI Review 面板可见区域内的任意指针按下（包括 formatted `StyledText` 正文和链接）都在
目标完成事件处理后接入共享 outside-dismiss 通道。输入 bridge 只上报通用窗口坐标，由
composition root 路由给当前挂载 surface；macOS 复用已有的应用级 `NSEvent` monitor，覆盖
被 Slint backend 消费的 formatted 正文。可复用 AI Review 面板不依赖 AppKit、PR 路由或
具体浮层实现；PR 与状态栏浮层会关闭，同时不拦截消息自身的链接或操作。

## Agent response projection / Agent 回复投影

Assistant messages in the formatted production variant render native structured
blocks for H1-H6, paragraphs, ordered/unordered/task lists,
strong/emphasized/struck text, inline code, quotes, rules, tables, and fenced
code. The compact renderer follows Codex desktop's system-font conversation
tokens rather than the full Preview document scale: a 14 px body at weight 445 in a 22 px
line box; 24/20/17/15 px H1-H6 steps at weight 600; 20 px between adjacent
paragraphs; 10 px between prose and a list group; zero extra space between
consecutive list items; a 24 px hanging-list gutter; subtle neutral code
surfaces with 10 px no-label/32 px labeled top insets and 10 px bottom padding;
equal responsive left/right message insets; and the registered platform code font.
Fenced code and its language label explicitly use regular weight 400 and an
upright face; they do not inherit the prose weight or select the registered
italic font face.
Codex's browser window prefers its bundled OpenAI Sans, but the default Codex
Light/Dark desktop themes leave the Electron UI family unset and inherit the
native system UI stack. Usher follows the desktop path: response prose directly
uses the application platform face. On macOS this resolves Latin and Chinese
through SF Pro and PingFang without an unrelated line-metric anchor. Codex's
document-level weight 445 is applied at Usher's root `Window`, allowing ordinary
`Text` and Slint 1.17 `StyledText` to use the same inherited value while preserving
Markdown formatting. Fenced blocks carry a
worker-computed line count so their
minimum height is independent of Slint font fallback; headings and prose retain
explicit glyph-height safety for mixed CJK/Latin/bold runs. The first heading becomes the compact
artifact action in place; a bounded fallback title is appended only when no
leading heading exists. The action always opens the original, untruncated Markdown through the shared
Artifact Preview. User messages remain raw text and render as content-sized,
right-aligned neutral bubbles without repeated avatar/name chrome. The controlled `baseline`
variant also keeps assistant messages on the former raw-text path. The A/B keeps
the persisted conversation source and fixture content identical; only projection
and Slint presentation-model shape differ.

生产默认的 formatted 变体把 Assistant 消息渲染为原生结构 block，覆盖 H1-H6、段落、
有序/无序/任务列表、粗体/强调/删除线、行内代码、引用、分隔线、表格和围栏代码。
紧凑 renderer 使用 Codex 桌面端的系统字体对话 token，而不是完整 Preview 的文档尺度：
14 px、字重 445 的正文与 22 px 行盒；字重 600 的 24/20/17/15 px H1-H6；相邻段落 20 px；正文与
列表组 10 px；连续列表项不额外加间距；24 px 悬挂列表 gutter；低对比中性代码
Surface（无标签/有标签顶部空间分别为 10px/32px，底部 10px）；响应式左右消息留白
保持相等；以及平台注册的代码字体。Codex 的 browser 窗口优先使用内置 OpenAI Sans，
但默认 Codex Light/Dark 桌面主题没有指定 Electron UI 字体，而是继承原生系统 UI 字体栈。
Usher 对齐桌面端路径，回复正文直接使用应用的平台字体；macOS 上拉丁字和中文分别由
SF Pro 与 PingFang 显示，不再借用无关字体作为行盒锚点。Codex 文档级的字重 445 设置在
Usher 根 `Window`，让普通 `Text` 与 Slint 1.17 `StyledText` 继承同一个值，同时保留
Markdown 格式。
围栏代码及其语言标签显式使用 400 常规字重和正体，不继承正文 445 字重，也不得选中
已注册的 Italic 字体 face。
Worker 为围栏代码提供行数，使最小高度不依赖 Slint 字体 fallback；标题和正文保留显式
字形高度安全量，CJK/Latin/粗体混排不得裁切。首个标题原位作为 Artifact
action；仅当没有 leading heading 时才追加有界 fallback。点击标题始终把未截断的原始 Markdown 交给共用
Artifact Preview。User 消息继续显示原始文本，并使用内容自适应宽度、右对齐的中性
气泡，不重复显示头像或姓名栏。受控 `baseline` 变体也让 Assistant
消息走旧的原始文本路径。A/B 保持持久化 conversation source 与 fixture 内容一致，
只有投影和 Slint 展示 model 的形状不同。

`src/agent_response/` parses on a worker through `usher-preview`'s structure-only
entry. That entry does not initialize or query code-highlighting loaders;
fenced-code language and source are retained, but highlighted `code_spans` stay
empty. Each inline projection is limited to 16 KiB of UTF-8 block text and 48
blocks. Truncation is Unicode-safe, is disclosed by the UI, and never modifies
the original response used by full Preview. The width-independent cache key
contains stable message identity plus content identity; the cache is bounded to
128 entries and an approximate 2 MiB accounted projection-payload budget,
excluding container and allocator overhead, and evicts least-recently-used projections. Window or
divider resize may reflow native text but must not parse Markdown, replace the
conversation model, or initialize syntax-highlighting repositories.

History projection has two explicit budgets. The newest 32 assistant rows retain
the complete structured compact projection; the preceding 96 assistant rows use
one marker-free plain projection plus the full-Preview action. Assistant rows
older than the 128-row projection window stay on the raw history path. This keeps
structured projection and cache work bounded while preserving the most relevant
conversation structure; it does not cap persisted conversation length or total
Slint row count.

`src/agent_response/` 在 Worker 上通过 `usher-preview` 的纯结构入口解析；该入口不
初始化也不查询代码高亮 loader，围栏代码保留语言和源码，但高亮 `code_spans` 为空。
每条行内投影最多包含 16 KiB UTF-8 block 文本和 48 个 block；截断必须 Unicode 安全、
在 UI 中明确提示，并且不得改变完整 Preview 使用的原始回复。宽度无关缓存 key 包含稳定
message identity 与 content identity，缓存最多 128 项，投影 payload 采用近似 2 MiB
记账预算（不包含容器与 allocator 开销），并按最近最少使用顺序淘汰。窗口或分隔线 resize 可以触发原生文字重排，但不得解析 Markdown、替换
conversation model 或初始化语法高亮 Repository。

历史投影使用两级明确预算：最近 32 条 Assistant 行保留完整结构化紧凑投影；此前 96 条
Assistant 行使用一个去标记的纯文本投影并保留完整 Preview action；超过 128 条投影窗口
的更早 Assistant 行继续使用 raw 历史路径。该预算限制的是结构化投影与缓存工作，不限制
持久化 conversation 长度或 Slint 总行数。

Safe relative `.md` and `.markdown` references are actionable whether authored
as an inline-code span such as `AGENTS.md` or as a normal Markdown link. The
projection replaces each accepted path with an opaque token; the visible Slint
label may remain the authored title, but its link target/callback carries only
the token, which is valid only in the current task projection. Absolute paths,
drive prefixes, parent traversal,
non-Markdown extensions, controls, and unsupported URI schemes never create a
repository action. HTTP(S) links remain ordinary external links.

安全的相对 `.md`/`.markdown` 引用无论写成 `AGENTS.md` 这类行内代码，还是普通
Markdown link，都可触发操作。投影把每个接受的路径替换为不透明 token；Slint 显示
标题可以保留原文，但 link target/callback 只携带 token，且 token 只在当前 task 投影
中有效。绝对路径、盘符前缀、父目录
穿越、非 Markdown 扩展名、控制字符和不支持的 URI scheme 都不得创建仓库 action；
HTTP(S) link 继续作为普通外部链接。

Activating a repository token first looks it up in the current task's action map
and binds it to that task's local repository, never to a later selected project.
The controller resolves the token and task binding; `src/agent_response/` and
the Preview facade independently reject unsafe path syntax. On Unix the facade
opens the canonical repository root once, traverses each relative component
from that directory descriptor with no-follow semantics, and reads the verified
regular-file handle. On Windows it compares the final paths of the opened root
and target handles. The same target handle enforces the Preview source-size and
UTF-8 limits before reading. Failure is bounded and does not fall back
to an external opener or another project. In Docked mode the resulting Preview
temporarily occupies the same PR trailing slot and width as AI Review, so the
conversation stays mounted and the diff width is reserved exactly once. Closing
Preview restores the conversation. Detached mode opens the same prepared payload
in the independent Preview window while AI Review remains visible.

激活仓库 token 时先在当前 task 的 action map 中查表，并绑定到该 task 的本地仓库，
绝不使用之后选中的项目。Controller 负责 token 查表与 task 绑定；`src/agent_response/`
和 Preview 门面分别拒绝不安全路径语法。Unix 上，门面只打开一次 canonical 仓库根，
再从该目录句柄逐 component 以 no-follow 语义遍历并读取已验证的普通文件句柄；Windows
比较已打开根目录与目标句柄的最终路径。同一目标句柄执行 Preview 来源大小与 UTF-8
限制后再读取。失败信息必须有界，不得回退到外部打开器或其他
项目。Docked 模式让结果 Preview 临时占用 AI Review 相同的 PR trailing slot 和宽度，
conversation 保持挂载且 Diff 宽度只预留一次；关闭 Preview 后恢复对话。Detached 模式
在独立 Preview 窗口显示同一 prepared payload，同时 AI Review 继续可见。

## Response acceptance protocol / 回复验收协议

Acceptance uses one Release binary and records its SHA-256 before either cell.
`USHER_AI_REVIEW_RESPONSE_VARIANT=baseline|formatted` selects the raw or
structured projection at runtime; production defaults to `formatted`.
`USHER_AI_REVIEW_RESPONSE_FIXTURE=markdown-v1` mounts deterministic,
non-persistent PR metadata and multiple response messages containing headings,
lists, emphasis, inline/fenced code, external links, repository Markdown
references, and enough bounded repetition to exercise scrolling. The harness
enables offline mode, isolates platform config/data per variant and run, and
skips startup network probes and automatic update checks. The direct-stdout
settled marker must prove exact row/assistant-byte counts, a zero structured
shape for baseline, and non-zero formatted blocks/actions/cache. A second marker
must prove that the response scroll view consumed wheel input. The fixture must
not write review/session persistence.

验收只使用一个 Release 二进制，并在两个 cell 运行前记录其 SHA-256。
`USHER_AI_REVIEW_RESPONSE_VARIANT=baseline|formatted` 在运行时选择原始文本或结构
投影，生产默认值为 `formatted`。`USHER_AI_REVIEW_RESPONSE_FIXTURE=markdown-v1`
挂载确定且不持久化的 PR 元数据和多条回复，覆盖标题、列表、强调、行内/围栏代码、
外部链接、仓库 Markdown 引用，以及足以触发滚动的有界重复内容。Harness 启用离线
模式，按 variant/run 隔离平台配置与数据目录，并跳过启动网络探测和自动更新检查。
直接写 stdout 的 settled marker 必须证明精确行数与 Assistant 字节数、baseline 的零
结构形状，以及 formatted 的非零 block/action/cache；第二个 marker 必须证明回复滚动区
实际消费了滚轮输入。Fixture 不得写入 Review/Session 持久化。

The macOS RSS protocol runs at least three isolated processes per variant from
that same binary, fixes the native window at 1280x860, waits for the fixture,
applies the same balanced wheel-scroll cycle, then samples settled RSS, process
RSS sample maxima, and physical footprint. Raw samples, run metadata, logs, binary hash,
baseline/formatted medians, and candidate-minus-baseline absolute and percentage
deltas must be retained. The acceptance gate evaluates both absolute and
relative bounds independently: formatted increase must be no more than 5% and
no more than 8 MiB for both settled median RSS and physical footprint. The RSS
"peak" is only the maximum of the discrete sample window and remains diagnostic;
startup samples are diagnostic and are not mixed
into settled samples. No improvement claim may rely on a Debug build, one
process, subjective inspection, or averages that hide a regression.

macOS RSS 协议从同一二进制为每个变体启动至少 3 个隔离进程，把原生窗口固定为
1280x860，等待 fixture settle，施加完全相同的往返滚轮滚动，再采样稳定 RSS、进程
RSS 离散采样最大值和 physical footprint。必须保留原始样本、run metadata、日志、二进制哈希、
baseline/formatted 中位数，以及 candidate-minus-baseline 的绝对值和百分比差异；验收
门禁要求 formatted 的 settled RSS 中位数和 physical footprint 同时满足增幅不超过
5% 且不超过 8 MiB。所谓 RSS “peak”仅是离散采样窗口的最大值，只作诊断；Startup
样本也不得混入 settled 样本。任何改进
结论都不得只依赖 Debug 构建、单进程、主观观察或掩盖回退的平均值。

Mock and UI acceptance additionally prove the formatted/baseline model shape,
block/link counts, Unicode-safe bounds, cache hits and eviction, repository-path
rejection and symlink containment, and deterministic fixture persistence behavior.
Headless Slint interaction must show response-title and repository-link callbacks
fire. Visual UI tests cover narrow and wide PR geometry, wheel scrolling,
Docked replacement and restore without conversation loss, Detached coexistence,
panel/route cleanup, and PR Dialog layering; screenshots are retained for the
formatted fixture where the platform test harness supports capture.

Mock 与 UI 验收还必须证明 formatted/baseline model 形状、block/link 数量、Unicode
安全边界、cache hit/淘汰、仓库路径拒绝与 symlink containment，以及 fixture 不持久化
行为均为确定结果。Headless Slint 交互必须证明回复标题和仓库 link callback 能触发。
视觉 UI 测试覆盖窄/宽 PR 几何、滚轮滚动、Docked 替换与无 conversation 丢失恢复、
Detached 共存、面板/路由清理及 PR Dialog 分层；平台测试环境支持截图时，应保留
formatted fixture 截图。

Executable acceptance commands / 可执行验收命令：

```sh
cargo fmt --all
./scripts/ui-check.sh
./scripts/check-ui-policy.sh
cargo check --locked
cargo check --locked --no-default-features --features renderer-femtovg
cargo test -p usher-preview --locked
cargo test --locked --test ai_review_response_ui
cargo test --lib --locked
cargo build --release --locked
git diff --check
./scripts/benchmark-ai-review-response.sh --runs 3 --no-build
```

The final command must reuse the already-built Release binary; the harness records
and verifies that both cells use the same SHA. / 最后一条命令必须复用已构建的 Release
binary；Harness 会记录并核对两个 cell 的同一 SHA。

## Build and failure behavior / 构建与失败

`ai-code-review` includes the core package and full UI/settings bridges. It is
enabled in the default product feature set and can be excluded with
`--no-default-features --features renderer-femtovg`; disabled UI packages keep
the root Slint contract compilable without the task engine.

Network, invalid model configuration, unreadable rule files, empty diffs, and
invalid provider JSON fail only the affected task. User-facing task errors are
bounded and credentials, complete prompts, source payloads, and rule contents
must never be logged. Explicitly cancellable AI Review and review-scoped Ask
completions have no total request deadline; the shared 10-second connect timeout
and a 300-second response-body idle timeout still apply. Every received body
chunk resets that idle timer by default. Non-streaming JSON Review/Ask requests
select a request-scoped semantic policy under which whitespace-only provider
heartbeats do not count as body progress; the first non-whitespace byte and
subsequent non-whitespace chunks reset the same deadline. This policy is based
on the expected response representation, not a provider host or model name.
Other AI completions retain the shared 30-second
total and body-idle timeouts. Ordinary application diagnostics record the task
stage, chunk index, completion message counts and byte totals, serialized request
body size, response framing, elapsed time, and bounded response-body chunk sizes,
arrival intervals, and progress classification. They never record authorization
data, prompt/rule/source content, or provider response content.

`ai-code-review` 包含核心包和完整 UI/Settings bridge；默认产品 feature 会启用它，
可通过 `--no-default-features --features renderer-femtovg` 排除。关闭时使用协议兼容的
UI stub，根 Slint 仍可编译。

网络、模型配置、规则文件、空 diff 或 Provider JSON 错误只影响对应任务。用户可见错误
必须有界，日志不得包含凭据、完整 Prompt、源码 payload 或规则正文。可显式取消的
AI Review 与 Review 专属 Ask completion 不设置请求总时限，但仍受共享的 10 秒连接
超时和 300 秒响应体空闲超时约束。普通请求默认按任意 body chunk 重置空闲计时；
非流式 JSON Review/Ask 请求会选择
请求级语义策略，Provider 仅发送空白心跳时不算正文进度，首个及后续非空白字节才重置
同一截止时间。该策略依据预期响应表示，不依据 Provider host 或模型名称。其他 AI
completion 继续使用共享的 30 秒总时限和响应体空闲时限。普通应用诊断会记录任务阶段、
chunk 序号、completion 消息数量与字节总量、序列化请求体大小、响应 framing、耗时，以及
有界的响应体 chunk 大小、到达间隔和进度分类；绝不记录鉴权数据、Prompt/规则/源码内容
或 Provider 响应正文。

## Regression checklist / 回归检查

- Queue more tasks than the configured limit with `USHER_AI_REVIEW_FIXTURE=review-v1`;
  verify the running high-water equals the configured limit and queued tasks
  start as slots become free.
- Start from the PR list and verify it remains visible. Open a completed task and
  verify PR detail selects Files changed and opens the panel.
- Select a finding and verify the matching file and changed line are revealed.
- Send a follow-up question, add one-off requirements, and run Review again.
- Send an Ask question and verify the user row appears before the Agent
  completes. Verify the pending Agent row advances from context preparation to
  thinking, its elapsed timer advances in real time, the Send action becomes
  Stop, and Stop ends only that response with a frozen duration.
- Let a review run for more than 300 seconds and verify it continues until the
  provider completes or the user cancels it. Verify ordinary AI Chat still
  reports a timeout at the shared 30-second deadline.
- While a multi-chunk review runs, verify the panel identifies the current and
  total chunk. Stall a successful response body and verify the task fails after
  300 seconds of no body progress rather than remaining at one percentage.
- Cancel or fail a review before a session is created, then focus Ask and send
  a question. Verify the configured Agent answers using PR context and that no
  hosting-provider write request is issued.
- Focus each composer input, then switch between `Ask` and `Review again` with
  one click. Verify the mode changes on that click and no horizontal rule is
  visible behind the rounded input.
- Start a review in `Review again` mode; verify the composer action changes to
  an enabled circular Stop button and cancels the task on the first click.
- Start a review from the PR list; verify the same Stop button appears to the
  right of Copilot and cancels without opening PR detail.
- Delete a rule used by a project and verify that project now inherits the
  repaired default.
- Try relative, missing, non-Markdown, directory, and oversized rule paths;
  verify each remains in the form with an inline error and no rule is added.
- Change the concurrent review limit repeatedly; verify the rule list keeps the
  same rows and order while only the numeric limit changes.
- Close the panel and leave PR Review while work runs; verify both actions clear
  projected models, the Review task still completes, and the current Ask is
  cancelled without committing a late response into a reopened mount.
- Drag the PR/AI Review divider to both limits and resize the window. Verify the
  pointer remains attached to the divider, the session preference survives a
  close/reopen, and the diff performs its final layout after release.
- Open each PR/status-bar popover, then click formatted AI Review prose and a
  link. Verify the popover closes and the link action still runs.
- Mount `markdown-v1` in both response variants. Verify user rows and baseline
  assistant rows are raw, while formatted assistant rows expose the expected
  blocks, repository actions, artifact titles, and truncation state.
- Activate `AGENTS.md` and a nested Markdown reference. Verify Docked Preview
  replaces the AI surface without clearing its conversation, Preview close
  restores it, unsafe/symlink-escape targets are rejected, and the target stays
  bound to the task repository after the selected project changes.
- Switch to Detached and activate the same response. Verify AI Review remains
  visible, then close the panel and leave PR Review; projected actions are
  cleared while the Detached Preview and background work remain available.
- Compile with and without `ai-code-review`, run library tests, and record
  feature build-time/binary and first-use/close-settled RSS measurements.

- 使用 `USHER_AI_REVIEW_FIXTURE=review-v1` 入队超过上限的任务，确认运行高水位等于
  配置值，空出 slot 后队列继续启动。
- 从 PR 列表启动后确认仍停留列表；打开完成任务后进入 PR 详情 Files changed 并打开面板。
- 点击 finding，确认对应文件和变更行被揭示。
- 发送追问、增加一次性要求并再次 Review。
- 发送 Ask 后确认用户行在 Agent 完成前立即出现；确认等待行从准备上下文进入思考状态，
  实时耗时持续增长，Send 操作切换为 Stop，且 Stop 只终止该次回复并冻结耗时。
- 让 Review 运行超过 300 秒，确认 Provider 完成或用户取消前任务继续运行；同时确认普通
  AI Chat 仍在共享的 30 秒截止时间触发超时。
- 多 chunk Review 运行时确认面板显示当前 chunk 与总数；让成功响应的 body 停止产生
  进度，确认任务在 300 秒后失败，而不是永久停在百分之一。
- 在 Session 创建前取消 Review 或让其失败，再聚焦 Ask 并发送问题；确认配置的 Agent
  使用 PR 上下文回答，且不会发出任何代码托管平台写请求。
- 分别聚焦两个 composer 输入框，再用一次点击切换 `Ask` / `Review again`；确认该次
  点击立即切换模式，且圆角输入框背后没有横向规则线。
- 在 `Review again` 模式启动 Review，确认 composer 主操作切换为可点击的圆形 Stop
  按钮，并在第一次点击时取消任务。
- 从 PR 列表启动 Review，确认相同的 Stop 按钮显示在 Copilot 右侧，并且无需打开 PR
  详情即可取消。
- 删除项目正在使用的规则，确认项目改为继承修复后的默认规则。
- 分别输入相对路径、不存在路径、非 Markdown 文件、目录和超限文件，确认表单保留输入、
  显示行内错误且不会新增规则。
- 连续调整并发 Review 数量，确认 rules 列表的内容与顺序均保持不变，只有并发数字变化。
- 工作运行时关闭面板或离开 PR Review，确认投影 model 被清空、Review task 仍能完成，
  当前 Ask 被取消，且迟到回复不会提交到随后重开的 mount。
- 将 PR/AI Review 分隔线拖到两侧边界后调整窗口大小，确认指针始终贴住分隔线、关闭再
  打开仍保留本次会话宽度偏好，且 Diff 只在释放指针后采用最终排版。
- 分别打开 PR/状态栏浮层，再点击 formatted AI Review 正文与链接；确认浮层关闭且链接
  操作仍正常执行。
- 在两个回复变体中挂载 `markdown-v1`；确认 User 行和 baseline Assistant 行显示原始
  文本，而 formatted Assistant 行暴露预期 block、仓库 action、Artifact 标题和截断态。
- 激活 `AGENTS.md` 与嵌套 Markdown 引用；确认 Docked Preview 替换 AI Surface 时不
  清空 conversation，关闭 Preview 后恢复，危险路径/symlink 逃逸被拒绝，且切换当前
  项目后目标仍绑定到原 task 仓库。
- 切换到 Detached 后激活同一回复；确认 AI Review 仍可见。随后关闭面板并离开 PR
  Review，确认投影 action 已清空，而 Detached Preview 和后台工作继续存在。
- 分别编译启用/关闭 feature，运行库测试，并记录编译时间/产物及首次使用/关闭稳定 RSS。
