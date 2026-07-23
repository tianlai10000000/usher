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
