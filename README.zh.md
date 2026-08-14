# Usher

[English](README.md) · [架构文档](docs/architecture.zh.md) · [规格索引](docs/specs/README.md)

Usher 是一个处于 Alpha 阶段的桌面 Git 项目工作台，用于管理本地项目以及重复性的交付流程。它把项目元数据、PR/MR 概览、Branch/Tag/PR/CD 引导流程、内置终端、聊天助手、应用更新和桌面通知整合到一个 Slint 应用中。

## 主要功能

- 管理本地仓库、别名、标签、Logo 和远程仓库地址。
- 统计 GitHub、GitLab 和 Gitee 兼容服务上 `fat`、`uat`、`prod` 分支的 PR/MR 数量。
- 按项目隔离执行 Branch 校验、Tag 自动化、分阶段 CD 与 Pull Request 流程。
- 在当前页面打开项目感知的终端抽屉。
- 配置独立的 OpenAI-compatible AI Agent Provider（如 DeepSeek）；既有 RAG Chat 页面保持独立。
- 提供可持久化应用内消息与操作系统原生通知，以及 Toast、托盘、更新和网络状态反馈。
- 支持亮色、暗色和跟随系统主题。
- 通过统一的 reqwest HTTP 拦截管线发送应用网络请求。

> Usher 正在持续开发；部分次要路由属于开发和演示界面，并非完整的产品功能。

## 支持平台

当前维护的桌面目标为：

- macOS：包含原生标题栏、托盘、App Bundle 和 DMG 支持。
- Windows：包含 Release 打包支持。

Linux 在具备合适 Slint 后端的环境中可能可以构建，但目前不在 Release 工作流覆盖范围内。原模板中的 WebAssembly 和 Android 说明已移除，因为当前桌面依赖和平台集成并未维护这些目标。

## 环境要求

- 支持 Rust 2024 edition 的当前稳定版工具链。
- `PATH` 中可用的 Git，用于工作流和仓库操作。
- Slint/winit 所需的平台构建依赖。
- 可选：macOS 打包需要 `cargo-bundle` 和 `create-dmg`。

生产构建只启用 FemtoVG，以固定渲染器选择并避免在常规应用中链接 Skia 或 Slint 实时预览支持。Skia 和实时预览均保留为显式开发选项。

## 快速开始

```sh
git clone https://github.com/tianlai10000000/usher.git
cd usher
cargo run
```

常用开发命令：

```sh
./scripts/check-ui-policy.sh
cargo fmt --all
cargo check --locked
cargo test --lib --locked
git diff --check
```

日常 `dev` 和 `test` 构建保留源码行号与增量编译，但不再使用 macOS 默认的
unpacked 调试对象。这样既能让 panic backtrace 保留文件与行号，也能避免松散
`.rcgu.o` 长期占据 `target/`。确实需要 LLDB 查看完整变量和类型时，使用隔离的
重量级 profile：

```sh
cargo build --profile dev-full
```

IDE 后台诊断使用紧凑、无增量缓存的独立 profile，避免 workspace 全目标检查
重复制造日常开发缓存。VS Code/Cursor 已自动使用该配置；命令行等价检查为：

```sh
cargo ide-check
```

从旧策略升级后，如果现有 `target/` 仍包含历史 unpacked 对象，可执行一次
`cargo clean`。实测取舍和清理建议见
[构建性能与产物策略](docs/build-performance.md)。

应用内 Slint 实时预览仅用于开发，常规构建和 Release 构建默认不包含它。UI 快速
校验和可视化迭代统一使用项目脚本：

```sh
./scripts/ui-check.sh
./scripts/ui-live-preview.sh
```

PowerShell 对应入口为 `scripts/ui-check.ps1` 和 `scripts/ui-live-preview.ps1`。
`ui-check` 不启动应用，只校验完整根 UI；live preview 保留应用 callback 和状态，
但在运行时解释 UI 改动。交付前仍需使用普通 `cargo run --locked` 验证静态构建和
受影响路由/布局。重复热重载可能保留较多诊断状态，长时间 UI 迭代期间应定期重启
preview 进程。在 macOS 上，该显式开发路径使用
`vendor/i-slint-live-preview` 中有文档记录的 Slint 文件监听补丁；默认构建和
Release 构建不会选择它。

UI Components 演练场同样只用于开发。普通 `cargo run` 和现有 Release 脚本不会
显示其菜单入口，也不会把该页面纳入编译图。上述 live preview 脚本会自动启用它；
等价的直接命令为：

```sh
SLINT_LIVE_PREVIEW=1 cargo run --features live-preview,ui-components
```

演练场中的 **Artifact preview fixtures** 区域可通过真实生产 Preview Registry 与
Host 打开 Markdown、PNG、纯文本、JSON 和不支持二进制样例。`ui-components` 会
自动启用 fixture bridge；普通构建和 Release 构建不会编译这些样例，也不会显示
相应入口。

如需在不依赖 Provider 响应的情况下检查 GitHub 风格 PR 列表评审状态，可使用下面
的显式固定 fixture 启动应用，再打开任意已配置项目。它覆盖已批准、需要评审、
请求修改、无状态、长标题和长分支；普通启动绝不会加载这些记录。

```sh
./tests/run-pr-list-review-status-fixture.sh
```

完整的 `cargo test --locked` 当前会暴露 `src/logger/debug_logger.rs` 中已有的 doctest 问题；库单元测试本身可以通过。

验证可选的 Skia 渲染路径：

```sh
SLINT_BACKEND=winit-skia cargo run --no-default-features --features renderer-skia
```

对比实验性的 FemtoVG/WGPU 路径（不是生产默认配置）：

```sh
SLINT_BACKEND=winit-femtovg-wgpu cargo run --no-default-features --features renderer-femtovg-wgpu
```

Debug 构建会通过现有 Usher 日志每 5 秒记录一次进程内存，并记录生命周期检查点。Release 构建可通过以下方式启用同样的诊断：

```sh
USHER_MEMORY_DIAGNOSTICS=1 ./usher
```

筛选 `[Memory][Diag]`、`[Memory][UI]` 和 `[Workflow][PR Review][Memory]`，可以把 RSS/物理 footprint 变化与窗口尺寸、PR model/cache 归属对应起来。

在 macOS 上，可运行以下可重复的双循环 Release 基准：

```sh
scripts/benchmark-memory.sh --runs 3 --scenarios home --fixture home-v1
```

该命令会构建选定的 Release 渲染器，每轮启动全新进程，执行固定窗口尺寸/全屏阶段，并把原始 TSV、汇总与应用日志写入已忽略的 `memory-benchmark-results/`。窗口自动化需要辅助功能权限。固定 Workflow、PR Review、Chat 或 Terminal fixture 时使用 `--prepare-hook`；可执行 hook 会收到 `PID RUN SCENARIO`。渲染器与采样选项见 `--help`。

## 配置和本地数据

Usher 使用 `directories::ProjectDirs::from("com", "usher", "app")` 选择各平台的原生目录。

| 数据 | 相对位置 |
| --- | --- |
| 设置、Git 平台 Token/API 根地址和 AI Agent 凭据 | 配置目录：`settings.toml` |
| 项目列表 | 本地数据目录：`project/projects.json` |
| 聊天历史 | 本地数据目录：`im/chat_history.jsonl` |
| 持久通知 Inbox 与已读状态 | 本地数据目录：`messages/messages.json` |
| 日志 | 本地数据目录：`logs/<date>/usher.log` |
| 远程项目 Logo 缓存 | 临时目录：`usher-logo-cache/` |

GitHub、GitLab 和 Gitee Token 可在设置页面管理。Gitee 公有版与企业版凭据独立保存，仓库 remote host 自动选择公有 v5 或已配置的企业 v8 API，两套 Token 不共用。**代码托管 API 配置**区块以 `[git.platform.<provider>]` 保存各平台企业版 API 地址和企业名称；可填写服务主机或完整 REST API 根地址（例如 `https://git.example.com` 或 `https://git.example.com/api/v4`），缺少版本路径时由对应 Provider 自动补齐。这些值只保存在本地，且仅在配置的 API 主机与项目 remote 主机匹配时使用。**AI Agent** 区块支持 OpenAI-compatible 的 Base URL、API Key 和模型名称，提供可扩展的 Provider 选择器、可编辑模型名和不产生生成费用的连通性测试。设置保存至 `[ai.agent]`。不要提交生成的设置、日志、聊天记录或凭据。详见 [AI Agent 配置规格](docs/specs/ai-agent.md)。

## 架构概览

```text
src/main.rs
  -> src/lib.rs                应用装配和 callback 绑定
     -> crates/usher-ui/       Slint 生成 API 边界
        -> ui/main.slint       根窗口和导出的 UI 契约
           -> ui/views/        页面级视图
           -> ui/components/   应用可复用组件
     -> src/app_runtime.rs     共享 Tokio runtime
     -> src/http/mod.rs        统一 HTTP 客户端和拦截器
     -> src/platform/          原生外观、标题栏、窗口交互和字体适配
     -> src/project/           项目、Logo、PR 概览
     -> src/jobs/              可复用 Job model 与应用级运行页投影
     -> src/workflow/          Branch / Tag / PR / CD 状态与执行
     -> src/terminal/          PTY 会话和终端渲染
     -> src/chat/              聊天请求、历史、文本投影、UI 绑定
     -> crates/usher-preview/ Provider 注册及 Markdown/图片/文本准备
     -> src/artifact_preview/ 通用右侧面板/独立窗口预览 Host
     -> crates/usher-messages/ 持久消息中心、系统投递与独立 Inbox UI
     -> crates/usher-search/   通用文本匹配、结果导航与视口揭示算法
     -> src/update/            更新检查、下载和安装
     -> src/{toast,notification,message_center,dialog}.rs
material-1.7.1-fixed/            内置 Slint v1.17.1 Material 库
```

模块职责、生命周期、数据流和扩展点参见 [docs/architecture.zh.md](docs/architecture.zh.md)。Coding agent 还必须遵守 [AGENTS.md](AGENTS.md)。

## HTTP 拦截管线

所有应用自有 HTTP 请求统一通过 `src/http/mod.rs`，该模块是在 reqwest 上进行封装，并没有重新实现 HTTP 客户端。它提供：

- 共享的异步客户端和阻塞客户端；
- 请求、响应和错误拦截器；
- 统一的 `HttpError` 分类和状态码处理；
- 每次请求可选静默、Toast、通知或两者同时提示；
- 保留 reqwest 的请求构建、JSON 解码和流式响应能力。

后台轮询必须保持静默；用户主动触发的请求可以选择 UI 错误反馈。详见 [docs/specs/http.md](docs/specs/http.md)。

## Release 打包

本地构建和验证 Release 产物：

```sh
./scripts/verify-release.sh
```

无法自动识别平台时显式传入 target：

```sh
./scripts/verify-release.sh x86_64-pc-windows-gnu
```

GitHub Release 工作流会生成 macOS 压缩包/DMG 和 Windows ZIP Bundle。

## 文档

- [中文架构文档](docs/architecture.zh.md)
- [English architecture](docs/architecture.md)
- [项目结构审计与重构路线图](docs/architecture-structure-audit.md)
- [Coding agent 指令](AGENTS.md)
- [规格索引](docs/specs/README.md)
- [GitHub/Primer UI 设计规范](docs/specs/ui-design.md)
- [GitHub/Primer UI 分批改造清单](docs/ui-github-migration-plan.md)
- [UI Bug 历史](bug-history.md)
- [Pull Request 工作流规格](docs/specs/pr-review.md)
- [终端行为规格](docs/specs/terminal.md)
- [构建性能与产物策略](docs/build-performance.md)
- [内存优化待办](docs/memory_optimization_backlog.zh.md)
- [Memory optimization backlog](docs/memory_optimization_backlog.md)
- [终端历史排障记录](docs/terminal_viewport_iteration_notes.md)

## 许可证

Usher 为**专有、非开源**软件。详见 [`LICENSE.txt`](LICENSE.txt)：版权所有，未经版权方书面许可不得使用、复制、修改或分发。

第三方组件仍保留各自许可证（例如 Material 见 `material-1.7.1-fixed/LICENSE.md`）。
