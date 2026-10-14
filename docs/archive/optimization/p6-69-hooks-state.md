# P6 #69: Hook state and projection work

## Predeclared protocol

Baseline is the accepted #68 implementation with identical #69 instrumentation
and workload. Candidate changes configuration sharing, automatic persistence
selection and UI projection preparation. Default Release, FemtoVG, 1100x700,
two Tokio workers, isolated offline data; three fresh pairs AB/BA/AB,
60 seconds each, unchanged memory/heartbeat/throughput and clean-exit gates.

Prepare 32 definitions with 64 KiB scripts and 64 history summaries. Each of
three cycles updates two active runs through the production facade, in bursts,
with 100 snapshots per run and a 32 KiB rolling log; the selected Job Run uses
the actual bridge. Finish both runs and verify the final selected replay.
Record writes, projection preparation and configuration-storage identity.
Output-only phases must perform zero configuration/summary file writes in B;
terminal summaries must survive flush/reload. COW snapshot and parsed-line reuse
are separately verified by unit tests. No external Node RSS claim is made.

Evidence: `memory-benchmark-results/p6-69-hooks-state-20260916/`.

## Candidate boundaries

- Hook definitions and event catalogs use shared immutable snapshots. Mutating
  a definition performs COW; run progress only clones bounded summary metadata.
- The existing automatic persistence worker compares a durable revision before
  serializing. Pure sequence/log changes do not write; failed writes do not
  advance the saved revision; terminal/configuration changes still flush.
- Snapshot coalescing now precedes selected-run preparation. A bounded
  LatestCompute worker prepares only a visible Hook Job; catalog and durable
  selectors avoid repeated project-option and Settings/history projection.
- `hooks/projection.rs` keeps one live-tail parser cache, reuses unchanged rows,
  and eliminates repeated whole-step copies during queued/start/finish assembly.
  Full history replay remains independently bounded; variable-height measurement
  continues through the existing exact Job virtualizer.
- Runtime locale refresh updates the existing Job row text without replacing
  its log model. Interrupted recovery reads complete journal records beyond the
  older saved summary sequence, while live replay retains its captured watermark.

The run benchmark uses the actual facade/bridge with synthetic output snapshots;
it does not claim subprocess throughput. Initial configuration/history setup is
outside cycle latency, but remains included in process peak/footprint metrics.

## Result: accepted

All six processes completed and exited cleanly, with zero framebuffer errors.
Both the original gates and the additional per-cycle terminal replay checks pass.
Every B process shares configuration for all 600 output commits. Per-cycle
output-only writes are A 20, B 0; projection preparations are A 200, B 16-25.
Cycles end at accepted full projection, not a separately measured final paint.

| Median metric | A | B |
| --- | ---: | ---: |
| Peak RSS MiB | 279.81 | 158.45 |
| Lifetime peak RSS MiB | 280.22 | 158.48 |
| Settled RSS MiB | 277.70 | 157.81 |
| Settled growth MiB | 0 | 0 |
| Peak footprint MiB | 291.53 | 171.77 |
| Final footprint MiB | 270.75 | 151.08 |
| Heartbeat p95 ms | 52.183 | 52.187 |
| Cycle ms | 445 | 427 |

Final checks: 633 library tests passed, 2 ignored; 30 Python tests passed;
`cargo fmt --all`, `cargo check --locked`, UI policy, i18n policy and
`git diff --check` passed. Source/zh-CN tests verify Job text refresh while
retaining outer/log model identity. Tests also cover snapshot COW, no-op
configuration actions, output-only disk suppression, terminal shutdown/reload,
incremental parsed-row identity and interrupted replay beyond the saved cursor.

`ab/metadata.json` records binary hashes; A reuses accepted #68 `usher-B-layout`.
`revalidated-batches.json` checks every final replay for all 36 #68/#69 cycles
without altering the original results. The legacy `replay_failures` counter is
3 on #69 because the fixture intentionally opens each run while still active:
those initial previews correctly lack a terminal record. Each completed cycle
is separately required to follow a terminal=true projection; duplicate successes
cannot cover a missing/failed final replay. This stricter check is now in the driver.

Known limits: the architecture gate still reports its original four violations
(AI controller 3876/3850, root Slint 3046/3000, root properties 421/410, PR actions
688/580). The retained event-page API has a non-fatal dead-code warning; the
existing compact-unwind linker warning remains. Full doctests were not run;
the documented logger doctest baseline is not addressed. No new Windows,
long-soak, external Node RSS or manual full interaction matrix was performed.
The starting worktree was clean; changes are scoped to these batches.
