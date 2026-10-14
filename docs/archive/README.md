# Historical archive / 历史归档

这里保留实验、设计取舍和验收时的原始结果。记录中的路径、版本、数字、失败与
未测范围按当时语境理解；采用或否决结论没有因整理而改变。当前入口见
[文档导航](../README.md)，行为契约见 [规格索引](../specs/README.md)。

These records preserve historical measurements, design decisions, failures and
untested scope. Organization does not change adoption decisions or turn an old
baseline into the current one. Use the documentation and specification indexes
for current development.

## 优化批次 / Optimization batches

完整的执行顺序与状态仍见根目录 [optimizing-proposal.md](../../optimizing-proposal.md)。
[P6 回归协议](../p6-regression.md) 仍是当前验证入口。

| 阶段 / Phase | 历史记录 / Records |
| --- | --- |
| P5 | [A](optimization/p5-a-performance.md) · [B](optimization/p5-b-performance.md) · [C](optimization/p5-c-performance.md) · [D](optimization/p5-d-performance.md) |
| P6 #64–66 | [Quality](optimization/p6-64-quality.md) · [Terminal](optimization/p6-65-terminal.md) · [Correctness](optimization/p6-66-correctness.md) · [Highlight](optimization/p6-66-highlight.md) |
| P6 #67–72 | [HTTP](optimization/p6-67-http.md) · [Hooks journal](optimization/p6-68-hooks-journal.md) · [Hooks state](optimization/p6-69-hooks-state.md) · [Job retention](optimization/p6-70-job-retention.md) · [Hook semantics](optimization/p6-71-hook-semantics.md) · [Regression](optimization/p6-72-regression.md) |
| P7 #73–79 | [Quality](optimization/p7-73-quality.md) · [Persistence](optimization/p7-74-persistence.md) · [Messages](optimization/p7-75-message-center.md) · [Session commits](optimization/p7-76-session-commits.md) · [Shutdown](optimization/p7-77-shutdown.md) · [Provider budgets](optimization/p7-78-provider-budgets.md) · [Conflict owner](optimization/p7-79-conflict-owner.md) |
| P7 #80–86 | [Chat index](optimization/p7-80-chat-index.md) · [Full source](optimization/p7-82-full-source.md) · [Session journal](optimization/p7-84-session-journal.md) · [Commit i18n](optimization/p7-85-commit-i18n.md) · [Owner structure](optimization/p7-86-owner-structure.md) |

## 内存与交互 / Memory and interaction

| 类别 / Category | 历史记录 / Records |
| --- | --- |
| Settings / Hook | [Settings Tooltip 09-24](memory/settings-tooltip-memory-20260924.md) · [Hook editor 09-26](memory/hook-editor-memory-20260926.md) |
| 09-27–28 批次 A–E | [PR Review A](memory/pr-review-memory-batch-a-20260927.md) · [Settings B](memory/settings-memory-batch-b-20260928.md) · [Settings C](memory/settings-memory-batch-c-attribution-20260928.md) · [Terminal D](memory/terminal-memory-batch-d-attribution-20260928.md) · [Project list E](memory/project-list-memory-batch-e-20260928.md) |
| MEM-14–16 | [English](memory-experiments.md) · [中文](memory-experiments.zh.md)；活跃任务仍见 [内存待办](../memory_optimization_backlog.zh.md) |
| 交互性能 | [侧栏 A/B](performance/sidebar_motion_ab.md) · [Chat](performance/chat-interaction-performance.md) · [Artifact preview](performance/artifact-preview-performance.md) |
| 大型本地证据 | [恢复说明](../local-archives.md)；归档内部保留原始相对路径 |

## 构建、结构与排查 / Build, structure and investigations

| 类别 / Category | 历史记录 / Records |
| --- | --- |
| 构建 A/B 与旧路线 | [build-performance.md](build-performance.md)；当前策略见 [主文档](../build-performance.md) |
| 结构审计 | [architecture-structure-audit.md](architecture-structure-audit.md) |
| Tooltip 取舍 | [tooltip-design.md](tooltip-design.md)；当前契约见 [tooltips](../specs/tooltips.md) |
| Terminal 排查 | [terminal_viewport_iteration_notes.md](terminal_viewport_iteration_notes.md) |
| AI Review 超时 | [ai-review-response-timeout-investigation.md](ai-review-response-timeout-investigation.md) |
| Workflow 回滚 | [workflow-optimization-rollback.md](workflow-optimization-rollback.md) |
| 2026-10-14 维护 | [安全批次 1–5](maintenance/security-maintenance-20261014.md) · [结构批次 6](maintenance/structure-cleanup-20261014.md)；原始门禁输出保留在 [validation/](../validation/) |

## UI 与 PR 冲突 / UI and PR conflicts

| 类别 / Category | 历史记录 / Records |
| --- | --- |
| UI 已完成批次 | [迁移批次 1–7](ui-github-migration.md)；后续任务见 [迁移计划](../ui-github-migration-plan.md) |
| 冲突设计与研究 | [design.zh.md](pr-conflicts/design.zh.md) · [视觉设计入口](../designs/pr-conflicts/README.md) · [旧设计图](pr-conflicts-20261014/) |
| 冲突四轮验收 | [validation.zh.md](pr-conflicts/validation.zh.md)，按原阶段分节；[截图](pr-conflicts/screenshots/) 保留修正前后状态 |
| PR 列表排版 | [pr-list-layout-validation.zh.md](ui/pr-list-layout-validation.zh.md) |
| Tab 溢出 | [tab-overflow-validation.md](ui/tab-overflow-validation.md) |

整理仅移除无内容的 `ui-check.txt` 和未被引用的 `preview.gif`、`spinner-en-b.png`。
其余设计和验收图片保留，移动后的 Markdown 链接同步修复。原路径
`docs/terminal_viewport_iteration_notes.md` 保留短入口，供既有预览示例和旧链接使用。
