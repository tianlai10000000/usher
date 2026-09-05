# Usher 架构文档

[English](architecture.md) · [README](../README.zh.md) · [规格索引](specs/README.md)

## 文档目标和范围

本文描述当前代码的真实结构、模块边界、运行生命周期和安全扩展点，是中文架构主文档。需要明确行为不变量的内容统一放在 `docs/specs/`。

Usher 是桌面优先的 Rust 应用。Slint 负责展示和声明式 UI 状态；Rust 负责持久化、进程执行、网络请求和应用装配。

生产构建在编译期固定选择 Winit + FemtoVG/OpenGL。Skia 和 FemtoVG/WGPU 作为显式启用的 Cargo feature，仅用于渲染器诊断和 A/B 对比，避免常规构建因同时链接多个实现而静默切换渲染器。Slint 应用内实时预览和 UI Components 演练场分别由显式的 `live-preview`、`ui-components` 开发 feature 控制，默认构建和生产构建都不会编译它们。当前 macOS 测试中 WGPU 虽降低 physical footprint，但会增加 RSS，因此不作为生产默认配置。

PR Review 通过稳定的 Slint `@pr-review` feature package 入口暴露，与打开它的
路由解耦。默认和 Release 构建把该入口映射到完整页面；显式、非默认的
`dev-shell` feature 会把整个包映射到协议兼容的空实现，并把 `PrReview` 导航
回退到 Workflow，使无关 Shell 修改不编译大型审核渲染树。这不是跨 crate 拆页；
完整页面的状态、浮层和交互仍作为一个整体保留。

在 macOS 上，进入或退出全屏后会在 AppKit 最终标题栏布局完成时重新对齐原生控件，并把原生全屏状态发布给 Slint，使面包屑释放已经移除的左侧 Chrome 占位；交通灯簇整体右移 8pt 以对齐侧栏图标中线，存在这些按钮时应用自有控件始终位于其右侧。

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
| `crates/usher-i18n/` | 与框架无关的 locale 注册表和编译后消息 catalog。构建阶段校验根 `i18n/` manifest/PO 资产、生成强类型 Slint `I18n` global，并为每个独立编译的 UI 根准备索引完全一致的翻译 bundle；运行时 locale 解析与 Rust 文案查询不依赖具体 UI 引擎。 |
| `crates/usher-messages/` | 独立编译的消息中心包：持久领域/存储服务、快照订阅、后台持久化/原生投递 Worker、`notify-rust` OS 适配器及生成的 Inbox 页面 API。可嵌入 Slint 页面使用 library-module 模式编译，不再依赖已废弃的非 Window 根组件兼容路径。 |
| `crates/usher-search/` | 与框架无关的通用 Find 引擎：Unicode 安全的非重叠文本命中、首尾循环的相邻结果导航，以及 Chromium/Blink `CenterIfNeeded` 视口策略。Feature 只提供领域/model 与视觉换行适配。 |
| `crates/usher-state-store/` | 与框架无关的跨页面 State 接口与内置引擎：强类型 reducer/action、不可变 revision 快照、selector、立即 hydration、自动单调订阅、RAII 取消，以及支持延迟启用、合并、flush 和 shutdown 的 Worker `PersistentStore` 门面。Feature 门面隐藏具体引擎，未来可通过 adapter 替换社区 Store。 |
| `crates/usher-virtual-list/` | 与框架无关的可变行高虚拟列表门面：精确 item extent、offset/index 映射、有界 overscan 窗口与精确 item reveal。私有 adapter 当前使用社区 `virtualizer` 引擎；Feature 与 Slint bridge 只依赖本地接口，底层可替换。 |
| `crates/usher-preview/` | 与框架无关的 Artifact 描述、Provider/Registry 契约、运行时策略、准备后 payload，以及构建期可拆卸的 Markdown/图片/纯文本 Provider；其纯结构 Markdown 入口保留 block 与行内语义，但不会初始化代码高亮资源；不依赖 Chat、MainWindow 或 Slint。 |
| `crates/usher-ai-review/` | 可选且与框架无关的 AI Review 领域包：规则/配置 reducer、动态有界调度器、Review 输入/Session 与 Prompt provenance、diff 分块、响应解析和 finding 聚合；不依赖 MainWindow 或 Slint。 |
| `src/ai/` | OpenAI-compatible transport、Feature 自有的强类型/持久化 AI Agent 配置 Store，以及位于 `prompts.toml` 的唯一受版本控制内置 Prompt catalog；`prompts.rs` 以 strict 模式渲染强类型、有界的 AI Chat、AI Review、Review Ask 消息 envelope 与纯元数据 provenance。 |
| `src/app_runtime.rs` | 进程级 Tokio runtime，生产默认使用 2 个 worker；runtime 线程安装 tracing dispatch，并保留显式环境覆盖用于受控基准。 |
| `src/memory_diagnostics.rs` | 统一采集进程 RSS/footprint 与生命周期高水位、启动/运行元数据、有界 Slint model 占用、UI 尺寸关联，以及可取消的周期内存采样。 |
| `src/diagnostics.rs` | 崩溃运行哨兵、保留策略 Worker、日志目录打开与隐私有界的诊断归档导出。 |
| `src/startup.rs` | 启动 elapsed/delta 时间轴，以及把磁盘 hydration 和高开销图片投影延后到首帧后的单次任务队列。 |
| `src/platform/` | 跨平台原生集成边界。`macos.rs` 负责 AppKit 外观、标题栏附件、窗口交互通知、原生标题栏点击桥接、PR Review 字体注册、`NSSound` 点击音和 Auto 主题实时跟随；同时成对管理并在正常关闭时注销 NSEvent monitor、窗口/主题 observer 与独立 Preview resize observer。`windows.rs` 负责 Windows 主题检测与 `MessageBeep`；`linux.rs` 负责 XDG Portal 主题检测和不阻塞 UI 的桌面音效播放。`mod.rs` 提供统一 API、Auto 主题监听开关（全平台 1 秒轮询兜底），以及用操作系统文件管理器打开目录（Finder / Explorer / `xdg-open`）。 |
| `src/shutdown.rs` | 按优先级协调关闭。正常关闭在 event loop 执行 UI/AppKit 清理；若 event-loop dispatch 失败，后台兜底只执行线程安全的 pre/post 清理并跳过 UI/AppKit。 |
| `src/single_instance.rs` | 进程锁与有界文件信号唤起通道；第二次启动时让已有窗口重新显示并获得焦点。 |
| `src/tray.rs` | 桌面托盘和托盘触发的生命周期操作。 |

`src/lib.rs` 是 composition root，不应继续堆积功能逻辑。原生窗口行为归 `src/platform/`，诊断统计归 `src/memory_diagnostics.rs`；新业务应放入对应模块，只向装配层暴露精简的 setup 或 service API。

### 功能模块

| 路径 | 职责 |
| --- | --- |
| `src/project/` | 稳定 façade/controller、强类型 State Store 门面，以及独立的持久化模型/存储、Logo、PR/MR overview 选择和 Workflow/Terminal 入口。 |
| `src/codereview/api/` | Gitee、GitLab、GitHub 仓库接口的 URL 与路径模板；不包含请求执行、鉴权或响应解析。 |
| `src/git/` | Provider-neutral façade，以及独立的领域模型、remote 定位、unified-diff 解析、PR 概览计数和 Provider adapter。Gitee 的传输、解码、Conversation/Commit 归一化、readiness 与 revert 语义位于专用子模块。 |
| `src/jobs/` | 应用级 Job 生命周期 model、来源无关运行描述符/UI 投影，以及按 source key 的动作分发；不拥有任何 Feature 进程。 |
| `src/workflow/` | 按项目隔离的 Branch、Tag、PR、CD 状态和命令执行；Tag 与 PR Review 通过稳定文件 façade 暴露内部 controller 和纯逻辑模块。 |
| `src/terminal/` | PTY 生命周期、viewport 同步、渲染、选择和项目切换；纯输入映射、诊断与刷新节流使用独立模块。 |
| `src/chat/` | AI/RAG 共用 Chat 展示和交互、按模式隔离的 Session/JSONL 历史、请求耗时管理、transport、宽度无关纯文本/Artifact 投影、有界渲染缓存、分页和纯有界虚拟布局；完整 Artifact 渲染归通用预览 Feature。 |
| `src/agent_response/` | Agent Markdown 的宽度无关投影；负责 16 KiB/48 block 行内预算、128 项/近似记账 2 MiB 的内容身份缓存，以及不透明仓库 Markdown action token；不执行文件 I/O，也不拥有 Preview Host。 |
| `src/ai_review/` | 可选 AI Review 配置门面、保留损坏 JSON 的统一存储边界和应用 controller；负责 State Store 订阅、自动持久化 Worker、动态任务调度、Review/Ask 耗时投影、按需加载 diff/规则、AI 调用、在途 Provider 取消、已完成 Review 与轻量 PR Chat Session 存储、通知和不依赖路由的 PR 结果投影。 |
| `src/artifact_preview/` | 应用预览门面：Provider 策略、后台准备、过期结果取消、Slint model 适配、单活动 Session、固定面板与独立窗口之间的即时迁移、向 Detached component tree 显式同步 effective theme，以及显式激活仓库 Markdown 时由 Worker 执行的 canonical containment 校验和读取。其 `test-fixtures` 适配器仅在开发构建存在，并让演练样例通过同一门面提交。 |
| `src/update/` | Stable/Alpha Release 选择、流式下载、进度、取消、安装和重启；重启时先释放进程单实例锁，再拉起替换进程。Stable 排除 SemVer 预发布版本，Alpha 同时接受稳定版与预发布版。 |
| `src/router/` | 路由映射、历史、可见性、普通 transition hook 与路由 scope 可达性 hook；不执行 feature 清理。 |
| `src/shortcuts/` | 应用命令 ID、声明式键位、平台归一化匹配、路由作用域、冲突校验、分发与展示标签。 |
| `src/app_lifecycle.rs` | composition 层生命周期注册，负责 Chat transition 可见性与 Workflow scope 清理。 |
| `src/settings.rs` | TOML 设置模型与串行原子持久化，包含 AI Agent 凭据和可配置 RAG endpoint。 |
| `src/ai/` | 共用 OpenAI-compatible AI Agent transport、Feature 自有的强类型/持久化配置 Store 与内置 Prompt catalog；`agent.rs` 负责连通性测试及 AI Chat/AI Code Review 使用的 chat completion，不负责 RAG transport。 |
| `src/network.rs` | 网络可达性与 `AppState.network-online`：macOS 使用 SystemConfiguration 回调，其它平台使用不发包的路由检查与离线退避。 |
| `src/http/mod.rs` | 唯一共享 reqwest client、单请求总超时/响应体空闲超时策略、统一响应体读取、拦截器、统一错误和可选 UI 反馈。 |
| `src/logger/` | Debug 日志、tracing 和 panic/error 支持；panic hook 自有直接 JSON 兜底写入，即使普通 tracing 已关闭仍保持生效。 |
| `src/toast.rs` | 单条短时 Toast 管理器绑定。 |
| `src/notification.rs` | 通知队列、计时器、Hover 暂停、操作按钮和图标。 |
| `src/message_center.rs` | 主窗口与 `usher-messages` 之间的薄桥接层；同步主题/视口、下发 All/Unread/Read 计数，并把不可变快照转换为独立页面 Slint model。 |
| `src/dialog.rs` | 命令式 `DialogManager` 绑定与操作分发；不再拥有独立视觉变体。 |

仓库接口地址与可执行行为必须保持分离：`src/git/` 可以引用 `src/codereview/api/` 中的常量，但 `src/codereview/api/` 不得依赖 `src/git/`、`src/http/`、reqwest 或 serde。

### Slint UI

| 路径 | 职责 |
| --- | --- |
| `crates/usher-ui/build.rs` | 编译根 Slint 源码树，将内置 Material 映射为 `@material`，并把 `@ui-components` 映射到真实开发演练场或契约兼容的关闭 stub。 |
| `crates/usher-ui/src/lib.rs` | 包含并公开导出 Slint 生成模块。 |
| `ui/main.slint` | 根窗口、路由装配、Rust-facing property/callback、Modal 层和 Terminal Dock。 |
| `ui/route/routeMap.slint` | 完整 `Route` 枚举；主导航只开放其中一部分。 |
| `ui/state/` | Slint 全局状态适配。 |
| `ui/views/` | 只负责路由级页面编排；活跃入口统一使用 `GitHub*Page` 命名，并从 `ui/components/` 引入全部视觉积木，view 文件不再声明局部控件。 |
| `ui/views/job_run.slint` | 应用级 `Route.JobRun` 装配，可从任意来源路由复用。 |
| `ui/views/ui_components.slint` | 交互式 UI Components 演练场的纯路由编排。 |
| `ui/views/workflow/` | Workflow 路由编排和完整的全窗口 PR Review 实现；Flow、Tab、Filter、Field、Panel 组件归 `ui/components/workflow/`。 |
| `ui/features/pr_review/` | 稳定的 PR Review feature package 入口及公开页面协议；构建配置把 `@pr-review` 映射到完整实现或显式 `dev-shell` 替代实现。路由只是使用方而非所有者，后续入口复用同一个包。 |
| `ui/features/ai_review/`、`ui/features/ai_review_settings/` | 可选编译包，提供完整实现和协议兼容 disabled 实现；完整 Review 包只暴露 PR 右侧面板，不拥有独立路由。 |
| `ui/components/` | 所有应用视觉组件的统一归属目录，包括 Control、Row、Card、Menu、Tab、Panel、Dialog、Field、Feedback 和功能专用视觉积木；GitHub/Primer 实现统一使用 `GitHub*` 符号。 |
| `ui/components/artifact_preview/` | 应用级通用预览外壳、基于 Primer token 的原生 Markdown/文本/图片 Surface、右侧固定面板与独立原生窗口；不承担解析或 I/O，也不引入 WebView。 |
| `ui/components/agent_response/` | Agent 回复的紧凑原生 block renderer；只消费准备好的结构并上报 link/action callback，不承担 Markdown 解析、仓库路径解析或文件 I/O。 |
| `ui/components/about/`、`ai_review/`、`chat/`、`project/`、`workflow/` | 从路由页面抽出的功能级 GitHub/Primer 视觉积木，不负责 I/O 或路由跳转。 |
| `ui/components/showcase/` | GitHub/Primer 属性对比矩阵，以及把依赖业务 model 的组合组件链接到真实路由的登记表；可选 Preview 样例只发出 fixture id，不得绕过 Provider 或 Host；新增或实质变更的视觉组件必须同步登记。 |
| `ui/components/button/github_icon_button.slint` | 图标按钮、浮动按钮、Terminal、项目卡片和复制操作共用的 `GitHubIconButton` 基础，统一键盘焦点、Tooltip、加载态和语义变体。 |
| `ui/components/label/github_counter_label.slint` | 共用的强类型 Primer CounterLabel，提供 Default/Primary/Secondary Scheme、可选上限、紧凑叠加几何与零值隐藏；状态栏通知徽标及后续导航/操作计数统一复用。 |
| `ui/components/github_link.slint` | 共用的 Primer 文本链接状态和键盘交互。 |
| `ui/components/github_spinner.slint` | 控件和加载界面统一使用的应用自有 Primer 不定进度 Spinner。 |
| `ui/components/github_progress_bar.slint` | 状态栏和更新界面共用的语义化确定/不确定进度条。 |
| `ui/components/github_scroll_view.slint` | 应用自有的常驻纵向视口和语义滚动条基础。 |
| `ui/components/navigation/github_underline_nav.slint` | Workflow 及其功能子页签共用的、支持键盘的 Primer UnderlineNav/UnderlinePanels 容器与 Item；容器统一管理全宽底线、安全区内边距和页签间距。 |
| `ui/components/github_segmented_control.slint` | 共用的紧凑 Primer SegmentedControl 外壳、Item、Divider 及焦点/禁用交互。 |
| `ui/components/github_tooltip.slint` | 共用的 GitHub TooltipV2 契约：调用方挂载透明锚点定位器，由 `ui/state/tooltip_overlay_state.slint` 与根层 `GitHubTooltipOverlayHost` 统一执行 offset/flip/shift/size，在窗口根绘制并避开滚动/裁剪祖先与窗口边界。 |
| `ui/state/tooltip_overlay_state.slint` | 窗口根 Tooltip 门户状态（文案、锚点矩形、首选方向/对齐、claim token、lease 心跳）。 |
| `ui/components/github_action_menu.slint` | 点击浮层共用的 Primer ActionMenu/AnchoredOverlay 表面，统一焦点归属、Esc 关闭、打开时 peer exclusive-claim、视口碰撞定位、限高纵向滚动、ActionList 行，以及 `GitHubActionMenuTriggerArea` 的 press 态开/关（避免标题栏 Chrome dismiss 与 toggle 同一次点击先关后开）。页面空白点击关闭仍由各宿主在菜单之下自行挂载 TouchArea。`ui/state/transient_overlay_state.slint` 提供 `dismiss-revision` 与 `exclusive-generation`。 |
| `ui/components/page/github_page_header.slint` | 标题、说明、前导视觉、分隔线和尾部操作共用的 Primer PageHeader 基础。 |
| `ui/components/button/github_copy_button.slint` | 所有图标复制操作共用的 GitHub CopyToClipboardButton 交互；组件负责短时 Copy→Check 状态和 `Copied!` Tooltip，feature callback 返回剪贴板写入是否成功。 |
| `ui/components/jobs/` | 可复用的 GitHub Actions 风格 Run/Job/Step/日志展示；只接收归一化快照，不执行流程命令。 |
| `ui/components/dialog/github_dialog.slint` | 全局、项目、PR Review 和更新弹窗共用的响应式 Primer Dialog 外壳，统一遮罩输入归属、初始关闭按钮焦点、Header/Body/Footer 安全边距、Esc 关闭（`modal` 时禁用）、窄屏全窗口布局、关闭行为与操作状态；可选的外部视口使路由内弹窗始终按完整应用窗口居中，并跨固定 Shell Chrome 绘制连续遮罩；根状态继续把遮罩延伸到 macOS 标题栏 inset，`src/platform/macos.rs` 在保留 AppKit 交通灯可用的同时屏蔽应用自有标题栏附件；原生标题栏点击复用普通遮罩关闭语义，不会削弱不可关闭 Dialog；功能正文以 `@children` 嵌套。 |
| `ui/components/toast/`、`ui/components/notify/` | 共用的紧凑 Primer Banner 风格瞬时/排队反馈；根层同一时刻只渲染最新的一条排队通知（最新在前、最多 3 条），用持久未读总数明示队列上限之外的通知，并把批量操作收入紧凑 ActionMenu。 |
| `ui/components/messages/` | 可复用 GitHub Inbox 消息行，统一已读/未读、语义级别、元数据和常驻行操作，并登记到 UI Components。 |
| `ui/components/pr_review/` | PR Review 的公开 UI struct、可复用外壳/文件树/Diff 控件；跨区域状态仍留在完整页面实现。 |
| `ui/theme/github_theme.slint` | 应用自有的 `GitHubTheme` 语义色，以及 `GitHubMetrics` Primer 几何、控件、字体、浮层与断点令牌；功能性 UI 原始颜色统一在此管理。 |
| `material-1.7.1-fixed/` | 与 Slint v1.17.1 对齐的 Material 源码。 |
| `vendor/i-slint-live-preview/` | 仅由显式 `live-preview` feature 选择的临时 Slint 1.17.1 override；其中有文档记录的 `mtime` 保护避免 macOS 文件监听误报引发无限重编译。 |

应用 UI 必须通过 `@material` 导入 Material，禁止直接引用 vendored 实现路径。
绝对禁止为应用行为或外观修改 `material-1.7.1-fixed/` 或其他 vendored Slint
Material UI 源码。Material palette、typography 和 style metrics 不定义产品
外观；应用界面统一消费 `GitHubTheme` 与 `GitHubMetrics`。所有视觉适配必须放在
项目自有代码中，确保依赖升级时可以直接替换 Material UI 目录而无需迁移补丁。
当前白名单有意保持最小：`ui/main.slint` 的 `MaterialWindowAdapter`，以及 PR
Review 工作区用于滚动的公开 `ListView`/`ScrollView`。Artifact Preview 使用应用
自有 `Flickable` 和共享精确 extent 虚拟化门面。
`scripts/check-ui-policy.sh` 自动检查该边界及 Octicon 命令图标策略。

所有产品页面和复用视觉组件均遵循
[GitHub/Primer 页面设计规范](specs/ui-design.md)。所有应用自有 GitHub/Primer
视觉组件都必须使用 `GitHub` 前缀。现有命名和视觉欠债按
[UI 分批改造清单](ui-github-migration-plan.md)逐批迁移，每批均需用户验收后才
能继续；发现的缺陷追加到 [`bug-history.md`](../bug-history.md)。GitHub 当前产品
界面以及 Primer 官方文档/源码是视觉事实来源。

根 Shell 持有两个常驻 Primer Surface：位于
`ui/components/navigation/custom_nav_drawer.slint` 的仅图标路由栏，以及
位于 `ui/components/status_bar.slint` 的运行状态工具栏。路由栏负责路由
选中态与 Tooltip 展示；状态工具栏负责更新、通知、网络连接及版本展示。
其透明宿主始终使用窗口坐标，只有可见表面与左侧临时状态区跟随实时内容边缘；右侧
常驻工具组直接锚定在根状态栏，不参与会重新分配宽度的 Layout。因此侧栏展开后其
正下方不显示状态栏表面，也不得移动或压缩快捷键、通知、网络或版本条目。通知数量
来自持久消息中心，仅在非零时显示为铃铛徽标；激活后打开 `Route.Messages`。
快捷键/网络健康色由对应 Feature 状态驱动，通知与版本不作为健康指标。右边缘
Tooltip 向左展开，状态浮层共享同一父坐标系。各页面不得重复实现这两类 Shell
Surface。详见 [specs/status-bar.md](specs/status-bar.md)。

导航侧栏及其内容边界装饰作为根 Shell 层挂载在包括 PR Review 在内的普通路由
表面之上，同时仍低于 Modal 遮罩。固定侧栏使用清晰可见且贯穿完整高度的 1px
边界与克制的内向 Elevation 阴影；临时 Hover 侧栏改为不透明白色、只有边框、尾部上下
10px 圆角，使用 Codex 浅色 `#dfdfe0` 中性边框且没有右边界阴影的浮层。同一个可中断
弹簧移动侧栏、路由表面、标题栏分割线和状态栏绘制内缩；路由在切换开始时只采用一次目标
宽度，动画不会逐帧重排页面树。原生 Live Resize 时冻结已挂载路由宽度，拖拽结束一次
应用精确最终宽度；根底面和原生不透明底面仍跟随窗口。PR Review 同时保持稳定的内部
虚拟化 Diff 几何。

应用级右侧 Artifact 预览是根 Shell 面板，从窗口顶边开始，并通过连续 1px 分隔线与
路由内容分开。开启和关闭使用同一个 260ms progress 同时控制面板裁剪宽度和路由预留
宽度；完整面板始终靠右，Preview 与 Chat 文档布局在边界移动期间保持稳定，不逐帧重排。
Feature 门面立即释放 prepared session，只读 Slint 视觉投影最多保留 280ms 绘制退出
动画，随后清空。FemtoVG 会在祖先 clip 移动时重放后代绘制，因此固定 Host 在 260ms
Shell 过渡期间只暂停文档绘制，同时保留准备后的 model；轻量 spinner 与解析器加载
文案在该过渡期间仍保持可见。稳定后 Markdown Surface 使用
`usher-virtual-list` 精确 extent 门面，只实例化有界的可见/overscan block，在不改变
Provider/Session 所有权的前提下限制正文恢复的一次性成本。清除视觉投影时还会重置
Feature 自有的虚拟 rows、extent、projection guard 以及 Parley 字体/layout 缓存，同时
保留根 callback 使用的稳定 Slint model 身份。预览模块自有的 resize-handle 组件负责稳定命中区、光标、
hover 反馈和 pointer cancel 生命周期；根 Shell 持有有界的会话宽度，并对预览与路由面板
对称应用。

Preview resize 的几何契约与 Provider 渲染分离。拖动固定面板分割线时，根 Shell
只更新可见面板边界并裁剪 Preview Surface，共享 Preview Panel 则把准备后的文档树
保持在拖动开始时的布局宽度。macOS 独立窗口 Live Resize 由按窗口对象过滤的 AppKit
observer 提供同样的 Host 局部状态；有界的窗口局部 debounce 覆盖只发布宽度
变化而不发布原生 begin/end 通知的 backend。两条路径都绝不复用主窗口 resize 标志。
macOS 独立 Host 保留 AppKit 文档标题，并将 capability 驱动的复制、外部打开和关闭
控件挂载到一个右侧原生标题栏 accessory；共享 Slint Panel 关闭原本会重复出现的内容
Header。该 accessory 每个原生预览窗口只安装一次，并且仅在 prepared payload 的
capability 变化时重新紧凑排列，不进入文档滚动、动画或 Live Resize 渲染路径。其他
平台继续使用共享内容 Header 及其控件。
两种手势松开后均只提交一次精确最终内容宽度；Markdown、图片和文本 Provider 不感知
Host 形态与 resize 生命周期。
原生 Markdown Surface 使用独立 `MarkdownParserLoader` 与语言解析器 Registry。
生产默认采用 `OnDemand`，`PreloadAll` 必须显式选择；Rust、Shell、JavaScript/JSX、
TypeScript/TSX、JSON、Python 与 Go 分别使用仅在选中对应语言族时初始化并缓存的
Tree-sitter 配置。未支持或未声明的围栏语言统一复用一个保留完整源码的纯文本 parser；
Preview 路径不再包含完整语法 Repository fallback。Loader 初始化与高亮都留在 Preview Worker；用于行内正文的
Slint `StyledText`、直接绘制彩色代码 token 的原生 Slint `Text` 行，以及官方 Primer
视觉 token，并复用紧凑 AI Review 回复的 Codex 文档节奏：14px 正文/22px 行盒、
24/20/17/15px 标题、20px 段落间距、10px 正文/列表组间距，以及连续列表项零额外间距。
H1-H6 不自动添加分隔线；引用缩进和低对比代码 Surface 共用一套 Slint typography token。
对于正文，Parley bridge 同步稳定宽度、字体、字号、行内样式、固定正文节奏及字形安全
余量；混合字体行不得裁切，也不得偏离虚拟 extent。围栏代码块保留语言并携带准备好的亮色/暗色 token 投影，不再经过
Markdown/HTML 二次转换；主题切换不重复解析或高亮；不引入 WebView 或浏览器 runtime。
冷解析器 cache miss 发布 `ParserLoading` 与 `ParserReady` 进度。应用门面通过 weak
window 在 Slint event loop 上更新共享 spinner 文案，并在投递前后都检查 request
generation，取消或过期任务不能覆盖新 Preview。Feature 自有 Parley 布局桥在稳定内容
宽度下测量全部 block，并把同一份精确 extent model 提供给 `usher-virtual-list`、Preview
`Flickable` 与应用自有 Primer scrollbar；普通滚动只更新有界的可见 block 投影。因此
viewport 位于精确的 3-block overscan 安全区内时复用同一个稳定 `VecModel`；跨出安全区
只增量删除/追加边界行，不替换 model，也不提高有界挂载行峰值。Slint 仅在投影 revision
变化时替换虚拟窗口值。因此滑块高度全程不变，滑块归一化行程与文档归一化 offset
始终一致，段落边界也不会触发每个滚轮事件的 delegate 重建。
Preview 内容宽度冻结期间隐藏滚动条，
稳定后在最终右边缘恢复；延伸到 macOS 全尺寸标题栏下方的根滚动区把可拖动 scrollbar
track 放在原生标题栏 inset 下方，避免 AppKit 抢占拖动并移动窗口。

同一 Markdown parser core 另行提供用于紧凑 Agent 回复的纯结构入口。该入口在 Worker
执行，不构造也不查询 `CodeParserRegistry`；围栏 block 保留语言和源码，但高亮
`code_spans` 始终为空。`src/agent_response/` 使用 Codex 桌面端的系统字体对话排版：
14 px 正文/22 px 行盒、24/20/17/15 px 标题、精确的段落/列表组节奏，以及独立的
24 px 悬挂列表 marker gutter。代码行使用平台注册的等宽字体，并由 Worker 投影行数
来稳定最小高度。非围栏 block 在组装后清理 Markdown 标点前残留的一层模型转义；围栏源码和链接
target 不变。随后在 event-loop adapter 创建 Slint model 之前应用与宽度无关的 16 KiB/48 block 投影预算，以及最多 128 项、投影 payload
近似记账 2 MiB 的内容身份缓存（不含容器与 allocator 开销）；同一 Worker 还准备 Slint `StyledText`，event loop 只实例化并提交
`ModelRc`。最近 32 条 Assistant 行保留完整结构，此前 96 条使用一个去标记纯文本投影
并保留 Preview action，更早的行保持 raw；这些限制约束结构化投影/缓存工作，不限制持久化
历史或 Slint 总行数。Assistant 行消费结构 block；User 行和同一二进制的基准模式继续使用
原始文本路径。仓库 `.md` 或 `.markdown` 引用绝不会作为可信 action 目标交给 Slint：
投影层把 link destination 替换为不透明 action token，并在 Rust 中保留 token 到相对
路径的映射。

AI Review 回复触发的 Preview 绑定到该 Review task 的仓库上下文。激活不透明仓库
action 后，由 Worker 解析并读取文件：Unix 从一次打开的 canonical 根目录 descriptor
逐 component 遍历并拒绝 link；Windows 比较已打开根目录和目标句柄的最终路径。验证后的
普通文件句柄执行大小限制并只读取一次，之后才提交给共用 Preview Provider 管线。
Docked 模式把唯一的应用 Preview host 移入 PR Shell 既有的右侧 trailing slot：Preview
Surface 临时替换 AI Review Surface，既不重复预留宽度，也不清空 conversation；关闭
Preview 后恢复完整对话。Detached 模式保持独立原生窗口，不卸载 AI Review 面板。
PR Dialog 始终绘制在两种停靠 Surface 之上。

关闭 AI Review 面板或离开 `Route.PrReview` 时，清空行内回复投影、不透明 action 映射
和 task 仓库绑定；仅当当前 PR 上下文 Preview 处于 Docked 模式时，同时关闭该 Preview。
已经 Detached 的 Preview 作为独立应用窗口继续存在。只关闭 Docked Preview 不触发
AI Review 清理。关闭 AI 面板或离开 PR 路由会取消当前 Ask，避免迟到结果进入重开的
mount；独立调度的 Review task 继续运行。

`src/platform/` 在启动时按平台选择一套清晰且支持中日韩文字的应用字体：
macOS 使用原生 `.AppleSystemUIFont`，Windows 使用 `Microsoft YaHei UI`，其他
平台使用 `Noto Sans`。`ui/main.slint` 将该选择与 Codex 桌面端的默认字重 445
作为完整普通 UI 树的窗口默认值，页面、共享组件和 Slint `StyledText` 统一继承，
不再自行选择其他字体。Terminal 内容完全
保持独立，避免改变既有单元格和工具栏度量；PR Review 中仅 Diff 源码与
文件树内容保留几何所需的平台专用字体，其余控件和正文继承应用字体。

UI 源码继续保留在仓库根目录的 `ui/` 下。主窗口与普通路由树由
`usher-ui` workspace crate 生成；Settings 与 Messages 路由分别由
`usher-ui-settings` 和 `usher-messages` 以 library-module 模式独立生成，再通过
Slint 实验性的 `ComponentContainer`/component-factory 边界挂载。这会正式
生成可嵌入的非 Window 组件，不再依赖 Slint 已废弃的应用根兼容路径。应用 crate 继续重新导出主 UI
API，并由 `src/settings_page.rs` 与 `src/message_center.rs` 显式桥接。
跨 crate factory 创建的组件拥有独立 Slint global 图，因此桥接层必须复制初始
`AppState`、把 callback 转发给 `MainWindow`，并同步后续状态变化，不能假定两边
global 自动共享。Messages Inbox 行只把动作枚举与锚点矩形发布到专用的 factory 本地
`MessageTooltipOverlayState`，由页面唯一的 `GitHubMessageTooltipOverlayHost` 把枚举
映射到相互独立的字面量 surface，任何标签字符串都不穿过列表 delegate。Settings 为
共享控件挂载本地通用 `GitHubTooltipOverlayHost`，并在每个新 claim token 到来时重建
文本测量 surface。Settings 与 Messages 都在首次访问后惰性创建一个稳定的 factory
产品。Slint 1.17.1 销毁并重挂载跨 crate 产品时可能复用错误的控件文案或 placeholder，
因此离开路由只隐藏这些 host；各 Feature bridge 分别负责状态重置生命周期。

在单次 Settings factory 产品内部，Slint 1.17.1 的选择器内容必须使用固定挂载的
叶子节点，并通过 `visible` 与显式活动高度选择当前内容。禁止用 `if` 节点重建互斥的
section 或 provider 表单，否则重复 Tab 切换可能保留兄弟节点的标签、
placeholder、值或选中态。路由级选择索引必须从哨兵值开始，并在下一次 UI tick 切到
默认值，以便新建视觉树主动失效旧绑定。选择器文字、选中样式和显示字段必须来自同一
状态源。该规则覆盖 Settings section 菜单、Git hosting provider 和 AI Agent provider
预设。
compact section 菜单为每个 section 使用独立的固定触发器叶子，并在每次展开时快照
当前 section，因此折叠标签与选中行不会在 factory 复用后保留上一条路由的 section。
界面语言与 AI 回复语言选择器同样遵守固定叶子规则，但不在应用 UI 中硬编码语言
列表。`usher-ui-settings` 构建脚本读取经过校验的 `usher-i18n` locale manifest，
为每个语言生成固定的触发器和选项叶子。因此新增语言仍然只需维护 manifest 与翻译
catalog，同时运行时 repeater delegate 不会在 factory 产品之间残留其他语言的标签、
id 或选中状态。每次展开菜单时，picker 都会把当前规范化选择快照到自身局部状态；
选项样式读取该快照，而不直接依赖真实渲染器在选项 surface 隐藏期间可能未失效的
global binding。

Settings 打开期间，凭据、Endpoint 与 AI Agent 文本字段都采用页面局部编辑所有权；
代码托管分组只有点击各自可见的 Save 命令才提交完整草稿。AI Agent 另用一份已提交的
强类型 Store 快照供 Settings 之外的 Chat 与 Review 消费。选择预设只在独立页面草稿中
原子更新 Provider、Base URL 与 Model，其他编辑同样不修改 `MainWindow` 或 Store。
Save 经 `src/ai/settings_state.rs` 把完整草稿 dispatch 为一个强类型 Action，随后 bridge
把规范化后的已提交快照投影到页面与 `MainWindow`。有效 commit 自动进入 Store 自有的
持久化 worker，并通过既有原子 TOML 边界写入 `[ai.agent]`；显式 flush 成功后才显示
保存反馈。每次进入 Settings 路由时，bridge 都在稳定 factory host 绘制前，用最近提交的
宿主/Store 快照覆盖这些页面局部草稿，不能再分别读取多个选择器字段。因此未点击 Save
就离开再返回时，编辑会被丢弃，但视觉树不会重挂载。

Messages 同样在首次访问后保留稳定 factory 根与视觉树。退出时 bridge 清空行 model，重新进入时先从最新
Store 快照 hydration 再激活，因此非活动 Inbox 不保留投影行。逐次重建视觉树的
Release A/B 没有测得实质 RSS 节省，且 physical footprint 出现混合回退，因此该
候选被拒绝。两个 factory
页面都复用与 `MainWindow` 相同的锚点式 offset/flip/shift/size 实现，但不尝试跨
factory 边界共享 global 实例。修改 `settings.slint` 或
`messages.slint` 不得重新生成或编译
`usher-ui`；修改主壳仍会重编主生成 crate。启用 `SLINT_LIVE_PREVIEW` 时，两套独立
build script 都会刻意保持 factory 产品为静态生成，因为 Slint 1.17 的
`ComponentFactory` 不能接收 live
preview 动态 handle；主窗口与 UI Components 路由仍可正常热更新。

独立挂载的路由只能拥有一个 viewport 和一条滚动链：它必须作为主壳 `Flickable`
的同级节点挂载，接收宿主真实宽高，显式绘制 `GitHubTheme.canvas-default`，把内容
宽度限制在可视区内，并避免主壳 padding 与页面 padding 重叠。主壳专用的预留几何
也必须从独立路由视口中扣除或补回，包括普通页面统一保留的 4px page-stack 底部
间隔。该约束用于避免嵌套滚轮抢占、底部内容裁剪、伪横向留白，以及透明页面透出
不同主壳底色。

### 独立页面状态桥接

所有跨 crate `ComponentFactory` 页面共用同一套桥接契约：宿主或 domain 拥有真相，
页面只展示镜像状态；桥接通过 `page.set_*` 或快照 apply 显式下推。Slint `changed`
可以保留为次级 host→page 镜像，但完成态与领域更新不得只依赖它——`changed` 是边沿
触发，会漏掉 `false→false` 这类“值未变化”的赋值。

两种同步模式共享该契约：

| 同步模式 | 适用场景 | 参考实现 |
| --- | --- | --- |
| Snapshot push | 持久列表/计数/领域数据 | Messages：`MessageCenter` → `src/message_center.rs` 的 `apply_page_snapshot` |
| Operation dual-write | 异步命令 busy 或短时反馈（`checking`、`testing`、保存闪烁） | Settings：更新检查 / AI Agent 测试，经 `SettingsPageBridge` |

通用规则：

1. 持久领域数据归 Rust feature 或 domain crate；页面只展示镜像快照或复制后的
   `AppState`。
2. 异步操作 busy 标志归宿主 feature 模块，且只在 Slint event loop 上改写。独立页
   点击处理只转发 command callback，不得自行赋值这些 busy 标志。
3. 纯本地 UI（选中分区、筛选 chips、reveal 开关、确认框）归页面所有，不同步。
4. 操作 begin/end 必须在 event loop 上双写宿主 property/global 与
   `*PageBridge::set_*(value)`，覆盖成功/失败/取消全部路径，并在 Notify/Toast
   之前或同一轮回写完成。
5. factory 创建时同步当前宿主 busy 与领域快照；拥有草稿的稳定 factory 页面还必须在
   每次重新进入路由前恢复已提交快照。
6. Worker 回写页面必须通过 `slint::invoke_from_event_loop`。

后续独立页复用同一清单，按状态种类选择 snapshot、operation 或两者并用；不得假定
生成 crate 之间共享 Slint global。

### 跨页面自动 State Store

凡是由多个路由、页面、状态展示或后台发布方消费的 State，都必须放在
`usher-state-store::StateStore` 接口与稳定 Feature 门面之后。强类型 Action 是唯一
变更入口；有效 reduction 安装带单调 revision 的不可变快照并自动通知订阅者，新
订阅者立即从当前快照 hydration。订阅者 revision guard 防止延迟回调覆盖更新的 UI
状态。Slint bridge 仍只是 event-loop adapter，不能成为 Store 引擎。

需要持久化的 Store owner 使用通用 `PersistentStore` 门面：每个有效 revision 排队到
单一 Worker，相邻待处理快照合并为最新 State，flush/shutdown 保持应用关闭边界；
reducer 与 Slint event loop 不执行文件 I/O。deferred store 在强类型启动 hydration 后
才启用持久化，使首帧后加载不会冗余回写已读取快照。包括内置 `LocalStateStore` 在内
的具体引擎都隐藏在 trait 之后。未来社区 Redux/Pinia 类库只能以 adapter 方式接入，
并保持 Action、Feature 门面、Schema、revision/hydration 契约和测试不变。详见
[specs/state-store.md](specs/state-store.md)。

## 构建产物策略

workspace 根 `Cargo.toml` 中的 profile 统一负责构建产物策略。日常 `dev` 与
`test` 使用行表级调试信息、`split-debuginfo = "off"`，同时保留增量编译。
这会覆盖 macOS 上 Cargo 默认的 full/unpacked 调试策略：panic backtrace
仍有文件名和行号，但 `target/debug/deps` 不再积累松散 `.rcgu.o`。

需要 LLDB 查看完整变量和类型时，显式使用 `dev-full` profile。它继承日常开发
设置，但把完整 packed 调试信息写入独立 profile 目录；因此可以通过
`cargo clean --profile dev-full` 单独移除，而不丢弃日常开发产物。Release 和
dist profile 保持独立：Release 继续禁用调试信息和增量编译。隔离的
`release-fast` profile 只用于日常优化构建；它继承 Release 运行时策略，但使用
Thin LTO、8 个 codegen unit 和增量编译。发布自动化继续使用未改变的 `release`
profile。

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
2. 创建 `MainWindow`，初始化空的 Messages/Task 门面及 debug/tracing logger。`[Startup][Timeline]` 记录累计 elapsed 与相邻 checkpoint delta 毫秒；logger 就绪前的点位先缓存在内存。
3. 应用设置并配置 updater 状态。
4. 绑定主题、路由、导航、应用快捷键和窗口 callback。
5. 安装持久消息中心门面与独立页面桥接，但暂不读取 JSON；随后初始化 Notification 和 Toast manager，排队 Banner 会镜像到消息中心。
6. 将统一 HTTP 模块绑定到 UI 错误反馈，然后以乐观初值启动网络监测，首次探测在 Worker 上执行。
7. 使用空启动 model 初始化 Dialog、Project、Chat、Terminal、Workflow 的 callback。
8. 在 macOS 上，若原生窗口句柄已可用，则在 `show()` 前安装标题栏附件与 Menu Vibrancy；否则在 `show()` 后重试。winit 窗口在事件循环启动后才创建，因此由首帧渲染钩子在与首帧同一事务内隐藏窗口并安装原生 Chrome；随后短轮询持续重试安装，确认就绪后再等若干 settle tick，让 Effect 第一次（未模糊的）Behind-Window 采样提交、并把 AppKit 挂载附件后的标题栏布局重置重新校正，最后同步校正交通灯位置才显示窗口；兜底时限保证窗口一定出现。因此首个可见帧就是最终 Shell，而不是分阶段过渡。
9. 首帧渲染完成后，在下一次 event-loop turn 统一调度：由 blocking worker 读取 Project JSON、投影并解码 Logo、hydration 持久消息与 AI Review Task State。强类型 Hydrate Action 会合并启动期间新增的数据，随后才恢复自动持久化。
10. 安装托盘并进入 Slint event loop。第二个进程只向唤起通道写入 nonce；主进程在 event loop 重新显示已有窗口并置前。
11. 关闭时通过优先级清理 Terminal、Updater、Tray、Runtime 和子进程。创建主窗口、`show()`、原生输入桥和 event loop 失败时显示平台原生启动告警并以非零状态退出，不再 panic。

如果模块依赖生成的 Slint global 或前一个模块安装的 callback，不要随意提前初始化。

原生侧栏 Ready 状态会显式同步给 UI。在 AppKit 完成 Vibrancy 挂载前，窗口根层和
固定侧栏都绘制主题对应的不透明 fallback。挂载后，Effect 与原生 Canvas 会在样式
改变后及原生 resize 时按全尺寸父视图 bounds 同步几何，确保 Menu 材质覆盖标题栏带，
而不是沿用启用 FullSizeContentView 前较短的 Render View frame。
Slint 窗口启动时不显示标准标题。macOS 等待原生标题栏附件时不挂载替代的 Slint
控件；该区域只显示最终 AppKit 侧栏与历史控件。

Debug 构建默认启用进程内存诊断；持久化的 Settings 控制项可在运行时关闭，Release 基准仍可通过 `USHER_MEMORY_DIAGNOSTICS=1` 强制启用。`[Memory][Diag]` 记录当前/峰值 RSS 与物理 footprint、相对上次/启动基线的增量、进程生命周期高水位、运行/场景标识，以及最近页面、逻辑/物理窗口尺寸、缩放和 Terminal 可见性；同时保留 VM 分类、macOS 全部 malloc zone 汇总的在用/保留/slack 容量、region、Mach port、线程、fault 与 page-in。`[Memory][UI Models]` 区分 AI Review 源 rows/blocks 与 retained/virtual delegate 创建计数，区分已挂载内容与实际绘制内容，并关联 PR 上下文 Preview host；`[Memory][UI]` 关联稳定后的页面/窗口变化，`[Workflow][PR Review][Memory]` 等功能归属日志只记录有界 model/cache 的数量和容量，不得输出源码内容或凭据。采样任务复用共享 runtime，并由正常的 pre-shutdown 流程停止。`scripts/benchmark-memory.sh` 负责 macOS Release 测量协议并汇总外部 RSS/footprint 采样。仅在显式设置 `USHER_MEMORY_WINDOW_CONTROL=signal-file-v1` 时，基准专用信号 Worker 才读取每个 run 独立的命令文件，并把 AppKit resize/fullscreen 请求投递到 Slint event loop；pre-shutdown 会关闭其信号 handle。外部 Swift helper 只读取 CoreGraphics 窗口状态，用于确认每次切换并独立记录实际全屏尺寸。基准元数据记录应用变体与二进制哈希；比较器只允许渲染器、应用所有权或 Tokio worker 中的一个维度变化，其他 fixture/协议不匹配一律拒绝。`.github/workflows/nightly-performance.yml` 每日使用 3 个隔离进程运行静态 Release Home 协议；`scripts/check-memory-benchmark-thresholds.py` 必须观察到预期路由，并在上传原始证据前同时检查 RSS/footprint 绝对峰值与周期中位增长预算。

## 并发模型

- Slint 对象和 global 归 event loop 所有。后台任务必须通过 `slint::invoke_from_event_loop` 更新 UI。
- 异步 IO 统一使用 `src/app_runtime.rs` 的共享 Tokio runtime；不要为单次操作新建 runtime 或 reqwest client。
- 现有阻塞 Git、PR 统计和文件操作在专用线程或 Tokio blocking task 上执行，禁止阻塞 Slint event loop。
- Rust 共享状态按所有权使用 `Arc<Mutex<_>>`、`Arc<RwLock<_>>`、Atomic 或线程本地缓存。
- 长任务必须具备取消或防重入机制。更新下载、历史分页、Workflow Run 和 Terminal Session 可作为参考。

## 状态和持久化

### 设置

`AppSettings` 以 TOML 保存到平台配置目录，包括主题、`auto` 或规范 locale id 的语言偏好、自动更新、相互独立的 Console/`usher.log`/`usher_tracing.log` 输出目标、Terminal verbose 与内存诊断、UI 声效开关、导航抽屉状态、Git 平台 Token、位于 `[git.platform.<provider>]` 的各平台企业版 API 地址和企业名称、位于 `[ai.agent]` 的 OpenAI-compatible AI Agent 连接配置，以及位于 `[ai.chat]` 的可选 HTTPS RAG endpoint（环境变量回退为 `USHER_RAG_API_URL`）。AI Agent 编辑由其 Feature Store 归约并自动持久化，其他设置继续使用既有的即时或显式保存入口。保存操作由写锁串行化，先写入并同步同目录唯一临时文件，再原子替换目标；解析失败时必须先把原文件保留为 `settings.toml.corrupt-*`，若保留失败，则本进程禁止继续写 Settings。Logger 输出目标通过运行时原子过滤控制；`usher.log` 在首次启用写入时按需打开，Settings 使用 GitHub 风格的 `Enable all` / `Disable all` 批量操作，同时保留每项独立状态。Gitee 的公有版 `token` 与 `enterprise_token` 分开保存；只有旧配置已经存在企业 API 地址时，旧版单一 Token 才迁移到企业字段，两套凭据不得跨 profile fallback。Slint 需要的数据同步到 `AppState`；API Key、企业名称和配置 URL 均属于本地配置，不得写入日志。Provider API 只有在企业 API 地址主机与项目 remote 主机匹配时才使用该配置；用户既可填写服务主机，也可填写完整 API 根地址，缺少版本路径时由 Provider 自动补齐。否则继续使用由 remote 推导出的公共平台默认地址。自动主题在 macOS 通过 AppKit、Windows 通过用户级应用主题设置、Linux 通过 XDG Desktop Portal 判断。选中 Auto 时会持续跟随系统外观变化：macOS 使用 `AppleInterfaceThemeChangedNotification` 并辅以 1 秒轮询兜底；Windows 与 Linux 每秒轮询同一检测路径。选择 Auto 会清除强制的 AppKit appearance，使进程跟随系统；显式浅色/深色则锁定 Aqua/DarkAqua。开启 UI 声效后，面包屑返回统一调用 `platform::play_ui_click`：macOS 播放 `Tink`，Windows 异步播放用户配置的默认系统音，Linux 在后台线程播放桌面 Sound Theme 事件，并以常见的 PipeWire/PulseAudio/ALSA 命令行播放器作为回退。

`[ai.agent]` 下的 `reply_language` 取值为 `auto` 或规范 locale id。`auto` 在组装每次请求时
解析当前 UI locale；该设置应用于 AI Chat、AI Review 与 Review Ask，绝不影响 RAG。
未知持久值归一化为 `auto`；独立 Settings factory 通过 event-loop bridge 同步选中 id 与
manifest 驱动的选项列表。

根目录 `i18n/` 是唯一翻译资产源：manifest 驱动 locale 发现、原生语言名称、别名、
回退和覆盖率门禁；语义消息 catalog 同时驱动生成的 Slint property 与 Rust 查询。
选择 bundled locale 后，应用会显式把全部 Slint translation 标记为 dirty，使独立编译的
MainWindow、Settings 与 Messages root 在同一个 event-loop turn 重新绘制既有 `@tr` binding。
`src/i18n.rs` 在 Slint event loop 上、创建独立 Factory 页面前应用语言。由于 Slint
每个运行时上下文只保存一个 bundle，MainWindow、Settings 和 Messages 必须编译同一
份完整生成 catalog，禁止各页面维护局部 `@tr()` 索引。Rust 动态状态在构建 UI 投影
之前始终保留语义 key、结构化类型或 `LocalizedText`；因此切换 locale 时可以重新投影
已经挂载的 Preview、Terminal、Project、Chat、Workflow 和 AI Review 状态，无需匹配
显示文案。Provider、PTY 与文件系统技术细节只进入日志，UI 只解析稳定的 catalog 错误。
仅由 Rust 使用的消息设置 `slint = false`：它们保留在语义与 PO catalog 中，但不会进入
或扰动 Slint 的实时 UI 翻译索引。
已持久化的应用自有 Chat 错误是兼容性例外：Renderer 会在所有已注册 locale catalog 中
精确反查语义 key，再按当前语言解析；不得用这套机制识别任意 Provider 内容。产品和
工程专业术语保留英文，命名占位符允许调序但禁止改名。详见
[specs/i18n.md](specs/i18n.md)。

手工保存凭据、代码托管 Endpoint 与 AI Agent 配置时，文件持久化必须在
Slint event loop 之外执行。完成结果回到 event loop 后，只有持久化成功
才更新临时 Saved 标签，并为最终结果显示一个底部 Toast。
所有延迟偏好保存共用 shared-runtime `spawn_blocking` 路径；100 ms Slint timer 只负责
合并/调度，绝不写 TOML。Provider 高频读取通过细粒度 `with_settings`/`with_git` 访问，
不再 clone 整棵 Settings；API base 解析事实按精确配置值进入有界进程缓存。

Settings 与 About 路由文件只负责组合；可复用导航、字段、Toggle、Section 和
详情行分别位于 `ui/components/settings/` 与 `ui/components/about/`，视觉组件
符号使用 `GitHub` 前缀。紧凑 Settings 导航与常规 NavList 暴露相同 Section
model 和键盘交互契约。

### 消息

`usher-messages` 在平台本地数据目录的 `messages/messages.json` 中保存最新 1,000
条应用消息。发布操作立即更新小型内存快照；唯一 Worker 在 Slint event loop 之外
执行 JSON 持久化和 `notify-rust` 系统投递。原生投递失败不影响应用内持久消息。
投递策略与分类、语义级别正交：普通事件使用 `WhenUnfocused`，明确的应用内事件使用
`Never`，严重或诊断事件可使用 `Always`。Shell 把窗口活动状态传给消息服务，避免
应用聚焦时 Banner 与系统通知重复。
已读状态、单条删除和批量操作都会发布不可变快照；根桥接层在 event loop 更新未读数
和独立页面 model。关闭短时 Banner 不得修改持久 Inbox。详见
[specs/messages.md](specs/messages.md)。

消息契约把入口 `channel` 与业务 `category` 分开。内置键覆盖通用、Webhook、软件更新
和 CD 流程消息，但分类使用开放字符串词表，因此 Feature 新增分类无需修改消息中心。
软件更新 Banner 在镜像时完成分类；CD 部署/回滚完成后通过共享 Notification manager
发布分类消息。UI Components 提供独立 Message center 测试分类，覆盖分类与投递策略。

未来 Webhook 接收器应位于单独入口适配器中，负责鉴权并把提供方 Payload 归一化为
`MessageDraft`，不得改变存储、订阅、系统投递或 Inbox。按入口通道限定的外部幂等键
用于消除提供方重试，只有有界且非敏感的元数据能够跨越适配器边界。当前实现不包含
Webhook HTTP Listener 或任何提供方专属 Schema。

### 项目

`project/projects.json` 保存于平台 local-data 目录。新增或编辑项目保存时，Rust 根据 `remote_repo` 推导 Provider，由 `src/git/` 使用 `src/codereview/api/` 中的 URL 模板查询项目详情，并把返回的 `repo_project_id` 与小写 `repo_type`（`gitee`、`gitlab`、`github`）一并保存。查询失败不会改写已有配置。Rust 将记录转换为 Slint model，解析本地/Base64/远程 Logo，并调度 PR 概览任务。远程 Logo 缓存位于进程临时目录，UI 渲染路径不应直接进行网络 IO。

用户主动刷新项目注册表时，会把阻塞式存储读取隔离到共享 runtime；加载期间
阻止重复触发，并通过 Slint event loop 回写 model 或可恢复错误状态。
`ProjectStateFacade` 是内存注册表与仓库元数据 selector 的权威来源。新增、更新、删除、
pending save、Logo 与概览均通过强类型 Action commit；controller 不再为元数据重读
`projects.json`。deferred `PersistentStore` 在首帧后 hydration 完成才启用，合并待处理
durable 快照，并以同目录临时文件原子 rename 替换 `projects.json`；仅 runtime 状态变化
不会重写未变化的持久记录。写入失败时只回滚仍为当前版本的乐观快照，不覆盖后续编辑；
只有真实写盘完成后才提示成功。
`ui/views/project.slint` 只负责路由编排；可复用的项目卡片、工具栏控件和对话框统一位于
`ui/components/project/`。Home、Search、Detail、Project 路由状态统一复用
`GitHubPageHeader` 与 `GitHubBlankslate`，不再维护页面局部占位模式。

### Workflow

Workflow 在所有支持的宽度下统一使用 GitHub 仓库页的 UnderlineNav 交互；低于 720px 时，仓库路径与操作从身份页头移入尾部 kebab 浮层。PR 列表严格复刻 GitHub 查询与响应式筛选交互：卡片上方放置 Search 风格查询框，Open/Closed 与筛选保持在卡片页头同一行；低于 620px 时直接保留 Assignee，把 Reviews 与 Sort 合并进一个分组 kebab ActionMenu。所有菜单互斥，第一次点击即可切换。Closed 使用 GitHub 语义，包含 merged 与关闭未合并。`is:merged` 和 `is:closed is:unmerged` 分别筛选两个精确子集；与 GitHub 一致，任何非标准查询都会把 Open/Closed 替换为“清除查询”入口与 Total 汇总。PR 行在 Slint 中测量标题换行后的实际高度，由纵向布局累加每行自然高度；Workflow Shell 直接消费组件最终高度，不再自行预测固定行高。Controller 解析可移植的状态/Review/Assignee qualifier，使查询框与工具栏切换保持同步，并在查询变化后重置分页。Provider adapter 负责翻译归一化查询：GitHub 使用原生 Search qualifier，Gitee 企业版使用文档规定的状态/search 参数，GitLab 为 Closed 合并按 Provider 顺序排列的 closed/merged 前缀，Gitee 公有 v5 无法精确服务端筛选时执行有界客户端扫描。项目或筛选条件变化时独立获取 Open 与 Closed 总数，页头不再复用当前选中分页的临时数量。总数仍在加载或计数请求失败时，页头按 Primer 降级体验隐藏数字并保留当前汇总标签，而不是长期停留在加载占位。列表时间使用相对时间，并提供东八区绝对时间 Tooltip。

PR 行把绝对时间 Tooltip 的命中区限制在时间文字本身，并预留响应式尾部操作组。长标题在操作组之前自动换行；目标/源分支对在宽布局同行展示，在窄布局移到续行。Provider 列表/详情字段可提供 Conversation 数量。Conversation 会直接在 Tab 0 打开审核路由；AI Review 打开既有 Files changed 审核工作区。

本地 Tag 刷新在 blocking worker 解析 Git 状态，并且只有项目 ID 与请求序号仍匹配时才
提交结果。GitLab Closed 分页与 Gitee 公有版客户端筛选共用 `src/git/` 的 TTL/LRU
PrefixCache 边界。每个 adapter 只增量补拉缺失的完整 Provider 页，缓存 30 秒且最多保留
32 项，按 IID 去重；显式访问第 1 页时刷新，并在合并成功后清除该仓库缓存。Gitee 公有版
的 merged 与关闭未合并子集共用一份原始 closed 前缀，再在本地完成筛选。

Workflow 顶层 PR/Tag/Branch/CD 导航和 Tag Add/Remove 导航统一复用
`GitHubUnderlineNavItem`；Tag Scope 与语义版本选项统一复用
`GitHubSegmentedControl*`。Branch 展示现有项目级校验计划，CD 展示现有分阶段
部署和回退 Controller，不再渲染不可用占位页。Branch/CD 的 Field、Action 和
Result 统一位于 `ui/components/workflow/`，在紧凑宽度下自适应，并使用应用自有
GitHub 控件。

`WorkflowStore` 保存当前项目和各项目的 `WorkflowViewState`。切换项目必须重新绑定 UI，避免日志、步骤和 running 标记串到其他项目。`Route.Workflow` 与 `Route.PrReview` 构成同一生命周期域：两者之间切换保留 Workflow 列表/model 状态；临时进入域外路由时，只要仍可通过 Back 返回任一 Workflow 路由，也继续保留这些状态。

跨页面 Back/Forward 由 `src/router/` 统一维护有界双历史栈：直接导航把当前路由压入 Back 并清空 Forward；Back 把当前路由移入 Forward；Forward 执行相反操作。Slint 只渲染当前路由并委托历史动作，不再自行改写单槽位的 `previous-route`。不改变 Route 的页内下钻返回点由对应 feature controller 管理。Back 先消费 feature 页内返回点，再弹出全局路由历史。只要任一历史栈仍保留 Workflow 路由，它就是可恢复的活动目标，因此从 PR 详情临时打开设置、关于或其他页面时不得清理审核状态；只有 Workflow 域既不是当前路由、也无法通过历史导航恢复时，才释放重状态。`RouteLifecycleHooks` 同时提供普通 transition hook 与命名 scope-unreachable hook；Chat 可见性和 Workflow 清理由 `src/app_lifecycle.rs` 而不是 Router 注册。hook 只在 Router history lock 与 hook registry borrow 释放后执行，因此回调可以安全重入应用代码。

应用级键盘命令统一归 `src/shortcuts/` 所有。`ui/main.slint` 中唯一的祖先
`FocusScope` 在当前焦点子控件之前转发原始按键；Rust 注册表负责归一化平台 Primary
修饰键、解析 Application/Route 作用域、校验重叠冲突、检查当前可用性并调用既有
根 callback。只有真正执行的命令才接受事件，因此文本编辑和 Feature 本地快捷键仍由
焦点控件处理。Modal Overlay 会阻断应用快捷键；原生 Chrome 和 Slint Tooltip
从同一注册表读取展示键位，不再硬编码 accelerator。详见
[specs/shortcuts.md](specs/shortcuts.md)。
窗口显示后，唯一的快捷键初始化入口会校验并绑定注册表，再等待平台输入桥接。
底部状态栏发布分阶段进度，只在两个边界都可用后报告就绪；初始化失败会保持可见。
就绪后底栏保留紧凑状态图标，共享状态栏 Overlay 列出阶段与键位状态。常驻工具组
按键盘、带可选数字徽标的铃铛、网络、版本固定排序，并使用稳定命中区与间距。
绿色只用于明确的就绪/在线状态；未读通知使用强调色而不是绿色，通知数为 0 时不挂载
徽标。所有底栏 Overlay 必须在同一父级坐标系中计算 viewport 与 anchor，保证各条目
在任何宽度下对齐。

窗口显示前，设置恢复会把持久化的侧栏开关、弹簧进度和路由宽度一次性吸附到同一个
最终状态；只有之后由用户触发的展开或收起才启用侧栏动画。

应用 Shell 统一拥有一个 240px 桌面侧栏。持久化固定状态驱动同一个可中断、按显示帧推进的弹簧，移动侧栏、内容表面、标题栏分割线和状态栏绘制起点。Slint 显示动画时钟把真实帧间隔交给闭式阻尼振子，因此在 120Hz 屏幕上每个渲染帧都会更新位置，延迟帧也不会积累固定步长误差；目标反转保留当前进度和速度。路由只在运动开始时采用一次目标布局宽度，状态栏透明宿主与右侧常驻工具组保持窗口坐标。连续点击标题栏按钮重定向同一运动，不排队旧回调。原生 Live Resize 会把侧栏运动吸附到目标，冻结已挂载路由宽度，并在拖拽结束后一次应用最终宽度。固定态边界 Elevation 由主内容面拥有并只向侧栏方向投射；取消固定后，窗口左侧 8px Hover 热区把同一侧栏作为只有边框、尾部上下圆角的根层临时浮层打开。macOS 通过原生标题栏附件提供侧栏、Back 和 Forward。面包屑占用剩余原生标题栏安全区，原生附件、Slint 面包屑和尾部操作统一按完整 46px inset 中轴线对齐。macOS 的 `NSVisualEffectView` 固定为 240px 宽，只在侧栏下方提供 Menu/BehindWindow 材质；Slint 渲染器下方另有与当前主题 Canvas 一致的全窗口不透明原生底面，GPU Surface resize 晚一帧时也不会在右侧露出壁纸蓝色。Vibrancy 安装先切换全尺寸透明标题栏样式，再在窗口仍不透明时挂上 Effect 与底面，最后才打开透明窗口路径并使用近透明背景而非 `clearColor`；整个过程在启动窗口 alpha 门控期间的同一个 AppKit 事务内完成，避免启动瞬间半透明 Slint 侧栏透出桌面内容。固定侧栏 Rectangle 在 Effect 上绘制随焦点变化的最终表面；失去 Key 时隐藏 Effect。临时 Hover 侧栏保持独立不透明浮层。按钮可用状态由 Router 历史驱动；三端共享 Router callback 和侧栏偏好，平台代码不持有路由历史。

Workflow PR 列表与全窗口审核工作区统一通过 `src/git/` 的 Provider-neutral façade 请求数据。`RepoContext` 一次性校验当前项目持久化的 Provider/id 与 `remote_repo`，随后列表、详情/diff、仓库文件、review state、review submit、merge 和 revert 均由对象安全的 `GitProvider` 分发；`ProviderCapabilities` 统一暴露 endpoint 能力，UI 不自行推断。共享 JSON 解码与分页 helper 归 `src/git/json.rs` 所有。`workflow/pr_review.rs` 是稳定 façade，`workflow/pr_review/controller.rs` 负责异步编排和 UI 回写；共享缓存/Diff 所有权与头像 I/O 分别位于 `cache.rs`、`avatar.rs`。固定源码对齐与 hunk 展开纯算法位于 `workflow/pr_review/hunk.rs`，本地文件树构建与分类位于 `workflow/pr_review/file_tree.rs`，Unified/Split 行映射位于 `workflow/pr_review/diff_projection.rs`，Slint 惰性行适配器位于 `workflow/pr_review/diff_model.rs`，选择几何与剪贴板投影位于 `workflow/pr_review/selection.rs`，Conversation/Commit 展示与范围映射位于 `workflow/pr_review/timeline.rs`。审核控制器只持有一份共享且不可变的归一化 diff，其 feature 自有 state-store façade 为 diff、高亮、展开、Conversation、Commits、远端 action-state 读取和远端 mutation 保存七类独立强类型 generation。归一化详情会把每个基础文件移动到独立 `Arc` 中，当前文件的惰性 model 与高亮任务直接共享该分配，不再深拷贝全部行；只有展开 hunk 时才创建对应文件的写时复制 override。Provider HTML 在 adapter 边界解码，每条 diff 记录在进入固定高度的 Slint 行渲染前都会归一为不含 CR/LF 控制符的单个视觉源码行。收起文件时删除 override 并重新使用共享基础文件。hunk 展开或应用内查看完整文件时按固定 head/base commit 懒加载源码；修改文件展开时会在本地对齐两份固定版本，确保单侧独有行不会丢失。每文件 Diff 高亮使用 4 项 LRU，固定源码文本使用 3 项 LRU；完整文件高亮 model 只是临时 UI 状态。当前 Diff 通过只读惰性适配器暴露给 Slint，最多物化 64 行转换后的 UI 数据；完整高亮结果继续由 Rust 持有，以保留任意语言支持和快速滚动。所有异步回写均由对应 generation 防止迟到响应污染当前项目或 PR。

PR Review 将逻辑选区与 shaped-text 几何分离：`selection.rs` 负责行/字符范围和剪贴板
投影；`text_geometry.rs` 是 styled-run 与软换行 caret 命中及范围边界的可替换数值门面，
默认 adapter 使用有界 Parley 0.10 cursor/selection 几何、Slint 共享字体集合和精确 run
样式。换行范围会合并同一视觉行内相接的 fallback-font fragment，并使用固定 pitch、互不
重叠的绘制矩形。Slint 继续
原生绘制 `syntect` token model，且不导入 Parley 类型。不嵌入 WebView/浏览器编辑器；
门面缓存有界，滚动或 resize 路径不执行 Find viewport 定位工作。

Tag 工作流采用相同的 façade 模式：`workflow/tag.rs` 暴露绑定和刷新，
`tag/controller.rs` 负责项目状态、UI 序列、取消和命令执行，`tag/version.rs` 与
`tag/commands.rs` 保存可直接单测的纯规则。应用级 `src/jobs/` model
把不可变的 `name + argv` Step 定义与运行时数据分开，并用单个 `JobSnapshot`
统一持有 Job status、conclusion、总耗时和有序 Steps。来源 Feature 发布
`JobRunDescriptor` 与快照，并用稳定 source key 注册 Stop 和日志懒加载动作。
`Route.JobRun` 不属于 Workflow 生命周期域，因此可从任意页面进入，并由全局 Router
历史精确返回来源；该路由只读取通用 Job Run 属性，不读取 Workflow 或 Tag 属性。
`ui/components/jobs/job.slint` 渲染选中 Job 快照，`github_job_run.slint` 提供
GitHub Actions 风格页头、Jobs 区和响应式日志布局；两者都不负责 I/O 或计时。Git 命令始终
保留 argv 边界，仓库路径、Tag 名称和描述都不经过 shell 插值。CD 把项目级
Stage Snapshot 适配到共用 `GitHubActionsJob` 视觉展示；Branch 当前只生成校验
计划、不执行 Job。未来若让 Branch 执行命令，必须采用复用 Job 边界，不得复制
展示组件或向通用组件加入领域分支。
Step 原始日志在 disclosure 第一次请求行 model 前始终保留在项目级 Rust 快照中，
该请求标记在后续 progress 快照替换时继续保留。Tag 命令执行期间持续读取
stdout/stderr 并发布有界进度，因此已展开 Step 可实时更新。展示组件把 GitHub
`notice`、`warning`、`error`、`debug` 标注映射为 Primer 语义行，为运行中 Step
旋转 attention 色 Spinner，并让 queued Step 保持不可交互。

Revert 命令与 review/merge 命令共用 Provider-neutral façade 和强类型 remote-mutation generation，迟到响应不得影响其他项目或 PR。已合并或已关闭请求隐藏页头 review/merge 控件；在已配置凭据时 Conversation 评论框保持可用（GitHub Conversation 评论按 issue comment 写入，因此合并后仍可留言）。只有 adapter 返回 `can_revert` 时，已合并请求的页头才在就绪徽标同一行显示明确可见的 Primer 风格次级回退按钮（Usher 不用 GitHub 页头 kebab ActionMenu，以保证完整标签可读）。GitHub GraphQL 创建独立的反向 PR；Gitee 企业版重新读取 MR，要求 merge/squash commit 的 `/repository/commits/:sha/refs` 响应同时返回 `merge_commit_by_pr` 与 `can_revert`，随后对目标分支调用文档定义的 commit revert endpoint。rebase/fast-forward、Gitee 公有版与 GitLab 均保持不支持；Provider 专属确认说明必须明确 Gitee 会直接修改目标分支。

Gitee 企业版 Commit 角标只把 MR detail/diff 中的正数摘要视为可信总数。部分部署在读取 commits 集合前会报告零，因此零值或缺失时必须在激活 Commits Tab 前执行轻量计数探测；完整 Commits 内容 model 仍保持懒加载。

Files changed 的 `Diff view` 弹层统一持有与 Provider 无关的 Unified/Split 偏好，以及按能力开放的 Hide whitespace 草稿。`workflow/pr_review/diff_projection.rs` 从缓存的归一化行生成 Unified 或 Split 投影；Split 将每个连续删除/新增块按行配对，并把上下文同时映射到左右两侧。只应用视图选择时在本地替换当前懒加载行 model，应用受支持的空白字符变化时才重新加载 Provider 数据。Slint 负责渲染投影，并让不支持的空白字符复选框保持可见但禁用。弹层完整表面持有指针拦截层，因此空白区域不会把 hover 穿透到下面的文件树。弹层挂在整行 Files changed 工具栏上，Primer 向上三角箭头的尖端跟随设置齿轮，即使面板被边缘夹紧也保持对准。

大文件按与 Provider 无关的变更行数或载荷体积预算判定。正常文件头保持可用，但语法高亮和 Slint 行 model 延迟到用户对该文件点击 `Load diff` 后才创建。原位加载状态仅属于当前完整 Diff/commit range，切换范围时重置。

PR Diff 行 model 每次原子替换都会递增内容 revision，使 Diff 内搜索基于当前已展开的全部行重新计算。该信号与 viewport 重置 revision 分离，因此同文件 hunk 展开仍保留滚动锚点。

PR 列表评审结论在 adapter 边界统一为“需要评审”“已批准”或“请求修改”。GitHub 根据当前 Review 与 requested reviewer 补齐可见行，Gitee 映射 `review_state`，GitLab 优先读取逐 MR approvals，并以 `detailed_merge_status` 回退。Slint 对所有 Provider 都在时间戳后显示相同的 GitHub muted 行内评审链接，点击进入 Conversation；列表不使用 Provider 专属彩色状态标签。

固定的 PR 列表评审状态验收数据归 `tests/` 所有，只有同时启用 `test-fixtures` Cargo feature 和指定的 `USHER_PR_LIST_FIXTURE` 选择器时才能进入。默认与生产构建不会编译 fixture 桥接模块，也不会嵌入其 JSON。fixture 仍通过正常列表控制器进入同一个 Slint model、筛选、排序与响应式布局，但不会进入 Provider adapter。

`workflow/pr_list.rs` 同时拥有可移植查询语法和逻辑分页边界。默认查询为 `is:pr is:open`；顶层状态操作只有 Open/Closed，归一化状态另含 merged 与关闭未合并。Controller 对每次 query/page 响应做序列保护。GitLab 只合并满足当前逻辑页所需的有界 closed/merged 前缀，确定最终可见行后才补全 approvals；Gitee 公有版仅在 v5 无法表达的精确子集或关键词搜索中扫描，且最多读取 30 个 Provider 分页。

仓库 PR 列表归现有 Workflow 的 PR Tab 所有。`workflow/pr_list.rs` 在 event loop 之外读取当前项目持久化的 `repo_project_id`、`repo_type`，并且只在进入或用户切换页码时，通过 `src/git/` 懒加载对应的 10 条远端数据。GitHub 保留搜索接口提供的总数，并发请求当前页每个 PR 的详情，从而补齐源分支和目标分支名称。控制器在将列表值传给 Slint 前，把可解析的 Provider 更新时间统一格式化为东八区（UTC+08:00），并在 Provider 提供时使用准确总数驱动顶部总数和底部分页。页面默认显式查询 open 状态，用户可切换 open/closed/merged/all；请求序号保证旧项目、旧筛选或旧页码的延迟响应不会覆盖当前视图。请求成功且零条数据时显示随筛选状态变化的插画空状态；网络、鉴权或响应结构异常仍使用独立且可重试的错误状态。点击列表行后导航到全窗口 `Route.PrReview`；Workflow 组件实例会被销毁，但列表/root model 状态继续保留，因此 Back 可在不重载列表的情况下重建 PR Tab。审核路由拥有 GitHub 风格的 Conversation、Commits、Files changed 三个 Tab。Conversation 与 Commits 的 *内容* 只在第一次选中时通过 Provider-neutral model 加载：GitHub 组合 PR detail、issue comments、reviews，以及 issue timeline 活动（`committed` 批量、`merged` 与部分系统事件）；Gitee 公版 v5 使用 PR detail/comments/commits，并从 commits 与合并元数据合成提交/合并活动；Gitee 企业版 v8 使用 MR detail/notes/commits，并根据 detail 中 `general_reviewers` / `primary_reviewers` 的 `approve_status` 生成评审摘要；在展开嵌套 `notes[].note` 后同样合成活动，普通评论正文从 `note` / 嵌套 `note.note` / `comment` / 字符串 `content` 读取（忽略对象形态的 `content`），MR commit 时间取 `created_at`；GitLab 使用 MR detail/notes/commits，合成同一套可移植的 `commits`/`commit`/`merged` 行并过滤重复系统 note。Tab 角标数字不等待 Conversation/Commits Tab 激活：它们来自随 diff 加载携带的轻量 PR/MR 摘要字段（`commit_count`、`conversation_count`、`files.length`）。若 Provider 省略总数，adapter 可发起一次有界计数探测（GitHub/GitLab 的 Link/`X-Total` + `per_page=1`；Gitee 公有版短页 `per_page=100`；Gitee 企业版 diffs `commits_size`、MR `user_notes_count`/`notes_count`，或 `with_page=true&per_page=1` + `meta.total_count`）。完整 Conversation 时间线与 commits 列表仍只在打开对应 Tab（或 commit 选择器）时懒加载。Provider-neutral 终结器把每个 `commits` 头与其后连续的 `commit` 行作为一个不可拆分的块排序，因此即使各提交 authored time 不同，三家 Provider 的 GitHub 风格 `added N commits` 记录也不会与提交列表分离。Conversation 批次内的 commit 子行与 Commits Tab（含 Files changed 的 commit 选择器）均按从新到旧展示；adapter 仍把列表归一为从旧到新，控制器在连续范围比较前把展示索引映射回去。远端 review 或 merge 成功后会使 Conversation 时间线缓存失效；若 Conversation 正处于激活状态，则立即重新加载。Files changed 控制器加载并归一化所选 PR diff，根据文件路径构建可折叠目录树，并让路径筛选、目录展开和上一项/下一项文件导航保持本地操作。其 commit 选择器复用懒加载提交列表，把连续范围分发到各 Provider 的仓库 compare endpoint；原始 PR detail 和已加载范围在当前审核期间保持缓存。`workflow/syntax_highlight.rs` 在 UI 层之外使用 `syntect` 把当前文件转换为适配主题的 token model，同时保留 Tab 和显示列宽。切换文件或展开 hunk 时，旧行 model 会保留到新高亮 model 完成，再由 event loop 原子替换；只有真正切换文件时才重置 viewport。点击 hunk 时先记录被点击行的实际屏幕位置。model 替换后，目标 delegate 上报布局后的内容位置，Slint 在有界的多个布局帧内重复应用由此得到的视觉锚点。该方案不再估算软换行行高：仍存在的 hunk 控件保持在指针下方；若分隔行消失，首条新增源码占据原控件位置。在 macOS 上，原生 live-resize 通知会冻结审核工作区的内部几何尺寸，并把每条可见行的 token repeater 临时替换为一个纯等宽文本节点；live resize 结束时恢复最终几何尺寸和完整的 token 着色组件树。

Gitee 企业版 Conversation 只使用 PAT 鉴权的 v8 API。detail/current-state URL 已在路径中携带 PR IID 时，不再把同一个 IID 重复作为 `pr_id`。只有 MR detail 的评审人字段明确返回 `approved` 或 `rejected` 才生成评审行。应用不调用依赖浏览器会话的 `/pull_requests/{iid}/activities`，也不保存、不转发、不记录浏览器 Cookie。Provider 原生话术不得进入 Slint。合并人从 `merged_by` 等专用详情字段取得；若这些字段只有邮箱/账号标签，则静默调用文档定义的项目成员接口做账号精确匹配，以成员资料的 `name` 与头像补全展示，用户目录作为静默 fallback。Merge commit 的 `committer_name` 只是更低优先级的 Git 身份。模糊目录结果必须拒绝，完整邮箱地址不得作为操作人标签，且绝不以 PR 作者作为合并人回退。GitLab 同样只使用 `merged_by` 并禁止 PR 作者回退；GitHub 保持原生 `actor.login` 展示。

提交活动在 Provider API 可用时保留原生分支更新批次。GitHub 使用 issue timeline `committed` 批次；GitLab 使用 `added N commits` system note，并把其 SHA 引用与当前 MR commits 匹配；Gitee 公有版 v5 使用 operation-log push 记录；Gitee 企业版 v8 加载项目 push event，按 MR 源分支过滤，并把 `commit_from`/`commit_to` 映射到归一化 commit 顺序。push event actor 与每个 commit 的 Git author 是两个独立身份。活动数据缺失或无法使用时，退化为单个 Provider-neutral commit 批次，不虚构更新者。PR 详情停留期间不轮询；离开路由时清除其自有 Conversation、Commits、diff 和 commit-range 状态，因此从保留的列表重新打开 PR 会执行全新 Provider 请求。

审核页头采用 GitHub 的两步命令模型。标题旁的 GitHub 风格就绪徽标使用 Primer 语义色渲染归一化类型/详情，但合并语义由各 adapter 独立负责：GitHub 映射 REST mergeability，Gitee 企业版映射 v8 `current_state` 条件，Gitee 公有版只映射 v5 字段，GitLab 映射 `detailed_merge_status` 并请求异步重算。共享 enum 只是展示边界，绝不能把一个 Provider 的判定条件套到另一个 Provider。`Review changes` 提交显式的 Provider-neutral Comment、Approve 或 Request changes；不受支持的选项保持可见但禁用。Conversation Tab 底部另有 GitHub 风格纯文本评论框（`Write` + 绿色 `Comment`），提交同一 Comment 动作。符合条件的 `review_summary` 行使用 GitHub 的逐条评审 kebab 与必填原因的 `Dismiss review` 弹窗；页头聚合状态只作展示。Provider-neutral 撤回门面在仓库权限允许时把选中的 review id 交给 GitHub dismissal endpoint；GitLab 先写入原因 note，再撤销当前 Token 用户的 approval；Gitee 企业版通过实际可用的 Web API 路由 `/api/code/api/{full_path}/-/pull_requests/{iid}/reviews` 撤回当前 Token 用户的评审，请求体严格为 `{"state":"initialized"}`。没有该 Web 路由的企业版部署会静默回退到文档规定的 v8 reviews 路由，成功后再以 best-effort MR note 记录必填原因；Gitee 公有版 v5 不具备撤回能力。Provider 成功后刷新远端状态，绝不自动开始合并。`Merge pull request` 只由最新的归一化 `PullRequestActionState` 启用，并且只展示 adapter 返回的策略。GitHub 支持仓库启用的 merge、squash、rebase；Gitee 企业 v8 映射评审状态，Approve 未填写 summary 时省略可选的 `comment` 字段，并按 `restrict_merge_type` 支持 merge、squash、rebase、fast-forward；Gitee 公有 v5 支持评论和 merge/squash，但没有 approve/request-changes endpoint。GitLab 支持评论、批准，以及携带 SHA 防护的 merge/squash 和显式删除来源分支；项目 `squash_option` 限制可选策略，稳定版 REST API 没有对应写命令，因此 Request changes 保持禁用。

审核文件树使用 GitHub 的平台 UI 字体栈（macOS 使用 SF 系统 UI 字体，Windows 使用 Segoe UI），代码区使用平台等宽字体栈。文件树保留虚拟化 `ListView`；Diff 行改由 `usher-virtual-list` 精确行高门面计算，并在 `ScrollView` 中挂载稳定、有界的 Slint overscan model。宽度/model revision 只重建轻量行 extent，普通滚动则增量移除和追加可见行。分隔线实时调整面板宽度；移动手柄以当前宽度叠加剩余的局部指针位移，使分隔线贴合指针，同时避免重建完整文件树或 Diff。会话宽度限制为 220–520 px 且不超过审核区宽度的 45%，确保 Diff 仍是主要区域。可见性对齐 GitHub Primer `$width-md`（768px）：宽度越过临界值向下时自动折叠、向上时自动展开；工具栏开关在任意宽度下都可用，受限工作区自动收起后仍可手动打开。应用窗口最小宽度为 800px，因此普通桌面 Resize 不再到达该内部断点；响应式规则继续用于受限 Host 和直接工作区测试。

审核工作区从 macOS 原生标题栏 inset 下方开始并复用 Shell 原生 Back。根渲染树固定在窗口 `x=0`，普通页头、Tab 和 Diff 内容通过内部左内缩跟随 Shell 弹簧，并在运动开始时只采用一次目标宽度。原生 Live Resize 时工作区保持上一次稳定内部几何直到拖拽结束，避免整棵已冻结/虚拟化审核树反复重排，同时 Dialog 使用全窗口坐标。Shell 先把 Back 交给审核控制器，真正离开时再使请求失效并由 Router 返回 Workflow。PR 操作位于标题栏；就绪 ActionMenu 在窗口根层渲染。文件筛选和文件操作菜单仍是互斥工作区 Popover，并支持第一次点击直接切换目标。

Conversation 与 Commits adapter 归一化 GitHub、GitLab、Gitee 公有 v5 与 Gitee 企业 v8 的可选 Git author、committer 身份及头像 URL。GitHub 顶层独立的 `author` 与 `committer` 用户会完整保留。Gitee 企业版通过 repository commit detail 惰性补齐 Commits 行；user-directory 查询只接受邮箱精确匹配，并拒绝歧义结果。UI 遵循 GitHub 话术与头像几何：单一身份显示 `{author} authored`；不同身份显示 `{author} authored and {committer} committed`，并配合交叠双头像。控制器通过共享 HTTP pipeline 并发获取去重头像，整体超时 3 秒、单图上限 2 MiB，并限制解码尺寸；只有可跨线程发送的 RGBA 字节进入 event loop，再创建 Slint Image。头像缺失或加载失败时回退为作者首字母。时间线卡片和评论框共用 52 px 头像/内容网格。GitHub 风格活动使用一条贯穿头像中心的 Primer 时间线轨道（首尾按 clipSidebar 裁剪）。评论/描述卡片为 1px 边框盒子并随短正文收紧高度；commit 图标与紧凑作者头像落在同一轨道；所有活动行与卡片右边缘对齐。Rust 提供准确的行高总和。Conversation Tab 把时间线与评论框放在同一文档滚动里，Write 区域跟在最后一条事件之后，而不是钉在视口底部。

Conversation 提交批次使用 Primer 官方 `repo-push` Octicon；相对时间紧跟 `added N commits`，共享的绝对时间 Tooltip 保留 Provider 的准确时间。GitHub Review 会展开为不可拆分的活动摘要与可选正文卡片；review state 决定话术，相对时间采用 GitHub 自动单位，merged 活动抑制冗余 closed 事件。连接线按事件语义边界分段，但连续 Review 对之间必须从前一张卡片到下一条 summary 标记保持连续轨道。Gitee 企业版普通评论使用 MR `/notes`，并固定发送 `noteable_type: PullRequest`；`/reviews` 只用于 Review 决策。commit 计数探测若没有 total 且响应恰好填满请求页，即使是裸数组也视为不确定，随后统计并丢弃文档约定的完整非分页集合，不挂载懒加载 Commits model。

Diff 几何只使用同一份实测宽度：Rust 在显示列元数据之外传入最长的已展开代码行，Slint 使用当前等宽字体测量，并让横向滚动与每行背景共用该像素宽度。代码栏支持 GitHub 风格的跨精确虚拟行拖拽选中；软换行继续使用 Slint `TextInput.word-wrap` 原生 UTF-8 选区几何。PR 虚拟列表 adapter 使用轻量显示列指标和最终稳定宽度计算相同换行数，再以精确前缀和统一驱动 content extent、滚动条映射、可见窗口与 Find reveal。⌘/Ctrl+F 定位仍是一次性命令：只有 Enter、Shift+Enter 或上一个/下一个控件计算并至多写一次 viewport。普通滚动只更新有界虚拟渲染窗口，绝不执行 Find 定位；Find 打开时活动与非活动高亮都限制在已挂载行内。review controller 的同步命中测试仍按投影 model 的实际换行高度解析字符，⌘/Ctrl+C 静默复制纯文本。

AI Review 只发出 finding 位置，位置解释统一由 PR Review 负责。控制器先校验 Review head SHA，再把 left/right 对应的 old/new 源行映射到 Unified 或 Split 展示行；延迟 large diff 会自动载入，目标位于隐藏区时复用固定源码缓存，只插入以目标为中心的一页上下文。独立 Reveal 代际保证快速连续点击只应用最后一次请求。Slint 通过虚拟列表门面执行一次精确 `CenterIfNeeded`，并在 gutter 绘制不覆盖增删语义背景的 side-aware 标记；过期或缺失目标保持明确未定位状态，直到用户显式选择 View file。

对不换行代码，token 内、跨 token、跨行拖拽目标的鼠标命中和相交选区/当前 Find 矩形
使用该门面的 shaped cursor 边界；run 序列命中累加每个独立 Slint token 的 shaped advance。
这替代了标点、CJK fallback、kerning、粗体和斜体
run 上的显示列/独立前缀宽度近似，同时把缓存上限固定为 256 个 layout。换行代码继续
使用 Slint 原生 UTF-8 `TextInput.word-wrap` 几何。

每次显式 Find 导航命令都会立即完成计算和可选的 viewport 写入，不存在等待后续滚动或布局处理的请求。修改查询、手动滚动、resize 和焦点变化只保留查询、计数、当前结果与当前高亮，不移动 viewport。

Conversation 与 Commits 的 commit 链接会按 SHA 定位并复用 Files changed 工作区，把当前 commit 应用为单项范围。审核控制器会把来源 Conversation 或 Commits Tab 记录为 feature 页内下钻返回点，因此 Shell Back 会先恢复来源 Tab，下一次 Back 才返回 Workflow 的 PR 列表；如果 commits 尚未加载，控制器会在加载完成后应用待处理选择，复制 SHA 保持独立。

合并成功会把保留的 PR 列表标记为过期。审核页顶层 Back 会重新加载当前列表筛选的第一页及状态总数，确保已合并请求不再留在 Open 列表中；页内 commit Back 仍只恢复来源 Tab，不会提前消费这次刷新。

PR 展示层遵循统一 UI 边界：列表、筛选、分页、审核外壳、文件树和 diff
等可复用控件位于 `ui/components/`，组件符号以 `GitHub` 开头；
`ui/views/workflow/` 只负责全窗口路由装配。公开的 `@material`
ListView/ScrollView 可以为大型时间线、文件树和 diff 提供虚拟化，但不拥有产品
排版、配色、焦点状态或控件外观；这些能力全部由项目自有 GitHub 组件负责。
空白/错误状态复用 `GitHubBlankslate`，图标操作复用 `GitHubIconButton`。

PR Review 为 diff、高亮、展开、AI finding Reveal、远端 action-state 读取、远端 mutation、Conversation 与 Commits 使用八个相互独立的 typed state-store 代际；启动 action-state 刷新不会使正在执行的 mutation 失效，启动一种惰性读取也不会使另一种失效。切换 PR、手动 Reload 与 ignore-whitespace 变更通过一个 reducer action 使八类代际全部失效；强刷绕过 diff 缓存、清空惰性 Tab model，并重新加载当前 Tab。Provider changed-file 分页共用一个有界收集器；最后一页仍满时把归一化 detail 标记为 `files_truncated`，Files changed 明确展示该限制。Conversation block 按解析后的 RFC3339 时刻而不是 Provider 原始字符串比较。Merge 能力采用 fail-closed：必需的 Review/approval 状态读取失败时，action-state bridge 清空全部写能力并发布不可用 readiness。

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

展示层不接收 Provider JSON 或凭据；Provider adapter 不拥有 UI 状态。仓库服务层通过 `RepoContext` 一次性校验持久化 Provider/id 与 remote URL，再由对象安全的 `GitProvider` 分发全部读写操作；`ProviderCapabilities` 统一暴露空白过滤和服务端回退等能力，UI 不自行推断。通用 JSON 解码与分页 helper 归 `src/git/json.rs` 所有。当前应用通过 Slint、Tokio 和 feature 自有 state-store 门面实现这些层；长期契约是归一化的 page/item/diff/source/action-state 数据，以及 [PR 工作流规格](specs/pr-review.md)定义的“最新请求优先”生命周期。Approve/Comment/Request changes 等决策只通过 Provider review 命令写入远端。

### Chat

聊天历史以 append-only JSONL 保存到 `im/chat_history.jsonl`。新记录持久化带本地时区偏移的完整 RFC 3339 时间，终态 assistant 行同时保存实测 `elapsed_ms`。Rust 展示投影按本地自然日转换时间标签，并把耗时转换为紧凑元数据；旧版记录继续兼容原样显示。等待中的对话只使用一个页面级实时 tick 和按模式保存的单调起始时刻；切换模式或重新挂载 Chat 时恢复实际耗时而不重新计时。AI 与 RAG 分别保留 composer 草稿，迟到的持久化失败也会把内容恢复到原模式草稿。启动时最近一段只加载到 Rust session；Chat 组件及有界 Slint 消息 model 仅在 Chat 路由挂载。加载更早分页时，先在 Session 锁内准备 Store 请求，释放锁执行 JSONL IO，再回锁校验并提交分页。离开 Chat 会销毁 UI 树并清空 Slint model/投影缓存，重新进入时从 session 重建可见窗口。Assistant 消息保留宽度无关的可读文本投影；超过 1,200 个 Unicode scalar value 的投影在 Chat 内有界展示，并通过现有标题入口打开完整回复。格式化 block 由 `usher-preview` 后台准备并交给应用级预览 Host 展示。迟到的历史分页或 assistant 响应继续更新持久化/session，但不得填充隐藏 UI。

生产路径默认启用有界虚拟窗口：一个稳定 `VecModel` 最多挂载 32 行，每行只实例化
当前角色子树，并使用 width-keyed prefix 估算的上下 spacer。启动加载最近 20 条，显式或
自动历史分页每次增加 10 条。`chat-performance` fixture 与
`USHER_CHAT_PERF_VARIANT=baseline` 保留非虚拟化对照路径；普通启动使用 optimized
虚拟路径。生产路径中的 viewport 位置变化必须在 viewport 消耗既定像素 overscan 前提交新的有界窗口；连续大行不得仅为凑满行数上限而
扩大挂载窗口，且不得重建完整历史。滚动停止后不得有延迟任务修改 model；路由自有的
`VecModel` 在整个挂载期保持同一身份，窗口移动只增删非重叠的头尾行，稳定消息 ID 重叠区的
delegate 保持不变。同一 event-loop 周期内的窗口请求会合并，并按最新 viewport 位置提交；
普通滚轮/触控板的重叠移动每次最多引入两行，直接拖拽滚动条则按随机访问处理并绕过该限制及
普通 2,400 px overscan，pointer-up 还会按精确松手位置执行一次最终无上限收敛，并把仅覆盖
可见区的 drag 窗口保留到下一次普通滚动。optimized 普通窗口变更不写 viewport 锚点，位置始终
由 Flickable 独占；只有历史 prepend 与稳定 resize 使用按稳定消息 ID 的精确补偿。Spacer 与
document extent 在滚动前由 width-keyed prefix 给出，该 prefix 使用与消息行相同的有界投影和
Parley shaping，并计入气泡区段、角色最小高度、间距及 Slint line-box rounding。mounted 内容
边缘保护必须在 viewport 进入 spacer 前替换窗口，保护距离为 2,400 px。稳定宽度变化只重建
一次 prefix，高度变化则独立更新 viewport 覆盖
范围。macOS live resize 期间完整消息 ScrollView 保持上一个稳定宽度，并由当前路由 host 裁剪，避免
viewport 宽度失效继续进入消息树；路由底面与编辑器继续跟随原生窗口。松开边缘后只
执行一次最终布局，仅宽度变化始终不替换 Chat 消息 model。同一二进制中的
baseline 路径仅供可复现交互基准通过环境变量选择，不进入用户设置或产品状态。程序主动滚到
底部后会把 Flickable 精确底部最多固定 800 ms，等待初始测量稳定；滚轮或滚动条输入会立即
取消固定。resize 从底部开始时保留贴底意图，其他位置继续保留可见稳定 ID 锚点。
基准分别记录模型变更耗时、变更到首个完成渲染的延迟、增删行数和提交相邻帧间隔；每个
optimized 轮次的提交相邻帧 p95 及每次变更到渲染的延迟都必须不超过 50 ms，并保持稳定空白为零、停止滚动后提交为零、
挂载行数不超过 32，且启动尺寸稳定期间的离底距离不超过 1 px。直接滚动条手势会冻结 scroll
maximum、thumb 几何和指针抓取偏移直到 release；完整 viewport 的滚轮/触控板方向由 Flickable
观测，基准还要求至少十个方向样本，拖拽或滚轮逆向移动不得超过 1 px；两个方向的 24 步
快速拖拽分别至少产生 5 个渲染样本，拖拽期间及松手后的虚拟空白也不得超过 1 px。

Chat 展示层拆分为 `ui/views/chat.slint` 的路由组合与
`ui/components/chat/` 的消息/欢迎组件，并统一使用应用自有 GitHub
ScrollView、Spinner、Button/IconButton、token 与焦点契约。每条消息正文只保留一个
常驻的自动换行普通文本布局；Chat 自有 Parley 缓存、指针层和按视觉行合并的覆盖层提供
选择能力，不挂载 `TextInput`、不改变 preferred height，也不替换消息 model。只有按下
平台复制快捷键时才复制当前 Unicode 安全范围，不提供逐消息复制控件。Chat viewport
把鼠标拖拽交给文本选择，同时保留滚轮、触控板和显式滚动条导航。滚动到顶部会启动
一次去重的历史分页；prepend 提交按稳定消息 ID 与布局前后的真实坐标保持首个完整
可见消息。锚点按 Chat 模式和 generation 隔离，并在有限个布局帧内只补偿剩余的精确
差值直至位置稳定；任何滚轮、触控板或滚动条输入都必须立即取消待执行补偿。消息流内的
分页操作固定在 top spacer 之前的真正历史顶部，不跟随挂载窗口移动。Prepend 在 32 行
预算内共同挂载相邻旧消息与阅读锚点；首次布局主动上报真实消息 viewport 高度，虚拟查询
扣除顶部控件的偏移。覆盖检查不依赖过期的提交方向或滚动 bucket，用户取消收敛时补处理
一次被抑制的滚动请求。该展示边界不得
扩大 Rust 的有界消息窗口，也不得把持久化或 HTTP 工作移入 event loop。

### Terminal

Terminal 状态感知项目；同一项目重新打开时复用。关闭 Drawer 只隐藏展示，PTY 与 Rust 渲染状态继续工作，Slint model 刷新延迟到重新打开。切换项目时先原子地摘下旧 Session、重置展示与工作目录，再由共享 Runtime 的 blocking Worker 终止旧进程树，UI callback 不等待优雅退出窗口；Session ID 会丢弃被替换项目迟到的输出与退出事件。稳定行为契约见 [specs/terminal.md](specs/terminal.md)。

`GitHubTerminalDrawer` 只负责 Drawer 外壳、viewport 上报、输入与单元格展示；
页头/选择操作复用共享 GitHub 控件。终端单元格几何、等宽字体、ANSI 色、PTY
归属与 scrollback 独立于产品排版。

启用非默认 Cargo feature `ui-components` 时，主菜单公开 `Route.UiComponents`
作为应用自有组件演练场。它使用真实共享控件和
根层 Toast、Notification、Dialog Manager 演练属性状态。依赖业务 model 的组合
组件继续在 Project、Workflow、Chat、Settings、Shell 或 PR Review 真实路由中
测试，并由演练场登记表链接过去。原 Login、Tools、List、Detail、Notification、
Toast、Dialog demo 路由已删除。关闭 feature 时，两个侧边栏都不显示入口，直接
导航到该路由会回退到 Home，展示页也不进入 Slint 编译和 live preview 依赖图。

## 主要数据流

### Project 到 Workflow

1. Project 卡片触发根组件的 workflow-open callback。
2. `workflow/bindings.rs` 构建 `WorkflowProjectContext`。
3. `WorkflowStore` 选择项目级状态。
4. 各 Workflow 模块初始化对应 Slint property。
5. PR 数量通过共享 runtime 和 Provider-neutral Git façade 加载，再经 event loop 回写。
6. PR 列表控制器读取持久化仓库元数据，经共享 runtime 懒加载当前选中的 open 远端分页；每次翻页只请求对应页。

### PR 列表到审核

1. 列表控制器在 UI 线程外读取仓库元数据；项目或列表状态变化时清空页缓存。
2. 仓库服务拒绝与 `remote_repo` 不匹配的已持久化 `repo_type`，然后把请求路由到对应的 Provider adapter。
3. Adapter 将 Provider 响应归一化为最多 10 条的页面。GitHub 保留搜索接口的准确总数，并发读取当前可见行详情以补充分支 ref。
4. 控制器只在强类型 Page 代际仍为最新时应用页面；Open/Closed 总数使用独立的 Totals 代际。在生成 view model 前将可解析的更新时间格式化为东八区（UTC+08:00）。
5. 打开某一行后，在共享 Workflow 生命周期域内从 `Workflow` 导航到 `PrReview`。Workflow 组件被卸载但列表/root model 保留；审核控制器重新读取元数据、请求一份归一化 diff，按项目/请求/空白模式缓存并渲染本地文件树。
6. 文件选择、筛选、目录展开、复制路径、比例条渲染、语法高亮和收起已展开上下文均在本地完成。只有 hunk 展开、AI finding 隐藏上下文揭示和应用内 View file 会产生后续 Provider 读取；三者共享有界的固定源码缓存。方向展开以 GitHub 登录后的新版 `/changes/{commit}` 页面为基准，每页补充 21 个主侧源码行；AI finding 只插入以目标为中心的一页，并为两侧剩余隐藏区保留分隔标记。未登录时回退的经典 `/commits/{commit}` 页面采用不同的 20 行行为，不是本工作区的设计基准。hunk 间剩余上下文不超过一页时，双向控件切换为单个“全部展开”，补齐最后上下文后移除已失效的 hunk 分隔行。文件级展开全部补齐当前文件全部隐藏区间，收起则恢复保留的原始 diff。
7. 返回 Workflow 或切换项目会使审核/Conversation/Commit/高亮/源码请求序列失效，迟到任务不得影响新上下文。

### 项目保存与 Provider 查询

1. Project 页面把本地仓库和远程仓库字段提交给 Rust callback。
2. `src/project/mod.rs` 校验本地字段，并为每次项目保存启动一个共享 runtime 查询任务。
3. `src/git/` 解析 `remote_repo`，使用 `src/codereview/api/` 的常量构建 Provider 请求，再通过 `src/http/mod.rs` 执行。
4. 只有查询成功才会 dispatch 带 `repo_project_id`、`repo_type` 的强类型 Project 新增/更新；`PersistentStore` 在 event loop 外写入 `projects.json`。
5. 查询或持久化失败时只显示一次项目模块错误 Toast，不保留不完整记录；成功提示只在 durable 写入完成后显示。

### 项目注册表刷新

1. 用户主动刷新时，在 Slint event loop 设置 Project 页面加载状态；加载中的
   IconButton 会阻止再次触发。
2. 共享应用 runtime 把 `load_projects` 放入 blocking task。
3. 完成结果通过 `slint::invoke_from_event_loop` 回写。
4. 成功时替换 Project model 并清除行内错误；失败时结束加载、显示可恢复的
   行内重试状态，同时保持导航响应。

### 项目终端

1. Project 卡片操作或 Workflow FAB 触发 Terminal callback。
2. Rust 解析 project key 和本地仓库路径。
3. `open_terminal_for_project` 决定复用还是重置。
4. PTY 会话在 UI 线程之外运行。
5. 渲染结果通过根 property 交给 `GitHubTerminalDrawer`。

### Chat 请求

1. `chat/interactive.rs` 追加并持久化用户消息。
2. 共享 runtime 使用配置的 HTTPS RAG endpoint（`[ai.chat].rag_api_url`，其次为 `USHER_RAG_API_URL`）和稳定请求格式调用 `chat/chat.rs`。
3. `src/http/mod.rs` 依次执行请求拦截、reqwest 请求、状态校验、响应/错误拦截和 RAG JSON 解码。
4. 按模式保存的单调起始时刻驱动一个可见页面计时器；结果持久化实测毫秒耗时，
   并在 event loop 中转换为 Slint model。
5. 失败会作为 assistant 消息显示，因此该请求保持静默，避免重复 Toast。

### AI Agent Prompt 组装

1. AI Chat、AI Review 与 Review Ask 读取 `[ai.agent]`，并把所有内置指令委托给
   `src/ai/prompts.rs`；调用点不保留 Prompt 字面量。
2. Adapter 只解析一次内嵌 `src/ai/prompts.toml` catalog，拒绝不支持的 schema，并以
   strict undefined 模式渲染每个带版本条目。
3. 所选回复 locale 向 system 消息恰好注入一条语言指令；强类型上下文组装在 transport
   之前执行 20 条对话消息与 Review Ask 48 KiB diff 上限。
4. PR Prompt 上下文排除本地路径、远端/Provider URL、作者、更新时间、凭据及模型连接设置。
   日志只包含 Prompt id/version、实际 locale、消息数和上下文字节/截断汇总。
5. Review Session 聚合 chunk 请求 provenance；成功的 Review Ask assistant 消息保存单次
   provenance。旧记录安全回退为空字段。

### AI Review 回复投影

1. Worker 加载或完成 Review Session 时，把 Assistant Markdown 交给
   `src/agent_response/`；User 内容继续保留原始文本，并由 AI Review Surface 显示为
   内容自适应宽度、右对齐的中性气泡，不重复绘制身份栏。
2. 纯结构 parser、有界缓存和 `StyledText` 准备都在 Worker 上生成宽度无关 block 与
   不透明仓库 action token；event loop 只实例化 `ModelRc` 并执行最终 model commit。
3. Controller 还可以为显式 `virtualized` 内存实验把 Review 状态、finding 与
   conversation 投影为单一扁平 response-row model；视觉行只共享源 model handle，绝不
   复制 Markdown payload。代表性 A/B 未通过采用门槛，因此生产继续使用完整 retained
   list。
4. 点击回复标题会把原始 Markdown 提交给 Artifact Preview；点击仓库 token 时，先在
   当前 task 投影中查表，再由 Worker 相对该 task 的本地仓库解析路径。
5. 唯一的 Docked Preview host 复用 PR trailing slot，只临时隐藏而不清空 AI Review
   Surface；关闭 Preview 后恢复。Detached Preview 不参与该替换。
6. 关闭 AI 面板或离开 PR 路由时，清空回复投影、token 映射和仓库绑定；仅关闭 Docked
   的 PR 上下文 Preview；已经 Detached 的 Preview 和后台 Review 工作继续独立存在，
   当前 Ask 被取消，mount generation 会拒绝迟到 Worker commit。

### AI Agent 连通性测试

1. 设置页独立保存 OpenAI-compatible Base URL、API Key、模型名称和回复语言。
2. 用户触发测试后，共享 runtime 执行 `ai/agent.rs`。
3. 统一 HTTP 模块请求 Provider 的 `/models` endpoint，并通过 Toast 反馈结果。
4. 此流程不得改变 RAG Chat 的 endpoint、payload 或历史行为。

### 更新下载

设置页主动检查更新时，通过统一 Notification manager 展示已是最新版、发现新版本和检查失败三种结果。发现新版本后的操作复用既有安装 callback；网络传输细节只保留在日志中，不直接展示给用户。

持久化的 Release Channel 根据当前构建选择默认值：稳定构建默认 `stable`，预发布构建默认 `alpha`。检查更新会记录本次选中的精确版本；安装阶段解析同一 Release，不会静默跳到之后出现的新版本。

1. `self_update` 在 blocking task 中解析 Release 元数据。
2. 统一 HTTP 管线启动具有长超时的流式下载。
3. 数据块写入临时压缩包并上报进度。
4. 取消时删除未完成文件。
5. Update UI 已管理成功/失败通知，因此 HTTP 层保持静默。

## HTTP 边界

`src/http/mod.rs` 是唯一允许创建应用自有 reqwest client 的位置。它返回 reqwest request builder，不隐藏 query、JSON 和 header 配置；执行后返回项目自有 `HttpResponse`，JSON、文本、字节和流式 chunk 读取都留在统一超时与错误管线内。

每个请求都提供 `RequestOptions`：

- `operation` 用于稳定日志和用户提示上下文。
- `reject_http_errors` 默认开启，把非 2xx 响应转换为 `HttpError`，并保留受长度限制的响应正文摘要。
- `feedback` 默认 `Silent`；用户操作可以选择 Toast、Notification 或两者。
- `timeout` 默认 30 秒，可传入自定义时长；零表示无总截止时间，独立的 10 秒连接超时仍然生效。
- `read_idle_timeout` 默认 30 秒，可自定义，零表示关闭空闲检测；每个收到的响应体传输
  chunk 都会重新计时。增量调用方使用 `HttpResponse::for_each_chunk`，与缓冲读取共享同一套
  诊断、错误拦截和 idle 语义。
- `retry_transient(3)` 为幂等 Provider 读取启用带 jitter 的有上限指数退避，覆盖连接/
  超时、429、5xx 及 rate-limit 耗尽的 403；识别有界 `Retry-After`/Provider reset 提示，
  中间尝试保持静默。独立读取使用 `join!`，逐项补全使用限并发 unordered stream。

Interceptor 按注册顺序执行。Request hook 可修改 header；Response hook 可读取状态、header 和耗时；Error hook 接收统一错误。详见 [specs/http.md](specs/http.md)。

## UI 组合规则

- macOS 侧栏切换由 AppKit 主线程直接重定向 Slint 动画目标，不再排队跨
  event-loop 回调；所有平台都即时更新同一个动画目标，运动期间页面宽度
  保持冻结，并且只在最终稳定后写入配置。
- 应用快捷键统一经过根捕获边界和 `src/shortcuts/`；原生标题栏按钮调用同一组
  callback，并从注册表读取展示 accelerator。
- Rust-facing property 和 callback 统一在 `ui/main.slint` 声明并保持命名一致。
- Feature view 负责布局；Rust 负责 IO 和业务状态转换。
- `ui/views/` 下的路由文件只声明 `GitHub*Page` 页面编排；视觉子组件统一放入
  `ui/components/<feature>/`；实现 GitHub/Primer 模式的组件使用 `GitHub*`
  名称，其他领域组件保持领域命名。
- 优先复用 `GitHubPageScrollActions`、`GitHubProjectCardActions`、
  `GitHubIconButton`、`GitHubPageHeader`、`GitHubScrollView`、
  `GitHubBlankslate`、`GitHubActionMenu`、`GitHubDialog`、Banner 反馈和
  项目终端入口。
- Hover 关键节点应常驻，通过 opacity/state 控制，不要条件创建命中区域。
- 全窗口 Modal 必须挂在根层。
- 滚动和窗口缩放期间避免重建 layout/model，精确计算应延迟到交互稳定后。

## 验证要求

Rust 或 Slint 改动至少运行：

```sh
./scripts/check-ui-policy.sh
cargo fmt --all
cargo check --locked
cargo test --lib --locked
git diff --check
```

Terminal viewport/session、更新取消、托盘关闭和路由布局需要做针对性手工验证。完整测试目前包含一个无关的 logger doctest 失败，不能用这个已知问题掩盖新增失败。

## 已知技术债

- `src/logger/debug_logger.rs` 有一个无法编译的 doctest。
- `Route.UiComponents` 是由 `ui-components` 控制的长期维护开发演练场；旧
  Login/Tools/List/Detail/Notification/Toast/Dialog demo 路由已删除。
- 应用为专有/非开源软件，见根目录 `LICENSE.txt`；第三方 vendored 组件仍保留各自许可证。
- `docs/terminal_viewport_iteration_notes.md` 是历史证据，不是当前规格。
