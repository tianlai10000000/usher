# Specifications / 规格索引

Specifications describe stable behavior and invariants. Architecture documents explain ownership and flow; historical notes explain how a problem was investigated. Do not mix those three roles.

Spec 描述稳定行为和不变量；架构文档解释职责与数据流；历史记录描述问题排查过程。三者不要混写。

| Spec | Status | Code owners |
| --- | --- | --- |
| [HTTP pipeline](http.md) | Current / 当前 | `src/http/mod.rs`, all HTTP callers |
| [Terminal behavior](terminal.md) | Current / 当前 | `src/terminal/`, `ui/components/drawer/terminal_drawer.slint`, `ui/main.slint` |

Documents that are not specs:

- `docs/architecture.md`, `docs/architecture.zh.md`: canonical architecture.
- `docs/ARCHITECTURE_FOR_AI.md`: compatibility entry point for older agent prompts.
- `docs/terminal_viewport_iteration_notes.md`: historical terminal investigation log.

When adding a spec, include scope, invariants, ownership, failure behavior, and a regression checklist. Update this index in the same change.
