# Application Hooks / 应用级 Hook

## Scope / 范围

`src/hooks/` owns the shared event registry, configuration, execution, cancellation,
bounded run history, journals, and Settings/Job Run bridges. PR review submission
and optional AI Review completion are the first sources. Other modules register a
versioned `EventDefinition` and call `hooks::emit(HookTrigger)` from their worker.
There is no remote-provider polling or automatic script installation.

`src/hooks/` 统一管理事件注册、配置、执行、取消、有界运行历史、日志和设置/Job Run
桥接。人工评审提交和可选的 AI Review 完成是首批来源。其他模块注册带版本的
`EventDefinition`，在后台通过 `hooks::emit(HookTrigger)` 发布事件。
本机制不轮询远端平台，也不自动安装脚本或运行时。

## Configuration / 配置

Host failures carry an optional stable `failure_kind` in run summaries and
terminal journal records. Settings and Job Run resolve friendly catalog copy
from that kind, conclusion, exit code and signal. Technical `detail` and external
stdout/stderr stay untranslated in the journal. Legacy summaries without a kind
use their typed conclusion; unknown future kinds use a generic failure message.
Validation returns typed categories, including integer/range timeout errors.
Runtime locale changes refresh mounted history, test feedback and Job headers/
step details without rebuilding the log model. New editors clear old feedback.

宿主失败在运行摘要和终态日志中保留可选的稳定 `failure_kind`；Settings 和 Job
按该类别、结论、退出码及信号投影友好文案。技术 `detail` 和外部 stdout/stderr
在日志中保留原文。无类别的旧记录按类型化结论降级，未知新类别使用通用失败提示。
表单校验返回类型化错误；切换语言立即刷新历史、测试反馈、Job 摘要及步骤说明，
不重建日志 model；新编辑器清空旧反馈。

New Hook notifications persist versioned semantic metadata (name, event, stage,
test flag and conclusion), never rendered translations. Only application/Hook
messages carrying this version are localized by Inbox, Banner and the native
delivery adapter. Old/unrecognized messages retain their original publisher copy.
Mounted banners refresh with the locale; already delivered OS notifications are
immutable. No script output or arbitrary remote message is classified by its text.

新 Hook 通知只保存带版本的语义元数据，不保存翻译结果。Inbox、Banner 及系统通知
发送适配器只翻译来源匹配且版本已知的应用 Hook 消息；旧通知和未知消息保持原文。
已挂载 Banner 随语言刷新，已发出的系统通知不会重发或修改。

Event and execution-history pickers use separate mounted option trees.
Rows use the shared Select 32 px geometry and explicit indexed positions;
single-option popups fit their content, and bottom-edge popups open above the
trigger. / 事件、执行历史选择器使用各自常驻的选项树；每行按共享 Select
的 32px 行高显式定位，单选项弹窗按内容收缩，靠近底部时在触发框上方展开。
Event/category captions and saved-definition descriptions retain semantic event
IDs in state and are re-projected for the active locale after every runtime
language switch, including an already-open editor and picker. / 事件/分类标题及
已保存定义描述在状态中保留语义事件 ID；每次运行时切换语言后都按当前 locale
重新投影，包括已经打开的编辑弹窗和选择菜单。

- Settings has a separate **Hooks** tab immediately before **Diagnostics**, in
  both wide navigation and the compact menu. It supports a global switch and up
  to 32 named definitions with independent switches, event, runtime, executable,
  one script, and a 1-3600 second timeout.
- Saving the global switch uses switch-local progress. It does not clear status,
  disable, dim, or reflow the execution-history controls.
- Each saved-definition row uses a Switch to update that Hook directly. The row
  save has its own progress state and does not rebuild or move execution history.
  While a row save is pending, only that row shows progress; other row Switches
  temporarily reject input without changing opacity or checked appearance.
  When the global switch is off, every row Switch appears off and is disabled
  while retaining its configured value. Turning the global switch back on restores
  each row's configured status. Edit/Delete actions remain available.
- Every registered event declares a stable module category. Saved definitions
  are grouped by that category using Primer ActionList headings and one bordered
  row list per group. The editor's event Select uses the same category order and
  headings; headings are not keyboard options, and entering a group exposes its
  label to assistive technology. Unknown legacy events remain visible under Other.
- The tab lists saved definitions with Edit/Delete icon actions. New and Edit
  use the shared `GitHubDialog`, matching Project dialogs with a fixed footer
  and scrollable form. Cancel/Close discard unsaved drafts; Save closes only
  after successful persistence. Validation and test errors remain in the dialog.
  Busy operations disable closing; a running test can be stopped. Delete requires
  a confirmation dialog and preserves execution history. The form stays mounted
  across dialog opens to preserve the Settings factory binding contract.
- Project Add/Edit persists explicit `hook_ids` in the Project Store. New and
  legacy projects default to empty; newly created Hooks never opt in existing
  projects. **Trigger Hooks** starts unchecked with the list collapsed. Checking
  selects all enabled rows and expands; partial enabled selection shows a mixed
  mark; clearing the parent (including mixed) clears enabled and retained disabled
  selections, then collapses. Edit restores saved IDs. Disabled Hooks and removed
  IDs are presented unchecked with disabled row/checkbox interaction and never
  execute. Their prior selection intent remains in the transient form and durable
  `hook_ids`, so enabling the same Hook restores its prior selected or unselected
  state; a newly created project has no retained selection. The shared
  `GitHubCheckbox` also appears in UI Components with interactive and disabled
  state examples.
- The project selector uses an inline parent/count header and a single bordered
  list with separated rows. Names, localized event captions and status Labels
  have distinct positions; known events do not expose internal versioned IDs.
  Checkbox hover changes the box outline, without a row-sized background pill.
- Project/outcome filter inputs are removed from the editor. New definitions use
  no additional filters; saved legacy values still round-trip and constrain
  dispatch, so editing a Hook does not silently widen its trigger scope.
  Save commits a new definition revision. Test executes the current unsaved draft
  with sample event data, even when either switch is off or the project has not
  selected the Hook. It does real work with
  the current user's permissions, not a sandboxed simulation.
- Runtime selection uses mutually exclusive shared `GitHubRadio` controls.
  Shell and Node interpreter fields are fixed mounted leaves with literal
  placeholders, following the Settings factory binding contract (UI-111). Their
  inherited generic placeholder is disabled so responsive factory reuse cannot
  paint another Settings field's text beneath the literal leaf.
  The mounted editor shows only the selected language's source and executable.
  JavaScript and TypeScript share one `JavaScript/TypeScript` runtime backed by
  Node's TypeScript-capable module execution. Switching between Shell and Node
  saves and restores separate temporary runtime drafts; first visits to the other
  runtime start empty. Save/Test consume only the active draft. Persisted
  definitions still contain one runtime and one source; reopening another/new
  Hook clears all inactive drafts. Legacy JavaScript and TypeScript values load
  as the combined runtime.
  Source, event, runtime and execution-parameter edits invalidate prior test
  feedback.
- Tests run in a temporary directory; normal dispatch uses the triggering
  project's working directory. The Settings form does not expose a test CWD.
- A test has its own run ID, explicit test markers,
  duplicate-start protection, Stop, a detailed result retained while editing, and
  a link to execution details. Empty source, invalid timeout, executable/CWD
  errors, syntax/runtime errors, exit status, signal, and bounded stderr remain
  distinguishable. No credential from Usher Settings is injected into scripts.
- The Dialog uses a compact form with a fixed test area above Save/Cancel.
  Running/passed/failed/cancelled feedback uses an icon and short status beside
  Test; only errors expand a bounded, scrollable detail below the action row.
  Success does not add a banner or move the form. Stop and execution details use
  labelled icon controls. Event captions are localized, not internal event IDs.
- Hook action tooltips use literal action leaves and permanently mounted literal
  surfaces at the Settings factory root, sharing viewport flip/shift geometry.
  No dynamic tooltip label crosses a recycled Hook row (UI-103).
- The history picker has a trailing clear action with confirmation. Clearing
  removes completed summaries and journals, preserving queued/running tasks and
  terminal snapshots whose runner still owns the journal. Mutations persist on
  workers; cleanup failures report the affected journal. Cleared run links expire.
- Add/Edit supports Cmd+Enter on macOS / Ctrl+Enter elsewhere to save, Esc to
  cancel, and ordinary Tab/Enter/Space button navigation. Plain Enter stays a
  newline in the source editor. Busy states block save/close shortcuts;
  destructive confirmation dialogs do not use the save shortcut.
- The multiline source editor owns its vertical viewport, so wheel input over
  the code surface scrolls the source without moving the dialog form. A single
  permanently mounted native `StyledText` supplies Primer syntax roles (red
  keywords, blue strings/constants, purple entities and muted comments); no
  per-token UI nodes are created. Highlighting is bounded to at most 256 lines,
  32 KiB of source and 2,048 token runs. If a script exceeds any bound or parsed
  rich text differs from the source, native plain text remains fully readable.
  Native text and rich text share the same monospace font and standard
  ScrollView content geometry, including the full first/last lines and padding.
  Native input stays above rich text and owns pointer/editing interaction. Its
  text retains alpha 1/255 during highlighting, so FemtoVG does not skip the
  built-in 2 px cursor and selection. macOS uses an independent selection-colored
  caret. Platforms whose caret follows text color use opaque native plain text
  while focused and restore highlighting on blur. Never use alpha zero, or move
  interactive rich text above the input where it consumes code-surface clicks.
  Opening paints the Dialog before hydrating saved source after a 24 ms timer;
  a reusable 90 ms debounce prepares syntax colors. Editing and runtime changes
  immediately clear stale rich text. Closing clears the projection and all
  temporary source drafts; there is no resident highlighter engine/language cache.
- `HookStateFacade` hides `usher-state-store`. Typed actions execute on workers;
  automatic commit persistence atomically writes private `hooks.json` in the
  configuration directory. The file contains definitions and the latest 64 run
  summaries. Output is omitted from state persistence. Corrupt/oversized input is
  preserved and disables configuration/execution rather than overwriting it.
  Definitions and event registration are immutable shared snapshots; configuration
  edits use copy-on-write. Output-only sequence/tail commits still notify live
  subscribers but do not rewrite hooks.json. Configuration and durable run-summary
  changes advance a separate persistence revision; terminal commits participate
  in the same automatic worker flush/shutdown barrier. No manual save path is added.
  UI delivery coalesces snapshots before preparing the selected Job projection;
  Settings/history refresh only on durable/catalog changes, and project choices
  only on catalog changes. One bounded live-tail parser cache reuses unchanged
  rows; a different run drops that cache. Runtime locale changes refresh the
  existing Job row text without replacing its log model.

- 设置页有独立的 **Hooks** Tab，位于 **Diagnostics** 前；宽屏和窄屏顺序一致。
  支持总开关、最多 32 个有独立开关的 Hook、事件、运行方式、解释器、
  脚本及 1-3600 秒超时。
- 总开关使用自身的保存中状态，不清空状态文字，也不禁用、淡化或移动执行历史控件。
- 每条已保存定义使用 Switch 直接切换该 Hook；单条保存使用自身的进度状态，不重建或
  移动执行历史。单条保存期间仅该行显示进度，其他行内 Switch 暂时拒绝输入，但透明度和
  选中外观保持不变。总开关关闭时，所有行内 Switch 显示为关闭且不可操作，但保留每条定义
  自身的配置；重新开启总开关后恢复各条目的原始状态，编辑/删除操作仍然可用。
- 每个已注册事件声明稳定的模块分类。已保存定义按该分类使用 Primer ActionList 标题和
  独立单边框行列表分组；编辑器的事件 Select 使用相同的分类顺序与标题。标题不参与
  键盘选项索引，进入新分组时向辅助技术提供分类名称；未知旧事件继续显示在“其他”。
- Tab 展示已保存定义与编辑/删除图标操作；新增和编辑复用 `GitHubDialog`，采用
  与项目弹窗一致的固定底部操作区及可滚动表单。取消/关闭丢弃未保存草稿，保存仅
  在持久化成功后关闭；校验和测试错误保留在弹窗内。忙碌时禁用关闭，运行中的测试
  可停止。删除需确认并保留执行历史。表单跨开关保持挂载，遵循 Settings factory
  绑定约束。
- 项目新增/编辑通过 Project Store 保存 `hook_ids`。新项目及旧数据默认空选，
  新建 Hook 不自动加入项目。“触发 Hook”默认未选且收起；勾选时只全选可用项并
  展开，可用子项部分选择时显示半选；取消总项（含半选）同时清除可用项与禁用项
  保留的选择并收起。编辑恢复保存的 ID。禁用 Hook 和已删除 ID 在界面中强制显示
  为未选中，整行及复选框均为 disabled，且不执行；临时表单与持久化 `hook_ids`
  仍保留其原选择意图，因此同一 Hook 恢复启用后会恢复禁用前的选中/未选中状态，
  新建项目则没有保留选择。共享 `GitHubCheckbox` 在 UI Components 展示交互和禁用状态。
- 项目选择器将父项和数量放在同一行，子项采用单一边框和分隔行，名称、可读事件
  说明、状态 Label 分开排布。已知事件不显示内部版本 ID；悬浮仅反馈方框描边，
  不绘制整行灰色圆角底色。
- 编辑器移除项目/结果筛选项；新增定义不附加限制，已有配置的旧过滤值继续保留
  并生效，避免编辑时意外扩大触发范围。草稿通过保存产生新
  revision。测试使用当前未保存的脚本和样例上下文，不受开关或项目勾选控制；这是以当前
  用户权限进行的真实执行，不是沙箱模拟。
- 运行方式使用共享 `GitHubRadio` 互斥单选；Shell/Node 解释器字段固定挂载并分别持有占位文字，
  禁用继承的通用占位节点，避免响应式 factory 复用时叠画其他设置项文字，遵循
  Settings factory 绑定约束（UI-111）。JavaScript 与 TypeScript 合并为一个由 Node
  TypeScript 模块执行能力承载的 `JavaScript/TypeScript` 运行方式。编辑区只显示
  当前运行方式的内容与解释器；Shell 与 Node 之间切换时分别暂存并恢复草稿，首次
  选择另一运行方式为空。保存/测试只读取当前选中项，持久化仍只有一种运行方式和
  一份内容；打开其他 Hook 或新建时清除所有临时草稿。旧 JavaScript/TypeScript
  值统一按合并后的运行方式加载。
  修改源码、事件、运行方式和执行参数会清除旧测试状态。
- 测试使用临时目录，正式触发使用来源项目的工作目录；表单不再提供测试 cwd。
- 测试有独立 run ID 和测试标识、防重复启动、停止、具体结果及流程
  入口。空脚本、超时参数、解释器/cwd、语法、运行时、退出码/信号和有界 stderr
  分别呈现。不会向脚本注入 Usher 设置里的凭据。
- Dialog 采用紧凑表单，测试区固定在保存/取消上方。运行中、通过、失败、取消
  使用测试按钮旁的图标与短状态；仅错误在操作下一行展开有界可滚动详情，成功
  不增加顶部 Banner、不推动表单。停止和执行详情使用带说明的图标按钮，事件
  显示本地化名称而非内部 ID。
- Hook 操作 Tooltip 采用独立固定文案叶子和设置页根层常驻文案 Surface，复用
  全窗口避让定位；动态文案不穿过列表复用节点（UI-103）。
- 执行历史右侧提供清空及确认操作，只清除已结束记录和日志，保留排队/运行及
  尚持有日志的收尾任务。Worker 自动持久化，日志清理失败明确报错，已清除流程链接失效。
- 新增/编辑支持 macOS Cmd+Enter、其他平台 Ctrl+Enter 保存，Esc 取消及
  Tab/Enter/Space 按钮操作。普通 Enter 在编辑区换行；忙碌时禁止快捷保存/关闭，
  删除和清空确认弹窗不接受保存快捷键。
- `HookStateFacade` 隐藏具体 Store；强类型 Action 和自动持久化均在 Worker 上。
  配置目录的私有 `hooks.json` 原子保存定义和最近 64 个运行摘要，不保存输出正文。
  损坏/超限文件保留原样并禁用操作，不静默覆盖。
  定义与事件目录使用不可变共享快照，配置修改才写时复制。纯 seq/日志尾部更新仍通知
  活跃订阅者，但不重写 hooks.json；配置及持久摘要变化推进独立持久化版本，终态
  继续通过同一自动 Worker 的 flush/退出屏障保存，不新增手工 save 路径。
  UI 先合并快照，再后台准备所选 Job；设置/历史只随持久摘要或目录变化刷新，项目
  选项只随目录变化刷新。仅缓存一个有界活动日志尾部并复用未变化的解析行，切换 run
  丢弃旧解析缓存。运行时切换语言更新原 Job 行文案，不替换日志 model。
- 多行源码编辑器拥有独立垂直视口，代码区上的滚轮直接滚动源码而不移动弹窗表单。
  单个常驻原生 `StyledText` 提供 Primer 语义色（红色关键词、蓝色字符串/常量、紫色
  类型及灰色注释），不创建逐 token UI 节点。高亮限制为最多 256 行、32 KiB 源码和
  2,048 个 token run；超过任一上限或解析后的富文本与源码不一致时，由原生编辑器
  以纯文本完整显示。原生文本与富文本共用相同等宽字体和标准 ScrollView 内容几何，
  包括完整首尾行及内边距。原生输入始终在富文本上方拥有指针与编辑交互，高亮时
  保留 1/255 文字 alpha，避免 FemtoVG 跳过原生 2 px 光标与选区。macOS 使用独立
  选区色光标；光标跟随文字色的平台聚焦时使用不透明原生纯文本，失焦后恢复高亮。
  禁止使用零 alpha，或把会接收代码点击的富文本放在输入上方。打开时先绘制
  Dialog，24 ms Timer 后注入已保存源码，
  复用 90 ms debounce 准备语法颜色。编辑或切换运行方式立即清除旧富文本；关闭时
  清空投影和各运行方式临时草稿，不常驻高亮引擎或语言缓存。

## Events and lifecycle / 事件与生命周期

The PR approval, PR merge, and AI review event payloads include `number`, `title`, `source_branch`,
`target_branch`, and `author` as strings, plus nullable `author_name` and
`author_email`. `author` retains the provider's existing PR author label (an
account or display name); it is the PR creator, not the reviewer or Git commit
author. Name/email come only from the PR detail's author/user object, including
`public_email` fallback. Missing values are JSON null; no profile lookup or
commit-email inference is performed. PR approval and merge capture the PR cache
before the remote mutation; AI completion uses the persisted task target captured
during context loading. Older AI targets default these optional fields to null.
Test samples include the same fields. Definitions saved with the legacy
`pr.review.submitted.v1` event migrate to `pr.review.approved.v1`; historical run
records keep their original event ID.

PR 审核通过、PR 合并及 AI 评审事件的 payload 共有字符串字段 `number`、`title`、`source_branch`、
`target_branch`、`author`，以及可空字段 `author_name`、`author_email`。
`author` 沿用平台已有的 PR 作者标签（账号或显示姓名），表示 PR 创建人，
不是评审人或 Git commit 作者。姓名/邮箱只读取 PR 详情中的 author/user 对象，
邮箱可回退到 `public_email`；缺失时为 JSON null，不额外查询用户资料，也不从
commit 推断邮箱。PR 审核和合并在远端操作前捕获 PR 缓存；AI 完成事件使用加载
上下文时捕获并持久化的任务目标。旧 AI 目标缺失字段默认 null。测试样例包含相同
字段。使用旧 `pr.review.submitted.v1` 的 Hook 定义在加载时迁移为
`pr.review.approved.v1`，历史运行记录保留原事件 ID。

| Event | Boundary | Outcome |
| --- | --- | --- |
| `pr.review.approved.v1` | Remote Approve submission succeeds, before generation-guarded UI delivery | `approved` |
| `pr.merged.v1` | Remote merge succeeds, before generation-guarded UI delivery; merge commit, squash, rebase, and fast-forward all use this boundary | `success` |
| `ai.review.completed.v1` | Session persisted and task completion committed | `success` (findings may exist) |

An immutable trigger contains event ID, source module, timestamp, optional
project/CWD, outcome, payload and test flag. Payloads are JSON data, never
interpolated into source or shell arguments. Event context is limited to 256 KiB.
Approval, merge, and AI completion are distinct events. Comment and Request changes
submissions do not emit the approval event. `pr.merged.v1` is emitted only after
the provider confirms the merge operation succeeded, independent of merge method.
Hooks never change or roll back the result of the completed source operation.

Registered events declare `project_scoped`; both review events require a project
ID. Every event carrying a project checks membership through the Project facade
using its normalized Workflow ID. Missing, unhydrated or ambiguous project
configuration fails closed. Application-scoped events without a project retain
global matching. Dispatch receipts explain unselected skips. The runner rechecks
membership after acquiring queue permits, before spawning the interpreter.

注册事件声明 `project_scoped`；两种评审事件均必传项目 ID。所有携带项目的事件
通过 Project 门面按归一化 Workflow ID 验证勾选；项目不存在、未加载或 ID 歧义
均不执行。不携带项目的应用级事件保留全局匹配。未选中原因写入 dispatch 回执；
Runner 获得排队许可后、启动解释器前再次验证勾选。

不可变上下文包含 event ID、来源模块、时间、可选项目/cwd、结果、payload 和测试
标识；JSON 数据不插入脚本或 Shell 参数，上下文上限为 256 KiB。Approve、实际合并
与 AI 完成是三个独立事件；Comment 和 Request changes 不发布审核通过事件。
`pr.merged.v1` 仅在平台确认合并成功后发布，merge commit、squash、rebase 和
fast-forward 使用同一边界。Hook 不改变已经结束的业务操作结果。

Host-authored lifecycle:

```text
hook.triggered -> hook.queued -> execution.started -> output*
              -> execution.finished -> hook.finished
```

Each record has `run_id`, `hook_id`, `event_id`, `seq`, `time_ms`, `kind`, and
structured data. `execution.started` means the interpreter was spawned, not that
the first user statement succeeded. Pre-spawn failures skip execution events and
still finish the Hook. Output cannot forge lifecycle transitions. Each stream
retains its read order; no exact stdout/stderr cross-stream order is promised.
An editor's entire script is one execution step, not one step per source line.

`emit` returns a `DispatchReceipt` with run IDs and skipped reasons. Upper layers
can call `wait`, `cancel`, subscribe to events, or replay `events_after(run, seq)`.
Broadcast subscribers must recover lag through the journal/current snapshot.
There is no automatic retry or hook-on-hook dispatch. Retained event ID + Hook ID
prevents repeated automatic execution. Crash recovery marks unfinished summaries
interrupted and never replays side effects; arbitrary external scripts do not
have an exactly-once guarantee.

每条宿主事件具有关联 ID、顺序号、时间、类型及结构化数据。解释器启动成功才发
`execution.started`，不代表首条用户语句成功；启动前失败仍产生 Hook 终态。
stdout/stderr 不能伪造生命周期，分别保持读取顺序，不保证两路的精确发生顺序。
整个输入框是一个执行 Step，不按行拆分脚本。

`emit` 返回 run ID 和跳过原因；上层可等待、取消、订阅或按 seq 回放。订阅落后时
从日志和当前快照恢复。默认不重试、不递归触发；保留期内 event ID + Hook ID
去重。崩溃后标记中断，不自动重放；外部副作用不承诺严格 exactly-once。

## Execution / 执行

- The shared Tokio runtime owns tasks, two concurrent execution slots, a bounded
  queue of 32 accepted runs, and per-project serialization. A queued definition
  is immutable; disabling/deleting it or removing project opt-in before spawn skips it. An already running
  process requires explicit cancellation. Source scripts are limited to 64 KiB.
- POSIX Shell uses `/bin/sh -e`; Windows uses noninteractive PowerShell without
  profiles. An explicit executable overrides the default. JavaScript/TypeScript
  uses external `node` with an `.mts` entry file. No Node, V8, TS compiler, npm install,
  `npx`, or long-lived JS process is bundled or started in the idle application.
- TypeScript uses Node's native type stripping: choose Node 22.18+ or a compatible
  newer release (tested with 24.15). It does not type-check or implement full
  tsconfig/TSX/enum transformation. Unsupported syntax returns Node's diagnostic.
  Entries are ES modules in a temporary directory; built-ins and explicit file
  URLs work. Relative imports resolve beside that entry, not relative to cwd.
- `USHER_HOOK_CONTEXT` points to a temporary JSON file; `USHER_HOOK_RUN_ID` and
  `USHER_HOOK_TEST` identify execution. The working directory comes from the
  captured event, or a temporary directory when the event has no project.
- stdout/stderr are drained concurrently with 4 KiB reads and a bounded channel.
  Memory keeps a 32 KiB tail; output admission counts serialized JSONL bytes
  (including metadata, escaping and newline) against 2 MiB per run and emits
  one `output.truncated`. Total journal size is bounded by 2 MiB + 64 KiB,
  reserving completion space. Each record is at most 64 KiB. Journals
  live at `hooks/runs/<run-id>.jsonl` in the data directory; their retention follows
  summaries. Diagnostic log switches affect mirrors, not execution history.
- Upper presentation owns start/end Inbox, Banner and native notifications; both
  notifications link to the same `hook-run:<id>` target. Job Run reuses the shared
  model and virtualized logs. Only event-loop closures touch Slint.
- Application shutdown drains Hook processes asynchronously before the existing
  ordered persistence/message/UI cleanup stages. Event-loop exit also awaits the
  same drain barrier, after the loop stops. Admission waits for startup hydration
  and orphan-journal pruning; shutdown closes admission and finishes outstanding
  configuration writes before stopping persistence.
- POSIX execution uses a private process group; Windows assigns a kill-on-close
  Job Object before releasing an stdin-gated bootstrap. Normal completion,
  cancellation and timeout terminate descendants as well as the interpreter.
  User scripts must not deliberately detach from the managed process group.

- 统一 Runtime 管理并发 2、最多 32 个待处理/运行任务及同项目串行。排队任务固定
  使用触发时的配置；启动前关闭、删除或取消项目勾选会跳过，运行中须显式取消。脚本上限 64 KiB。
- Shell 使用系统解释器；JavaScript/TypeScript 统一调用外部 Node.js 的 `.mts`。不捆绑
  Node/V8/TS 编译器，不自动安装依赖，不在空闲时常驻 JS 进程。
- TS 使用 Node 原生类型擦除，建议 22.18+（验证版本 24.15），不做类型检查或完整
  TS 转换。不支持的语法展示 Node 诊断。脚本为临时目录中的 ES module，相对 import
  相对于脚本文件；项目模块可通过显式 file URL 导入。
- `USHER_HOOK_CONTEXT` 指向临时 JSON 文件，另外提供 run ID 和测试标识；cwd
  来自触发时的项目，没有项目时使用临时目录。
- 两路输出并发读取，有界通道和 32 KiB 内存尾部；2 MiB 输出准入预算按完整 JSONL
  字节计算，包含字段、转义和换行；超限只标记一次并继续排空输出。日志物理总上限
  2 MiB + 64 KiB，预留终态空间，单记录最多 64 KiB。历史位于 `hooks/runs/`，跟随摘要
  保留；关闭诊断日志不会关闭流程历史。
- 上层负责开始/结束通知和相同的流程入口，复用 Job Run。UI 更新仅在 event loop。
  退出时先异步排空 Hook，再按原顺序清理持久化、消息和 UI；异常 event-loop 返回
  后也等待同一屏障。任务接收等待启动加载及孤立日志清理完成；退出关闭接收入口，
  等待已提交的配置操作完成，再停止持久化。

## Regression checklist / 回归清单

Replay streams records with bounded buffers and an enforced 16 MiB read budget
(including legacy journals), rather than trusting file metadata. Unterminated
live/crash tails are ignored until a later read; malformed complete records fail
explicitly. `events_after` returns at most 256 events and stops at approximately
256 KiB (at most one extra bounded record); callers resume from the last sequence.
UI replay captures the summary sequence and formats directly into one string.
Crash-recovered interrupted runs have no active writer; their display watermark
comes from the complete journal records, so skipped output-only summary writes
cannot hide durable pre-crash output. Their interrupted conclusion is retained.
Repeated history opens and leaving Job Run cancel older work; stale completions
cannot overwrite the selected run. Scrolling reuses the shared Job virtualizer.

回放使用有界缓冲区和实际读取累计 16 MiB 上限（兼容旧日志），不只信任文件大小。
活动/崩溃日志未结束的尾记录等待下次读取；损坏的完整记录明确报错。
`events_after` 每页最多 256 条，约 256 KiB（最多多一个有界记录），以上页末 seq 续读。
UI 回放捕获摘要 seq 并直接格式化到一个字符串；连续打开历史及离开 Job Run 会取消
旧工作，旧完成回调不得覆盖当前选择。滚动继续复用共享 Job 虚拟化。
崩溃恢复的 interrupted 运行已经没有写入方，其显示水位从日志的完整记录恢复，
避免跳过纯输出摘要写入后丢失已落盘输出；结论仍保持 interrupted，不重放脚本。

- Switches, project/outcome filtering, config revision, event deduplication.
- Default-empty migration, selection save/edit/clear/restart, mixed parent and
  collapsed/expanded lists, disabled/deleted Hooks, project isolation, queued
  deselection, test bypass and application-scoped events.
- Unsaved draft test, duplicate test, success, stderr/exit error, missing runtime,
  invalid cwd, JS syntax/runtime errors, TS type erasure and unsupported syntax.
- Queued/running cancellation, timeout, descendants, shutdown while running.
- Invalid UTF-8/long lines/output cap, lifecycle ordering/replay, corrupt state,
  restart interruption, history retention, no cross-project cwd/state leakage.
- Settings Hooks before Diagnostics at wide/narrow sizes; multiline editor,
  error wrapping, test results, effective row Switch states and stable history
  controls during global or row Switch saves, notification links, Job Run logs and Stop.
- Definition and event-Select categories: stable ordering, per-group counts,
  pointer selection, arrow/Home/End navigation, scroll reveal and unknown-event fallback.
- New/Edit dialogs cover the sidebar/status bar, scroll without moving footer
  actions, discard on Cancel and close only after successful Save. Repeated
  opens restore saved data; delete confirmation retains history.
- Repeated runtime changes, focus/blur and Settings re-entry: exactly one radio
  is checked, only its runtime draft is projected, Shell/Node paths stay isolated, and old
  run updates cannot revalidate edited source. Errors stay below Test.
- Long/short/long editor opens, scrolling to both source edges and back, and
  editing after scrolling: source order, indentation and first/last lines remain
  exact, with no stale syntax fragments. Compare long/short first-frame latency
  in the compiled app; closing releases rich text and pending updates cannot
  repopulate a closed editor. Oversized source falls back to complete plain text.
- With syntax colors ready, click to focus, verify a visible blinking native
  caret, move it with arrows, select text and type. Rich text must not consume
  input or suppress native selection/cursor rendering.
- Follow the mandatory [Tooltip matrix](tooltips.md) for fixed action labels,
  factory re-entry, cross-action switching, locales and viewport bounds.
- Dialog save/cancel shortcuts work with focus inside and outside the factory;
  plain Enter edits source, Escape dismisses an open selector before its dialog,
  and busy states reject duplicate submission. Clear history preserves active runs.
- Default and AI-disabled builds, full root UI, library tests and formatting.

Reference: [Node TypeScript execution](https://nodejs.org/api/typescript.html).
