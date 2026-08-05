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
invoke Cargo with `--release`.

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

每批统一记录工具链、硬件、commit、feature/profile、无改动/Rust/Slint 三类样本、
Cargo timing、生成文件体积、target/incremental 体积。合并前至少执行三次热样本，
冷构建使用独立 target，并完成 `cargo fmt --all`、`cargo check --locked`、
`cargo test --lib --locked` 与 `git diff --check`。
