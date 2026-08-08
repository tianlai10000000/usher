# Usher Architecture

[中文](architecture.zh.md) · [README](../README.md) · [Specifications](specs/README.md)

## Purpose and scope

This document describes the current codebase, ownership boundaries, runtime lifecycle, and safe extension points. It is the canonical English architecture reference. Behavioral requirements that need explicit invariants live under `docs/specs/`.

Usher is a desktop-first Rust application. Slint owns presentation and declarative UI state; Rust owns persistence, process execution, networking, and application orchestration.

Production builds select Winit with FemtoVG/OpenGL at compile time. Skia and FemtoVG/WGPU are opt-in Cargo features used for renderer diagnostics and A/B comparisons, so normal builds cannot silently change renderer because multiple implementations were linked. Slint's in-process live preview and the UI Components playground are explicit `live-preview` and `ui-components` development features; neither is compiled into default or production builds. The WGPU path is not a production default because current macOS tests reduce physical footprint but increase RSS.

On macOS, entering or leaving full screen re-aligns native title-bar controls after AppKit's final layout; the traffic-light cluster is shifted 8 pt right onto the sidebar icon centerline, and application-owned controls are always placed beyond those lights.

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
| `crates/usher-messages/` | Independently compiled message-center package: durable domain/storage service, snapshot subscriptions, background persistence/native-delivery worker, `notify-rust` OS adapter, and generated Inbox page API. Its embeddable Slint page is compiled in library-module mode rather than through the deprecated non-Window root fallback. |
| `crates/usher-search/` | Framework-neutral shared Find engine: Unicode-safe non-overlapping text matches, wrapped adjacent-result navigation, and Chromium/Blink `CenterIfNeeded` viewport policy. Features supply only domain/model and visual-wrap adapters. |
| `crates/usher-state-store/` | Framework-neutral cross-page state port and built-in engine: typed reducers/actions, immutable revisioned snapshots, selectors, immediate hydration, automatic monotonic subscriptions, and RAII cancellation. Feature facades hide the engine so a community Store can replace it through an adapter. |
| `crates/usher-virtual-list/` | Framework-neutral variable-height virtualization facade: exact item extents, offset/index mapping, bounded overscan windows, and exact item reveal. Its private adapter currently uses the community `virtualizer` engine; features and Slint bridges depend only on the local port so the engine remains replaceable. |
| `src/app_runtime.rs` | Process-wide multi-thread Tokio runtime. Runtime threads install the tracing dispatch. |
| `src/memory_diagnostics.rs` | Process-wide RSS/footprint snapshots, startup checkpoints, bounded Slint-model accounting, UI-size correlation, and the cancellable periodic memory sampler. |
| `src/platform/` | Cross-platform native integration boundary. `macos.rs` owns AppKit appearance (forced Light/Dark or follow-system for Auto), title-bar accessories, window-interaction notifications, the native-title-bar click bridge for dismissible transient overlays, macOS PR-review font registration, the `NSSound` click, and `AppleInterfaceThemeChangedNotification` for live Auto theme updates; `windows.rs` owns Windows theme detection and `MessageBeep`; `linux.rs` owns XDG Portal theme detection and non-blocking desktop sound playback. `mod.rs` exposes the shared API, Auto theme watch enablement with a 1s poll fallback on all platforms, plus opening a directory in the OS file manager (Finder/Explorer/`xdg-open`). |
| `src/shutdown.rs` | Ordered shutdown coordination. |
| `src/single_instance.rs` | Single-instance process behavior. |
| `src/tray.rs` | Desktop tray integration and tray-driven lifecycle actions. |

`src/lib.rs` is a composition root, not the preferred home for feature logic. Native-window behavior belongs in `src/platform/`, diagnostics accounting belongs in `src/memory_diagnostics.rs`, and new business behavior belongs in a feature module that exposes a small setup or service API.

### Feature modules

| Path | Responsibility |
| --- | --- |
| `src/project/` | Stable façade/controller plus separated persistence models/storage, logo handling, PR/MR overview selection, and project-to-workflow/terminal entry points. |
| `src/codereview/api/` | URL and path templates for Gitee, GitLab, and GitHub repository APIs. It contains no request execution, authentication, or response decoding. |
| `src/git/` | Provider-neutral façade plus separated domain models, remote locator, unified-diff parsing, PR overview counting, and provider adapters. Gitee keeps transport, decoding, conversation/commit normalization, readiness, and revert semantics in dedicated submodules. |
| `src/jobs/` | Application-wide Job lifecycle model, source-independent run descriptor/UI projection, and source-keyed action dispatch. It owns no feature process. |
| `src/workflow/` | Per-project Branch, Tag, PR, and CD state and command execution. Tag and PR Review expose stable file façades with internal controller and pure-logic modules. |
| `src/terminal/` | PTY lifecycle, viewport synchronization, rendering, selection, and project switching; pure input mapping, diagnostics, and refresh throttling use dedicated modules. |
| `src/chat/` | Chat transport, sessions, JSONL persistence, Markdown conversion, rendering cache, pagination, interaction binding, and pure bounded virtual layout. |
| `src/update/` | Release checks, streamed downloads, progress, cancellation, installation, and restart. |
| `src/router/` | Route mapping, history, visibility, and reachability transitions; it does not perform feature cleanup. |
| `src/shortcuts/` | Application command IDs, declarative key bindings, platform-normalized matching, route scopes, conflict validation, dispatch, and display labels. |
| `src/app_lifecycle.rs` | Composition-level route lifecycle hook for cross-feature cleanup after transitions. |
| `src/settings.rs` | TOML settings model and persistence, including AI Agent credentials. |
| `src/ai/` | Standalone OpenAI-compatible AI Agent integration; `agent.rs` owns the connection test. It does not own RAG Chat. |
| `src/network.rs` | Background connectivity probe and `AppState.network-online`. |
| `src/http/mod.rs` | Shared asynchronous/blocking reqwest clients, interceptors, error normalization, and optional UI feedback. |
| `src/logger/` | Debug logging, tracing, and panic/error support. |
| `src/toast.rs` | Transient single-toast manager binding. |
| `src/notification.rs` | Queued notifications, timers, hover pause, actions, and icons. |
| `src/message_center.rs` | Thin bridge between the root window and `usher-messages`; mirrors theme/viewport state, pushes total/unread/read filter counts, and converts immutable snapshots into isolated-page Slint models. |
| `src/dialog.rs` | Imperative `DialogManager` binding and action dispatch; it owns no visual variant. |

Repository endpoint definitions and executable behavior are intentionally separated: `src/git/` may reference constants from `src/codereview/api/`, while `src/codereview/api/` must not depend on `src/git/`, `src/http/`, reqwest, or serde.

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
| `ui/views/workflow/` | Workflow route composition and the full-window PR review page. Flow, tab, filter, field, and panel components live under `ui/components/workflow/`. |
| `ui/components/` | Central ownership for every application visual component: controls, rows, cards, menus, tabs, panels, dialogs, fields, feedback, and feature-specific building blocks. GitHub/Primer implementations use `GitHub*` symbols. |
| `ui/components/about/`, `chat/`, `project/`, `workflow/` | Feature-owned GitHub/Primer visual building blocks extracted from route views; they own no I/O or route transitions. |
| `ui/components/showcase/` | Interactive GitHub/Primer property matrices and the registry linking model-dependent feature composites to their live routes. New or materially changed visual components must be registered here. |
| `ui/components/button/github_icon_button.slint` | Shared `GitHubIconButton` foundation for icon-only, floating, terminal, project-card, and copy actions, including keyboard focus, Tooltip, loading, and semantic variants. |
| `ui/components/label/github_counter_label.slint` | Shared typed Primer CounterLabel with default/primary/secondary schemes, optional upper limit, compact overlay geometry, and zero-count hiding; status notification badges and future navigation/action counts reuse it. |
| `ui/components/github_link.slint` | Shared Primer text-link states and keyboard interaction. |
| `ui/components/github_spinner.slint` | Single application-owned Primer indeterminate spinner used by controls and loading surfaces. |
| `ui/components/github_progress_bar.slint` | Shared semantic determinate/indeterminate progress track for status and update surfaces. |
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
| `ui/components/toast/`, `ui/components/notify/` | Shared compact Primer Banner-style transient and queued feedback; the root renders one newest-first queued notification Banner at a time, discloses capped-queue overflow against the durable unread total, and keeps batch operations in a compact ActionMenu. |
| `ui/components/messages/` | Reusable GitHub Inbox message rows with read/unread, semantic level, metadata, and mounted row actions; registered in UI Components. |
| `ui/components/pr_review/` | PR Review public UI structs plus reusable shell, file-tree, and diff controls; cross-region state stays in the workspace view. |
| `ui/theme/github_theme.slint` | Application-owned `GitHubTheme` semantic color roles and `GitHubMetrics` Primer geometry, control, typography, overlay, and breakpoint tokens. Raw functional UI colors are centralized here. |
| `material-1.7.1-fixed/` | Vendored Material source aligned with Slint v1.17.1. |
| `vendor/i-slint-live-preview/` | Temporary Slint 1.17.1 override selected only by the opt-in `live-preview` feature. Its documented `mtime` guard prevents spurious macOS watcher events from causing an endless recompile loop. |

Application UI must import Material through `@material`; direct imports from
vendored implementation paths are forbidden. Never modify
`material-1.7.1-fixed/` or other vendored Slint Material UI source for
application behavior or appearance. Material palette, typography, and style
metrics do not define the product UI. Application surfaces consume
`GitHubTheme` and `GitHubMetrics`, and every visual adaptation stays in
application-owned code so a dependency upgrade can replace the Material tree
without carrying patches.
The current allowlist is intentionally narrow: `MaterialWindowAdapter` in
`ui/main.slint`, plus public `ListView`/`ScrollView` scrolling in the PR
Review workspace. `scripts/check-ui-policy.sh` enforces this boundary and the
Octicon command-icon policy.

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

`src/platform/` selects one clear CJK-capable application font at startup:
the native `.AppleSystemUIFont` on macOS, `Microsoft YaHei UI` on Windows, and `Noto Sans`
elsewhere. `ui/main.slint` exposes that choice as the default for the complete
ordinary UI tree; page and shared-component code inherit it instead of selecting
independent faces. Terminal content remains fully independent so its existing
cell and toolbar metrics do not change. Within PR Review, only diff source code
and file-tree content keep the platform-selected specialized faces required by
their geometry; all other review controls and prose inherit the application font.

The UI source remains under the repository-level `ui/` directory. The main
window and ordinary route tree are generated by the `usher-ui` workspace crate;
the Settings and Messages routes are generated independently by
`usher-ui-settings` and `usher-messages` in library-module mode, then mounted
through Slint's experimental `ComponentContainer`/component-factory boundary.
This generates embeddable non-Window components without relying on Slint's
deprecated application-root fallback. The application crate re-exports
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
token. Both factory pages reuse the
same anchor-based offset/flip/shift/size implementation as `MainWindow` without
attempting to share a global instance across the factory boundary.
Changing `settings.slint` or
`messages.slint` must not regenerate or compile `usher-ui`; changing the main shell
still rebuilds the main generated crate. When `SLINT_LIVE_PREVIEW` is enabled,
both isolated build scripts deliberately keep factory products static because Slint 1.17's
`ComponentFactory` cannot accept the live-preview dynamic handle; the main
window and UI Components route remain live-reloadable.

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
   snapshot so remount mid-flight does not lose UI state.
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

Durable Store owners attach persistence as a non-blocking commit subscription.
Each changed revision is queued in commit order to the feature worker and
shutdown flush; reducers and the Slint event loop perform no file I/O. Concrete
engines, including the built-in `LocalStateStore`, stay behind the trait. A
future community Redux/Pinia-style library is accepted only as an adapter that
preserves the same Actions, feature facade, schema, revision/hydration contract,
and tests. See [specs/state-store.md](specs/state-store.md).

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
4. Bind theme, routing, navigation, application shortcuts, and window callbacks.
5. Open the durable message center and install its isolated-page bridge, then initialize notification and toast managers; queued Banners mirror into the center.
6. Bind the shared HTTP module to UI feedback, then start network monitoring.
7. Initialize dialog, project, chat, terminal, and workflow modules.
8. On macOS, install title-bar accessories and Menu vibrancy before `show()` when the native window handle is already available; retry after `show()` if needed. The winit window is created only after the event loop starts, so a first-render hook hides the window and installs native chrome in the same transaction as the first frame. A short poll then keeps retrying installation until the native chrome is confirmed installed, waits a few settle ticks so the effect's first (unblurred) behind-window sample commits and AppKit's post-accessory layout reset is realigned while invisible, re-aligns the traffic lights, then reveals the window; a deadline guarantees the window always appears. The first visible frame is therefore the final shell rather than a staged transition.
9. Install tray integration and run the Slint event loop.
10. On close/quit, coordinate terminal, updater, tray, runtime, and process cleanup through shutdown priorities.

Do not move modules earlier if they depend on generated Slint globals or on callbacks installed by a preceding module.

Native sidebar readiness is explicit UI state. Before AppKit finishes mounting
vibrancy, the root window and docked sidebar render opaque theme fallbacks.
Once mounted, the effect and native canvas are sized from their full-size parent
bounds after style changes and on native resize, so Menu material includes the
title-bar strip rather than inheriting the shorter pre-full-size render frame.
The Slint window starts with no visible standard title. macOS does not mount
substitute Slint controls while the native title-bar accessory is pending; only
the final AppKit sidebar and history controls appear in that region.

Debug builds enable process memory diagnostics automatically; release builds enable them with `USHER_MEMORY_DIAGNOSTICS=1`. `[Memory][Diag]` records current/peak RSS, physical footprint, VM categories, regions, Mach ports, threads, faults, page-ins, and deltas from the previous/startup samples. `[Memory][UI]` correlates settled route/window changes with logical and physical dimensions. Feature-owned logs, such as `[Workflow][PR Review][Memory]`, must report bounded model/cache ownership without exposing source content or credentials. The sampler uses the shared runtime and is stopped by the normal pre-shutdown pipeline.

## Concurrency model

- Slint objects and globals are event-loop-owned. Background tasks must use `slint::invoke_from_event_loop` before updating them.
- Async I/O uses the shared Tokio runtime from `src/app_runtime.rs`; do not create a new runtime or a new reqwest client per operation.
- Existing blocking Git, PR-statistics, and filesystem work runs on dedicated threads or Tokio blocking tasks. Never block the Slint event loop.
- Shared Rust state uses `Arc<Mutex<_>>`, `Arc<RwLock<_>>`, atomics, or per-thread caches depending on ownership.
- Long-running actions need cancellation or re-entry protection. Update downloads, history pagination, workflow runs, and terminal sessions are reference patterns.

## State and persistence

### Settings

`AppSettings` is persisted as TOML in the platform config directory. It contains theme mode, auto-update, logger flags, terminal diagnostics, UI sound-effect preference, navigation drawer state, Git platform tokens, plus per-provider enterprise API URLs and names under `[git.platform.<provider>]`, and OpenAI-compatible AI Agent connection values under `[ai.agent]`. Gitee stores public `token` and `enterprise_token` separately. A legacy single Gitee token is migrated to the enterprise field only when an enterprise API root was already configured; credentials never fall back across profiles. `AppState` mirrors values required by Slint. API keys, enterprise names, and configured URLs are local configuration and must not be logged. Provider API code uses an enterprise API URL only when its host matches the repository remote; users may supply the host or the complete API root, with the provider version path added if omitted. Public-provider defaults remain remote-derived. Auto theme resolves through AppKit on macOS, the Windows per-user application-theme setting, and the XDG Desktop Portal on Linux. While Auto is selected, Usher watches OS appearance changes: macOS uses `AppleInterfaceThemeChangedNotification` plus a 1s poll fallback; Windows and Linux poll the same detectors every 1s. Selecting Auto clears any forced AppKit appearance so the process can track the system; explicit Light/Dark lock Aqua/DarkAqua. When UI sound effects are enabled, breadcrumb Back uses `platform::play_ui_click`: macOS plays `Tink`, Windows queues the user-configured default sound, and Linux plays the desktop sound-theme event on a worker thread with common PipeWire/PulseAudio/ALSA command-line fallbacks.

Manual credential, hosting-endpoint, and AI Agent saves run off the Slint event
loop. Their completion returns through the event loop, updates the transient
Saved label only after persistence succeeds, and emits one bottom Toast for the
terminal result.

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
`ui/views/project.slint` is route composition only; reusable project cards,
toolbar controls, and dialogs live in `ui/components/project/`.
Home/Search/Detail/Project route states share `GitHubPageHeader` and
`GitHubBlankslate` rather than maintaining page-local placeholder patterns.

### Workflow

The Workflow shell follows GitHub repository UnderlineNav behavior at every supported width; below 720 px, repository paths/actions move from the identity header into a trailing kebab overlay. The PR list reproduces GitHub's responsive filter toolbar: Open/Closed and filters stay on one row; below 620 px, Assignee remains direct while Reviews and Sort move into one grouped kebab ActionMenu. All menus are mutually exclusive and switch on the first click. For GitHub repositories these map to native Search qualifiers and sort parameters and reset pagination before reloading, while other providers ignore unsupported GitHub qualifiers. Open and Closed totals are fetched independently when the project or filters change, so the header does not reuse the selected page's transient count. When a total is still loading or the count request fails, the header hides the number and keeps the Open/Closed label (Primer degraded-experience rule) instead of a perpetual loading placeholder. List timestamps use relative labels with UTC+08:00 absolute tooltips.

The PR row limits that absolute-time tooltip hit target to the timestamp text. It reserves a responsive trailing action group, wraps long titles ahead of the actions, and shows the target/source branch pair inline or on a narrow-layout continuation row. Provider list/detail fields supply the Conversation count when available. Conversation opens the review route on tab 0; AI Review opens the existing Files changed review workspace.

Workflow-level PR/Tag/Branch/CD navigation and Tag Add/Remove navigation share
`GitHubUnderlineNavItem`; Tag scope and semantic-version choices share
`GitHubSegmentedControl*`. Branch exposes the existing project-scoped
validation plan, while CD exposes the existing staged deployment and rollback
controller instead of rendering unavailable placeholders. Branch and CD
fields/actions/results live in `ui/components/workflow/`, adapt at compact
widths, and use application-owned GitHub controls.

`WorkflowStore` keeps a current project plus per-project `WorkflowViewState`. Switching projects must rebind UI state and prevent logs, steps, or running flags from leaking between projects. `Route.Workflow` and `Route.PrReview` form one lifecycle domain: moving between them retains the Workflow list/model state, and a temporary route outside the domain retains it while either workflow route remains reachable through Back.

Cross-page Back/Forward navigation is owned by `src/router/` as bounded dual history stacks; direct navigation pushes the current route onto Back and clears Forward, Back moves the current route onto Forward, and Forward performs the inverse. Slint renders the current route and delegates history actions instead of mutating a one-slot `previous-route` value. Feature controllers own drill-down checkpoints that do not change the route. Back first consumes such a feature-local checkpoint, then pops the global route history. A workflow route retained in either history stack remains a live return target, so temporarily opening Settings, About, or another page does not discard the PR review; heavy workflow state is released only when the workflow domain is no longer current or reachable through history navigation. Router reports that reachability transition through an injected hook; `src/app_lifecycle.rs`, not Router, owns the resulting cross-feature cleanup.

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

The application shell owns one 240 px desktop sidebar. Its persisted docked state drives one interruptible frame-paced spring for the sidebar, content surface, title-bar divider, and status-bar painted leading edge. The mounted route adopts the target layout width once at motion start, while the status bar's transparent host and persistent right cluster remain in window coordinates. Repeated title-bar clicks retarget the same motion without queueing stale callbacks. Native live resize snaps any in-flight sidebar motion to its target, freezes the mounted route width, and applies the exact final width when dragging ends. The docked boundary elevation belongs to the main content surface, spans the complete window height, and projects only toward the sidebar as a clear stroke plus ambient and near shadow lobes. When undocked, an independent 8 px left-edge hover target opens the same sidebar as a border-only temporary root-layer overlay with rounded trailing corners and dismisses it shortly after pointer exit. macOS exposes sidebar, Back, and Forward through native accessory-bar buttons with hover-only backgrounds, tooltips, and accessibility labels. Its sidebar button uses Codex's outlined `sidebar.left` symbol while collapsed and a filled left-third symbol while expanded. The route breadcrumb occupies the remaining native title-bar inset rather than a second content row; its empty area remains input-transparent for native window dragging. Native accessory buttons, the Slint breadcrumb, and trailing actions share the geometric center of the full 46 px title-bar inset. The title/content divider begins at the animated main-content edge. On macOS a width-fixed 240 px `NSVisualEffectView` supplies Menu/BehindWindow material only below the sidebar. A full-window opaque native backdrop matching the active canvas stays below Slint's renderer so a late GPU surface resize cannot expose wallpaper tint in the content region. Vibrancy installation adopts the full-size transparent-title-bar style first, then attaches the effect and backdrop while the window is still opaque, and only then opens the transparent window path with a near-clear background instead of `clearColor`, all inside one AppKit transaction while the startup window is still alpha-gated, so the translucent Slint sidebar never flashes desktop content at startup. The docked-sidebar rectangle paints the final focus-aware renderer surface above the effect: translucent `#ffffffa6` light or `#2b2b2ba3` dark while key, then opaque `#fdfdfd` light or `#212121` dark after resigning key. Resign-key also hides the effect. The temporary hover sidebar keeps its independent opaque overlay surface. Router history owns the controls' enabled state. Windows and Linux expose the same controls in the Slint sidebar header and in a compact collapsed-state control strip. All platforms share the same Router callbacks and sidebar preference; platform code never owns route history.

The Workflow PR list and full-window review workspace use a single provider-neutral façade in `src/git/`. Every list, detail/diff, repository-file, review-state, review-submit, and merge request is selected from the current project's persisted `repo_type`, then checked against `remote_repo` before the provider implementation runs. `workflow/pr_review.rs` is the stable façade and `workflow/pr_review/controller.rs` owns asynchronous orchestration and UI writes; shared cache/diff ownership and avatar I/O live in `cache.rs` and `avatar.rs`. Pure immutable-source alignment and hunk expansion live in `workflow/pr_review/hunk.rs`, local tree construction and file classification live in `workflow/pr_review/file_tree.rs`, Unified/Split row mapping lives in `workflow/pr_review/diff_projection.rs`, the Slint lazy row adapter lives in `workflow/pr_review/diff_model.rs`, selection geometry plus clipboard projection live in `workflow/pr_review/selection.rs`, and Conversation/Commit presentation plus range mapping live in `workflow/pr_review/timeline.rs`. The review controller owns one shared immutable normalized diff and a separate action sequence that prevents stale review or merge responses from updating another project or PR. The normalized detail moves each base file into an `Arc`, so the selected-file lazy model and highlighting task share that allocation instead of cloning every line; only a hunk expansion creates a per-file copy-on-write override. Provider HTML is decoded at the adapter boundary, and each diff record is normalized to one visual source line without CR/LF controls before fixed-height Slint rendering. Collapsing an expanded file drops its override and exposes the shared base file again. Immutable head/base file content is fetched lazily for hunk expansion or in-app full-file viewing; edited-file expansion aligns both fixed revisions locally so one-sided lines are retained. Per-file highlighted diffs use a four-entry LRU and immutable source text uses a three-entry LRU; the full-file highlighted model is transient UI state. The active diff is exposed to Slint through the read-only lazy adapter, which materializes at most 64 converted rows, while the complete highlighted result remains Rust-owned for arbitrary-language support and fast scrolling. All late async responses are sequence-guarded.

PR Review splits logical selection from shaped-text geometry. `selection.rs`
owns row/character ranges and clipboard projection. `text_geometry.rs` is the
replaceable numeric facade for styled-run and soft-wrapped caret hit-testing plus
range bounds; its default adapter uses bounded Parley 0.10 cursor/selection
geometry with Slint's shared font collection and exact run styles. Wrapped
ranges coalesce touching fallback-font fragments within each visual line and
use fixed-pitch, non-overlapping paint rectangles. Slint
continues to paint native `syntect` token models and never imports Parley types.
No WebView/editor is embedded; the facade cache is bounded and no Find viewport
positioning work enters scrolling or resizing.

The Tag workflow follows the same façade pattern: `workflow/tag.rs` exposes binding and refresh,
`tag/controller.rs` owns project state, UI sequencing, cancellation, and command execution,
while `tag/version.rs` and `tag/commands.rs` are pure, directly tested rules. The application-wide
`src/jobs/` model separates immutable `name + argv` step definitions from runtime step data
and makes one `JobSnapshot` the source of job status, conclusion, total duration, and ordered
steps. A source feature publishes a `JobRunDescriptor` plus that snapshot and registers its Stop
and lazy-log actions under a stable source key. `Route.JobRun` is outside the Workflow lifecycle
domain, so any page may enter it and global Router history returns to the exact source. The route
reads only generic Job Run properties; it never reads Workflow or Tag properties.
`ui/components/jobs/job.slint` renders the selected Job snapshot, while
`github_job_run.slint` provides the GitHub Actions-style header, Jobs region, and responsive log
layout. Both own no I/O or timing. Git
commands remain argument vectors so repository paths, tag names, and descriptions never pass
through shell interpolation. CD adapts its project-scoped stage snapshot to the shared
`GitHubActionsJob` visual presenter. Branch currently produces a validation plan rather than
executing a Job; any future executable Branch plan must adopt the reusable Job boundary instead
of copying the presenter or adding domain branches to it.
Raw step logs remain in the project-scoped Rust snapshot until a disclosure first requests its
line model; that request latch survives later progress snapshots. Tag commands stream stdout and
stderr while running and publish bounded progress, so an opened step updates live. The presenter
maps GitHub `notice`, `warning`, `error`, and `debug` annotations to Primer semantic rows, rotates
an attention spinner for the active step, and leaves queued steps non-interactive.

Revert commands use the same provider-neutral façade and action sequence as review/merge commands, so stale results cannot affect another project or PR. A merged or closed request hides header review/merge controls while the Conversation composer stays available when credentials are configured (GitHub posts Conversation comments as issue comments so they work after merge). A merged request may expose a visible same-row Primer-style secondary header revert button only when the adapter returns `can_revert` (Usher prefers that over GitHub's kebab ActionMenu so the full label stays readable). GitHub GraphQL creates a separate reverse PR. Gitee Enterprise reloads the MR, requires a merge/squash commit whose `/repository/commits/:sha/refs` response reports both `merge_commit_by_pr` and `can_revert`, then calls the documented commit-revert endpoint against the target branch; rebase/fast-forward, public Gitee, and GitLab remain unsupported. Provider-specific confirmation text makes the direct Gitee branch mutation explicit.

Gitee Enterprise commit badges treat only positive MR detail/diff summaries as authoritative. A zero or missing summary triggers the lightweight commits count probe before the Commits tab is activated, because some deployments advertise zero until that collection is read; the full Commits content model remains lazy.

The Files changed `Diff view` popover owns a provider-neutral Unified/Split preference and the capability-gated Hide whitespace draft. `workflow/pr_review/diff_projection.rs` projects cached normalized lines for Unified or Split rendering; Split pairs each contiguous deletion/addition block and duplicates context across both sides. Applying only the view choice replaces the current lazy line model locally, while applying a supported whitespace change reloads provider data. Slint renders that projection and keeps unsupported whitespace checkboxes visible but disabled. The complete popover surface owns a pointer shield, so blank overlay areas cannot leak hover to the file tree below. It is hosted on the full Files changed toolbar row with a Primer upward caret whose tip tracks the settings gear even when the panel is edge-clamped.

Large files are classified by a provider-neutral changed-line or payload-size budget. Their normal file header remains available, but syntax highlighting and the Slint line model are deferred until that file's `Load diff` action is invoked. The in-place loading decision is scoped to the active complete/range diff and resets when the commit range changes.

Each atomic PR diff line-model replacement advances a content revision so the in-diff find model recomputes against all currently expanded lines. This signal is separate from the viewport-reset revision, preserving same-file hunk scroll anchors.

The existing Workflow PR tab owns the repository pull-request list. `workflow/pr_list.rs` reads the selected project's persisted `repo_project_id` and `repo_type` off the event loop, then lazily requests one 10-record remote page through `src/git/` when the user opens or changes a page. GitHub preserves search-result totals and concurrently enriches only that page's rows with per-PR detail requests so source and target branch names are available. The controller formats parseable provider update timestamps for UTC+08:00 before passing list values to Slint, and uses exact totals where the provider supplies them. The tab defaults to open requests and explicitly supplies the selected open/closed/merged/all filter. Request sequencing prevents a late response from a previous project, state, or page from replacing the current view. A successful zero-item page renders a filter-specific illustrated empty state, while transport, authentication, and schema failures remain a distinct retryable error state. Clicking a row navigates to the full-window `Route.PrReview`; the Workflow component instance is destroyed, while its root/model state is retained so Back reconstructs the PR tab without reloading the list. The review route owns GitHub-style Conversation, Commits, and Files changed tabs. Conversation and Commits *content* are loaded only on first selection through provider-neutral models: GitHub combines PR detail, issue comments, reviews, and issue timeline activity (`committed` batches, `merged`, and selected system events); Gitee public v5 uses PR detail/comments/commits and synthesizes commit/merge activity from commits plus merge metadata; Gitee Enterprise v8 uses MR detail/notes/commits (plus best-effort `/reviews`) with the same synthesized activity after flattening nested enterprise `notes[].note` threads and reading ordinary note bodies from `note` / nested `note.note` / `comment` / string `content` (never object `content`), and MR commit timestamps from `created_at`; GitLab uses MR detail/notes/commits and synthesizes the same portable `commits`/`commit`/`merged` rows while filtering redundant system notes. Tab badge numbers do not wait for Conversation/Commits tab activation: they come from lightweight PR/MR summary fields carried with the diff load (`commit_count`, `conversation_count`, `files.length`). When a provider omits totals, adapters may issue one bounded count probe (GitHub/GitLab Link/`X-Total` with `per_page=1`; Gitee public short-page `per_page=100`; Gitee Enterprise diffs `commits_size`, MR `user_notes_count`/`notes_count`, or `with_page=true&per_page=1` + `meta.total_count`). Full Conversation timelines and commit lists stay lazy until those tabs (or the commit selector) are opened. The provider-neutral finalizer sorts each `commits` header with its consecutive `commit` rows as one atomic block, preserving the GitHub-style `added N commits` record for every provider even when individual authored dates differ. Conversation commit children and the Commits tab (including the Files changed commit selector) render newest→oldest; adapters still normalize lists oldest→newest, and the controller remaps display indices before contiguous range compares. A successful remote review or merge invalidates the cached Conversation timeline and reloads it immediately when Conversation is active. The Files changed controller loads and normalizes the selected PR diff, derives a collapsible directory tree, and keeps path filtering, folder expansion, and previous/next file navigation local. Its commit selector reuses the lazy commit list and dispatches contiguous ranges to each provider's repository compare endpoint; the original PR detail and previously loaded ranges remain cached for the active review. `workflow/syntax_highlight.rs` uses `syntect` off the UI layer to convert the selected file into theme-aware token models while preserving tabs and display-column widths. During file switches or hunk expansion, the previous line model stays mounted until the new highlighted model is ready; the event loop swaps it atomically, and only a real file switch resets the viewport. A hunk click records the clicked row's actual screen position before asynchronous expansion. After the model swap, the target delegate reports its post-layout content position and Slint reapplies the resulting visual anchor for a bounded number of layout frames. This avoids estimating wrapped row heights: a surviving hunk control remains under the pointer, while a removed separator places the first revealed row at the old control position. On macOS, native live-resize notifications freeze the review workspace's internal geometry and replace each visible token repeater with one plain monospace line; the final geometry and full token-colored tree are restored when live resize ends.

PR presentation ownership follows the common UI boundary: reusable list,
filter, pagination, review-shell, file-tree, and diff controls live under
`ui/components/` with `GitHub`-prefixed symbols. `ui/views/workflow/` only
composes the full-window route. Public `@material` ListView/ScrollView may
provide virtualization for large timelines, trees, and diffs, but they do not
own product typography, palette, focus treatment, or controls; those stay in
application-owned GitHub components. Empty/error states reuse
`GitHubBlankslate`, and icon actions reuse `GitHubIconButton`.

The review header follows GitHub's two-step command model. A GitHub-styled readiness badge beside the title renders the normalized kind/detail with Primer semantic colors, while each adapter owns its merge semantics: GitHub maps REST mergeability, Gitee Enterprise maps v8 `current_state` conditions, public Gitee maps only v5 fields, and GitLab maps `detailed_merge_status` with an asynchronous recheck. The shared enum is a presentation boundary and must never cause one provider's conditions to be applied to another. `Review changes` submits an explicit provider-neutral Comment, Approve, or Request changes command; unsupported choices remain visible but disabled. The Conversation tab also exposes a GitHub-style plain-text composer (`Write` + green `Comment`) that submits the same Comment action. Provider success triggers a remote state refresh and never starts a merge. `Merge pull request` is enabled only by the latest normalized `PullRequestActionState` and presents only adapter-returned methods. GitHub supports repository-enabled merge, squash, and rebase methods. Gitee Enterprise v8 maps review states, omits the optional `comment` field for an Approve with no summary, and supports merge, squash, rebase, and fast-forward according to `restrict_merge_type`; public Gitee v5 supports comments plus merge/squash but has no approve/request-changes endpoint. GitLab supports comments, approvals, and SHA-guarded merge/squash with explicit source-branch deletion; its project `squash_option` limits the presented methods, and Request changes remains disabled because the stable REST API has no matching write command.

The review file tree uses GitHub's platform UI font stack (SF system UI on macOS and Segoe UI on Windows), while source code uses the platform monospace stack. The file tree keeps its virtualized `ListView`; Diff rows use the exact-height `usher-virtual-list` facade and a stable, bounded Slint overscan model positioned inside a `ScrollView`. Width/model revisions rebuild only lightweight row extents, while ordinary scrolling incrementally removes and appends visible rows. The divider resizes the pane live; its moving-handle calculation applies the remaining local pointer delta to the current width, keeping the divider attached to the pointer without rebuilding the complete tree or diff. The session preference is clamped to 220–520 px and no more than 45% of the review width so the diff remains the primary surface. Visibility follows GitHub Primer `$width-md` (768px): crossing below auto-collapses the tree and crossing back above auto-expands it, while the toolbar toggle always works so a narrow workspace can reopen the tree manually. The application window minimum width is 800px, so ordinary desktop resizing no longer reaches that internal breakpoint; the responsive rule remains valid for constrained hosts and direct workspace tests.

The review workspace starts below the macOS native title-bar inset and relies on the shell's native Back control instead of rendering a second page-local Back button. Its root render tree remains fixed at window `x=0`; ordinary header/tab/diff content follows the shell spring through an internal leading inset while adopting the target width once at motion start. During native live resize the workspace preserves its last stable internal geometry until drag end, avoiding repeated reflow of the large frozen/virtualized review tree while dialogs retain full-window coordinates. The shell routes Back through the review controller first, so commit-detail checkpoints are consumed before Router history; leaving the review still invalidates its requests and clears feature state before the Router returns to Workflow. PR actions are hosted in the title bar; their readiness ActionMenu is anchored to the badge but rendered at the root window layer so it is not clipped or hit-test-limited by the title-bar strip. File-extension and file-action menus remain mutually exclusive workspace popovers; switching triggers preserves hover and opens the target popover on the first click instead of relying on a native popup mouse grab.

Conversation adapters normalize optional author and commit-author avatar URLs from GitHub, GitLab, Gitee public v5, and Gitee Enterprise v8. The controller fetches unique avatars concurrently through the shared HTTP pipeline with a three-second aggregate timeout, a 2 MiB per-image limit, and bounded decoded dimensions; only send-safe RGBA bytes cross to the event loop, where Slint images are created. Missing or failed avatars fall back to an author initial. Timeline cards and the composer share one 52 px avatar/content grid. GitHub-style activity uses one continuous Primer timeline rail through avatar centers (clipped on the first and last items). Comment/description cards are 1px framed boxes that hug short bodies; commit marks and compact author avatars sit on the same rail; every activity row shares the cards' right edge. Rust supplies the exact row-height sum. The Conversation tab scrolls timeline and composer as one document so the Write box follows the last event instead of staying pinned to the viewport bottom.

Conversation commit batches use Primer's official `repo-push` Octicon. Their relative timestamp remains inline immediately after `added N commits`, while the shared absolute-time tooltip preserves exact provider time. GitHub reviews become an atomic activity summary plus optional body card; review state controls wording, automatic relative-time units match GitHub, and merged activity suppresses redundant closed activity. Connector segments follow semantic boundaries, but consecutive review pairs share a continuous rail from the preceding review card to the next summary marker. Gitee Enterprise ordinary comments use MR `/notes` with `noteable_type: PullRequest`; `/reviews` is reserved for review decisions. A commit-count probe without totals treats a response exactly filling the requested page as ambiguous—even for a bare array—and falls back to counting and discarding the documented complete non-paginated collection without mounting the lazy Commits model.

Diff geometry uses one measured extent: Rust passes the longest expanded code line alongside display-column metadata, and Slint measures it with the active monospace font for horizontal scrolling and row-background painting. This prevents an unpainted strip at the right edge and keeps tabs/wide glyphs aligned. The code column accepts GitHub-style drag selection across exact virtual rows (gutters excluded; hunk/meta rows never selectable and omitted from clipboard text); double-click expands to the same character-class run (word / whitespace / punctuation) and triple-click selects the full code line, with the multi-click streak owned by the review workspace so row-window changes cannot break the gesture. Soft-wrapped rows keep Slint `TextInput.word-wrap` auto-wrap and use its native UTF-8 selection geometry. The PR virtual-list adapter calculates the same wrap count from lightweight display-column metrics and final stable width, then uses exact prefix sums for content extent, scrollbar mapping, visible-window selection, and Find reveal. ⌘/Ctrl+F positioning remains a one-shot command: only Enter, Shift+Enter, or the previous/next controls calculate and perform at most one viewport write. Ordinary scrolling updates only the bounded virtual render window; it never runs Find positioning. Inactive and active match painting remains bounded to mounted rows while Find is open. The synchronous review-controller hit test walks actual projected row wrap heights so a forward or reverse drag can resolve an exact character in a wrapped endpoint. ⌘/Ctrl+C copies the plain selection silently.

For unwrapped code, pointer hits within a token, across tokens, or on a crossed
row and intersecting selection/current-Find rectangles use that facade's shaped
cursor boundaries. Run-sequence hits accumulate each independently painted
Slint token's shaped advance. This replaces display-column and independent-prefix
width approximations for punctuation, CJK fallback, kerning, bold, and italic
runs while retaining a 256-layout cache ceiling. Wrapped rows continue to use
Slint's native UTF-8 `TextInput.word-wrap` geometry.

Each explicit Find navigation command completes its calculation and optional viewport write immediately. There is no pending request for later scrolling or layout work to cancel or restart. Query changes, manual scrolling, resize, and focus changes preserve the query, counter, active result, and active highlight without moving the viewport.

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

Chat history is append-only JSONL at `im/chat_history.jsonl`. Startup loads a recent tail into the Rust session, but the Chat component and bounded Slint message model are mounted only on the Chat route. Leaving Chat destroys its UI tree and clears the Slint model/Markdown cache; re-entry reconstructs the visible window from the session. Late history or assistant responses continue updating persistence/session without repopulating a hidden UI. Rendering and scroll-anchor calculations are deliberately separated to limit work during scrolling/resizing.

Chat presentation is split between route composition in `ui/views/chat.slint`
and reusable message/welcome components in `ui/components/chat/`. It uses the
application-owned GitHub ScrollView, Spinner, Button/IconButton, Copy, tokens,
and focus contracts. This presentation boundary must not expand the bounded
Rust message window or move persistence/HTTP work onto the event loop.

### Terminal

Terminal state is project-aware and persists while reopening the same project. Switching projects resets session-specific presentation and changes the working directory. The stable contract is documented in [specs/terminal.md](specs/terminal.md).

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

Settings-triggered update checks publish their up-to-date, update-available, and failure outcomes through the shared Notification manager. The update-available action starts the existing install callback; transport details remain in logs rather than user-facing messages.

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

## Validation

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
