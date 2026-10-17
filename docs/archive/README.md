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
| Release artifact 配额 | [2026-10-16 调查与保留策略](maintenance/release-artifact-quota-20261016.md)；当前契约见 [Release CI](../specs/release-ci.md) |
| Release 成功运行 | [2026-10-16 阶段、缓存与编译瓶颈审计](maintenance/release-ci-success-audit-20261016.md)；观察结果不是受控性能 A/B |
| Release fingerprint | [2026-10-16 诊断与严格缓存探针](maintenance/release-ci-fingerprint-20261016.md)；本地验证与尚未执行的云端范围分别记录 |
| MainWindow PR 编译拆分 / PR compilation split | [2026-10-16 十组编译对照与回归](maintenance/mainwindow-build-split-20261016.md)；编译收益确认，运行时门禁失败，B3 拒绝进入新基线 / compile gains measured, runtime gates failed, candidate rejected |
| PR 运行时后续 / PR runtime follow-up | [2026-10-17 归因与有界高亮对照](maintenance/pr-review-runtime-20261017.md)；B4 拒绝，B5 候选门槛与限定 macOS 原生检查通过；联合 strict 保留 12 项原 A 失败，保留 B5 候选但不晋升整批基线 / B4 rejected, B5 scoped criteria/native checks pass, joint strict retains A-only failures, no whole-batch baseline promotion |
| B5 完整 P6 / Full P6 and soak | [2026-10-17 完整回归与长时失败](maintenance/b5-full-p6-20261017.md)；40/40 完成关闭，六组短对照及另外三组 soak 通过，15 分钟 PR RSS 增长/斜率失败，B5 仍未采用；旧 2× 与新 1× 批次不混用 / complete, paired fixtures and three other soaks pass, PR sustained growth/slope fail, no adoption |
| B6 PR soak 归因 / Investigation | [2026-10-17 语法资源生命周期对照](maintenance/b6-pr-soak-20261017.md)；2 次长时及 6 次原 A/B6 短对照完成，B6 RSS 增长/斜率通过，但最终 footprint 增量 8.73 MiB 超过 8 MiB，拒绝 B6；完整 P6 未执行，原 A/B5 历史保留 / two sustained and six original-A/B6 paired runs complete; B6 RSS growth/slope pass, final footprint delta 8.73 MiB exceeds 8 MiB, rejected; full P6 not run, original A/B5 history preserved |
| B7 PR 空闲资源 / Idle resources | [2026-10-17 合并空闲检查与后台释放](maintenance/b7-pr-idle-cache-20261017.md)；40/40 完成且证据完整性通过，六组通用/相对短对照及 PR 门槛通过，Hook soak 69 footprint 斜率 0.979011 >0.5 失败；strict 原 A12/B7 0/global0 独立保留，B7/整批未采用；原 A 基线、Release 与门槛不变 / complete; candidate PR passes, Hook footprint slope fails, historical A-only strict failures preserved, no baseline promotion |
| 2026-10-14 维护 | [安全批次 1–5](maintenance/security-maintenance-20261014.md) · [结构批次 6](maintenance/structure-cleanup-20261014.md)；原始门禁输出已校验并本地归档，恢复索引见 [validation/（归档：docs/validation/）](../local-archives.md#validation-tracked-history-20261016) |

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
