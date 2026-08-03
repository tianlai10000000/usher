# Memory Optimization Backlog

[中文](memory_optimization_backlog.zh.md) · [Architecture](architecture.md) · [Specifications](specs/README.md)

This document records measured memory behavior and follow-up optimization work. It is an investigation backlog, not a stable product specification or a claim that every item should be implemented.

## Status

- Last measurement: 2026-08-02
- Platform: macOS, Retina scale factor 2.0
- Build: debug, default FemtoVG/OpenGL renderer
- Route: Home renderer baseline; Projects interaction A/B
- Current decision: prioritize renderer and native-window A/B measurements; do not optimize small business models based on the idle sample

All numbers below describe this single controlled run. Release builds, other display scales, other routes, and large PR review sessions require separate measurements.

## Current diagnosis and confidence

The present evidence supports three separate conclusions:

1. **Confirmed, high confidence:** the largest observed post-fullscreen growth is macOS graphics physical footprint, especially `IOSurface`, while Rust malloc allocation changed very little. The second identical cycle plateaued, so this run does not demonstrate an unbounded leak.
2. **Confirmed, medium confidence:** mounting only the active Projects representation saved about 2.2 MiB stable RSS. This is real but much smaller than the graphics high-water mark. Sidebar and native-resize interactions still have material sampled CPU cost.
3. **Static risk, not yet measured:** PR Review, Chat, and Terminal are bounded at their Slint presentation boundaries, but some Rust-owned values can still be large or can grow after explicit user loading. They need workload-specific attribution before changing cache capacities or lifecycle behavior.

### Memory metric boundaries

- **RSS** answers how many mapped pages are resident now. It is the primary cross-platform process metric, but it does not fully represent macOS graphics pressure.
- **Physical footprint** is the primary macOS pressure metric and must be paired with RSS for renderer/window decisions.
- **Owned bytes** from feature diagnostics explain application ownership but are estimates, not replacements for RSS or footprint.
- **Peak minus settled** distinguishes a transient spike from retained memory. **Cycle-to-cycle settled slope** distinguishes a bounded high-water mark from cumulative growth.
- Binary size and the 19 GiB workspace `target/` directory observed on 2026-08-02 are disk/build concerns, not runtime RSS. The current debug executable is about 325 MiB and is not a stripped release artifact; do not use that size as an RSS diagnosis.

### Static ownership map

| Area | Existing bound | Remaining question | Priority |
| --- | --- | --- | --- |
| Renderer + macOS window | Compile-time renderer selection; repeated cycle plateaued | Which renderer/window flags own the 137 MiB `IOSurface` and 38 MiB graphics increase? | P0 |
| PR Review | 64 converted Slint rows; highlight 4 entries; source 3 entries; range diff 4 entries; route cleanup | Count limits do not cap one very large diff/highlight/source entry; verify release-on-leave and byte ownership | P1 |
| Workflow reachability | Router retains Workflow/PR Review while either route remains in bounded Back/Forward history | Detect the exact reachable→unreachable edge when the final Workflow entry is evicted from history; current event derives only from the immediately previous route | P1 |
| PR list pages | 10 records per remote page; cache clears on project/filter/state changes | `visited_pages` retains every page visited under one active query; quantify deep pagination and clear it with the lifecycle owner if material | P2 |
| Chat | At most 56 Slint rows; Markdown cache 96 entries and 8 MiB; cache/UI clear on route leave | Rust `loaded_messages` and its O(n) layout prefix remain for every page explicitly loaded in the session | P1 after graphics |
| Terminal | 1,000-row `vt100` scrollback; only viewport rows projected to Slint; project switch/shutdown reset state | Quantify simultaneous parser, rendered Rust snapshot, and Slint span-model copies under colorful long output; preserve the stable drawer lifecycle | P2 |
| Project logos | Decoded raster dimensions are capped at 128×128 | Thread-local Slint image cache has no entry-count bound; measure many distinct edited/remote/base64 logo keys before adding eviction | P3 |
| Workflow jobs | Tag step logs clamp to 32 KiB per step and Slint log rows are lazy | Confirm no other job producer bypasses the bound; expected low idle impact | P2 |
| Tokio/runtime | One shared multi-thread runtime; observed thread residency below 1 MiB | Worker-count saving versus I/O latency and shutdown behavior | P3 |
| Dependencies | Renderer is narrowed, but `tokio/full`, reqwest default features, and `image` defaults remain; current tree includes native TLS and rustls paths | Feature reduction may improve binary/startup mappings, but RSS benefit is unproven | P3 |

## Measured baseline

| Scenario | RSS | Physical footprint |
| --- | ---: | ---: |
| Before showing the window | about 75 MiB | about 19 MiB |
| Small window, settled | about 139 MiB | about 68 MiB |
| Fullscreen | about 140 MiB | about 253 MiB |
| Exit fullscreen and return to a small window | about 141 MiB | about 243 MiB |
| Repeat the same resize/fullscreen cycle | about 140 MiB | about 243 MiB |

The second cycle did not continue growing. In this run the behavior looked like a bounded renderer/window-surface high-water mark, not a per-cycle unbounded leak.

The main footprint changes after fullscreen were:

- `IOSurface`: about 23 MiB to 160 MiB, an increase of about 137 MiB.
- Owned unmapped graphics memory: about 11 MiB to 49 MiB, an increase of about 38 MiB.
- Allocated malloc memory: about 11.1 MiB to 11.7 MiB, an increase of only about 0.6 MiB.
- Resident malloc memory stayed around 21 MiB, including about 10 MiB of fragmentation.
- The process used about 17–18 threads; resident thread stacks were below 1 MiB.

`ps` RSS does not include all of the graphics-footprint growth seen by macOS. For optimization decisions, always record both RSS and physical footprint. Very large VSZ values are mostly address-space reservations and mappings and are not, by themselves, evidence of real memory pressure.

## Sidebar and native-resize A/B

The 2026-08-02 Projects-route follow-up keeps sidebar motion while avoiding per-frame route-width changes: the route adopts the target width once and the shell surface follows one interruptible spring. Native live resize deliberately freezes mounted route width until drag end. Projects mounts only its active list/grid representation, and PR Review also freezes its expensive internal diff geometry. A bounded native backdrop separately prevents wallpaper-colored flashes below a late GPU resize.

The controlled setup used a fixed 1000×700 Projects window, a 6-second warm-up, 16 evenly spaced sidebar toggles or eight automated left-edge drag cycles, 100 ms RSS/CPU sampling, and three isolated processes per variant. Values are medians of per-process measurements.

| Sidebar toggle variant | CPU sample mean | CPU sample peak | Peak RSS delta | Retained RSS delta |
| --- | ---: | ---: | ---: | ---: |
| One-shot width + shell spring | 51.54% | 65.4% | +2768 KiB | +2528 KiB |
| Atomic dock + active Project tree + bounded native backdrop | 20.65% | 39.0% | +3472 KiB | +3264 KiB |

The atomic variant quantified the cost of shell animation, but was rejected because it removed the required sidebar motion. The active-only Project tree is retained independently: it reduced median stable absolute RSS from about 236.7 MiB to 234.5 MiB while Project model counts remained identical.

| Left-edge drag variant | CPU sample mean | CPU sample peak | Peak RSS delta | Retained RSS delta |
| --- | ---: | ---: | ---: | ---: |
| Frozen route width until drag end | 55.88% | 74.2% | +2592 KiB | +2304 KiB |
| Uncoalesced live route layout | 74.80% | 96.0% | +2096 KiB | +1712 KiB |
| 16 ms latest-width coalescing | 65.89% | 89.8% | +1792 KiB | +1424 KiB |

The 16 ms path reduced sampled mean CPU by 11.9%, peak RSS growth by 14.5%, and retained RSS growth by 16.8% relative to uncoalesced live layout, but manual validation still showed right-content flashing, so it was rejected. Product behavior therefore uses whole-drag route-width freezing; the opaque Slint/native underlays remain live, and the final layout is applied once at drag end. PR Review keeps its additional stable-geometry/token simplification.

## Backlog

### MEM-01 — Extend physical-footprint deltas

Status: Backlog

`src/memory_diagnostics.rs` currently logs absolute physical footprint but calculates previous/baseline deltas only for RSS.

Work:

- Add `phys_footprint_delta_last_mib` and `phys_footprint_delta_baseline_mib`.
- Retain both RSS and footprint peaks for one process lifetime.
- Correlate high-water changes with `[Memory][UI]` physical pixel dimensions.
- Keep the sampler cancellable and on the shared runtime.

Acceptance:

- A resize/fullscreen run shows RSS and footprint deltas in the same sample.
- Logs identify the first event that establishes a new footprint high-water mark.
- Release diagnostics remain opt-in through `USHER_MEMORY_DIAGNOSTICS=1`.

### MEM-02 — Establish a repeatable release benchmark

Status: Backlog

Work:

- Extend or wrap `scripts/monitor-footprint.sh` to capture startup, settled small window, several fixed sizes, fullscreen, exit fullscreen, and a second identical cycle.
- Run at least three iterations per configuration and record median and maximum RSS/footprint.
- Record macOS version, hardware, display scale, renderer, build profile, route, and loaded model counts.
- Include Home, Workflow PR list, one representative large PR review, and Terminal-open scenarios.

Acceptance:

- One command produces comparable output for all renderer/window variants.
- The result distinguishes startup cost, steady-state cost, bounded high-water retention, and cumulative growth.
- Debug-only logging overhead is not used as the production baseline.

### MEM-03 — Renderer A/B comparison

Status: Backlog

Compare the existing compile-time renderer variants:

- Default FemtoVG/OpenGL.
- FemtoVG/WGPU.
- Skia.

Work:

- Use the MEM-02 matrix without changing application models or test data.
- Record RSS, physical footprint, fullscreen high-water retention, launch time, resize smoothness, and visual regressions.
- Confirm the selected backend explicitly; do not infer it only from enabled Cargo features.

Acceptance:

- Results show the trade-off for both RSS and physical footprint.
- A production renderer change is made only if total memory, responsiveness, rendering correctness, and packaging are acceptable.
- Update `README.md`, `README.zh.md`, and both architecture documents if the production renderer decision changes.

Existing architecture notes say the WGPU path reduced physical footprint but increased RSS in earlier macOS tests. Treat that as a hypothesis to revalidate against the repeatable release benchmark, not as a final selection.

### MEM-04 — Native macOS window compositing A/B

Status: Backlog

The current macOS window enables a transparent titlebar and sets the native window to non-opaque in `src/platform/macos.rs`.

Work:

- Compare the current configuration with an opaque-window variant while keeping renderer, dimensions, route, and content constant.
- Measure `IOSurface`, graphics footprint, total physical footprint, and RSS.
- Inspect titlebar, background, shadows, corners, fullscreen transitions, light/dark themes, and traffic-light controls.

Acceptance:

- Adopt an opaque configuration only when it produces a meaningful repeatable saving without unacceptable visual regressions.
- Keep all adaptation in application-owned code; do not modify vendored Material sources.

### MEM-05 — Isolate renderer surface retention

Status: Backlog

Work:

- Adapt the existing `dpi-flicker-repro/` into a minimal memory reproducer, or create a separate isolated case if changing it would mix DPI and memory variables. Use the same renderer and macOS window flags.
- Verify whether the high-water retention occurs without project models, routing, native tray integration, or feature controllers.
- Compare normal resize, fullscreen transition, display-scale change, and repeated cycles.
- If the minimal reproducer confirms framework/backend ownership, search for an upstream fix and prepare a focused upstream report.
- Do not recreate the GL context or renderer surface in production without measuring flicker, state loss, and teardown safety.

Acceptance:

- The retained memory is attributed to either application-owned window configuration, Slint/backend behavior, or a smaller reproducible interaction between them.
- Any workaround has an explicit regression test and benchmark result.

### MEM-06 — Tune Tokio worker count

Status: Backlog, lower priority

The shared Tokio runtime currently uses the default multi-thread worker count. Startup measurements showed the process growing from about 5 to about 15 threads when the runtime-backed services initialized.

Work:

- Benchmark explicit worker counts such as 2 and 4 under HTTP, project loading, Workflow, Terminal, update checks, and shutdown.
- Record responsiveness, queueing latency, CPU utilization, thread count, RSS, footprint, and shutdown behavior.

Acceptance:

- Reduce workers only when concurrent operations and UI responsiveness do not regress.
- Treat any memory saving as secondary; resident thread stacks were below 1 MiB in the baseline.

### MEM-07 — Evaluate malloc fragmentation

Status: Backlog, lower priority

The baseline showed roughly 10 MiB of malloc fragmentation. This is much smaller than graphics-memory retention.

Work:

- Re-measure fragmentation in a release build after realistic navigation and PR review workloads.
- Identify large allocation owners before considering a different allocator.
- If comparing allocators, include binary size, startup time, CPU, RSS, footprint, and macOS compatibility.

Acceptance:

- Do not introduce another allocator without a repeatable material improvement.
- Do not attribute framework/Objective-C/graphics allocations to the Rust allocator without evidence.

### MEM-08 — Measure large PR review ownership

Status: Backlog

Idle Home measurements do not exercise PR review. Existing caches are bounded, but their retained values can still be large for a large diff.

Work:

- Use a fixed large PR fixture and the existing `[Workflow][PR Review][Memory]` events.
- Run the existing `scripts/measure-pr-review-memory-ab.sh` synthetic A/B as a repeatable supplement, not as a replacement for the real provider fixture.
- Measure opening the review, selecting several files, syntax highlighting, expanding/collapsing context, full-file view, commit-range switching, route changes, and leaving the Workflow lifecycle domain.
- Confirm that collapsed copy-on-write overrides and out-of-domain review state are released.
- Record cache entry counts and retained bytes without logging source content or credentials.

Acceptance:

- Repeated file/range navigation stays within documented cache capacities.
- Leaving the Workflow lifecycle domain releases heavy review state.
- Optimize only allocations demonstrated by this scenario; do not infer them from the idle Home baseline.

### MEM-09 — Bound loaded Chat session ownership

Status: Measurement first

The UI window and Markdown cache are bounded, but `ChatSessionService` keeps every history page explicitly loaded during the process lifetime. The virtual layout prefix also has one `f32` entry per loaded message. This is user-driven growth, not evidence of an idle leak.

Work:

- Add diagnostics for loaded message count, source text bytes, layout-prefix capacity, mounted Slint rows, and render-cache bytes.
- Benchmark startup 20 messages, then 100, 1,000, and 10,000 loaded messages using fixed-size fixture content; record RSS before load, after load, after leaving Chat, and after re-entering.
- If retained Rust ownership is material, keep only a bounded in-memory history window while preserving JSONL as the source of truth and stable pagination anchors. Do not merely reduce the 56-row Slint window.

Acceptance:

- Slint rows remain at or below the documented 56-row window.
- Leaving Chat releases UI/Markdown allocations; any retained Rust session bytes are visible in diagnostics.
- If a Rust history window is introduced, revisiting older pages remains correct and repeated pagination does not create duplicates or lose newly persisted messages.

### MEM-10 — Attribute Terminal copy layers

Status: Measurement first, lower priority

Terminal scrollback is bounded, but a visible frame can exist simultaneously in the `vt100` parser, `TerminalDrawerState.lines`, the thread-local cached rendered lines, and Slint span models. These are bounded copies and should not be called a leak without a long-output measurement.

Work:

- Add owned-byte estimates for parser cell capacity, rendered line/span strings, cached UI lines, and selection text.
- Run plain text, ANSI-heavy text, wide-character text, and continuous output at narrow/wide viewport sizes until the 1,000-row scrollback limit is reached.
- Compare open, closed, same-project reopen, project switch, and shutdown.
- Only after attribution, evaluate reducing duplicate string ownership or updating only changed visible rows.

Acceptance:

- Settled memory plateaus after the configured scrollback capacity.
- Project switch and drawer close follow the stable Terminal lifecycle contract.
- Any optimization preserves ANSI styles, wide glyph alignment, selection, scrollback, and the 16 ms refresh throttle.

### MEM-11 — Audit dependency feature footprint

Status: Backlog, lower priority

The locked direct-feature tree currently shows broad features including `tokio/full`, reqwest defaults plus `rustls-tls`, and default `image` codecs. The resolved graph contains both native-TLS and rustls paths. This is evidence of a broad link/startup surface, not proof of proportional steady-state RSS.

Work:

- Use `cargo tree -e features --locked` and `cargo tree -d --locked` as the before/after dependency record.
- Determine the exact Tokio APIs used before replacing `full` with a minimal feature list.
- Determine whether application reqwest and `self_update` can both disable default TLS/features without breaking update, proxy, and platform behavior.
- Restrict `image` codecs only after inventorying every accepted project logo, icon, clipboard, and bundled asset format.
- Compare clean release binary size, launch RSS/footprint, first-use RSS for HTTP/image/update paths, build time, and packaging.

Acceptance:

- No feature is removed only because it appears large in the dependency tree.
- HTTP TLS, streaming updates, clipboard images, project logos, tray assets, and packaging pass their regressions.
- Record binary/startup improvements separately from steady-state RSS; reject a change that saves only negligible RSS while increasing compatibility risk.

### MEM-12 — Separate interaction CPU from retained RSS

Status: Backlog

The accepted sidebar and native-resize behavior is visually correct but still shows high sampled CPU. Its retained RSS deltas are small and noisy compared with the renderer footprint, so CPU and memory require separate gates.

Work:

- Add frame-time or signpost data to the existing sidebar-toggle and drag benchmark instead of relying only on process-wide sampled CPU.
- Record mean/p95 frame time, dropped-frame count, sampled CPU, peak RSS delta, and settled RSS delta.
- Profile the Slint event loop and layout/model work during interaction before changing animation timing or geometry ownership.

Acceptance:

- A CPU optimization must preserve the required spring motion and eliminate the previously observed flashing.
- Do not accept a visually regressive variant merely because it has lower CPU.
- Do not describe a transient interaction RSS delta as a steady-state memory saving.

### MEM-13 — Verify Workflow reachability cleanup

Status: Correctness and measurement

The product intentionally retains Workflow and PR Review while either route is reachable through Back/Forward history. The history is capped at 64 entries. `workflow_became_unreachable` is currently emitted only when the immediately previous route is a Workflow route and the post-transition history has none. If later non-Workflow navigation evicts the final retained Workflow entry, that condition may miss the actual reachable→unreachable edge.

Work:

- Add pure Router tests for direct leave, retained Back target, Back/Forward movement, forward-stack clearing, and eviction of the final Workflow entry at the 64-entry limit.
- Track reachability before and after each history mutation rather than inferring it only from `previous`, if the tests confirm the missed edge.
- When the domain becomes unreachable, clear PR Review controller caches/models, PR-list visited pages, highlight resources, and reconstructable Workflow details through their owning modules. Preserve Tag execution history where the current stable behavior requires it.
- Record `[Workflow][PR Review][Memory]`, root Slint model counts, RSS, and footprint before leave, while still reachable, and after true unreachability.

Acceptance:

- Opening Settings/About and using Back restores the exact active review as required by the PR Review spec.
- Cleanup fires exactly once when no current, Back, or Forward route can return to Workflow/PR Review, including bounded-history eviction.
- Late async responses remain sequence-guarded and cannot repopulate cleared state.
- The fixed large PR fixture shows heavy owned models/caches released after the true unreachable event; allocator high-water behavior is reported separately from live ownership.

## Items not currently worth prioritizing

- Idle Home UI models: the measured run had 9 project rows, 10 tags, no PR rows, no review lines, and only a few KiB of model text.
- Project logos in the measured idle fixture: raster images are limited to 128×128 before conversion to Slint images. Reconsider only with a many-distinct-logo scenario because the process-lifetime image cache is not count-bounded.
- Thread stack residency: below 1 MiB in the measured process.
- VSZ: not a useful standalone optimization target.

These items should be reconsidered only when a representative scenario attributes material retained memory to them.

## Suggested implementation batches

Execute one batch at a time. A batch ends with a documented **adopt**, **reject**, or **insufficient evidence** decision before the next batch starts. Do not merge several speculative changes into one A/B process.

### Batch 0 — Measurement contract

Items: MEM-01, MEM-02

- Extend diagnostics with footprint previous/baseline deltas and process-lifetime peaks.
- Make the release benchmark emit machine-readable raw samples and a summary.
- Freeze fixtures, routes, window sizes, scale factor, renderer/backend confirmation, warm-up time, cycle count, and model/cache counts.
- Run at least three fresh processes per scenario. Keep raw runs; report median, maximum, and cycle-to-cycle settled slope.

Exit gate: one command can reproduce Home, Projects, PR list, fixed large PR Review, Chat history, and Terminal-output scenarios without using the debug build as the production baseline.

### Batch 1 — Graphics and window ownership

Items: MEM-03, MEM-04, MEM-05

- First compare default FemtoVG/OpenGL, FemtoVG/WGPU, and Skia with no application model changes.
- Then compare current transparent/non-opaque AppKit composition with a minimal opaque variant.
- Use the minimal reproducer only after the two A/B runs identify renderer/window ownership.

Exit gate: attribute the fullscreen high-water mark to renderer, native-window composition, or their interaction. Any adopted change must pass visual, resize, fullscreen, theme, title-bar, packaging, and responsiveness checks in addition to memory.

### Batch 2 — PR Review large-object budget

Items: MEM-08, MEM-13

- Measure base diff, range diffs, expanded overrides, highlights, immutable sources, timeline/commit models, avatars, and Slint rows separately.
- Validate file/range navigation and leaving the Workflow lifecycle domain.
- Verify both intentional history retention and cleanup after the final Workflow route becomes unreachable, including history-cap eviction.
- If count-bounded caches retain material large entries, add byte budgets or skip caching over-budget entries while preserving the existing count limits.

Exit gate: the fixed large fixture has an explained owned-byte total, bounded repeated-navigation plateau, verified restoration while history remains reachable, and verified release after true unreachability. No cache policy or lifecycle change is allowed without this evidence.

### Batch 3 — User-loaded feature state

Items: MEM-09, MEM-10, plus Workflow job-log verification

- Benchmark Chat pagination growth and Terminal long output independently.
- Confirm Tag's 32 KiB-per-step bound and audit other Job producers.
- Measure deep PR-list pagination and many distinct project-logo keys; add a count/byte bound only if either owner is material.
- Preserve route/session behavior while changing only an owner proven material.

Exit gate: each feature has a documented growth curve, plateau or release point, and regression result. A user-controlled bounded cache is not treated as a leak.

### Batch 4 — Runtime and dependency surface

Items: MEM-06, MEM-11

- Tune Tokio workers only after representative concurrent HTTP, Git, Terminal, update, and shutdown workloads exist.
- Reduce dependency features one package at a time, rebuilding and rerunning the same release matrix after every change.

Exit gate: adopt only changes with repeatable launch/binary/RSS or operational benefit and no compatibility or latency regression.

### Batch 5 — Allocator and residual heap

Items: MEM-07

- Revisit fragmentation only after graphics and feature-owned memory are attributed.
- Compare allocators only if release workloads still show material allocator-owned fragmentation.

Exit gate: the allocator experiment must beat the existing allocator on a predeclared memory threshold without unacceptable CPU, startup, binary, packaging, or platform cost.

### Batch 6 — Interaction performance guard

Items: MEM-12

- Optimize sidebar/resize CPU from frame/layout evidence.
- Add the accepted benchmark to the regression routine after visual approval.

Exit gate: required animation and visual correctness remain intact, while frame-time/CPU improves repeatably; memory results are reported independently.

### Default decision thresholds

Declare scenario-specific thresholds before running an experiment. Until a different threshold is recorded, use all of the following:

- Treat cumulative growth as actionable when settled RSS or footprint continues rising across repeated identical cycles instead of reaching a plateau.
- For an optimization claim, require at least three fresh processes and a median settled saving of at least 10 MiB or 5% in the targeted metric.
- Reject the change if p95 interaction time regresses by more than 5%, a stable behavior contract breaks, or visual/package validation fails.
- Smaller wins may still be accepted for low-risk cleanup, but must be labeled as such and must not displace a P0/P1 investigation.

### Per-batch evidence record

Append the following fields when a batch completes:

- Date, commit, dirty-tree note, hardware, macOS version, display/scale factor.
- Build profile, exact command, enabled features, confirmed renderer/backend.
- Fixture identity and size, route, window dimensions, warm-up, repetitions.
- Raw result artifact path; median/max RSS and footprint; peak/settled delta; settled slope; owned bytes; CPU/frame-time where relevant.
- Hypothesis, single changed variable, result, adopt/reject/insufficient-evidence decision, regressions, and rollback condition.

After completing an item, append the tested commit, command, environment, before/after table, decision, and any remaining risk. Do not replace the original baseline; keep later results clearly dated so regressions remain comparable.

## Related references

- `src/memory_diagnostics.rs`: process RSS/footprint sampler.
- `scripts/monitor-footprint.sh`: macOS footprint/RSS and resize sampling.
- `scripts/measure-pr-review-memory-ab.sh`: synthetic PR Review selected-file memory A/B.
- `src/lib.rs`: startup checkpoints and diagnostics wiring.
- `src/platform/macos.rs`: native macOS window configuration.
- `src/app_runtime.rs`: shared Tokio runtime.
- `src/workflow/pr_review.rs`: bounded PR review caches and memory diagnostics.
- `docs/architecture.md`, `docs/architecture.zh.md`: current renderer and ownership decisions.
- `docs/specs/pr-review.md`: stable PR review memory bounds.
