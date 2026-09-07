# P5-A performance and regression evidence / 性能与回归记录

Date: 2026-09-07. Scope: proposal #55 and #56 only. This is a dated measurement
record, not the architecture/specification source of truth.

## Protocol / 协议

- A baseline: application source at `2ff3f3b`; preserve the existing proposal
  document edit. Build from source using `cargo build --release --locked`.
- #55 compares baseline against only the task-persistence shutdown fix.
  #56 compares the accepted #55 binary against #55 plus the Logo driver fix.
- Same machine, production FemtoVG renderer, two shared Tokio workers, Light
  theme, English locale, isolated configuration/data directories, no real
  repository, tokens or AI provider calls. RSS covers the main Usher process,
  not the compiler, local HTTP fixture server or OS helper processes.
- `scripts/benchmark-p5-a.py` runs three fresh-process pairs in AB/BA/AB order,
  with 24 seconds per process and external `ps` RSS sampling every 250ms plus
  sampling overhead. Samples are taken from process start, so the peak includes
  startup. Stable RSS is the median of the last quarter; growth is that value
  minus the median of the second quarter. The comparison uses medians across
  the three processes on each side. This is not a frame-time or long-soak test.
- Before measurement, set the acceptance budget: B peak and stable RSS may not
  exceed A by more than `max(8 MiB, 5% of A)`; B phase growth and the increase in
  phase growth must each be at most 8 MiB. All B processes must exit normally.
- #55 fixture contains 128 interrupted task summaries. Verify startup hydrates
  and persists all 128 as failed, then measure startup/settled memory and exit.
  Deterministic queue/write/shutdown race tests complement this app-level run;
  the RSS fixture does not simulate sustained AI provider traffic.
- #56 fixture contains 32 projects with distinct local Logo URLs and PR prefetch
  disabled. The local server delays all responses until three seconds after
  server start, then returns empty 404 responses with 50ms latency. Both sides
  must reveal the window. Freeze request/concurrency counters before shutdown;
  require B to finish exactly three attempts for each of the 32 URLs (96
  requests total), with at most four concurrent requests. The grace period
  prevents the original failure loop from starving window reveal and invalidating the visible-window RSS comparison.
- Save binary SHA256, protocol metadata, per-process logs, raw samples and
  results under the ignored `memory-benchmark-results/p5-a/` directory. Keep
  source checkpoints as separate Release binaries under `target/release/`.

## Fixture pilots / 夹具预检

The baseline task pilot hydrated and persisted 128 interrupted summaries, with
113.69 MiB sampled peak RSS and a normal exit. The initial immediate-404 pilot
starved window reveal and normal shutdown; its logs remain in
`memory-benchmark-results/p5-a/logo-pilot/` and it is excluded from A/B acceptance.
With the common three-second response grace period, the 16-second Logo pilot
revealed the window and recorded 4,010 requests, 32 peak concurrent requests,
126.66 MiB peak RSS and 2.53 MiB phase growth. Shutdown still exceeded eight
seconds and the benchmark killed that process. Pilot values are not paired
acceptance results.

## Paired results / 成对结果

Both tasks passed all three pairs independently. Values are medians, in MiB:

| Task | Peak A → B | Stable A → B | Phase growth A → B | Gate |
| --- | ---: | ---: | ---: | --- |
| #55 | 114.86 → 113.75 | 114.69 → 113.34 | +0.06 → -0.36 | Pass |
| #56 | 128.02 → 125.45 | 127.56 → 125.08 | +1.16 → 0.00 | Pass |

All six #55 processes exited normally (48–196ms) and persisted all 128 task
summaries correctly. The decrease is small and is not a claimed general memory
optimization; the shutdown fix passed the preselected RSS regression budget.
Raw results: `memory-benchmark-results/p5-a/task55/{metadata,summary}.json`.

#56 request count fell from a median 6,620 to exactly 96 in every B process:
each of the 32 URLs was requested three times, with no requests left active at
the end. Peak concurrency fell from 32 to 4. All three A processes exceeded the
eight-second shutdown limit and were killed; all three B processes exited
normally in 74–130ms. All six revealed their windows. The failure loop is
removed; this is not evidence for unrelated Project projection or image-cache
optimizations. Raw results: `memory-benchmark-results/p5-a/task56/`.

### Per-process samples / 单进程结果

| Task | Run | Peak MiB | Stable MiB | Growth MiB | Requests | Shutdown ms | Clean exit |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| #55 | A-1 | 113.97 | 113.55 | -0.36 | 0 | 88 | Yes |
| #55 | A-2 | 114.86 | 114.69 | +0.06 | 0 | 126 | Yes |
| #55 | A-3 | 115.48 | 115.31 | +0.38 | 0 | 126 | Yes |
| #55 | B-1 | 113.92 | 113.48 | -0.36 | 0 | 196 | Yes |
| #55 | B-2 | 113.55 | 113.12 | -0.36 | 0 | 48 | Yes |
| #55 | B-3 | 113.75 | 113.34 | -0.34 | 0 | 125 | Yes |
| #56 | A-1 | 124.12 | 123.95 | +1.16 | 6620 | 8008 | No (killed) |
| #56 | A-2 | 128.02 | 127.56 | +1.11 | 6689 | 8005 | No (killed) |
| #56 | A-3 | 129.48 | 129.33 | +2.39 | 6527 | 8007 | No (killed) |
| #56 | B-1 | 126.38 | 126.33 | +0.11 | 96 | 74 | Yes |
| #56 | B-2 | 124.61 | 124.20 | -0.38 | 96 | 130 | Yes |
| #56 | B-3 | 125.45 | 125.08 | +0.00 | 96 | 127 | Yes |

## Functional checks / 功能检查

#55 deterministic tests exercise the actual worker module: idle shutdown with
live senders; coalesced queued snapshots followed by shutdown; writes in progress
with later snapshots and an I/O error; disabled writes and repeated shutdown.
The queue barrier must persist the latest preceding snapshot and must not process
data after the barrier. Headless module validation initially passed all four.

#56 pure policy checks cover finite retries, cooldowns, manual recovery,
four-slot admission, reload without duplicate in-flight requests, stale
completion rejection, and metadata removal when sources are retired. All four
pure-policy tests and both actual HTTP driver tests passed. The latter
cover 404 cooldown and explicit reload recovery to a decodable PNG, the shared
client’s real 30-second timeout, cooldown before retry, and socket closure when
shutdown cancels the next stalled attempt. The first sandbox run could not bind
a local listener; it was rerun with local-network permission. An early timeout
fixture inherited nonblocking sockets on macOS; setting accepted sockets to
blocking fixed the fixture, and the final six-test run passed in 32.03 seconds.

## Repository validation / 仓库验证

- `cargo fmt --all`: passed.
- `cargo check --locked`: passed after restoring the final sources.
- `cargo test --lib --locked project::logo` outside the sandbox: six passed,
  including both real-loopback HTTP tests (32.03s).
- `cargo test --lib --locked` outside the sandbox: 503 passed, one failed,
  two ignored, on both runs (32.37s and 32.34s). The failure is the untouched
  `chat::interactive::pagination_tests::real_layout_prepend_and_bidirectional_scrolling`,
  at `src/chat/pagination_tests.rs:174`, “viewport must contain a real message
  at 9984”. Serial full execution also reproduces it (63.58s); concurrency alone
  does not explain the failure. All ten new tests passed in these full runs.
- The current Chat test alone inside the sandbox passed (49.99s).
- To establish a baseline, temporarily compiled the six modified source files
  from `HEAD` (new modules were unreferenced), copied that library test executable,
  and restored the final files byte-for-byte before running it. All 494 baseline
  tests passed inside the sandbox (two ignored, 48.91s). **The same unmodified
  baseline Chat test outside the sandbox failed at the identical line/offset**
  (14.68s). Thus the full-suite failure is environment-dependent and predates
  these changes; this does not claim the existing Chat behavior is fixed.
- `cargo test --lib --locked -- --skip project::logo_loader::tests` inside the
  sandbox: 502 passed, zero failed, two ignored, two filtered (49.37s). Together with
  the two passing loopback driver tests outside the sandbox, every non-ignored
  library test passed in its supported test environment; the single-command
  full-suite failure above remains explicitly recorded.
- `git diff --check` and `python3 -m py_compile scripts/benchmark-p5-a.py`: passed.
- `scripts/check-architecture-structure.sh`: failed on seven existing budgets:
  composition root lines, PR Review controller lines/functions, Chat controller
  lines/functions, AI Review controller lines, and root Slint callbacks. The
  baseline had eight; extracting task persistence brought the AI Review function
  count within its budget without raising any limits.

The existing linker warning about `__eh_frame` compact-unwind size remains.
No full workspace/doctest run or all-route manual visual QA was performed;
the known logger doctest issue was not rechecked. Real app A/B runs verify window
reveal, fixture execution, RSS and shutdown, not full Project-dialog behavior.
Long-soak memory, large successful image payloads and P5-B projection/queue/cache
work remain outside this fixed-fixture acceptance. Existing proposal edits were
preserved and augmented; no unrelated working-tree edits were overwritten. The
untracked `code-quality-proposal.md` that appeared during final validation was
left untouched.

Detailed local validation logs are in `/private/tmp/usher-p5-*`; paired raw
samples and app logs remain under `memory-benchmark-results/p5-a/`.

## Reproduction / 复现

Capture each source stage with `cargo build --release --locked`, copying the
binary before editing the next stage. Output directories must be new:

```sh
python3 scripts/benchmark-p5-a.py --a target/release/p5-baseline/usher --b target/release/p5-55/usher --scenario tasks --output memory-benchmark-results/p5-a/task55
python3 scripts/benchmark-p5-a.py --a target/release/p5-55/usher --b target/release/p5-56/usher --scenario logos --output memory-benchmark-results/p5-a/task56
```

Recorded binary SHA256:

| Stage | SHA256 |
| --- | --- |
| Baseline | `20c8ef593df2721b529026ee1fcef31b56b1246e0ead078070c32671d4ef1282` |
| #55 | `ceb9a8087ddddea3a0bf8dfe3d217ec35216cade390b459fd623e5833d8c6c18` |
| #55 + #56 | `eb3e6fbe3a414653ac8dffdd760ee67c8084360e5c7572c0f793bcea79147fdd` |

The benchmark requires permission to launch the desktop app, bind its loopback
fixture server and read process RSS. It never uses the user's normal app state.
