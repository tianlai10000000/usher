# Pull request workflow / Pull Request 工作流

## Scope / 范围

This specification is the portable contract for the current pull-request capability: project-scoped counts, the paginated PR/MR list, the full-window review workspace, diff/source enrichment, and the local recommendation flow. It deliberately distinguishes read-only inspection from unsupported remote write operations, so another technology stack can reproduce the behavior without reproducing Rust, Slint, or the current UI layout.

本文是当前 Pull Request 能力的可迁移契约：项目级计数、分页 PR/MR 列表、全窗口审核工作区、diff/源码补全以及本地建议流程。它刻意区分只读检查和暂未支持的远端写操作，使后续技术栈能够复现行为，而不必复现 Rust、Slint 或当前 UI 布局。

## Product boundary / 产品边界

The current product supports the following read-oriented workflow:

1. A selected project optionally displays counts of open changes targeting `fat`, `uat`, and `prod`.
2. The PR tab loads one page of open, closed, merged, or all changes and displays number, title, branch pair, author, and UTC+08:00 update time.
3. Opening a row presents a full-window review workspace with file tree, per-file unified diff, GitHub-style file actions, added/removed totals and ratio bar, immutable base/head references, filtering, folder collapse, and next/previous file navigation.
4. The selected file toolbar can copy the file path, toggle between expanding all hidden lines and collapsing non-diff lines, or switch between the diff and the complete immutable file source. Hidden diff context can also be expanded in bounded 20-line increments.
5. The manual “Analyze PR” form produces a local risk recommendation; the user can record an approve/reject decision in the app state.

It does **not** create a provider review, approve, reject, merge, close, comment on, or otherwise mutate a remote pull request. The current recommendation is a deterministic local rule, not an LLM call and not a CI or code-scanning integration.

当前产品支持以下以只读为主的流程：

1. 选中项目后，可选显示目标为 `fat`、`uat`、`prod` 的 open 变更数量。
2. PR Tab 加载 open、closed、merged 或 all 的一页变更，显示编号、标题、分支对、作者和东八区更新时间。
3. 打开某一行后，在全窗口审核工作区展示文件树、按文件的 unified diff、GitHub 风格文件操作、增删行总数和比例条、固定的 base/head 引用、筛选、目录折叠与上/下一个文件导航。
4. 选中文件工具栏支持复制文件路径、在“展开全部隐藏行”和“收起非 diff 行”之间切换，以及在 diff 与固定提交的完整文件源码之间切换；也可按每次最多 20 行单独展开隐藏上下文。
5. 手动填写的“Analyze PR”表单生成本地风险建议；用户可在应用状态中记录 approve/reject 决定。

它**不会**创建 Provider review，也不会在远端 approve、reject、merge、close、comment 或以其他方式修改 PR。当前建议是确定性的本地规则，不调用 LLM，也不接入 CI 或代码扫描。

## Ownership / 职责

- `src/workflow/pr_review.rs` owns the active request, stale-response protection, one-entry diff cache, immutable-ref source cache, hunk expansion, and Slint model updates.
- `src/git/` owns provider-neutral diff models and provider-specific request/response handling.
- `src/codereview/api/` owns diff endpoint URL constants only.
- `ui/views/workflow/pr_review_workspace.slint` owns review layout and user interaction; `ui/main.slint` mounts it at the root overlay layer.
- `src/workflow/pr_list.rs` owns list-page selection, list request sequencing, pagination presentation, and UTC+08:00 timestamp formatting.
- `src/workflow/pr.rs` owns the local recommendation and manual-decision state; it has no provider write API.
- `src/workflow/bindings.rs` supplies the selected project context and the optional `fat`/`uat`/`prod` count refresh.
- `src/project/pr_stats.rs` owns the legacy blocking branch-count adapters; it is separate from the provider-neutral list/review façade.

- `src/workflow/pr_review.rs` 负责当前请求、旧响应防护、单项 diff 缓存、固定提交源码缓存、hunk 展开和 Slint model 更新。
- `src/git/` 负责 Provider-neutral diff 模型及各 Provider 的请求/响应处理。
- `src/codereview/api/` 只负责 diff 接口 URL 常量。
- `ui/views/workflow/pr_review_workspace.slint` 负责审核布局和交互；`ui/main.slint` 将其挂载到根 overlay 层。
- `src/workflow/pr_list.rs` 负责列表页选择、列表请求序列、防止旧响应覆盖、分页展示和东八区时间格式化。
- `src/workflow/pr.rs` 负责本地建议和人工决定状态；它不拥有 Provider 写接口。
- `src/workflow/bindings.rs` 提供当前项目上下文并触发可选的 `fat`/`uat`/`prod` 数量刷新。
- `src/project/pr_stats.rs` 负责历史遗留的阻塞式分支计数适配；它与 Provider-neutral 的列表/审核 façade 分离。

## Portable domain contract / 可移植领域契约

An implementation in another stack should preserve these provider-neutral records rather than expose provider JSON to the UI:

| Record | Required fields | Notes |
| --- | --- | --- |
| `RepositoryContext` | project key, provider type, provider project id, remote URL | The persisted provider type must agree with the remote URL before any request is made. |
| `PullRequestPage` | items, total, `total_exact`, optional page count | The UI distinguishes an exact total from a lower bound. Page size is 10. |
| `PullRequestItem` | number, title, author, source branch, target branch, updated time, web URL | A missing branch is shown as `—`; parseable timestamps are rendered as `YYYY-MM-DD HH:MM:SS +08:00`. |
| `PullRequestDiffDetail` | files, added/removed totals, base SHA, head SHA, start SHA, too-many-commits flag | The SHA values are immutable source references for later context expansion. |
| `PullRequestDiffFile` | path, old path, change type, added/removed counts, lines | Change types are `new_file`, `deleted_file`, `renamed_file`, or `edit_file`. |
| `PullRequestDiffLine` | old line, new line, kind, content | Kinds are normalized to hunk/context/addition/deletion before rendering. |

其他技术栈应保留这些 Provider-neutral 记录，而不是向 UI 暴露 Provider JSON：

| 记录 | 必需字段 | 说明 |
| --- | --- | --- |
| `RepositoryContext` | 项目 key、Provider 类型、Provider 项目 ID、remote URL | 每次请求前，已持久化的 Provider 类型必须与 remote URL 一致。 |
| `PullRequestPage` | items、total、`total_exact`、可选 page count | UI 区分准确总数和下界；每页固定 10 条。 |
| `PullRequestItem` | 编号、标题、作者、源分支、目标分支、更新时间、网页 URL | 缺失分支显示 `—`；可解析时间显示为 `YYYY-MM-DD HH:MM:SS +08:00`。 |
| `PullRequestDiffDetail` | 文件、增删总数、base SHA、head SHA、start SHA、过多提交标记 | SHA 是后续展开上下文所用的固定源码引用。 |
| `PullRequestDiffFile` | 路径、旧路径、变更类型、增删统计、行 | 变更类型归一为 `new_file`、`deleted_file`、`renamed_file`、`edit_file`。 |
| `PullRequestDiffLine` | 旧行号、新行号、类型、内容 | 在渲染前归一为 hunk/context/addition/deletion。 |

## Provider adapter contract / Provider 适配契约

Provider adapters accept a validated `RepositoryContext` and return only the domain records above. They own provider URLs, authentication headers, pagination interpretation, JSON decoding, and normalization. The UI and workflow controller neither construct URLs nor parse provider JSON.

| Provider | List | Branch pair | Diff and source | Authentication |
| --- | --- | --- | --- | --- |
| GitHub / GitHub Enterprise | issue search filtered by `repo:` and PR state | GitHub search lacks refs, so load each visible PR detail concurrently | PR detail, PR files, repository contents | `Authorization: Bearer <token>` when configured |
| GitLab | project merge-request list | supplied by list response | MR detail, MR diffs, raw repository file | `PRIVATE-TOKEN` when configured |
| Gitee public | repository pull list | supplied by list response | PR detail, PR files, repository contents | optional configured token |
| Gitee-compatible enterprise | project merge-request list | supplied by list response | merge-request diff and repository file | enterprise header, `OAUTH: enabled`, optional `PRIVATE-TOKEN` |

GitHub list search provides an exact total but not `head.ref`/`base.ref`. Its extra detail requests are limited to the visible page; a failed row enrichment must not discard the page. GitLab and Gitee may report totals through headers or provider contracts; when an exact total is absent, the paginator shows a lower bound and permits the next page after a full response.

Provider 适配层接收已验证的 `RepositoryContext`，只返回上述领域记录。它负责 Provider URL、鉴权 header、分页解释、JSON 解码和归一化；UI 和 Workflow controller 均不构造 URL，也不解析 Provider JSON。

| Provider | 列表 | 分支对 | Diff 与源码 | 鉴权 |
| --- | --- | --- | --- | --- |
| GitHub / GitHub Enterprise | 按 `repo:` 和 PR 状态过滤的 issue search | GitHub search 不含 ref，因此并发读取当前可见 PR 的详情 | PR detail、PR files、repository contents | 已配置时使用 `Authorization: Bearer <token>` |
| GitLab | 项目 MR 列表 | 列表响应直接提供 | MR detail、MR diffs、raw repository file | 已配置时使用 `PRIVATE-TOKEN` |
| Gitee 公有服务 | 仓库 PR 列表 | 列表响应直接提供 | PR detail、PR files、repository contents | 可选已配置 token |
| Gitee 兼容企业服务 | 项目 MR 列表 | 列表响应直接提供 | merge-request diff 和 repository file | enterprise header、`OAUTH: enabled`、可选 `PRIVATE-TOKEN` |

GitHub 搜索列表提供准确总数，但不含 `head.ref`/`base.ref`。额外详情请求只限于当前可见页；单行补全失败不得丢弃整页。GitLab 和 Gitee 可通过 header 或 Provider 契约给出总数；没有准确总数时，分页器显示下界，并在满页响应后允许进入下一页。

## Execution and state flow / 执行与状态流

```text
project selected
  -> load persisted RepositoryContext off the UI thread
  -> validate provider type against remote URL
  -> fetch the selected list page through one provider adapter
  -> normalize records and apply only if list sequence is current
  -> user opens a row
  -> fetch normalized diff, cache one shared immutable base by project + request + whitespace mode
  -> build a local file tree and select/highlight the first file
  -> optional hunk expansion fetches immutable source once, then creates or mutates only that file's copy-on-write override
  -> collapse drops that override and exposes the provider-normalized base file without another request
```

Every asynchronous boundary needs a monotonically increasing request sequence (separate sequences for list, diff, syntax highlighting, and hunk expansion are recommended). A result may update the visible state only if its sequence, selected project, selected PR, and selected file still match. Closing the review or switching projects invalidates all in-flight work and clears review caches. Never let a stale result reopen an overlay or replace a newer page.

```text
选择项目
  -> 在 UI 线程外读取持久化的 RepositoryContext
  -> 校验 Provider 类型与 remote URL
  -> 通过一个 Provider adapter 获取当前选中的列表页
  -> 归一化记录，且仅在列表序列仍为最新时回写
  -> 用户打开某一行
  -> 获取归一化 diff，并按项目 + 请求 + 空白模式缓存一份共享且不可变的基础数据
  -> 构建本地文件树，选择并高亮第一个文件
  -> 可选的 hunk 展开只获取一次固定源码，再只创建或修改该文件的写时复制 override
  -> 收起操作删除该 override 并重新使用 Provider 归一化后的基础文件，不发起新请求
```

每个异步边界都需要单调递增的请求序列（建议列表、diff、语法高亮和 hunk 展开各自独立）。只有当序列、当前项目、当前 PR 和当前文件仍匹配时，结果才可更新可见状态。关闭审核或切换项目会使所有在途任务失效并清理审核缓存；旧结果绝不能重新打开 overlay 或覆盖新页。

## Invariants / 不变量

1. Clicking a PR list row opens the review workspace and retains the underlying Workflow PR list state.
2. Network and project-file work runs off the Slint event loop and uses the shared HTTP/runtime modules.
3. Only the latest project/PR/reload request may update the workspace. Closing or switching projects invalidates in-flight work.
4. Provider responses are normalized before reaching UI code. The UI must not parse JSON or construct API URLs.
5. Selecting a changed file replaces only the visible line model; it does not request the diff again.
6. The ignore-whitespace option is shown only when the active provider endpoint supports it (currently Gitee-compatible enterprise). Changing it explicitly reloads the current PR diff. GitHub, GitLab, and public Gitee must not show the unsupported control or issue a no-op reload.
7. Tokens, authentication headers, and response bodies must not be logged.
8. Approve/Reject currently forwards to the existing human-confirmation callback; it must not pretend a remote provider approval succeeded.
9. The left navigator groups changed files by path, supports folder collapse and case-insensitive path filtering, and keeps the original provider file index when a filtered result is selected. File-state Octicons use Primer semantic foreground tokens: success for additions, danger for deletions, attention for renames, and muted for edits, with separate light/dark values. Its GitHub-style filter menu lists file extensions with local counts, supports independent extension selection, an `Only manifest files` restriction, and a `Viewed files` visibility toggle. The `Files changed` toolbar has a GitHub sidebar toggle: hiding the tree also removes its divider and resize hit target so the diff fills the workspace; showing it restores the retained width. File-tree and diff rows are virtualized so resizing and scrolling instantiate only visible delegates. Its divider resizes live with the pointer, with a 220 px minimum, a 520 px absolute maximum, and a dynamic maximum of 45% of the review width.
10. Filtering and file-to-file navigation are local model operations. They must not reload the diff or introduce an additional provider request.
11. The normalized `viewed` field controls the local `Viewed files` filter. Provider adapters whose similarly named field has different semantics must normalize it to `false` instead of exposing that provider field as review progress.
12. Syntax highlighting is produced in Rust with `syntect` using the selected file path; Slint receives styled token models and never parses source languages.
13. Tabs expand to four-column stops before highlighting. Visible token and whitespace widths use the selected monospace face's measured advance; display columns (including double-width CJK characters) size the horizontal scroll extent.
14. PR list, PR detail/diff, and repository-source requests are dispatched by the current project's persisted `repo_type`. The dispatcher also verifies that it matches `remote_repo`; no caller may silently fall back to another provider.
15. A clickable hunk control is shown only when hidden context exists. Each click reveals at most 20 preceding lines, fetching the file lazily at the diff's immutable head/base SHA and caching it for the active review.
16. Source expansion requests use the same stale-response protection as diff loading. Switching projects, closing the review, or opening another PR prevents a late source response from modifying the visible model.
17. The PR list has a fixed page size of 10. Changing state resets to page 1; changing page requests only that page, except for GitHub's visible-row branch enrichment.
18. Provider timestamps are converted to UTC+08:00 before presentation when they are valid RFC 3339 values. An unparseable value remains visible verbatim rather than being replaced with a misleading time.
19. The file filter, folder collapse state, tree construction, selected-file navigation, and syntax highlighting are local presentation operations; none may reload a diff.
20. The current Analyze/Approve/Reject controls update only project-scoped local workflow state. They must never claim that a remote provider accepted a review decision.
21. Tokens, authorization headers, and unbounded provider response bodies are not logged. Background list, diff, branch-enrichment, and source requests use silent feedback; the view owns its inline loading and error state.
22. File switching and hunk expansion keep the currently rendered code model mounted while syntax highlighting runs, then replace the model atomically. File switching resets the code viewport after the replacement; hunk expansion preserves the current viewport and must not flash an empty surface.
23. The file toolbar follows GitHub's ordering: file icon, path, copy-path, the two-state `Expand all lines`/`Collapse non-diff lines` context toggle, statistics, previous/next navigation, and overflow. Every icon action is vertically aligned and has a hover tooltip. Expansion uses the `unfold` icon; once any hidden context is inserted, the action switches to the `fold` icon and restores the retained original diff when clicked. The overflow popup uses GitHub's eye Octicon. Its action toggles `View file`/`View diff` inside the application and always reads source at the diff's immutable head SHA, or base SHA for a deleted file. It never opens a browser or reads a mutable branch tip.
24. Review caches are memory-bounded. The normalized diff has one shared immutable base; expanded context is retained only as per-file copy-on-write overrides, and collapsing a file removes its override. Highlighted diff models retain at most four recently used files and immutable source text retains at most three. The complete-file highlighted model lives only in the active UI model and is not added to the highlighted-diff cache.
25. A successful zero-item list uses a GitHub-style filter-specific headline and exact `0 total` inside the same large-image Vuetify-style layout as the error state. In both states, the illustration, title, description, and optional action are horizontally centered as one visual column. Empty data uses the bundled teamwork illustration. Request, authentication, and schema failures use the bundled connection illustration and a `Retry Request` action; technical adapter errors are logged but are not presented as empty data or exposed verbatim in the UI.

1. 点击 PR 列表行后打开审核工作区，并保留下层 Workflow PR 列表状态。
2. 网络和项目文件工作必须离开 Slint event loop，并复用统一 HTTP/runtime 模块。
3. 只有最新项目/PR/重载请求可以更新工作区；关闭工作区或切换项目会使在途请求失效。
4. Provider 响应必须在进入 UI 前归一化；UI 不解析 JSON，也不构造 API URL。
5. 选择变更文件时只替换当前可见的行 model，不重复请求 diff。
6. 仅当当前 Provider endpoint 支持时才显示忽略空白字符选项（目前仅 Gitee 兼容企业版）。切换后显式重载当前 PR diff；GitHub、GitLab 和 Gitee 公有版不得显示不受支持的控件或发起无效重载。
7. Token、鉴权 header 和响应正文不得写入日志。
8. Approve/Reject 当前只转发现有人工确认 callback，不得伪装成远端 Provider 审批成功。
9. 左侧导航按路径组织变更文件，支持目录折叠和不区分大小写的路径筛选；选中筛选结果时仍使用 Provider 原始文件索引。文件状态 Octicon 使用 Primer 语义前景色：新增为 success、删除为 danger、重命名为 attention、修改为 muted，并分别适配浅色和深色值。GitHub 风格筛选菜单显示带本地计数的扩展名，可分别勾选扩展名，并提供 `Only manifest files` 限制和 `Viewed files` 可见性开关。`Files changed` 工具栏提供 GitHub 侧栏开关：隐藏文件树时同时移除分隔线和拖拽命中区，让 Diff 占满工作区；再次显示时恢复先前宽度。文件树和 Diff 行采用虚拟化，只实例化可见 delegate；分隔线随指针实时调整宽度，最小宽度 220 px、绝对最大宽度 520 px，并且动态上限为审核区宽度的 45%。
10. 文件筛选和上一项/下一项导航只操作本地 model，不得重新加载 diff 或增加 Provider 请求。
11. 归一化后的 `viewed` 字段驱动本地 `Viewed files` 筛选；若某 Provider 的同名字段语义不同，adapter 必须将其归一化为 `false`，不得把该 Provider 字段冒充审核进度。
12. 语法高亮由 Rust 使用 `syntect` 并根据当前文件路径生成；Slint 只接收带样式的 token model，不解析源码语言。
13. 高亮前将 Tab 按四列制表位展开；可见 token 和空白宽度使用当前等宽字体的实际测量值，显示列数（包括双宽中日韩字符）用于计算横向滚动范围。
14. PR 列表、PR 详情/diff 和仓库源码请求必须按当前项目持久化的 `repo_type` 分发；分发层同时校验其与 `remote_repo` 一致，任何调用方都不得静默回退到其他 Provider。
15. 只有确实存在隐藏上下文时才显示可点击的 hunk 控件。每次点击最多展开前置 20 行；源码按 diff 的固定 head/base SHA 懒加载，并在当前审核期间缓存。
16. 源码展开请求复用 diff 加载的旧响应防护；切换项目、关闭审核或打开其他 PR 后，迟到的源码响应不得修改当前 model。
17. PR 列表固定每页 10 条。切换状态时重置到第 1 页；切换页码时只请求该页，GitHub 为可见行补充分支的详情请求除外。
18. Provider 时间戳若为有效 RFC 3339，必须在展示前转换为东八区（UTC+08:00）。无法解析的值应原样展示，不能替换成误导性的时间。
19. 文件筛选、目录折叠状态、树构建、选中文件导航和语法高亮均是本地展示操作；它们不得重载 diff。
20. 当前 Analyze/Approve/Reject 控件只更新项目级本地 Workflow 状态，绝不能声称远端 Provider 已接受审核决定。
21. 不得记录 token、鉴权 header 或无边界的 Provider 响应正文。后台列表、diff、分支补全和源码请求必须静默；视图自身拥有行内 loading 和 error 状态。
22. 切换文件和展开 hunk 时，语法高亮期间必须保留当前已渲染代码 model，完成后再原子替换。切换文件在替换完成后重置代码 viewport；展开 hunk 保持当前 viewport，且不得闪现空白区域。
23. 文件工具栏按 GitHub 顺序排列：文件图标、路径、复制路径、双状态 `Expand all lines`/`Collapse non-diff lines`、增删统计、上一/下一文件和更多操作。所有图标必须垂直对齐并提供 Hover Tooltip。展开状态使用 `unfold` 图标；插入任意隐藏上下文后切换为 `fold` 图标，点击后恢复保留的原始 diff。更多菜单使用 GitHub 的眼睛 Octicon；其操作在应用内切换 `View file`/`View diff`，并始终按 diff 的固定 head SHA 读取源码，删除文件使用 base SHA。不得打开浏览器或读取可变分支 tip。
24. 审核缓存必须有内存上限：归一化 diff 只保留一份共享且不可变的基础数据；展开上下文只按文件保存写时复制 override，收起文件时必须删除该 override。Diff 高亮最多保留最近使用的 4 个文件，固定源码文本最多保留 3 个文件；完整文件的高亮 model 只存在于当前 UI model，不进入 Diff 高亮缓存。
25. 请求成功且列表为零条时，在与错误状态一致的 Vuetify 风格大图布局中显示 GitHub 风格、随筛选变化的标题和准确的 `0 total`；两种状态下的插画、标题、说明和可选操作必须作为同一视觉列水平居中。空数据使用内置 teamwork 插画；请求、鉴权和响应结构失败使用内置 connection 插画和 `Retry Request` 操作。Provider adapter 的技术错误写入日志，但不得伪装成空数据或原样暴露在 UI 中。

## Rewrite checklist / 重写清单

- Keep repository configuration (`provider type`, `provider project id`, `remote URL`) persistent and project-scoped; keep the list/review cache session-scoped and disposable.
- Split the implementation into UI, workflow/controller, provider adapters, HTTP/auth, and persistence layers. Only adapters know provider response shapes.
- Preserve the normalized records and error/loading states above as the cross-layer API. A web, mobile, or native UI can replace Slint without changing provider adapters.
- Treat remote reads as cancellable or sequence-guarded. Prefer cancellation where the target runtime supports it, but preserve the “latest request wins” rule either way.
- Use immutable base/head references for expanded source. Do not expand context from a mutable branch tip.
- Keep per-file highlighting and source contents cached only for the active review; invalidate on project switch, PR switch, close, or whitespace-mode change as appropriate.
- Introduce remote approval, comments, merge, or CI integration as new explicit commands with their own permissions, confirmation, optimistic-state policy, and audit/error behavior. They are not extensions of the current local manual-status control.

- 将仓库配置（`provider type`、`provider project id`、`remote URL`）持久化并按项目隔离；将列表/审核缓存视为可丢弃的会话状态。
- 将实现拆分为 UI、workflow/controller、Provider adapters、HTTP/auth 与持久化层；只有 adapter 知道 Provider 响应结构。
- 将上述归一化记录和 loading/error 状态作为跨层 API。Web、移动端或原生 UI 可替换 Slint，而无需改动 Provider adapter。
- 远端读取必须支持取消或请求序列防护。目标运行时若支持则优先取消，但无论如何都要保留“最新请求优先”的规则。
- 展开源码必须使用固定的 base/head 引用，不能从可变的分支 tip 展开上下文。
- 每文件的高亮和源码内容只缓存于当前审核；在项目、PR、关闭或空白模式切换时按需失效。
- 远端 approve、comment、merge 或 CI 集成应作为新的显式命令，独立设计权限、确认、乐观状态策略以及审计/错误行为；它们不是当前本地 manual-status 控件的简单延伸。

## Regression checklist / 回归清单

- Open a PR row and verify title, branches, author, file counts, and first file diff.
- Switch files and verify no network request is sent.
- Filter by a path fragment, collapse and expand folders, clear the filter, and verify the selected diff remains mapped to the correct file. Drag the divider to both limits and resize the window; verify the tree remains within 220–520 px and 45% of the review width while the session preference is retained.
- Open the filter menu and verify extension counts, independent extension toggles, manifest-only filtering, and viewed-file visibility all compose locally without a provider request.
- Toggle the file tree from the `Files changed` toolbar. Verify the icon direction changes, the diff expands to full width while hidden, the divider cannot be dragged while hidden, and the previous tree width returns when shown again.
- Use previous/next file controls and verify boundary controls are disabled.
- Toggle ignore whitespace and verify one new request uses the selected option.
- Verify the ignore-whitespace control is hidden for GitHub, GitLab, and public Gitee, and visible for a configured Gitee-compatible enterprise repository.
- Close during loading, switch projects, or rapidly open two PRs; verify late responses do not reopen or overwrite the workspace.
- Verify binary/empty files show a stable placeholder and long paths/source lines remain scrollable or elided.
- Verify Rust, TypeScript/Vue, Markdown, and Slint files show token colors; verify leading spaces, tabs, long lines, and Chinese comments retain readable alignment in light and dark themes.
- Compare the diff with GitHub at the same display scale: line numbers, markers, hunk labels, and source tokens use one readable monospace size; hunk rows use distinct GitHub-style blue gutter/content backgrounds and remain vertically centered in light and dark themes.
- Compare the file tree with GitHub at the same display scale: headers, filter text, folder/file names, folder counts, and addition/deletion statistics use the platform UI family with GitHub-equivalent 14px primary and 12px secondary text.
- Verify provider tokens and diff bodies do not appear in logs.
- Exercise the PR list, diff, and one hunk expansion against Gitee, GitLab, and GitHub projects; verify each uses only its configured provider endpoint and token.
- Verify the list columns remain aligned across a wide desktop layout and that a GitHub timestamp such as `2026-07-15T05:26:44Z` is displayed as `2026-07-15 13:26:44 +08:00`.
- Verify an Analyze/Approve/Reject interaction changes only local status and emits no provider write request.
- Click the same hunk repeatedly and verify at most 20 new context lines appear per click without re-fetching the source file; close or switch projects during the first fetch and verify the late response is ignored.
- Switch to an uncached file and expand a hunk while highlighting is slow; verify the previous code remains visible with a small rendering indicator, no white frame is shown, and hunk expansion preserves the viewport.
- Verify copy-path writes the selected path; `Expand all lines` reveals every hidden range and switches to the fold icon; `Collapse non-diff lines` restores the original diff and unfold icon without a network request; the ratio bar reflects additions/deletions; and the overflow menu contains only `View file` (or `View diff` while active).
- Hover every file-toolbar icon, including disabled previous/next or expand-all states, and verify its tooltip appears above the diff without shifting the vertically centered toolbar layout. Open the overflow menu and click the code, file tree, or review header; verify the menu closes each time.
- Switch among more than four diffs and more than three full-file/source views; verify recently used entries remain fast while cache sizes stay bounded and evicted files are reloaded safely.
- Return an empty first page for every list filter and verify the filter-specific GitHub-style message, teamwork illustration, and `0 total`; then simulate a provider/schema failure and verify the connection illustration and retry action without exposing the adapter error text. In both states, verify the image, title, description, and optional button share the same horizontal center at narrow and wide window sizes.
