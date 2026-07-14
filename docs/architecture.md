# Usher Architecture

[中文](architecture.zh.md) · [README](../README.md) · [Specifications](specs/README.md)

## Purpose and scope

This document describes the current codebase, ownership boundaries, runtime lifecycle, and safe extension points. It is the canonical English architecture reference. Behavioral requirements that need explicit invariants live under `docs/specs/`.

Usher is a desktop-first Rust application. Slint owns presentation and declarative UI state; Rust owns persistence, process execution, networking, and application orchestration.

## System context

```text
User
  -> Slint window and reusable components
     -> callbacks exported by ui/main.slint
        -> Rust feature modules
           -> local Git / PTY / filesystem
           -> shared reqwest HTTP pipeline
           -> Tokio runtime or dedicated blocking threads
        -> UI updates scheduled on the Slint event loop
```

External integrations currently include Git executables, GitHub/GitLab/Gitee-compatible APIs, the configured RAG chat endpoint, GitHub Releases through `self_update`, platform tray/window APIs, and OS-native application directories.

## Source layout and ownership

### Composition and runtime

| Path | Responsibility |
| --- | --- |
| `src/main.rs` | Minimal binary entry point. Delegates to the library entry point. |
| `src/lib.rs` | Creates `MainWindow`, initializes modules, wires cross-cutting callbacks, manages window lifecycle and shutdown. |
| `src/app_runtime.rs` | Process-wide multi-thread Tokio runtime. Runtime threads install the tracing dispatch. |
| `src/shutdown.rs` | Ordered shutdown coordination. |
| `src/single_instance.rs` | Single-instance process behavior. |
| `src/tray.rs` | Desktop tray integration and tray-driven lifecycle actions. |

`src/lib.rs` is a composition root, not the preferred home for feature logic. New business behavior belongs in a feature module and should expose a small setup or service API.

### Feature modules

| Path | Responsibility |
| --- | --- |
| `src/project/` | Project registry, persistence, filtering, logos, PR/MR summaries, and project-to-workflow/terminal entry points. |
| `src/workflow/` | Per-project Branch, Tag, PR, and CD view state and command execution. |
| `src/terminal/` | PTY lifecycle, viewport synchronization, rendering, input, selection, and project switching. |
| `src/chat/` | Chat transport, sessions, JSONL persistence, Markdown conversion, rendering cache, pagination, and interaction binding. |
| `src/update/` | Release checks, streamed downloads, progress, cancellation, installation, and restart. |
| `src/router/` | Route-to-navigation mapping and route-specific visibility state. |
| `src/settings.rs` | TOML settings model and persistence. |
| `src/network.rs` | Background connectivity probe and `AppState.network-online`. |
| `src/http/mod.rs` | Shared asynchronous/blocking reqwest clients, interceptors, error normalization, and optional UI feedback. |
| `src/logger/` | Debug logging, tracing, and panic/error support. |
| `src/toast.rs` | Transient single-toast manager binding. |
| `src/notification.rs` | Queued notifications, timers, hover pause, actions, and icons. |
| `src/dialog.rs` | Global modal dialog binding. |

`src/git/` currently contains placeholders; provider HTTP behavior is implemented in `src/project/pr_stats.rs`. Do not assume the placeholder files are active abstractions.

### Slint UI

| Path | Responsibility |
| --- | --- |
| `ui/main.slint` | Root window, route composition, Rust-facing properties/callbacks, modal layers, and terminal docking. |
| `ui/route/routeMap.slint` | Complete `Route` enum. Only a subset is exposed in main navigation. |
| `ui/state/` | Slint global state adapters. |
| `ui/views/` | Page-level views. |
| `ui/views/workflow/` | Branch, Tag, PR, and CD workflow views. |
| `ui/components/` | Reusable application components. |
| `ui/theme/` | Application palette mapping. |
| `material-1.7.1-fixed/` | Vendored Material source aligned with Slint v1.17.1. |

Application UI must import Material through `@material` unless direct access to a non-exported vendored component is required. Changes to the vendored directory require an upstream comparison and a documented reason.

## Startup lifecycle

The effective startup sequence in `src/lib.rs` is intentionally ordered:

1. Load settings before window creation so platform appearance can be selected.
2. Create `MainWindow` and initialize debug/tracing loggers.
3. Apply settings and configure updater state.
4. Bind theme, routing, navigation, and window callbacks.
5. Initialize notification and toast managers.
6. Bind the shared HTTP module to UI feedback, then start network monitoring.
7. Initialize dialog, project, chat, terminal, and workflow modules.
8. Install tray integration and run the Slint event loop.
9. On close/quit, coordinate terminal, updater, tray, runtime, and process cleanup through shutdown priorities.

Do not move modules earlier if they depend on generated Slint globals or on callbacks installed by a preceding module.

## Concurrency model

- Slint objects and globals are event-loop-owned. Background tasks must use `slint::invoke_from_event_loop` before updating them.
- Async I/O uses the shared Tokio runtime from `src/app_runtime.rs`; do not create a new runtime or a new reqwest client per operation.
- Existing blocking Git, PR-statistics, and filesystem work runs on dedicated threads or Tokio blocking tasks. Never block the Slint event loop.
- Shared Rust state uses `Arc<Mutex<_>>`, `Arc<RwLock<_>>`, atomics, or per-thread caches depending on ownership.
- Long-running actions need cancellation or re-entry protection. Update downloads, history pagination, workflow runs, and terminal sessions are reference patterns.

## State and persistence

### Settings

`AppSettings` is persisted as TOML in the platform config directory. It contains theme mode, auto-update, logger flags, terminal diagnostics, navigation drawer state, and Git platform tokens. `AppState` mirrors values required by Slint.

### Projects

`project/projects.json` stores project records in the platform local-data directory. Rust converts records to Slint models, resolves local/base64/remote logos, and schedules PR overview work. Remote logo bytes are cached under the process temporary directory; UI rendering should read cached data rather than perform network work.

### Workflow

`WorkflowStore` keeps a current project plus per-project `WorkflowViewState`. Switching projects must rebind UI state and prevent logs, steps, or running flags from leaking between projects. Heavy workflow state is explicitly cleared when leaving the workflow route.

### Chat

Chat history is append-only JSONL at `im/chat_history.jsonl`. Startup loads a recent tail; upward pagination loads older batches. Rendering and scroll-anchor calculations are deliberately separated to limit work during scrolling/resizing.

### Terminal

Terminal state is project-aware and persists while reopening the same project. Switching projects resets session-specific presentation and changes the working directory. The stable contract is documented in [specs/terminal.md](specs/terminal.md).

## Main data flows

### Project to workflow

1. A project card invokes the root workflow-open callback.
2. `workflow/bindings.rs` creates a `WorkflowProjectContext`.
3. `WorkflowStore` selects the project-scoped state.
4. Workflow modules initialize their Slint properties.
5. PR counts are loaded on a worker thread and applied through the event loop.

### Project-aware terminal

1. Project card actions or the workflow FAB invoke a terminal callback.
2. Rust resolves the project key and local repository path.
3. `open_terminal_for_project` chooses reuse versus reset.
4. The PTY session runs off the UI thread.
5. Rendered terminal state is applied to `TerminalDrawer` through root properties.

### Chat request

1. `chat/interactive.rs` appends and persists the user message.
2. The shared runtime calls `chat/chat.rs`.
3. `src/http/mod.rs` runs request interceptors, executes reqwest, validates status, runs response/error interceptors, and decodes JSON.
4. The result is persisted and transformed for Slint on the event loop.
5. Failures remain an inline assistant message; the request is silent to prevent duplicate toast feedback.

### Update download

1. `self_update` resolves release metadata on a blocking task.
2. The shared HTTP pipeline starts a long-timeout streaming request.
3. Chunks are written to a temporary archive and progress is reported.
4. Cancellation deletes the partial file.
5. Existing update UI owns success/error notifications, so HTTP feedback remains silent.

## HTTP boundary

`src/http/mod.rs` is the only place allowed to construct application-wide reqwest clients. It offers request builders rather than hiding reqwest, so callers retain query, JSON, headers, timeouts, and streaming.

Every request supplies `RequestOptions`:

- `operation` provides stable logs and user-facing context.
- `reject_http_errors` defaults to true and converts non-2xx responses into `HttpError` with a bounded body excerpt.
- `feedback` defaults to `Silent`; user actions may opt into toast, notification, or both.

Interceptors execute in registration order. Request hooks may mutate headers; response hooks observe status/headers/timing; error hooks receive normalized failures. See [specs/http.md](specs/http.md).

## UI composition rules

- Root Rust-facing properties and callbacks are declared in `ui/main.slint` with consistent names.
- Feature views own layout; Rust owns I/O and business state transitions.
- Reuse `PageScrollActions`, `ProjectCardActions`, `IconActionButton`, and project-aware terminal entry points.
- Keep hover-sensitive nodes mounted and vary opacity/state instead of conditionally creating hit targets.
- Mount full-window modal overlays at the root layer.
- Avoid layout/model rebuilds during scroll and window resize; defer precise work until interaction settles.

## Validation

Minimum validation for Rust or Slint changes:

```sh
cargo fmt --all
cargo check --locked
cargo test --lib --locked
```

Run targeted manual checks for terminal viewport/session changes, update cancellation, tray shutdown, and route-specific layouts. The complete test command currently includes one unrelated failing logger doctest; do not hide a new failure behind that known issue.

## Known technical debt

- The RAG endpoint is compiled into `src/constants/chat.rs` and should become configuration before general distribution.
- `src/git/{github,gitlab,gitee}.rs` are not yet the provider abstraction their names suggest.
- `src/logger/debug_logger.rs` contains a doctest that does not compile.
- Some routes in `Route` are development/demo surfaces and are not in primary navigation.
- Root project licensing still needs an explicit decision.
- `docs/terminal_viewport_iteration_notes.md` is historical evidence, not a current specification.
