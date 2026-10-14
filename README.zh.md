# Usher

[English](README.md) · [架构文档](docs/architecture.zh.md) · [规格索引](docs/specs/README.md)

Usher 是一个处于 Alpha 阶段的桌面 Git 项目工作台，用于管理本地项目以及重复性的交付流程。它把项目元数据、PR/MR 概览、Branch/Tag/PR/CD 引导流程、内置终端、聊天助手、应用更新和桌面通知整合到一个 Slint 应用中。

## 主要功能

- 管理本地仓库、别名、标签、Logo 和远程仓库地址。
- 统计 GitHub、GitLab 和 Gitee 兼容服务上 `fat`、`uat`、`prod` 分支的 PR/MR 数量。
- 按项目隔离执行 Branch 校验、Tag 自动化、分阶段 CD 与 Pull Request 流程。
- 在当前页面打开项目感知的终端抽屉。
- 在同一 Chat 页面使用 OpenAI-compatible AI 对话或隔离的 RAG 对话，两种模式历史独立。
- 按设置并发执行最多 8 个 AI Code Review，在 PR diff 右侧查看 finding，并以当前 PR 为上下文和已配置 AI Agent 对话。
- 提供可持久化应用内消息与操作系统原生通知，以及 Toast、托盘、更新和网络状态反馈。
- 支持亮色、暗色和跟随系统主题。
- 支持跟随系统或选择任意内置语言，并在运行时即时切换界面语言。
- 通过统一的 reqwest HTTP 拦截管线发送应用网络请求。

> Usher 正在持续开发；部分次要路由属于开发和演示界面，并非完整的产品功能。

## 支持平台

当前维护的桌面目标为：

- macOS：包含原生标题栏、托盘、App Bundle 和 DMG 支持。
- Windows：包含 Release 打包支持。

Linux 在具备合适 Slint 后端的环境中可能可以构建，但目前不在 Release 工作流覆盖范围内。原模板中的 WebAssembly 和 Android 说明已移除，因为当前桌面依赖和平台集成并未维护这些目标。

## 环境要求

- Rust 1.98.1 稳定版工具链，由 `rust-toolchain.toml` 固定，支持 Rust 2024 edition。
- `PATH` 中可用的 Git，用于工作流和仓库操作。
- Slint/winit 所需的平台构建依赖。
- 可选：macOS 打包需要 `cargo-bundle` v0.12.0 和 `create-dmg` v1.3.0；发布脚本会自动安装锁定版本的 `cargo-bundle`。

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

统一运行本地质量检查（不改写源码）：

```sh
./scripts/check-quality.sh          # 默认快速层；等同 --quick
./scripts/check-quality.sh --full   # workspace 与 feature 契约完整层
```

两层都检查格式、结构/UI/i18n、脚本测试及空白错误。快速层另执行默认 root check
和 root 库测试；完整层改为 workspace all-targets check、workspace 单元/集成测试、
所有 target 的 workspace Clippy（警告视为错误），以及排除可选 AI core 和
UI/Settings bridge 的根应用 all-targets check。完整层固定 `CARGO_INCREMENTAL=0`，
并为 workspace 测试启用 Slint debug metadata，与手动 CI 一致。两层均另行执行
根包 doctest，并将失败作为阻断项。

普通失败保留原退出码并继续后续检查，最终汇总返回非零；中断立即停止。需要
`cargo`、`rg`、`python3`、`git`；HTTP 夹具需要本地监听权限。已知结构和 lint
失败仍会阻断。macOS 手动 CI 复用完整入口，Windows 保留独立的默认 workspace
编译检查；不会启用托管 schedule，也不替代专项 UI/Release A/B 验收。原结构
职责方案见[第 64 批](docs/p6-64-quality.md)。

AI Code Review 是默认产品构建启用的可拆卸 Cargo feature。受限构建可排除其核心包和
UI/Settings bridge：

```sh
cargo check --no-default-features --features renderer-femtovg --locked
```

日常 `dev` 构建对第三方依赖和生成的 Slint UI crate 使用 opt-level 1，同时让应用
自有 Rust 保持 opt0。`dev` 与 `test` 仍保留源码行号和增量编译，但不再使用 macOS
默认的 unpacked 调试对象。这样既保留应用 panic backtrace 与源码断点，也降低普通
`cargo run` 的代码常驻。确实需要 LLDB 查看完整变量和类型时，使用隔离的重量级
profile：

```sh
cargo build --profile dev-full
```

日常需要优化构建时，使用隔离的快速 Release profile。它保留正式 Release 的 strip、
体积优化和 abort 策略，但改用 Thin LTO、8 个 codegen unit 与增量编译，以缩短后续
重建时间：

```sh
cargo build --profile release-fast --locked
cargo bundle --profile release-fast --format osx
```

该 profile 的产物写入 `target/release-fast`，发布自动化不会使用它。最终分发产物仍由
现有发布脚本通过 `--release` 构建。

IDE 后台诊断使用紧凑、无增量缓存的独立 profile，避免 workspace 全目标检查
重复制造日常开发缓存。VS Code/Cursor 已自动使用该配置；命令行等价检查为：

```sh
cargo ide-check
```

从旧策略升级后，如果现有 `target/` 仍包含历史 unpacked 对象，可执行一次
`cargo clean`。实测取舍和清理建议见
[构建性能与产物策略](docs/build-performance.md)。

修改内容不涉及 PR Review 详情页时，可使用显式开发 Shell。它使用独立 target
目录，并把完整 PR Review feature package 替换为协议兼容的空实现：

```sh
./scripts/dev-shell-check.sh
./scripts/dev-shell-run.sh
```

PowerShell 对应入口为 `scripts/dev-shell-check.ps1` 和
`scripts/dev-shell-run.ps1`。该模式会有意把进入 PR Review 的尝试回退到
Workflow。默认构建和所有现有 Release 命令仍会编译完整页面；涉及 PR Review 或
最终交付前，仍必须执行普通 `cargo check --locked` 和 `cargo run --locked`。

应用内 Slint 实时预览仅用于开发，常规构建和 Release 构建默认不包含它。

UI 快速校验要求安装版本一致的稳定版 viewer：

```sh
cargo install slint-viewer --version 1.18.1 --locked
```

UI 快速校验和可视化迭代统一使用项目脚本：

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

使用下面已有的 mock 命令检查 PR 列表布局、评审与冲突状态。启动后自动进入
离线测试列表，并使用独立临时状态目录，无需配置项目。36 条数据覆盖各种评审
与冲突状态、中文作者、长标题、长分支、缺失分支和分页；普通启动不会加载这些记录。

```sh
./tests/run-pr-list-review-status-fixture.sh
```

可选环境变量：`USHER_PR_LIST_WIDTH=800`、`USHER_PR_LIST_HEIGHT=600`、
`USHER_PR_LIST_LOCALE=zh-CN`（或 `en`）、`USHER_PR_LIST_PROVIDER=gitee`
（或 `github`/`gitlab`）。Provider 选项只切换 mock 展示，不代表线上接口验收。

使用交互式批准/合并离线 mock 检查详情操作与返回列表的状态：

```sh
./tests/run-pr-approval-state-fixture.sh --list --locale zh-CN
./tests/run-pr-approval-state-fixture.sh --detail --locale en
```

Open 包含待评审、已批准和请求修改；Closed 包含已合并和关闭未合并。Approve 后底层
mock 数据立即更新，隐藏列表保持原 model；两秒后返回列表显示新结论。批准不会合并；
可合并记录需要显式确认。该工具使用真实 Slint UI 和模拟 callback，不读取普通应用
配置，也不调用 Provider；共享数据及 Worker 顺序另由库回归测试验证。


使用 `cargo test --doc --locked` 验证根包文档示例。旧 logger 示例已修复，不再保留测试豁免。

验证可选的 Skia 渲染路径：

```sh
SLINT_BACKEND=winit-skia cargo run --no-default-features --features renderer-skia
```

对比实验性的 FemtoVG/WGPU 路径（不是生产默认配置）：

```sh
SLINT_BACKEND=winit-femtovg-wgpu cargo run --no-default-features --features renderer-femtovg-wgpu
```

Debug 构建默认每 5 秒记录一次进程内存并记录生命周期检查点，可在 **Settings → Diagnostics → Memory diagnostics** 中关闭。Release 基准可通过以下方式强制启用同样的诊断：

```sh
USHER_MEMORY_DIAGNOSTICS=1 ./usher
```

筛选 `[Memory][Diag]`、`[Memory][UI]` 和 `[Workflow][PR Review][Memory]`，可以把 RSS/物理 footprint 变化与窗口尺寸、PR model/cache 归属对应起来。

交互式排查时，打开 **Settings → Diagnostics**，根据复现问题手动开启最小必要的输出目标与诊断来源。`Terminal verbose diagnostics` 和 `Memory diagnostics` 只增加记录内容，仍需同时开启 `Console output` 和/或 `Application file logs` 才能看到这些记录。只有跨模块问题暂时无法定位时，才在最短复现期间使用 `Enable all`；采集完成后恢复原开关或选择 `Disable all`。应记录复现时间；Terminal verbose 开启期间不要输入或分享密钥。开关组合、文件位置与采集清单见[诊断日志规格](docs/specs/diagnostics.md)。

在 macOS 上，可运行以下可重复的双循环 Release 基准：

```sh
scripts/benchmark-memory.sh --runs 3 --scenarios home --fixture home-v1
```

该命令会构建选定的 Release 渲染器，每轮启动全新进程，执行固定窗口尺寸/全屏阶段，并把原始 TSV、汇总与应用日志写入已忽略的 `memory-benchmark-results/`。窗口自动化需要辅助功能权限。固定 Workflow、PR Review、Chat 或 Terminal fixture 时使用 `--prepare-hook`；可执行 hook 会收到 `PID RUN SCENARIO`。渲染器与采样选项见 `--help`。

为避免自动消耗托管 macOS runner，Nightly performance 的定时触发目前暂时暂停。
P6 Terminal/Hooks/Job 等六场景的按需回归与持续循环入口见
[P6 验证协议](docs/p6-regression.md)，包含独立 A/B、15 分钟真实循环和原始证据。
完整 workflow 仍可通过 `workflow_dispatch` 手动运行；需要恢复每日性能门禁时，取消
其中 `schedule` 配置的注释即可。

## 配置和本地数据

Usher 使用 `directories::ProjectDirs::from("com", "usher", "app")` 选择各平台的原生目录。

| 数据 | 相对位置 |
| --- | --- |
| 设置、Git 平台 Token/API 根地址和 AI Agent 凭据 | 配置目录：`settings.toml` |
| 项目列表 | 本地数据目录：`project/projects.json` |
| RAG / AI Chat 历史 | 本地数据目录：`im/chat_history.jsonl` / `im/ai_chat_history.jsonl` |
| AI Review 规则、默认值、项目绑定、并发数 | 配置目录：`ai-review.json` |
| AI Review 任务摘要与完成 Session | 本地数据目录：`ai-review/` |
| 持久通知 Inbox 与已读状态 | 本地数据目录：`messages/messages.json` |
| 日志 | 本地数据目录：`logs/<date>/usher.log` 和 `logs/<date>/usher_tracing.log` |
| 远程项目 Logo 缓存 | 临时目录：`usher-logo-cache/` |

GitHub、GitLab 和 Gitee Token 可在设置页面管理。Gitee 公有版与企业版凭据独立保存，仓库 remote host 自动选择公有 v5 或已配置的企业 v8 API，两套 Token 不共用。**代码托管 API 配置**区块以 `[git.platform.<provider>]` 保存各平台企业版 API 地址和企业名称；可填写服务主机或完整 REST API 根地址（例如 `https://git.example.com` 或 `https://git.example.com/api/v4`），缺少版本路径时由对应 Provider 自动补齐。这些值只保存在本地，且仅在配置的 API 主机与项目 remote 主机匹配时使用。**AI Agent** 区块支持 OpenAI-compatible 的 Base URL、API Key、模型名称和回复语言；该语言应用于 AI Chat、AI Code Review 与 Review Ask，`auto` 跟随当前界面语言，RAG 不受影响。**AI Review** 配置本地 Markdown 规则、默认规则和并发上限。配置分别保存到 `[ai.agent]` 和 `ai-review.json`。所有内置 Agent 指令统一位于受版本控制的 `src/ai/prompts.toml` catalog。不要提交生成的设置、日志、聊天记录、Review Session 或凭据。详见 [AI Agent 配置规格](docs/specs/ai-agent.md)和 [AI Code Review](docs/specs/ai-code-review.md)。

## 架构概览

```text
src/main.rs
  -> src/lib.rs                应用装配和 callback 绑定
     -> crates/usher-ui/       Slint 生成 API 边界
     -> crates/usher-i18n/     locale 注册表与 Rust/Slint 共用翻译 catalog
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
     -> src/chat/              AI/RAG 共用 Chat、隔离历史、UI 绑定
     -> crates/usher-ai-review/ 可选无 UI Review 领域和调度器
     -> src/ai_review/         可选 Review controller 与持久化
     -> crates/usher-preview/ Provider 注册及 Markdown/图片/文本准备
     -> src/artifact_preview/ 通用右侧面板/独立窗口预览 Host
     -> crates/usher-messages/ 持久消息中心、系统投递与独立 Inbox UI
     -> crates/usher-search/   通用文本匹配、结果导航与视口揭示算法
     -> src/update/            更新检查、下载和安装
     -> src/{toast,notification,message_center,dialog}.rs
material-1.18.1/            Slint v1.18.1 标签中的原版 Material 源码
```

模块职责、生命周期、数据流和扩展点参见 [docs/architecture.zh.md](docs/architecture.zh.md)。Coding agent 还必须遵守 [AGENTS.md](AGENTS.md)。

## HTTP 拦截管线

所有应用自有 HTTP 请求统一通过 `src/http/mod.rs`，该模块是在 reqwest 上进行封装，并没有重新实现 HTTP 客户端。它提供：

- 所有应用自有 HTTP 请求共用一个请求客户端；
- 请求、响应和错误拦截器；
- 统一的 `HttpError` 分类和状态码处理；
- 相互独立的单请求总超时和响应体空闲超时；
- 每次请求可选静默、Toast、通知或两者同时提示；
- 保留 reqwest 请求构建能力，并统一处理 JSON、文本、字节和流式响应体读取。

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
push `v*` tag 是唯一的自动构建触发器，只会启动 Release workflow；直接 push
`main` 或创建 Pull Request 都不会再启动第二次构建。需要额外验证时仍可通过
`workflow_dispatch` 手动运行 CI。

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

第三方组件仍保留各自许可证（例如 Material 见 `material-1.18.1/LICENSE.md`）。
