# Usher Architecture

[中文](architecture.zh.md) · [README](../README.md) · [Specifications](specs/README.md)

## Purpose and scope

This document describes the current codebase, ownership boundaries, runtime lifecycle, and safe extension points. It is the canonical English architecture reference. Behavioral requirements that need explicit invariants live under `docs/specs/`.

Usher is a desktop-first Rust application. Slint owns presentation and declarative UI state; Rust owns persistence, process execution, networking, and application orchestration.

Production builds select Winit with FemtoVG/OpenGL at compile time. Skia and FemtoVG/WGPU are opt-in Cargo features used for renderer diagnostics and A/B comparisons, so normal builds cannot silently change renderer because multiple implementations were linked. Slint's in-process live preview and the UI Components playground are explicit `live-preview` and `ui-components` development features; neither is compiled into default or production builds. The WGPU path is not a production default because current macOS tests reduce physical footprint but increase RSS.

PR Review is exposed through the stable `@pr-review` Slint feature-package
entry, independent of the route that opens it. Default and release builds map
that entry to the complete page. The explicit non-default `dev-shell` feature
maps the whole package to a protocol-compatible empty implementation and
normalizes `PrReview` navigation back to Workflow, so unrelated shell work does
not compile the large review render tree. This is not a cross-crate page split;
all complete-page state, overlays, and interactions remain together.

On macOS, entering or leaving full screen re-aligns native title-bar controls after AppKit's final layout and publishes the native full-screen state to Slint so the breadcrumb can release the removed leading chrome; the traffic-light cluster is shifted 8 pt right onto the sidebar icon centerline, and application-owned controls are always placed beyond those lights while present.

## System context

P6 validation is owned by `scripts/benchmark-p6-regression.py` and opt-in
feature fixtures sharing `src/p6_benchmark.rs` duration control. Historical
migration gates remain separate from current/current regression gates. The
manual performance workflow runs real repeated workloads with cycle correctness,
memory slopes and resource-growth checks; see [protocol](p6-regression.md).

PR highlight completion validation is shared by diff and full-file callbacks in
`controller/highlight_completion.rs`: generation, index, path and display mode
are checked before loading/model writes. Worker scheduling remains unchanged.

PR syntax workers lease a session-local lazy resource slot before being queued
(`controller/highlight_session.rs`). Reset detaches that slot instead of clearing
a shared slot that an old worker could refill. Source/expand completions also
validate generation and file index before clearing loading. An opt-in, offline
`test-fixtures` candidate in `controller/lifecycle.rs` releases derived Diff/Find
presentation on route exit and rebuilds it on return; it retains domain detail,
range/expanded context, source cache and lazy-tab state. It is not the production
default and has not passed native Release performance acceptance. Suspension is
a typed action in the existing PR State Store; remote mutations remain valid.

Normal route hiding retains the current review's syntax resources, token/source
caches and root models. The unadopted syntax-only background-release experiment
has been removed from application code; its source snapshots and measurements
remain in the historical memory diagnostics, not as an active runtime mode.
A controller owns only the current PR cache, not a cache per visited PR.
Replacing a PR invalidates generations, detaches its resource slot, clears
request-scoped caches and replaces root models as new results arrive. Existing
workers finish on their old lease and cannot refill the replacement slot.
Object lifetime tests do not establish an allocator-residency/RSS plateau.

Sidebar navigation to Home is not an explicit review close: Router history can
keep the Workflow scope reachable and the controller/root models alive. Shell
Back closes the review after any commit-tab checkpoint is consumed; unreachable
scope cleanup and project replacement are separate reset boundaries. Do not
infer whole-session cleanup merely because the conditional Slint page unmounted.

Hook journals budget serialized JSONL bytes (2 MiB output admission plus 64 KiB
lifecycle reserve). `hooks/replay.rs` streams legacy/new logs with a 16 MiB read
budget and 64 KiB record limit. The Hook bridge cancels superseded replay and
route-exit replay; completion also checks generation and selected run.
Hook state shares immutable configuration/event catalogs and tracks durable
summary revisions independently of output sequence changes. The automatic
persistence worker skips unchanged durable projections. UI snapshots are
coalesced before `hooks/projection.rs` prepares the selected Job on the shared
blocking pool; one tail parser cache retains unchanged rows. Catalog revisions
gate project-option refresh, while durable revisions also refresh Settings history.
`hooks/errors.rs` owns typed host/validation errors; `hooks/presentation.rs`
maps run semantics to catalog copy and versioned notification metadata. The
Settings/Job bridges re-project on locale changes, retaining log models. Inbox,
Banner and the application native-notification adapter resolve Hook metadata at
delivery; technical diagnostics and external output remain original journal text.
Job log layout shapes bounded UTF-8 pieces and carries incomplete measured lines
to avoid upstream 16-bit cluster-offset overflow on long uninterrupted output.
`jobs/log_model.rs` exposes its concrete Slint model identity so layout requests
share the immutable parsed snapshot instead of materializing all UI rows.
`jobs/log_viewport.rs` retains exact extents and UTF-8 wrap ranges over semantic
prefix/body text; only visible source rows allocate visual-line strings. The
viewport cache uses 16 entries and 16 MiB of accounted owners as soft eviction
targets, not hard heap limits. The current layout callback and every viewport
whose `JobLogWindow.rows` model remains mounted by Slint are pinned; their
combined working set may stay above either target for the whole mount lifetime.
While over target, a 250 ms event-loop timer retries pruning. Once a rows lease
is released, pruning evicts the oldest unmounted viewport and stops when both
targets are met. Viewport drop requests non-blocking worker cancellation.

PR and AI review Hooks share PR title, source/target branch, author label, and
nullable author name/email in their payloads. Git adapters retain explicitly
returned PR author identity in diff details; the PR cache and persisted AI target
carry it to completion without profile requests or commit-author inference.

Application-wide custom Hooks are owned by `src/hooks/`. Feature workers publish
versioned domain events after successful operations. PR Approve and actual merge
are separate events: only Approve emits the former, while merge commit, squash,
rebase, and fast-forward emit the latter only after the provider confirms success.
Comment and Request changes emit neither. AI Review publishes after session
persistence and task completion. `HookStateFacade` hides the shared state-store engine and
automatically persists configuration and bounded run summaries. Separate bounded
JSONL journals preserve ordered lifecycle/output events. Shell and external Node.js
run as cancellable subprocesses on the shared runtime. The independent Settings
page has a Hooks tab immediately before Diagnostics, including unsaved-draft
testing in shared New/Edit dialogs. Its saved-definition list owns a directly
persisted row Switch plus Edit/Delete actions. Code-owned event registrations
also declare a stable module category; the bridge projects those categories into
grouped definition rows and grouped event Select options, so later modules extend
the same layout without UI name matching. The global switch disables row
interaction without losing configured values. Cancel discards drafts, Save closes after persistence, and Delete opens
a confirmation that retains history. Tests show concrete failure details.
New/Edit paints the Dialog before hydrating a saved script after a short UI timer,
then builds a bounded syntax projection after a reusable debounce. One permanently
mounted native `StyledText` supplies Primer syntax colors below an editable
`TextInput`; both use the same monospace font and standard `ScrollView` content
geometry. No per-token UI subtree is created. The styled projection must preserve
the source exactly, otherwise native plain text remains visible. Editing or
changing runtime clears stale colors immediately; closing drops the rich-text
projection. The native input stays on top for pointer/editing ownership. While
syntax colors are shown its text brush retains alpha 1/255, rather than zero,
so FemtoVG still draws the built-in cursor and selection. macOS uses its separate
selection-colored caret; text-colored-caret platforms use opaque native plain
text while focused and restore syntax colors on blur. No highlighter engine or
language cache remains resident.
Its UI bridge owns start/end notification delivery,
deep links and the reusable Job Run projection. Registered asynchronous shutdown
drains finish processes before existing prioritized cleanup, with a shared barrier
for explicit shutdown and event-loop exit. See [Hooks](specs/hooks.md).

Project Add/Edit owns a project-local Hook mode (`hooks_enabled`) and retained
selection (`hook_ids`) in its automatically persisted registry. Legacy records
derive the mode from whether `hook_ids` is empty. `project/hook_selection.rs`
projects the global Hook catalog into a transient form initialized from the
current project on every open. Shared `GitHubRadio` provides the explicit
off/selected mode and progressively discloses the shared `GitHubCheckbox` list
only for selected mode. Disabled rows keep their project-owned checked state but
disable interaction and show the Settings-owned availability status; Settings
changes never rewrite project selection. Hook admission and pre-spawn execution
require both the project mode and selected Hook ID through the Project facade by
normalized Workflow ID. Registered
project-scoped events require an ID; unselected or unresolved projects never run
scripts. Settings draft tests bypass project membership.
The selector presents localized mode captions, divided list rows, readable event
captions, inline group validation and shared status Labels; checkbox hover stays
on the box outline. Off mode does not mount the Hook list.

At compact widths, Settings section navigation uses the shared root-mounted
Select popup. Its trigger remains in the scrolling content, while opening the
option surface never changes the form height or moves the selected settings pane.

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
| `crates/usher-i18n/` | Framework-neutral locale registry and compiled message catalogs. Its build validates the root `i18n/` manifest/PO assets, generates the typed Slint `I18n` global, and stages one index-identical translation bundle for every independently compiled UI root. Runtime locale resolution and Rust copy lookup do not depend on a generated UI engine. |
| `crates/usher-messages/` | Independently compiled message-center package: durable domain/storage service, snapshot subscriptions, background persistence/native-delivery worker, `notify-rust` OS adapter, and generated Inbox page API. Its embeddable Slint page is compiled in library-module mode rather than through the deprecated non-Window root fallback. |
| `crates/usher-search/` | Framework-neutral shared Find engine: Unicode-safe non-overlapping text matches, wrapped adjacent-result navigation, and Chromium/Blink `CenterIfNeeded` viewport policy. Features supply only domain/model and visual-wrap adapters. |
| `crates/usher-state-store/` | Framework-neutral cross-page state port and built-in engine: typed reducers/actions, immutable revisioned snapshots, selectors, immediate hydration, automatic monotonic subscriptions, RAII cancellation, and the worker-owned `PersistentStore` facade with deferred enable, coalescing, flush, and shutdown. Feature facades hide the engine so a community Store can replace it through an adapter. |
| `crates/usher-virtual-list/` | Framework-neutral variable-height virtualization facade: exact item extents, offset/index mapping, bounded overscan windows, and exact item reveal. Its private adapter currently uses the community `virtualizer` engine; features and Slint bridges depend only on the local port so the engine remains replaceable. |
| `crates/usher-preview/` | Framework-neutral artifact descriptors, provider/registry contracts, runtime policy, prepared payloads, and build-time detachable Markdown/image/plain-text providers. Its structure-only Markdown entry preserves blocks and inline semantics without initializing code-highlighting resources. It has no Chat, MainWindow, or Slint dependency. |
| `crates/usher-ai-review/` | Optional framework-neutral AI review domain package: rule/config reducers, dynamic bounded scheduler, review inputs/sessions and prompt provenance, diff chunking, response parsing, and finding aggregation. It has no MainWindow or Slint dependency. |
| `src/ai/` | OpenAI-compatible transport, the feature-owned typed/persistent AI Agent settings Store, plus the sole source-controlled built-in prompt catalog in `prompts.toml`; `prompts.rs` strictly renders typed, bounded AI Chat, AI Review, and Review Ask message envelopes and metadata-only provenance. |
| `src/app_runtime.rs` | Process-wide Tokio runtime with two production worker threads. Runtime threads install the tracing dispatch; an explicit environment override exists for controlled benchmarks. |
| `src/memory_diagnostics.rs` | Process-wide RSS/footprint snapshots and lifetime high-water marks, startup/run metadata, bounded Slint-model accounting, UI-size correlation, and the cancellable periodic memory sampler. |
| `src/diagnostics.rs` | Crash-run sentinel, retention workers, log-directory reveal, and privacy-bounded diagnostic archive export. |
| `src/startup.rs` | Elapsed/delta startup timeline plus the one-shot post-first-frame queue used to defer disk hydration and expensive image projection. |
| `src/platform/` | Cross-platform native integration boundary. `macos.rs` owns AppKit appearance (forced Light/Dark or follow-system for Auto), title-bar accessories, window-interaction notifications, the native-title-bar click bridge for dismissible transient overlays, macOS PR-review font registration, the `NSSound` click, and live Auto theme updates. It also owns paired shutdown removal for its event monitor, window/theme observers, and detached-preview resize observers. `windows.rs` owns Windows theme detection and `MessageBeep`; `linux.rs` owns XDG Portal theme detection and non-blocking desktop sound playback. `mod.rs` exposes the shared API, Auto theme watch enablement with a 1s poll fallback on all platforms, plus opening a directory in the OS file manager (Finder/Explorer/`xdg-open`). |
| `src/shutdown.rs` | Ordered shutdown coordination. Normal cleanup runs UI/AppKit tasks on the event loop; a dispatch-failure fallback skips those tasks and runs only thread-safe pre/post cleanup before process exit. |
| `src/single_instance.rs` | Process lock plus a bounded file-signal activation channel that reveals and focuses the existing window when a second launch is attempted. |
| `src/tray.rs` | Desktop tray integration and tray-driven lifecycle actions. |

`src/lib.rs` is a composition root, not the preferred home for feature logic. Native-window behavior belongs in `src/platform/`, diagnostics accounting belongs in `src/memory_diagnostics.rs`, and new business behavior belongs in a feature module that exposes a small setup or service API.

### Feature modules

| Path | Responsibility |
| --- | --- |
| `src/project/` | Stable façade/controller plus a typed state-store facade, separated persistence models/storage, logo handling, PR/MR overview selection, and project-to-workflow/terminal entry points. |
| `src/codereview/api/` | URL and path templates for Gitee, GitLab, and GitHub repository APIs. It contains no request execution, authentication, or response decoding. |
| `src/git/` | Provider-neutral façade plus separated domain models, remote locator, unified-diff parsing, PR overview counting, and provider adapters. Gitee keeps transport, decoding, conversation/commit normalization, readiness, and revert semantics in dedicated submodules. |
| `src/jobs/` | Application-wide Job lifecycle model, source-independent run descriptor/UI projection, and source-keyed action dispatch. It owns no feature process. |
| `src/workflow/` | Per-project Branch, Tag, PR, and CD state and command execution. Tag and PR Review expose stable file façades with internal controller and pure-logic modules. |
| `src/terminal/` | PTY lifecycle, viewport synchronization, rendering, selection, and project switching; pure input mapping, diagnostics, and refresh throttling use dedicated modules. |
| `src/chat/` | Shared AI/RAG Chat presentation and interaction, mode-isolated sessions/JSONL histories, request elapsed-time ownership, transports, width-independent plain-text/artifact projection, bounded rendering cache, pagination, and pure bounded virtual layout. Full artifact rendering belongs to the shared preview feature. |
| `src/agent_response/` | Width-independent projection for agent-authored Markdown. It owns the 16 KiB/48-block inline budget, the 128-entry/approximately 2 MiB accounted content-identity cache, and opaque repository-Markdown action tokens; it performs no file I/O and does not own a Preview host. |
| `src/ai_review/` | Optional AI review configuration facade, corruption-preserving JSON storage boundary, and application controller. It owns state-store subscriptions, automatic persistence workers, dynamic task scheduling, review/Ask elapsed-time projection, on-demand diff/rule loading, AI calls, in-flight provider cancellation, completed-review and lightweight PR-chat session storage, notifications, and route-independent PR result projection. |
| `src/artifact_preview/` | Application preview facade: provider policy, background preparation, stale-result cancellation, Slint model adaptation, one active session, immediate migration between docked and detached hosts, explicit effective-theme synchronization into the detached component tree, and worker-side canonical containment/read for explicitly activated repository Markdown. Its `test-fixtures` adapter is development-only and submits playground samples through this same facade. |
| `src/update/` | Stable/alpha release selection, streamed downloads, progress, cancellation, installation, and restart; restart releases the process instance lock before launching the replacement. Stable excludes SemVer prereleases, while alpha accepts both stable and prerelease versions. |
| `src/router/` | Route mapping, history, visibility, transition hooks, and route-scope reachability hooks; it does not perform feature cleanup. |
| `src/shortcuts/` | Application command IDs, declarative key bindings, platform-normalized matching, route scopes, conflict validation, dispatch, and display labels. |
| `src/app_lifecycle.rs` | Composition-level lifecycle registrations for Chat transition visibility and Workflow-scope cleanup. |
| `src/settings.rs` | TOML settings model and serialized atomic persistence, including AI Agent credentials and the configurable RAG endpoint. |
| `src/settings_feedback.rs` | Shared settings-save worker completion, saved-state reset timer, and localized toast feedback; the composition root only binds actions. |
| `src/ai/` | Shared OpenAI-compatible AI Agent transport and settings state; `agent.rs` owns connection tests and chat completions used by AI Chat and AI Code Review, while `settings_state.rs` owns the typed automatically persisted configuration facade. It does not own RAG transport. |
| `src/network.rs` | Connectivity ownership and `AppState.network-online`: macOS uses SystemConfiguration reachability callbacks; other platforms use a no-packet route check with offline backoff. |
| `src/http/mod.rs` | The single shared reqwest client, per-request total/body-idle timeout policy, centralized response-body reads, interceptors, error normalization, and optional UI feedback. |
| `src/logger/` | Debug logging, tracing, and panic/error support. The panic hook owns a direct JSON fallback write that remains active when ordinary tracing is disabled. |
| `src/toast.rs` | Transient single-toast manager binding. |
| `src/notification.rs` | Stable queued-notification model, generation-guarded timers, hover pause, actions, durable-message reconciliation, and icons. |
| `src/message_center.rs` | Thin bridge between the root window and `usher-messages`; mirrors theme/viewport state, pushes total/unread/read filter counts, and converts immutable snapshots into isolated-page Slint models. |
| `src/dialog.rs` | Imperative `DialogManager` binding and action dispatch; it owns no visual variant. |

Repository endpoint definitions and executable behavior are intentionally separated: `src/git/` may reference constants from `src/codereview/api/`, while `src/codereview/api/` must not depend on `src/git/`, `src/http/`, reqwest, or serde.

PR Review keeps loading, context and navigation in `controller.rs`; its child
`controller/actions.rs` owns remote review/merge/revert commands and capability
presentation, while `controller/diff_bindings.rs` owns event-loop geometry,
selection, virtual-list and Find bindings. `controller/diagnostics.rs` owns
opt-in scroll/reveal evidence. The stateless `PrDiffBridge` global in
`ui/features/pr_review/diff_bridge.slint` declares that feature's Diff callback
API; `ui/main.slint` routes page events through it and re-exports its generated
Rust API. It holds no shared domain state or persistence.

Chat's `interactive.rs` retains route/request orchestration and timers;
`viewport_policy.rs` owns coverage/window-shift/anchor settlement policy and
`message_model.rs` owns the stable incremental Slint row model. AI Review's
`sessions.rs` owns Review/PR Chat session serialization and interrupted-task
hydration; `controller.rs` schedules those operations and `task_persistence.rs`
continues to own automatic task commit persistence. These leaf owners do not
import their controllers; extraction does not change cache bounds, event-loop
scheduling, generation guards, persisted formats, or shutdown priorities.

The architecture gate retains the original controller/root budgets and also
checks extracted owners and reverse controller dependencies. CI is manual-only;
direct `main` pushes and pull requests intentionally do not start a second
workflow. A pushed `v*` tag is the sole automatic build trigger and is owned by
the Release workflow. Nightly builds one Release binary,
runs the existing three-process Home RSS/footprint gate, then serial Project,
Job, Preview code/paragraphs and Find fixtures through
`scripts/benchmark-hot-paths.py`. Each hot path runs three AB/BA/AB pairs with
identical hashes as repeatability cohorts; absolute RSS, phase growth,
callback latency and fixture-effect budgets prevent relative comparisons from
hiding regressions. An explicit `--baseline` supports before/after comparisons.
Missing/invalid raw samples, incomplete fixtures and failed subprocesses fail
the gate. Logs, per-run state/samples and the consolidated report are uploaded
even on failure. These short, fixed fixtures do not establish long-term memory
bounds or replace manual input/paint validation.

### Slint UI

| Path | Responsibility |
| --- | --- |
| `crates/usher-ui/build.rs` | Compiles the root Slint source tree, maps vendored Material to `@material`, and maps `@ui-components` to the real development playground or a contract-compatible disabled stub. |
| `crates/usher-ui/src/lib.rs` | Includes and publicly exports the generated Slint module. |
| `ui/main.slint` | Root window, route composition, Rust-facing properties/callbacks, modal layers, and terminal docking. |
| `ui/route/routeMap.slint` | Complete `Route` enum. Only a subset is exposed in main navigation. |
| `ui/state/` | Slint global state adapters. |
| `ui/views/` | Route-level page composition only. Active entries use `GitHub*Page` names and import all visual building blocks from `ui/components/`; view files do not declare local controls. |
| `ui/views/job_run.slint` | Application-level `Route.JobRun` composition, reusable from any source route. |
| `ui/views/ui_components.slint` | Route-only composition for the interactive UI Components playground. |
| `ui/views/workflow/` | Workflow route composition and the complete full-window PR Review implementation. Flow, tab, filter, field, and panel components live under `ui/components/workflow/`. |
| `ui/features/pr_review/` | Stable PR Review feature-package entry and public page protocol. Build configuration maps `@pr-review` to the complete implementation or the explicit `dev-shell` replacement; routes are consumers, not owners, so future entry points reuse the same package. |
| `ui/features/ai_review/`, `ui/features/ai_review_settings/` | Optional compile-time packages with full and disabled protocol-compatible implementations. The full review package exposes a PR-side panel, not a standalone route. |
| `ui/components/` | Central ownership for every application visual component: controls, rows, cards, menus, tabs, panels, dialogs, fields, feedback, and feature-specific building blocks. GitHub/Primer implementations use `GitHub*` symbols. |
| `ui/components/artifact_preview/` | Shared application-level preview shell, width-independent Markdown/text/image surfaces, docked panel, and detached native window. It owns no parsing or I/O. |
| `ui/components/agent_response/` | Compact native block renderer for agent responses. It consumes prepared structure and emits link/action callbacks; it owns no Markdown parsing, repository resolution, or file I/O. |
| `ui/components/about/`, `ai_review/`, `chat/`, `project/`, `workflow/` | Feature-owned GitHub/Primer visual building blocks extracted from route views; they own no I/O or route transitions. |
| `ui/components/showcase/` | Interactive GitHub/Primer property matrices grouped by foundations, actions, containment, communication, navigation, selection, text input, then scenarios; a localized directory and return links scroll the existing page host. Muted Primer Box-header bands mark sections; example copy binds to the shared catalog, and Agent response feedback stores action identity so it follows runtime locale changes. The registry links model-dependent feature composites to their live routes. Bordered and borderless PR menus are adjacent in Selection's Select Menu section; the bordered menu uses a live window anchor and the shared Floating UI adapter in the root overlay, keeping section height fixed. Select samples share a constrained column width, stack below the shared small breakpoint, and derive section height from the controls. PR menu samples reuse the production toolbar and `GitHubPullRequestRuleMenu` with local 2/14-rule models, visible viewport bounds, and no persisted selection. Its opt-in Preview fixtures emit fixture ids rather than bypassing providers or hosts. New or materially changed visual components must be registered here. |
| `ui/components/button/github_icon_button.slint` | Shared `GitHubIconButton` foundation for icon-only, floating, terminal, project-card, and copy actions, including keyboard focus, Tooltip, loading, and semantic variants. |
| `ui/components/label/github_counter_label.slint` | Shared typed Primer CounterLabel with default/primary/secondary schemes, optional upper limit, compact overlay geometry, and zero-count hiding; status notification badges and future navigation/action counts reuse it. |
| `ui/components/github_metadata_label.slint` | Shared small Primer Label: transparent background, border-inclusive padding, centered content and typed Default/Primary/Secondary variants; Hook status metadata uses Secondary. |
| `ui/components/github_link.slint` | Shared Primer text-link states and keyboard interaction. |
| `ui/components/github_spinner.slint` | Single application-owned Primer indeterminate spinner used by controls and loading surfaces. |
| `ui/components/github_progress_bar.slint` | Shared semantic determinate/indeterminate progress track for status and update surfaces; determinate fill is anchored to the left edge and grows in width with progress. |
| `ui/components/github_scroll_view.slint` | Application-owned mounted vertical viewport and semantic scrollbar foundation. |
| `ui/components/navigation/github_underline_nav.slint` | Shared keyboard-accessible Primer UnderlineNav/UnderlinePanels shell and item used by Workflow and nested feature tabs; the shell owns the full-width baseline, safe-area inset, and item spacing. |
| `ui/components/github_segmented_control.slint` | Shared compact Primer SegmentedControl shell, items, dividers, and focus/disabled interaction. |
| `ui/components/github_tooltip.slint` | Shared GitHub TooltipV2 contract: call sites mount a transparent anchor locator; `ui/state/tooltip_overlay_state.slint` plus the root `GitHubTooltipOverlayHost` apply shared offset/flip/shift/size placement above clipped scroll/page hosts. |
| `ui/state/tooltip_overlay_state.slint` | Window-root tooltip portal state (text, anchor rectangle, preferred placement/alignment, claim token, lease heartbeat). |
| `ui/components/github_action_menu.slint` | Shared Primer ActionMenu/AnchoredOverlay surface with focus ownership, Escape dismissal, peer exclusive-claim on open, viewport collision placement, bounded vertical scrolling, reusable ActionList rows, and `GitHubActionMenuTriggerArea` press-state open/close so title-bar chrome dismiss cannot dismiss-then-reopen on the same click. Page blank outside-click stays on each host's sibling TouchArea. `ui/state/transient_overlay_state.slint` owns chrome `dismiss-revision` and popover `exclusive-generation`. |
| `ui/components/page/github_page_header.slint` | Shared Primer PageHeader foundation for title, description, leading visual, divider, and trailing actions. |
| `ui/components/button/github_copy_button.slint` | Shared GitHub CopyToClipboardButton interaction for every icon-copy action. It owns the transient Copy→Check state and `Copied!` tooltip; feature callbacks return clipboard-write success. |
| `ui/components/jobs/` | Reusable GitHub Actions-style run, Job, Step, and log presentation. It accepts normalized snapshots and never executes workflow commands. |
| `ui/components/dialog/github_dialog.slint` | Shared responsive Primer Dialog chrome, backdrop input ownership, initial close-control focus, header/body/footer safe areas, Esc dismiss (unless `modal`), close behavior, narrow full-window layout, and action states for global, project, PR-review, and update dialogs. Its optional external viewport keeps route-owned surfaces centered on the complete application window and paints one continuous backdrop across docked shell chrome. Root state continues that backdrop through the macOS title-bar inset; `src/platform/macos.rs` shields application-owned title-bar accessories while leaving AppKit traffic lights interactive. Native-title-bar clicks mirror ordinary backdrop dismissal without weakening non-dismissible dialogs. Feature body content nests as `@children`. |
| `ui/components/toast/`, `ui/components/notify/` | Shared compact Primer Banner-style transient and queued feedback; the root renders one newest-first queued notification Banner at a time, discloses capped-queue overflow against the durable unread total, and keeps batch operations in a compact ActionMenu. `GitHubNotificationQueueMenu` shares queue actions between root notifications and the showcase root portal; inline Banners publish live window anchors instead of painting menus inside page sections. Both hosts anchor shared Floating UI placement to the footer button, with a 6 px gap below or above it. |
| `ui/components/messages/` | Reusable GitHub Inbox message rows with read/unread, semantic level, metadata, and mounted row actions; registered in UI Components. |
| `ui/components/pr_review/` | PR Review public UI structs plus reusable shell, file-tree, and diff controls; cross-region state stays in the complete page implementation. |
| `ui/theme/github_theme.slint` | Application-owned `GitHubTheme` semantic color roles and `GitHubMetrics` Primer geometry, control, typography, overlay, and breakpoint tokens. Raw functional UI colors are centralized here. |
| `material-1.18.0/` | Byte-for-byte `ui-libraries/material/src` from Slint v1.18.0 commit `bd20dab8529add087b5cbc81aec70bf30861ae4c`; no application patches live in this tree. |
| `vendor/i-slint-live-preview/` | Temporary Slint 1.18 override selected only by the opt-in `live-preview` feature. Its documented `mtime` guard prevents spurious macOS watcher events from causing an endless recompile loop. |

Application UI must import Material through `@material`; direct imports from
vendored implementation paths are forbidden. Never modify
`material-1.18.0/` or other vendored Slint Material UI source for
application behavior or appearance. Material palette, typography, and style
metrics do not define the product UI. Application surfaces consume
`GitHubTheme` and `GitHubMetrics`, and every visual adaptation stays in
application-owned code so a dependency upgrade can replace the Material tree
without carrying patches.
Compatibility aliases and input-policy changes belong to application-owned
wrappers such as `ui/components/github_scroll_view.slint`; upgrading Material
is an upstream directory replacement, never a patch rebase.
The current direct-import allowlist is intentionally narrow:
`MaterialWindowAdapter` in `ui/main.slint`, ordinary `ScrollView` usage in the
PR Review workspace, and the public `ScrollView` base of the application-owned
exact-name `ListView` shim in `ui/components/github_scroll_view.slint`. PR
Review, AI Review, Artifact Preview, and Jobs import that shim instead of the
vendor library. Artifact Preview otherwise uses an application-owned
`Flickable` plus the shared exact-extent virtualization facade.
`scripts/check-ui-policy.sh` enforces this boundary and the Octicon command-icon
policy.

All product pages and reusable visual components follow the
[GitHub/Primer UI design specification](specs/ui-design.md). Every
application-owned GitHub/Primer visual component uses a `GitHub`-prefixed
component name. Existing naming and visual debt is migrated under the staged
[UI migration plan](ui-github-migration-plan.md), one user-accepted batch at a
time; discovered defects are appended to [`bug-history.md`](../bug-history.md).
GitHub's current product UI and official Primer documentation/source are the
visual source of truth.

The root shell owns two persistent Primer surfaces: the icon-only route rail in
`ui/components/navigation/custom_nav_drawer.slint` and the runtime utility bar
in `ui/components/status_bar.slint`. The rail owns route selection and tooltip
presentation; the utility bar owns update, notification, connectivity, and
version presentation. Its persistent right cluster is root-anchored rather than
layout-distributed. The transparent host remains in window coordinates while
only its painted surface and transient left lane follow the live content edge,
so opening the sidebar leaves no status surface below it and cannot move or
compress the shortcut, notification, network, or version items. Notification
count comes from the durable message center and is rendered as a bell badge only
when nonzero; activating it opens `Route.Messages`. Shortcut/network health colors come from their
feature state, while notification and version remain non-health indicators.
Right-edge tooltips expand leftward and status popups share one parent
coordinate space. Pages must not recreate either shell surface. See
[specs/status-bar.md](specs/status-bar.md).

The navigation sidebar and its content-boundary decoration are mounted as root
shell layers above ordinary route surfaces, including PR Review, while modal
backdrops remain above them. The docked sidebar uses a clearly visible
full-height 1 px edge plus restrained inward elevation lobes. The temporary
hover sidebar is instead an opaque white border-only sheet with restrained 10 px rounded trailing corners,
Codex's neutral `#dfdfe0` light-mode hairline, and no right-edge shadow. One
interruptible spring moves the sidebar, route surface, title-bar divider, and
status-bar painted inset. The route adopts the target width once when a toggle
begins, so animation does not reflow the mounted page tree on every frame.
During native live resize, the mounted route width remains frozen and the exact
final width is applied when dragging ends; the root and opaque native underlays
continue following the window. PR Review also preserves its stable internal
virtualized diff geometry during that interaction.

The application-level docked artifact preview is a root-shell pane aligned to
the window's top edge and separated from route content by one continuous 1 px
hairline. One 260ms animated progress value controls both its right-edge clipped
width and the route's reclaimed width. The complete pane stays right-anchored,
and Preview/Chat document layout remains stable while the clip moves, so opening
and closing do not reflow either heavy tree per frame. The feature facade drops
its prepared session immediately; the read-only Slint projection survives for a
bounded 280ms exit and is then cleared. FemtoVG replays descendants when an
ancestor clip moves, so the docked host suspends only document painting during
the 260ms shell transition while retaining the prepared model. The lightweight
spinner and parser-loading label remain visible during that transition. At rest the
Markdown surface uses the exact-extent `usher-virtual-list` facade; only the
bounded visible/overscan block window is instantiated, which bounds the one-time
reveal cost without changing provider or session ownership. Clearing the visual
projection also resets the feature-owned virtual rows, extents, projection
guards, and Parley font/layout caches while preserving the stable Slint model
identity used by the root callback. A preview-owned resize-handle component owns
the stable hit target, cursor, hover feedback, and pointer-cancel lifecycle; the
root shell owns the bounded session width and applies it symmetrically to the
preview and route panes. The complete docked-divider hit target sits above the
preview host, so either side captures the first press on every owning route;
modal overlays disable the handle.
Settings selects split (`docked`, default), floating (`floating`), or detached
preview presentation. Split always reserves a column and adjusts route/preview
widths together; floating leaves the route width unchanged. Pane minimums adapt
to narrow windows without switching modes. PR-context previews retain their
dedicated AI side-panel slot for both in-app choices. `artifact_preview/presentation.rs`
owns the typed preference Store, ordered runtime actions, automatic worker
persistence and shutdown drain; the UI subscribes through the feature facade.

Preview resize has a separate geometry contract from provider rendering. During
a docked-divider drag, the root shell updates the visible pane boundary and
clips the Preview surface, while the shared Preview panel keeps its prepared
document tree at the drag-start layout width. During detached macOS live resize,
an object-filtered AppKit observer supplies the same host-local state; a bounded
window-local debounce covers backends that publish width changes without
native begin/end notifications. Neither path reuses the main-window resize flag.
The macOS detached host keeps the AppKit document title and mounts its
capability-driven Copy, External Open, and Close controls in one trailing native
title-bar accessory. Its shared Slint panel disables the otherwise duplicated
content Header. The accessory is installed once per native preview window and
only reflows when a prepared payload changes capabilities, so it does not enter
the document scroll, animation, or live-resize render paths. Other platforms
retain the shared content Header and its controls.
The platform adapter also restores an opaque native window background and standard
title bar after creation/reopening, and synchronizes native appearance on effective
theme changes. Slint's content background alone does not cover AppKit chrome.
The detached host commits its initial logical size before its first native map,
so the backend never presents an intermediate default-size frame. Subsequent
payload replacement keeps the current user-resized window geometry.
Its first rendering callback gates native presentation at alpha zero until the
opaque appearance, title-bar accessory and prepared content frame have settled;
the normal render and payload-update path does not re-enter that gate.
Releasing either gesture commits the exact final content width once. Markdown,
image, and text providers remain unaware of host shape and resize lifecycle.
The native Markdown surface uses an independent `MarkdownParserLoader` and a
language-parser registry. `OnDemand` is the production default and
`PreloadAll` is explicit. Rust, Shell, JavaScript/JSX, TypeScript/TSX, JSON,
Python, and Go use independently cached Tree-sitter configurations initialized
only for the selected language family. Unsupported or omitted fence languages
share one source-preserving plain-text parser; the Preview path contains no
complete syntax-repository fallback. Loader initialization and highlighting stay
on the Preview worker. Slint `StyledText` handles inline prose, direct native Slint
`Text` rows render colored code tokens, and the surface uses official Primer
visual tokens with the same Codex document rhythm as compact AI Review responses:
a 14 px body/22 px line box, 24/20/17/15 px headings, 20 px paragraph gaps,
10 px prose/list-group gaps, and zero extra space between consecutive list rows.
H1-H6 do not add automatic rules; quote insets and subtle code surfaces share one
Slint typography token set. For prose, the Parley bridge mirrors the settled
width, font family, size, inline styles, fixed body rhythm, and glyph safety allowance.
This prevents mixed-font rows from being clipped or diverging from virtual extents. Fenced code
retains its language and carries prepared light/dark token projections without
an intermediate Markdown/HTML conversion; theme changes do not repeat parsing
or highlighting. No WebView/browser runtime is
part of the host.
Cold parser cache misses publish `ParserLoading` and `ParserReady` progress.
The application facade maps these to the shared spinner label through a weak
window on the Slint event loop and checks the request generation both before and
after scheduling, so canceled work cannot overwrite a newer Preview.
The provider worker also prepares immutable measurement input. `measurement.rs`
measures blocks on the shared blocking pool, keyed by document revision, settled
width and font family. `app_runtime/latest_compute.rs` retains one active
calculation, one replaceable input and one output, cancels stale generations
between blocks, and drains active work during shutdown. `virtual_layout.rs`
keeps the last exact layout until a current result arrives; host-local epochs
refresh layout without resetting the document or scroll position. New documents
reuse the preparation indicator until measurement completes. Both hosts use the
same contract. `projection_model.rs` lazily converts at most 32 cached blocks
and 128 cached code lines per converted block; fences keep their full geometry
while mounting visible lines plus four lines of overscan on each side. Copy
still uses the complete source. The bridge supplies one exact extent model to `usher-virtual-list`, the Preview
`Flickable`, and the application-owned Primer scrollbar. Ordinary scrolling
reuses one stable `VecModel` while the viewport remains inside the exact
three-block overscan guard. Crossing that guard incrementally removes and appends
only boundary rows; it never replaces the model or increases the bounded mounted
row peak. Slint replaces the projected-window value only when its revision
changes. Thumb height therefore stays constant, normalized thumb travel maps to
the same normalized document offset through the entire document, and paragraph
boundaries do not cause per-wheel-event delegate reconstruction.
Frozen Preview content hides its scrollbar until the settled trailing edge.
Root scrollbars beneath macOS full-size content keep their draggable tracks below
the native title-bar inset so AppKit cannot steal thumb drags for window movement.

The same Markdown parser core exposes a separate structure-only entry for compact
agent responses. It runs on a worker without constructing or consulting a
`CodeParserRegistry`; fenced blocks retain their language and source text while
their highlighted `code_spans` stay empty. `src/agent_response/` uses a
Codex desktop conversation presentation with the platform system font, a 14 px
body/22 px line box, 24/20/17/15 px headings, exact paragraph/list-group rhythm,
and a separate 24 px hanging-list marker gutter. Code rows use the
platform-registered monospace face
and a worker-projected line count for stable minimum height. Non-fenced blocks
normalize one residual model escape layer before Markdown punctuation after
block assembly; fenced source and link targets remain unchanged. The layer then
applies a width-independent 16 KiB/48-block projection budget and a content-identity cache
bounded to 128 entries and an approximate 2 MiB accounted projection-payload
budget, excluding container and allocator overhead. The same worker prepares Slint `StyledText`;
the event loop only materializes `ModelRc` values and commits them. The newest
32 assistant rows retain full structure, the preceding 96 use one marker-free
plain projection plus the Preview action, and older rows remain raw. These limits
bound structured projection/cache work, not persisted history or total Slint row
count. Assistant rows consume the structured blocks; user rows and the
same-binary benchmark baseline keep the raw-text path. A repository `.md` or
`.markdown` reference is never sent to Slint as a trusted action target: the
projection replaces the link destination with an opaque action token and retains
the token-to-relative-path mapping in Rust.

An AI Review response Preview is bound to the review task's repository context.
Activating an opaque repository action resolves and reads the file on a worker.
On Unix, traversal starts from one opened canonical-root descriptor and rejects
links in every relative component; Windows verifies containment from the final
paths of the opened root and target handles. The verified regular-file handle is
size-bounded and read once before it is
submitted to the common Preview provider pipeline. In Docked mode the PR shell
uses its existing trailing slot: the one application Preview host moves into
that geometry and temporarily replaces the
AI Review surface without reserving the width twice or clearing the conversation.
Closing that Preview restores the intact conversation. Detached mode remains an
independent native window and leaves the AI Review panel mounted. PR Dialogs stay
above either docked surface.

Closing the AI Review panel or leaving `Route.PrReview` clears the inline response
projection, opaque-action map, and task repository binding. It also dismisses an
active PR-context Preview only when that Preview is Docked; an already Detached
Preview remains available as an independent application window. Closing only the
Docked Preview does not run AI Review cleanup. AI panel close and PR-route exit
cancel the current Ask so a late completion cannot enter a reopened mount; the
independently scheduled Review task continues.

`src/platform/` selects one clear CJK-capable application font at startup:
the native `.AppleSystemUIFont` on macOS, `Microsoft YaHei UI` on Windows, and `Noto Sans`
elsewhere. `ui/main.slint` exposes that choice as the default for the complete
ordinary UI tree together with Codex desktop's default weight 445; page,
shared-component, and Slint `StyledText` code inherit those window values instead
of selecting independent faces. Terminal content remains fully independent so its existing
cell and toolbar metrics do not change. Within PR Review, only diff source code
and file-tree content keep the platform-selected specialized faces required by
their geometry; all other review controls and prose inherit the application font.

The UI source remains under the repository-level `ui/` directory. The main
window and ordinary route tree are generated by the `usher-ui` workspace crate;
the Settings and Messages routes are generated independently by
`usher-ui-settings` and `usher-messages` in library-module mode, then mounted
through Slint's experimental `ComponentContainer`/component-factory boundary.
Settings declares a transparent Window root embedded by the factory, without
opening another native window. This avoids the opaque style background of Slint's
implicit Window wrapper. The application crate re-exports
the main public API and owns explicit bridges in `src/settings_page.rs` and
`src/message_center.rs`. A component created by a
cross-crate factory has its own Slint global graph, so the bridge must copy the
initial `AppState`, forward callbacks to `MainWindow`, and mirror later state
changes rather than assuming globals are shared. Messages inbox rows report an
action enum and anchor rectangle through the dedicated factory-local
`MessageTooltipOverlayState`; one page-owned `GitHubMessageTooltipOverlayHost`
maps those enum values to separate literal surfaces. No label string crosses a
list delegate. Settings mounts a local generic `GitHubTooltipOverlayHost` for
shared controls; that host remounts its text-measurement surface for each claim
token. Settings and Messages each lazily create one stable factory product after
their first visit. Slint 1.17.1 can reuse the wrong control text or placeholder
when a cross-crate product is destroyed and remounted, so route exit only hides
these hosts; feature bridges own the separate state-reset lifecycle.

The Settings factory host occupies the root window above the status bar; its
ordinary ScrollView retains the content pane's origin and viewport dimensions.
`src/settings_page.rs` mirrors both geometries and Hook dialog visibility to the
main window so native chrome participates in modality. Dialogs and their picker
overlays share the factory's Hook draft globals and cover the whole window.
The global hover sidebar and its edge trigger sit above this factory, including
page-local sticky navigation and menus. Modal visibility hides the hover sidebar
and disables its edge trigger while a dialog is open.
Global notification/Toast hosts paint above Dialog surfaces and scrims so Hook
test feedback remains visible. On macOS, modal entry captures only the native
92x28 pt sidebar/history control strip at its backing scale. A display-only Slint
image at the original window coordinates composites below Dialogs and scrims,
so buttons remain visible through the backdrop and are covered by opaque dialog
content. Native drawing is suppressed while this image is active; the input
shield remains mounted. Native layout updates mirror its geometry. Closing
releases the image and restores native controls with current navigation state.

Within a Settings factory product, Slint 1.17.1 selector bodies must use fixed
mounted leaves and select them with `visible` plus an explicit active height.
Do not recreate mutually exclusive section or provider forms with `if` nodes:
repeated tab changes can otherwise retain a sibling's label,
placeholder, value, or selected state. Route-local selector indices start at a
sentinel and transition to their default on the next UI tick so a newly built
tree invalidates cached bindings. A selector label, its selected appearance, and
its displayed fields must derive from the same state source. This rule covers
the Settings section menu, Git hosting providers, AI Agent provider presets, and
Hook runtime selection. Hooks use shared mutually exclusive `GitHubRadio`
controls and keep literal Shell/Node interpreter leaves mounted. The bridge
owns isolated temporary language drafts and projects only the selected source and
interpreter for Save/Test; durable definitions still own one language/source.
Edits invalidate test feedback. Hook action tooltips use literal leaves and fixed
surfaces at the Settings factory root (UI-103). Tooltip text is centered, and
Hook tips center over their anchor unless viewport collision handling shifts them.
The history clear operation removes
completed summaries/journals on workers while retaining active runners. Add/Edit
opts into shared Dialog primary-modifier+Enter saving, with plain Enter reserved
for source newlines and Escape for cancellation. The root shortcut boundary
forwards modal keys through `SettingsPageBridge` when focus is outside the
factory; macOS normalizes Return/Escape at its existing native input monitor.
The fixed test area shows a compact status and inline
error details below its actions. New tests use temporary directories; legacy
project/outcome constraints remain compatible without editable filter inputs.

Every Tooltip addition/change must first follow [the Tooltip implementation contract](specs/tooltips.md),
including factory isolation and native paint verification. This is a mandatory
repository entry gate in `AGENTS.md`.
The compact section menu uses one fixed trigger leaf per section and snapshots
the active section whenever it opens, so both its collapsed label and selected
row survive factory reuse without retaining the previous route's section.
The interface-language and AI reply-language pickers follow the same fixed-leaf
rule without hard-coding a locale list in application UI. The
`usher-ui-settings` build script reads the validated `usher-i18n` locale
manifest and generates one fixed trigger and option leaf per locale. Adding a
locale therefore remains a manifest/catalog operation, while no runtime
repeater delegate can retain another locale's label, id, or selected state
across factory products. On every menu open, the picker snapshots the current
canonical selection into picker-local state; option styling reads that snapshot
instead of a global binding that the real renderer may leave stale while the
option surface is hidden.

Both language pickers use the shared `GitHubSelectPopup`/`GitHubSelectOption`
form-control foundation. Only the 40 px trigger contributes to form layout;
the Settings factory root mounts the overlay outside the scrolling form.
Select triggers toggle their own menu: a second activation closes without
committing a value and restores trigger focus. Hook event/history triggers use
`GitHubSelectState.toggle()` for the same contract.
`GitHubFloatingOverlayHost`, also used by Tooltips, owns flip/shift and
pixel-aligned placement; Select bounds the width and available vertical height,
while `GitHubActionMenu` provides the surface, scrolling, and focus lifecycle.
Option leaves use the explicit menu-content width and a zero horizontal origin,
not the outer factory width. Native
`PopupWindow` must not be used from the isolated factory product because its
window context is separate from the visible host. Escape cancels, arrow/Home/End keys move the active option, and
Enter/Space commits it. Options use a leading Check rather than navigation-row
selection styling. UI Components exercises the same popup with an adjacent
field and a disabled trigger, with its overlay mounted at `MainWindow` root.
The grouped showcase trigger resolves its anchor from the zero-offset plain
trigger's viewport origin plus its own local offset, avoiding duplicated
component offsets in Slint 1.17.1's inherited `absolute-position`. Both showcase
menus scope active-option focus to their open owner.
There is no full-page input scrim. The existing window pointer observer reports
outside releases after the target handles its click; the Settings bridge forwards page-local
coordinates into its isolated global graph. The Select excludes its trigger and
resolved surface from dismissal, forwards native/chrome dismissal as well, and
does not restore trigger focus after an outside click. The full-window keyboard
scope disables pointer focus acquisition, so other controls act on the first
click. Escape and selection retain trigger focus restoration.

The window pointer bridge restores text-field blur semantics across ordinary and
independently generated page trees. On a primary press, it clears a focused
`TextInput` only outside its visible field bounds, including the clipped field's
padding. `src/transient_input.rs` walks down the mounted item tree, carrying local
pointer coordinates through container offsets, transforms and viewport clips;
mapping upward from an independent factory product can omit its host offset.
This remains reliable when a `Flickable` consumes blank presses and preserves
first-click focus transfer. macOS queues the same classifier from its local
mouse-down monitor; other platforms use the post-dispatch window event hook.
Mouse-up does not clear text focus, so dragging a selection outside the field
preserves keyboard focus.

Credential, endpoint, and AI Agent text fields use page-local edit ownership
while Settings is open. The code-hosting groups commit their complete drafts only
through their visible Save commands. AI Agent additionally owns one committed
typed Store snapshot for consumption by Chat and Review. Selecting a preset
updates Provider, Base URL, and Model atomically in the isolated page draft;
other edits likewise do not mutate `MainWindow` or the Store. Save dispatches
the complete draft as one typed action through `src/ai/settings_state.rs`, then
the bridge projects the normalized committed snapshot into the page and
`MainWindow`. The changed commit is queued to the Store-owned persistence worker,
which writes `[ai.agent]` through the existing atomic TOML boundary, and success
feedback waits for an explicit flush. Every route entry overwrites these
page-local drafts from the last committed host/Store snapshot before the stable
factory host paints, never from independent selector fields. Thus leaving and
returning without Save discards edits without remounting the visual tree.

Messages likewise keeps its factory root and page shell stable after the first
visit. `MessageSnapshot::page` filters the immutable history before selecting a
20-record page; `src/message_center.rs` converts only that page to Slint rows.
Total/unread/read counts still describe the complete Store. Filtering resets to
page one while preserving scroll position, clamping only when the filtered
content is shorter than the current offset. Explicit pagination resets scroll
to the top; deletion clamps an invalid page to the new last page. The page
owns only its filter/page/scroll presentation, with Primer pagination below the
list. Per-row borders form one outline with four 6px corners; rounded clipping
is limited to the first and last rows, never the full list texture. The bridge
retains 20 lightweight delegate slots and updates their records individually to
avoid stale factory text bindings when delegates are recreated. Unused slots
have zero height and empty payloads. On exit the bridge clears every payload and
queries the latest snapshot before reactivation, so an inactive Inbox retains no
projected message data. Rebuilding the page shell per
visit was rejected after a release A/B showed no material RSS saving and mixed
physical-footprint regressions. Both factory pages reuse the
same anchor-based offset/flip/shift/size implementation as `MainWindow` without
attempting to share a global instance across the factory boundary.
Changing `settings.slint` or
`messages.slint` must not regenerate or compile `usher-ui`; changing the main shell
still rebuilds the main generated crate. When `SLINT_LIVE_PREVIEW` is enabled,
both isolated build scripts deliberately keep factory products static because Slint 1.17's
`ComponentFactory` cannot accept the live-preview dynamic handle; the main
window and UI Components route remain live-reloadable.

Settings keeps its wide navigation sticky with a 16 px viewport top inset,
bounded by the navigation column's bottom. `ui/views/settings.slint` derives the
offset from the existing scroll viewport without rebuilding navigation nodes or
adding a second scroll chain. Switching wide navigation sections aligns the
section top with the sticky navigation's 16 px inset, leaving the page title
above the viewport. Short sections retain enough scroll extent for this anchor.
Compact navigation remains in the document flow with its
existing scroll behavior.

Hook picker overlays position each option at its index times the shared
32 px row height so history and event choices have separate hit targets.
Each selector keeps its own mounted model subtree. The shared Select chooses
above/below from available viewport space before floating placement; one plain
option occupies 40 px including the two 4 px padding edges.
Settings passes the pane's viewport origin, including the Flickable offset,
to Hook controls; opening adds the trigger's local layout position before
collision checks rather than using stale factory absolute-position values.

An independently mounted route owns one viewport and one scroll chain. It must
be a sibling of the shell `Flickable`, receive the exact host width and height,
paint `GitHubTheme.canvas-default` explicitly, constrain content to the visible
width, and avoid stacking shell and page padding. Shell-only reserved geometry,
including the ordinary 4 px page-stack bottom spacer, must be removed from the
isolated route's viewport. These rules prevent nested wheel capture, clipped
bottom content, apparent horizontal gutters, and transparent pages exposing a
different shell canvas.

### Isolated page state bridge

Every cross-crate `ComponentFactory` page follows one bridge contract. Host or
domain code owns truth; the page only displays mirrored state. Bridges push
explicitly through `page.set_*` or a snapshot apply helper. Slint `changed`
handlers may act as a secondary host→page mirror, but completion and domain
updates must not rely on them alone—`changed` is edge-triggered and misses
already-equal assignments such as `false→false`.

Two sync modes share that contract:

| Sync mode | When to use | Reference |
| --- | --- | --- |
| Snapshot push | Durable list/count/domain state | Messages: `MessageCenter` → `apply_page_snapshot` in `src/message_center.rs` |
| Operation dual-write | Async command busy or short-lived feedback (`checking`, `testing`, saved flash) | Settings: update check / AI Agent test via `SettingsPageBridge` |

Common rules:

1. Durable domain data belongs to the Rust feature or domain crate; the page
   displays a mirrored snapshot or copied `AppState`.
2. Async operation busy flags belong to the host feature module on the Slint
   event loop. Isolated-page click handlers forward command callbacks only and
   must not assign those busy flags themselves.
3. Purely local UI (selected section, filter chips, reveal toggles, confirm
   dialogs) stays page-owned and is not synced.
4. Operation begin/end dual-writes the host property/global and
   `*PageBridge::set_*(value)` on the event loop, on every success/error/cancel
   path, before or with Notify/Toast feedback.
5. Factory create-time sync restores the current host busy flags and domain
   snapshot. Stable factory pages that own drafts also restore the committed
   snapshot before every route re-entry.
6. All page writes from workers use `slint::invoke_from_event_loop`.

Future isolated pages reuse the same checklist and pick snapshot mode,
operation mode, or both per state kind. Do not assume globals are shared across
generated crates.

### Cross-page automatic state stores

Any State consumed by more than one route, page, status surface, or background
publisher is owned behind the `usher-state-store::StateStore` port and a stable
feature facade. Typed Actions are the only mutation path. A changed reduction
installs an immutable snapshot with a monotonically increasing revision and
automatically notifies subscribers; a new subscriber immediately hydrates from
the current snapshot. Subscriber revision guards prevent delayed callbacks from
overwriting newer UI state. Slint bridges remain event-loop adapters and never
become Store engines.

Durable Store owners use the reusable `PersistentStore` facade. Each changed
revision is queued to one worker, adjacent pending snapshots collapse to the
latest State, and flush/shutdown preserve the application close boundary;
reducers and the Slint event loop perform no file I/O. Deferred stores enable
persistence after typed startup hydration so post-first-frame loading does not
rewrite the loaded snapshot. Concrete engines, including the built-in
`LocalStateStore`, stay behind the trait. A future community Redux/Pinia-style
library is accepted only as an adapter that preserves the same Actions, feature
facade, schema, revision/hydration contract, and tests. See
[specs/state-store.md](specs/state-store.md).

## Build artifact policy

Workspace-root Cargo profiles own the build-artifact policy. Everyday `dev` and
`test` builds use line-table debug information with `split-debuginfo = "off"`
and keep incremental compilation enabled. On macOS this overrides Cargo's
full/unpacked debug default, retaining file-and-line backtraces without
accumulating loose `.rcgu.o` files in `target/debug/deps`.

The `dev` profile compiles non-workspace dependencies at optimization level 1.
The generated `usher-ui`, `usher-ui-settings`, and `usher-messages` workspace
crates use the same optimization level without debug metadata. Application-owned
Rust crates remain opt0 with line tables, so ordinary `cargo run` keeps source
breakpoints and application panic locations while avoiding the largest Debug
code-residency cost. This policy changes development artifacts only; test and
release profiles retain their independent settings.

Full LLDB variable/type inspection is explicit through the `dev-full` profile.
It inherits normal development behavior but emits full, packed debug
information into its own profile directory. This keeps heavyweight symbols
separate and allows `cargo clean --profile dev-full` to remove them without
discarding normal development artifacts. Release and dist profiles remain
independent: release builds disable debug information and incremental
compilation. The isolated `release-fast` profile is for routine optimized local
builds only: it inherits the release runtime policy but uses Thin LTO, eight
codegen units, and incremental compilation. Release automation continues to use
the unchanged `release` profile.

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
2. Create `MainWindow`, initialize empty message/task facades, and initialize debug/tracing loggers. `[Startup][Timeline]` records elapsed and previous-checkpoint delta milliseconds, buffering pre-logger checkpoints until the logger is ready.
3. Apply settings and configure updater state.
4. Bind theme, routing, navigation, application shortcuts, and window callbacks.
5. Install the durable message-center facade and isolated-page bridge without reading its JSON, then initialize notification and toast managers; queued Banners mirror into the center.
6. Bind the shared HTTP module to UI feedback, then start network monitoring with an optimistic initial state while the first probe runs on its worker.
7. Initialize dialog, project, chat, terminal, and workflow callbacks with empty startup models.
8. On macOS, install title-bar accessories and Menu vibrancy before `show()` when the native window handle is already available; retry after `show()` if needed. The winit window is created only after the event loop starts, so a first-render hook hides the window and installs native chrome in the same transaction as the first frame. A short poll then keeps retrying installation until the native chrome is confirmed installed, waits a few settle ticks so the effect's first (unblurred) behind-window sample commits and AppKit's post-accessory layout reset is realigned while invisible, re-aligns the traffic lights, then reveals the window; a deadline guarantees the window always appears. The first visible frame is therefore the final shell rather than a staged transition.
9. After the first render completes, schedule one next-turn handoff that loads Project JSON, decodes projected logos, hydrates durable messages, and hydrates AI Review task state on blocking workers. Typed hydration Actions merge any data created during startup before automatic persistence resumes.
10. Install tray integration and run the Slint event loop. A second process writes only a nonce to the activation channel; the primary process reveals and focuses its existing window on the event loop.
11. On close/quit, coordinate terminal, updater, tray, runtime, and process cleanup through shutdown priorities. Main-window creation, `show()`, native-input installation, and event-loop failures use a native startup alert and terminate with a nonzero status instead of panicking.

Do not move modules earlier if they depend on generated Slint globals or on callbacks installed by a preceding module.

Native sidebar readiness is explicit UI state. Before AppKit finishes mounting
vibrancy, the root window and docked sidebar render opaque theme fallbacks.
Once mounted, the effect and native canvas are sized from their full-size parent
bounds after style changes and on native resize, so Menu material includes the
title-bar strip rather than inheriting the shorter pre-full-size render frame.
The Slint window starts with no visible standard title. macOS does not mount
substitute Slint controls while the native title-bar accessory is pending; only
the final AppKit sidebar and history controls appear in that region.

Debug builds enable process memory diagnostics by default; the persisted Settings control can disable them at runtime, while release benchmarks can force them with `USHER_MEMORY_DIAGNOSTICS=1`. `[Memory][Diag]` records current/peak RSS and physical footprint, previous/startup deltas, process-lifetime high-water marks, run/scenario identity, and the latest route, logical/physical window dimensions, scale, and Terminal visibility. It also retains VM categories, aggregate macOS malloc-zone in-use/reserved/slack ownership, regions, Mach ports, threads, faults, and page-ins. `[Memory][UI Models]` distinguishes AI Review source rows/blocks from retained or virtual delegate creation counts, distinguishes mounted content from painted content, and correlates the contextual Preview host; `[Memory][UI]` correlates settled route/window changes, while feature-owned logs such as `[Workflow][PR Review][Memory]` report bounded model/cache ownership without exposing source content or credentials. The sampler uses the shared runtime and is stopped by the normal pre-shutdown pipeline. `scripts/benchmark-memory.sh` owns the macOS release measurement protocol and aggregates external RSS/footprint samples. Only when `USHER_MEMORY_WINDOW_CONTROL=signal-file-v1` is explicitly set, a benchmark-only signal worker reads the per-run command file and marshals AppKit resize/fullscreen requests onto the Slint event loop; its signal handle is closed by pre-shutdown. The external Swift helper only reads CoreGraphics window state to verify each transition and record independently observed fullscreen dimensions. Benchmark metadata names the application variant and binary hash; the comparator accepts exactly one changed renderer, application-ownership, or Tokio-worker dimension while rejecting all other fixture/protocol mismatches. `.github/workflows/nightly-performance.yml` retains the complete performance gates, but its hosted schedule is temporarily paused and only `workflow_dispatch` can start it. When invoked, it runs the static Release Home protocol in three isolated processes. `scripts/check-memory-benchmark-thresholds.py` requires the expected route and enforces absolute RSS/footprint peak budgets plus median cycle-growth budgets before the raw evidence is uploaded.

## Concurrency model

- Slint objects and globals are event-loop-owned. Background tasks must use `slint::invoke_from_event_loop` before updating them.
- Async I/O uses the shared Tokio runtime from `src/app_runtime.rs`; do not create a new runtime or a new reqwest client per operation.
- Existing blocking Git, PR-statistics, and filesystem work runs on dedicated threads or Tokio blocking tasks. Never block the Slint event loop.
- Shared Rust state uses `Arc<Mutex<_>>`, `Arc<RwLock<_>>`, atomics, or per-thread caches depending on ownership.
- Long-running actions need cancellation or re-entry protection. Update downloads, history pagination, workflow runs, and terminal sessions are reference patterns.

## State and persistence

`crates/usher-state-store/src/latest.rs` provides `LatestPersistence`: one active
write and one replaceable pending snapshot. Store revisions reject stale input;
flush captures the latest accepted revision and waits for that revision or a
newer successful write. Errors remain observable and flush returns failure.
Shutdown rejects new submissions, drains the accepted state, and joins once;
concurrent/repeated shutdown is safe. These semantics apply to state snapshots,
not lossless events or audit records.

AI Review task persistence keeps its feature policy in `task_persistence.rs`
and uses the same bounded writer. It rejects older Store revisions but permits
an explicit repeat of an unchanged hydration revision after writes become enabled;
a separate monotonic submission revision covers that write in the barrier.
Temporary-file cleanup runs before the first submitted snapshot is processed,
and successful writes still reconcile session files. Disabled/failed writes
cannot lose the shutdown barrier.

`app_runtime/latest_ui.rs` coalesces replaceable view snapshots using
`LatestDelivery`, with one pending value and one queued Slint callback. The Inbox,
unread badge, AI Review task/current-result, PR-badge and configuration bridges
use this helper. Project owns its generation-aware preparation pipeline. Ordered
banner deletions, user actions and incremental event streams retain their own
lossless delivery semantics.

### Settings

`[diagnostics]` owns `application_console_logging`, `application_file_logging`,
`tracing_logger_active`, `terminal_verbose_logging`, and `memory_diagnostics_logging`.
`[update]` owns `auto_update` and `update_channel`. Loading migrates legacy root
keys per field, with explicit section values taking precedence. Missing fields
retain existing defaults; the next save writes only the nested sections.

`AppSettings` is persisted as TOML in the platform config directory. It contains theme mode, the `auto` or canonical locale-id language preference, auto-update, independent console/`usher.log`/`usher_tracing.log` destinations, Terminal verbose and memory diagnostics, UI sound-effect preference, navigation drawer state, Git platform tokens, plus per-provider enterprise API URLs and names under `[git.platform.<provider>]`, OpenAI-compatible AI Agent connection values under `[ai.agent]`, and an optional HTTPS RAG endpoint under `[ai.chat]` (with `USHER_RAG_API_URL` as the environment fallback). AI Agent edits are reduced and auto-persisted by its feature Store; other settings retain their existing immediate or explicit-save entry points. Saves are serialized, write and sync a unique same-directory temporary file, and atomically replace the destination. A parse failure preserves the source as `settings.toml.corrupt-*` before defaults may be saved; if preservation fails, settings writes remain disabled for that process. Logger destinations use runtime atomic filters; `usher.log` opens lazily on the first enabled write, and Settings exposes GitHub-style `Enable all` / `Disable all` actions while retaining every independent state. Gitee stores public `token` and `enterprise_token` separately. A legacy single Gitee token is migrated to the enterprise field only when an enterprise API root was already configured; credentials never fall back across profiles. `AppState` mirrors values required by Slint. API keys, enterprise names, and configured URLs are local configuration and must not be logged. Provider API code uses an enterprise API URL only when its host matches the repository remote; users may supply the host or the complete API root, with the provider version path added if omitted. Public-provider defaults remain remote-derived. Auto theme resolves through AppKit on macOS, the Windows per-user application-theme setting, and the XDG Desktop Portal on Linux. While Auto is selected, Usher watches OS appearance changes: macOS uses `AppleInterfaceThemeChangedNotification` plus a 1s poll fallback; Windows and Linux poll the same detectors every 1s. Selecting Auto clears any forced AppKit appearance so the process can track the system; explicit Light/Dark lock Aqua/DarkAqua. When UI sound effects are enabled, breadcrumb Back uses `platform::play_ui_click`: macOS plays `Tink`, Windows queues the user-configured default sound, and Linux plays the desktop sound-theme event on a worker thread with common PipeWire/PulseAudio/ALSA command-line fallbacks.

Within `[ai.agent]`, `reply_language` is either `auto` or a canonical locale id.
`auto` resolves from the active UI locale at request assembly time; the setting
applies to AI Chat, AI Review, and Review Ask, never RAG. Unknown persisted values
normalize to `auto`, and the isolated Settings factory mirrors both the selected
id and manifest-driven options through its event-loop bridge.

The root `i18n/` directory is the only translation asset source. Its manifest
drives locale discovery, native labels, aliases, fallback, and coverage gates;
its semantic message catalog drives generated Slint properties and Rust lookup.
After selecting a bundled locale, the application explicitly marks all Slint
translations dirty so independently compiled MainWindow, Settings, and Messages
roots repaint their existing `@tr` bindings in the same event-loop turn.
`src/i18n.rs` applies the selected bundled locale on the Slint event loop before
factory pages are created. Because Slint keeps one bundle per runtime context,
MainWindow, Settings, and Messages compile the same complete generated catalog
instead of owning page-local `@tr()` indexes. Rust-owned dynamic state retains a
semantic key, structured kind, or `LocalizedText` until its UI projection is
built; locale changes therefore re-project mounted Preview, Terminal, Project,
Chat, Workflow, Hooks, and AI Review state without text matching. Provider, PTY, and
filesystem details remain in logs while the UI resolves stable catalog errors.
Rust-only messages set `slint = false`: they remain in the semantic and PO
catalogs without entering or perturbing Slint's live UI translation indexes.
Persisted application-authored Chat errors are the compatibility exception: the
renderer recognizes their exact value across all registered locale catalogs and
then resolves the recovered key in the active locale. Product and engineering
terms remain English, and named placeholders may be reordered but not renamed.
See [specs/i18n.md](specs/i18n.md).

Manual credential, hosting-endpoint, and AI Agent saves run off the Slint event
loop. Their completion returns through the event loop, updates the transient
Saved label only after persistence succeeds, and emits one bottom Toast for the
terminal result.
`settings/save_worker.rs` owns one latest-state preference writer. Each request
resets a 100 ms debounce deadline; at most one save and one pending request exist.
Normal-priority pre-shutdown flush runs after high-priority feature shutdown,
bypasses the deadline and joins the writer. Explicit credential/provider saves
retain their existing feedback and serialized atomic-write path.
Read-heavy provider helpers use fine-grained `with_settings`/`with_git` access
instead of cloning the complete settings tree. Parsed API-base facts are cached
by exact configured value with a bounded process cache.

Settings and About route files are composition-only. Reusable navigation,
fields, toggles, sections, and detail rows live under
`ui/components/settings/` and `ui/components/about/` with `GitHub`-prefixed
symbols. Compact Settings navigation exposes the same section model and
keyboard interaction as the regular NavList.

### Messages

`usher-messages` stores the newest 1,000 application messages in
`messages/messages.json` under the platform local-data directory. Publishing
updates a small in-memory snapshot immediately; one owned worker performs JSON
persistence and `notify-rust` OS delivery outside the Slint event loop. Native
delivery failure is non-fatal and leaves the durable application message intact.
Delivery policy is orthogonal to category and severity: normal events use
`WhenUnfocused`, explicit in-app events use `Never`, and critical/diagnostic
events may use `Always`. The shell feeds window activity into the service so a
focused app does not duplicate its Banner in the OS notification center.
Read state, individual deletion, and bulk operations publish immutable snapshots;
the root bridge schedules unread-count and isolated-page model updates on the
event loop. Transient Banner dismissal never mutates the durable Inbox. See
[specs/messages.md](specs/messages.md).

The message contract separates ingress `channel` from semantic `category`.
Built-in keys cover general, Webhook, software-update, and CD-workflow messages,
but category is an open string vocabulary so a feature can add a category
without changing the center. Software-update Banners are classified while they
are mirrored, and completed CD deploy/rollback runs flow through the shared
Notification manager as categorized messages. UI Components owns a dedicated
Message center test section that exercises categories and delivery policies.

A future Webhook receiver belongs in a separate ingress adapter that authenticates
and normalizes provider payloads into `MessageDraft`; it does not change storage,
subscriptions, native delivery, or the Inbox. Channel-scoped external keys make
provider retries idempotent, while only bounded non-sensitive metadata crosses
the adapter boundary. No Webhook HTTP listener or provider schema is part of the
current implementation.

### Projects

`project/projects.json` stores project records in the platform local-data directory. On an add or edit save, Rust derives the provider from `remote_repo`, fetches its project details through `src/git/` using URL templates from `src/codereview/api/`, and stores the resulting `repo_project_id` and lower-case `repo_type` (`gitee`, `gitlab`, or `github`) together with the project record. A failed lookup leaves the previous configuration untouched. Rust converts records to Slint models, resolves local/base64/remote logos, and schedules PR overview work. Remote logo bytes are cached under the process temporary directory; UI rendering should read cached data rather than perform network work.

User-triggered project-registry reloads isolate blocking storage reads on the
shared runtime, prevent duplicate activation while loading, and return model
or recoverable error state through the Slint event loop.
`ProjectStateFacade` is the authoritative in-memory registry and repository
metadata selector. Typed add, update, delete, pending-save, logo, and overview
Actions commit through the Store; controllers do not reread `projects.json` for
metadata. Its deferred `PersistentStore` is enabled after post-first-frame
hydration, coalesces pending durable snapshots, and atomically renames a
temporary sibling over `projects.json`. Runtime-only commits do not rewrite
unchanged durable records. A failed write rolls back its still-current optimistic
snapshot without overwriting a later edit; success feedback is emitted only
after a real write completes.

Project projection is owned by `projection.rs`: registry identity, query, preview,
and logo/reload revisions select updates; pending-save/request bookkeeping does
not rebuild the list. One worker prepares the latest selection and one queued
UI delivery applies it to a stable `VecModel`, preserving unchanged rows and
nested tag models. `logo_prepare.rs` reads and decodes images off the event loop,
limits source bytes to 8 MiB, checks standalone raster RGBA size and sets its
decoder allocation limit to 64 MiB, and shares thumbnails of at most 128 pixels
per side with the UI. These limits do not bound all SVG embedded-resource memory.
The cache retains selected sources
only. Diagnostic byte counts reuse prepared metadata. Search and form-preview
callbacks only dispatch intent; stale generations cannot overwrite newer views.
Shutdown stops projection before cancelling the loader and flushing the Store.
SVG text uses bundled Noto Sans SC as its fallback, loaded on demand from shared
static bytes; projection never scans installed system fonts.

Remote list logos and the form preview share one `logo_loader.rs` driver on
`app_runtime`. The Project Store owns the `logo_requests.rs` admission policy:
at most four active requests, three attempts per continuously selected URL,
and 2/4-second failure cooldowns. Explicit registry reload resets failed entries
without duplicating in-flight work. Search and preview callbacks only enqueue
the latest input; source selection and typed scheduling Actions run on the
worker. UI projection never starts Logo downloads. Metadata is retained only
for filtered project URLs, the current preview and at most four retired active
requests; the existing image cache is separate. Project shutdown cancels the
driver before flushing its Store. HTTP uses the existing shared silent pipeline.

`ui/views/project.slint` is route composition only; reusable project cards,
toolbar controls, and dialogs live in `ui/components/project/`.
Home/Search/Detail/Project route states share `GitHubPageHeader` and
`GitHubBlankslate` rather than maintaining page-local placeholder patterns.

### Workflow

`crates/usher-ui/src/floating_overlay.rs` adapts native rectangles to the pinned
`floating-ui-core` engine. A pure `FloatingOverlayBridge` callback connects it to
the shared ActionMenu's opt-in adaptive placement; PR-list and showcase menus enable it.
The callback preserves the caller's preferred side/alignment. Two library Flip
passes configure Primer's side-before-alignment fallback; Select Menu uses
bottom/start with a 4 px gap, while PR filters explicitly use end alignment.
Offset, side/alignment flipping, shifting and available-size calculation belong
to the library. Slint owns rendering, scrolling, focus and dismissal.
PR-list menus receive live window-space bounds from the active filter trigger.
`GitHubPullRequestFlow` publishes a transient menu projection with an instance
owner token to `GitHubPrMenuState`; `GitHubPullRequestMenuOverlay` renders it at
the window root, outside page scrolling clips and section stacking. Selections
return through an owner-scoped action revision to the existing flow callbacks;
the projection is not persisted and owns no Workflow business state. MainWindow
supplies the visible page viewport. Resizing, scrolling and locale changes update
the anchor while open; a fully offscreen trigger, hidden flow, route change or
compact-toolbar replacement closes the menu. Scrolling back never reopens it.

The Workflow shell follows GitHub repository UnderlineNav behavior at every supported width; below 720 px, repository paths/actions move from the identity header into a trailing kebab overlay. The PR list reproduces GitHub's query and responsive filter interaction: a Search-style query field sits above the card, Open/Closed and filters stay on one card-header row, and below 620 px Assignee remains direct while Reviews and Sort move into one grouped kebab ActionMenu. All menus are mutually exclusive and switch on the first click. Closed uses GitHub semantics and includes both merged and closed-without-merge requests. `is:merged` and `is:closed is:unmerged` select those exact subsets; like GitHub, any non-canonical query replaces Open/Closed with a clear-query action and a Total summary. PR rows measure wrapped title height in Slint and a vertical layout accumulates each row's natural extent; the Workflow shell consumes the resulting component height instead of predicting a fixed per-row height. The controller parses portable state/review/assignee qualifiers, keeps the query field synchronized with toolbar changes, and resets pagination before reloading. Provider adapters translate the normalized query: GitHub uses native Search qualifiers, Gitee Enterprise uses documented state/search parameters, GitLab composes sorted closed/merged prefixes for the Closed union, and Gitee public bounds client-side scanning where v5 lacks an exact server filter. Open and Closed totals are fetched independently when the project or filters change, so the header does not reuse the selected page's transient count. When a total is still loading or the count request fails, the header hides the number and keeps the active summary label (Primer degraded-experience rule) instead of a perpetual loading placeholder. List timestamps use relative labels with UTC+08:00 absolute tooltips.

The PR row limits that absolute-time tooltip hit target to the timestamp text. It reserves a responsive trailing action group, wraps long titles ahead of the actions, and shows the target/source branch pair inline or on a narrow-layout continuation row. Provider list/detail fields supply the Conversation count when available. Conversation opens the review route on tab 0; AI Review opens the existing Files changed review workspace.

Workflow-level PR/Tag/Branch/CD navigation and Tag Add/Remove navigation share
`GitHubUnderlineNavItem`; Tag scope and semantic-version choices share
`GitHubSegmentedControl*`. Branch exposes the existing project-scoped
validation plan, while CD exposes the existing staged deployment and rollback
controller instead of rendering unavailable placeholders. CD uses separate deployment and
rollback stage enums and keeps step status typed until Slint projection. Branch and CD
fields/actions/results live in `ui/components/workflow/`, adapt at compact
widths, and use application-owned GitHub controls.
Local Tag refresh resolves Git state on a blocking worker and commits only when
its project ID and request sequence still match. GitLab Closed pagination and
public Gitee client-side filtering share the `src/git/` TTL/LRU PrefixCache
boundary. Each adapter incrementally extends complete provider-page prefixes,
keeps at most 32 entries for 30 seconds, deduplicates by IID, refreshes page one
explicitly, and invalidates repository entries after a successful merge. Public
Gitee reuses one raw closed prefix for merged and closed-unmerged local subsets.

`WorkflowStore` keeps a current project plus per-project `WorkflowViewState`. Switching projects must rebind UI state and prevent logs, steps, or running flags from leaking between projects. `Route.Workflow` and `Route.PrReview` form one lifecycle domain: moving between them retains the Workflow list/model state, and a temporary route outside the domain retains it while either workflow route remains reachable through Back.

Cross-page Back/Forward navigation is owned by `src/router/` as bounded dual history stacks; direct navigation pushes the current route onto Back and clears Forward, Back moves the current route onto Forward, and Forward performs the inverse. Slint renders the current route and delegates history actions instead of mutating a one-slot `previous-route` value. Feature controllers own drill-down checkpoints that do not change the route. Back first consumes such a feature-local checkpoint, then pops the global route history. A workflow route retained in either history stack remains a live return target, so temporarily opening Settings, About, or another page does not discard the PR review; heavy workflow state is released only when the workflow domain is no longer current or reachable through history navigation. `RouteLifecycleHooks` exposes ordinary transition hooks and named scope-unreachable hooks; `src/app_lifecycle.rs`, not Router, registers Chat visibility and Workflow cleanup. Hooks run only after the Router history lock and hook-registry borrows are released, so callbacks may safely re-enter application code.

Application keyboard commands are owned by `src/shortcuts/`. One ancestor
`FocusScope` in `ui/main.slint` forwards raw key events before the focused
child; the Rust registry normalizes the platform primary modifier, resolves an
application or route scope, validates overlapping conflicts, checks current
availability, and invokes existing root callbacks. It accepts only commands
that actually execute, so text editing and feature-local keys remain with the
focused component. Modal overlays block application shortcuts. Native chrome
and Slint tooltips obtain labels from the same registry instead of hard-coding
accelerators. See [specs/shortcuts.md](specs/shortcuts.md).
After the window is shown, one shortcut initialization entry validates and
binds the registry, then waits for the platform input bridge. The bottom status
bar publishes that staged progress and reports readiness only after both
boundaries are usable. It then retains a compact status icon whose shared
status-bar overlay lists stage and binding state; initialization failure remains
visible. The persistent cluster orders keyboard, bell-with-optional-count,
network, and version with fixed hit targets and gaps. Green is reserved for
explicit ready/online state; unread notification uses accent rather than green,
and zero notifications mount no badge. Status-bar overlays keep viewport and
anchors in one parent coordinate space so all items align at any width.

Before the window is shown, settings restoration snaps the persisted sidebar
toggle, spring progress, and route width to the same final state. Interactive
sidebar animation is enabled only for later user-driven toggles.

The application shell owns one 240 px desktop sidebar. Its persisted docked state drives one interruptible frame-paced spring for the sidebar, content surface, title-bar divider, and status-bar painted leading edge. Slint's display animation clock supplies the actual elapsed frame delta to a closed-form damped oscillator, so a 120 Hz display updates position on every rendered frame and a delayed frame does not accumulate fixed-step timing error. Target reversals preserve the current position and velocity. The mounted route adopts the target layout width once at motion start, while the status bar's transparent host and persistent right cluster remain in window coordinates. Repeated title-bar clicks retarget the same motion without queueing stale callbacks. Native live resize snaps any in-flight sidebar motion to its target, freezes the mounted route width, and applies the exact final width when dragging ends. The docked boundary elevation belongs to the main content surface, spans the complete window height, and projects only toward the sidebar as a clear stroke plus ambient and near shadow lobes. When undocked, an independent 8 px left-edge hover target opens the same sidebar as a border-only temporary root-layer overlay with rounded trailing corners and dismisses it shortly after pointer exit. macOS exposes sidebar, Back, and Forward through native accessory-bar buttons with hover-only backgrounds, tooltips, and accessibility labels. Its sidebar button uses Codex's outlined `sidebar.left` symbol while collapsed and a filled left-third symbol while expanded. The route breadcrumb occupies the remaining native title-bar inset rather than a second content row; its empty area remains input-transparent for native window dragging. Native accessory buttons, the Slint breadcrumb, and trailing actions share the geometric center of the full 46 px title-bar inset. The title/content divider begins at the animated main-content edge. On macOS a width-fixed 240 px `NSVisualEffectView` supplies Menu/BehindWindow material only below the sidebar. A full-window opaque native backdrop matching the active canvas stays below Slint's renderer so a late GPU surface resize cannot expose wallpaper tint in the content region. Vibrancy installation adopts the full-size transparent-title-bar style first, then attaches the effect and backdrop while the window is still opaque, and only then opens the transparent window path with a near-clear background instead of `clearColor`, all inside one AppKit transaction while the startup window is still alpha-gated, so the translucent Slint sidebar never flashes desktop content at startup. The docked-sidebar rectangle paints the final focus-aware renderer surface above the effect: translucent `#ffffffa6` light or `#2b2b2ba3` dark while key, then opaque `#fdfdfd` light or `#212121` dark after resigning key. Resign-key also hides the effect. The temporary hover sidebar keeps its independent opaque overlay surface. Router history owns the controls' enabled state. Windows and Linux expose the same controls in the Slint sidebar header and in a compact collapsed-state control strip. All platforms share the same Router callbacks and sidebar preference; platform code never owns route history.

The Workflow PR list and full-window review workspace use a single provider-neutral façade in `src/git/`. `RepoContext` validates the persisted provider/id against `remote_repo` once, then every list, detail/diff, repository-file, review-state, review-submit, merge, and revert operation dispatches through one object-safe `GitProvider`; `ProviderCapabilities` exposes endpoint support without UI inference. Shared JSON decoding and pagination helpers live in `src/git/json.rs`. `workflow/pr_review.rs` is the stable façade and `workflow/pr_review/controller.rs` owns asynchronous orchestration and UI writes; shared cache/diff ownership and avatar I/O live in `cache.rs` and `avatar.rs`. Pure immutable-source alignment and hunk expansion live in `workflow/pr_review/hunk.rs`, local tree construction and file classification live in `workflow/pr_review/file_tree.rs`, Unified/Split row mapping lives in `workflow/pr_review/diff_projection.rs`, the Slint lazy row adapter lives in `workflow/pr_review/diff_model.rs`, selection geometry plus clipboard projection live in `workflow/pr_review/selection.rs`, and Conversation/Commit presentation plus range mapping live in `workflow/pr_review/timeline.rs`. The review controller owns one shared immutable normalized diff, while its feature-owned state-store facade holds independent typed generations for diff, highlighting, expansion, Conversation, Commits, remote action-state reads, and remote mutations. The normalized detail moves each base file into an `Arc`, so the selected-file lazy model and highlighting task share that allocation instead of cloning every line; only a hunk expansion creates a per-file copy-on-write override. Provider HTML and diff aliases are decoded at the adapter boundary. `DiffChangeType` and `DiffLineKind` remain typed through repository, hunk, search, highlighting, and AI unified-diff logic; the lazy Slint adapter alone projects the existing strings. Each diff record is normalized to one visual source line without CR/LF controls before fixed-height Slint rendering. Collapsing an expanded file drops its override and exposes the shared base file again. Immutable head/base file content is fetched lazily for hunk expansion or in-app full-file viewing; edited-file expansion aligns both fixed revisions locally so one-sided lines are retained. Per-file highlighted diffs use a four-entry LRU and immutable source text uses a three-entry LRU; the full-file highlighted model is transient UI state. The active diff is exposed to Slint through the read-only lazy adapter, which materializes at most 64 converted rows, while the complete highlighted result remains Rust-owned for arbitrary-language support and fast scrolling. All late async responses are generation-guarded.

PR Review splits logical selection from shaped-text geometry. `selection.rs`
owns row/character ranges and clipboard projection. `text_geometry.rs` is the
replaceable numeric facade for styled-run and soft-wrapped caret hit-testing plus
range bounds; its default adapter uses bounded Parley 0.10 cursor/selection
geometry with Slint's shared font collection and exact run styles. Its four
Rust entry points accept one `TextGeometryOptions` value assembled by
`controller/diff_bindings.rs`; the `PrDiffBridge` Slint callback protocol stays
unchanged. Wrapped
ranges coalesce touching fallback-font fragments within each visual line and
use fixed-pitch, non-overlapping paint rectangles. Slint
continues to paint native `syntect` token models and never imports Parley types.
No WebView/editor is embedded; the facade cache is bounded and no Find viewport
positioning work enters scrolling or resizing.

The Tag workflow follows the same façade pattern: `workflow/tag.rs` exposes binding and refresh,
`tag/controller.rs` owns project state, UI sequencing and command orchestration;
`tag/command_output.rs` owns streaming process execution and cancellation,
`tag/version.rs` owns typed semantic-version bump rules and the Slint string boundary, while
`tag/commands.rs` owns typed create/remove plans and their projection to immutable Job step
definitions, including explicit refresh/completion steps. The application-wide
`src/jobs/` model separates immutable `name + argv` step definitions from runtime step data
and makes one `JobSnapshot` the source of job status, conclusion, total duration, and ordered
steps. Runtime durations remain optional `Duration` values until UI projection; step and Job
lifecycle methods update status, conclusion, detail, duration, and initial disclosure state as
one transition. A source feature publishes a `JobRunDescriptor` plus that snapshot and registers its Stop
and lazy-log actions under a stable source key. `Route.JobRun` is outside the Workflow lifecycle
domain, so any page may enter it and global Router history returns to the exact source. The route
reads only generic Job Run properties; it never reads Workflow or Tag properties.
`jobs/log_layout.rs` shapes mounted log rows with Parley into visual lines;
the UI paints them at a 20 px pitch with one number per source row. Source-row
padding remains independent, and resizing does not rebuild the source log model.
Empty source rows bypass shaping and retain one blank visual line, avoiding
synthetic shaping ranges outside the original string.
While a first load or width reflow is pending, the log viewport shows the shared
centered Spinner/loading state and hides the previous row projection, so loading
copy never overlaps a stale first line.
`ui/components/jobs/job.slint` renders the selected Job snapshot, while
`github_job_run.slint` provides the GitHub Actions-style header, Jobs region, and responsive log
layout. Both own no I/O or timing. Git
commands remain argument vectors so repository paths, tag names, and descriptions never pass
through shell interpolation. CD adapts its project-scoped stage snapshot to the shared
`GitHubActionsJob` visual presenter. Branch currently produces a validation plan rather than
executing a Job; any future executable Branch plan must adopt the reusable Job boundary instead
of copying the presenter or adding domain branches to it.
`jobs/log_model.rs` incrementally prepares Send-safe semantic lines on log mutations,
reusing unchanged lines. `jobs/presenter.rs` preserves Step and nested log model identities;
the lazy UI cache retains at most 128 converted source rows. `jobs/log_viewport.rs`
measures wrapped rows on the cancellable shared blocking pool, then installs exact
extents in `usher-virtual-list`. A bounded source-numbered window is painted inside
`GitHubScrollView`; document height and thumb range use the same exact extent, not
ListView's visible-row estimate. Same-count edits advance the log revision.
Scrolling never rewraps the document; only source/font/width changes request layout.
The unmounted viewport cache has soft eviction targets of 16 entries and 16 MiB
of accounted source/layout owners. The current callback and all rows models still
mounted by Slint are pinned, so the working set can remain over target until those
leases are released; a 250 ms event-loop retry then evicts the oldest unmounted
viewport and stops when no longer over target. Snapshot text bytes are cached when
the snapshot is produced, so pruning reads counters in O(viewports) time without
rescanning source rows. A generic fallback's concrete pending snapshot is counted,
but its opaque backing model is not estimable. Accounting also excludes cancelled
old in-flight work and other worker transients, allocator overhead, fonts, GPU
resources and total process RSS. Viewport drop only issues a non-blocking
cancellation request.
The cross-feature integration and acceptance contract is
[Exact virtual scrolling](specs/virtual-scrolling.md).
JobRun is mounted outside the generic page Flickable in a fixed-height viewport.
Its header and Jobs navigation stay fixed; log panes shrink to the remaining
height and retain their virtual-list scrollbars. Single-step run logs fill that
space even in tall windows instead of retaining the embedded Job's 420 px cap.
If ordered steps overflow, a
passive clipped output frame handles unconsumed wheel input and an output-only
scrollbar, without a Flickable ancestor capturing log gestures. Other routes
retain their normal page scrolling.
The process driver reads 4 KiB blocks into a 16-block queue, preserves UTF-8/CRLF across
blocks, and keeps a strict 32 KiB tail including the truncation marker. It drains while
readers run, aborts/joins them on cancellation, and limits inherited-pipe drain after direct
child exit to two seconds (a timeout reports failure).
Raw step logs remain in the project-scoped Rust snapshot until a disclosure first requests its
line model; that request latch survives later progress snapshots. Tag commands stream stdout and
stderr while running and publish bounded progress, so an opened step updates live. The presenter
maps GitHub `notice`, `warning`, `error`, and `debug` annotations to Primer semantic rows, rotates
an attention spinner for the active step, and leaves queued steps non-interactive.

Revert commands use the same provider-neutral façade and typed remote-mutation generation as review/merge commands, so stale results cannot affect another project or PR. A merged or closed request hides header review/merge controls while the Conversation composer stays available when credentials are configured (GitHub posts Conversation comments as issue comments so they work after merge). A merged request may expose a visible same-row Primer-style secondary header revert button only when the adapter returns `can_revert` (Usher prefers that over GitHub's kebab ActionMenu so the full label stays readable). GitHub GraphQL creates a separate reverse PR. Gitee Enterprise reloads the MR, requires a merge/squash commit whose `/repository/commits/:sha/refs` response reports both `merge_commit_by_pr` and `can_revert`, then calls the documented commit-revert endpoint against the target branch; rebase/fast-forward, public Gitee, and GitLab remain unsupported. Provider-specific confirmation text makes the direct Gitee branch mutation explicit.

Gitee Enterprise commit badges treat only positive MR detail/diff summaries as authoritative. A zero or missing summary triggers the lightweight commits count probe before the Commits tab is activated, because some deployments advertise zero until that collection is read; the full Commits content model remains lazy.

The Files changed `Diff view` popover owns a provider-neutral Unified/Split preference and the capability-gated Hide whitespace draft. `workflow/pr_review/diff_projection.rs` projects cached normalized lines for Unified or Split rendering; Split pairs each contiguous deletion/addition block and duplicates context across both sides. Applying only the view choice replaces the current lazy line model locally, while applying a supported whitespace change reloads provider data. Slint renders that projection and keeps unsupported whitespace checkboxes visible but disabled. The complete popover surface owns a pointer shield, so blank overlay areas cannot leak hover to the file tree below. It is hosted on the full Files changed toolbar row with a Primer upward caret whose tip tracks the settings gear even when the panel is edge-clamped.

Large files are classified by a provider-neutral changed-line or payload-size budget. Their normal file header remains available, but syntax highlighting and the Slint line model are deferred until that file's `Load diff` action is invoked. The in-place loading decision is scoped to the active complete/range diff and resets when the commit range changes.

Each atomic PR diff line-model replacement advances a content revision so the in-diff find model recomputes against all currently expanded lines. This signal is separate from the viewport-reset revision, preserving same-file hunk scroll anchors.

PR-list review decisions are normalized at the adapter boundary to review-required, approved, or changes-requested. GitHub hydrates visible rows from current reviews and requested reviewers, Gitee maps `review_state`, and GitLab prefers per-MR approvals with `detailed_merge_status` as fallback. Slint renders the same GitHub muted inline review link after the timestamp for every provider and opens Conversation when activated; provider-colored list badges are excluded.

The fixed PR-list review-status acceptance data lives under `tests/` and is reachable only through the opt-in `test-fixtures` Cargo feature plus the named `USHER_PR_LIST_FIXTURE` selector. Default and production builds do not compile the fixture bridge or embed its JSON. The normal controller remains the UI boundary so fixture rows exercise the same Slint model, filters, sorting, and responsive layout without entering provider adapters.

`workflow/pr_list.rs` also owns the portable query grammar and logical pagination boundary. The default query is `is:pr is:open`; Open and Closed are the only top-level state actions, while normalized states additionally include merged and closed-without-merge. The controller sequence-guards every query/page response. GitLab joins only the bounded closed and merged prefixes needed for the requested logical page before hydrating visible approvals; Gitee public scans at most 30 provider pages only for exact subsets or keyword searches that v5 cannot express.

The existing Workflow PR tab owns the repository pull-request list. `workflow/pr_list.rs` reads the selected project's persisted `repo_project_id` and `repo_type` off the event loop, then lazily requests one 10-record remote page through `src/git/` when the user opens or changes a page. GitHub preserves search-result totals and concurrently enriches only that page's rows with per-PR detail requests so source and target branch names are available. The controller formats parseable provider update timestamps for UTC+08:00 before passing list values to Slint, and uses exact totals where the provider supplies them. The tab defaults to open requests and explicitly supplies the selected open/closed/merged/all filter. Request sequencing prevents a late response from a previous project, state, or page from replacing the current view. A successful zero-item page renders a filter-specific illustrated empty state, while transport, authentication, and schema failures remain a distinct retryable error state. Clicking a row navigates to the full-window `Route.PrReview`; the Workflow component instance is destroyed, while its root/model state is retained so Back reconstructs the PR tab without reloading the list. The review route owns GitHub-style Conversation, Commits, and Files changed tabs. Conversation and Commits *content* are loaded only on first selection through provider-neutral models: GitHub combines PR detail, issue comments, reviews, and issue timeline activity (`committed` batches, `merged`, and selected system events); Gitee public v5 uses PR detail/comments/commits and synthesizes commit/merge activity from commits plus merge metadata; Gitee Enterprise v8 uses PAT-authenticated MR detail/notes/commits, derives active review summaries from the detail's `general_reviewers` / `primary_reviewers` `approve_status` values, and synthesizes the same activity after flattening nested enterprise `notes[].note` threads and reading ordinary note bodies from `note` / nested `note.note` / `comment` / string `content` (never object `content`), with MR commit timestamps from `created_at`; GitLab uses MR detail/notes/commits and synthesizes the same portable `commits`/`commit`/`merged` rows while filtering redundant system notes. Tab badge numbers do not wait for Conversation/Commits tab activation: they come from lightweight PR/MR summary fields carried with the diff load (`commit_count`, `conversation_count`, `files.length`). When a provider omits totals, adapters may issue one bounded count probe (GitHub/GitLab Link/`X-Total` with `per_page=1`; Gitee public short-page `per_page=100`; Gitee Enterprise diffs `commits_size`, MR `user_notes_count`/`notes_count`, or `with_page=true&per_page=1` + `meta.total_count`). Full Conversation timelines and commit lists stay lazy until those tabs (or the commit selector) are opened. The provider-neutral finalizer sorts each `commits` header with its consecutive `commit` rows as one atomic block, preserving the GitHub-style `added N commits` record for every provider even when individual authored dates differ. Conversation commit children and the Commits tab (including the Files changed commit selector) render newest→oldest; adapters still normalize lists oldest→newest, and the controller remaps display indices before contiguous range compares. A successful remote review or merge invalidates the cached Conversation timeline and reloads it immediately when Conversation is active. The Files changed controller loads and normalizes the selected PR diff, derives a collapsible directory tree, and keeps path filtering, folder expansion, and previous/next file navigation local. Its commit selector reuses the lazy commit list and dispatches contiguous ranges to each provider's repository compare endpoint; the original PR detail and previously loaded ranges remain cached for the active review. `workflow/syntax_highlight.rs` uses `syntect` off the UI layer to convert the selected file into theme-aware token models while preserving tabs and display-column widths. During file switches or hunk expansion, the previous line model stays mounted until the new highlighted model is ready; the event loop swaps it atomically, and only a real file switch resets the viewport. A hunk click records the clicked row's actual screen position before asynchronous expansion. After the model swap, the target delegate reports its post-layout content position and Slint reapplies the resulting visual anchor for a bounded number of layout frames. This avoids estimating wrapped row heights: a surviving hunk control remains under the pointer, while a removed separator places the first revealed row at the old control position. On macOS, native live-resize notifications freeze the review workspace's internal geometry and replace each visible token repeater with one plain monospace line; the final geometry and full token-colored tree are restored when live resize ends.

Commit activity preserves provider-native branch-update boundaries when the API exposes them. GitHub uses issue-timeline `committed` batches. GitLab uses `added N commits` system notes and matches their SHA references to the current MR commits. Gitee public v5 uses operation-log push records. Gitee Enterprise v8 loads project push events, filters them to the MR source branch, and maps `commit_from`/`commit_to` to the normalized commit order; the push-event actor is distinct from each commit's Git author. Missing or unusable activity data falls back to one provider-neutral commit batch without inventing an updater. Detail does not poll while it remains open. Explicit review Back clears owned Conversation, Commits, diff, and commit-range state, so reopening the PR from the retained list performs fresh provider reads. Sidebar Home is not this close operation and may retain the current session through Router history.

For Gitee Enterprise Conversation, Usher uses PAT-authenticated v8 APIs only. A detail/current-state URL that already carries the PR IID in its path does not repeat that IID as `pr_id`. Only explicit `approved` or `rejected` reviewer values from MR detail produce review rows. The browser-session `/pull_requests/{iid}/activities` feed is not called, and the adapter never stores, forwards, or logs browser Cookies. Provider-native wording never reaches Slint. Merge activity uses a dedicated detail identity such as `merged_by`; when it contains only an email/account label, a silent documented project-member lookup resolves an exact account match to its profile `name` and avatar. The documented user directory is a silent fallback, while merge-commit `committer_name` remains lower priority because it is Git identity. Fuzzy directory results are rejected, full email addresses are not presented as actor labels, and the PR author is never used as a merger fallback. GitLab follows the same no-author-fallback rule through `merged_by`; GitHub retains its native `actor.login` presentation.

PR presentation ownership follows the common UI boundary: reusable list,
filter, pagination, review-shell, file-tree, and diff controls live under
`ui/components/` with `GitHub`-prefixed symbols. `ui/views/workflow/` only
composes the full-window route. Public `@material` ListView/ScrollView may
provide virtualization for large timelines, trees, and diffs, but they do not
own product typography, palette, focus treatment, or controls; those stay in
application-owned GitHub components. Empty/error states reuse
`GitHubBlankslate`, and icon actions reuse `GitHubIconButton`.

PR Review uses eight independent typed state-store generations for diff, highlight, expansion, AI-finding Reveal, remote action-state reads, remote mutations, Conversation, and Commits work. Switching PRs, manual Reload, and ignore-whitespace changes invalidate all eight generations in one reducer action; starting an action-state refresh does not invalidate an in-flight mutation, and starting one lazy read does not invalidate an unrelated one. Forced reload bypasses the diff cache, clears lazy tab models, and reloads the active tab. Provider changed-file pages share one bounded collector. A full final page marks the normalized detail as `files_truncated`, and the Files changed surface presents that limitation explicitly. Conversation blocks compare parsed RFC3339 instants rather than provider strings. Merge capability is fail-closed: if required review/approval state cannot be loaded, the action-state bridge clears every write capability and publishes unavailable readiness.

The review header follows GitHub's two-step command model. A GitHub-styled readiness badge beside the title renders the normalized kind/detail with Primer semantic colors, while each adapter owns its merge semantics: GitHub maps REST mergeability, Gitee Enterprise maps v8 `current_state` conditions, public Gitee maps only v5 fields, and GitLab maps `detailed_merge_status` with an asynchronous recheck. The shared enum is a presentation boundary and must never cause one provider's conditions to be applied to another. `Review changes` submits an explicit provider-neutral Comment, Approve, or Request changes command; unsupported choices remain visible but disabled. The Conversation tab also exposes a GitHub-style plain-text composer (`Write` + green `Comment`) that submits the same Comment action. Eligible `review_summary` rows expose GitHub's per-review kebab and required-reason `Dismiss review` dialog; the aggregate header state remains informational. The provider-neutral dismissal facade delegates the selected review id to GitHub's dismissal endpoint when repository permissions allow it, posts a reason note and unapproves the authenticated user on GitLab, and withdraws the authenticated user's Gitee Enterprise review through the working Web API route `/api/code/api/{full_path}/-/pull_requests/{iid}/reviews` with the exact payload `{"state":"initialized"}`. Gitee falls back silently to the documented v8 reviews route for deployments without that Web route, then records the required reason as a best-effort MR note. Gitee public v5 has no dismissal capability. Provider success triggers a remote state refresh and never starts a merge. `Merge pull request` is enabled only by the latest normalized `PullRequestActionState` and presents only adapter-returned methods. GitHub supports repository-enabled merge, squash, and rebase methods. Gitee Enterprise v8 maps review states, omits the optional `comment` field for an Approve with no summary, and supports merge, squash, rebase, and fast-forward according to `restrict_merge_type`; public Gitee v5 supports comments plus merge/squash but has no approve/request-changes endpoint. GitLab supports comments, approvals, and SHA-guarded merge/squash with explicit source-branch deletion; its project `squash_option` limits the presented methods, and Request changes remains disabled because the stable REST API has no matching write command.

The review file tree uses GitHub's platform UI font stack (SF system UI on macOS and Segoe UI on Windows), while source code uses the platform monospace stack. The file tree keeps its virtualized `ListView`; Diff rows use the exact-height `usher-virtual-list` facade and a stable, bounded Slint overscan model positioned inside a `ScrollView`. Width/model revisions rebuild only lightweight row extents, while ordinary scrolling incrementally removes and appends visible rows. The divider resizes the pane live; its moving-handle calculation applies the remaining local pointer delta to the current width, keeping the divider attached to the pointer without rebuilding the complete tree or diff. The session preference is clamped to 220–520 px and no more than 45% of the review width so the diff remains the primary surface. Visibility follows GitHub Primer `$width-md` (768px): crossing below auto-collapses the tree and crossing back above auto-expands it, while the toolbar toggle always works so a narrow workspace can reopen the tree manually. The application window minimum width is 800px, so ordinary desktop resizing no longer reaches that internal breakpoint; the responsive rule remains valid for constrained hosts and direct workspace tests.

The review workspace starts below the macOS native title-bar inset and relies on the shell's native Back control instead of rendering a second page-local Back button. Its root render tree remains fixed at window `x=0`; ordinary header/tab/diff content follows the shell spring through an internal leading inset while adopting the target width once at motion start. During native live resize the workspace preserves its last stable internal geometry until drag end, avoiding repeated reflow of the large frozen/virtualized review tree while dialogs retain full-window coordinates. The shell routes Back through the review controller first, so commit-detail checkpoints are consumed before Router history; leaving the review still invalidates its requests and clears feature state before the Router returns to Workflow. PR actions are hosted in the title bar; their readiness ActionMenu is anchored to the badge but rendered at the root window layer so it is not clipped or hit-test-limited by the title-bar strip. File-extension and file-action menus remain mutually exclusive workspace popovers; switching triggers preserves hover and opens the target popover on the first click instead of relying on a native popup mouse grab.

Conversation and Commits adapters normalize optional Git author and committer identities and avatar URLs from GitHub, GitLab, Gitee public v5, and Gitee Enterprise v8. GitHub's separate top-level `author` and `committer` users are preserved. Enterprise Gitee lazily enriches Commits rows through repository commit detail; its user-directory lookup accepts only an exact email match and rejects ambiguous results. The UI follows GitHub wording and avatar geometry: one identity reads `{author} authored`, while distinct identities read `{author} authored and {committer} committed` beside overlapping avatars. The controller fetches unique avatars concurrently through the shared HTTP pipeline with a three-second aggregate timeout, a 2 MiB per-image limit, and bounded decoded dimensions; only send-safe RGBA bytes cross to the event loop, where Slint images are created. Missing or failed avatars fall back to an author initial. Timeline cards and the composer share one 52 px avatar/content grid. GitHub-style activity uses one continuous Primer timeline rail through avatar centers (clipped on the first and last items). Comment/description cards are 1px framed boxes that hug short bodies; commit marks and compact author avatars sit on the same rail; every activity row shares the cards' right edge. Rust supplies the exact row-height sum. The Conversation tab scrolls timeline and composer as one document so the Write box follows the last event instead of staying pinned to the viewport bottom.

Conversation commit batches use Primer's official `repo-push` Octicon. Their relative timestamp remains inline immediately after `added N commits`, while the shared absolute-time tooltip preserves exact provider time. GitHub reviews become an atomic activity summary plus optional body card; review state controls wording, automatic relative-time units match GitHub, and merged activity suppresses redundant closed activity. Connector segments follow semantic boundaries, but consecutive review pairs share a continuous rail from the preceding review card to the next summary marker. Gitee Enterprise ordinary comments use MR `/notes` with `noteable_type: PullRequest`; `/reviews` is reserved for review decisions. A commit-count probe without totals treats a response exactly filling the requested page as ambiguous—even for a bare array—and falls back to counting and discarding the documented complete non-paginated collection without mounting the lazy Commits model.

Diff geometry uses one measured extent: Rust passes the longest expanded code line alongside display-column metadata, and Slint measures it with the active monospace font for horizontal scrolling and row-background painting. This prevents an unpainted strip at the right edge and keeps tabs/wide glyphs aligned. The code column accepts GitHub-style drag selection across exact virtual rows (gutters excluded; hunk/meta rows never selectable and omitted from clipboard text); double-click expands to the same character-class run (word / whitespace / punctuation) and triple-click selects the full code line, with the multi-click streak owned by the review workspace so row-window changes cannot break the gesture. Soft-wrapped rows keep Slint `TextInput.word-wrap` auto-wrap and use its native UTF-8 selection geometry. The PR virtual-list adapter calculates the same wrap count from lightweight display-column metrics and final stable width, then uses exact prefix sums for content extent, scrollbar mapping, visible-window selection, and Find reveal. ⌘/Ctrl+F positioning remains a one-shot command: only Enter, Shift+Enter, or the previous/next controls calculate and perform at most one viewport write. Ordinary scrolling updates only the bounded virtual render window; it never runs Find positioning. Inactive and active match painting remains bounded to mounted rows while Find is open. The synchronous review-controller hit test walks actual projected row wrap heights so a forward or reverse drag can resolve an exact character in a wrapped endpoint. ⌘/Ctrl+C copies the plain selection silently.

AI Review emits finding locations but PR Review owns their interpretation. It validates the Review head SHA, maps left/right old/new source indices onto Unified or Split display rows, automatically admits a deferred large diff, and reuses the immutable-source cache to insert one target-centered hidden-context page when required. A dedicated Reveal generation makes rapid activations latest-request-wins. Slint applies one exact `CenterIfNeeded` command through the virtual-list facade and paints a side-aware gutter marker without replacing diff semantic backgrounds; stale or missing targets remain visibly unresolved until the user explicitly chooses View file.

For unwrapped code, pointer hits within a token, across tokens, or on a crossed
row and intersecting selection/current-Find rectangles use that facade's shaped
cursor boundaries. Run-sequence hits accumulate each independently painted
Slint token's shaped advance. This replaces display-column and independent-prefix
width approximations for punctuation, CJK fallback, kerning, bold, and italic
runs while retaining a 256-layout cache ceiling. Wrapped rows continue to use
Slint's native UTF-8 `TextInput.word-wrap` geometry.

`pr_review/find_bridge.rs` owns the event-loop Find model and callback bindings;
`find_worker.rs` scans shared immutable source/projection data on the shared
blocking pool using `usher-search`. The latest-computation mailbox bounds work
to one active scan, one pending query and one output, with generation cancellation
on typing, line replacement, close and review reset. Results contain compact hit
ranges and a row/pane index; converted hit and row-model caches each hold at most
128 entries. Storage remains proportional to the active file and its hit count.
Row paint reuses an index only for the same source model and query, otherwise it
resolves the requested mounted row locally.

Typing keeps the 180 ms debounce, clears stale results, and selects/highlights the
first result when the current scan completes. Completion never moves the viewport.
Each explicit Find navigation command uses completed results for at most one
viewport write. A command during a pending scan flushes the debounce but is not
replayed later. Manual scrolling, resize and focus changes preserve completed
query presentation without moving the viewport through Find.

Commit links in Conversation and Commits reuse the Files changed workspace by resolving the selected SHA and applying it as a one-item commit range. The review controller records the originating Conversation or Commits tab as a feature-local drill-down checkpoint, so shell Back restores that tab before the next Back returns to the Workflow PR list. When the commit list is still lazy, the controller completes that load before applying the pending selection; Copy SHA remains independent.

A successful merge marks the retained PR list stale. The top-level review Back refreshes page one of the active list filter and its state totals, so a merged request cannot remain in the Open list; feature-local commit Back continues to restore its source tab without consuming that refresh.

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

Chat history is append-only JSONL at `im/chat_history.jsonl`. New records persist a complete RFC 3339 instant with the local offset; terminal assistant rows also persist measured `elapsed_ms`. The Rust presentation projection converts timestamps into calendar-aware local labels (time today, yesterday, the day before yesterday, weekday within six days, month/day within the year, and full date across years) and durations into compact metadata, while legacy rows remain readable unchanged. A pending turn uses one page-level real-time tick and one optional mode-owned monotonic start instant; the instant's presence is the sending state, and switching modes or remounting Chat restores the actual elapsed value instead of starting over. AI and RAG keep independent composer drafts, including a draft restored after a late persistence failure. Startup loads a recent tail into the Rust session, but the Chat component and bounded Slint message model are mounted only on the Chat route. Older-page loading prepares a store request under the session lock, performs JSONL I/O without that lock, then validates and commits the page under the lock. Leaving Chat destroys its UI tree and clears the Slint model/projection cache; re-entry reconstructs the visible window from the session. Assistant messages retain a width-independent readable text projection; projections longer than 1,200 Unicode scalar values are bounded inline and expose the complete response through the existing title action. Formatted blocks are prepared by `usher-preview` and shown by the application-level preview host. Late history or assistant responses continue updating persistence/session without repopulating a hidden UI.

Production enables the bounded virtual window by default. One stable `VecModel`
contains at most 32 mounted rows, instantiates only each row's active role
subtree, and uses estimated top/bottom spacers from the width-keyed prefix.
Startup loads 20 recent messages and explicit or automatic history pagination
adds 10 at a time. The `chat-performance` fixture and
`USHER_CHAT_PERF_VARIANT=baseline` retain a non-virtualized comparison path;
normal launches use the optimized virtual path. In production, viewport-position changes commit a new bounded
window before the viewport consumes the configured pixel overscan and does not
pad a window of large rows merely to reach the row cap; it never rebuilds
the complete history, and no delayed idle work mutates the model after scrolling
stops. One route-owned `VecModel` keeps its identity while shifts remove and insert
only the non-overlapping prefix/suffix rows, preserving delegates for overlapping
stable message IDs. Same-event-loop window requests are coalesced and commit from the
latest viewport position. An ordinary wheel/touchpad overlapping shift introduces at most two
new rows, while direct scrollbar drag commits treat the destination as random access and bypass
that cap and the ordinary 2,400 px overscan. Pointer-up performs one final uncapped convergence
at the exact release position and retains the visible-only drag window until ordinary scrolling. Each
optimized mutation leaves viewport position ownership with Flickable and performs no
ordinary anchor write. Exact stable-ID correction is reserved for history prepend and
settled resize. Spacer and document extents come from a width-keyed prefix computed
before scrolling with the same bounded projection and Parley shaping model as the
message rows, including bubble sections, role minimums, spacing, and Slint line-box
rounding. The 2,400 px mounted-content edge guard replaces a window before the viewport
can enter a spacer. A stable width change rebuilds that prefix once; height changes
update viewport coverage independently. During macOS live resize, the message column retains its
last stable width while the viewport and composer continue following the native
window. The complete message ScrollView retains that stable width and is clipped
by the current route host, preventing viewport-width invalidation from entering
the message tree. Releasing the edge applies one final layout and width-only
resize never replaces the Chat message model. A same-binary baseline path exists only
for the reproducible interaction benchmark and is selected by environment, not
by user-facing application state. Programmatic bottom navigation pins the exact Flickable
bottom for at most 800 ms while initial measurements settle; wheel or scrollbar input
cancels it immediately. A resize beginning at the bottom retains that intent, while a
resize elsewhere retains its visible stable-ID anchor. The benchmark records mutation cost, mutation-to-first-render
latency, changed row counts, and commit-adjacent frame gaps; every optimized run
must keep commit-adjacent p95 and every mutation-to-render latency at or below 50 ms
while retaining zero stable blank exposure, zero post-stop commits, no more than 32 mounted rows, and at most one pixel
of bottom distance during startup size settlement. Direct scrollbar gestures freeze
their scroll maximum, thumb geometry, and pointer grab offset until release. Flickable
observes wheel/touchpad direction across the complete viewport; the benchmark also
requires at least ten such samples, no more than one pixel of drag or wheel reverse
movement, at least five rendered samples in each 24-step fast drag, and no more than
one pixel of virtual blank during either drag or release.

Chat presentation is split between route composition in `ui/views/chat.slint`
and reusable message/welcome components in `ui/components/chat/`. It uses the
application-owned GitHub ScrollView, Spinner, Button/IconButton, tokens, and
focus contracts. Initial entry and AI/RAG mode switches project the destination
history's tail before revealing the viewport and scrolling to its actual bottom;
the old mode's offset does not select the new mode's initial window.
Each message body has one permanent wrapped plain-text layout.
A Chat-owned Parley cache, pointer surface, and coalesced visual-line overlay
provide selection without mounting `TextInput`, changing preferred height, or
replacing the message model. Clipboard work occurs only for the active
Unicode-safe range when the platform copy shortcut is pressed; there is no
per-message copy control. The Chat viewport gives pointer drags to text
selection while retaining wheel/touchpad and explicit scrollbar navigation.
Reaching the top starts one deduplicated history-page
load. Prepend commits preserve the first fully visible message by stable ID and
its actual pre/post-layout position. The mode- and generation-scoped anchor
applies only the exact remaining delta over a bounded number of layout frames
until that position is stable; any wheel, touchpad, or scrollbar input cancels
the pending correction immediately. The history action stays before the top
spacer at the actual loaded-history boundary, not above every moving mounted
window. Prepend mounts neighboring older rows together with the reading anchor
within the 32-row budget. Initial layout publishes the real message viewport
height; virtual lookups exclude the header inset. Coverage checks are independent
of stale commit direction or scroll buckets, and input cancellation replays a
scroll request suppressed by settlement once. This presentation boundary must not expand the bounded Rust message window or move
persistence/HTTP work onto the event loop.

### Terminal

`session.rs` owns the PTY Reader, single ordered Writer and their termination;
`io.rs` owns input admission and partial-write retries. Output uses a bounded
64 x 8 KiB queue that backpressures the PTY. Crossbeam selection waits for capacity
or independent cancellation without polling. The 1 MiB/128-item input budget
includes the in-flight write; UI keys/paste and parser replies only enqueue
whole inputs. Rejected user input projects a locale-reactive error; Writer or
protocol-reply failure stops the session. Shutdown cancels publication but keeps
draining kernel output while reaping the child, then stops and joins both I/O
workers. This prevents macOS wait from hanging on full PTY output; termination
never waits under the presentation-state lock.
`lifecycle.rs` retains pending session retirements and joins them in the shared
asynchronous shutdown drain. The drain closes session admission before detaching
the active PTY. The existing high-priority pre-shutdown registration remains,
but only schedules cleanup and never waits on the event loop.

Terminal state is project-aware and persists while reopening the same project. Closing the drawer only hides its presentation; the PTY and Rust render state remain active, while Slint model refresh is deferred until reopen. Switching projects atomically detaches the old session, resets presentation and working directory, and terminates the detached process tree on a shared-runtime blocking worker so UI callbacks never wait for the graceful-exit window. Session IDs reject late output and exit events from the replaced project. The stable contract is documented in [specs/terminal.md](specs/terminal.md).

`GitHubTerminalDrawer` owns only drawer chrome, viewport reporting, input, and
cell presentation. Header/selection actions reuse shared GitHub controls;
terminal cell geometry, monospace font, ANSI colors, PTY ownership, and
scrollback remain independent of product typography.

With the non-default `ui-components` Cargo feature enabled, the primary rail
exposes `Route.UiComponents` as the application-owned component playground. It exercises shared control properties and the real
root-layer Toast, Notification, and Dialog managers. Model-dependent feature
components remain tested on their owning Project, Workflow, Chat, Settings,
shell, or PR Review route and are linked from the playground registry. The
former Login, Tools, List, Detail, Notification, Toast, and Dialog demo routes
have been removed. Without the feature, both sidebar variants omit the entry,
direct attempts to enter the route return to Home, and the showcase stays out
of the Slint compile and live-preview dependency graph.

## Main data flows

### Project to workflow

1. A project card invokes the root workflow-open callback.
2. `workflow/bindings.rs` creates a `WorkflowProjectContext`.
3. `WorkflowStore` selects the project-scoped state.
4. Workflow modules initialize their Slint properties.
5. PR counts are loaded through the shared runtime and provider-neutral Git façade, then applied through the event loop.
6. The PR-list controller reads persisted repository metadata and lazily loads the selected remote open page through the shared runtime; every page change requests that page only.

### Pull request list to review

1. The list controller loads repository metadata off the UI thread and resets its page cache whenever the selected project or list state changes.
2. The repository service rejects a persisted `repo_type` that does not match `remote_repo`, then routes the request to the matching provider adapter.
3. The adapter normalizes provider responses into a 10-item page. GitHub retains its exact search total and enriches visible rows with concurrent detail requests for branch refs.
4. The controller applies a page only if its typed Page generation is current; Open/Closed totals use an independent Totals generation. It formats parseable update times as UTC+08:00 before producing view models.
5. Opening a row navigates from `Workflow` to `PrReview` inside the shared Workflow lifecycle domain. The Workflow component is unmounted but its list/root models remain retained; the review controller reads fresh metadata, requests one normalized diff, caches it by project/request/whitespace mode, and renders its local file tree.
6. File selection, filtering, folder expansion, copy-path, ratio rendering, syntax highlighting, and collapsing expanded context are local. Hunk expansion, AI-finding hidden-context reveal, and in-app View file are the only follow-up provider reads; all share the bounded immutable-source cache. Directional expansion follows GitHub's signed-in improved `/changes/{commit}` view and reveals 21 primary-side source rows per page; AI finding reveal inserts one target-centered page and preserves separators around remaining hidden ranges. The signed-out classic `/commits/{commit}` view has different 20-row behavior and is not this UI's baseline. Once the remaining between-hunk context fits one page, the directional controls become one expand-all action; revealing the final context removes the obsolete hunk separator. File-level expand-all fills every hidden range, and collapse restores the retained original diff.
7. Returning to Workflow or switching project invalidates review/conversation/commit/highlight/source sequences so late work cannot affect the new context.

### Project save and provider lookup

1. The Project page submits the local/remote repository fields to its Rust callback.
2. `src/project/mod.rs` validates local fields and starts one shared-runtime lookup per project save.
3. `src/git/` parses `remote_repo`, builds the matching provider request from constants in `src/codereview/api/`, and executes it through `src/http/mod.rs`.
4. Only a successful response dispatches a typed Project add/update with `repo_project_id` and `repo_type`; `PersistentStore` writes `projects.json` off the event loop.
5. Lookup or persistence failures show one project-owned error toast and do not retain a partial record. Success is shown only after the durable write completes.

### Project-registry reload

1. A user reload sets the Project page loading state on the Slint event loop;
   the loading IconButton prevents another activation.
2. The shared application runtime moves `load_projects` to a blocking task.
3. Completion returns through `slint::invoke_from_event_loop`.
4. Success replaces the Project model and clears the inline error; failure
   clears loading and exposes a recoverable inline retry state while retaining
   navigation responsiveness.

### Project-aware terminal

1. Project card actions or the workflow FAB invoke a terminal callback.
2. Rust resolves the project key and local repository path.
3. `open_terminal_for_project` chooses reuse versus reset.
4. The PTY session runs off the UI thread.
5. Rendered terminal state is applied to `GitHubTerminalDrawer` through root properties.

### Chat request

1. `chat/interactive.rs` appends and persists the user message.
2. The shared runtime calls `chat/chat.rs` with the configured HTTPS RAG endpoint (`[ai.chat].rag_api_url`, then `USHER_RAG_API_URL`) and the stable request format.
3. `src/http/mod.rs` runs request interceptors, executes reqwest, validates status, runs response/error interceptors, and decodes the RAG response.
4. A mode-owned monotonic start instant drives one visible page timer. The result
   persists its measured elapsed milliseconds and is transformed for Slint on
   the event loop.
5. Failures remain an inline assistant message; the request is silent to prevent duplicate toast feedback.

### AI Agent prompt assembly

1. AI Chat, AI Review, and Review Ask load `[ai.agent]` and delegate all built-in
   instructions to `src/ai/prompts.rs`; call sites contain no prompt literals.
2. The adapter parses the embedded `src/ai/prompts.toml` catalog once, rejects an
   unsupported schema, and renders each versioned entry with strict undefined variables.
3. The selected reply locale contributes exactly one language instruction to the
   system message. Typed context assembly applies the 20-message conversation and
   48 KiB Review Ask diff bounds before transport.
4. Local paths, remote/provider URLs, author, update time, credentials, and model
   connection settings are excluded from PR prompt context. Logs contain only prompt
   id/version, effective locale, message counts, and context byte/truncation totals.
5. Review Sessions aggregate chunk-request provenance; successful Review Ask assistant
   messages keep per-request provenance. Legacy records default these fields safely.

### AI Review response projection

1. A worker loading or completing a review session passes assistant Markdown to
   `src/agent_response/`; user content remains raw text and the AI Review surface
   presents it as a content-sized, right-aligned neutral bubble without identity chrome.
2. The structure-only parser, bounded cache, and `StyledText` preparation produce
   width-independent blocks plus opaque repository-action tokens on a worker.
   Only `ModelRc` materialization and the final model commit run on the event loop.
3. The controller can also project review state, findings, and conversation into
   one flattened response-row model for the explicit `virtualized` memory
   experiment. Those rows share source model handles and never copy Markdown
   payloads. Production remains on the fully retained list because the
   representative A/B did not pass the adoption threshold.
4. Activating a response title submits the original Markdown to Artifact Preview.
   Activating a repository token looks it up in the current task projection and
   resolves the path against that task's local repository on a worker.
5. The one Docked Preview host reuses the PR trailing slot and temporarily hides,
   but does not clear, the AI Review surface. Preview close restores it; Detached
   Preview does not participate in this replacement.
6. AI panel close or PR-route exit clears the response projection, token map, and
   repository binding. It dismisses only a Docked PR-context Preview; an already
   Detached Preview and background Review work remain independent; the current
   Ask is cancelled and mount generation rejects stale worker commits.

### AI Agent connection test

1. Settings saves an independent OpenAI-compatible Base URL, API Key, model name, and reply language.
2. A user-triggered test runs `ai/agent.rs` on the shared runtime.
3. The shared HTTP module requests the provider's `/models` endpoint and reports the result through Toast feedback.
4. This flow must not change the RAG Chat endpoint, payload, or history behavior.

### Update download

Settings-triggered update checks publish their up-to-date, update-available, and failure outcomes through the shared Notification manager. The update-available action starts the existing install callback; transport details remain in logs rather than user-facing messages.

The persisted release channel defaults from the running build: stable builds use `stable`, while prerelease builds use `alpha`. A check records the exact selected version and installation resolves that same release instead of silently advancing to a newer result.

1. `self_update` resolves release metadata on a blocking task.
2. The shared HTTP pipeline starts a long-timeout streaming request.
3. Chunks are written to a temporary archive and progress is reported.
4. Cancellation deletes the partial file.
5. Existing update UI owns success/error notifications, so HTTP feedback remains silent.

## HTTP boundary

`src/http/mod.rs` is the only place allowed to construct the application-owned reqwest client. It offers request builders rather than hiding reqwest query, JSON, and header configuration. Execution returns the project-owned `HttpResponse`; JSON, text, bytes, and streaming chunk reads stay inside the shared timeout and error pipeline.

Every request supplies `RequestOptions`:

- `response_byte_limit` is opt-in and counts actual chunks before they reach
  buffering or consumers. An exceeded limit stays failed and emits feedback once.
  `HttpResponse::response_byte_limit` can only tighten a limit after inspecting
  headers. Remote Logos use 8 MiB; AI model-list JSON and buffered completions, including JSON fallback
  from streaming requests, use 16 MiB. Repository raw source uses 8 MiB, Base64
  JSON uses 12 MiB, and `src/git/source.rs` bounds decoded source to 8 MiB.
  SSE and updater streams keep their own policies. These are payload limits,
  not total RSS limits: transport chunks, JSON/text expansion and consumers add
  allocations. Error excerpts collect at most an 8,000-byte prefix before truncation.

- `operation` provides stable logs and user-facing context.
- `reject_http_errors` defaults to true and converts non-2xx responses into `HttpError` with a bounded body excerpt.
- `feedback` defaults to `Silent`; user actions may opt into toast, notification, or both.
- `timeout` defaults to 30 seconds, accepts a custom duration, and treats zero as no total deadline; the 10-second connect timeout remains independent.
- `read_idle_timeout` defaults to 30 seconds, accepts a custom duration, and
  treats zero as disabled. Every received response-body transport chunk resets
  the deadline. Incremental consumers use `HttpResponse::for_each_chunk`, which
  retains the same diagnostics and error interception as buffered reads.
- `retry_transient(3)` opts idempotent provider reads into capped exponential
  backoff with jitter for connect/timeout, 429, 5xx, and exhausted-rate-limit
  403 responses. It honors bounded `Retry-After`/provider reset hints and keeps
  intermediate attempts silent. Independent reads use `join!`; item hydration
  uses bounded unordered streams.

Interceptors execute in registration order. Request hooks may mutate headers; response hooks observe status/headers/timing; error hooks receive normalized failures. See [specs/http.md](specs/http.md).

## UI composition rules

- macOS sidebar toggle actions retarget Slint state directly from AppKit's main
  thread instead of queuing cross-event-loop callbacks. All platforms update
  the same single motion target immediately; page width remains frozen during
  motion and persistence still occurs only after the final settled state.
- Application keyboard commands pass through the one root capture boundary and
  `src/shortcuts/`; native title-bar buttons invoke the same callbacks and read
  their displayed accelerators from that registry.
- Root Rust-facing properties and callbacks are declared in `ui/main.slint` with consistent names.
- Feature views own layout; Rust owns I/O and business state transitions.
- Route files under `ui/views/` declare only `GitHub*Page` composition.
  Visual subcomponents belong in `ui/components/<feature>/`; components that
  implement GitHub/Primer patterns use `GitHub*` names, while other domain
  components retain domain names.
- Reuse `GitHubPageScrollActions`, `GitHubProjectCardActions`,
  `GitHubIconButton`, `GitHubPageHeader`, `GitHubScrollView`,
  `GitHubBlankslate`, `GitHubActionMenu`, `GitHubDialog`, Banner feedback, and
  project-aware terminal entry points.
- Keep hover-sensitive nodes mounted and vary opacity/state instead of conditionally creating hit targets.
- Mount full-window modal overlays at the root layer.
- Avoid layout/model rebuilds during scroll and window resize; defer precise work until interaction settles.
- Multiline typography follows [the text-layout contract](specs/text-layout.md): native `Text` and direct `TextInput` use Slint's supported `line-height-factor`, while Slint 1.18 `StyledText`/`TextEdit` surfaces keep an explicit documented fallback. Artifact Preview and Agent Response rich prose currently retain their `StyledText` typography and matching worker measurements. Any factor or fallback change is a layout-key change and must update paint, preferred height and exact virtual extents together.

## Validation

`scripts/check-quality.sh` is the local read-only validation entry point. It runs
fmt, structure/UI/i18n gates, script regressions, default check, library tests and
diff checks sequentially, continuing after ordinary failures and returning a
failing aggregate status. Interrupts stop the sequence. It does not waive known
structure overruns or replace feature-specific UI/Release A/B acceptance.

Minimum validation for Rust or Slint changes:

```sh
./scripts/check-ui-policy.sh
cargo fmt --all
cargo check --locked
cargo test --lib --locked
git diff --check
```

Run targeted manual checks for terminal viewport/session changes, update cancellation, tray shutdown, and route-specific layouts. The complete test command currently includes one unrelated failing logger doctest; do not hide a new failure behind that known issue.

## Known technical debt

- `src/logger/debug_logger.rs` contains a doctest that does not compile.
- `Route.UiComponents` is the maintained, `ui-components`-gated development
  playground; obsolete Login/Tools/List/Detail/Notification/Toast/Dialog demo
  routes are removed.
- The application is proprietary / non-open-source; see root `LICENSE.txt`. Third-party vendored components keep their own licenses.
- `docs/terminal_viewport_iteration_notes.md` is historical evidence, not a current specification.
