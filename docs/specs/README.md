# Specifications / 规格索引

Specifications describe stable behavior and invariants. Architecture documents explain ownership and flow; historical notes explain how a problem was investigated. Do not mix those three roles.

Spec 描述稳定行为和不变量；架构文档解释职责与数据流；历史记录描述问题排查过程。三者不要混写。

| Spec | Status | Code owners |
| --- | --- | --- |
| [HTTP pipeline](http.md) | Current / 当前 | `src/http/mod.rs`, all HTTP callers |
| [AI Agent configuration](ai-agent.md) | Current / 当前 | `src/settings.rs`, `src/ai/agent.rs`, `ui/views/settings.slint` |
| [Chat behavior](chat.md) | Current / 当前 | `src/chat/`, `ui/views/chat.slint`, `ui/main.slint` |
| [Project repository metadata](project-repository.md) | Current / 当前 | `src/project/mod.rs`, `src/git/`, `src/codereview/api/` |
| [Pull request workflow](pr-review.md) | Current / 当前 | `src/workflow/pr_list.rs`, `src/workflow/pr_review.rs`, `src/git/`, `src/project/pr_stats.rs`, `ui/views/workflow/` |
| [Terminal behavior](terminal.md) | Current / 当前 | `src/terminal/`, `ui/components/drawer/terminal_drawer.slint`, `ui/main.slint` |

The PR workflow specification includes the reversible GitHub-style expand-all/collapse-non-diff context toggle. / PR 工作流规格包含可逆的 GitHub 风格“展开全部/收起非 Diff 行”上下文切换。

Documents that are not specs:

- `docs/architecture.md`, `docs/architecture.zh.md`: canonical architecture.
- `docs/ARCHITECTURE_FOR_AI.md`: compatibility entry point for older agent prompts.
- `docs/terminal_viewport_iteration_notes.md`: historical terminal investigation log.

When adding a spec, include scope, invariants, ownership, failure behavior, and a regression checklist. Update this index in the same change.
