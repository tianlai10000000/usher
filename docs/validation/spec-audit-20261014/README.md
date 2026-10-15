# Spec source audit / 规格源码核对

本次整理以 coding agent 的迭代效率为目标。核对 25 份功能/横切 spec 的主要代码
入口、相关测试与 UI 声明，修正文档漂移并记录已确认实现缺口。没有修改 Rust、Slint、
依赖或运行时行为；这份记录不代表逐条不变量、线上 Provider、全路由原生 UI 或性能
已重新验收。

This audit checks source/test entry points across 25 specs, fixes documentation
drift and records verified implementation gaps. It changes documentation only;
it does not certify every invariant, native platform, provider or performance path.

## 结构决策 / Organization

- [规格索引](../../specs/README.md) 改为任务→横切契约→功能入口的分类导航，移除
  重复实现摘要，补维护规则及实现/候选/验收状态的区分。
- 保留全部 spec 文件名和路径。UI design 的大段组件契约增加职责章节；PR、AI、
  Search、Hooks 的导航/生命周期/回放契约归入对应章节，不再藏在回归清单尾部。
- 为功能文档补代码与定向测试入口；保留平台、feature 与原生验收限制。
- 同步中英文架构，并从 `AGENTS.md` 的 HTTP 指引移除不存在的 `send_blocking`。
  应用级请求仍须通过共享异步管线与共享 Runtime。

Keep stable spec paths, group long contracts by responsibility and provide
source/test navigation. Requirements, current gaps, candidates and passed
acceptance remain distinct. No runtime or tooling optimization is claimed;
Release A/B was not run for this documentation-only change.

## 核对范围与修正 / Inspected scope and corrections

下表列出本次检查的主要路径；它不是每条 spec 的完整端到端证明。

| Specs | Inspected source/test boundary | Confirmed correction or boundary |
| --- | --- | --- |
| AI Agent / AI Review | `src/ai/{agent,settings_state,prompts}.rs`, Settings callbacks; `src/ai_review/` controller/session transactions/persistence/response; `crates/usher-ai-review/`; response benchmark script | 普通请求捕获可配置超时；五配置字段；PR Preview 的 Floating 清理；A/B 构建需 `test-fixtures`；生产事务和测试 drain 分开 |
| Chat | `src/chat/` transport/schema/renderer/timestamp/interactive/viewport/message model; `ui/views/chat.slint`, `tests/chat_history_ui.rs` | catalog 错误与 locale 时间；七类已知历史错误的精确 catalog 反查；历史分页位于 top spacer 之前 |
| Artifact preview | `crates/usher-preview/src/lib.rs` and provider modules; `src/artifact_preview/` repository-file/presentation/measurement/virtual layout/projection; `ui/main.slint` | 真实 `MaterializedSource` trait/body、来源/图片预算、package feature 边界；Unsupported 当前无外部打开操作；Docked/Floating 共享 PR slot |
| Search / virtual scrolling / text layout | `crates/usher-search/`, `crates/usher-virtual-list/src/lib.rs`, `src/app_runtime/latest_compute.rs`; PR Find/geometry/diff adapters; Preview/Agent Response paint and measurement | 异步适配章节；`VirtualWindow.content_extent` 与复用 `write_window`；LatestCompute 生产 acknowledge/take_output 与测试 helper 区分；保留行高例外 |
| Jobs / Hooks | `src/jobs/` model/presenter/log layout/viewport; `src/hooks/` service/state/runner/replay/bridge/projection/presentation/tests; business event emitters | Hooks 已是 Job 生产来源；三个项目级内置事件；occurrence ID 与事件名称区分；回放契约独立成章 |
| Project / PR / conflicts / Tag | Project registry/controller/storage/logo/projection/Hook selection; Git locator/provider/model/diff/conflict adapters; PR state/list/shared data/review actions/lifecycle/fixtures; Tag controller/version/commands/output and Slint panels | 旧 PR 表格列改行内元信息；八类请求代际含 Reveal；四导航入口及保留统计的 More；冲突 Back 释放快照，只保留数量；Tag Local Remove 仍访问远端 |
| HTTP / security | `src/http/mod.rs`, privacy/budget/redirect tests; Git locator/GitLab; update archive/restart and platform argv helpers; request preferences/UI | 无 `send_blocking`，错误类型补 `ResponseTooLarge`，生产使用本地化出口；补负例/平台验收入口 |
| State / messages | `crates/usher-state-store/src/{lib,latest}.rs`, `crates/usher-messages/src/`, `src/message_center.rs`, paging tests, notification/Inbox/Tooltip hosts | subscriber latest 合并；shutdown API 与准入边界；Tooltip 直接 I18n binding；Banner 菜单以按钮锚定 |
| Diagnostics / i18n | Settings/logger/diagnostics/memory/shutdown; nightly workflow; catalog build/facade/root locale callback and feature projections | schedule 已暂停；补文案变更流程；避免用 catalog 检查代替挂载文案/全路由验收 |
| Terminal / shortcuts / status / tray / tooltips / UI design | `src/terminal/` controller/io/session/retirement/tests; shortcut registry/resolver/dispatch; root UI and platform inputs; status UI/test; tray player/locale; shared/factory Tooltip; UI policy and component contracts | Terminal 由 Slint 逻辑几何报告整数网格；快捷键含 Project 与 Workflow；补职责章节，修正旧 PR TabButton 入口；保留托盘和 Tooltip 原生矩阵 |

## 已确认但未修代码的缺口 / Unfixed implementation gaps

以下是本次审计时的状态；Preview 读取预算与 UI 同步 reducer 两项后续修复见
[2026-10-15 修复与回归记录](../spec-gap-fixes-20261015/README.md)；其余审计行及消息最终关闭见
[后续修复记录](../spec-gap-followups-20261015/README.md)。
This table preserves the audit-time findings; the follow-up records subsequent fixes.

这些是后续代码迭代的入口，原有约束不因此放宽：

| Gap | Concrete owner / follow-up contract |
| --- | --- |
| 通用 File Preview 先 metadata 后 `fs::read`，增长可造成超预算临时分配 | `crates/usher-preview/src/lib.rs::materialize`; [Preview](../../specs/artifact-preview.md)。仓库 Markdown 已使用验证句柄/有界读取，不得退化 |
| 消息 hydration 无条件提交，包括纯磁盘加载和 revision 0 | `crates/usher-messages/src/center.rs::hydrate_from_storage`; [State store](../../specs/state-store.md), [Messages](../../specs/messages.md) |
| 消息 UI Action 和 AI 配置保存仍在 UI 线程同步 reduce | `src/message_center.rs::bind_page_callbacks`, `src/notification.rs` mirror publish, `src/settings_page.rs`, `src/ai/settings_state.rs`; [Messages](../../specs/messages.md), [AI Agent](../../specs/ai-agent.md) |
| 诊断导出成功后未 prune，同进程可暂时超过 5 份归档 | `src/diagnostics.rs::{setup,export_bundle}`; [Diagnostics](../../specs/diagnostics.md) |
| 状态栏已展开详情保留点击时译文/状态快照 | `ui/components/status_bar.slint` popup handlers; [Status bar](../../specs/status-bar.md), [i18n](../../specs/i18n.md) |
| Workflow legacy Mutex state、Tag 初始化无进程 timeout/token、description 草稿卸载丢失 | `src/workflow/state.rs`, `src/workflow/tag/controller.rs`, `ui/components/workflow/github_tag_add_panel.slint`; [Tag](../../specs/tag.md) |

Chat 的历史文本反查和 `PersistentStore::shutdown` 的 void/不关闭 Action API 属于
必须准确理解的现有边界；不能分别误报为七类错误完全不切换语言，或默认已经提供
checked shutdown。各 spec 中原有 Messages 生命周期/原生焦点、Preview、托盘、
i18n 全路由和安全平台未验收项保持明确，未作为本次“新修复”处理。

## 执行检查 / Executed checks

| Command/check | Result |
| --- | --- |
| `cargo fmt --all` | PASS, exit 0 |
| `cargo check --locked` | PASS, exit 0; default full root UI included, dev build 2m 30s |
| `cargo test --lib --locked` | PASS after allowing localhost fixtures: 833 passed, 0 failed, 2 ignored; exit 0 |
| `cargo test --doc --locked` | PASS: 1 passed, 0 failed, 1 ignored; exit 0 |
| `python3 -m unittest scripts.tests.test_architecture_checks scripts.tests.test_memory_benchmark_thresholds` | PASS, 12 tests (10 architecture + 2 threshold), 7.978s |
| Local Markdown links/anchors, explicit source paths, all 25 specs indexed | PASS; [navigation-check.txt](navigation-check.txt), 31 documents; staged deletion records checked separately |
| `git diff --check` | PASS, exit 0 |

第一轮沙箱库测试为 819 passed / 14 failed / 2 ignored（exit 101）。14 个失败全部在
本地监听夹具创建时发生 `Operation not permitted`；原始输出见
[cargo-test-lib.log](cargo-test-lib.log)。允许 localhost 夹具后重跑同一完整命令全部通过，
见 [cargo-test-lib-localhost.log](cargo-test-lib-localhost.log)。Doctest 输出见
[cargo-test-doc.log](cargo-test-doc.log)。未新增忽略或测试豁免。

本次未运行原生 UI、
线上 Provider、Windows 平台、Release 打包或性能 A/B；没有 UI/进程/性能代码变更。
单测与编译不能代替这些专项验收。

原有 staged 删除 `ui/assets/.DS_Store`、`ui/assets/fonts/.DS_Store`、
`ui/assets/fonts/Noto_Sans_SC/.DS_Store` 保持原样；未改写或取消暂存。
