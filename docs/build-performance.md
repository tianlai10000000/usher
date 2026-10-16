# Build Performance and Artifact Policy / 构建性能与产物策略

This document keeps the current build profiles, supported development commands,
cleanup policy and measurement contract. Dated measurements and earlier roadmaps
are preserved in [the build-performance archive](archive/build-performance.md).
They describe their recorded source/toolchain and are not the current baseline.

本文保留当前构建 profile、支持的开发命令、清理策略与测量合同。带日期的测量和旧路线
完整保存在[构建性能历史档案](archive/build-performance.md)；历史结果仅代表记录中的
源码与工具链，不作为当前基线。

Run the example commands from the repository root. / 示例命令均从仓库根目录执行。

## Policy

The workspace uses compact profiles for everyday development:

```toml
[profile.dev]
debug = "line-tables-only"
split-debuginfo = "off"
incremental = true

[profile.dev.package."*"]
opt-level = 1

[profile.dev.package.usher-ui]
debug = "none"
opt-level = 1

[profile.dev.package.usher-ui-components]
debug = "none"
opt-level = 1

[profile.dev.package.usher-ui-settings]
debug = "none"
opt-level = 0

[profile.dev.package.usher-messages]
debug = "none"
opt-level = 1

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
application backtraces. Third-party dependencies and generated MainWindow,
playground and Messages UI crates use opt-level 1. Independently generated
Settings uses dev opt0 to reduce LLVM optimization/codegen work; its codegen-unit
count remains at the default. Application-owned Rust remains opt0 with line
tables. Test settings follow Cargo's dev inheritance and explicit profile/package
overrides; this change adds no test or Release profile fields. The profile
intentionally does not preserve full local-variable and type information for
LLDB. Use the isolated profile when that information is needed:

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
artifact. `cargo bundle --profile release-fast --format osx` provides the matching local
application bundle when needed.

## Supported development commands / 支持的开发命令

Normal development and integration use the complete default application:

```sh
cargo check --locked
cargo build --locked
cargo run --locked
```

For repeated `.slint` edits to the main shell or UI Components playground,
start live preview once and leave that process running while saving files:

```sh
./scripts/ui-live-preview.sh
```

PowerShell uses `scripts/ui-live-preview.ps1`. The script enables both
`live-preview` and `ui-components`. Its first launch still compiles the application
and preview dependencies; supported subsequent UI edits reload in that process.
Restarting `cargo run --features ui-components` after every save repeats static
Slint generation, Rust compilation and linking. Rust changes require a rebuild.
Settings and Messages remain independently compiled static factory products;
edits to those pages require a rebuild even during a main-shell preview session.

连续修改主壳或 UI Components 演练场的 `.slint` 时，优先启动一次上述实时预览，
保持进程运行，保存文件后刷新；PowerShell 使用对应 `.ps1`。首次启动仍需编译应用
和预览依赖。每次保存后都重启 `cargo run --features ui-components`，会重复执行
静态 Slint 生成、Rust 编译和链接。Rust 改动仍需重建；Settings、Messages 是独立
编译的静态 factory 页面，修改后也需重建，不能依赖主壳热重载。

For a complete root UI check without starting the application, install the
matching viewer and use the checked-in script:

```sh
cargo install slint-viewer --version 1.18.1 --locked
./scripts/ui-check.sh
```

PowerShell uses `scripts/ui-check.ps1`. Live preview interprets edits at runtime;
the final complete static playground comparison uses:

```sh
cargo run --locked --features ui-components
```

Also run normal `cargo run --locked` for the affected product routes. Default
and release builds do not enable live preview or the playground. Repeated reloads
can retain diagnostic state, so periodically restart long preview sessions.

`ui-check` 不启动应用，只校验完整根 UI；PowerShell 使用对应 `.ps1`。实时预览在
运行时解释改动，交付前仍须通过 `cargo run --locked --features ui-components`
对照完整静态演练场，并使用普通 `cargo run --locked` 验证受影响的产品路由。默认
与 Release 构建不启用实时预览或演练场。重复热重载可能保留诊断状态，长时间预览
期间应定期重启进程。

When the change does not involve PR Review, the explicit development shell uses
an isolated Cargo target and a compatible empty implementation of the complete
PR Review feature package:

```sh
./scripts/dev-shell-check.sh
./scripts/dev-shell-run.sh
./scripts/dev-shell-run.sh --features ui-components
```

PowerShell equivalents are `scripts/dev-shell-check.ps1` and
`scripts/dev-shell-run.ps1`; append `--features ui-components` to the run script
when the playground is needed. These scripts keep artifacts under
`target/dev-shell`, so that target's first cold build still compiles its
dependencies. This mode redirects PR Review navigation to Workflow and cannot
validate PR Review. It remains a static build unless live preview is explicitly
enabled. Normal and release commands compile the complete page; final delivery
requires the default checks and manual validation of affected routes.

改动不涉及 PR Review 时，可使用显式开发 Shell；对应 PowerShell 入口为
`dev-shell-check.ps1` 与 `dev-shell-run.ps1`；run 脚本追加
`--features ui-components` 可启用演练场。脚本把产物隔离在 `target/dev-shell`，
该 target 首次冷构建仍需编译依赖。PR Review 导航会回退到 Workflow，此模式不能
用于 PR Review 验收，本身也不会自动启用实时预览。普通与 Release 命令仍编译完整
页面；最终交付仍须执行默认检查与受影响路由的手工验收。

The [2026-10-15 investigation（归档：docs/validation/ui-components-build-20261015/README.md）](local-archives.md#validation-experiments-20261016)
separates cached launches from static Slint rebuilds and records a rejected `usher-ui` opt0
profile experiment. Use its dated measurements as diagnostic evidence, not a
promise about cold builds or application startup time.

[2026-10-15 专项调查（归档：docs/validation/ui-components-build-20261015/README.md）](local-archives.md#validation-experiments-20261016) 区分缓存命中与
静态 Slint 重建，并记录已否决的 `usher-ui` opt0 配置。数据只代表当次环境，不承诺冷构建或
应用实际开窗耗时。

The [actual startup trace（归档：docs/validation/startup-timing-20261015/README.md）](local-archives.md#validation-experiments-20261016) records
one current-worktree `cargo run --features ui-components` with Cargo timing
output: build units, application initialization, render callbacks and the reveal
request. Its existing-cache/user-state observation is not a cold build or A/B,
and neither AfterRendering nor the reveal log proves screen presentation.

[实际启动计时（归档：docs/validation/startup-timing-20261015/README.md）](local-archives.md#validation-experiments-20261016) 保存当前工作区一次真实
运行的编译单元、应用初始化、渲染回调与 reveal 请求时间。它使用现有缓存和正常
用户状态，不是冷构建或 A/B；渲染完成回调、reveal 日志均不等于实测屏幕呈现。

[Settings 优化后复测（归档：docs/validation/startup-timing-20261016-after-settings/README.md）](local-archives.md#validation-experiments-20261016)
记录后续一次真实运行：Cargo 23.026 s、reveal 日志 26.965 s，四个 UI library 均
fresh；根 library/bin 为新缓存身份补编。紧随其后的无修改 build 为 0.728 s 且
597 个产物全 fresh。`Running` 后到 Timeline 起点仍约 3.28 s，需后续拆分定位。
这两次实际启动的缓存/源码状态不同，不能把总耗时差直接当作 Settings 优化 A/B 收益。

The [post-Settings startup observation（归档：docs/validation/startup-timing-20261016-after-settings/README.md）](local-archives.md#validation-experiments-20261016)
records Cargo at 23.026 s and the reveal log at 26.965 s. All four UI libraries
were fresh; root library/binary artifacts needed their new cache identities.
A separate subsequent no-change build took 0.728 s with all 597 artifacts fresh.
About 3.28 s remains between `Running` and the internal Timeline origin. Different
source/cache states prevent treating the startup-total difference as an A/B gain.

The [later actual rerun（归档：docs/validation/startup-timing-20261016-100256/README.md）](local-archives.md#validation-experiments-20261016)
failed at linking after 218.58 s when the MainWindow UI artifact was unavailable.
After clearing only the `usher-ui` dev package cache, the
[recovery run（归档：docs/validation/startup-timing-20261016-recovered-cache/README.md）](local-archives.md#validation-experiments-20261016)
built successfully in 213.73 s and requested window reveal at 217.67 s.
The UI library took 207.92 s (67.14 s frontend, 140.78 s codegen); Settings and
the other UI libraries remained fresh. Local Settings improvements do not remove
this MainWindow rebuild bottleneck. These different-cache observations are not A/B.

[后续实际复跑（归档：docs/validation/startup-timing-20261016-100256/README.md）](local-archives.md#validation-experiments-20261016) 在 MainWindow UI
缓存产物不可用后触发重建，218.58 s 时链接失败。仅清理 `usher-ui` 包的 dev 缓存后，
[恢复运行（归档：docs/validation/startup-timing-20261016-recovered-cache/README.md）](local-archives.md#validation-experiments-20261016) 构建成功，
Cargo 用时 213.73 s，到请求显示窗口共 217.67 s。UI 库耗时 207.92 s，其中
frontend 67.14 s、codegen 140.78 s；Settings 和其余 UI 库保持 fresh。
Settings 的局部优化尚未消除 MainWindow 重编瓶颈；这些缓存条件不同的观察不是 A/B。

### MainWindow cache triggers and codegen / MainWindow 缓存触发与 codegen

The [2026-10-16 follow-up（归档：docs/validation/mainwindow-codegen-20261016/README.md）](local-archives.md#validation-experiments-20261016)
separates an unavailable historical `.rlib` output from a reproduced generated-output
watch. Cargo's earlier `FailedToReadMetadata` concerned filesystem metadata/mtime,
not decoding Rust crate metadata; the missing-output and linker failure causes
remain unproven. A later no-intentional-change probe directly traced an extra
rebuild to the generated translation directory being newer than Cargo's build
output reference.

Static UI builds now use application-owned `crates/usher-ui-build/` with Slint's
public output-path API. It retains the four callers' source/resource and environment
inputs, translation domain, generated include path and named-library metadata,
while excluding the current `OUT_DIR`'s ordinary generated outputs. Original
catalog/locale inputs remain watched by `usher-i18n-build`. The wrapper supports
the application's absolute configuration paths, generated bundles and default
Rust module; external bundles require caller-owned directory watches. It does
not touch or backdate filesystem timestamps. Final real Cargo/Slint fixture
comparisons pass 16 root and 25 library assertions: initial generation and catalog
updates no longer cause the following unchanged build to recompile, while real
source/catalog/locale/environment changes still do.

A separate frozen-source, three-pair semantic-edit comparison rejected MainWindow
dev CGU64: median build time was **168.607055 → 172.578309 s (+2.3553%)**.
Its remaining controls and native acceptance were stopped after the primary
criterion failed; the full summary remains explicitly incomplete. The default
CGU, opt1, incremental compilation, debug assertions and overflow checks stay in
place. CGU1's independent screening was invalidated when sampled external
Cargo/rustc activity overlapped its baseline, before the candidate result was
known. Its raw observations are exploratory, establish no controlled speed
comparison and do not support adoption; no full CGU1 matrix or native candidate
acceptance was completed. The watch fix proves invalidation
correctness, not faster LLVM codegen for real Slint edits or Release/runtime gains.

[本轮核对与原始证据（归档：docs/validation/mainwindow-codegen-20261016/README.md）](local-archives.md#validation-experiments-20261016) 区分先前不可用的
`.rlib` 与新复现的输出自监听：历史 `FailedToReadMetadata` 是文件系统 metadata/mtime
读取失败，其输出缺失及链接失败根因尚未证实；本轮无主动修改探测则直接捕获生成翻译
目录晚于 Cargo build output 参考时间，导致下一次构建额外重编。

四个静态 UI build script 现通过 `crates/usher-ui-build/` 使用公开 Slint API，排除
当前 `OUT_DIR` 普通生成物自监听，保留真实源/资源、原始 catalog/locale 与 8 个 Slint
环境触发器，以及当前入口所需 domain/include/library metadata；不 touch 或回拨时间。
最终真实 Cargo/Slint 对照 16 + 25 项断言通过，首次生成和 catalog 更新后的下一次
无修改构建保持 fresh；合法源、翻译、locale 和环境改动仍触发构建。
独立 CGU64 真实语义编辑三对中位耗时反而慢 2.3553%，已拒绝，剩余控制与原生验收
未执行，不能写成完整门禁通过。当前 MainWindow 参数保留；CGU1 单对筛选因外部
Cargo/rustc 与基线重叠而在候选出结果前判无效，原始观测仅作探索，不计算受控变慢
比例、不采用候选，完整矩阵与候选原生接受未执行。
监听修复的行为证据不能证明真实 Slint 语义修改的 codegen 或生产构建已提速。

Final real-workspace checks passed all 13 regression steps and an offline native
macOS smoke in English and zh-CN. After excluded build-entry initialization,
three unchanged builds took 0.780735 / 0.369593 / 0.358589 s: all four UI libraries
and root library/binary were fresh and no UI build script ran. These are cache
observations, not window-startup timings or a controlled speedup comparison.

最终真实工作区 13 项回归及 macOS 中英文离线 smoke 通过。排除新入口初始化后，
三次无修改构建为 0.780735 / 0.369593 / 0.358589 秒，四个 UI 库与根 library/bin
全 fresh，UI build script 均未执行。这是缓存行为观察，不是开窗时间或受控提速比例。

The [stack/project analysis（归档：docs/validation/mainwindow-codegen-20261016/stack-vs-project.md）](local-archives.md#validation-experiments-20261016)
locates most measured time after Slint generation: baseline build-script median
2.55 s versus MainWindow rustc 163.24 s, including 140.84 s codegen. Native
generation creates compiler work; this application's large remaining compilation
graph amplifies it. Generated text ownership is not a CPU profile, and no minimal
same-stack or equivalent cross-framework benchmark establishes an attribution
percentage or a framework-switch benefit.

[技术栈/项目归因核对（归档：docs/validation/mainwindow-codegen-20261016/stack-vs-project.md）](local-archives.md#validation-experiments-20261016)
记录 Slint 生成后处理大型 Rust 的主要成本，以及 PR Review、Workflow 等仍在同一
编译根的范围。未执行最小同栈/同功能跨框架对照，不能量化框架与架构各占多少，
也不能把生成文本体积等同于 CPU 热点或换栈收益。

### Settings generated Rust / Settings 生成 Rust

The adopted Settings dev opt0 override keeps the independently generated UI and its
default codegen-unit count, reducing LLVM optimization work on that generated
Rust. Changing the setting creates a new compiler-artifact identity: allow its
initial warmup without cleaning the target. The
[Settings A/B record（归档：docs/validation/settings-codegen-20261016/README.md）](local-archives.md#validation-experiments-20261016) and
[raw summary（归档：docs/validation/settings-codegen-20261016/o0-summary.json）](local-archives.md#validation-experiments-20261016) compare opt1
and opt0 on the same frozen source and target with `ui-components`, using three
paired warm dev samples per group:

| Trigger | opt1 median | opt0 median | Observed change |
| --- | ---: | ---: | ---: |
| Settings page-width semantic edit | 62.287524 s | 15.774179 s | -74.7% wall time |
| Settings Slint comment edit | 9.796214 s | 10.145641 s | +0.35 s |
| Binary-only Rust comment edit | 1.365427 s | 1.456279 s | +0.09 s |

The CGU16 candidate was rejected separately. These measurements concern warm
incremental compilation; they establish no cold-build, window-startup, runtime
or target-size gain. Automatic A/B gates, full regressions and the scoped macOS
native smoke passed. Adoption accepts the small comment/bin control regressions
above and a 42.605 MiB (+19.65%) larger dev executable on disk; this is not an RSS
measurement. The linked validation record retains the final decision and limits.

已采用的 Settings dev opt0 覆盖保留独立 UI 生成边界与默认 codegen-unit 数量，通过减少生成
Rust 的 LLVM 优化工作缩短对应重建。首次切换需要预热新产物，无需 clean。上表是同源、
同 target、启用 `ui-components` 的 dev 暖增量对照，每组各三对样本；语义修改、注释
修改与 bin-only 控制的结果分别列出。CGU16 已另行否决。当前数值证据不代表冷构建、
实际开窗、运行时或体积收益。自动 A/B 门禁、完整回归和限定范围的 macOS 原生冒烟均
通过；采用时接受上表注释/bin 控制的小幅回退，以及 dev 可执行文件磁盘体积增加
42.605 MiB（+19.65%）的代价，此体积不是 RSS。最终结论和限制见链接记录。

### Catalog build boundary / Catalog 构建边界

Static UI build scripts use the catalog-free `usher-i18n-build` helper. They
read `manifest.toml`, the Slint-visible `messages.toml`, and each target locale's
`usher.po`. Rust-only copy lives in `rust-messages.toml` and `usher-rust.po`;
only the runtime catalog consumes both scopes. A Rust-only copy edit therefore
does not invalidate MainWindow, Settings, Messages or the enabled static
playground. Shared locale metadata,
Slint-visible copy/translations and helper implementation changes still rebuild
the affected UI graph. No manual generated-catalog synchronization is required.

静态 UI 构建脚本使用不内嵌 catalog 的 `usher-i18n-build`，只读取共享 manifest、
Slint 可见的 `messages.toml` 和各目标语言的 `usher.po`。Rust 专用文案归
`rust-messages.toml` 与 `usher-rust.po`，运行时合并两组查询。因此仅修改 Rust
专用文案不会使 MainWindow、Settings、Messages 或已启用的静态演练场失效；共享语言元数据、Slint
可见文案/译文及 helper 实现改动仍会重建相应 UI 图，无需手工同步生成 catalog。

The [isolation A/B record（归档：docs/validation/i18n-build-isolation-20261015/README.md）](local-archives.md#validation-experiments-20261016)
measures valid Rust-only catalog additions and records Cargo freshness separately
from generated-file equality. It concerns incremental compilation, not cold
builds, native window startup or runtime performance.

[隔离 A/B 记录（归档：docs/validation/i18n-build-isolation-20261015/README.md）](local-archives.md#validation-experiments-20261016) 使用合法的 Rust
专用 catalog 新增项，同时记录 Cargo freshness 与生成文件一致性；范围是增量
编译，不代表冷构建、原生开窗或运行时收益。

### Playground build boundary / 演练场构建边界

Ordinary `cargo run --locked --features ui-components` builds the showcase in
the optional `usher-ui-components` package. MainWindow uses a lightweight host
contract. A showcase-only semantic edit must preserve `usher-ui` freshness and
generated/library hashes, and its watched Slint inputs must exclude the entire
`ui/components/showcase/` tree. Shared widget edits still invalidate all importing
roots. This reduces the affected compilation unit; Rust compilation and final
application linking may still run.

The `live-preview,ui-components` combination keeps the gallery in MainWindow's
interpreter graph and skips the unused static gallery product. Use the existing
live-preview scripts for that path. Setting `SLINT_LIVE_PREVIEW` alone does not
select the integrated gallery. Static factory lifecycle, feedback forwarding and
native acceptance are defined in the [playground spec](specs/ui-components.md).
The static product shares the host's actual `WindowAdapter`; its lazy constructor
immediately restores MainWindow's root before application synchronization/binding.
Adapter/root identity checks and native model/route-cycle painting are required
alongside build timings.
Incremental build measurements do not establish cold-build or window-startup
gains.

普通静态命令将演练场放在可选的 `usher-ui-components` 包，MainWindow 只使用轻量
host 契约。仅修改演练场语义时，必须同时确认 `usher-ui` 为 fresh、生成源码与库产物
hash 不变，且其 Slint 监听输入不包含 `ui/components/showcase/`。共享控件仍使所有
使用方失效；应用 Rust 编译和最终链接仍可能执行。
`live-preview,ui-components` 组合保留集成解释器图，可选静态包跳过演练场生成；
只设置环境变量不会选择此路径。工厂生命周期、反馈桥接和原生验收见
[演练场 spec](specs/ui-components.md)。增量编译对照不能证明冷构建或实际开窗收益。
静态产品共用实际宿主 adapter，惰性创建后立即恢复 MainWindow 根，再同步和绑定。
构建计时之外还须验证 adapter/root 身份与原生 model 更新、路由循环后的绘制。

## Production CI cache and timings / 生产 CI 缓存与计时

Production build commands use `--release --locked --timings`, default product
features, full LTO, one codegen unit and `CARGO_INCREMENTAL=0`. Optimization
parameters, toolchain, target matrix, bundle commands and automatic triggers are
unchanged. macOS/Windows restore Cargo downloads and unsigned compiler artifacts;
build caches cover host `target/release/`, the selected target's Release directory
and `.rustc_info.json`. They are saved after successful compilation and before
packaging/signing. Credentials, signing files and development profiles are outside
these paths; the existing macOS Cargo-tool binary cache remains available.

Build-cache lookup uses three ordered levels:

1. The primary key contains OS, host architecture, target, installed Rust cache
   key, `release-default`, manifest/lock/config/workflow hashes and commit SHA.
2. The first fallback removes only commit SHA, preferring the same configuration.
3. The second fallback keeps OS, host architecture, target, Rust and
   `release-default`, permitting reuse across manifest, lock, config and workflow
   changes. It can also restore after actual Release profile settings change.

The coarse fallback is a namespace boundary, not proof of complete configuration
compatibility. Cargo always runs with `--locked` and checks its tracked profiles,
features, inputs and fingerprints; a restored cache need not make every unit
fresh. Untracked SDK/native-toolchain changes require a deliberate cache-epoch
bump in primary and fallback keys or an empty cache. Fresh checkout mtimes can
also invalidate application crates. Transfer/save costs and retained cache size
must be measured before concluding that restoration saves wall time.

GitHub caches are immutable and ref-scoped: sibling release tags cannot read
each other's caches, but can read default-branch caches. After merging the workflow
to the default branch, choose that branch in **Release → Run workflow** and set
the boolean `cache_only` to `true` (default: `false`). Before checkout, both build
jobs compare the complete `GITHUB_REF` against `refs/heads/<default branch>` using
case-sensitive, quoted Bash operands. Other branches, case variants and tags fail
explicitly. This mode builds the same three targets and saves caches without
installing packaging tools, packaging, signing, product uploads or publication.
It does not prewarm cargo-bundle/create-dmg installation. With `cache_only=false`,
normal tag publication and ordinary branch dispatch packaging retain their flow;
a branch dispatch still does not create a GitHub Release. No automatic prewarm
push or schedule trigger is added. See [GitHub's cache scope rules](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching).

Diagnostic Python setup, cache metadata and diagnostic artifact upload are
best-effort in production jobs. The separate `Release timing report` job is also
non-blocking, with read-only Actions access. Diagnostic failures remain visible;
failed collection does not fabricate a successful summary. Build, packaging,
enabled signing/notarization, product upload and publication remain blocking.
Per-target `release-diagnostics-<target>-attempt-<N>` artifacts contain allowlisted
cache metadata and Cargo HTML; the reporter writes current-attempt job/step
start/end/status/duration data to the summary and
`release-stage-timings-attempt-<N>`. Missing/zero dates stay unknown, parallel job
durations cannot be summed as workflow wall time, and the report excludes itself.
It exports no full environment, commands, logs or credentials. Product downloads
exclude diagnostic artifacts.

生产 build 使用 `--release --locked --timings`、默认产品 feature、完整 LTO、单 CGU
与关闭增量编译，保持优化参数、工具链、target 矩阵、bundle 命令及自动触发器。
macOS/Windows 恢复下载和未签名编译缓存，编译成功后、打包/签名前保存；缓存路径
包含 host/当前 target 的 Release 目录及 `.rustc_info.json`，不包含凭据、签名文件或
开发 profile。保留现有 macOS Cargo 工具二进制缓存。

恢复顺序为含配置 hash 与 commit SHA 的主 key、只去 SHA 的严格 fallback、再到只
保留 OS/宿主架构/target/Rust/`release-default` 的粗层 fallback。粗层允许跨
manifest、lock、config、workflow 甚至实际 Release profile 设置变化恢复；它不是
完整配置兼容证明。Cargo 始终以 `--locked` 校验已跟踪的 profile、feature、输入和
fingerprint，命中不等于全部单元 fresh。未被跟踪的 SDK/native 工具链变化需同步
提升主 key 与 fallback 的缓存 epoch，或使用空缓存。新 checkout 的 mtime 也可能
使应用包重编；传输/保存及缓存体积成本仍需实测。

不同 tag 不能互读缓存。预热时在默认分支手动运行 Release，并设置 `cache_only=true`
（默认 `false`）；checkout 前以 Bash 引用数据、区分大小写比较完整默认分支 ref，
非默认分支、大小写变体和 tag 明确失败。预热复用相同三个 target 的构建/保存路径，
跳过打包工具安装、打包、签名、产品上传与发布，因此不预热 cargo-bundle/create-dmg。
默认模式保留正常 tag 发布与普通分支手动打包行为，不增加自动 push/schedule 预热。
诊断 Python、缓存元数据、诊断上传和只读报告 job 尽力执行；错误仍可见，构建、
打包、已启用签名/公证、产品上传和发布仍阻断。诊断 artifact 与产品下载分开，
缺失时间保持未知，并行 job 耗时不能相加作为总耗时。

The diagnostic-blocking and reuse gaps in the earlier
[static review（归档：docs/validation/release-ci-instrumentation-20261016/static-review.md）](local-archives.md#validation-experiments-20261016)
are corrected and covered by local positive/negative policy fixtures. The
[Release CI contract](specs/release-ci.md) and
[follow-up validation（归档：docs/validation/release-ci-reliability-20261016/README.md）](local-archives.md#validation-experiments-20261016)
record the current behavior and limitations. No hosted run or cold/warm A/B was
executed; cache matching and local dev timings establish no CI speedup.

旧[静态审查（归档：docs/validation/release-ci-instrumentation-20261016/static-review.md）](local-archives.md#validation-experiments-20261016)中的诊断阻断
和复用缺口已修复并完成本地正反例，当前契约见 [Release CI](specs/release-ci.md)，
证据见[后续验证（归档：docs/validation/release-ci-reliability-20261016/README.md）](local-archives.md#validation-experiments-20261016)。云端构建及冷/暖
A/B 仍未执行，不能从缓存匹配或本机 dev 耗时推断 CI 提速。

### Incremental compilation / 增量编译的取舍

Disabling incremental compilation removes rustc's within-crate incremental reuse;
edits to a crate can therefore require more compiler work. It does not disable
Cargo's reuse of complete, unchanged dependency artifacts. It avoids retaining
incremental-state directories and their write/transfer costs. Runtime execution
does not depend on those build caches. The existing production policy stays
disabled; everyday dev keeps it enabled. No runtime, size or production-build
gain is inferred from toggling this setting without a controlled experiment.
See the [Cargo reference](https://doc.rust-lang.org/cargo/reference/profiles.html#incremental).

关闭增量编译的主要负面影响是修改源码后的重编更慢，失去的是 rustc 在同一 crate
内部复用中间结果的能力；不是每次都把全部依赖重编。未变化的完整依赖产物仍可通过
Cargo/Actions 缓存复用。关闭后无需保存和传输增量中间状态，运行时也不依赖它。
本批保持生产策略关闭，日常 dev 继续开启，未擅自改变 Release 的优化取舍。

## Measurement protocol for every batch

1. Record `rustc -Vv`, `cargo -Vv`, hardware, source commit, features, profile,
   and target-directory state.
2. Use `cargo build --locked --timings` and save the generated timing report.
3. Measure a no-change build, a real harmless Rust comment edit, and a real
   harmless Slint comment edit. Derive each variant from the same original bytes,
   then restore and rebuild the original source after each edited sample or
   consecutive variant group. Use the same sequence in both A/B arms and exclude
   restore builds from acceptance samples.
   For a compilation-unit boundary batch only, a predefined harmless Slint
   semantic fixture may replace the Slint comment group when it changes generated
   code in the isolated owner. Record that substitution and the unmeasured comment
   path explicitly; semantic-edit timings do not establish comment-edit timings.
4. Use at least three warm samples for acceptance decisions. Use separate fresh
   target directories for cold A/B results.
5. Record wall time, `usher-ui(build)`, `usher-ui`, `usher`, final link time,
   generated `main.rs` bytes/lines, target size, and incremental size.
6. Run `cargo fmt --all`, `cargo check --locked`, `cargo test --lib --locked`,
   and `git diff --check` before handoff.

For catalog-boundary changes, also measure valid registry-plus-PO edits in the
isolated scope and verify a real Slint translation edit still rebuilds every
root's complete bundle. Record build-script and library `fresh` flags: unchanged
generated bytes alone do not prove Cargo skipped UI compilation.

For a compilation-boundary change, use a harmless semantic edit in the isolated
scope, plus a shared-component positive control that must still invalidate every
importing root. Record generated/library hashes and watched inputs alongside
Cargo freshness. Comment-only timing does not prove that a real layout edit
avoids the unrelated compilation unit.

The repository handoff additionally requires `cargo test --doc --locked`;
see [AGENTS.md](../AGENTS.md). Preserve raw evidence, failures, unexecuted checks
and the adopt/reject decision. A future candidate needs a new controlled baseline
with the same source, locked graph, fixture and environment; archived figures do
not substitute for that comparison.

For build/profile candidates, retain the existing acceptance rule: improve the
relevant median build time by at least 10%, or reduce steady target size by at
least 10% without slowing the relevant median by more than 5%. A structural,
renderer or UI boundary change also needs its relevant spec and native acceptance;
build measurements alone do not establish runtime or interaction correctness.

每批记录工具链、硬件、源码 commit、feature/profile 与 target 状态，并保存 Cargo
timing、无改动/Rust/Slint 修改样本、生成源码和缓存体积。热样本至少三次；冷 A/B
使用独立全新 target。交付检查还包括根包 doctest；保留原始证据、失败、未执行检查
以及采用/否决结论。后续候选必须重新建立受控基线，不能用档案数字代替当前对照。
仅针对编译单元边界批次，可用两侧预先定义、确实改变隔离 owner 生成代码的无害
Slint 语义夹具替代注释组；必须明确记录替代方式及未测的注释路径，不能据此宣称
注释编辑耗时收益。其它批次不自动适用，热样本数、采用门槛及共享组件正控保持不变。

构建/profile 候选沿用既有门槛：相关构建中位耗时改善至少 10%，或稳定 target
体积减少至少 10% 且相关中位耗时回退不超过 5%。结构、渲染器或 UI 边界变化还须
完成对应 spec 与原生验收；构建对照不能证明运行时或交互行为正确。

## Cleanup

The Settings opt-level change needs an initial artifact warmup without cache
cleanup. To reclaim historical full/unpacked debug artifacts from the older
policy, remove the previous target once:

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

配置修改不会删除旧产物。升级后应执行一次 `cargo clean`，以后尽量按 profile
或交叉编译 target 定向清理。Cargo 当前不会自动限制项目 `target/` 的容量。

## Historical evidence / 历史证据

[The archive](archive/build-performance.md) retains the original English and
Chinese records, including the July profile/IDE A/B, August P0–P5 roadmap,
compile-boundary and PR-filter rejections, dependency/toolchain and development
shell decisions, and September dev-runtime measurements. Measurements, failed
attempts and reported limitations keep their original dates and context.

[历史档案](archive/build-performance.md)保留原中英文记录，包括七月 profile/IDE
A/B、八月 P0–P5 路线与编译边界/PR 筛选否决实验、依赖/工具链和开发 Shell 决策，
以及九月 dev 运行时测量。测量、失败尝试与局限均保留原日期和上下文。
