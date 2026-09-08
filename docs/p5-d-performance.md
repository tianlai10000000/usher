# P5-D architecture and performance gates / 架构与性能门禁

Date: 2026-09-08. Scope: proposal #62 and #63. This is a dated measurement
record, not a stable feature specification. / 本文记录实测与验收，不替代功能 spec。

## #62 ownership / 职责收敛

Started from clean `8b29a8f` after #60/#61. No existing controller tests were
moved or deleted, and no original architecture budget was raised.

| Owner | Before lines | After lines | Original budget |
| --- | ---: | ---: | ---: |
| `src/lib.rs` | 1117 | 1034 | 1100 |
| PR Review controller | 5315 | 4287 | 4600 |
| Chat controller | 2650 | 2165 | 2200 |
| AI Review controller | 3933 | 3846 | 3850 |
| Root Slint callbacks (count, not lines) | 149 | 135 | 140 |

PR Review separates remote actions/capability presentation, Diff event-loop
bindings, and opt-in diagnostics. Chat separates viewport/anchor policy and
the stable incremental message model. AI Review separates session I/O and
interrupted-task hydration. Settings save feedback leaves the composition root.
The stateless feature-owned `PrDiffBridge` global replaces fourteen root Diff
callbacks; names, signatures, page protocol and event-loop ownership are preserved.
All extracted owners have size tripwires; leaf modules cannot import controller
internals through the dependency forms checked by the architecture script.
PR action methods intentionally remain child implementations of the controller
to retain the existing typed request generations and lifecycle.

## #62 Release A/B

Captured immutable executables before and after this batch:

- A: `target/release/p5-62-a/usher`, SHA256
  `1d7f9e630d2b6e1e17614771584f869b2e16721f73bab9b43af8258d05f952fa`.
- B: `target/release/p5-62-b/usher`, SHA256
  `0e11fc30b78a6ab724bd98bcf44dd4dfad5f25673371ccd2ab290f7ac992c7dc`.

macOS 26.6.2 (25G83), arm64; default Release, winit/femtovg, two Tokio workers, isolated offline state,
English/light theme, three pairs in AB/BA/AB order. Main-process RSS is sampled
externally every 250 ms. Stable RSS is the last-quarter median; phase growth is
that median minus the second-quarter median. The unchanged acceptance gate is
B peak/stable increase <= max(8 MiB, 5% of A), B growth <= 8 MiB and growth
increase <= 8 MiB. Every process must reveal its window and exit cleanly.

| Fixture | Seconds/process | Peak A -> B MiB | Stable A -> B MiB | Growth A -> B MiB | Gate |
| --- | ---: | ---: | ---: | ---: | --- |
| PR Find: 30k Unified, 30k Split, 7k Unified | 80 | 150.55 -> 150.63 | 150.08 -> 147.48 | 0.00 -> 0.00 | Pass |
| AI interrupted-task hydration: 128 records | 30 | 113.48 -> 114.48 | 113.38 -> 113.95 | -0.25 -> -0.33 | Pass |

All 72 Find round counts and first/last results were correct; no framebuffer
errors. All six hydration runs persisted all 128 interrupted tasks as failed.
All twelve processes exited cleanly. Find uses the unchanged #61 automatic
fixture, with only its callback binding redirected through `PrDiffBridge`.
This is behavior-preserving restructuring, with no claimed speedup.

Raw evidence: `memory-benchmark-results/p5-62-find/` and `p5-62-tasks/`.

```sh
python3 scripts/benchmark-p5-c.py --a target/release/p5-62-a/usher --b target/release/p5-62-b/usher --task 61 --scenario find --seconds 80 --output memory-benchmark-results/p5-62-find-repeat
python3 scripts/benchmark-p5-a.py --a target/release/p5-62-a/usher --b target/release/p5-62-b/usher --scenario tasks --seconds 30 --output memory-benchmark-results/p5-62-tasks-repeat
```

## #63 continuous gates / 持续门禁

CI adds `main` pushes alongside PR/manual triggers, read-only repository
permissions, and same-ref cancellation of superseded runs. Nightly builds one
Release executable, retains the existing Home RSS/footprint gate, then runs
Project, short-line Job, Preview code, Preview paragraphs and Find serially.
Hot paths run even if Home validation fails, provided the Release build succeeded.
The 150-minute job limit includes a cold build plus about 22 minutes of hot-path
sampling. Artifacts are uploaded on failure and retained for 14 days.

`scripts/benchmark-hot-paths.py` reuses `benchmark-p5-b.py` and
`benchmark-p5-c.py`; it does not duplicate their data seeding or app drivers.
Default A/B are identical-binary repeatability cohorts, explicitly labeled
`repeatability`; `--baseline` allows a captured before/after executable.
#63 changes only scripts, CI and documentation, so it uses #62 B unchanged on
both sides. Same-binary A/B is harness validation, not optimization evidence.

The policy requires every one of the six processes per fixture to fit absolute
budgets, in addition to the existing paired median gate. Limits were selected
from the accepted #57-#61 Release baselines before the new suite ran:

| Metric | Limit |
| --- | ---: |
| Every sampled RSS peak and settled RSS | 256 MiB |
| Per-process phase growth | 8 MiB |
| Project/Job callback P95 | 50 ms |
| Preview/Find layout/query callback P95 | 25 ms |
| Job queue / retained output / cancellation | 16 blocks / 32768 bytes / 2000 ms |
| Job cumulative row creations | 1024 |
| Code Preview cumulative line creations | 2000 |
| Every Find query delivery | 2000 ms |

The suite checks hashes/protocols, all 24 Project rounds and final 128/64 rows,
Job output/cancellation/tail flags, Preview final tail, all twelve Find
rounds/counts/head-tail validity, and clean shutdown. It recomputes RSS metrics
from finite, ordered raw samples with coverage of both measurement windows.
Missing files/metrics, invalid numbers, false completion, renderer errors,
failed child runners or changed executables fail closed. Failed runs and partial
suite summaries remain available; the suite continues to collect other cases.
The hot-path runner measures RSS, not physical footprint; Home retains both.

Reproduce with a new output directory on an idle macOS desktop:

```sh
cargo build --release --locked
python3 scripts/benchmark-hot-paths.py --binary target/release/usher --output memory-benchmark-results/hot-paths-repeat
python3 -m unittest discover -s scripts/tests -p 'test_*thresholds.py'
```

## #63 measured results / 实测结果

All five cases passed, with thirty fresh processes completing and exiting
cleanly. No thresholds were relaxed during the run. Values below are A/B
cohort medians except the last column, which is the largest per-process P95.

| Fixture | Peak A -> B MiB | Stable A -> B MiB | Growth A -> B MiB | Max callback P95 ms |
| --- | ---: | ---: | ---: | ---: |
| Project | 123.00 -> 122.95 | 122.56 -> 122.48 | -0.42 -> -0.47 | 12.569 |
| Job short-line tail | 135.72 -> 136.48 | 135.25 -> 136.00 | -0.36 -> -0.36 | 14.809 |
| Preview code | 184.22 -> 184.03 | 160.56 -> 168.58 | -23.55 -> -18.22 | 0.161 |
| Preview paragraphs | 121.22 -> 120.75 | 120.16 -> 119.66 | -0.44 -> -0.45 | 0.572 |
| Find | 151.81 -> 149.69 | 151.33 -> 148.84 | 0.05 -> 0.00 | 2.145 |

Across all thirty processes, the largest sampled RSS was 187.22 MiB and the
largest phase growth was 0.375 MiB. Job cancellation peaked at 267 ms, queue
occupancy at 16 blocks, retained output at 32768 bytes, and cumulative row
creations at 278. Code Preview created 710 lines per process. Every Project
fixture completed 24 rounds; every Find fixture completed all twelve rounds.
These timings measure callbacks/query delivery, not rendered frame latency.

Raw evidence: `memory-benchmark-results/p5-63-hot-paths/`, consolidated
`hot-paths.json`, child stdout logs and per-run app logs/state/RSS samples.

The code Preview repeatability pair has a narrow stable-RSS margin: medians
160.5625 -> 168.578125 MiB, increase 8.015625 MiB against the unchanged
8.028125 MiB budget. Both cohorts have negative phase growth, with different
timing of memory release. This passed result must not be described as zero
measurement variance; the full per-process evidence is retained.

## Checks and limits / 检查与限制

| Check | Exact outcome |
| --- | --- |
| `cargo build --release --locked` before/after #62 | Both passed; B build had no compiler warnings |
| `cargo fmt --all` and `git diff --check` | Passed |
| `cargo check --locked` | Passed |
| `scripts/check-architecture-structure.sh` | Passed, original budgets unchanged |
| `scripts/check-ui-policy.sh` and `scripts/check-i18n.sh` | Passed |
| Python threshold-policy unittest discovery | 12 passed (10 hot-path policy tests plus 2 existing Home policy tests) |
| `cargo test --locked --test pr_review_core_journeys` | 4 passed |
| `CARGO_INCREMENTAL=0 cargo check --workspace --all-targets --no-default-features --features renderer-femtovg --locked` | Passed; two existing conditional-import warnings in `src/settings_page.rs` |
| `CARGO_INCREMENTAL=0 cargo check --lib --bins --features test-fixtures,dev-shell --locked` | Passed |
| `CARGO_INCREMENTAL=0 cargo check --workspace --all-targets --features test-fixtures,dev-shell --locked` | Failed in unchanged `tests/ai_review_response_ui.rs`: inline Slint harness has no `@i18n` library mapping |
| Workflow YAML and push/build/artifact contract inspection | Parsed with Ruby YAML; `main` push, one Release build and failure upload conditions checked |

The first all-target check exposed one stale integration-test registration on
`MainWindow`; it now binds `PrDiffBridge`. Its debounce assertions also now
include the immediate empty-query cancellation already present in #61's
`changed diff-find-query` handler at `8b29a8f`, followed by the unchanged 180 ms
replacement query. No test was moved, skipped, or deleted. The four PR core
journey tests then passed. These test-only adjustments do not change Release B.

The root unit suite outside the sandbox returned 523 passed, 1 failed, 2 ignored.
The failure is the pre-existing
`chat::interactive::pagination_tests::real_layout_prepend_and_bidirectional_scrolling`
at `src/chat/pagination_tests.rs:174`, missing a real message at offset 9984.
That test passed in the sandbox, where two localhost Logo tests instead failed
because listening was denied; both Logo tests passed outside the sandbox.
The environment-dependent Chat baseline remains a failure, not a green suite.
The macOS test linker also emitted the existing `__eh_frame` size warning.

Manual CUA inspection used an isolated offline temporary app bundle containing
the same B SHA256. Narrow Diff showed wrapped Chinese text and selection, and
scrolling advanced from the first rows to rows 42-48. Chat showed its loaded
fixture, survived up/down scrolling, switched to the separate empty RAG state,
and restored AI messages after leaving and reopening the route. The temporary
window exited cleanly. These checks were separate from RSS measurement.

No new paired Chat input/resize trace or complete remote-provider matrix was
run. Both `super+f` and `ctrl+f` through CUA inserted a literal `f` in the file
filter, as in the preceding batch; manual Find-bar focus/painted navigation is
still unverified. The automatic Find fixture exercises callbacks with the bar
closed. Short RSS runs do not establish a long-term soak guarantee.
GitHub-hosted macOS/Windows jobs have not been dispatched from this local task;
the existing unit/inline-harness failures mean full CI is not claimed green.
The known logger doctest is outside `cargo test --lib`; full doctests were not run.
Workspace started clean; no unrelated user changes were overwritten, and no
commit was created.
