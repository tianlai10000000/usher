# Usher 架构文档

[English](architecture.md) · [README](../README.zh.md) · [规格索引](specs/README.md)

## 文档目标和范围

本文描述当前代码的真实结构、模块边界、运行生命周期和安全扩展点，是中文架构主文档。需要明确行为不变量的内容统一放在 `docs/specs/`。

Usher 是桌面优先的 Rust 应用。Slint 负责展示和声明式 UI 状态；Rust 负责持久化、进程执行、网络请求和应用装配。

## 系统上下文

```text
用户
  -> Slint 主窗口和复用组件
     -> ui/main.slint 导出的 callback
        -> Rust 功能模块
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
| `src/lib.rs` | 创建 `MainWindow`、初始化模块、绑定跨模块 callback、管理窗口和关闭流程。 |
| `src/app_runtime.rs` | 进程级多线程 Tokio runtime，并为 runtime 线程安装 tracing dispatch。 |
| `src/shutdown.rs` | 按优先级协调关闭。 |
| `src/single_instance.rs` | 单实例进程行为。 |
| `src/tray.rs` | 桌面托盘和托盘触发的生命周期操作。 |

`src/lib.rs` 是 composition root，不应继续堆积功能逻辑。新业务应放入对应模块，只向装配层暴露精简的 setup 或 service API。

### 功能模块

| 路径 | 职责 |
| --- | --- |
| `src/project/` | 项目列表、持久化、过滤、Logo、PR/MR 概览以及进入 Workflow/Terminal 的入口。 |
| `src/codereview/api/` | 远程仓库 Provider 解析，以及 Gitee/GitLab/GitHub 项目详情 API；在项目持久化前解析不可变的 Provider 项目 ID。 |
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

`src/git/` 当前主要是占位目录，实际 Provider HTTP 行为仍在 `src/project/pr_stats.rs`。不要误以为这些空文件已经形成可用抽象。

### Slint UI

| 路径 | 职责 |
| --- | --- |
| `ui/main.slint` | 根窗口、路由装配、Rust-facing property/callback、Modal 层和 Terminal Dock。 |
| `ui/route/routeMap.slint` | 完整 `Route` 枚举；主导航只开放其中一部分。 |
| `ui/state/` | Slint 全局状态适配。 |
| `ui/views/` | 页面级视图。 |
| `ui/views/workflow/` | Branch、Tag、PR、CD 视图。 |
| `ui/components/` | 应用复用组件。 |
| `ui/theme/` | 应用主题色映射。 |
| `material-1.7.1-fixed/` | 与 Slint v1.17.1 对齐的 Material 源码。 |

应用 UI 原则上通过 `@material` 导入 Material。只有确实需要访问未导出组件时才允许直接引用内置路径。修改内置库前必须与上游对比并记录原因。

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

## 并发模型

- Slint 对象和 global 归 event loop 所有。后台任务必须通过 `slint::invoke_from_event_loop` 更新 UI。
- 异步 IO 统一使用 `src/app_runtime.rs` 的共享 Tokio runtime；不要为单次操作新建 runtime 或 reqwest client。
- 现有阻塞 Git、PR 统计和文件操作在专用线程或 Tokio blocking task 上执行，禁止阻塞 Slint event loop。
- Rust 共享状态按所有权使用 `Arc<Mutex<_>>`、`Arc<RwLock<_>>`、Atomic 或线程本地缓存。
- 长任务必须具备取消或防重入机制。更新下载、历史分页、Workflow Run 和 Terminal Session 可作为参考。

## 状态和持久化

### 设置

`AppSettings` 以 TOML 保存到平台配置目录，包括主题、自动更新、日志开关、Terminal 诊断、导航抽屉状态、Git 平台 Token、位于 `[git.platform.<provider>]` 的各平台企业版 API 地址和企业名称，以及位于 `[ai.agent]` 的 OpenAI-compatible AI Agent 连接配置。Slint 需要的数据同步到 `AppState`；API Key、企业名称和配置 URL 均属于本地配置，不得写入日志。Provider API 只有在企业 API 地址主机与项目 remote 主机匹配时才使用该配置；用户既可填写服务主机，也可填写完整 API 根地址，缺少版本路径时由 Provider 自动补齐。否则继续使用由 remote 推导出的公共平台默认地址。

### 项目

`project/projects.json` 保存于平台 local-data 目录。新增或编辑项目保存时，Rust 根据 `remote_repo` 推导 Provider，通过 `src/codereview/api/` 查询项目详情，并把返回的 `repo_project_id` 与小写 `repo_type`（`gitee`、`gitlab`、`github`）一并保存。查询失败不会改写已有配置。Rust 将记录转换为 Slint model，解析本地/Base64/远程 Logo，并调度 PR 概览任务。远程 Logo 缓存位于进程临时目录，UI 渲染路径不应直接进行网络 IO。

### Workflow

`WorkflowStore` 保存当前项目和各项目的 `WorkflowViewState`。切换项目必须重新绑定 UI，避免日志、步骤和 running 标记串到其他项目。离开 Workflow 路由时会主动清理重型状态。

仓库 PR 列表归现有 Workflow 的 PR Tab 所有，不新增独立项目详情 PR 页面。`workflow/pr_list.rs` 在 event loop 之外读取当前项目持久化的 `repo_project_id`、`repo_type`，并且只在进入或用户切换页码时，通过 `src/codereview/api/` 懒加载对应的 10 条远端数据。接口响应使用 `meta.total_count`、`meta.total_pages` 返回准确总条数和总页数，控制器直接用其驱动顶部总数和底部分页。页面默认显式查询 open 状态，用户可切换 open/closed/merged/all；请求序号保证旧项目、旧筛选或旧页码的延迟响应不会覆盖当前视图。

### Chat

聊天历史以 append-only JSONL 保存到 `im/chat_history.jsonl`。启动时只读最近一段，向上滚动时分页读取旧数据。渲染和滚动锚点计算分离，以减少滚动/缩放期间的工作量。

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

### 项目保存与 Provider 查询

1. Project 页面把本地仓库和远程仓库字段提交给 Rust callback。
2. `src/project/mod.rs` 校验本地字段，并为每次项目保存启动一个共享 runtime 查询任务。
3. `src/codereview/api/` 解析 `remote_repo`，并通过 `src/http/mod.rs` 调用对应的 Gitee、GitLab 或 GitHub 详情接口。
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

- `src/git/{github,gitlab,gitee}.rs` 尚未形成文件名所暗示的 Provider 抽象。
- `src/logger/debug_logger.rs` 有一个无法编译的 doctest。
- `Route` 中部分页面是开发/演示界面，没有进入主导航。
- 根项目许可证仍需明确。
- `docs/terminal_viewport_iteration_notes.md` 是历史证据，不是当前规格。
