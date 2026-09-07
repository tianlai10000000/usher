# Project repository metadata / 项目仓库元数据

## Scope / 范围

This specification covers provider metadata resolved on the Project page and the repository-context precondition for the provider-neutral pull-request list in the existing Workflow PR tab. The complete list/review behavior is specified in [Pull request workflow](pr-review.md). It does not change RAG Chat behavior.

本文约束 Project 页面解析的 Provider 元数据，以及现有 Workflow PR Tab 中 Provider-neutral PR 列表所依赖的仓库上下文。完整的列表/审核/建议行为见 [Pull Request 工作流](pr-review.md)；不改变 RAG Chat 行为。

## Ownership / 职责

- `src/project/mod.rs` is the stable feature façade. `controller.rs` validates form data and
  coordinates asynchronous saves; `state.rs` owns the typed Project reducer, selectors,
  request de-duplication, and `PersistentStore`; `model.rs`, `storage.rs`, `logo.rs`, and
  `overview.rs` own persistence records, `projects.json`, logo handling, and PR-overview tasks.
- `logo_loader.rs` owns the shared list/preview effect driver; `logo_requests.rs`
  implements admission and backoff inside the Project Store.
- `src/codereview/api/` owns provider URL/path constants only.
- `src/git/` parses remotes and owns provider-specific operations, authentication headers, and response decoding.
- `src/git/overview.rs` aggregates normalized open PR/MR pages into `fat`, `uat`, and `prod` counts; Project and Workflow do not implement provider requests.
- `src/http/mod.rs` owns request execution, interception, timeout/status handling, and safe logging.
- `src/workflow/pr_list.rs` owns selected-project list presentation, status filters, and
  pagination; `pr_list_state.rs` owns independent typed Page and Totals request generations.
- `ui/views/project.slint` owns only `GitHubProjectPage` route composition.
  Project controls/cards/dialogs live under `ui/components/project/`; generic
  empty/loading/error presentation is owned by `GitHubBlankslate`.

- `src/project/mod.rs` 是稳定 feature façade。`controller.rs` 校验表单并编排异步保存；
  `state.rs` 负责强类型 Project reducer、selector、请求去重和 `PersistentStore`；
  `model.rs`、`storage.rs`、`logo.rs`、`overview.rs` 分别负责持久化记录、
  `projects.json`、Logo 处理和 PR overview 任务选择。
- `logo_loader.rs` 负责列表/预览共用的副作用驱动器；`logo_requests.rs` 在 Project Store
  中实现请求准入与退避。
- `src/codereview/api/` 只负责 Provider URL/路径常量。
- `src/git/` 解析 remote，负责 Provider 实际操作、鉴权 header 和响应解码。
- `src/git/overview.rs` 将归一化的 open PR/MR 分页聚合为 `fat`、`uat`、`prod` 数量；Project 与 Workflow 不实现 Provider 请求。
- `src/http/mod.rs` 负责请求执行、拦截、超时/状态处理及安全日志。
- `src/workflow/pr_list.rs` 负责当前项目的 PR 列表展示、状态筛选和分页；
  `pr_list_state.rs` 负责相互独立的强类型 Page 与 Totals 请求代际。
- `ui/views/project.slint` 只负责 `GitHubProjectPage` 路由编排。Project
  控件、卡片和对话框统一位于 `ui/components/project/`；通用空白、加载和错误
  展示由 `GitHubBlankslate` 负责。

## Invariants / 不变量

1. `remote_repo` determines the provider. Supported values are `gitee`, `gitlab`, and `github`.
2. A supported URL may use HTTPS or SSH Git syntax and must include an owner/group plus repository path.
3. Configured Gitee-compatible enterprise project search uses the final `remote_repo` path segment before `.git` as `project_name`; it never uses the editable project name or alias.
4. Provider project details must be loaded through the shared HTTP module. Tokens and enterprise API configuration come from the matching Git platform settings and must not be logged.
5. Enterprise API URLs are supplied only by `[git.platform.<provider>].api_base_url`; source code must not contain environment-specific endpoints or enterprise identifiers. A configured URL is used only if its host matches `remote_repo`. The service host or full versioned API root is accepted; the provider version path is added when omitted.
6. A successful save writes non-empty `repo_project_id` and lower-case `repo_type` to `project/projects.json`.
7. If lookup, parsing, or local persistence fails, no new partial project record is written and an existing record remains unchanged. A failed optimistic add, update, or delete rolls the in-memory registry and Project UI back when that operation is still the current version; it must not overwrite a later edit.
8. A duplicate save for the same project while a lookup is running is rejected. Network work must not block the Slint event loop.
9. Opening Workflow selects `repo_project_id` and `repo_type` from the hydrated Project Store, then requests open PRs by default. It must not reread `projects.json` for each lookup. Open, closed, merged, and all are explicit caller-selected filters; provider APIs do not inject a default state query.
10. The PR tab displays 10 records per page and bottom-centered pagination. It must lazily request only the page selected by the user. Page loads and Open/Closed total loads use independent typed generations so either request class can advance without invalidating the other. GitHub may issue one detail request for each visible row to enrich its search response with source and target branch names; these requests remain scoped to that selected page. All parseable provider `updated_at` timestamps are displayed in UTC+08:00. Branch pairs and Updated timestamps wrap to additional lines instead of eliding; when the list card is narrower than the table floor width, the table scrolls horizontally rather than dropping columns. The documented response `meta.total_count` and `meta.total_pages` determine the exact total and total pages where available. Stale responses must not overwrite the current project/filter/page.
11. `src/codereview/api/` must remain constants-only. It must not execute requests or own provider response models; endpoint changes must not require changing operation call sites.
12. Project-card and Workflow environment counts use the persisted provider type/id and the same Git façade as the PR list. They run on the shared runtime, count only open requests whose normalized target branch is exactly `fat`, `uat`, or `prod`, and write UI state only from the Slint event loop.
13. User-triggered project-registry reloads run on the shared runtime with
    blocking storage work isolated from the Slint event loop. The Project page
    disables duplicate reload activation and exposes visible loading and
    recoverable inline error/retry states; an empty search result is distinct
    from an empty registry.
14. The Project page uses the shared GitHub/Primer PageHeader, Blankslate,
    Button, IconButton, TextField, Dialog, and ScrollView contracts. Required
    dialog fields remain visibly labeled, validation is descriptive, and
    deletion remains a confirmed danger action.
15. `is_pr_auto_update` currently means one-shot PR-count prefetch, not a
    recurring background refresh. When enabled, the Project list fetches the
    FAT/UAT/PROD overview while it is loaded or rebuilt, and Workflow fetches
    the same overview when that project is opened. The current implementation
    has no polling interval or automatic retry timer.
16. Project grid columns are calculated from the constrained page content
    width. Each card keeps at least 280 px before another column is added, and
    both grid cards and list rows expose keyboard activation with an accessible
    project label.
17. Each project grid card and list row presents persisted `repo_type` as a
    compact 45-degree upper-right corner ribbon with the provider logo and
    readable GitHub, GitLab, or Gitee name. Both ends are clipped by the card
    edges. The ribbon must reserve no metadata row and scale down before a
    responsive host would crowd the project identity or paths. Brand
    backgrounds remain fixed across themes: GitHub `#24292E`, GitLab
    `#FC6D26`, and Gitee `#C71D23`, with white provider artwork and text.
18. Project-registry reads and writes run off the Slint event loop. Add, update,
    and delete dispatch typed Actions; each changed commit is automatically
    queued through `PersistentStore`, whose single worker may collapse adjacent
    snapshots to the newest registry before atomically renaming a temporary
    sibling over `projects.json`. Runtime-only logo, overview, and pending-state
    commits do not rewrite unchanged durable records. Initial registry hydration
    and logo decode start only after the first rendered frame; persistence is
    enabled after hydration so the loaded snapshot is not rewritten. The Project
    page exposes loading until the event-loop commit applies the prepared model.
    Success feedback is emitted only after a real write completes; failure rolls
    back only while the failed durable registry is still current.
19. Provider adapters use the shared `src/git/prefix_cache.rs` TTL/LRU boundary
    for client-composed logical pages. GitLab Closed merges provider-sorted
    closed and merged prefixes. Gitee public Merged and Closed-unmerged share a
    raw closed prefix, then apply state and keyword filters locally. Complete
    provider pages are cached incrementally for 30 seconds, bounded to 32
    repository/filter/state/page-size keys per adapter, and deduplicated by IID.
    Explicit page-one refresh bypasses the cached prefix; a successful merge
    invalidates every prefix for that repository.

20. Remote Logo loading is independent of UI projection. Filtered project URLs
    and the current form preview share URL deduplication and at most four active
    requests. Each continuously selected URL has three total attempts with 2/4s
    cooldowns after failures; ordinary Store/UI refreshes cannot bypass them.
    Explicit registry reload resets failures, preserving active request identities.
    Removed sources release scheduling metadata after any active request completes;
    selecting them again starts a new budget. One shared-runtime driver owns all
    futures and the next retry deadline, with no task/timer per waiting URL.
    Shutdown cancels its HTTP futures before Project persistence shuts down.
    Downloads remain silent and failed images use the default Logo.

1. `remote_repo` 决定 Provider，支持 `gitee`、`gitlab`、`github`。
2. 支持 HTTPS 或 SSH Git URL，且必须包含 owner/group 与仓库路径。
3. 已配置的 Gitee 兼容企业仓库搜索使用 `remote_repo` 中 `.git` 前的最后一段作为 `project_name`；绝不使用可编辑的项目名称或别名。
4. Provider 详情必须经由统一 HTTP 模块请求；Token 和企业版 API 配置来自对应 Git 平台设置，且不得写入日志。
5. 企业版 API 地址只能由 `[git.platform.<provider>].api_base_url` 提供；源码不得保留环境专属地址或企业标识。只有 API 地址主机与 `remote_repo` 匹配时才使用该配置；既支持服务主机，也支持带版本路径的完整 API 根地址，缺少版本路径时由 Provider 自动补齐。
6. 保存成功后，`project/projects.json` 中写入非空 `repo_project_id` 和小写 `repo_type`。
7. 查询、解析或本地持久化失败时，不写入新的不完整项目记录，已有记录也保持不变。乐观新增、更新或删除写入失败时，若该操作仍是当前版本，必须回滚内存注册表和 Project UI；不得覆盖后续编辑。
8. 同一项目在查询未完成时重复保存必须被拒绝；网络工作不得阻塞 Slint event loop。
9. 进入 Workflow 时从已 hydration 的 Project Store selector 读取 `repo_project_id`、`repo_type`，不得为每次查询重新读取 `projects.json`；默认查询 open PR。open、closed、merged、all 均由调用场景显式选择，Provider API 不自行注入默认状态 query。
10. PR Tab 每页显示 10 条以及底部水平居中的分页，并且必须只懒加载用户选中的页。Page 加载与 Open/Closed 总数加载使用相互独立的强类型代际，任一类请求递增不得使另一类失效。GitHub 可以为当前可见的每一行补充发起一个详情请求，以从搜索响应中补齐源分支和目标分支；这些请求必须始终限定在已选中的页面内。所有可解析的 Provider `updated_at` 时间戳均按东八区（UTC+08:00）显示。Branch 与 Updated 列换行完整显示，不得省略号截断；列表卡片窄于表宽下限时横向滚动，而不是丢列。可用时使用接口响应中定义的 `meta.total_count`、`meta.total_pages` 显示准确总条数和总页数；旧项目、旧筛选或旧页码响应不得覆盖当前视图。
11. `src/codereview/api/` 必须保持仅包含常量，不得执行请求或持有 Provider 响应模型；修改接口地址时不应要求修改业务调用点。
12. Project 卡片和 Workflow 环境计数使用持久化的 Provider 类型/id，并与 PR 列表共用 Git façade。任务运行在共享 runtime，只统计归一化目标分支严格为 `fat`、`uat`、`prod` 的 open 请求，且只能在 Slint event loop 回写 UI。
13. 用户主动刷新项目注册表时，存储读取在共享 runtime 的 blocking task 中
    执行，不得阻塞 Slint event loop。Project 页面必须阻止重复刷新，并展示可见
    加载状态及可恢复的行内错误/重试状态；搜索无结果与项目注册表为空是两个
    不同状态。
14. Project 页面统一复用 GitHub/Primer PageHeader、Blankslate、Button、
    IconButton、TextField、Dialog 和 ScrollView 契约。对话框必填字段始终有
    可见标签，校验信息必须可描述，删除操作必须使用危险语义并二次确认。
15. `is_pr_auto_update` 当前表示“一次性预取 PR 数量”，而不是后台周期刷新。
    开启后，Project 列表加载或重建时会请求 FAT/UAT/PROD 概览；打开该项目的
    Workflow 时也会请求同一概览。当前实现没有轮询间隔或自动重试定时器。
16. Project 网格必须按受约束的页面内容宽度计算列数；新增一列前每张卡片至少保留
    280px 可读宽度。网格卡片和列表行都必须提供带项目名称的无障碍标签及键盘激活。
17. 每个项目网格卡片和列表行必须将持久化的 `repo_type` 呈现为紧凑的右上角
    45° 对角缎带，包含 Provider Logo 以及可读的 GitHub、GitLab 或 Gitee 名称，
    缎带两端由卡片边缘裁切。缎带不得占用独立元数据行；响应式宿主空间不足时必须
    先缩小，不能挤压项目身份或路径。品牌底色在明暗主题下保持固定：GitHub
    `#24292E`、GitLab `#FC6D26`、Gitee `#C71D23`，Provider 图形与文字统一使用白色。
18. 项目注册表读写必须离开 Slint event loop。新增、更新和删除统一 dispatch 强类型
    Action；每个有效 commit 自动进入 `PersistentStore`，单一 Worker 可把相邻快照合并为
    最新注册表，再通过同目录临时文件原子 rename 替换 `projects.json`。仅 Logo、概览和
    pending 状态变化时不得重写未变化的持久字段。初始注册表 hydration 与 Logo 解码只在
    首帧后启动；hydration 后才启用持久化，不能冗余回写刚加载的快照。Project 页面保持
    Loading，直到准备后的 model 在 event loop 提交。只有真实落盘完成后才提示成功；
    持久化失败也只能在失败的 durable 注册表仍为当前版本时回滚。
19. Provider adapter 统一使用 `src/git/prefix_cache.rs` 的 TTL/LRU 边界组装客户端逻辑页。
    GitLab Closed 按 Provider 排序合并 closed/merged 前缀；Gitee 公有版 Merged 与关闭未合并
    共用原始 closed 前缀，再在本地应用状态与关键词筛选。完整 Provider 页按 30 秒 TTL
    增量缓存，每个 adapter 最多保留 32 个仓库/筛选/状态/页大小 key，并按 IID 去重。
    显式刷新第 1 页必须绕过旧前缀；合并成功后必须使该仓库的全部前缀失效。

20. 远程 Logo 加载必须独立于 UI 投影。筛选后的项目 URL 与当前表单预览共享 URL
    去重及最多 4 个活跃请求。持续选中的每个 URL 总共最多尝试 3 次，失败后分别
    冷却 2/4 秒；普通 Store/UI 刷新不得绕过退避。显式刷新注册表可重置失败项，
    但必须保留正在执行的请求身份。移除来源后，其活跃请求完成时释放调度元数据；
    再次选中时开始新的预算。单一共享 runtime 驱动器持有请求 future 与下一次重试
    截止时间，不为每个等待 URL 创建任务/定时器。关闭 Project 持久化前取消 HTTP
    future。下载失败继续静默并显示默认 Logo。

## Deferred enhancement / 后续优化

The persisted `is_pr_auto_update` setting is a future candidate for a true
periodic refresh mode. Before changing its behavior, define the refresh
interval, project/page visibility scope, duplicate-request and cancellation
rules, silent HTTP feedback, failure backoff/retry policy, and visible
last-updated/loading/error state. If the one-shot behavior is retained, rename
the control so it describes preloading rather than automatic refresh.

持久化的 `is_pr_auto_update` 设置可在后续升级为真正的周期刷新模式。改动前需要
明确刷新间隔、项目/页面可见性范围、重复请求与取消规则、静默 HTTP 反馈、失败
退避/重试策略，以及可见的最近更新时间/加载/错误状态。若保留一次性行为，应将
控件改名为“预加载 PR 数量”，避免使用“自动刷新”。

## Provider requests / Provider 请求

| Provider | Detail request | Authentication |
| --- | --- | --- |
| Configured Gitee-compatible enterprise | `GET <api_base_url>/projects?search=<repo name>` | configured `enterprise` (when present), `OAUTH: enabled`, optional `Private-Token` |
| Gitee | `GET /api/v5/repos/<namespace>/<repo>` | optional Gitee token |
| GitLab | `GET <configured api_base_url or remote-derived /api/v4>/projects/<URL-encoded namespace/repo>` | optional `PRIVATE-TOKEN` |
| GitHub | `GET <configured api_base_url or https://api.github.com>/repos/<owner>/<repo>` | optional Bearer token |

The enterprise Gitee search result is selected by exact namespace/path where available; a single-result response is accepted as a fallback. GitLab and GitHub use their configured API root only for a remote with the same host.

企业版 Gitee 搜索结果优先按精确 namespace/path 选择；仅当响应只有一个项目时才接受单结果回退。GitLab 和 GitHub 也只有在 remote 主机匹配时才使用其配置的 API 根地址。

## Regression checklist / 回归清单

- Save a GitHub, GitLab, and Gitee project and verify `repo_project_id` plus `repo_type` in `projects.json`.
- For `git@gitee.example.com:group/repo.git`, verify the search query is `search=repo`.
- Return multiple same-name Gitee results and verify the exact remote namespace is selected.
- Simulate an unsupported remote, 401/403, or timeout and verify no partial project record is written.
- Save twice before the first lookup finishes and verify only one request is sent.
- Confirm the UI remains responsive during the lookup and no token appears in logs.
- Reload the project registry and verify the visible loading state, inline
  retry state on storage failure, no-project action, and filtered-empty reset
  action without blocking navigation.
- At the minimum supported width, verify the Project header/toolbar/cards and
  add/edit dialog remain usable, fields scroll inside the dialog body, and
  footer actions remain visible.
- Resize across grid column changes and verify no card becomes narrower than
  280 px; open both a list row and grid card with Enter/Space.
- Enter Workflow and verify the PR tab shows the saved provider/id and the first 10 open requests.
- For a GitHub PR page, verify every available `head.ref → base.ref` pair is shown and a failed row-detail request does not hide the rest of the page.
- Switch open/closed/merged/all and paginate rapidly; verify only the latest response updates the list.

- Use local 404, stalled and recovered image responses to verify finite attempts,
  cooldowns, explicit reload recovery, shared list/preview admission and shutdown
  cancellation. Batch 32 failing URLs and verify at most four concurrent requests.
- 使用本地 404、无响应和恢复后的图片响应，检查有限次数、冷却、显式刷新恢复、列表/
  预览共享准入与关闭取消；批量 32 个失败 URL 时并发请求不能超过 4 个。

## Bounded projection / 有界投影

- `projection.rs` selects registry identity, query, preview and image revisions.
  Save/request bookkeeping alone must not prepare or replace list models.
- At most one input, one prepared output and one event-loop delivery are pending.
  Older generations are rejected; unchanged rows retain their model/image identity.
- `logo_prepare.rs` performs file/base64/SVG/raster work on a worker. Source input
  is limited to 8 MiB; standalone raster decoding checks RGBA dimensions and sets
  the decoder allocation limit to 64 MiB. Shared thumbnails are at most 128 pixels
  per side. These are not a total-memory bound for SVG embedded resources. Only
  selected sources remain cached; diagnostics reuse byte metadata.
- Regression: repeated pending actions, rapid searches, overview bursts, local,
  SVG and failed remote previews, refresh/loading and shutdown during preparation.

- `projection.rs` 按注册表身份、查询、预览和图片版本选择更新；仅保存/请求记账
  不得重新准备或替换列表 model。
- 最多保留一个待处理输入、一个准备结果和一个排队 event-loop 回调。拒绝过期
  代际，未变化行保留 model 与图片身份。
- `logo_prepare.rs` 的文件/Base64/SVG/栅格工作在 Worker 执行。源输入最多 8 MiB；
  独立栅格图片检查 RGBA 尺寸并设置 64 MiB 解码器分配限制，共享缩略图每边最多
  128 像素。这不构成 SVG 内嵌资源的总内存上限。缓存只保留当前选择来源，诊断
  复用字节元数据。
- 回归覆盖 pending 连续变更、快速搜索、概览突发、本地/SVG/失败远程预览、
  刷新 loading，以及准备期间关闭。

Project SVG text uses the bundled Noto Sans SC font as its fallback, loaded on
demand from shared static bytes; projection never scans installed system fonts.

Project SVG 文字按需使用应用自带 Noto Sans SC 字体兜底，共享静态字节，
投影不得扫描已安装的全部系统字体。
