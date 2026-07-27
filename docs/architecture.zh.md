# Usher 架构文档

[English](architecture.md) · [README](../README.zh.md) · [规格索引](specs/README.md)

## 文档目标和范围

本文描述当前代码的真实结构、模块边界、运行生命周期和安全扩展点，是中文架构主文档。需要明确行为不变量的内容统一放在 `docs/specs/`。

Usher 是桌面优先的 Rust 应用。Slint 负责展示和声明式 UI 状态；Rust 负责持久化、进程执行、网络请求和应用装配。

生产构建在编译期固定选择 Winit + FemtoVG/OpenGL。Skia 和 FemtoVG/WGPU 作为显式启用的 Cargo feature，仅用于渲染器诊断和 A/B 对比，避免常规构建因同时链接多个实现而静默切换渲染器。Slint 应用内实时预览同样是显式启用的 `live-preview` 开发 feature，默认构建和生产构建不会编译它。当前 macOS 测试中 WGPU 虽降低 physical footprint，但会增加 RSS，因此不作为生产默认配置。

## 系统上下文

```text
用户
  -> Slint 主窗口和复用组件
     -> ui/main.slint 导出的 callback
        -> crates/usher-ui 负责的生成 API
           -> 由 src/lib.rs 绑定的 Rust 功能模块
              -> 本地 Git / PTY / 文件系统
              -> 统一 reqwest HTTP 管线
              -> Tokio runtime 或专用阻塞线程
        -> 通过 Slint event loop 回写 UI
```

当前外部集成包括 Git 命令、GitHub/GitLab/Gitee 兼容 API、用户配置的 OpenAI-compatible AI Agent Provider、基于 `self_update` 的 GitHub Release、系统托盘/窗口 API，以及平台原生应用目录。

## 目录和职责

### 应用装配和运行时

| 路径 | 职责 |
| --- | --- |
| `src/main.rs` | 最小二进制入口，转发到库入口。 |
| `src/lib.rs` | 导入并重新导出生成的 UI API，创建 `MainWindow`、初始化模块、绑定跨模块 callback，并管理窗口和关闭流程。 |
| `crates/usher-ui/` | 负责 Slint build script 和生成的 Rust API；编译仓库根目录的 `ui/main.slint` 树，并导出窗口、global、model、struct 和 enum。 |
| `src/app_runtime.rs` | 进程级多线程 Tokio runtime，并为 runtime 线程安装 tracing dispatch。 |
| `src/memory_diagnostics.rs` | 统一采集进程 RSS/footprint、启动检查点、UI 尺寸关联，以及可取消的周期内存采样。 |
| `src/shutdown.rs` | 按优先级协调关闭。 |
| `src/single_instance.rs` | 单实例进程行为。 |
| `src/tray.rs` | 桌面托盘和托盘触发的生命周期操作。 |

`src/lib.rs` 是 composition root，不应继续堆积功能逻辑。新业务应放入对应模块，只向装配层暴露精简的 setup 或 service API。

### 功能模块

| 路径 | 职责 |
| --- | --- |
| `src/project/` | 项目列表、持久化、过滤、Logo、PR/MR 概览以及进入 Workflow/Terminal 的入口。 |
| `src/codereview/api/` | Gitee、GitLab、GitHub 仓库接口的 URL 与路径模板；不包含请求执行、鉴权或响应解析。 |
| `src/git/` | Provider 识别、仓库实际操作、鉴权 header，以及将各 Provider 响应归一化为应用模型。 |
| `src/workflow/` | 按项目隔离的 Branch、Tag、PR、CD 状态和命令执行。 |
| `src/terminal/` | PTY 生命周期、viewport 同步、渲染、输入、选择和项目切换。 |
| `src/chat/` | 聊天请求、会话、JSONL 持久化、Markdown、渲染缓存、分页和 UI 绑定。 |
| `src/update/` | Release 检查、流式下载、进度、取消、安装和重启。 |
| `src/router/` | 路由到导航项的映射，以及各路由的可见性状态。 |
| `src/settings.rs` | TOML 设置模型和持久化，包含 AI Agent 凭据。 |
| `src/ai/` | 独立的 OpenAI-compatible AI Agent 集成；`agent.rs` 负责连通性测试，不负责 RAG Chat。 |
| `src/network.rs` | 后台网络探测和 `AppState.network-online`。 |
| `src/http/mod.rs` | 共享异步/阻塞 reqwest 客户端、拦截器、统一错误和可选 UI 反馈。 |
| `src/logger/` | Debug 日志、tracing 和 panic/error 支持。 |
| `src/toast.rs` | 单条短时 Toast 管理器绑定。 |
| `src/notification.rs` | 通知队列、计时器、Hover 暂停、操作按钮和图标。 |
| `src/dialog.rs` | 全局 Modal Dialog 绑定。 |

仓库接口地址与可执行行为必须保持分离：`src/git/` 可以引用 `src/codereview/api/` 中的常量，但 `src/codereview/api/` 不得依赖 `src/git/`、`src/http/`、reqwest 或 serde。

### Slint UI

| 路径 | 职责 |
| --- | --- |
| `crates/usher-ui/build.rs` | 编译根 Slint 源码树，并将内置 Material 库映射为 `@material`。 |
| `crates/usher-ui/src/lib.rs` | 包含并公开导出 Slint 生成模块。 |
| `ui/main.slint` | 根窗口、路由装配、Rust-facing property/callback、Modal 层和 Terminal Dock。 |
| `ui/route/routeMap.slint` | 完整 `Route` 枚举；主导航只开放其中一部分。 |
| `ui/state/` | Slint 全局状态适配。 |
| `ui/views/` | 页面级视图。 |
| `ui/views/workflow/` | Branch、Tag、PR、CD 视图。 |
| `ui/components/` | 应用复用组件。 |
| `ui/theme/` | 应用主题色映射。 |
| `material-1.7.1-fixed/` | 与 Slint v1.17.1 对齐的 Material 源码。 |
| `vendor/i-slint-live-preview/` | 仅由显式 `live-preview` feature 选择的临时 Slint 1.17.1 override；其中有文档记录的 `mtime` 保护避免 macOS 文件监听误报引发无限重编译。 |

应用 UI 原则上通过 `@material` 导入 Material。只有确实需要访问未导出组件时才允许直接引用内置路径。修改任一 vendored dependency 前必须与上游对比并记录原因；上游提供等价文件监听保护后，应删除 live-preview override。

UI 源码继续保留在仓库根目录的 `ui/` 下，但生成的 Rust 只由 `usher-ui` workspace crate 编译。应用 crate 重新导出这套公开 API，使现有功能模块仍可通过 `crate::{MainWindow, Route, ...}` 引用类型，而不再拥有生成模块。这个边界保证普通 Rust 功能逻辑变化不会迫使 Slint 生成 crate 重编；UI 源码变化则先重编 `usher-ui`，再检查应用绑定。

## 构建产物策略

workspace 根 `Cargo.toml` 中的 profile 统一负责构建产物策略。日常 `dev` 与
`test` 使用行表级调试信息、`split-debuginfo = "off"`，同时保留增量编译。
这会覆盖 macOS 上 Cargo 默认的 full/unpacked 调试策略：panic backtrace
仍有文件名和行号，但 `target/debug/deps` 不再积累松散 `.rcgu.o`。

需要 LLDB 查看完整变量和类型时，显式使用 `dev-full` profile。它继承日常开发
设置，但把完整 packed 调试信息写入独立 profile 目录；因此可以通过
`cargo clean --profile dev-full` 单独移除，而不丢弃日常开发产物。Release 和
dist profile 保持独立：Release 继续禁用调试信息和增量编译。

workspace 全目标 IDE 后台诊断使用隔离的 `ide` profile：不生成调试信息，也不
保留增量状态，避免后台分析重复制造数 GiB 的应用与生成 UI 缓存。VS
Code/Cursor 通过 workspace 设置自动选择该 profile；RustRover 则在 Cargo
external linter 中选择。交互式 build、run、test 仍使用各自的增量 profile。

Cargo 目前不会对 workspace 的 `target/` 实施容量上限或自动保留策略。从旧的
unpacked 策略升级后，开发者应执行一次完整 `cargo clean`；后续优先按 profile
或交叉编译 target 定向清理。可复现测试数据与命令记录在
[build-performance.md](build-performance.md)。

## 启动生命周期

`src/lib.rs` 中的实际初始化顺序具有依赖关系：

1. 创建窗口前加载设置，以便选择平台外观。
2. 创建 `MainWindow`，初始化 debug/tracing logger。
3. 应用设置并配置 updater 状态。
4. 绑定主题、路由、导航和窗口 callback。
5. 初始化 Notification 和 Toast manager。
6. 将统一 HTTP 模块绑定到 UI 错误反馈，然后启动网络监测。
7. 初始化 Dialog、Project、Chat、Terminal 和 Workflow。
8. 安装托盘并进入 Slint event loop。
9. 关闭时通过优先级清理 Terminal、Updater、Tray、Runtime 和子进程。

如果模块依赖生成的 Slint global 或前一个模块安装的 callback，不要随意提前初始化。

Debug 构建默认启用进程内存诊断；Release 构建通过 `USHER_MEMORY_DIAGNOSTICS=1` 启用。`[Memory][Diag]` 记录当前/峰值 RSS、物理 footprint、VM 分类、region、Mach port、线程、fault、page-in，以及相对上次和启动基线的增量；`[Memory][UI]` 将稳定后的路由/窗口变化与逻辑、物理尺寸关联。`[Workflow][PR Review][Memory]` 等功能归属日志只记录有界 model/cache 的数量和容量，不得输出源码内容或凭据。采样任务复用共享 runtime，并由正常的 pre-shutdown 流程停止。

## 并发模型

- Slint 对象和 global 归 event loop 所有。后台任务必须通过 `slint::invoke_from_event_loop` 更新 UI。
- 异步 IO 统一使用 `src/app_runtime.rs` 的共享 Tokio runtime；不要为单次操作新建 runtime 或 reqwest client。
- 现有阻塞 Git、PR 统计和文件操作在专用线程或 Tokio blocking task 上执行，禁止阻塞 Slint event loop。
- Rust 共享状态按所有权使用 `Arc<Mutex<_>>`、`Arc<RwLock<_>>`、Atomic 或线程本地缓存。
- 长任务必须具备取消或防重入机制。更新下载、历史分页、Workflow Run 和 Terminal Session 可作为参考。

## 状态和持久化

### 设置

`AppSettings` 以 TOML 保存到平台配置目录，包括主题、自动更新、日志开关、Terminal 诊断、导航抽屉状态、Git 平台 Token、位于 `[git.platform.<provider>]` 的各平台企业版 API 地址和企业名称，以及位于 `[ai.agent]` 的 OpenAI-compatible AI Agent 连接配置。Gitee 的公有版 `token` 与 `enterprise_token` 分开保存；只有旧配置已经存在企业 API 地址时，旧版单一 Token 才迁移到企业字段，两套凭据不得跨 profile fallback。Slint 需要的数据同步到 `AppState`；API Key、企业名称和配置 URL 均属于本地配置，不得写入日志。Provider API 只有在企业 API 地址主机与项目 remote 主机匹配时才使用该配置；用户既可填写服务主机，也可填写完整 API 根地址，缺少版本路径时由 Provider 自动补齐。否则继续使用由 remote 推导出的公共平台默认地址。

### 项目

`project/projects.json` 保存于平台 local-data 目录。新增或编辑项目保存时，Rust 根据 `remote_repo` 推导 Provider，由 `src/git/` 使用 `src/codereview/api/` 中的 URL 模板查询项目详情，并把返回的 `repo_project_id` 与小写 `repo_type`（`gitee`、`gitlab`、`github`）一并保存。查询失败不会改写已有配置。Rust 将记录转换为 Slint model，解析本地/Base64/远程 Logo，并调度 PR 概览任务。远程 Logo 缓存位于进程临时目录，UI 渲染路径不应直接进行网络 IO。

### Workflow

`WorkflowStore` 保存当前项目和各项目的 `WorkflowViewState`。切换项目必须重新绑定 UI，避免日志、步骤和 running 标记串到其他项目。`Route.Workflow` 与 `Route.PrReview` 构成同一生命周期域：两者之间切换保留 Workflow 列表/model 状态；临时进入域外路由时，只要仍可通过 Back 返回任一 Workflow 路由，也继续保留这些状态。

跨页面返回由 `src/router/` 统一维护 LIFO 路由历史栈；Slint 只渲染当前路由并委托 Back，不再自行改写单槽位的 `previous-route`。不改变 Route 的页内下钻返回点由对应 feature controller 管理。Back 先消费 feature 页内返回点，再弹出全局路由历史。只要历史栈中仍保留 Workflow 路由，它就是可返回的活动目标，因此从 PR 详情临时打开设置、关于或其他页面时不得清理审核状态；只有 Workflow 域既不是当前路由、也无法通过 Back 返回时，才释放重状态。

Workflow PR 列表与全窗口审核工作区统一通过 `src/git/` 的 Provider-neutral façade 请求数据。所有列表、详情/diff、仓库文件、review state、review submit 和 merge 请求都根据当前项目持久化的 `repo_type` 选择实现，并在执行前校验其与 `remote_repo` 一致。审核控制器只持有一份共享且不可变的归一化 diff，并用独立 action 序列防止迟到的 review/merge 响应更新其他项目或 PR。归一化详情会把每个基础文件移动到独立 `Arc` 中，当前文件的惰性 model 与高亮任务直接共享该分配，不再深拷贝全部行；只有展开 hunk 时才创建对应文件的写时复制 override。Provider HTML 在 adapter 边界解码，每条 diff 记录在进入固定高度的 Slint 行渲染前都会归一为不含 CR/LF 控制符的单个视觉源码行。收起文件时删除 override 并重新使用共享基础文件。hunk 展开或应用内查看完整文件时按固定 head/base commit 懒加载源码；修改文件展开时会在本地对齐两份固定版本，确保单侧独有行不会丢失。每文件 Diff 高亮使用 4 项 LRU，固定源码文本使用 3 项 LRU；完整文件高亮 model 只是临时 UI 状态。当前 Diff 通过只读惰性 model 暴露给 Slint，最多物化 64 行转换后的 UI 数据；完整高亮结果继续由 Rust 持有，以保留任意语言支持和快速滚动。所有异步回写都使用序列号防止迟到响应污染当前项目或 PR。

Revert 命令与 review/merge 命令共用 Provider-neutral façade 和 action 序列，迟到响应不得影响其他项目或 PR。已合并请求隐藏 review/merge 控件；只有 adapter 返回 `can_revert` 时，页头才在就绪徽标同一行显示明确可见的 Primer 风格次级回退按钮（Usher 不用 GitHub 页头 kebab ActionMenu，以保证完整标签可读）。GitHub GraphQL 创建独立的反向 PR；Gitee 企业版重新读取 MR，要求 merge/squash commit 的 `/repository/commits/:sha/refs` 响应同时返回 `merge_commit_by_pr` 与 `can_revert`，随后对目标分支调用文档定义的 commit revert endpoint。rebase/fast-forward、Gitee 公有版与 GitLab 均保持不支持；Provider 专属确认说明必须明确 Gitee 会直接修改目标分支。

Gitee 企业版 Commit 角标只把 MR detail/diff 中的正数摘要视为可信总数。部分部署在读取 commits 集合前会报告零，因此零值或缺失时必须在激活 Commits Tab 前执行轻量计数探测；完整 Commits 内容 model 仍保持懒加载。

Files changed 的 `Diff view` 弹层统一持有与 Provider 无关的 Unified/Split 偏好，以及按能力开放的 Hide whitespace 草稿。`workflow/pr_review.rs` 从缓存的归一化行生成 Unified 或 Split 投影；Split 将每个连续删除/新增块按行配对，并把上下文同时映射到左右两侧。只应用视图选择时在本地替换当前懒加载行 model，应用受支持的空白字符变化时才重新加载 Provider 数据。Slint 负责渲染投影，并让不支持的空白字符复选框保持可见但禁用。弹层完整表面持有指针拦截层，因此空白区域不会把 hover 穿透到下面的文件树。弹层挂在整行 Files changed 工具栏上，Primer 向上三角箭头的尖端跟随设置齿轮，即使面板被边缘夹紧也保持对准。

大文件按与 Provider 无关的变更行数或载荷体积预算判定。正常文件头保持可用，但语法高亮和 Slint 行 model 延迟到用户对该文件点击 `Load diff` 后才创建。原位加载状态仅属于当前完整 Diff/commit range，切换范围时重置。

仓库 PR 列表归现有 Workflow 的 PR Tab 所有。`workflow/pr_list.rs` 在 event loop 之外读取当前项目持久化的 `repo_project_id`、`repo_type`，并且只在进入或用户切换页码时，通过 `src/git/` 懒加载对应的 10 条远端数据。GitHub 保留搜索接口提供的总数，并发请求当前页每个 PR 的详情，从而补齐源分支和目标分支名称。控制器在将列表值传给 Slint 前，把可解析的 Provider 更新时间统一格式化为东八区（UTC+08:00），并在 Provider 提供时使用准确总数驱动顶部总数和底部分页。页面默认显式查询 open 状态，用户可切换 open/closed/merged/all；请求序号保证旧项目、旧筛选或旧页码的延迟响应不会覆盖当前视图。请求成功且零条数据时显示随筛选状态变化的插画空状态；网络、鉴权或响应结构异常仍使用独立且可重试的错误状态。点击列表行后导航到全窗口 `Route.PrReview`；Workflow 组件实例会被销毁，但列表/root model 状态继续保留，因此 Back 可在不重载列表的情况下重建 PR Tab。审核路由拥有 GitHub 风格的 Conversation、Commits、Files changed 三个 Tab。Conversation 与 Commits 的 *内容* 只在第一次选中时通过 Provider-neutral model 加载：GitHub 组合 PR detail、issue comments、reviews，以及 issue timeline 活动（`committed` 批量、`merged` 与部分系统事件）；Gitee 公版 v5 使用 PR detail/comments/commits，并从 commits 与合并元数据合成提交/合并活动；Gitee 企业版 v8 使用 MR detail/notes/commits（并尽力加载 `/reviews`），在展开嵌套 `notes[].note` 后同样合成活动，普通评论正文从 `note` / 嵌套 `note.note` / `comment` / 字符串 `content` 读取（忽略对象形态的 `content`），MR commit 时间取 `created_at`；GitLab 使用 MR detail/notes/commits，合成同一套可移植的 `commits`/`commit`/`merged` 行并过滤重复系统 note。Tab 角标数字不等待 Conversation/Commits Tab 激活：它们来自随 diff 加载携带的轻量 PR/MR 摘要字段（`commit_count`、`conversation_count`、`files.length`）。若 Provider 省略总数，adapter 可发起一次有界计数探测（GitHub/GitLab 的 Link/`X-Total` + `per_page=1`；Gitee 公有版短页 `per_page=100`；Gitee 企业版 diffs `commits_size`、MR `user_notes_count`/`notes_count`，或 `with_page=true&per_page=1` + `meta.total_count`）。完整 Conversation 时间线与 commits 列表仍只在打开对应 Tab（或 commit 选择器）时懒加载。Provider-neutral 终结器把每个 `commits` 头与其后连续的 `commit` 行作为一个不可拆分的块排序，因此即使各提交 authored time 不同，三家 Provider 的 GitHub 风格 `added N commits` 记录也不会与提交列表分离。Conversation 批次内的 commit 子行与 Commits Tab（含 Files changed 的 commit 选择器）均按从新到旧展示；adapter 仍把列表归一为从旧到新，控制器在连续范围比较前把展示索引映射回去。远端 review 或 merge 成功后会使 Conversation 时间线缓存失效；若 Conversation 正处于激活状态，则立即重新加载。Files changed 控制器加载并归一化所选 PR diff，根据文件路径构建可折叠目录树，并让路径筛选、目录展开和上一项/下一项文件导航保持本地操作。其 commit 选择器复用懒加载提交列表，把连续范围分发到各 Provider 的仓库 compare endpoint；原始 PR detail 和已加载范围在当前审核期间保持缓存。`workflow/syntax_highlight.rs` 在 UI 层之外使用 `syntect` 把当前文件转换为适配主题的 token model，同时保留 Tab 和显示列宽。切换文件或展开 hunk 时，旧行 model 会保留到新高亮 model 完成，再由 event loop 原子替换；只有真正切换文件时才重置 viewport。点击 hunk 时先记录被点击行的实际屏幕位置。model 替换后，目标 delegate 上报布局后的内容位置，Slint 在有界的多个布局帧内重复应用由此得到的视觉锚点。该方案不再估算软换行行高：仍存在的 hunk 控件保持在指针下方；若分隔行消失，首条新增源码占据原控件位置。在 macOS 上，原生 live-resize 通知会冻结审核工作区的内部几何尺寸，并把每条可见行的 token repeater 临时替换为一个纯等宽文本节点；live resize 结束时恢复最终几何尺寸和完整的 token 着色组件树。

审核页头采用 GitHub 的两步命令模型。标题旁的 GitHub 风格就绪徽标使用 Primer 语义色渲染归一化类型/详情，但合并语义由各 adapter 独立负责：GitHub 映射 REST mergeability，Gitee 企业版映射 v8 `current_state` 条件，Gitee 公有版只映射 v5 字段，GitLab 映射 `detailed_merge_status` 并请求异步重算。共享 enum 只是展示边界，绝不能把一个 Provider 的判定条件套到另一个 Provider。`Review changes` 提交显式的 Provider-neutral Comment、Approve 或 Request changes；不受支持的选项保持可见但禁用。Conversation Tab 底部另有 GitHub 风格纯文本评论框（`Write` + 绿色 `Comment`），提交同一 Comment 动作。Provider 成功后刷新远端状态，绝不自动开始合并。`Merge pull request` 只由最新的归一化 `PullRequestActionState` 启用，并且只展示 adapter 返回的策略。GitHub 支持仓库启用的 merge、squash、rebase；Gitee 企业 v8 映射评审状态，Approve 未填写 summary 时省略可选的 `comment` 字段，并按 `restrict_merge_type` 支持 merge、squash、rebase、fast-forward；Gitee 公有 v5 支持评论和 merge/squash，但没有 approve/request-changes endpoint。GitLab 就绪状态只读，远端写操作继续作为显式不支持的能力。

审核文件树使用 GitHub 的平台 UI 字体栈（macOS 使用 SF 系统 UI 字体，Windows 使用 Segoe UI），代码区使用平台等宽字体栈。变更文件树和 Diff 行都使用虚拟化 `ListView` delegate，只实例化当前可见行。分隔线实时调整面板宽度；移动手柄以当前宽度叠加剩余的局部指针位移，使分隔线贴合指针，同时避免重建完整文件树或 Diff。会话宽度限制为 220–520 px 且不超过审核区宽度的 45%，确保 Diff 仍是主要区域。可见性对齐 GitHub Primer `$width-md`（768px）：宽度越过临界值向下时自动折叠、向上时自动展开；工具栏开关在任意宽度下都可用，窄屏自动收起后仍可手动打开。主窗口最小宽度为 640px，保证 768px 临界值可以到达。

审核页头使用紧凑的 36 px Primer 风格返回 IconButton 和官方 `arrow-left` Octicon。移除宽文字控件及分隔线后，换行标题获得更多空间；紧凑布局的操作区紧跟该图标开始。返回 Tooltip 仍在图标下方弹出，但挂在页头层并以高 z 绘制，避免被摘要行 Open 徽章或就绪控件遮挡。就绪详情、文件扩展名筛选和文件操作菜单由审核工作区统一管理为互斥的窗口内 popover；在触发器之间切换时必须保留 hover，并在第一次点击时直接打开目标 popover，不能依赖原生 popup 的鼠标 grab。

Conversation adapter 归一化 GitHub、GitLab、Gitee 公有 v5 与 Gitee 企业 v8 的可选作者和 commit 作者头像 URL。控制器通过共享 HTTP pipeline 并发获取去重头像，整体超时 3 秒、单图上限 2 MiB，并限制解码尺寸；只有可跨线程发送的 RGBA 字节进入 event loop，再创建 Slint Image。头像缺失或加载失败时回退为作者首字母。时间线卡片和评论框共用 52 px 头像/内容网格。GitHub 风格活动使用一条贯穿头像中心的 Primer 时间线轨道（首尾按 clipSidebar 裁剪）。评论/描述卡片为 1px 边框盒子并随短正文收紧高度；commit 图标与紧凑作者头像落在同一轨道；所有活动行与卡片右边缘对齐。Rust 提供准确的行高总和。Conversation Tab 把时间线与评论框放在同一文档滚动里，Write 区域跟在最后一条事件之后，而不是钉在视口底部。

Conversation 提交批次使用 Primer 官方 `repo-push` Octicon；相对时间紧跟 `added N commits`，共享的绝对时间 Tooltip 保留 Provider 的准确时间。GitHub Review 会展开为不可拆分的活动摘要与可选正文卡片；review state 决定话术，相对时间采用 GitHub 自动单位，merged 活动抑制冗余 closed 事件。连接线按事件语义边界分段，但连续 Review 对之间必须从前一张卡片到下一条 summary 标记保持连续轨道。Gitee 企业版普通评论使用 MR `/notes`，并固定发送 `noteable_type: PullRequest`；`/reviews` 只用于 Review 决策。commit 计数探测若没有 total 且响应恰好填满请求页，即使是裸数组也视为不确定，随后统计并丢弃文档约定的完整非分页集合，不挂载懒加载 Commits model。

Diff 几何只使用同一份实测宽度：Rust 在显示列元数据之外传入最长的已展开代码行，Slint 使用当前等宽字体测量，并让横向滚动与每行背景共用该像素宽度，从而避免最右侧露出未绘制区域，并保持 Tab/双宽字符对齐。代码栏支持 GitHub 风格的跨 ListView 行拖拽选中（不含行号栏；hunk/meta 行不可选且不进入剪贴板）；review controller 的同步命中测试按投影 model 中每个逻辑行的实际换行高度遍历，因此正向或反向拖入软换行终点时都能精确落到字符。⌘/Ctrl+C 静默写入剪贴板。选中状态只保存两个端点，选区矩形只由已选且可见的 delegate 创建，因此保留有界懒加载 model 与普通 token-only 滚动路径。

Conversation 与 Commits 的 commit 链接会按 SHA 定位并复用 Files changed 工作区，把当前 commit 应用为单项范围。审核控制器会把来源 Conversation 或 Commits Tab 记录为 feature 页内下钻返回点，因此页头 Back 会先恢复来源 Tab，下一次 Back 才返回 Workflow 的 PR 列表；如果 commits 尚未加载，控制器会在加载完成后应用待处理选择，复制 SHA 保持独立。

### Pull Request 子系统边界

PR 功能由五个可替换层组成。这是后续迁移到其他技术栈时应保持的边界：

```text
展示层
  PR 列表/表格 + 独立审核路由
        ↕ 归一化 view model 与 loading/error 状态
Workflow controllers
  项目选择、页码/状态切换、旧结果防护、审核缓存
        ↕ Provider-neutral use cases
仓库服务层
  校验持久化 Provider 类型与 remote URL；列表、diff、源码、review、merge、revert
        ↕ Provider-neutral domain records
Provider adapters
  GitHub、GitLab、Gitee 公有服务、Gitee 兼容企业 API 映射
        ↕ 统一 HTTP/auth/persistence
基础设施层
  项目注册表、本地 token/配置、HTTP client/interceptors、runtime
```

展示层不接收 Provider JSON 或凭据；Provider adapter 不拥有 UI 状态。仓库服务层是唯一可在校验 remote URL 后选择 adapter 的层，并把空白字符过滤、评审动作、可合并性和允许的合并策略暴露为归一化能力，而不是让 UI 自行推断。当前应用通过 Slint、Tokio 和 Rust mutex 实现这些层，但长期契约是归一化的 page/item/diff/source/action-state 数据，以及 [PR 工作流规格](specs/pr-review.md)定义的“最新请求优先”生命周期。Approve/Comment/Request changes 等决策只通过 Provider review 命令写入远端。

### Chat

聊天历史以 append-only JSONL 保存到 `im/chat_history.jsonl`。启动时最近一段只加载到 Rust session；Chat 组件及有界 Slint 消息 model 仅在 Chat 路由挂载。离开 Chat 会销毁 UI 树并清空 Slint model/Markdown 缓存，重新进入时从 session 重建可见窗口。迟到的历史分页或 assistant 响应继续更新持久化/session，但不得填充隐藏 UI。渲染和滚动锚点计算分离，以减少滚动/缩放期间的工作量。

### Terminal

Terminal 状态感知项目；同一项目重新打开时复用，切换项目时重置会话展示并切换工作目录。稳定行为契约见 [specs/terminal.md](specs/terminal.md)。

## 主要数据流

### Project 到 Workflow

1. Project 卡片触发根组件的 workflow-open callback。
2. `workflow/bindings.rs` 构建 `WorkflowProjectContext`。
3. `WorkflowStore` 选择项目级状态。
4. 各 Workflow 模块初始化对应 Slint property。
5. PR 数量在线程中加载，再通过 event loop 回写。
6. PR 列表控制器读取持久化仓库元数据，经共享 runtime 懒加载当前选中的 open 远端分页；每次翻页只请求对应页。

### PR 列表到审核

1. 列表控制器在 UI 线程外读取仓库元数据；项目或列表状态变化时清空页缓存。
2. 仓库服务拒绝与 `remote_repo` 不匹配的已持久化 `repo_type`，然后把请求路由到对应的 Provider adapter。
3. Adapter 将 Provider 响应归一化为最多 10 条的页面。GitHub 保留搜索接口的准确总数，并发读取当前可见行详情以补充分支 ref。
4. 控制器只在请求序列仍为最新时应用页面；在生成 view model 前将可解析的更新时间格式化为东八区（UTC+08:00）。
5. 打开某一行后，在共享 Workflow 生命周期域内从 `Workflow` 导航到 `PrReview`。Workflow 组件被卸载但列表/root model 保留；审核控制器重新读取元数据、请求一份归一化 diff，按项目/请求/空白模式缓存并渲染本地文件树。
6. 文件选择、筛选、目录展开、复制路径、比例条渲染、语法高亮和收起已展开上下文均在本地完成。只有 hunk 展开和应用内 View file 会产生后续 Provider 读取；两者共享有界的固定源码缓存。方向展开以 GitHub 登录后的新版 `/changes/{commit}` 页面为基准，每页补充 21 个主侧源码行；未登录时回退的经典 `/commits/{commit}` 页面采用不同的 20 行行为，不是本工作区的设计基准。hunk 间剩余上下文不超过一页时，双向控件切换为单个“全部展开”，补齐最后上下文后移除已失效的 hunk 分隔行。文件级展开全部补齐当前文件全部隐藏区间，收起则恢复保留的原始 diff。
7. 返回 Workflow 或切换项目会使审核/Conversation/Commit/高亮/源码请求序列失效，迟到任务不得影响新上下文。

### 项目保存与 Provider 查询

1. Project 页面把本地仓库和远程仓库字段提交给 Rust callback。
2. `src/project/mod.rs` 校验本地字段，并为每次项目保存启动一个共享 runtime 查询任务。
3. `src/git/` 解析 `remote_repo`，使用 `src/codereview/api/` 的常量构建 Provider 请求，再通过 `src/http/mod.rs` 执行。
4. 只有查询成功才会将 `repo_project_id`、`repo_type` 写入 `projects.json`；UI 刷新回到 Slint event loop 执行。
5. 查询或持久化失败时只显示一次项目模块错误 Toast，不写入不完整记录。

### 项目终端

1. Project 卡片操作或 Workflow FAB 触发 Terminal callback。
2. Rust 解析 project key 和本地仓库路径。
3. `open_terminal_for_project` 决定复用还是重置。
4. PTY 会话在 UI 线程之外运行。
5. 渲染结果通过根 property 交给 `TerminalDrawer`。

### Chat 请求

1. `chat/interactive.rs` 追加并持久化用户消息。
2. 共享 runtime 使用既有 RAG endpoint 和请求格式调用 `chat/chat.rs`。
3. `src/http/mod.rs` 依次执行请求拦截、reqwest 请求、状态校验、响应/错误拦截和 RAG JSON 解码。
4. 结果持久化，并在 event loop 中转换为 Slint model。
5. 失败会作为 assistant 消息显示，因此该请求保持静默，避免重复 Toast。

### AI Agent 连通性测试

1. 设置页独立保存 OpenAI-compatible Base URL、API Key 和模型名称。
2. 用户触发测试后，共享 runtime 执行 `ai/agent.rs`。
3. 统一 HTTP 模块请求 Provider 的 `/models` endpoint，并通过 Toast 反馈结果。
4. 此流程不得改变 RAG Chat 的 endpoint、payload 或历史行为。

### 更新下载

1. `self_update` 在 blocking task 中解析 Release 元数据。
2. 统一 HTTP 管线启动具有长超时的流式下载。
3. 数据块写入临时压缩包并上报进度。
4. 取消时删除未完成文件。
5. Update UI 已管理成功/失败通知，因此 HTTP 层保持静默。

## HTTP 边界

`src/http/mod.rs` 是唯一允许创建应用级 reqwest client 的位置。它返回 reqwest request builder，不隐藏 query、JSON、header、timeout 和 streaming 能力。

每个请求都提供 `RequestOptions`：

- `operation` 用于稳定日志和用户提示上下文。
- `reject_http_errors` 默认开启，把非 2xx 响应转换为 `HttpError`，并保留受长度限制的响应正文摘要。
- `feedback` 默认 `Silent`；用户操作可以选择 Toast、Notification 或两者。

Interceptor 按注册顺序执行。Request hook 可修改 header；Response hook 可读取状态、header 和耗时；Error hook 接收统一错误。详见 [specs/http.md](specs/http.md)。

## UI 组合规则

- Rust-facing property 和 callback 统一在 `ui/main.slint` 声明并保持命名一致。
- Feature view 负责布局；Rust 负责 IO 和业务状态转换。
- 优先复用 `PageScrollActions`、`ProjectCardActions`、`IconActionButton` 和项目终端入口。
- Hover 关键节点应常驻，通过 opacity/state 控制，不要条件创建命中区域。
- 全窗口 Modal 必须挂在根层。
- 滚动和窗口缩放期间避免重建 layout/model，精确计算应延迟到交互稳定后。

## 验证要求

Rust 或 Slint 改动至少运行：

```sh
cargo fmt --all
cargo check --locked
cargo test --lib --locked
```

Terminal viewport/session、更新取消、托盘关闭和路由布局需要做针对性手工验证。完整测试目前包含一个无关的 logger doctest 失败，不能用这个已知问题掩盖新增失败。

## 已知技术债

- `src/logger/debug_logger.rs` 有一个无法编译的 doctest。
- `Route` 中部分页面是开发/演示界面，没有进入主导航。
- 根项目许可证仍需明确。
- `docs/terminal_viewport_iteration_notes.md` 是历史证据，不是当前规格。
