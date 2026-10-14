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

[profile.dev.package.usher-ui-settings]
debug = "none"
opt-level = 1

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
application backtraces. Third-party dependencies and generated Slint UI crates
use opt-level 1; application-owned Rust remains opt0 with line tables. The
profile intentionally does not preserve full local-variable and type information
for LLDB. Use the isolated profile when that information is needed:

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

For UI iteration, install the viewer version required by the checked-in script,
then use the application-owned entry points:

```sh
cargo install slint-viewer --version 1.18.1 --locked
./scripts/ui-check.sh
./scripts/ui-live-preview.sh
```

PowerShell equivalents are `scripts/ui-check.ps1` and
`scripts/ui-live-preview.ps1`. UI checks compile the complete root UI; live
preview enables the development playground and watches UI edits in the running
application. Keep preview sessions short and validate the affected behavior with
normal `cargo run --locked` before handoff. Default and release builds use static
Slint compilation and do not enable live preview or the playground.

普通开发与集成使用完整默认应用。UI 迭代统一通过上述脚本；PowerShell 使用对应
`.ps1` 入口。live preview 开启开发演练场，只用于短时迭代；交付前仍通过普通
`cargo run --locked` 验证受影响行为。默认与 Release 构建继续静态编译 Slint。

When the change does not involve PR Review, the explicit development shell uses
an isolated Cargo target and a compatible empty implementation of the complete
PR Review feature package:

```sh
./scripts/dev-shell-check.sh
./scripts/dev-shell-run.sh
```

PowerShell equivalents are `scripts/dev-shell-check.ps1` and
`scripts/dev-shell-run.ps1`. These scripts keep artifacts under `target/dev-shell`.
This mode redirects PR Review navigation to Workflow. Normal and release commands
compile the complete page; final delivery requires the default checks and manual
validation of affected routes.

改动不涉及 PR Review 时，可使用显式开发 Shell；对应 PowerShell 入口为
`dev-shell-check.ps1` 与 `dev-shell-run.ps1`。脚本把产物隔离在 `target/dev-shell`，
并把 PR Review 导航回退到 Workflow。普通与 Release 命令仍编译完整页面；最终
交付仍须执行默认检查与受影响路由的手工验收。

## Measurement protocol for every batch

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

构建/profile 候选沿用既有门槛：相关构建中位耗时改善至少 10%，或稳定 target
体积减少至少 10% 且相关中位耗时回退不超过 5%。结构、渲染器或 UI 边界变化还须
完成对应 spec 与原生验收；构建对照不能证明运行时或交互行为正确。

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
