# P6 #70: Job log retention

## Predeclared protocol

The accepted #69 tree is A. Add identical opt-in fixture and accounting to both
sides before building A; change only Job measurement retention in B. This is
the Job sub-item of #70, not acceptance of PR eviction or Chat memory paging.

Default Release, FemtoVG, 1100x700, two Tokio workers, isolated offline state.
Three fresh pairs in AB/BA/AB order, 90 seconds/process; the existing P6 memory,
heartbeat, throughput, completion and clean-exit gates remain unchanged.
Do not build or test during measured runs. Keep rejected attempts separately.

Each of three cycles opens 16 distinct completed Job sources sequentially on
the actual JobRun route. Each log has 512 unique mixed ASCII/CJK/annotation
rows of about 2 KiB, within the current Hook replay budget. Production worker
layout must finish before moving on. Record the retained source/measurement
text and wrapped text bytes across the 16-entry viewport cache. Raw logs and
parsed semantics must remain intact; every completed cycle verifies the last
source and all 16 accepted layouts. Settled sampling follows all cycles.

Before acceptance, test Unicode/blank/control/long-row wrapping, unchanged
exact extents, append/edit/eviction snapshots, old snapshot stability, scrolling,
source changes and en/zh-CN. Existing production scroll tests remain required.
Byte accounting is owned allocation accounting, not allocator RSS.
The B fixture must reach shared snapshots for every accepted layout, with zero
additional measurement text; missing accounting or repeated/missing source IDs
fails the behavioral gate even when process memory passes.

Evidence: `memory-benchmark-results/p6-70-job-retention-20260916/`.

## Status

The diagnostic A process completed all 48 layouts. At 16 retained sources it
held 18,108,096 bytes of copied measurement text and 18,042,560 bytes of wrapped
text (34.48 MiB combined), excluding container allocations and source models.
Its settled RSS was 229.34 MiB. This single diagnostic run is not the A/B result.

`LogModel` did not override `Model::as_any`; Slint 1.17.1 defaults that method
to `&()`. Consequently the existing snapshot downcast never matched: production
measurement converted every row through Slint, then copied it into ParsedText.
The diagnostic A field `source_bytes=0` means the downcast was unavailable,
not that the source models held no text. The per-source raw fixture is 1,128,128
or 1,128,640 bytes; no cross-feature RSS ranking is inferred from this workload.

The Job sub-item is selected because it directly follows the accepted bounded
Hook replay and has a reproducible duplicate-retention path. PR cache policy
and Chat history retention remain independent #70 candidates, with no performance
acceptance claimed. Their complete-document reload, selection/Find and history
anchor contracts require separate fixtures; this change does not establish
byte budgets for PR, Chat or the 16-entry Job viewport cache.

## Candidate

- Implement the model downcast and pass the existing immutable `LogSnapshot`
  into requests/results by sharing its Arc. Generic external Slint models retain
  their conversion fallback. An old measurement retains its own immutable version
  until the existing generation/layout-key checks permit replacement.
- Keep exact Parley wrapping, bounded shaping pieces and the shared virtualizer.
  Store UTF-8 byte ranges over the semantic prefix/body instead of copied strings.
  Only visible source rows materialize Slint visual-line strings. Preserve the
  original raw log, annotations, line numbers, source cache policy and cancellation.
- B's `measurement_bytes` counts text additionally owned by a measurement, so a
  snapshot shared with its current model contributes zero. B's `wrapped_bytes`
  counts range-vector capacity; A's value counts wrapped String capacity. Both
  omit outer containers, and A also omits each String header, making the derived
  retention comparison conservative. These values are not RSS or total heap.

Before Release comparison, 25 Job tests pass. `layout-A.json` and `layout-B.json`
are byte-identical across 24 mixed-text/width scenarios; the fingerprint includes
each visual-line boundary as well as content. New tests exercise the actual
production downcast, no UI-row materialization, old-snapshot stability and all
UTF-8 prefix/body split boundaries.

## Result: Job sub-item accepted

All six processes pass the original gates, complete 48 layouts each and exit
cleanly with zero framebuffer errors. Every B layout shares its current model
snapshot. At 16 sources, additional measurement text is zero and wrap ranges
occupy 3,276,800 bytes (3.13 MiB), versus A's combined 34.48 MiB of copied text.

| Median metric | A | B |
| --- | ---: | ---: |
| Peak RSS MiB | 228.72 | 183.59 |
| Lifetime peak RSS MiB | 228.73 | 183.62 |
| Settled RSS MiB | 227.67 | 182.59 |
| Settled growth MiB | -0.92 | -0.52 |
| Peak footprint MiB | 247.31 | 192.27 |
| Final footprint MiB | 233.22 | 178.05 |
| Heartbeat p95 ms | 51.171 | 51.171 |
| Cycle ms | 11233 | 11381 |

Settled RSS decreases by 19.8%. Cycle duration increases by 1.3%, within the
unchanged throughput gate; no throughput improvement is claimed. Measurements
are specific to this workload and must not be added to previous batch savings.

Final validation: 637 library tests pass, 0 fail, 2 ignored; 31 Python tests pass.
Formatting, `cargo check --locked`, UI policy, i18n policy and whitespace checks
pass. The original four structure failures remain: AI controller 3876/3850,
root Slint 3046/3000, root properties 421/410, PR actions 688/580.
Existing unused Hook test import, unused event-page API and compact-unwind
warnings remain. Full doctests (known logger baseline), Windows and long-soak
validation were not run. The first separate visual/manual attempt failed because
the desktop tool did not recognize the raw executable and stalled on the wrapper.
A retry on 2026-09-17 completed the desktop checks described below. All manual
runs remain excluded from A/B.

The starting worktree contained accepted #66/#68/#69 changes; these are preserved.
No commit was made. PR/Chat sub-items and a Job cache byte budget remain open.

## Desktop retry: 2026-09-17

Used the accepted B binary unchanged (SHA256
`bacadd268c9cb8ce84cffb4a298a2b291040d4c1d8cec9bddbad0c8d3970859c`),
an isolated offline state directory, English UI and the actual JobRun fixture.
Started the temporary app explicitly with fixture environment, then attached the
desktop tool to its bundle ID. The original failure was not reproduced after
attachment; its exact underlying cause is not established. AX exposes window
chrome only, so log checks use screenshots and coordinate input.

Verified by desktop input and visual inspection:

- Wheel down from row 1 to row 4, then back to row 1; run and step headers stay fixed.
- Drag the log thumb to the middle (row 267 visible), then to the bottom.
  Scroll slightly upward to expose row 512, `cycle=2 source=15 row=511`;
  the last row's final continuation is reachable at the bottom.
- CJK text and warning styling remain visible; observed wraps have no overlaps
  or repeated source numbers. No missing content or blank viewport was observed.
- Collapse and reopen the log; content returns to the first row normally.
- Native window zoom and restore both reflow the log correctly and retain the header.
- Quit the isolated app normally; process exits with code 0.

Two window-edge drags did not resize the window and are not counted as narrow
desktop validation. This retry does not replace the automated narrow/tall and
en/zh-CN regression matrix or constitute exhaustive manual locale/platform QA.
One thumb drag was temporarily rejected because the approval service disconnected;
after verifying the same isolated fixture, retrying the same action succeeded.

`manual-retry/app.log` retains fixture completion and the task conversation retains
the screenshots. No production source changed and no new performance A/B was
needed; `git diff --check` validates the documentation-only update.

## Reproduction and evidence

`ab/metadata.json` records binary hashes, fixed display/runtime settings and gates;
`ab/summary.json`, six raw logs/results and RSS samples retain the accepted run.
`profile-A/` is the initial diagnostic, not an extra comparison pair.
`baseline-source/src/jobs/` and `candidate-source/src/jobs/` preserve the two Job
implementations; other application code is the accepted #69 baseline. The
test-only fingerprint capture was added after building A, and does not alter A's
Release workload. `layout-reference-tests.rs` preserves that test source.

```sh
python3 scripts/benchmark-p6-terminal.py --batch 70 --seconds 90 \
  --a memory-benchmark-results/p6-70-job-retention-20260916/usher-A \
  --b memory-benchmark-results/p6-70-job-retention-20260916/usher-B \
  --output memory-benchmark-results/p6-70-job-retention-20260916/ab-repeat
```

The output directory must not already exist. Desktop/process sampling permission
is required. Rebuild each source with default features and `cargo build --release
--locked`; `release-fast` is not the measured profile. The evidence directory is
local and Git-ignored.
