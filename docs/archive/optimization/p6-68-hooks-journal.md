# P6 #68: Hook journal and replay bounds

## Predeclared protocol

Baseline: accepted #66 correction on top of #67, plus identical offline Hook
fixture/instrumentation. Candidate: serialized byte accounting and streaming,
cancellable history replay. Default Release, FemtoVG, 1100x700, two Tokio workers,
three fresh pairs AB/BA/AB, 60 seconds each. Existing memory, heartbeat, throughput
and clean-exit gates are unchanged. No compilation/tests during measurement.

Three cycles use the production Journal: 9,000 one-byte output records, then
1,024 1-KiB control-character chunks (JSON escaping), and a terminal record.
Each history is opened twelve times consecutively through the actual bridge.
Completion requires the applied full log to contain the terminal record.
B journals must stay within 2 MiB + 64 KiB. Capture raw journals, app logs,
RSS/CPU samples, footprint, lifetime peak, UI heartbeat and completion time.
This fixture does not measure external Node RSS or interactive manual scrolling.

## Implementation

Output admission counts complete serialized JSONL bytes, including metadata and
escaping. At 2 MiB it emits one truncation marker and continues draining pipes;
64 KiB is reserved for lifecycle completion. Records are capped at 64 KiB.
Legacy replay accepts at most 16 MiB read bytes with an 8 KiB reader and a
64 KiB record buffer; damaged complete records report failure, incomplete tails
are retried later. The events API returns bounded pages (256 records or about
256 KiB plus at most one bounded record); UI replay streams into one log string.
Opening history again or leaving Job Run aborts the previous replay and invalidates
its completion. Replay captures the summary sequence before reading.

Evidence: `memory-benchmark-results/p6-68-hooks-journal-20260916/`.

## Initial invalid run and common correction

The first six processes (`ab/`, original `usher-A/B`) all panicked in
`jobs/log_layout.rs` after the first full history projection. The global crash
records report a reversed byte range, 65499..63. Both sides are invalid; none
of their partial metrics are accepted. Installed Parley 0.10 stores cluster
offsets as u16, and the fixture creates uninterrupted rows longer than 64 KiB.

Both sides receive the same application-owned correction: shape UTF-8 pieces
of at most 32 KiB and carry an unfinished measured line into the next piece.
This preserves source text and uses actual measured extents, not estimated
scroll geometry. Tests cover long ASCII/control/Chinese rows, multiple widths,
and equality against unsplit shaping below the upstream limit. Neither vendor
sources nor the workload/acceptance thresholds change. Rebuilt binaries are
`usher-A-layout` and `usher-B-layout`; corrected evidence is `ab-layout/`.

## Result: accepted

The corrected six-process run is valid and passes every original gate. All
processes completed three cycles, exited cleanly and reported zero framebuffer
errors. Each process applied three full terminal logs. Journals are 8,224,397
bytes on A and 2,091,318 bytes on B, including the explicit truncation marker.

| Median metric | A | B |
| --- | ---: | ---: |
| Peak RSS MiB | 283.19 | 126.67 |
| Lifetime peak RSS MiB | 283.22 | 126.73 |
| Settled RSS MiB | 282.31 | 126.05 |
| Settled growth MiB | -0.33 | -0.33 |
| Peak footprint MiB | 242.75 | 142.95 |
| Final footprint MiB | 224.52 | 131.52 |
| Heartbeat p95 ms | 52.180 | 52.180 |
| Cycle ms | 298 | 297 |

32 Hook tests and the pre-layout full library run (627 passed, 2 ignored) passed.
The added long-row regression passed, followed by the final combined library
suite recorded with #69. Formatting, check, UI/i18n policy and the 28-test script
suite passed. The initial shared panic is preserved as invalid evidence.
This proves the fixed fixture's bounds and memory improvement; it is not a
general application RSS or external Node benchmark. Long soak, Windows and a
fresh manual interaction/resize matrix were not run.
