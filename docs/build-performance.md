# Build Performance and Artifact Policy / 构建性能与产物策略

## Policy

The workspace uses compact profiles for everyday development:

```toml
[profile.dev]
debug = "line-tables-only"
split-debuginfo = "off"
incremental = true

[profile.test]
debug = "line-tables-only"
split-debuginfo = "off"
incremental = true

[profile.ide]
inherits = "dev"
debug = "none"
incremental = false

[profile.release-fast]
inherits = "release"
lto = "thin"
codegen-units = 8
incremental = true
```

This preserves incremental compilation and source file/line information for
backtraces. It intentionally does not preserve full local-variable and type
information for LLDB. Use the isolated profile when that information is needed:

```sh
cargo build --profile dev-full
```

Background IDE diagnostics use `profile.ide`: they still check the complete
workspace and all targets, but do not retain debug information or incremental
state. VS Code/Cursor is configured by `.vscode/settings.json`. In RustRover,
open **Settings | Rust | External Linters** and set **Additional arguments** to:

```text
--profile ide --locked
```

The same check can be reproduced without an IDE:

```sh
cargo ide-check
```

Release behavior is unchanged. `profile.release` continues to disable debug
information and incremental compilation, and release automation continues to
invoke Cargo with `--release`. Routine optimized local builds may instead use
`cargo build --profile release-fast --locked`; the isolated profile retains the
release runtime policy while using Thin LTO, eight codegen units, and incremental
compilation. Its output under `target/release-fast` is not a final distributable
artifact. `cargo bundle --profile release-fast` provides the matching local
application bundle when needed.

## Controlled A/B result

Measurements were taken on 2026-07-23 on Apple Silicon macOS with Cargo 1.92.0
and rustc 1.92.0. Each side used a fresh, independent temporary target
directory, the same locked dependency graph, default FemtoVG features, and the
same source tree.

- A: Cargo's previous macOS dev defaults (`full` + `unpacked`, incremental on).
- B: the committed policy (`line-tables-only` + `off`, incremental on).
- Physical disk usage was measured with `du -sk`.
- Rust and UI samples changed actual harmless source comments, not only mtimes.
- The execution order was alternated where practical to reduce thermal/order
  bias.

| Workload | A | B | Change |
| --- | ---: | ---: | ---: |
| Cold `cargo build --locked` | 111.62 s | 97.48 s | 12.7% faster |
| Cold target size | 5.87 GiB | 3.83 GiB | 34.8% smaller |
| Three ordinary Rust source rebuilds, mean | 4.12 s | 3.40 s | 17.4% faster |
| Three Slint UI source rebuilds, mean | 20.79 s | 19.50 s | 6.2% faster |
| Target size after the mixed workload | 6.77 GiB | 4.70 GiB | 30.6% smaller |
| Loose `debug/deps/*.rcgu.o` after workload | 5,386 files | 0 files | eliminated |

An additional candidate using `line-tables-only` + `packed` reduced cold disk
usage to 4.16 GiB, but ordinary repeated Rust builds averaged about 6.84 s
because a dSYM was repacked after each link. It was rejected in favor of `off`.
The opt-in `dev-full` profile accepts that packing cost because full-symbol
debugging is expected to be occasional and isolated.

### IDE-analysis A/B

The first post-clean `cargo run`, followed by background IDE analysis, occupied
6.66 GiB physically (`du -sk`; Finder reported about 7.26 GB). Inspection found
2.79 GiB in `target/debug/incremental`, including multiple `usher-ui` and
`usher-lib` analysis identities. There were still zero loose `.rcgu.o` files:
this was a separate IDE-cache issue, not a regression of the dev profile fix.

On the same source tree, `cargo check --workspace --all-targets` with the normal
incremental dev profile occupied 5.51 GiB in a clean target after the preceding
build. The new cold `profile.ide` all-target check took 61.49 s and occupied
1.11 GiB with zero bytes of retained incremental state. Combined with the
3.83 GiB normal build, the projected steady baseline is 4.93 GiB: 1.73 GiB
(26.0%) below the observed 6.66 GiB workspace.

| IDE check behavior | Normal dev profile | `ide` profile |
| --- | ---: | ---: |
| Cold all-target check artifacts | included in 5.51 GiB debug tree | 1.11 GiB isolated tree |
| Three real Rust edits, mean check time | 1.00 s | 2.27 s |
| Incremental cache after edits | 2.14 GiB | 0 |
| Growth across those three checks | about 81 MiB | 0 |

The tradeoff is deliberate: diagnostics after a real source edit take roughly
1.3 seconds longer in this sample, while repeated idle checks do not accumulate
large incremental object graphs. Interactive `cargo run`, `cargo build`, and
`cargo test` retain incremental compilation.

These figures are directional, not a promise for every machine. The main
invariant is independently verifiable: normal dev/test builds must not leave
loose `.rcgu.o` files in `target/debug/deps` on macOS.

### 2026-08-28 dev build-override A/B

P2 tested `[profile.dev.build-override] opt-level = 2` on the same Apple M2 Pro
(10 cores, 32 GiB) using rustc/Cargo 1.97.1 and source revision `b6f6d85` plus
the current P2 worktree. Each side ran `cargo check --locked` in a separate,
fresh target directory; the only B-side change was
`CARGO_PROFILE_DEV_BUILD_OVERRIDE_OPT_LEVEL=2`.

| Candidate | Cold wall time | Change |
| --- | ---: | ---: |
| Existing dev profile (A) | 128.67 s | baseline |
| Build override `opt-level=2` (B) | 180.23 s | 40.1% slower |

The candidate was rejected and is intentionally absent from `Cargo.toml`.
Optimizing every build script and procedural macro increased the cold critical
path substantially on the current dependency graph; the earlier estimated
Slint code-generation gain did not offset that cost. No nightly `-Z threads`
setting enters the stable build chain.

P2 于 2026-08-28 在同一台 Apple M2 Pro（10 核、32 GiB）、rustc/Cargo
1.97.1、`b6f6d85` 加当前 P2 工作树上验证了
`[profile.dev.build-override] opt-level = 2`。A/B 分别使用全新且独立的 target
目录，B 侧唯一变量为 `CARGO_PROFILE_DEV_BUILD_OVERRIDE_OPT_LEVEL=2`。A 为
128.67 秒，B 为 180.23 秒，回退 40.1%，因此不写入 `Cargo.toml`。当前依赖图中，
统一优化 build script 与 proc-macro 的冷编译代价超过了预期的 Slint 代码生成收益；
nightly `-Z threads` 也不进入 stable 交付链。

## 2026-08-03 regression baseline

The UI compile graph has grown enough that the previous profile optimization is
no longer sufficient by itself. A measured incremental build after a Slint
source change took 66.7 seconds on a 10-core Apple M2 Pro with 32 GiB of RAM:

| Build unit | Time | Share of critical path |
| --- | ---: | ---: |
| `usher-ui` build script / Slint code generation | 9.77 s | 14.7% |
| Generated `usher-ui` Rust crate | 43.20 s | 64.8% |
| `usher` library | 10.21 s | 15.3% |
| `usher` binary | 2.66 s | 4.0% |

The generated `usher-ui` source is 48.98 MB and 523,456 lines. Its current
artifacts include an approximately 770 MB rlib and 80-104 MB rmeta files. The
hand-written Slint tree grew from 15,574 lines at commit `24061bd` to 23,426
lines, an increase of about 50%. Since `ui/main.slint` imports and instantiates
every route, a change to any of its 217 tracked Slint/asset dependencies
regenerates one large Rust module. Cargo cannot parallelize the build-script ->
`usher-ui` -> `usher` -> binary dependency chain.

The cache is still functioning correctly: a no-change build took 0.81 seconds,
and a harmless Rust-only edit in `src/lib.rs` took 6.11 seconds. The 9.3 GiB
target directory and its 5.8 GiB incremental subtree warrant cleanup work, but
they are not the primary explanation for the Slint edit latency. A
`slint-viewer --check` of the same root completed in 0.65 seconds, establishing
that an interpreter-based UI feedback loop is viable.

## Prioritized optimization roadmap

Priorities describe execution order, not just importance. Each batch must be
measured and accepted independently so a speculative optimization cannot hide a
regression from another change.

| Priority | Batch | Purpose | Risk |
| --- | --- | --- | --- |
| P0 | Fast UI feedback loop | Avoid static Rust regeneration during ordinary UI iteration | Low |
| P1 | Cargo/profile hygiene | Remove unnecessary outputs and duplicate build identities | Low-medium |
| P2 | Slint compile boundaries | Make large UI areas independently compilable | High |
| P3 | Generated-tree reduction and toolchain experiments | Reduce remaining work after boundaries are fixed | Medium-high |

### P0 - Fast UI feedback loop

Status: implemented and runtime-verified on 2026-08-03.

1. Add documented project commands for the measured fast paths:
   - syntax/type validation with `slint-viewer --check -L material=...`;
   - visual iteration with `SLINT_LIVE_PREVIEW=1 cargo run --features live-preview,ui-components`;
   - Rust-only iteration with `cargo check --locked`;
   - static `cargo run --locked` at integration checkpoints.
2. Add a Cargo alias or application-owned script so contributors do not have to
   reproduce environment variables and Material library paths manually.
3. Keep live preview opt-in and development-only. Default and release builds
   must retain static Slint compilation.
4. Document live-preview limitations and run a normal static build before each
   handoff involving UI behavior.

Acceptance gate:

- UI syntax/type feedback remains below 2 seconds on the baseline machine.
- Live-preview edits do not invoke Cargo/rustc after the preview application is
  running.
- `cargo build --locked` and `cargo test --lib --locked` remain unchanged.
- The normal static application passes the affected route/layout manual check.

Implemented entry points:

- POSIX: `./scripts/ui-check.sh` and `./scripts/ui-live-preview.sh`;
- PowerShell: `scripts/ui-check.ps1` and `scripts/ui-live-preview.ps1`.

The POSIX root-UI check completed in 0.62 seconds. A portable, checksum-verified
PowerShell 7.6.3 runtime executed both PowerShell entry points successfully.
Outside the filesystem sandbox, changing and then restoring the window title
produced two `Reloaded component MainWindow` events. Process inspection during
the reload showed only `pwsh` and `usher`, with no Cargo or rustc process.
Each reload increased RSS by roughly 300 MiB in this diagnostic run, so live
preview remains a short-lived development tool rather than a long-running app
mode.

### P1 - Cargo and profile hygiene

Status: implemented and measured on 2026-08-03.

1. Verify that no packaging, FFI, or supported target consumes
   `libusher_lib.{dylib,dll,so}`. If confirmed, remove the stale `cdylib` crate
   type and retain the normal Rust library used by the `usher` binary.
2. A/B test `debug = "none"` for the generated `usher-ui` package under dev and
   test profiles. Keep source line tables for application-owned Rust crates.
3. Verify `usher-ui` default features and root feature forwarding. Preserve a
   usable standalone default while ensuring the application root disables it
   and forwards exactly one renderer explicitly.
4. Correct the stale `material-1.0-fixed` Slint IDE library path to the current
   application-owned `material-1.7.1-fixed` mapping.
5. After profile/feature identities are finalized, perform one controlled target
   cleanup. Do not present `cargo clean` itself as a compile-speed optimization.

Acceptance gate:

- Run three warm Rust edits and three warm Slint edits per candidate, alternating
  candidate order and using the same locked source/dependency graph.
- Keep a candidate only if it improves the relevant median build time by at
  least 10%, or reduces steady target size by at least 10% without slowing the
  relevant median by more than 5%.
- The default application must not produce an unused dynamic library.
- Renderer, live-preview, dev, test, IDE, and release feature sets must each have
  one intentional artifact identity.
- Run the normal validation suite and the release verification script when
  crate types or release outputs change.

Implemented results:

| Candidate | Baseline warm samples | Candidate warm samples | Median change |
| --- | --- | --- | ---: |
| Remove unused `cdylib` | 6.25 / 5.11 / 5.12 s | 4.77 / 3.92 / 3.90 s | 5.12 -> 3.92 s, 23.4% faster |
| `usher-ui` `debug = "none"` | 47.51 / 46.27 / 45.84 s | 41.61 / 42.27 / 40.81 s | 46.27 -> 41.61 s, 10.1% faster |

The generated UI rlib fell from 792,121,424 bytes to 482,158,224 bytes, a
39.1% reduction. The first profile-identity migration build took 2 minutes 35
seconds; that one-time warm-up is not included in the warm-edit comparison.
An empty `usher-ui` standalone default was tested and rejected because Slint's
winit backend requires a renderer. The package therefore retains FemtoVG for
direct builds; the application root already uses `default-features = false` and
continues to select and forward exactly one renderer explicitly. The VS Code
Material mapping now matches `material-1.7.1-fixed`.

A controlled `cargo clean -p usher-ui` removed 13.1 GiB of Cargo artifacts. In
physical `du` measurements, `target/` fell from about 18.4 GiB to 6.5 GiB and
`target/debug/incremental` from about 11.4 GiB to 1.2 GiB before rebuilding the
current intentional identities. This cleanup is a migration action, not a
recurring development command.

The two stale `target/debug/libusher_lib.dylib` artifacts left by the previous
crate type were deleted explicitly. Subsequent dev and release builds did not
recreate them. The UI policy baseline was also restored by registering provider
identity marks separately from command icons and aligning the Lucide allowlist
with its existing asset registry.

The native release verification also passed after a 14 minute 27 second cold
release build. It produced the application bundle, three tar archives, and a
DMG; when Finder automation was unavailable, the existing script's `hdiutil`
fallback completed the DMG successfully.

### P2 - Real Slint compile boundaries

Status: measured and rejected on Slint 1.17.1 on 2026-08-03; no experimental
module code is retained.

1. Measure route contribution with small diagnostic root files before choosing
   boundaries. At minimum isolate PR Review, the UI Components showcase,
   Workflow, and the shared shell/foundations.
2. Evaluate moving the UI Components showcase behind a development feature or
   into a dedicated development binary. Preserve its mandatory component
   registry and acceptance role even if it leaves the production route graph.
3. Prototype Slint 1.17.1 `experimental-module-builds` with separate Cargo
   crates for shared foundations, PR Review, the showcase, and the application
   root. A successful prototype must compile imported libraries as Rust crate
   dependencies instead of flattening them back into `main.rs`.
4. Keep exported globals, structs, enums, callbacks, and root property names
   stable across the split. Rust bindings must continue to touch Slint only on
   the event loop.
5. Update both architecture language versions and the UI design spec if route
   availability or ownership changes.

Acceptance gate:

- A single feature-page edit must not regenerate or recompile unrelated page
  crates.
- Generated source for the application root should shrink by at least 30%.
- The median static UI edit-to-build time should fall below 25 seconds on the
  baseline machine; otherwise the added experimental complexity is not
  justified.
- Cold build time and steady target size may not regress by more than 15%.
- Compile the full root UI and manually inspect navigation, overlays, terminal,
  Workflow, PR Review, and the component showcase before acceptance.

Measured diagnostic roots:

| Area | Generated Rust | Share of 48,976,640-byte root |
| --- | ---: | ---: |
| PR Review workspace | 11,588,003 bytes | 23.7% |
| UI Components showcase | 10,319,184 bytes | 21.1% |
| Workflow page | 8,455,559 bytes | 17.3% |
| Shared navigation shell probe | 546,083 bytes | 1.1% |

These areas overlap through shared imports, so their sizes are diagnostic
contributions rather than additive savings. PR Review and the showcase were the
only plausible boundary combination.

A two-crate minimal prototype and a repository PR Review library prototype both
showed that Slint 1.17.1 `experimental-module-builds` externalizes public data
types and globals, but still expands imported component implementations into
the consuming root's generated Rust. The repository prototype generated an
11,336,834-byte PR library while the application root grew from 48,975,280 to
49,038,044 bytes instead of shrinking. It therefore could not isolate page
edits or meet the 30% root reduction gate, and all prototype code was removed.
Alternating experimental and normal build-script features left stale Cargo
fingerprints that forced repeated 21-34 second no-change checks. A final
package-scoped `cargo clean -p usher-ui` removed 11.5 GiB of prototype artifacts;
after rebuilding the one retained identity, a no-change `cargo check --locked`
returned to 0.91 seconds.

Moving only the showcase to a separate development binary has a theoretical
maximum root reduction of 21.1% before shared-import overlap, below the 30%
gate, while also removing a documented route from the application. That
candidate was rejected without changing route behavior. P2 should be revisited
only when a Slint version provides real cross-crate component code generation,
or after an explicit product decision permits a separate PR Review process or
removes development surfaces from the application route graph. That product
decision was later made for UI Components, and P4 implements the narrower
feature-gated boundary without reviving the failed cross-crate prototype.

### P3 - Generated-tree reduction and toolchain experiments

Status: implemented and measured on 2026-08-03.

1. Use per-route generated-size measurements to remove duplicate component
   instantiations and overly broad imports. Splitting files without creating a
   compilation boundary is not considered an optimization.
2. Keep large showcase matrices out of the production compile graph when the P2
   acceptance work proves an equivalent development surface.
3. Evaluate newer Slint versions in an isolated branch/target only after reading
   their compiler and module-build release notes. Require the same UI regression
   matrix as any other Slint upgrade.
4. Reconsider compiler caches or an alternative linker only for cold builds and
   linking after new timings show those phases are material. The current 32.3
   second Rust frontend phase cannot be fixed by more Cargo jobs or a faster
   linker.

Acceptance gate:

- Every retained experiment has an isolated before/after timing report.
- No optimization may weaken release renderer determinism, event-loop safety,
  project-scoped state, or the Material vendor boundary.
- Update this document with the final measurements and rejected alternatives.

Implemented result:

The UI Components gallery contained many static matrices that instantiated the
same component once for every visual state. They now use Slint repeaters with
data records and shared action dispatch. The visible labels, state variants,
callbacks, menu choices, notifications, dialogs, and feature-route links remain
the same, but each homogeneous matrix generates one delegate item tree instead
of one tree per sample.

| Metric | P2/P3 baseline | Repeater result | Change |
| --- | ---: | ---: | ---: |
| Generated root Rust bytes | 48,975,280 | 44,968,820 | -4,006,460 (-8.18%) |
| Generated root Rust lines | 523,456 | 481,343 | -42,113 (-8.05%) |
| Generated `item_tree` functions | 1,171 | 1,064 | -107 (-9.14%) |
| Three warm Slint edit checks | 41.61 s median from P1 | 28.62 / 28.16 / 27.95 s | 28.16 s median, 32.3% faster |

The first rebuild after changing the generated identity took 87.01 seconds and
is treated as migration warm-up, not a warm sample. The retained change affects
only the application-owned showcase representation; it does not alter root
routes, business state, renderer selection, or vendored Material sources.
After the direct build-script probes and `check`/`run`/test identities had been
exercised, Cargo again reported the generated crate dirty because its
`build_script_build` dependency was rebuilt. A final package-scoped
`cargo clean -p usher-ui` removed 6.7 GiB (336 generated files). Rebuilding the
single retained check identity took 81.59 seconds; the immediate no-change
`cargo check --locked` then returned to 0.93 seconds. This is another
experiment-cleanup action, not a recurring optimization command.

Two smaller/compiler experiments were rejected:

- Replacing every `GitHubIconButton` tooltip locator with direct publication to
  the root tooltip overlay removed only 10,672 generated bytes (0.022%) and five
  item trees while expanding the global tooltip protocol. The prototype was
  removed.
- `SLINT_INLINING=true` increased generated output from 48,975,280 bytes and
  523,456 lines to 71,796,692 bytes and 797,629 lines (+46.6% bytes). The
  default non-inline configuration remains correct.

The official latest Rust API documentation resolved `slint_build` to 1.17.1,
which is already the locked project version, so there was no newer stable
toolchain candidate to A/B in this batch. P3 therefore closes without a Slint
dependency upgrade, linker change, or compiler-cache addition.

### P4 - Feature-gated UI Components playground

Status: implemented and runtime-verified on 2026-08-03.

An explicit product decision now permits the development playground to be
absent from production. The non-default `ui-components` Cargo feature is
forwarded to `usher-ui`; its build script maps the stable `@ui-components`
library name either to the real playground entry or to a tiny disabled stub.
Both sidebar instances use the same capability, and Rust excludes the fourth
primary-navigation route and normalizes direct disabled-route navigation to
Home. Existing release commands do not enable the feature.

The UI validation and live-preview scripts deliberately enable the real
playground. The equivalent direct hot-reload command is
`SLINT_LIVE_PREVIEW=1 cargo run --features live-preview,ui-components`.
Changing and restoring
`ui/components/showcase/github_ui_components_gallery.slint` in a running
process each produced `Reloaded component MainWindow`, proving that the mapped
page remains in the live-preview watcher graph.

| Generated root metric | Feature enabled | Default/production | Change |
| --- | ---: | ---: | ---: |
| Rust bytes | 44,970,152 | 39,131,311 | -5,838,841 (-12.98%) |
| Rust lines | 481,420 | 420,331 | -61,089 (-12.69%) |

The disabled build-script dependency list contains only
`ui_components_disabled.slint`, not `ui/views/ui_components.slint` or the
gallery. Showcase-only edits therefore do not invalidate the default UI crate.
Feature switching still creates separate Cargo identities and pays a one-time
rebuild; normal development should keep the feature disabled unless working on
the component registry.

### Measurement protocol for every batch

1. Record `rustc -Vv`, `cargo -Vv`, hardware, source commit, features, profile,
   and target-directory state.
2. Use `cargo build --locked --timings` and save the generated timing report.
3. Measure a no-change build, a real harmless Rust comment edit, and a real
   harmless Slint comment edit; restore and rebuild the original source after
   each sample.
4. Use at least three warm samples for acceptance decisions. Use separate fresh
   target directories for cold A/B results.
5. Record wall time, `usher-ui(build)`, `usher-ui`, `usher`, final link time,
   generated `main.rs` bytes/lines, target size, and incremental size.
6. Run `cargo fmt --all`, `cargo check --locked`, `cargo test --lib --locked`,
   and `git diff --check` before handoff.

### Explicit non-priorities

- Do not increase Cargo job count as a response to the current serial critical
  path.
- Do not disable incremental compilation globally; the existing controlled A/B
  supports retaining it for normal dev/test work.
- Do not switch to release builds for iteration; release LTO and one codegen unit
  intentionally optimize size at the expense of build time.
- Do not use broad `cargo clean` runs as a recurring remedy.
- Do not add `sccache`, mold/lld, or another linker until timings identify cold
  dependency builds or linking as the dominant remaining phase.

## 2026-08-15 baseline and rejected PR filter reduction

Measurements were repeated on the same 10-core Apple M2 Pro with 32 GiB of
memory, Cargo 1.92.0, rustc 1.92.0, the default FemtoVG feature set, and commit
`e9e9e4d`. The workspace had no `target/` directory at the start. A cold
`cargo build --locked --timings` took 165 seconds. The critical generated
`usher-ui` unit alone took 100.85 seconds after its 9.21-second Slint build
script, while the final binary unit took 2.65 seconds. This confirms that the
generated UI Rust frontend remains the dominant cold-build unit; the linker is
not the bottleneck.

Before optimization, the default main UI generated 40,566,802 bytes and
436,532 lines of Rust from 214 tracked Slint/resource inputs. Its artifacts
included a 429,445,528-byte rlib, an 85,605,053-byte rmeta, and 1.89 GiB of
`usher-ui` incremental state after the cold build. The handwritten application
Slint tree had reached 28,290 lines, up from the 23,426-line 2026-08-03
baseline.

Real harmless content edits produced the following warm baseline. Cargo's
reported build duration is used so an unrelated external Cargo lock wait cannot
distort the compiler comparison.

| Scenario | Samples | Median |
| --- | --- | ---: |
| No source change | 0.75 / 0.28 / 0.27 s | 0.28 s |
| `src/lib.rs` comment edit | 7.47 / 3.58 / 3.47 s | 3.58 s |
| `ui/main.slint` comment edit | 51.97 / 31.42 / 29.90 s | 31.42 s |

Direct Slint code-generation probes identified PR Review at 12,917,608 bytes,
Workflow at 10,139,453 bytes, and the PR-list filter/menu dependency surface at
3,715,355 bytes. The PR filter menu represented Reviews, Assignee, Sort, and
the compact More menu as 19 separate static `GitHubFilterMenuItem` instances.
A candidate replaced those duplicates with three fixed option models and three
repeater delegates while preserving labels, values, ordering, 32 px row
geometry, compact section offsets, selection routing, and the existing typed
callbacks. The generated-size results below describe that experiment, not the
current source tree.

| Generated probe | Before | After | Change |
| --- | ---: | ---: | ---: |
| PR filter/list components | 3,715,355 bytes | 1,530,462 bytes | -58.8% |
| `GitHubPullRequestFlow` | 5,207,308 bytes | 2,927,141 bytes | -43.8% |
| Workflow page | 10,139,453 bytes | 7,854,494 bytes | -22.5% |
| Default main UI | 40,566,802 bytes | 38,274,867 bytes | -5.65% |
| Default main UI lines | 436,532 | 408,312 | -6.46% |

The initial sequential screen produced baseline samples of 51.97 / 31.42 /
29.90 seconds and candidate samples of 43.84 / 27.59 / 42.84 / 27.06 / 26.96
seconds. The independent medians appeared to improve from 31.42 to 27.59
seconds, or 12.2%. That screen was not a controlled A/B: the sample counts were
unequal, the complete baseline ran before the complete candidate, and both
sides shared changing incremental state. It is retained only as preliminary
evidence and is superseded by the controlled result below.

### Controlled PR-filter A/B

The 2026-08-16 follow-up exported commit `e9e9e4d` into equal-length isolated A
and B source paths. A kept the 19 static menu instances; B contained only the
repeater candidate. Each side used its own target directory, the same locked
dependency graph and default FemtoVG features, one isolated cold build, and one
real Slint-edit warm-up. The measured hot workload then alternated the same
equal-length comment edit in `ui/main.slint` for eight rounds. Order was `BA`,
`AB`, repeated four times. No measured sample was discarded.

| Round | Order | A static | B repeater | B - A |
| ---: | :---: | ---: | ---: | ---: |
| 1 | BA | 60.00 s | 55.54 s | -4.46 s |
| 2 | AB | 38.89 s | 39.08 s | +0.19 s |
| 3 | BA | 56.35 s | 50.67 s | -5.68 s |
| 4 | AB | 35.89 s | 55.53 s | +19.64 s |
| 5 | BA | 40.37 s | 52.99 s | +12.62 s |
| 6 | AB | 39.57 s | 55.37 s | +15.80 s |
| 7 | BA | 60.00 s | 36.03 s | -23.97 s |
| 8 | AB | 56.58 s | 56.54 s | -0.04 s |

The independent totals are bimodal and misleading when separated from their
rounds: A's median is 48.36 seconds and B's is 54.18 seconds. The paired
`B - A` median is only +0.075 seconds. Cargo timing provides the more useful
critical-path breakdown:

| Hot metric | A median | B median | Independent change | Paired B - A median |
| --- | ---: | ---: | ---: | ---: |
| Total | 48.360 s | 54.180 s | +12.03% | +0.075 s |
| Slint code generation | 7.390 s | 7.215 s | -2.37% | -0.255 s |
| Generated `usher-ui` | 38.075 s | 44.020 s | +15.61% | -0.195 s |
| Application library | 2.085 s | 2.310 s | +10.79% | +0.225 s |
| Binary | 2.415 s | 2.720 s | +12.63% | +0.105 s |

Three cold pairs used fresh independent targets with `AB`, `BA`, `AB` order.
The total samples were A = 154 / 164 / 178 seconds and B = 177 / 167 / 170
seconds. Their independent medians are 164 and 170 seconds; paired differences
are +23 / +3 / -8 seconds, with a +3-second median. Median target size was
5.83 GiB for A and 5.65 GiB for B, a 3.2% reduction.

| Cold metric | A median | B median | Independent change | Paired B - A median |
| --- | ---: | ---: | ---: | ---: |
| Total | 164.00 s | 170.00 s | +3.66% | +3.00 s |
| Slint code generation | 10.40 s | 10.34 s | -0.58% | -0.38 s |
| Generated `usher-ui` | 95.79 s | 103.10 s | +7.63% | +4.98 s |
| Application library | 9.34 s | 10.28 s | +10.06% | +0.94 s |
| Binary | 3.05 s | 2.82 s | -7.54% | -0.15 s |

Final decision: reject the repeater candidate and restore the static menu. It
does not improve paired hot or cold wall time by the required 10%, and its 3.2%
target reduction also misses the size gate. The exact generated-code reduction
is real but is too small at the full-root level to produce a reliable build
improvement. The wide and compact fixture checks passed, so correctness did not
cause the rejection. Repeated measurements expanded the temporary targets; all
isolated cold targets were removed after their timing reports and sizes were
recorded. This cleanup is benchmark disposal, not a compile-speed technique.

## 2026-08-16 dependency and Rust upgrade

This batch separated dependency modernization from compiler performance. The
direct-dependency A/B used Rust 1.92.0 on both sides. A was the previous lock
and manifest; B upgraded `webbrowser` 1.2.2 -> 1.2.4, `base64` 0.23.0 ->
0.23.1, and `dark-light` 2.0.0 -> 3.0.0. `dark-light` now has default features
disabled because Usher calls only its synchronous platform API; this removes
the old `async-std` stack on the supported resolver graph.

Both sides used the same source, default FemtoVG features, fresh independent
target directories, and `cargo build --locked --timings`. The dependency
upgrade is retained for maintenance and platform-dependency hygiene, not as a
compile-time optimization:

| Metric | A: previous dependencies | B: upgraded dependencies | Change |
| --- | ---: | ---: | ---: |
| Cold wall time | 134.99 s | 136.93 s | 1.4% slower |
| Cargo-reported time | 134 s | 136 s | 1.5% slower |
| Packages in `Cargo.lock` | 853 | 842 | -11 |
| Unique dependency-tree lines | 612 | 599 | -13 |
| Duplicate-tree lines | 354 | 363 | +9 |
| Target size | 2,591,508 KiB | 2,591,684 KiB | effectively flat |
| Incremental size | 1,100,392 KiB | 1,100,520 KiB | effectively flat |

The macOS callback dependency was then migrated from unmaintained `block`
0.1.6 to `block2` 0.6.2. `RcBlock` retains the copied Objective-C block
lifetime used by notification observers and the application-local event
monitor; the AppKit callback boundary uses ABI-equivalent `*mut c_void` and
converts to `objc::runtime::Object` internally. An isolated locked check of the
application after this migration completed in 3.19 seconds, and Rust 1.97 no
longer reports the `block` future-incompatibility warning. This removes one
lock package and one unique dependency-tree line.

Rustup stable was upgraded from rustc/Cargo 1.92.0 to 1.97.1, LLVM 21.1.3 to
22.1.6, and rustup 1.28.2 to 1.29.0. C below uses the same upgraded dependency
graph as B, so it isolates the compiler/toolchain change:

| Metric | B: Rust 1.92.0 | C: Rust 1.97.1 | Change |
| --- | ---: | ---: | ---: |
| Cold wall time | 136.93 s | 130.07 s | 5.0% faster |
| Cargo-reported time | 136 s | 130 s | 4.4% faster |
| Cold `usher-ui` check unit | 72.8 s | 64.8 s | 11.0% faster |
| Target size | 2,591,012 KiB | 2,780,852 KiB | 7.3% larger |
| Incremental size | 1,099,864 KiB | 1,164,396 KiB | 5.9% larger |

Three Rust 1.97 hot Slint samples were 29.93 / 26.73 / 48.17 seconds, with a
29.93-second median. The comparable Rust 1.92 median was 31.42 seconds, a 4.7%
improvement. The upgrade is retained because the compiler is newer, removes a
future-incompatibility warning, and improves the dominant cold `usher-ui` unit;
it is not presented as a workspace-wide 10% build-speed result. The larger
artifacts are the measured tradeoff.

After the compiler upgrade, `cargo update` refreshed another 68 compatible
transitive packages, including Rustls, ICU, Futures, Ureq, Wasm bindings, and
platform support crates. A subsequent online dry run selected zero additional
packages for Rust 1.97.1. The current graph contains 841 lock packages, 598
unique dependency-tree lines, and 363 duplicate-tree lines. `generic-array`
0.14.9 and old `toml` 0.8 patch releases remain constrained by transitive
consumers; the application's direct `toml` dependency is already on 1.1.

The following candidates were deliberately not retained:

- Parley 0.11 requires Fontique 0.11, while Slint 1.17 exposes its shared
  custom-font collection through Fontique 0.10. A separate collection would
  break alignment with fonts registered into Slint on macOS, so Parley remains
  at 0.10.
- Restricting the application `image` features is not behavior-preserving:
  project logos and downloaded avatars currently accept any decoder enabled by
  `image`, while the Preview spec guarantees PNG/JPEG/GIF/WebP. Slint's macro
  compiler also enables the complete image format set, so narrowing only the
  application dependency would create another feature identity without
  removing the existing full-format compile unit.
- Slint debug metadata was already disabled. Explicitly enabling it increased
  generated output from 40,566,802 bytes / 436,532 lines to 41,094,241 bytes /
  446,045 lines. `SLINT_EMBED_RESOURCES=false` left output effectively flat at
  40,568,133 bytes and introduced runtime source-path dependencies.
- A content-aware build-script output experiment rewrote generated `main.rs`
  only when its bytes changed, but Cargo still rechecked `usher-ui` after a
  Slint comment edit. That sample took 38.14 seconds in total, including 29.2
  seconds for `usher-ui` and 7.6 seconds for its build script. The experiment
  was fully reverted.

## 2026-08-16 opt-in PR Review development shell

This batch keeps the complete PR Review detail page as one feature package
instead of repeating the rejected cross-crate Workflow/PR Review split. The
stable `@pr-review` Slint library entry exposes one public page protocol.
Default and release builds map it to the complete implementation; the explicit
non-default `dev-shell` feature maps the whole package to an empty compatible
implementation. Rust redirects direct `PrReview` navigation to Workflow only
in that development mode. Future Project, notification, or search entry points
can consume the same package without making Workflow route ownership part of
the page contract.

The default generated output remains effectively unchanged from the preceding
40,566,802-byte / 436,532-line baseline. The package switch removes the complete
PR Review render tree from the opt-in shell:

| Generated root metric | Full/default | `dev-shell` | Change |
| --- | ---: | ---: | ---: |
| Rust bytes | 40,566,851 | 27,941,262 | -12,625,589 (-31.1%) |
| Rust lines | 436,535 | 304,945 | -131,590 (-30.1%) |
| Item trees | 984 | 702 | -282 (-28.7%) |
| Tracked dependencies | 214 | 192 | -22 (-10.3%) |
| Direct Slint generation | 6,314 ms | 4,356 ms | -1,958 ms (-31.0%) |

The controlled build used rustc/Cargo 1.97.1 on the same 10-core M2 Pro with
32 GiB, default FemtoVG, commit `e9e9e4d` plus this working-tree candidate, and
separate `target/dev-shell-ab/full` and `target/dev-shell-ab/dev-shell`
directories. One full-then-shell cold pair used `cargo build --locked
--timings`; the order can favor the second run through system caches, so the
cold pair is supporting evidence rather than an independent multi-pair median.
The dominant `usher-ui` reduction and alternating hot pairs point in the same
direction.

| Cold metric | Full/default | `dev-shell` | Change |
| --- | ---: | ---: | ---: |
| Wall time | 177.38 s | 128.56 s | -48.82 s (-27.5%) |
| Cargo-reported total | 177 s | 128 s | -49 s (-27.7%) |
| Generated `usher-ui` unit | 106.5 s | 66.9 s | -39.6 s (-37.2%) |
| `usher-ui` build script | 11.8 s | 9.3 s | -2.5 s (-21.2%) |
| Target size | 7,336,264 KiB | 6,208,248 KiB | -15.4% |
| Incremental size | 3,033,040 KiB | 2,287,568 KiB | -24.6% |

After one real-edit warm-up per target, three equal-length harmless comment
changes in `ui/main.slint` ran in `BA`, `AB`, `BA` order. No measured sample was
discarded:

| Round | Order | Full/default | `dev-shell` | Shell - full |
| ---: | :---: | ---: | ---: | ---: |
| 1 | BA | 41.28 s | 34.44 s | -6.84 s |
| 2 | AB | 40.57 s | 33.84 s | -6.73 s |
| 3 | BA | 41.16 s | 23.82 s | -17.34 s |
| Median | | 41.16 s | 33.84 s | -6.84 s paired |

Independent hot medians improve by 17.8%; the paired-difference median is a
16.6% improvement relative to the full median. Median `usher-ui`, build-script,
and application units were 30.1 / 7.7 / 1.8 seconds for full builds and 25.7 /
5.8 / 2.1 seconds for the shell. The retained batch exceeds the 10% acceptance
gate and the 15% hot target, although its 33.84-second hot median does not reach
the stretch goal below 25 seconds; only the fastest shell sample did.

Changing and restoring only
`ui/views/workflow/pr_review_workspace.slint` completed a warmed `dev-shell`
build in 0.37 seconds without `Compiling usher-ui`, proving that the complete
page is absent from that dependency graph. `scripts/dev-shell-check.sh` and
`scripts/dev-shell-run.sh`, plus PowerShell equivalents, keep this Cargo
identity under `target/dev-shell`. Normal and release commands do not enable
the feature, and final delivery still requires the complete default checks and
manual PR Review validation.

## Cleanup

Changing the profile does not remove historical artifacts. After upgrading,
remove the previous target once:

```sh
cargo clean
```

Thereafter prefer targeted cleanup:

```sh
cargo clean --profile ide
cargo clean --profile dev-full
cargo clean --release
cargo clean --target x86_64-pc-windows-gnu
```

Cargo's automatic global-cache cleanup does not currently bound a workspace
`target/` directory, so monitor it when frequently switching renderers,
features, profiles, toolchains, or cross-compilation targets.

---

## 中文说明

日常 `dev` 与 `test` profile 保留增量编译和 backtrace 所需的源码文件/行号，
但不保留 LLDB 查看局部变量与完整类型所需的全部调试信息。需要深度调试时执行：

```sh
cargo build --profile dev-full
```

IDE 后台诊断改用不生成调试信息、不开启增量缓存的 `ide` profile，但仍检查整个
workspace 的所有 target。VS Code/Cursor 已由项目设置自动配置；RustRover 需要在
**Settings | Rust | External Linters** 的 **Additional arguments** 中填写
`--profile ide --locked`。命令行可用 `cargo ide-check` 复现。

2026-07-23 的同机独立 target A/B 表明，新策略使冷构建磁盘占用减少 34.8%，
混合增量负载后减少 30.6%，并消除了普通构建留下的松散 `.rcgu.o`；冷构建、
普通 Rust 修改和 UI 修改也分别快约 12.7%、17.4% 和 6.2%。原计划中的
`packed` 日常方案虽然节省空间，但每次链接都要重新打包 dSYM，普通 Rust
重编译平均约 6.84 秒，因此没有采用。

本次实际 7.26 GB（Finder 十进制显示；`du` 为 6.66 GiB）的目录中，IDE 增量
检查缓存占 2.79 GiB，且 `.rcgu.o` 仍为 0。新的 IDE profile 在相同全目标检查
中只占 1.11 GiB；加上 3.83 GiB 的正常构建，预计稳定基线约 4.93 GiB，比本次
实测少 1.73 GiB（26.0%）。代价是实际 Rust 修改后的后台检查平均从 1.00 秒
增加到 2.27 秒，但缓存不再随检查持续增长；`cargo run/build/test` 的增量编译
不受影响。

配置修改不会删除旧产物。升级后应执行一次 `cargo clean`，以后尽量按 profile
或交叉编译 target 定向清理。Cargo 当前不会自动限制项目 `target/` 的容量。

## 2026-08-03 当前基线与分批优化计划

本次在 10 核 M2 Pro、32 GiB 内存环境中复现了 66.7 秒的 Slint 增量构建：
`usher-ui(build)` 代码生成耗时 9.77 秒，生成后的 `usher-ui` Rust crate 编译
耗时 43.20 秒，应用库和最终 binary 分别耗时 10.21 秒与 2.66 秒。生成源码已达
48.98 MB、523,456 行，rlib 约 770 MB；手写 Slint 从基线提交 `24061bd` 的
15,574 行增长到 23,426 行。零改动构建仅需 0.81 秒，纯 Rust 注释改动为
6.11 秒，说明缓存正常，主因是 `main.slint` 单入口将所有路由压成一个串行编译单元。

按以下顺序推进，每一批独立测量、独立验收：

| 优先级 | 批次 | 目标 | 合并门槛 |
| --- | --- | --- | --- |
| P0 | 快速 UI 反馈链路 | 日常 UI 修改不触发静态 Rust 重生成 | UI 校验低于 2 秒，live preview 启动后不调用 Cargo/rustc |
| P1 | Cargo/profile 整理 | 去除无用动态库、调试信息和重复 feature identity | 中位耗时改善至少 10%，或空间减少至少 10% 且耗时退化不超过 5% |
| P2 | Slint 编译边界 | PR Review、showcase、共享基础和主窗口独立编译 | 根生成源码减少至少 30%，UI 增量中位耗时低于 25 秒 |
| P3 | 生成树与工具链实验 | 处理 P2 后剩余的主导单元 | 每项都有独立 A/B 且不破坏稳定不变量 |

### P0：先改善开发反馈

补充统一命令或脚本，分别覆盖 `slint-viewer --check`、应用内 live preview、
Rust-only `cargo check --locked` 和阶段性静态 `cargo run --locked`。live preview
继续保持显式、仅开发启用；所有 UI 交付仍需执行普通静态构建并手工检查受影响路由。
当前 `slint-viewer --check` 实测 0.65 秒，可作为本批基线。

本批已于 2026-08-03 实施并完成运行时验证。POSIX 入口为 `./scripts/ui-check.sh` 和
`./scripts/ui-live-preview.sh`，PowerShell 入口为 `scripts/ui-check.ps1` 和
`scripts/ui-live-preview.ps1`。脚本化根 UI 校验实测 0.62 秒；经 SHA-256 校验的
PowerShell 7.6.3 便携运行时真实执行了两个 PowerShell 入口。沙箱外修改并还原窗口
标题分别产生一次 `Reloaded component MainWindow`，期间进程列表只有 `pwsh` 和
`usher`，没有 Cargo/rustc。该诊断中每次重载 RSS 约增加 300 MiB，因此 live
preview 只用于短时开发迭代，不作为长期运行模式。

### P1：低风险构建配置

先确认发布、FFI 和支持平台没有使用动态库，再移除旧模板遗留的 `cdylib`；单独
A/B `usher-ui` 的 `debug = "none"`；统一根包与 `usher-ui` 的 renderer/default
feature 组合；修正 IDE 中过期的 `material-1.0-fixed` 路径。每个候选分别交替
执行三次 Rust 和三次 Slint 热构建。profile 和 feature identity 稳定后才做一次
受控清理，不能把 `cargo clean` 本身当作提速手段。

本批已于 2026-08-03 实施并通过门槛：移除无消费者的 `cdylib` 后，纯 Rust 热
构建中位数从 5.12 秒降到 3.92 秒，改善 23.4%；`usher-ui` 单独关闭 debug info
后，UI 热构建中位数从 46.27 秒降到 41.61 秒，改善 10.1%，rlib 从
792,121,424 字节降到 482,158,224 字节，减少 39.1%。首次 profile identity
迁移预热耗时 2 分 35 秒，不计入热构建对比。实测 `usher-ui` standalone
default 置空会因 winit 缺少 renderer 而无法独立构建，因此保留 FemtoVG 默认值；
应用根已有 `default-features = false`，仍只显式选择并转发一个 renderer。VS Code
Material 路径已同步到 `material-1.7.1-fixed`。

定向 `cargo clean -p usher-ui` 删除了 13.1 GiB Cargo 逻辑产物；按 `du` 的物理
占用，`target/` 从约 18.4 GiB 降到 6.5 GiB，`debug/incremental` 从约
11.4 GiB 降到 1.2 GiB，随后只重建当前有意保留的 identity。该清理仅用于本次
迁移，不作为日常命令。

旧 crate type 遗留的两个 `libusher_lib.dylib` 已精确删除，后续 dev/release 构建
均未重新生成。通过单独登记 provider 身份标识，并让 Lucide allowlist 与已有资产
登记一致，UI policy 基线也已恢复为通过。

原生 release 验证也已通过：冷 release 构建耗时 14 分 27 秒，随后成功生成 App
Bundle、三个 tar 包和 DMG；Finder 自动化不可用时，现有脚本按设计回退到
`hdiutil` 并成功完成 DMG。

### P2：建立真正的 Slint 编译边界

先用诊断 root 测出 PR Review、UI Components showcase、Workflow 和共享 shell
各自的生成体积。评估将 showcase 放入开发 feature 或独立开发 binary，同时保留
其组件登记和验收职责。随后原型验证 Slint 1.17.1 的
`experimental-module-builds`，目标是让共享基础、PR Review、showcase 和应用根
成为独立 Cargo crate，而不是最终再次内联进一个 `main.rs`。若单页面改动仍重编译
无关页面、根生成源码未减少 30%，或 UI 增量中位耗时无法低于 25 秒，则不合并该
实验复杂度。任何路由或职责变化必须同步中英文架构文档和 UI design spec。

本批已完成测量和原型，但在 Slint 1.17.1 上未通过门槛，因此没有保留实验代码。
诊断 root 的生成 Rust 分别为：PR Review 11,588,003 字节（根树的 23.7%）、
UI Components showcase 10,319,184 字节（21.1%）、Workflow 8,455,559 字节
（17.3%）、共享导航 shell 546,083 字节（1.1%）。这些区域存在共享 import，
数值用于定位贡献，不能直接相加为节省量。

最小双 crate 原型和仓库内 PR Review 原型均证明：Slint 1.17.1 的
`experimental-module-builds` 会外置公开 struct/global，但仍把导入组件的实现完整
展开进消费方根生成代码。仓库原型额外生成了 11,336,834 字节的 PR library，根树
却从 48,975,280 字节增加到 49,038,044 字节，既不能隔离页面修改，也不满足根树
减少 30% 的门槛，故已全部撤销。只把 showcase 移到独立开发 binary 的理论上限也
仅为 21.1%，且会改变已记录的应用路由，因此同样否决。后续只有在 Slint 提供真正的
跨 crate 组件代码生成，或产品明确允许 PR Review 独立进程/移除开发路由时，才重新
开启 P2。随后产品已明确允许生产构建移除 UI Components 开发路由；P4 因此实现了
范围更窄的 feature 编译边界，但没有恢复失败的跨 crate 原型。

实验与常规 build-script feature 交替后留下的 Cargo fingerprint 曾导致无改动检查
仍耗时 21～34 秒。最终再次执行包级 `cargo clean -p usher-ui`，删除 11.5 GiB 原型
产物；只重建保留的静态 identity 后，无改动 `cargo check --locked` 已恢复到
0.91 秒。

### P3：按剩余数据继续压缩

本批已于 2026-08-03 完成。UI Components 展示页原先为每个静态视觉状态分别实例化
同一种组件；现在用数据记录加 Slint repeater 表达同构状态矩阵，并统一分发 Toast、
Notification、Dialog 和路由动作。标签、状态、菜单选项、回调和路由行为保持不变。

根生成 Rust 从 48,975,280 字节、523,456 行降到 44,968,820 字节、481,343 行，
分别减少 8.18% 和 8.05%；`item_tree` 函数从 1,171 个降到 1,064 个。三个真实
Slint 注释热改样本为 28.62 / 28.16 / 27.95 秒，中位数 28.16 秒，相比 P1 后
41.61 秒中位数改善 32.3%。生成 identity 变化后的首次 87.01 秒重建只作为迁移
预热，不计入热样本。

直接 build-script 探针以及 `check`、`run`、test identity 交替后，Cargo 再次因
`build_script_build` 输出变化而持续判定生成 crate 为 dirty。最终执行一次包级
`cargo clean -p usher-ui`，删除 336 个生成文件、共 6.7 GiB；唯一保留的 check
identity 重建耗时 81.59 秒，紧接着的无改动 `cargo check --locked` 恢复到 0.93 秒。
这仍是实验收尾动作，不是日常优化命令。

两个实验未保留：IconButton 直接发布 tooltip 锚点只减少 10,672 字节（0.022%）和
5 个 item tree，却扩大了全局 tooltip 协议；`SLINT_INLINING=true` 则把生成树增至
71,796,692 字节、797,629 行，字节数反而增加 46.6%。官方 latest Rust API 文档
当前仍解析到项目已锁定的 Slint 1.17.1，因此本批不升级 Slint，也不引入 linker 或
`sccache`。

### P4：用 feature 隔离 UI Components 演练场

本批已于 2026-08-03 实施并完成编译图及运行时验证。产品决策现已允许生产构建不包含开发
演练场，因此新增非默认 `ui-components` Cargo feature 并转发给 `usher-ui`。
build script 将稳定的 `@ui-components` 名称映射到真实演练场入口或极小的关闭 stub；
两个侧边栏共用同一构建能力，Rust 在关闭时移除第四个主导航映射，并把直接访问该
路由的尝试回退到 Home。现有 Release 命令不会启用该 feature。

UI 校验和 live preview 脚本会显式启用真实演练场；等价命令为
`SLINT_LIVE_PREVIEW=1 cargo run --features live-preview,ui-components`。
运行中修改并还原 `github_ui_components_gallery.slint` 均产生了
`Reloaded component MainWindow`，证明映射后的页面仍在 live preview 监听图中。
生成根 Rust 从开启时的 44,970,152 字节、481,420 行降到默认构建的
39,131,311 字节、420,331 行，分别减少 12.98% 和 12.69%。关闭身份的 build-script
依赖列表只包含 `ui_components_disabled.slint`，不包含页面和 gallery，因此只修改
演练场不会使默认 UI crate 失效。切换 feature 仍会产生独立 Cargo identity 并支付
一次重建成本；只有开发组件登记表时才应开启。

### P5：用 ComponentContainer 拆分生产页面

Slint 1.17.1 的 `ComponentContainer`/component-factory 虽然仍标记为实验能力，
但与此前失败的 `experimental-module-builds` 不同：它可以在运行时挂载另一个 Rust
crate 生成的组件，而不把组件实现再次展开进 `usher-ui`。首个落地边界是 Settings：
`usher-ui-settings` 独立编译 `ui/views/settings.slint`，`src/settings_page.rs` 负责
状态同步和 callback 转发，`ui/main.slint` 只保留 factory、viewport 与更新状态的
小型宿主协议。

Settings 拆出后，默认根生成 Rust 从 39,131,311 字节、420,331 行、924 个
`item_tree` 降到 34,969,310 字节、374,472 行、819 个 `item_tree`；分别减少
10.64%、10.91% 和 11.36%。独立 Settings 根为 4,224,055 字节、46,909 行、
104 个 `item_tree`。一次真实 Settings UI 增量构建只编译
`usher-ui-settings` 与应用根，耗时 16.67 秒，没有出现 `Compiling usher-ui`；
修改主壳仍会重编 `usher-ui`，最近两次实测为 49.85 秒和 47.00 秒。当前根体积尚未
达到 P2 的 30% 减少门槛，因此下一候选仍是约占旧根 23.7% 的 PR Review。

后续页面拆分必须复用以下回归清单：

1. 跨 crate factory 组件的 Slint globals 不共享；必须显式复制初值、转发 callback，
   并同步主题、异步状态和保存反馈。状态桥接遵循两种模式：领域列表/计数用
   snapshot push（参考 Messages `apply_page_snapshot`）；异步 busy/短时反馈用
   operation dual-write（宿主 property/global 与 `*PageBridge::set_*` 同时写入）。
   完成态不得只依赖 Slint `changed`（边沿触发，会漏掉 `false→false`）。独立页
   点击处理只转发 command，不得自行赋值宿主拥有的 busy 标志。
2. 独立页面必须作为主壳 `Flickable` 的同级节点，不能嵌套在主滚动器中；同一路由
   只允许一个滚动所有者。
3. factory 产品必须接收宿主真实 viewport 宽高；固定默认尺寸会破坏响应式布局并
   让内部滚动范围与实际裁剪区不一致。
4. 主壳与独立页面不能重复添加 padding。页面根必须显式绘制
   `GitHubTheme.canvas-default`、把内容宽度限制在可视宽度并固定横向 viewport，避免
   灰色透底和伪横向滚动留白。
5. 检查主壳为普通页面预留的特殊几何。Settings 需要补回 page-stack 统一减掉的
   4px 底部间隔，否则固定状态栏上方会残留空条。
6. 至少在最小宽度和宽窗口中逐个检查 General、Code hosting、AI Agent、
   Diagnostics 的顶部、最底部、滚动条、主题和 callback；不能只确认“滚动条出现”。
7. `SLINT_LIVE_PREVIEW` 只能动态生成主窗口。Slint 1.17 的 `ComponentFactory`
   要求静态 `VWeak`，因此独立 factory 产品的 build script 必须屏蔽该环境变量；
   否则 `live-preview,ui-components` 组合会在 Rust 类型检查阶段失败。
8. Slint 1.17.1 的实验 `ComponentContainer` 不得随路由反复销毁/重建同一个独立
   页面产品。Messages 已复现二次挂载后的文字、选中态与布局损坏。使用首次访问
   懒创建的稳定 shell，隐藏时释放大型 Slint model/行投影，返回前从
   `usher-state-store` 最新 revision 自动 hydration；不得用常驻 1,000 行换稳定性，
   也不得用手工 property 重写代替 Store 同步。
三次可还原的 Settings 注释热改样本为 9.26 / 6.94 / 6.50 秒，中位数
6.94 秒；每次都只出现 `Compiling usher-ui-settings` 和 `Compiling usher`。
`SLINT_LIVE_PREVIEW=1 cargo run --features live-preview,ui-components` 首次暴露了
动态 handle 与 `ComponentFactory` 的类型冲突；独立 build script 屏蔽
`SLINT_LIVE_PREVIEW` 后，组合构建在 17.39 秒完成。修改和还原
`github_ui_components_gallery.slint` 各触发一次 `Reloaded component MainWindow`，
期间没有 Cargo/rustc。与既有结论一致，连续两次 live reload 将 RSS 推高至约
1,011 MiB，因此该模式只用于短时 UI 迭代。

P5 的 Settings 边界判定为**采用**：它把该页面热改反馈稳定压到 10 秒内，并建立了
可复用的真实跨 crate 页面协议。单独的根体积 30% 门槛尚未满足，下一批应优先审计
Workflow（8,455,237 字节）而不是 PR Review（11,587,636 字节）：Workflow 只有一类
小型 PR list model，而 PR Review 有多类大型 diff/conversation model，跨边界转换和
回归风险明显更高。Home、Search、About、Job Run、Project、Chat 六页独立根合计约
6.86 MB，考虑共享 import 后不足以单独补齐门槛，不作为下一批的优先组合。

#### Workflow 边界实验（已回滚）

后续实验曾让 `usher-ui-workflow` 独立生成 Workflow 页面，并通过
`src/workflow_page.rs` 桥接 MainWindow 状态。实验数据本身有效：主生成 Rust 从
34,969,310 字节、374,472 行降到 26,914,095 字节、286,604 行，分别减少 23.04%
与 23.46%；独立 Workflow 根为 8,561,867 字节、94,364 行；三次 Workflow 注释热改
为 6.84 / 6.14 / 5.90 秒，中位数 6.14 秒，且没有重编 `usher-ui`。

但是该实验引入了跨 crate global/viewport/滚动/浮层桥接复杂度，并在 Workflow
筛选菜单、仓库详情弹窗和 Tooltip 上产生多轮可见回归。编译和单元测试无法覆盖这些
交互错误，实验未通过人工 UI 验收。2026-08-04 已按用户要求完整回滚：删除独立
Workflow crate、Rust 桥接、临时模型和相关 UI 协议，Workflow 恢复到 Settings
间距调整完成时的主 `usher-ui` 内联实现。以上数字只保留为否决实验的历史对比，不代表
当前编译图，也不得据此继续拆分 Workflow 或 PR Review。详细问题与回滚范围见
[`workflow-optimization-rollback.md`](workflow-optimization-rollback.md)。

## 2026-08-15 当前基线与已否决的 PR 筛选菜单压缩

本轮继续使用同一台 10 核 Apple M2 Pro、32 GiB 内存机器，工具链为 Cargo 1.92.0、
rustc 1.92.0，源码为提交 `e9e9e4d`，默认只启用 FemtoVG。开始测量时不存在
`target/`。冷 `cargo build --locked --timings` 耗时 165 秒；其中 Slint build
script 为 9.21 秒，生成后的 `usher-ui` 单元为 100.85 秒，最终 binary 单元仅为
2.65 秒。当前冷构建主因仍是生成 UI 的 Rust 前端，不是链接器。

优化前默认主 UI 由 214 个 Slint/资源输入生成 40,566,802 字节、436,532 行 Rust；
产物包含 429,445,528 字节 rlib、85,605,053 字节 rmeta，以及冷构建后的 1.89 GiB
`usher-ui` 增量状态。应用自有 Slint 已增长到 28,290 行，高于 2026-08-03 的
23,426 行基线。

真实无语义内容修改的热构建基线如下。表中使用 Cargo 报告的构建时间，避免一次无关的
外部 Cargo 锁等待干扰编译器 A/B。

| 场景 | 样本 | 中位数 |
| --- | --- | ---: |
| 无源码修改 | 0.75 / 0.28 / 0.27 秒 | 0.28 秒 |
| `src/lib.rs` 注释修改 | 7.47 / 3.58 / 3.47 秒 | 3.58 秒 |
| `ui/main.slint` 注释修改 | 51.97 / 31.42 / 29.90 秒 | 31.42 秒 |

直接 Slint codegen 探针测得 PR Review 为 12,917,608 字节，Workflow 为
10,139,453 字节，PR 列表筛选/菜单依赖面为 3,715,355 字节。PR 筛选菜单原先为
Reviews、Assignee、Sort 和紧凑 More 菜单静态展开 19 个
`GitHubFilterMenuItem`。候选实现曾将其替换为三组固定 option model 和三个 repeater
delegate，同时保持标签、值、顺序、32px 行高、紧凑菜单分区位置、选中路由和现有
强类型 callback 不变。下表的生成体积属于该实验，不代表当前源码。

| 生成探针 | 修改前 | 修改后 | 变化 |
| --- | ---: | ---: | ---: |
| PR 筛选/列表组件 | 3,715,355 字节 | 1,530,462 字节 | -58.8% |
| `GitHubPullRequestFlow` | 5,207,308 字节 | 2,927,141 字节 | -43.8% |
| Workflow 页面 | 10,139,453 字节 | 7,854,494 字节 | -22.5% |
| 默认主 UI | 40,566,802 字节 | 38,274,867 字节 | -5.65% |
| 默认主 UI 行数 | 436,532 | 408,312 | -6.46% |

初筛先后运行了完整基线与完整候选：基线为 51.97 / 31.42 / 29.90 秒，候选为
43.84 / 27.59 / 42.84 / 27.06 / 26.96 秒。两组独立中位数看似从 31.42 秒降到
27.59 秒，即提升 12.2%。但该初筛不是受控 A/B：两侧样本数不同、所有基线都先于
所有候选运行，且共享 target 的增量状态持续变化。该结果只作为初步证据保留，以下面的
受控结果为准。

### PR 筛选菜单受控 A/B

2026-08-16 的复测将提交 `e9e9e4d` 导出到路径等长的隔离 A、B 源码目录。A 保持 19 个
静态菜单项，B 仅增加 repeater 候选。两侧分别使用独立 target、相同 locked 依赖、默认
FemtoVG feature、一次隔离冷构建和一次真实 Slint 修改预热。随后在
`ui/main.slint` 中反复切换同一条等长注释，按 `BA`、`AB` 交替执行 8 轮。所有正式
样本均保留，没有人工删点。

| 轮次 | 顺序 | A 静态实现 | B repeater | B - A |
| ---: | :---: | ---: | ---: | ---: |
| 1 | BA | 60.00 秒 | 55.54 秒 | -4.46 秒 |
| 2 | AB | 38.89 秒 | 39.08 秒 | +0.19 秒 |
| 3 | BA | 56.35 秒 | 50.67 秒 | -5.68 秒 |
| 4 | AB | 35.89 秒 | 55.53 秒 | +19.64 秒 |
| 5 | BA | 40.37 秒 | 52.99 秒 | +12.62 秒 |
| 6 | AB | 39.57 秒 | 55.37 秒 | +15.80 秒 |
| 7 | BA | 60.00 秒 | 36.03 秒 | -23.97 秒 |
| 8 | AB | 56.58 秒 | 56.54 秒 | -0.04 秒 |

两侧总耗时呈双峰分布，脱离配对轮次单看独立中位数会产生误导：A 为 48.36 秒，B 为
54.18 秒；逐轮 `B - A` 的中位数仅为 +0.075 秒。Cargo timing 的关键路径拆分更有
解释力：

| 热构建指标 | A 中位数 | B 中位数 | 独立变化 | 配对 B - A 中位数 |
| --- | ---: | ---: | ---: | ---: |
| 总耗时 | 48.360 秒 | 54.180 秒 | +12.03% | +0.075 秒 |
| Slint codegen | 7.390 秒 | 7.215 秒 | -2.37% | -0.255 秒 |
| 生成 `usher-ui` | 38.075 秒 | 44.020 秒 | +15.61% | -0.195 秒 |
| 应用 library | 2.085 秒 | 2.310 秒 | +10.79% | +0.225 秒 |
| binary | 2.415 秒 | 2.720 秒 | +12.63% | +0.105 秒 |

冷构建使用全新的独立 target，按 `AB`、`BA`、`AB` 执行三组。A 的总耗时为
154 / 164 / 178 秒，B 为 177 / 167 / 170 秒；独立中位数分别为 164 和 170 秒，
配对差为 +23 / +3 / -8 秒，中位数 +3 秒。target 体积中位数从 A 的 5.83 GiB
降到 B 的 5.65 GiB，仅减少 3.2%。

| 冷构建指标 | A 中位数 | B 中位数 | 独立变化 | 配对 B - A 中位数 |
| --- | ---: | ---: | ---: | ---: |
| 总耗时 | 164.00 秒 | 170.00 秒 | +3.66% | +3.00 秒 |
| Slint codegen | 10.40 秒 | 10.34 秒 | -0.58% | -0.38 秒 |
| 生成 `usher-ui` | 95.79 秒 | 103.10 秒 | +7.63% | +4.98 秒 |
| 应用 library | 9.34 秒 | 10.28 秒 | +10.06% | +0.94 秒 |
| binary | 3.05 秒 | 2.82 秒 | -7.54% | -0.15 秒 |

最终决定：否决 repeater 候选并恢复静态菜单。它没有让配对热构建或冷构建达到 10%
收益门槛，target 仅减少 3.2%，也未达到体积门槛。生成代码减少本身真实存在，但落到完整
根 UI 后不足以形成可靠构建收益。宽屏和紧凑 fixture 验收均通过，因此否决原因不是功能
正确性。重复测量产生的隔离冷 target 已在保存 timing 和体积后删除；这只是清理临时基准
产物，不是编译提速手段。

## 2026-08-16 依赖与 Rust 升级

本批把“依赖现代化”与“编译器性能”分开测量。直接依赖 A/B 两侧都使用 Rust
1.92.0：A 为原 manifest/lock；B 将 `webbrowser` 1.2.2 升到 1.2.4、`base64`
0.23.0 升到 0.23.1、`dark-light` 2.0.0 升到 3.0.0。Usher 只调用
`dark-light` 的同步平台 API，因此关闭其默认 feature，移除了旧 `async-std` 栈。

两侧源码、默认 FemtoVG feature 相同，分别使用全新独立 target，并执行
`cargo build --locked --timings`。这批依赖升级的保留理由是版本维护和平台依赖图
清理，不是构建提速：

| 指标 | A：原依赖 | B：升级后依赖 | 变化 |
| --- | ---: | ---: | ---: |
| 冷构建 wall time | 134.99 秒 | 136.93 秒 | 慢 1.4% |
| Cargo 报告耗时 | 134 秒 | 136 秒 | 慢 1.5% |
| `Cargo.lock` package 数 | 853 | 842 | -11 |
| 依赖树唯一行数 | 612 | 599 | -13 |
| 重复依赖树行数 | 354 | 363 | +9 |
| Target 体积 | 2,591,508 KiB | 2,591,684 KiB | 基本不变 |
| Incremental 体积 | 1,100,392 KiB | 1,100,520 KiB | 基本不变 |

macOS callback 随后从无人维护的 `block` 0.1.6 迁移到 `block2` 0.6.2。
`RcBlock` 保持 notification observer 与应用内 event monitor 所需的 Objective-C
复制 block 生命周期；AppKit callback 边界使用 ABI 等价的 `*mut c_void`，内部再转换
为 `objc::runtime::Object`。迁移后的隔离 locked check 为 3.19 秒，Rust 1.97 不再
报告 `block` 的 future-incompatibility warning；锁文件和唯一依赖树各再减少一项。

Rustup stable 从 rustc/Cargo 1.92.0 升到 1.97.1，LLVM 从 21.1.3 升到 22.1.6，
rustup 从 1.28.2 升到 1.29.0。下表 C 与 B 使用同一份升级后依赖图，只比较工具链：

| 指标 | B：Rust 1.92.0 | C：Rust 1.97.1 | 变化 |
| --- | ---: | ---: | ---: |
| 冷构建 wall time | 136.93 秒 | 130.07 秒 | 快 5.0% |
| Cargo 报告耗时 | 136 秒 | 130 秒 | 快 4.4% |
| 冷 `usher-ui` check 单元 | 72.8 秒 | 64.8 秒 | 快 11.0% |
| Target 体积 | 2,591,012 KiB | 2,780,852 KiB | 增加 7.3% |
| Incremental 体积 | 1,099,864 KiB | 1,164,396 KiB | 增加 5.9% |

Rust 1.97 的三次 Slint 热改样本为 29.93 / 26.73 / 48.17 秒，中位数 29.93 秒；
Rust 1.92 的可比中位数为 31.42 秒，改善约 4.7%。保留新版工具链是因为它更新、消除
未来兼容 warning，且主导冷路径的 `usher-ui` 单元达到 11.0% 改善；不能把它表述为
整个 workspace 达到 10% 提速，产物增大也是实测代价。

工具链升级后，`cargo update` 又刷新了 68 个 semver 兼容的间接依赖，包括 Rustls、
ICU、Futures、Ureq、Wasm bindings 和平台支撑包。联网 dry run 随后显示 Rust
1.97.1 下还可选择的更新为 0。当前锁文件含 841 个 package、依赖树 598 个唯一行和
363 个重复行。`generic-array` 0.14.9 与旧 `toml` 0.8 patch 仍受间接消费者版本范围
限制；应用直接依赖的 `toml` 已是 1.1。

以下候选明确不保留：

- Parley 0.11 要求 Fontique 0.11，而 Slint 1.17 通过 Fontique 0.10 暴露共享自定义
  字体集合。另建字体集合会破坏 macOS 注册字体与 Slint 的一致性，因此保留 Parley
  0.10。
- 收窄应用 `image` feature 不能保证行为不变：项目 Logo 与下载头像当前接受 `image`
  已启用的任意 decoder，Preview spec 则明确承诺 PNG/JPEG/GIF/WebP。Slint macro
  compiler 同时会启用完整图片格式，只收窄应用依赖会新增 feature identity，却不能
  移除已有完整格式编译单元。
- Slint debug metadata 默认已经关闭；显式开启后，生成量从 40,566,802 字节、
  436,532 行增至 41,094,241 字节、446,045 行。`SLINT_EMBED_RESOURCES=false`
  的输出仍为 40,568,133 字节，几乎不变，还引入源码绝对路径运行时依赖。
- content-aware build output 实验只在字节变化时改写生成 `main.rs`，但 Slint 注释修改
  后 Cargo 仍重查 `usher-ui`。该样本总计 38.14 秒，其中 `usher-ui` 29.2 秒、build
  script 7.6 秒，实验已完整回滚。

每批统一记录工具链、硬件、commit、feature/profile、无改动/Rust/Slint 三类样本、
Cargo timing、生成文件体积、target/incremental 体积。合并前至少执行三次热样本，
冷构建使用独立 target，并完成 `cargo fmt --all`、`cargo check --locked`、
`cargo test --lib --locked` 与 `git diff --check`。

## 2026-08-16 显式 PR Review 开发 Shell

本批没有再次采用已否决的 Workflow/PR Review 跨 crate 拆分，而是把 PR Review
详情页完整保留为一个 feature package。稳定的 `@pr-review` Slint library 入口暴露
统一页面协议：默认和 Release 构建映射到完整实现；显式、非默认的 `dev-shell`
feature 把整个包映射到兼容的空实现。Rust 只在该开发模式把直接进入
`PrReview` 的导航回退到 Workflow。未来 Project、通知或搜索入口可消费同一个包，
无需让 Workflow 路由成为页面所有者。

默认生成量相对上一轮 40,566,802 字节 / 436,532 行基线仅增加 49 字节 / 3 行，
可视为不变。显式开发 Shell 从编译图中移除了完整 PR Review 渲染树：

| 根生成指标 | 完整/默认 | `dev-shell` | 变化 |
| --- | ---: | ---: | ---: |
| Rust 字节 | 40,566,851 | 27,941,262 | -12,625,589（-31.1%） |
| Rust 行数 | 436,535 | 304,945 | -131,590（-30.1%） |
| Item tree | 984 | 702 | -282（-28.7%） |
| 跟踪依赖 | 214 | 192 | -22（-10.3%） |
| 直接 Slint 生成 | 6,314 ms | 4,356 ms | -1,958 ms（-31.0%） |

受控构建使用同一台 10 核 M2 Pro / 32 GiB、rustc/Cargo 1.97.1、默认
FemtoVG、提交 `e9e9e4d` 加本批工作区候选，并分别使用
`target/dev-shell-ab/full` 与 `target/dev-shell-ab/dev-shell`。冷构建只执行一组
完整模式在前、Shell 在后的 `cargo build --locked --timings`；系统缓存可能偏向
第二次，因此这组冷数据只作为辅助证据，不冒充多组独立中位数。主导的
`usher-ui` 单元和交替热样本给出了同方向结果。

| 冷构建指标 | 完整/默认 | `dev-shell` | 变化 |
| --- | ---: | ---: | ---: |
| Wall time | 177.38 秒 | 128.56 秒 | -48.82 秒（-27.5%） |
| Cargo 报告总耗时 | 177 秒 | 128 秒 | -49 秒（-27.7%） |
| 生成 `usher-ui` 单元 | 106.5 秒 | 66.9 秒 | -39.6 秒（-37.2%） |
| `usher-ui` build script | 11.8 秒 | 9.3 秒 | -2.5 秒（-21.2%） |
| Target 体积 | 7,336,264 KiB | 6,208,248 KiB | -15.4% |
| Incremental 体积 | 3,033,040 KiB | 2,287,568 KiB | -24.6% |

两侧各完成一次真实修改预热后，对 `ui/main.slint` 进行三次等长无语义注释修改，
按 `BA`、`AB`、`BA` 交替执行，未删除任何正式样本：

| 轮次 | 顺序 | 完整/默认 | `dev-shell` | Shell - 完整 |
| ---: | :---: | ---: | ---: | ---: |
| 1 | BA | 41.28 秒 | 34.44 秒 | -6.84 秒 |
| 2 | AB | 40.57 秒 | 33.84 秒 | -6.73 秒 |
| 3 | BA | 41.16 秒 | 23.82 秒 | -17.34 秒 |
| 中位数 | | 41.16 秒 | 33.84 秒 | 配对 -6.84 秒 |

两侧独立热中位数改善 17.8%，配对差中位数相对完整模式中位数改善 16.6%。完整
模式的 `usher-ui` / build script / 应用单元中位数为 30.1 / 7.7 / 1.8 秒，
Shell 为 25.7 / 5.8 / 2.1 秒。本批达到 10% 接受门槛及 15% 热改目标，因此保留；
但 `33.84` 秒热中位数尚未达到低于 25 秒的延伸目标，只有最快样本达到。

只修改并恢复 `ui/views/workflow/pr_review_workspace.slint` 后，预热的
`dev-shell` 构建在 0.37 秒内完成，且没有出现 `Compiling usher-ui`，证明完整页面
不在该模式依赖图中。`scripts/dev-shell-check.sh`、`scripts/dev-shell-run.sh` 及
PowerShell 对应入口把该 Cargo identity 固定在 `target/dev-shell`。普通和 Release
命令不会启用此 feature；最终交付仍需执行完整默认检查并手工验证 PR Review。
