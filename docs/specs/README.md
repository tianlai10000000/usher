# Specifications / 规格索引

Specifications describe stable behavior and invariants. Architecture documents explain ownership and flow; historical notes explain how a problem was investigated. Do not mix those three roles.

Spec 描述稳定行为和不变量；架构文档解释职责与数据流；历史记录描述问题排查过程。三者不要混写。

| Spec | Status | Code owners |
| --- | --- | --- |
| [HTTP pipeline](http.md) | Current / 当前 | `src/http/mod.rs`, all HTTP callers |
| [AI Agent configuration](ai-agent.md) | Current / 当前 | `src/settings.rs`, `src/ai/agent.rs`, `ui/views/settings.slint` |
| [Chat behavior](chat.md) | Current / 当前 | `src/chat/`, `ui/views/chat.slint`, `ui/components/chat/`, `ui/main.slint` |
| [Project repository metadata](project-repository.md) | Current / 当前 | `src/project/mod.rs`, `src/git/`, `src/codereview/api/` |
| [Pull request workflow](pr-review.md) | Current / 当前 | `src/workflow/pr_list.rs`, `src/workflow/pr_review.rs`, `src/workflow/pr_review/`, `src/git/`, `ui/views/workflow/` |
| [Reusable job runs](jobs.md) | Current / 当前 | `src/jobs/`, `ui/views/job_run.slint`, `ui/components/jobs/`, feature controllers |
| [Tag workflow](tag.md) | Current / 当前 | `src/workflow/tag.rs`, `src/workflow/tag/`, `src/workflow/state.rs`, `ui/views/workflow/` |
| [Terminal behavior](terminal.md) | Current / 当前 | `src/terminal/`, `ui/components/drawer/terminal_drawer.slint`, `ui/main.slint` |
| [Application keyboard shortcuts](shortcuts.md) | Current / 当前 | `src/shortcuts/`, `ui/main.slint`, `src/platform/` |
| [Application status bar](status-bar.md) | Current / 当前 | `ui/components/status_bar.slint`, `ui/main.slint`, shortcut/notification/network/update state owners |
| [GitHub/Primer UI design](ui-design.md) | Current / 当前 | `ui/theme/github_theme.slint`, `ui/`, `ui/assets/icons/`, all UI contributors |

The UI design specification defines `GitHub*` component naming,
`GitHubTheme`/`GitHubMetrics` ownership, Primer layout and interaction
contracts, shared Button/IconButton/Link/Tooltip/CounterLabel/Copy/Spinner/form-control
foundations, shared shell/title-bar breadcrumb/PageHeader/ScrollView/Progress foundations,
UnderlineNav/SegmentedControl/Actions Job foundations,
Dialog/ActionMenu/Banner overlay contracts, the Material dependency boundary,
the mandatory UI Components playground/property-matrix contract,
repository-wide component placement (`ui/components/<feature>/`) and route-view
composition (`ui/views/`), and the per-batch acceptance gate. The
implementation sequence and current status live in
[`docs/ui-github-migration-plan.md`](../ui-github-migration-plan.md); defects
live in [`bug-history.md`](../../bug-history.md). / UI 设计规范定义
`GitHub*` 组件命名、`GitHubTheme`/`GitHubMetrics` 归属、Primer 布局与交互
契约、CounterLabel、Shell/标题栏面包屑/PageHeader/ScrollView/Progress、
UnderlineNav/SegmentedControl/Actions Job 基础、Dialog/ActionMenu/Banner
浮层契约、通用 Filter ActionMenu、Project 可读卡片宽度、Chat 历史流内操作、
PR 单一状态/窄屏 Tab 及 diff/文件树字体颜色保护约束、强制 UI Components 演练场/属性矩阵契约、
Material 依赖边界、仓库级组件目录
（`ui/components/<feature>/`）与路由页面编排（`ui/views/`）边界，以及逐批
验收门禁；实施顺序和当前状态见
[`docs/ui-github-migration-plan.md`](../ui-github-migration-plan.md)，缺陷记录见
[`bug-history.md`](../../bug-history.md)。

Documents that are not specs:

- `docs/architecture.md`, `docs/architecture.zh.md`: canonical architecture.
- `docs/architecture-structure-audit.md`: dated structure audit and refactoring roadmap.
- `docs/ARCHITECTURE_FOR_AI.md`: compatibility entry point for older agent prompts.
- `docs/memory_optimization_backlog.md`, `docs/memory_optimization_backlog.zh.md`: measured memory baseline and phased optimization backlog.
- `docs/ui-github-migration-plan.md`: staged UI implementation checklist and
  user-acceptance gate.
- `bug-history.md`: append-only migration defect ledger.
- `docs/terminal_viewport_iteration_notes.md`: historical terminal investigation log.

When adding a spec, include scope, invariants, ownership, failure behavior, and a regression checklist. Update this index in the same change.
