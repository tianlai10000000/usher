# Specifications / 规格索引

[Documentation / 文档导航](../README.md) · [Architecture / 架构](../architecture.zh.md) · [English architecture](../architecture.md) · [Historical archive / 历史归档](../archive/README.md)

Spec 描述稳定行为、失败边界和验收要求；架构描述职责与数据流；历史记录保存实验
和采用/否决证据。索引只负责导航，具体契约放在对应 spec 中。

Specifications define behavior, failure boundaries and acceptance. Architecture
owns module/data flow; archives own dated experiments and decisions. Keep
implementation details in their owning spec rather than appending them here.

## 按任务开始 / Start from the task

先读根 [AGENTS.md](../../AGENTS.md) 和相关 README/架构，再选择功能 spec，并按下表
补读横切契约。修改 UI 行为时同时定位 Rust 绑定与 Slint 声明；功能自己的迭代入口
和回归清单负责给出具体代码与检查范围。

Read the repository guide and relevant README/architecture, then the feature
spec and applicable shared contracts below. Trace both Rust bindings and Slint
API declarations before UI behavior changes.

| 修改任务 / Change | 补读契约 / Additional contracts |
| --- | --- |
| 任意 UI 或用户可见文案 / UI or user-facing copy | [UI design](ui-design.md), [i18n](i18n.md) |
| Tooltip、factory 行操作 / Tooltips or factory row actions | [Tooltips](tooltips.md), [UI design](ui-design.md), [i18n](i18n.md) |
| 多行文本、换行/行高测量 / Multiline text or shaping | [Text layout](text-layout.md); 可变行高另读 / variable rows also read [virtual scrolling](virtual-scrolling.md) |
| 跨页面/后台共享状态与持久化 / Shared state and persistence | [State store](state-store.md); 同时核对 feature facade / trace the feature facade |
| HTTP、凭据、重定向、诊断、更新文件、进程启动 / HTTP, credentials, diagnostics, update files or launching | [HTTP](http.md), [security boundaries](security-boundaries.md); 日志行为另读 / logging also reads [diagnostics](diagnostics.md) |
| Job/Hook 执行或输出 / Job or Hook execution/output | [Jobs](jobs.md), [Hooks](hooks.md), [state store](state-store.md); 日志布局另读 / log layout also reads [text layout](text-layout.md) and [virtual scrolling](virtual-scrolling.md) |
| 优化或资源生命周期 / Optimization or resource lifetime | 功能验收矩阵 / feature acceptance, [P6 protocol](../p6-regression.md), [memory backlog](../memory_optimization_backlog.zh.md) |

## 共享基础设施 / Shared infrastructure

下面是入口而非完整依赖清单。类型/方法、限额和验证状态以 spec 正文及当前代码为准。

These are starting points, not exhaustive dependency lists. API details, budgets
and verification limits belong in the linked spec and current implementation.

| Spec | 修改入口 / Code entry points |
| --- | --- |
| [Security boundaries / 安全边界](security-boundaries.md) | `src/git/locator.rs`, `src/git/gitlab.rs`, `src/http/`, `src/update/`, `src/platform/mod.rs`, `crates/usher-preview/src/file_source.rs` |
| [HTTP pipeline / HTTP 管线](http.md) | `src/http/mod.rs`, `src/settings/request_preferences.rs`, feature request callers |
| [State store / 跨页面状态](state-store.md) | `crates/usher-state-store/`, feature-owned facades and persistence workers |
| [Shared search / 搜索与定位](search.md) | `crates/usher-search/`, feature Find adapters; PR `find_bridge.rs`, `controller/diff_bindings.rs`, `PrDiffBridge` |
| [Internationalization / 国际化](i18n.md) | `i18n/`, `crates/usher-i18n/`, `src/i18n.rs`, locale callbacks and feature projections |
| [Diagnostics / 诊断日志](diagnostics.md) | `src/diagnostics.rs`, `src/diagnostics/archive.rs`, `src/logger/`, `src/memory_diagnostics.rs`, `src/settings.rs`, shutdown and Settings bridges |

## UI 与布局 / UI and layout

| Spec | 修改入口 / Code entry points |
| --- | --- |
| [GitHub/Primer UI design / 视觉与交互](ui-design.md) | `ui/theme/github_theme.slint`, `ui/components/`, `ui/views/`, `scripts/check-ui-policy.sh` |
| [Tooltips / Tooltip 工厂与 Portal](tooltips.md) | Shared Tooltip/Float/IconButton, factory-local action state and root portals |
| [Multiline text layout / 多行文本](text-layout.md) | Native `Text`/`TextInput`, documented `StyledText`/`TextEdit` exceptions, feature measurement adapters |
| [Exact virtual scrolling / 精确虚拟滚动](virtual-scrolling.md) | `crates/usher-virtual-list/`, `src/app_runtime/latest_compute.rs`, `ui/components/github_scroll_view.slint`, feature layout adapters |

UI design 的共享组件章节按按钮、Shell、Tooltip/标签、状态/消息、菜单、导航、
滚动/Job、表单、Dialog、反馈、功能编排、演练场和 Escape 分组。专项契约以本表
对应 spec 为准，避免把局部迭代摘要复制到索引或架构首部。

The shared component chapter is grouped by responsibility. Follow the specialized
contracts here rather than duplicating local iteration summaries in navigation.

## 项目与交付流程 / Projects and delivery

| Spec | 修改入口 / Code entry points |
| --- | --- |
| [Project repository / 项目与仓库元数据](project-repository.md) | `src/project/`, `ui/features/project/bridge.slint`, `src/git/`, `src/codereview/api/` |
| [PR workflow / PR 读取与评审](pr-review.md) | `src/workflow/pr_data.rs`, `pr_list.rs`, `pr_review/`, `src/git/`, `ui/features/pr_review/`, `ui/components/pr_review/` |
| [PR conflicts / 只读冲突工作区](pr-conflicts.md) | `src/git/conflicts/`, `src/workflow/pr_conflict_status.rs`, `pr_conflict_status_owner.rs`, `src/workflow/pr_review/conflicts/` |
| [Tag workflow / Tag 流程](tag.md) | `src/workflow/tag.rs`, `src/workflow/tag/`, `src/workflow/state.rs`, `src/workflow/state/`, `ui/components/workflow/` |
| [Reusable jobs / Job 运行页与日志](jobs.md) | `src/jobs/`, `ui/views/job_run.slint`, `ui/components/jobs/`, feature controllers |
| [Application Hooks / 应用 Hook](hooks.md) | `src/hooks/`, project selection, Settings, PR/AI completion sources and shutdown drains |

## AI、Chat 与预览 / AI, Chat and preview

| Spec | 修改入口 / Code entry points |
| --- | --- |
| [AI Agent configuration / Agent 配置](ai-agent.md) | `src/ai/agent.rs`, `src/ai/settings_state.rs`, `src/ai/settings_ui.rs`, `src/settings.rs`, Settings bridge |
| [AI Code Review / AI 评审与 Ask](ai-code-review.md) | `crates/usher-ai-review/`, `src/ai_review/`, `src/agent_response/`, PR and Settings bridges |
| [Chat / 对话与历史](chat.md) | `src/chat/interactive.rs`, `viewport_policy.rs`, `message_model.rs`, `renderer.rs`, `ui/views/chat.slint` |
| [Artifact preview / 通用预览](artifact-preview.md) | `crates/usher-preview/`, `src/artifact_preview/`, `ui/components/artifact_preview/`, preview callers and Settings |

## 桌面 Shell 与消息 / Desktop shell and messages

| Spec | 修改入口 / Code entry points |
| --- | --- |
| [Terminal / 项目终端](terminal.md) | `src/terminal/`, `ui/components/drawer/terminal_drawer.slint`, root drawer callbacks |
| [Keyboard shortcuts / 应用快捷键](shortcuts.md) | `src/shortcuts/`, root `FocusScope`, `src/platform/` |
| [Status bar / 状态栏](status-bar.md) | `ui/components/status_bar.slint`, `src/status_bar_tests.rs`, `src/update/download_status.rs`, `ui/main.slint`, shortcut/message/network/update owners |
| [Tray / 托盘](tray.md) | `src/tray.rs`, `src/tray/animation.rs`, `src/tray/locale_fixture.rs`, root locale/shutdown callbacks |
| [Message center / 消息中心](messages.md) | `crates/usher-messages/`, `src/message_center.rs`, `src/message_center/` command/focus owners, `src/notification.rs`, `src/notification/ingress.rs`, Inbox/status UI |

## 实现、缺口与验收 / Implementation and acceptance

“不变量/必须”是要保持的契约，不等于每个调用方已通过验收。已发现的实现违约应在
功能 spec 中明确写成缺口，保留约束；不要改写为允许错误行为。“测试专用”“候选”
和“待原生验收”也不能读成生产默认或已通过。

Invariants are requirements, not a claim that every caller passes. Record verified
implementation gaps explicitly without weakening the contract. Test-only paths,
unadopted candidates and pending native acceptance are separate from production.

需要特别区分的当前边界：

- [AI Review](ai-code-review.md)：生产 Session 事务与仅测试的聚合 drain；
  [PR Review](pr-review.md)：仅离线候选的派生展示释放。
- [PR conflicts](pr-conflicts.md)：当前只读；后续编辑/提交见
  [iteration plan](../pr-conflicts-iterations.md)。
- [Tray](tray.md)：原生 locale 夹具已有证据，macOS 点击矩阵和 Windows 原生验收仍待完成；
  [i18n](i18n.md)：全路由视觉验收仍待完成。
- [Chat](chat.md)、[state store](state-store.md)：旧文本兼容与底层 API 边界详见正文；
  不应把兼容机制当作通用语义 Schema 或 checked close。

See those specs for the current production/test/candidate boundaries and gaps.
The source-audit scope and executed checks are recorded in
[2026-10-14 spec audit](../validation/spec-audit-20261014/README.md); this is not
native UI or performance acceptance.

Preview 文件增长预算与 AI Agent/Inbox/Banner 的 UI 同步 reducer 缺口已按
[2026-10-15 修复记录](../validation/spec-gap-fixes-20261015/README.md)处理；
其余审计行及消息最终关闭的修复见
[后续修复记录](../validation/spec-gap-followups-20261015/README.md)。
UI-120 消息分页的宿主焦点修复及 macOS 原生前后对照见
[专项回归记录](../validation/ui120-macos-focus-20261015/README.md)；旧侧栏焦点问题已修复，
中英文鼠标/正向键盘分页、禁用描边清理、放大还原、快捷键和重入已做 macOS 原生回归。
原生反向遍历受输入工具限制，Windows 和读屏仍待专项检查。应用最小窗口为
800 × 600px，强制 640px fallback 只用于 compiled 组件边界验证。
完整平台/视觉验收仍按各自 spec 保留，不以单测代替。
The follow-ups record code fixes and actual regression scope. UI-120 has native
macOS evidence for the reproduced failure and corrected pointer/forward-keyboard
behavior, disabled-direction outlines, zoom/restore, shortcuts and reentry;
physical reverse traversal and other native acceptance limits remain explicit.

## 维护方式 / Maintaining a spec

新增 spec 至少包含：范围/非目标、代码与 UI 所有者、稳定不变量、失败/取消/生命周期、
迭代入口和回归清单。较长 spec 使用职责明确的二/三级标题；具体 API 示例必须与
当前签名一致，测试入口应注明 feature/platform 限制。

A new spec needs scope/non-goals, code/UI ownership, invariants,
failure/cancellation/lifecycle behavior, iteration entry points and regression
checks. Use searchable responsibility headings and accurate API signatures;
state feature/platform limits on test commands.

更新时核对相关实现与测试，明确区分文档漂移和实现违约。行为/归属变化同步中英文
架构，稳定契约变化同步本索引；检查链接和代码路径。验收清单不能写成未执行的
“通过”，也不要给整篇文档添加无法证明的“全部最新”标记。

Verify referenced code/tests, distinguish documentation drift from code violations,
and update both architecture languages when behavior/ownership changes. Keep
this index synchronized when contracts change. Do not turn unexecuted checklists
into passed results or mark an entire document current from a partial audit.

构建命令见 [build policy](../build-performance.md)，优化任务见
[memory backlog](../memory_optimization_backlog.zh.md)，UI 实施批次见
[migration plan](../ui-github-migration-plan.md)，缺陷登记见
[bug-history](../../bug-history.md)，历史证据和本地大型归档见
[archive index](../archive/README.md) 与 [local archives](../local-archives.md)。
这些文档提供计划或证据，不替代当前 spec。
