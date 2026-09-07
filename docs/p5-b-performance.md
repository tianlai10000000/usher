# P5-B performance and regression evidence / 性能与回归记录

Date: 2026-09-07. Scope: proposal #57–#59. This is a dated measurement record.

## Protocol / 协议

Starting source: `a42ce0e` with a clean working tree. Each item is implemented
and measured separately before proceeding to the next. A and B include identical
opt-in fixture code; A retains the preceding production implementation. Fixtures
require `USHER_P5_BENCHMARK`, offline mode and an isolated state root together.
No real provider, credentials or project repositories are used.

`scripts/benchmark-p5-b.py` uses the P5-A real-app runner: Release/FemtoVG, two
Tokio workers, Light theme, English locale, externally sampled main-process RSS
approximately every 250 ms, three fresh-process pairs in AB/BA/AB order. Every
process runs for 30 seconds. Peak includes startup; stable is the median of the
last quarter and phase growth is stable minus the median of the second quarter.
Reported comparisons are medians of the three runs on each side. Compilation
and tests are stopped during measurement.

Preselected gate: B peak/stable increase must be <= max(8 MiB, 5% of A). Both B
phase growth and its increase over A must be <= 8 MiB. Every process must reveal
its window, complete the fixture, and exit normally within eight seconds.
Raw logs, per-process results, samples and binary hashes live in ignored
`memory-benchmark-results/p5-b/`; source checkpoints are Release executables in
`target/release/p5-{task}-{side}/`.

#57 fixture: 128 projects, 16 local 512x256 raster sources and one SVG, 24 bursts
of up to 64 overview completions, alternating real search callbacks every 250 ms.
Verify final row count and overview values. Repeat separately with diagnostics
on and 16 projects. Dispatch-to-UI latency measures callback scheduling, not
rendered frame time. Fixture completion verifies the final projected values.

Results are appended only after each paired run finishes. These short fixtures
do not establish a process-wide memory ceiling or replace a long soak test.

## #57 rejected pilot / 已否决首轮

The first three pairs failed the RSS gate: A peak/stable medians were
123.36/122.94 MiB; B was 915.66/915.20 MiB. The new SVG decoder eagerly scanned
and retained the installed system font database. All fixtures completed and
exited cleanly, which does not override the failed memory budget. Evidence:
`memory-benchmark-results/p5-b/task57/`; rejected binary:
`target/release/p5-57-b/usher`.

The corrected decoder lazily loads the bundled application font using shared
static bytes when SVG text requests a font. Geometry-only SVGs need no font
database. Custom SVG font families fall back to the bundled Noto Sans SC; no
installed-font scan occurs. The repeated paired fixture includes Chinese SVG
text on both sides to exercise this path explicitly.

## #57 accepted results / 验收结果

| Scenario | Peak A → B MiB | Stable A → B MiB | Growth A → B MiB | Callback P95 A → B ms | Gate |
| --- | ---: | ---: | ---: | ---: | --- |
| 128 projects + Chinese SVG | 119.56 → 120.08 | 119.19 → 119.64 | -0.33 → -0.44 | 63.84 → 9.01 | Pass |
| 16 projects + diagnostics | 114.83 → 116.16 | 114.73 → 115.52 | -0.06 → -0.62 | 15795.31 → 2.98 | Pass |

All twelve processes completed 24 rounds with the expected final row/count
values, revealed windows and exited cleanly. B applied 25–26 UI projections per
process. The latency is dispatch-to-callback scheduling, not rendered frame time.
The corrected binary is `target/release/p5-57-b2/usher`; raw evidence is under
`task57-v2/` and `task57-diagnostics/`.

Validation: 13 Project unit tests passed, plus both real localhost HTTP Logo
driver tests passed outside the sandbox. `cargo check --locked` and Release
build passed. SVG text now uses the bundled font fallback as documented.

## #58 protocol and pilot / 协议与预检

The real Tag command driver executes a local Python child: one 4 MiB line,
then 16 concurrent stdout/stderr bursts at 250 ms intervals, including Chinese
workflow annotations. A second busy child is cancelled after 250 ms. The real
Job Run presenter is expanded and records cumulative row creations and
publish-to-UI latency. Final stdout/stderr markers, cancellation and the visible
model's final tail must agree. B additionally requires queue occupancy <= 16,
retained tail <= 32768 bytes and cancellation <= 2000 ms.

Run two three-pair scenarios. Default retains part of the initial long line.
`--short-lines` uses an identical saved interpreter adapter on A and B to change
only stdout bursts from 256 two-byte lines to 512 eight-byte lines. This fills
more than 32 KiB and evicts the long line so short-line virtualization is also
exercised. The adapter source and SHA256 are saved alongside each result; both
immutable application binaries still use identical fixture entry points.

The single A pilot (`task58-pilot/`) completed and exited normally: peak 255.50
MiB, stable 255.00 MiB, 4,147 final model rows, 31,284 cumulative row creations,
259 ms cancellation. This pilot is not paired acceptance evidence.

## #58 accepted results / 验收结果

| Scenario | Peak A → B MiB | Stable A → B MiB | Growth A → B MiB | Created rows A → B | UI P95 A → B ms | Gate |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| Retained long line | 253.02 → 143.61 | 252.50 → 143.12 | -0.38 → -0.36 | 34942 → 43 | 342.25 → 4.91 | Pass |
| Short-line tail | 227.08 → 136.73 | 226.61 → 136.27 | -0.36 → -0.34 | 37375 → 264 | 398.08 → 4.97 | Pass |

All twelve processes completed and exited normally. Every B transport retained
at most 32768 bytes and stayed within the 16-block queue limit; cancellation
remained below 2000 ms. Created rows are cumulative mounts, not simultaneous
live-widget counts. Fixtures use zero projects (the original runner's generic
`project_count` metadata field was applicable only to #57; later runs correct it).

Validation: seven Jobs tests and twelve Tag tests passed, including UTF-8/CRLF,
backpressure, exit drain, cancellation, parsing reuse and stable model identities.
Full root UI compiled; UI policy checks passed. In the same B executable wrapped
in a temporary isolated app bundle, visual checks covered inner/outer scrolling,
reaching the final `TAIL-1-15` row, collapse/reopen, 800x600 and zoomed window
layouts. Model checks confirmed both stream markers. This does not claim complete
light/dark, all source-workflow or live-Stop visual coverage. Window Close hides
to tray, so the owned test process was stopped with SIGTERM after inspection.

## #59 protocol and pilot / 协议与预检

Four rounds hold a real PersistentStore callback for one second while 500 newer
64 KiB payloads arrive. Each flush must leave exactly the requested revision
(501, 1002, 1503, 2004) in the isolated output file. Each round also invokes 101
real Settings preference callbacks and publishes 50 local-only Inbox messages.
Verify the final persisted preference and UI unread count of 200. B must perform
at most eight preference file saves, allowing startup/exit writes around four
bursts. Raw logs record actual Settings saves and Inbox UI deliveries. Generic
unit tests separately assert one pending snapshot, flush/shutdown barriers,
failure recovery and one delivery while a synthetic UI consumer is busy.

The single A pilot (`task59-pilot/`) passed all revision, preference, UI and exit
checks: peak 190.22 MiB, stable 186.20 MiB. It is not paired acceptance evidence.

## #59 accepted results / 验收结果

| Scenario | Peak A → B MiB | Stable A → B MiB | Growth A → B MiB | Settings saves A → B | Inbox UI deliveries A → B | Gate |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| Slow persistence + preference/Inbox bursts | 187.91 → 122.84 | 184.14 → 122.41 | -3.72 → -0.42 | 405 → 5 | 201 → 73 | Pass |

All six processes persisted revisions 501/1002/1503/2004, retained the final
preference (`false`), displayed unread count 200 and exited normally. B shutdown
took 75–138 ms. Settings saves were exactly five on every B run; Inbox deliveries
were 65/73/97 (scheduling-dependent, median 73). UI deliveries represent replaced
view snapshots, not dropped Inbox messages. Evidence: `task59/`. This fixture
exercises the shared writer via PersistentStore; AI Review hydration, obsolete
revision rejection and shutdown are covered separately by its four unit tests.

## Reproduction / 复现

Run from the repository root with the saved Release checkpoints and a macOS
desktop session that allows local process inspection/listeners. Each output
directory must be new. Builds used `cargo build --release --locked`; A checkpoints
contain the same fixture as B and the preceding production implementation.
Building the final source alone cannot recreate the intermediate A binaries.
The recorded metadata includes the SHA256 of every measured executable.

```sh
python3 scripts/benchmark-p5-b.py --task 57 --a target/release/p5-57-a/usher --b target/release/p5-57-b2/usher --output memory-benchmark-results/p5-b/repeat57
python3 scripts/benchmark-p5-b.py --task 57 --diagnostics --a target/release/p5-57-a/usher --b target/release/p5-57-b2/usher --output memory-benchmark-results/p5-b/repeat57-diagnostics
python3 scripts/benchmark-p5-b.py --task 58 --a target/release/p5-58-a/usher --b target/release/p5-58-b/usher --output memory-benchmark-results/p5-b/repeat58
python3 scripts/benchmark-p5-b.py --task 58 --short-lines --a target/release/p5-58-a/usher --b target/release/p5-58-b/usher --output memory-benchmark-results/p5-b/repeat58-short-lines
python3 scripts/benchmark-p5-b.py --task 59 --a target/release/p5-59-a/usher --b target/release/p5-59-b/usher --output memory-benchmark-results/p5-b/repeat59
```

## Final validation and limits / 最终验证与限制

| Command/check | Actual result |
| --- | --- |
| `cargo fmt --all` | Pass |
| `cargo check --locked` | Pass, including full root Slint compilation |
| `cargo test -p usher-state-store --locked` | 12 passed; zero doctests |
| `cargo test --lib --locked` outside sandbox | **514 passed, 1 failed, 2 ignored** |
| Same library test executable inside sandbox, `--skip project::logo_loader::tests::` | 513 passed, 0 failed, 2 ignored, 2 filtered |
| Two localhost Logo driver tests | Both passed outside sandbox, including in the full library run |
| `bash scripts/check-ui-policy.sh` | Pass |
| `bash scripts/check-architecture-structure.sh` | Fails seven existing budget categories; see below |
| `git diff --check` | Pass |

The full library failure is
`chat::interactive::pagination_tests::real_layout_prepend_and_bidirectional_scrolling`
at `src/chat/pagination_tests.rs:174`, viewport offset 9984. The previous
[P5-A baseline comparison](p5-a-performance.md) reproduced this on unchanged
source outside the sandbox; this batch does not modify Chat. The same final
test executable passes that test inside the sandbox. Full library output is
`/private/tmp/usher-p5-b-full-tests.log`, sandbox output
`/private/tmp/usher-p5-b-sandbox-tests.log`, and State Store output
`/private/tmp/usher-p5-59-store-tests.log`. Split-environment passes do not turn
the full command's failure into a pass.

Structure failures (actual/budget): composition root lines 1117/1100, PR Review
controller lines 5351/4600 and functions 107/95, Chat controller lines 2650/2200
and functions 76/70, AI Review controller lines 3933/3850, root Slint callbacks
146/140. These categories already exceeded their budgets at the starting
revision; no budgets were raised. Proposal #62 remains pending. Shared HTTP
client and Tokio runtime ownership searches in that check found no violations.

Architecture documents in both languages and affected specs/index were updated.
No vendored Material code was edited. The starting working tree was clean; no
unrelated user changes were overwritten. Manual coverage is limited to the #58
checks listed above. Root-package doctests (including the known logger doctest
baseline), long-duration stress and full-route/theme visual regression were not
run. The 30-second fixed-fixture RSS gates do not establish long-term bounds.
