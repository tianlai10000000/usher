# Usher Architecture

[中文](architecture.zh.md) · [README](../README.md) · [Specifications](specs/README.md)

## Purpose and scope

This document describes the current codebase, ownership boundaries, runtime lifecycle, and safe extension points. It is the canonical English architecture reference. Behavioral requirements that need explicit invariants live under `docs/specs/`.

Usher is a desktop-first Rust application. Slint owns presentation and declarative UI state; Rust owns persistence, process execution, networking, and application orchestration.

Production builds select Winit with FemtoVG/OpenGL at compile time. Skia and FemtoVG/WGPU are opt-in Cargo features used for renderer diagnostics and A/B comparisons, so normal builds cannot silently change renderer because multiple implementations were linked. Slint's in-process live preview is also an explicit `live-preview` development feature and is not compiled into default or production builds. The WGPU path is not a production default because current macOS tests reduce physical footprint but increase RSS.

## System context

```text
User
  -> Slint window and reusable components
     -> callbacks exported by ui/main.slint
        -> generated API owned by crates/usher-ui
           -> Rust feature modules bound by src/lib.rs
              -> local Git / PTY / filesystem
              -> shared reqwest HTTP pipeline
              -> Tokio runtime or dedicated blocking threads
        -> UI updates scheduled on the Slint event loop
```

External integrations currently include Git executables, GitHub/GitLab/Gitee-compatible APIs, a user-configured OpenAI-compatible AI Agent provider, GitHub Releases through `self_update`, platform tray/window APIs, and OS-native application directories.

## Source layout and ownership

### Composition and runtime

| Path | Responsibility |
| --- | --- |
| `src/main.rs` | Minimal binary entry point. Delegates to the library entry point. |
| `src/lib.rs` | Imports and re-exports the generated UI API, creates `MainWindow`, initializes modules, wires cross-cutting callbacks, and manages window lifecycle and shutdown. |
| `crates/usher-ui/` | Owns the Slint build script and generated Rust API. Compiles the repository-level `ui/main.slint` tree and exports its window, globals, models, structs, and enums. |
| `src/app_runtime.rs` | Process-wide multi-thread Tokio runtime. Runtime threads install the tracing dispatch. |
| `src/memory_diagnostics.rs` | Process-wide RSS/footprint snapshots, startup checkpoints, UI-size correlation, and the cancellable periodic memory sampler. |
| `src/shutdown.rs` | Ordered shutdown coordination. |
| `src/single_instance.rs` | Single-instance process behavior. |
| `src/tray.rs` | Desktop tray integration and tray-driven lifecycle actions. |

`src/lib.rs` is a composition root, not the preferred home for feature logic. New business behavior belongs in a feature module and should expose a small setup or service API.

### Feature modules

| Path | Responsibility |
| --- | --- |
| `src/project/` | Project registry, persistence, filtering, logos, PR/MR summaries, and project-to-workflow/terminal entry points. |
| `src/codereview/api/` | URL and path templates for Gitee, GitLab, and GitHub repository APIs. It contains no request execution, authentication, or response decoding. |
| `src/git/` | Provider detection, repository operations, authentication headers, and normalization of provider responses into application models. |
| `src/workflow/` | Per-project Branch, Tag, PR, and CD view state and command execution. |
| `src/terminal/` | PTY lifecycle, viewport synchronization, rendering, input, selection, and project switching. |
| `src/chat/` | Chat transport, sessions, JSONL persistence, Markdown conversion, rendering cache, pagination, and interaction binding. |
| `src/update/` | Release checks, streamed downloads, progress, cancellation, installation, and restart. |
| `src/router/` | Route-to-navigation mapping and route-specific visibility state. |
| `src/settings.rs` | TOML settings model and persistence, including AI Agent credentials. |
| `src/ai/` | Standalone OpenAI-compatible AI Agent integration; `agent.rs` owns the connection test. It does not own RAG Chat. |
| `src/network.rs` | Background connectivity probe and `AppState.network-online`. |
| `src/http/mod.rs` | Shared asynchronous/blocking reqwest clients, interceptors, error normalization, and optional UI feedback. |
| `src/logger/` | Debug logging, tracing, and panic/error support. |
| `src/toast.rs` | Transient single-toast manager binding. |
| `src/notification.rs` | Queued notifications, timers, hover pause, actions, and icons. |
| `src/dialog.rs` | Global modal dialog binding. |

Repository endpoint definitions and executable behavior are intentionally separated: `src/git/` may reference constants from `src/codereview/api/`, while `src/codereview/api/` must not depend on `src/git/`, `src/http/`, reqwest, or serde.

### Slint UI

| Path | Responsibility |
| --- | --- |
| `crates/usher-ui/build.rs` | Compiles the root Slint source tree and maps the vendored Material library to `@material`. |
| `crates/usher-ui/src/lib.rs` | Includes and publicly exports the generated Slint module. |
| `ui/main.slint` | Root window, route composition, Rust-facing properties/callbacks, modal layers, and terminal docking. |
| `ui/route/routeMap.slint` | Complete `Route` enum. Only a subset is exposed in main navigation. |
| `ui/state/` | Slint global state adapters. |
| `ui/views/` | Page-level views. |
| `ui/views/workflow/` | Branch, Tag, PR, and CD workflow views. |
| `ui/components/` | Reusable application components. |
| `ui/theme/` | Application palette mapping. |
| `material-1.7.1-fixed/` | Vendored Material source aligned with Slint v1.17.1. |
| `vendor/i-slint-live-preview/` | Temporary Slint 1.17.1 override selected only by the opt-in `live-preview` feature. Its documented `mtime` guard prevents spurious macOS watcher events from causing an endless recompile loop. |

Application UI must import Material through `@material` unless direct access to a non-exported vendored component is required. Changes to either vendored dependency require an upstream comparison and a documented reason. The live-preview override must be removed when upstream provides an equivalent watcher guard.

The UI source remains under the repository-level `ui/` directory, but generated Rust is compiled only by the `usher-ui` workspace crate. The application crate re-exports that public API so existing feature modules can continue to import `crate::{MainWindow, Route, ...}` without owning the generated module. This is a compile boundary: changing Rust feature logic must not force the Slint-generated crate to rebuild, while a UI source change rebuilds `usher-ui` before the application bindings are checked.

## Build artifact policy

Workspace-root Cargo profiles own the build-artifact policy. Everyday `dev` and
`test` builds use line-table debug information with `split-debuginfo = "off"`
and keep incremental compilation enabled. On macOS this overrides Cargo's
full/unpacked debug default, retaining file-and-line backtraces without
accumulating loose `.rcgu.o` files in `target/debug/deps`.

Full LLDB variable/type inspection is explicit through the `dev-full` profile.
It inherits normal development behavior but emits full, packed debug
information into its own profile directory. This keeps heavyweight symbols
separate and allows `cargo clean --profile dev-full` to remove them without
discarding normal development artifacts. Release and dist profiles remain
independent: release builds disable debug information and incremental
compilation.

Workspace-wide IDE diagnostics use the isolated `ide` profile. It checks all
targets without debug information or incremental state, preventing background
analysis from duplicating several GiB of application and generated-UI caches.
VS Code/Cursor selects it through workspace settings; RustRover users select it
as the Cargo external-linter profile. Interactive build, run, and test commands
continue to use their incremental profiles.

Cargo does not currently enforce a size limit or automatic retention policy for
workspace `target/` artifacts. Developers upgrading from the previous unpacked
policy should run one full `cargo clean`; later cleanup should prefer a specific
profile or cross-compilation target. Reproducible measurements and commands are
recorded in [build-performance.md](build-performance.md).

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

Debug builds enable process memory diagnostics automatically; release builds enable them with `USHER_MEMORY_DIAGNOSTICS=1`. `[Memory][Diag]` records current/peak RSS, physical footprint, VM categories, regions, Mach ports, threads, faults, page-ins, and deltas from the previous/startup samples. `[Memory][UI]` correlates settled route/window changes with logical and physical dimensions. Feature-owned logs, such as `[Workflow][PR Review][Memory]`, must report bounded model/cache ownership without exposing source content or credentials. The sampler uses the shared runtime and is stopped by the normal pre-shutdown pipeline.

## Concurrency model

- Slint objects and globals are event-loop-owned. Background tasks must use `slint::invoke_from_event_loop` before updating them.
- Async I/O uses the shared Tokio runtime from `src/app_runtime.rs`; do not create a new runtime or a new reqwest client per operation.
- Existing blocking Git, PR-statistics, and filesystem work runs on dedicated threads or Tokio blocking tasks. Never block the Slint event loop.
- Shared Rust state uses `Arc<Mutex<_>>`, `Arc<RwLock<_>>`, atomics, or per-thread caches depending on ownership.
- Long-running actions need cancellation or re-entry protection. Update downloads, history pagination, workflow runs, and terminal sessions are reference patterns.

## State and persistence

### Settings

`AppSettings` is persisted as TOML in the platform config directory. It contains theme mode, auto-update, logger flags, terminal diagnostics, navigation drawer state, Git platform tokens, plus per-provider enterprise API URLs and names under `[git.platform.<provider>]`, and OpenAI-compatible AI Agent connection values under `[ai.agent]`. Gitee stores public `token` and `enterprise_token` separately. A legacy single Gitee token is migrated to the enterprise field only when an enterprise API root was already configured; credentials never fall back across profiles. `AppState` mirrors values required by Slint. API keys, enterprise names, and configured URLs are local configuration and must not be logged. Provider API code uses an enterprise API URL only when its host matches the repository remote; users may supply the host or the complete API root, with the provider version path added if omitted. Public-provider defaults remain remote-derived.

### Projects

`project/projects.json` stores project records in the platform local-data directory. On an add or edit save, Rust derives the provider from `remote_repo`, fetches its project details through `src/git/` using URL templates from `src/codereview/api/`, and stores the resulting `repo_project_id` and lower-case `repo_type` (`gitee`, `gitlab`, or `github`) together with the project record. A failed lookup leaves the previous configuration untouched. Rust converts records to Slint models, resolves local/base64/remote logos, and schedules PR overview work. Remote logo bytes are cached under the process temporary directory; UI rendering should read cached data rather than perform network work.

### Workflow

`WorkflowStore` keeps a current project plus per-project `WorkflowViewState`. Switching projects must rebind UI state and prevent logs, steps, or running flags from leaking between projects. `Route.Workflow` and `Route.PrReview` form one lifecycle domain: moving between them retains the Workflow list/model state, and a temporary route outside the domain retains it while either workflow route remains reachable through Back.

Cross-page Back navigation is owned by `src/router/` as a LIFO route history; Slint renders the current route and delegates Back instead of mutating a one-slot `previous-route` value. Feature controllers own drill-down checkpoints that do not change the route. Back first consumes such a feature-local checkpoint, then pops the global route history. A workflow route retained in that history remains a live return target, so temporarily opening Settings, About, or another page does not discard the PR review; heavy workflow state is released only when the workflow domain is no longer current or reachable through Back.

The Workflow PR list and full-window review workspace use a single provider-neutral façade in `src/git/`. Every list, detail/diff, repository-file, review-state, review-submit, and merge request is selected from the current project's persisted `repo_type`, then checked against `remote_repo` before the provider implementation runs. The review controller owns one shared immutable normalized diff and a separate action sequence that prevents stale review or merge responses from updating another project or PR. The normalized detail moves each base file into an `Arc`, so the selected-file lazy model and highlighting task share that allocation instead of cloning every line; only a hunk expansion creates a per-file copy-on-write override. Provider HTML is decoded at the adapter boundary, and each diff record is normalized to one visual source line without CR/LF controls before fixed-height Slint rendering. Collapsing an expanded file drops its override and exposes the shared base file again. Immutable head/base file content is fetched lazily for hunk expansion or in-app full-file viewing; edited-file expansion aligns both fixed revisions locally so one-sided lines are retained. Per-file highlighted diffs use a four-entry LRU and immutable source text uses a three-entry LRU; the full-file highlighted model is transient UI state. The active diff is exposed to Slint through a read-only lazy model that materializes at most 64 converted rows, while the complete highlighted result remains Rust-owned for arbitrary-language support and fast scrolling. All late async responses are sequence-guarded.

Revert commands use the same provider-neutral façade and action sequence as review/merge commands, so stale results cannot affect another project or PR. A merged request hides review/merge controls and may expose a visible same-row Primer-style secondary header revert button only when the adapter returns `can_revert` (Usher prefers that over GitHub's kebab ActionMenu so the full label stays readable). GitHub GraphQL creates a separate reverse PR. Gitee Enterprise reloads the MR, requires a merge/squash commit whose `/repository/commits/:sha/refs` response reports both `merge_commit_by_pr` and `can_revert`, then calls the documented commit-revert endpoint against the target branch; rebase/fast-forward, public Gitee, and GitLab remain unsupported. Provider-specific confirmation text makes the direct Gitee branch mutation explicit.

Gitee Enterprise commit badges treat only positive MR detail/diff summaries as authoritative. A zero or missing summary triggers the lightweight commits count probe before the Commits tab is activated, because some deployments advertise zero until that collection is read; the full Commits content model remains lazy.

The Files changed `Diff view` popover owns a provider-neutral Unified/Split preference and the capability-gated Hide whitespace draft. `workflow/pr_review.rs` projects cached normalized lines for Unified or Split rendering; Split pairs each contiguous deletion/addition block and duplicates context across both sides. Applying only the view choice replaces the current lazy line model locally, while applying a supported whitespace change reloads provider data. Slint renders that projection and keeps unsupported whitespace checkboxes visible but disabled. The complete popover surface owns a pointer shield, so blank overlay areas cannot leak hover to the file tree below. It is hosted on the full Files changed toolbar row with a Primer upward caret whose tip tracks the settings gear even when the panel is edge-clamped.

Large files are classified by a provider-neutral changed-line or payload-size budget. Their normal file header remains available, but syntax highlighting and the Slint line model are deferred until that file's `Load diff` action is invoked. The in-place loading decision is scoped to the active complete/range diff and resets when the commit range changes.

The existing Workflow PR tab owns the repository pull-request list. `workflow/pr_list.rs` reads the selected project's persisted `repo_project_id` and `repo_type` off the event loop, then lazily requests one 10-record remote page through `src/git/` when the user opens or changes a page. GitHub preserves search-result totals and concurrently enriches only that page's rows with per-PR detail requests so source and target branch names are available. The controller formats parseable provider update timestamps for UTC+08:00 before passing list values to Slint, and uses exact totals where the provider supplies them. The tab defaults to open requests and explicitly supplies the selected open/closed/merged/all filter. Request sequencing prevents a late response from a previous project, state, or page from replacing the current view. A successful zero-item page renders a filter-specific illustrated empty state, while transport, authentication, and schema failures remain a distinct retryable error state. Clicking a row navigates to the full-window `Route.PrReview`; the Workflow component instance is destroyed, while its root/model state is retained so Back reconstructs the PR tab without reloading the list. The review route owns GitHub-style Conversation, Commits, and Files changed tabs. Conversation and Commits *content* are loaded only on first selection through provider-neutral models: GitHub combines PR detail, issue comments, reviews, and issue timeline activity (`committed` batches, `merged`, and selected system events); Gitee public v5 uses PR detail/comments/commits and synthesizes commit/merge activity from commits plus merge metadata; Gitee Enterprise v8 uses MR detail/notes/commits (plus best-effort `/reviews`) with the same synthesized activity after flattening nested enterprise `notes[].note` threads and reading ordinary note bodies from `note` / nested `note.note` / `comment` / string `content` (never object `content`), and MR commit timestamps from `created_at`; GitLab uses MR detail/notes/commits and synthesizes the same portable `commits`/`commit`/`merged` rows while filtering redundant system notes. Tab badge numbers do not wait for Conversation/Commits tab activation: they come from lightweight PR/MR summary fields carried with the diff load (`commit_count`, `conversation_count`, `files.length`). When a provider omits totals, adapters may issue one bounded count probe (GitHub/GitLab Link/`X-Total` with `per_page=1`; Gitee public short-page `per_page=100`; Gitee Enterprise diffs `commits_size`, MR `user_notes_count`/`notes_count`, or `with_page=true&per_page=1` + `meta.total_count`). Full Conversation timelines and commit lists stay lazy until those tabs (or the commit selector) are opened. The provider-neutral finalizer sorts each `commits` header with its consecutive `commit` rows as one atomic block, preserving the GitHub-style `added N commits` record for every provider even when individual authored dates differ. Conversation commit children and the Commits tab (including the Files changed commit selector) render newest→oldest; adapters still normalize lists oldest→newest, and the controller remaps display indices before contiguous range compares. A successful remote review or merge invalidates the cached Conversation timeline and reloads it immediately when Conversation is active. The Files changed controller loads and normalizes the selected PR diff, derives a collapsible directory tree, and keeps path filtering, folder expansion, and previous/next file navigation local. Its commit selector reuses the lazy commit list and dispatches contiguous ranges to each provider's repository compare endpoint; the original PR detail and previously loaded ranges remain cached for the active review. `workflow/syntax_highlight.rs` uses `syntect` off the UI layer to convert the selected file into theme-aware token models while preserving tabs and display-column widths. During file switches or hunk expansion, the previous line model stays mounted until the new highlighted model is ready; the event loop swaps it atomically, and only a real file switch resets the viewport. A hunk click records the clicked row's actual screen position before asynchronous expansion. After the model swap, the target delegate reports its post-layout content position and Slint reapplies the resulting visual anchor for a bounded number of layout frames. This avoids estimating wrapped row heights: a surviving hunk control remains under the pointer, while a removed separator places the first revealed row at the old control position. On macOS, native live-resize notifications freeze the review workspace's internal geometry and replace each visible token repeater with one plain monospace line; the final geometry and full token-colored tree are restored when live resize ends.

The review header follows GitHub's two-step command model. A GitHub-styled readiness badge beside the title renders the normalized kind/detail with Primer semantic colors, while each adapter owns its merge semantics: GitHub maps REST mergeability, Gitee Enterprise maps v8 `current_state` conditions, public Gitee maps only v5 fields, and GitLab maps `detailed_merge_status` with an asynchronous recheck. The shared enum is a presentation boundary and must never cause one provider's conditions to be applied to another. `Review changes` submits an explicit provider-neutral Comment, Approve, or Request changes command; unsupported choices remain visible but disabled. The Conversation tab also exposes a GitHub-style plain-text composer (`Write` + green `Comment`) that submits the same Comment action. Provider success triggers a remote state refresh and never starts a merge. `Merge pull request` is enabled only by the latest normalized `PullRequestActionState` and presents only adapter-returned methods. GitHub supports repository-enabled merge, squash, and rebase methods. Gitee Enterprise v8 maps review states, omits the optional `comment` field for an Approve with no summary, and supports merge, squash, rebase, and fast-forward according to `restrict_merge_type`; public Gitee v5 supports comments plus merge/squash but has no approve/request-changes endpoint. GitLab readiness is read-only and remote writes remain an explicit unsupported capability.

The review file tree uses GitHub's platform UI font stack (SF system UI on macOS and Segoe UI on Windows), while source code uses the platform monospace stack. Both the changed-file tree and diff rows use virtualized `ListView` delegates so only visible rows are instantiated. The divider resizes the pane live; its moving-handle calculation applies the remaining local pointer delta to the current width, keeping the divider attached to the pointer without rebuilding the complete tree or diff. The session preference is clamped to 220–520 px and no more than 45% of the review width so the diff remains the primary surface. Visibility follows GitHub Primer `$width-md` (768px): crossing below auto-collapses the tree and crossing back above auto-expands it, while the toolbar toggle always works so a narrow window can reopen the tree manually. The main window minimum width is 640px so the 768px threshold remains reachable.

The review header uses a compact 36 px Primer-style back IconButton with the official `arrow-left` Octicon. Removing the wide text control and divider gives the wrapping title more room; compact-layout actions begin immediately after this icon. The back tooltip keeps its below-icon placement but is hosted on the header layer with a high z so summary Open/readiness chrome cannot cover it. The readiness detail, file-extension filter, and file-action menu are mutually exclusive in-window popovers owned by the workspace; switching triggers preserves hover and opens the target popover on the first click instead of relying on a native popup mouse grab.

Conversation adapters normalize optional author and commit-author avatar URLs from GitHub, GitLab, Gitee public v5, and Gitee Enterprise v8. The controller fetches unique avatars concurrently through the shared HTTP pipeline with a three-second aggregate timeout, a 2 MiB per-image limit, and bounded decoded dimensions; only send-safe RGBA bytes cross to the event loop, where Slint images are created. Missing or failed avatars fall back to an author initial. Timeline cards and the composer share one 52 px avatar/content grid. GitHub-style activity uses one continuous Primer timeline rail through avatar centers (clipped on the first and last items). Comment/description cards are 1px framed boxes that hug short bodies; commit marks and compact author avatars sit on the same rail; every activity row shares the cards' right edge. Rust supplies the exact row-height sum. The Conversation tab scrolls timeline and composer as one document so the Write box follows the last event instead of staying pinned to the viewport bottom.

Conversation commit batches use Primer's official `repo-push` Octicon. Their relative timestamp remains inline immediately after `added N commits`, while the shared absolute-time tooltip preserves exact provider time. GitHub reviews become an atomic activity summary plus optional body card; review state controls wording, automatic relative-time units match GitHub, and merged activity suppresses redundant closed activity. Connector segments follow semantic boundaries, but consecutive review pairs share a continuous rail from the preceding review card to the next summary marker. Gitee Enterprise ordinary comments use MR `/notes` with `noteable_type: PullRequest`; `/reviews` is reserved for review decisions. A commit-count probe without totals treats a response exactly filling the requested page as ambiguous—even for a bare array—and falls back to counting and discarding the documented complete non-paginated collection without mounting the lazy Commits model.

Diff geometry uses one measured extent: Rust passes the longest expanded code line alongside display-column metadata, and Slint measures it with the active monospace font for horizontal scrolling and row-background painting. This prevents an unpainted strip at the right edge and keeps tabs/wide glyphs aligned. The code column accepts GitHub-style drag selection across ListView rows (gutters excluded; hunk/meta rows never selectable and omitted from clipboard text); the synchronous review-controller hit test walks actual projected row wrap heights so a forward or reverse drag can resolve an exact character in a wrapped endpoint. ⌘/Ctrl+C copies the plain selection silently. Selection state is only two endpoints, and selection rectangles are created only by selected visible delegates, preserving the bounded lazy model and normal token-only scroll path.

Commit links in Conversation and Commits reuse the Files changed workspace by resolving the selected SHA and applying it as a one-item commit range. The review controller records the originating Conversation or Commits tab as a feature-local drill-down checkpoint, so the review header's Back action restores that tab before the next Back returns to the Workflow PR list. When the commit list is still lazy, the controller completes that load before applying the pending selection; Copy SHA remains independent.

### Pull request subsystem boundary

The PR feature has five replaceable layers. This is the intended boundary for a rewrite in another technology stack:

```text
Presentation
  PR list/table + dedicated review route
        ↕ normalized view models and loading/error state
Workflow controllers
  project selection, page/state changes, stale-result guards, review cache
        ↕ provider-neutral use cases
Repository service
  validate persisted provider type against remote URL; list, diff, source, review, merge, revert
        ↕ provider-neutral domain records
Provider adapters
  GitHub, GitLab, public Gitee, Gitee-compatible enterprise API mappings
        ↕ shared HTTP/auth/persistence
Infrastructure
  project registry, local tokens/configuration, HTTP client/interceptors, runtime
```

Presentation does not receive provider JSON or credentials. Provider adapters do not own UI state. The repository service is the only layer allowed to select an adapter after validating the remote URL, and it exposes whitespace filtering, review actions, mergeability, and allowed merge methods as normalized capabilities rather than making the UI infer them. The current application uses Slint, Tokio, and Rust mutexes to realize those layers, but the durable contract is the normalized page/item/diff/source/action-state data and the “latest request wins” lifecycle described in [the PR workflow specification](specs/pr-review.md). Provider review commands remain the only write path for approve/comment/request-changes decisions.

### Chat

Chat history is append-only JSONL at `im/chat_history.jsonl`. Startup loads a recent tail into the Rust session, but the Chat component and bounded Slint message model are mounted only on the Chat route. Leaving Chat destroys its UI tree and clears the Slint model/Markdown cache; re-entry reconstructs the visible window from the session. Late history or assistant responses continue updating persistence/session without repopulating a hidden UI. Rendering and scroll-anchor calculations are deliberately separated to limit work during scrolling/resizing.

### Terminal

Terminal state is project-aware and persists while reopening the same project. Switching projects resets session-specific presentation and changes the working directory. The stable contract is documented in [specs/terminal.md](specs/terminal.md).

## Main data flows

### Project to workflow

1. A project card invokes the root workflow-open callback.
2. `workflow/bindings.rs` creates a `WorkflowProjectContext`.
3. `WorkflowStore` selects the project-scoped state.
4. Workflow modules initialize their Slint properties.
5. PR counts are loaded on a worker thread and applied through the event loop.
6. The PR-list controller reads persisted repository metadata and lazily loads the selected remote open page through the shared runtime; every page change requests that page only.

### Pull request list to review

1. The list controller loads repository metadata off the UI thread and resets its page cache whenever the selected project or list state changes.
2. The repository service rejects a persisted `repo_type` that does not match `remote_repo`, then routes the request to the matching provider adapter.
3. The adapter normalizes provider responses into a 10-item page. GitHub retains its exact search total and enriches visible rows with concurrent detail requests for branch refs.
4. The controller applies a page only if its request sequence is current; it formats parseable update times as UTC+08:00 before producing view models.
5. Opening a row navigates from `Workflow` to `PrReview` inside the shared Workflow lifecycle domain. The Workflow component is unmounted but its list/root models remain retained; the review controller reads fresh metadata, requests one normalized diff, caches it by project/request/whitespace mode, and renders its local file tree.
6. File selection, filtering, folder expansion, copy-path, ratio rendering, syntax highlighting, and collapsing expanded context are local. Hunk expansion and in-app View file are the only follow-up provider reads; both share the bounded immutable-source cache. Directional expansion follows GitHub's signed-in improved `/changes/{commit}` view and reveals 21 primary-side source rows per page; the signed-out classic `/commits/{commit}` view has different 20-row behavior and is not this UI's baseline. Once the remaining between-hunk context fits one page, the directional controls become one expand-all action; revealing the final context removes the obsolete hunk separator. File-level expand-all fills every hidden range, and collapse restores the retained original diff.
7. Returning to Workflow or switching project invalidates review/conversation/commit/highlight/source sequences so late work cannot affect the new context.

### Project save and provider lookup

1. The Project page submits the local/remote repository fields to its Rust callback.
2. `src/project/mod.rs` validates local fields and starts one shared-runtime lookup per project save.
3. `src/git/` parses `remote_repo`, builds the matching provider request from constants in `src/codereview/api/`, and executes it through `src/http/mod.rs`.
4. Only a successful response updates `projects.json` with `repo_project_id` and `repo_type`; the UI refresh runs on the Slint event loop.
5. Lookup or persistence failures show one project-owned error toast and do not write a partial record.

### Project-aware terminal

1. Project card actions or the workflow FAB invoke a terminal callback.
2. Rust resolves the project key and local repository path.
3. `open_terminal_for_project` chooses reuse versus reset.
4. The PTY session runs off the UI thread.
5. Rendered terminal state is applied to `TerminalDrawer` through root properties.

### Chat request

1. `chat/interactive.rs` appends and persists the user message.
2. The shared runtime calls `chat/chat.rs` with the existing RAG endpoint and request format.
3. `src/http/mod.rs` runs request interceptors, executes reqwest, validates status, runs response/error interceptors, and decodes the RAG response.
4. The result is persisted and transformed for Slint on the event loop.
5. Failures remain an inline assistant message; the request is silent to prevent duplicate toast feedback.

### AI Agent connection test

1. Settings saves an independent OpenAI-compatible Base URL, API Key, and model name.
2. A user-triggered test runs `ai/agent.rs` on the shared runtime.
3. The shared HTTP module requests the provider's `/models` endpoint and reports the result through Toast feedback.
4. This flow must not change the RAG Chat endpoint, payload, or history behavior.

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

- `src/logger/debug_logger.rs` contains a doctest that does not compile.
- Some routes in `Route` are development/demo surfaces and are not in primary navigation.
- Root project licensing still needs an explicit decision.
- `docs/terminal_viewport_iteration_notes.md` is historical evidence, not a current specification.
