# P5-C performance and regression evidence / 性能与回归记录

Date: 2026-09-07. Scope: proposal #60 and #61. Starting revision: `6183769`;
the working tree was clean. Each item uses the preceding production implementation
as A and is accepted separately before starting the next item.
Documentation and manual-check closeout: 2026-09-08.

## Protocol / 协议

Release/FemtoVG, Light/en, two Tokio workers, isolated offline state, no real
repository or provider. A and B carry identical opt-in fixture code. The runner
uses `scripts/benchmark-p5-a.py` to sample the real application's main-process RSS
roughly every 250 ms. Three fresh-process pairs run in AB/BA/AB order: Preview
uses 40 seconds per process; Find additionally uses an 80-second measurement
after a rejected 40-second run exposed mismatched workload phases. Peak includes startup;
stable is the median of the last quarter;
growth is stable minus the second-quarter median. Reported A/B values are medians
of three runs. Compilation/tests stop during paired measurement.

Preselected budget: B peak/stable increase <= max(8 MiB, 5% of A); B phase growth
and its increase over A <= 8 MiB. Every process must reveal its window, complete
the fixture and exit normally within eight seconds. These are fixed short
fixtures, not a long-soak guarantee or global memory bound.

Preview fixtures open the real Markdown provider and docked host: a single
12,000-line mixed CJK/English code fence, or 2,000 mixed-script paragraphs with
inline styles. Sixteen scroll steps cover fractional positions and the final
tail, with two window-size changes. Record layout callback time and cumulative
code-line mounts; final document tail must be reachable. Worker preparation
timing is separate from UI callback timing; neither is rendered frame latency.

Find uses the real root callbacks and visible Files changed route. Twelve rounds
each issue seven rapid queries (`a`, `alpha`, `İ`, `中文`, `none`, `end`, `中文`).
Rounds 0-3 scan 30,000 Unified rows; rounds 4-7 replace the model with 30,000 Split
rows; rounds 8-11 replace it with a 7,000-row Unified file. The final query must
publish exactly 30,000, 60,000 or 7,000 matches respectively, with correct first
and last row/text. Each round permits five seconds for the final result. Timing
is the synchronous cost of the seven callback invocations, not worker completion
or rendered frame time. These rapid bursts intentionally exercise latest-query
cancellation, while semantic unit tests compare completed scans across queries.

Runner: `scripts/benchmark-p5-c.py`. Raw logs, samples, result JSON and executable
hashes are under ignored `memory-benchmark-results/p5-c/`; captured Release
executables are under `target/release/p5-{task}-{side}/`. Only finished paired
results are acceptance evidence. Baseline executables require the corresponding
intermediate source and cannot be reconstructed by building the final source.

## Results / 结果

### #60 accepted / 已验收

| Scenario | Peak A → B MiB | Stable A → B MiB | Growth A → B MiB | UI callback P95 A → B ms | Created code lines A → B |
| --- | ---: | ---: | ---: | ---: | ---: |
| 12,000-line fence | 361.89 → 183.38 | 314.30 → 158.58 | -0.36 → -23.56 | 42.57 → 0.11 | 36000 → 710 |
| 2,000 mixed-script paragraphs | 121.09 → 120.14 | 120.67 → 119.22 | -0.34 → -0.41 | 200.02 → 0.58 | 0 → 0 |

Both three-pair scenarios passed the preselected RSS gate. All twelve processes
reached the final tail and exited normally; B emitted no framebuffer errors.
P95 is the median of each run's callback P95, not rendered frame time. Row mounts
are cumulative over the complete fixture, not simultaneous live-widget counts.
Accepted evidence: `task60-code/`, `task60-paragraphs/`; binaries:
`target/release/p5-60-a/usher`, `target/release/p5-60-b/usher`.

### #61 RSS accepted / RSS 已验收

| Scenario | Peak A → B MiB | Stable A → B MiB | Growth A → B MiB | UI callback P95 A → B ms |
| --- | ---: | ---: | ---: | ---: |
| Rapid Find, Unified/Split/file replacement, 80 s | 313.88 → 139.34 | 309.05 → 136.75 | +0.03 → 0.00 | 1396.99 → 1.82 |

`task61-aligned/` passed the original three-run median RSS gate. All 72 final-query
checks across six processes had correct counts and first/last matches; all six
exited normally, and B emitted no framebuffer errors. B's three measured growth
values were -7.39, 0.00 and 0.00 MiB. Maximum shutdown time was 199 ms on A and
137 ms on B. B includes accepted #60; this comparison isolates #61.

## #60 preflight / 预检

The A Release code-fence pilot completed and exited normally: peak 362.78 MiB,
stable 316.27 MiB, cumulative code-line mounts 36,000, exact extent 228,187 px,
final tail reached. Evidence: `task60-pilot/`. This single run is not paired
acceptance evidence.

The first optimized development-window inspection exposed a black code region
with OpenGL `Invalid framebuffer operation`: rounded clipping still required
an offscreen surface as tall as the complete fence. The corrected implementation
keeps the full background/border geometry but clips the bounded code-line window
separately. A second real-window inspection showed continuous lines 11971–11999
and the final tail, scrolling upward, divider resizing and closing the preview;
no framebuffer errors remained. Development-window RSS is not A/B evidence.
The runner also rejects B when a framebuffer error appears in its log.

After the correction, 29 Preview tests and two shared-computation tests passed.
Coverage includes worker/synchronous exact-extent agreement, replaced documents,
width changes returning to a previous width, mixed-script typography, source
ordering, cache limits and shutdown. Full root UI compiled; UI policy passed.

Build note: the first B production build spent about 75 minutes optimizing the
generated `usher-ui` crate. A macOS compiler sample localized the time to LLVM
`GlobalsAAResult::DeletionCallbackHandle::deleted()` during inlining cleanup.
The UI archive completed just before the build was interrupted; restarting
`cargo build --release --locked` reused it and continued with the application.
No compiler/profile or vendored-source changes were made. This compilation
anomaly is separate from application RSS; sample evidence is retained at
`/private/tmp/usher-p5-60-compiler-sample.txt`.

## #61 preflight / 预检

The A pilot completed all twelve rounds and exited normally: peak 324.13 MiB,
stable 284.31 MiB. Seven synchronous queries occupied the event loop for about
0.18-1.41 seconds per round. Evidence: `task61-pilot/`; this is not paired
acceptance evidence. Eleven focused Find tests passed, including exact comparison
with the original matcher for CJK, Unicode lowercase expansion, sanitized tabs,
repeated hits, excluded hunk rows and Unified/Split ordering, plus cancellation
and bounded converted-hit caching.

The B adapter shares the original source/projection, publishes a compact hit and
row index from the bounded latest-computation worker, and lazily converts hits
and row models with separate 128-entry caches. A dedicated result revision
refreshes the current item even when successive scans have equal hit counts.
Close/replacement cancels stale work. Results select the first hit on completion
without any deferred viewport command. Enter during a pending scan flushes the
debounce but is not replayed; navigation uses a subsequent explicit command on
completed results. Memory still scales with the active source and number of hits;
the matching library's call for one unusually long line cannot be interrupted
internally, though subsequent rows/hit projection observe cancellation.

### Rejected 40-second Find comparison / 未通过的 40 秒对比

`task61/` retained all six successful fixture/clean-exit runs, but its unchanged
RSS gate returned **false**. Median peak was 319.56 -> 144.31 MiB and stable RSS
291.41 -> 142.58 MiB; growth was -22.84 -> -0.44 MiB. The growth difference of
+22.41 MiB exceeded the 8 MiB budget. A's 10-20 second reference phase still ran
large searches before the smaller-file replacement, while B completed much
earlier. Thus this difference mixes source/workload transitions with retention.
Do not count this run as accepted or discard its evidence.

The accepted follow-up uses the exact same binary hashes, scenarios, order and thresholds,
with **80 seconds for both sides**. Growth then compares the 20-40 second median
with the 60-80 second median, placing both medians after the final query and file
replacement. A-1 still showed delayed RSS release near 30 seconds (-27.78 MiB),
while A-2/A-3 each measured +0.03 MiB; the preselected three-run median gate passed.
A 120-second extension was considered while inspecting A-1, but was not run after
the complete three-pair result passed. This duration adjustment and per-run
variation are retained explicitly; the memory budget was not relaxed.

### Manual scope / 手工范围

The isolated Release B window rendered the narrow wrapped CJK/English source and
completed the fixture again. The automated fixture deliberately uses no syntax
tokens and does not open the Find bar; its timing is a real callback/worker test,
not a syntax-paint or keyboard-to-frame measurement. Opening the file list made
the narrow wrapped source visible through the normal plain-source path.

Native automation delivered `super+f`, `ctrl+f` and `Meta_L+f` as ordinary `f`
characters in the file filter, even after raising the window. Therefore Find bar
typing, equal-count visual replacement, Enter/Shift+Enter, wheel/thumb scrolling
after navigation and their complete width/theme matrix were **not manually
verified**. Do not treat callback fixture completion or unit tests as those checks.
Preview manual scope is recorded above; the detached/theme matrix and long soak
remain unverified. The isolated manual instance was closed after inspection.

## Reproduction / 复测

Use the retained executables and fresh output directories; stop other builds and
tests while sampling. On macOS the runner needs access to the desktop and `ps`.

```sh
python3 scripts/benchmark-p5-c.py --task 60 --scenario code --a target/release/p5-60-a/usher --b target/release/p5-60-b/usher --output memory-benchmark-results/p5-c/repeat60-code
python3 scripts/benchmark-p5-c.py --task 60 --scenario paragraphs --a target/release/p5-60-a/usher --b target/release/p5-60-b/usher --output memory-benchmark-results/p5-c/repeat60-paragraphs
python3 scripts/benchmark-p5-c.py --task 61 --scenario find --seconds 80 --a target/release/p5-61-a/usher --b target/release/p5-61-b/usher --output memory-benchmark-results/p5-c/repeat61
```

## Final checks / 最终检查

- `cargo fmt --all`: passed.
- `cargo check --locked`: passed, including the complete root Slint UI.
- `cargo test --lib --locked` outside the sandbox: 523 passed, one failed, two
  ignored. The sole failure is the previously recorded
  `chat::interactive::pagination_tests::real_layout_prepend_and_bidirectional_scrolling`
  at `src/chat/pagination_tests.rs:174` (missing real message at offset 9984).
  Its source and Chat implementation are unchanged in this batch. The logger
  doctest was not run by this library-only command.
- `bash scripts/check-ui-policy.sh` and `git diff --check`: passed.
- `bash scripts/check-architecture-structure.sh`: seven existing over-budget
  categories remain: composition-root lines 1117/1100, PR controller lines
  5315/4600 and functions 107/95, Chat lines 2650/2200 and functions 76/70,
  AI controller lines 3933/3850, root Slint callbacks 149/140. PR controller
  decreased from 5351 lines; callback count is unchanged. Budgets were not raised.
- The full library test link emitted the existing macOS `__eh_frame` size
  warning. No new Rust warning or unrelated worktree modification was found.
