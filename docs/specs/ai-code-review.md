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
- `ui/features/ai_review/`: feature package contract and the optional PR-side
  panel. Primer finding rows and severity labels live in
  `ui/components/ai_review/` and are registered in UI Components.

- `crates/usher-ai-review/`：与框架无关的输入、规则、调度器、reducer、Prompt、
  diff 分块、结果解析和 Session。
- `src/ai_review/config.rs`：基于 `usher-state-store` 的门面和自动配置持久化。
- `src/ai_review/controller.rs`：任务编排、Git/Provider diff 获取、AI 调用、取消、
  Session 持久化、通知和按需 UI 投影。
- `ui/features/ai_review/`：Feature 包协议和可选 PR 右侧面板；Primer finding row
  与严重级标签位于 `ui/components/ai_review/` 并登记到 UI Components。

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
6. Opening a completed result loads its session on a worker. Closing the panel
   immediately clears finding/conversation models, input text, and the current
   projection. Leaving PR Review performs the same cleanup; background tasks
   continue without a UI dependency.
7. Cancellation uses a task flag and queued-task removal. Shutdown stops the
   configuration and task persistence workers in the registered shutdown order.

1. 从 PR 列表启动时只入队元数据并留在列表；从 PR 详情启动时可立即打开右侧面板。
2. 调度器最多启动 Settings 配置的任务数；调高后立即调度队列，调低不打断运行任务，
   在任务完成后生效。
3. 同一项目/PR 只允许一个活动任务；不同项目和 PR 可以并发。
4. 只有获得调度槽后才读取 diff、规则、Prompt chunk 和 Provider 响应；持久任务 Store
   不保留大缓冲区。
5. 完成后持久化一个 Session 并发布 Message Center 结果；任务摘要最多保留 128 条。
6. 打开完成结果时由 Worker 加载 Session；关闭面板立即清空 finding/conversation model、
   输入和当前投影。离开 PR Review 执行同样清理；后台任务不依赖 UI。
7. 取消使用任务标记和队列移除；关闭应用时按已注册顺序停止配置与任务持久化 Worker。

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

PR 详情左侧持续显示 Files changed diff；右侧面板采用 GitHub Copilot Chat 的 PR 侧栏
信息结构：一个显示规则/模型上下文的紧凑标题栏，一个连续承载 Review 状态、摘要、
High/Medium/Low finding 与 Review 专属对话的滚动流，以及一个固定在底部的 composer。
`Ask` 与 `Review again` 通过 Primer 分段模式复用同一个 composer，不再堆叠两个输入框
或挂载第二个对话标题栏；即使输入框此前持有焦点，第一次指针点击也必须完成模式切换。
固定 composer 的画布不绘制贯穿圆角输入框背后的横线，只由输入框自身边框界定区域。
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

## Build and failure behavior / 构建与失败

`ai-code-review` includes the core package and full UI/settings bridges. It is
enabled in the default product feature set and can be excluded with
`--no-default-features --features renderer-femtovg`; disabled UI packages keep
the root Slint contract compilable without the task engine.

Network, invalid model configuration, unreadable rule files, empty diffs, and
invalid provider JSON fail only the affected task. User-facing task errors are
bounded and credentials, complete prompts, source payloads, and rule contents
must never be logged.

`ai-code-review` 包含核心包和完整 UI/Settings bridge；默认产品 feature 会启用它，
可通过 `--no-default-features --features renderer-femtovg` 排除。关闭时使用协议兼容的
UI stub，根 Slint 仍可编译。

网络、模型配置、规则文件、空 diff 或 Provider JSON 错误只影响对应任务。用户可见错误
必须有界，日志不得包含凭据、完整 Prompt、源码 payload 或规则正文。

## Regression checklist / 回归检查

- Queue more tasks than the configured limit with `USHER_AI_REVIEW_FIXTURE=review-v1`;
  verify the running high-water equals the configured limit and queued tasks
  start as slots become free.
- Start from the PR list and verify it remains visible. Open a completed task and
  verify PR detail selects Files changed and opens the panel.
- Select a finding and verify the matching file and changed line are revealed.
- Send a follow-up question, add one-off requirements, and run Review again.
- Focus each composer input, then switch between `Ask` and `Review again` with
  one click. Verify the mode changes on that click and no horizontal rule is
  visible behind the rounded input.
- Delete a rule used by a project and verify that project now inherits the
  repaired default.
- Try relative, missing, non-Markdown, directory, and oversized rule paths;
  verify each remains in the form with an inline error and no rule is added.
- Change the concurrent review limit repeatedly; verify the rule list keeps the
  same rows and order while only the numeric limit changes.
- Close the panel and leave PR Review while a task runs; verify both actions
  clear projected models and the task still completes.
- Drag the PR/AI Review divider to both limits and resize the window. Verify the
  pointer remains attached to the divider, the session preference survives a
  close/reopen, and the diff performs its final layout after release.
- Compile with and without `ai-code-review`, run library tests, and record
  feature build-time/binary and first-use/close-settled RSS measurements.

- 使用 `USHER_AI_REVIEW_FIXTURE=review-v1` 入队超过上限的任务，确认运行高水位等于
  配置值，空出 slot 后队列继续启动。
- 从 PR 列表启动后确认仍停留列表；打开完成任务后进入 PR 详情 Files changed 并打开面板。
- 点击 finding，确认对应文件和变更行被揭示。
- 发送追问、增加一次性要求并再次 Review。
- 分别聚焦两个 composer 输入框，再用一次点击切换 `Ask` / `Review again`；确认该次
  点击立即切换模式，且圆角输入框背后没有横向规则线。
- 删除项目正在使用的规则，确认项目改为继承修复后的默认规则。
- 分别输入相对路径、不存在路径、非 Markdown 文件、目录和超限文件，确认表单保留输入、
  显示行内错误且不会新增规则。
- 连续调整并发 Review 数量，确认 rules 列表的内容与顺序均保持不变，只有并发数字变化。
- 任务运行时关闭面板或离开 PR Review，确认投影 model 被清空且后台任务仍能完成。
- 将 PR/AI Review 分隔线拖到两侧边界后调整窗口大小，确认指针始终贴住分隔线、关闭再
  打开仍保留本次会话宽度偏好，且 Diff 只在释放指针后采用最终排版。
- 分别编译启用/关闭 feature，运行库测试，并记录编译时间/产物及首次使用/关闭稳定 RSS。
