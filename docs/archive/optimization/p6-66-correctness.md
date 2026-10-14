# P6 #66: narrow highlighting completion correction

This is a separate attempt from the rejected scheduler change in
[p6-66-highlight.md](p6-66-highlight.md). Only the two Highlight completions
change: generation, selected index, path and diff/full-source mode must match
before clearing loading or applying a result. Scheduling and caches are unchanged.
Expand/source-fetch requests are outside this correction.

## Predeclared validation

Use the existing batch-66 production fixture, identical on A and B, Release
default features, FemtoVG, two Tokio workers, 1100x700, isolated offline state.
Three fresh pairs AB/BA/AB, **90 seconds per process**. The original 30-second
experiment sampled active obsolete computation as its first growth interval;
90 seconds places the growth windows at 22.5-45 and 67.5-90 seconds.
Retain all existing memory, heartbeat and completion gates unchanged, including
zero premature loading clears on B. A is accepted #67 binary SHA256
`7bcfb78e5a2d0727813f2c5c0b2baa275207504f09a5e00b7e917be93960365e`.
Evidence: `memory-benchmark-results/p6-66-correctness-20260916/`.

## Result: accepted

624 library tests passed, 2 ignored; the focused real-window regression also
passed in en/zh-CN for stale generation/index/path/mode and session invalidation.
`cargo check --locked`, formatting and whitespace checks passed.
All six Release processes completed and exited cleanly, with no framebuffer
errors. Premature clears: A 71/81/75, B 0/0/0. Summary valid=true, passed=true.

| Median metric | A | B |
| --- | ---: | ---: |
| Peak RSS MiB | 414.84 | 414.25 |
| Settled RSS MiB | 413.45 | 412.92 |
| Settled growth MiB | 0 | 0 |
| Peak footprint MiB | 391.28 | 385.39 |
| Final footprint MiB | 258.60 | 130.19 |
| Heartbeat p95 ms | 52.183 | 52.158 |
| Cycle ms | 2965 | 2957 |

This is a correctness fix, not a demonstrated RSS optimization. A's final
footprint varied substantially (124.78/259.78/258.60 MiB); do not attribute the
median footprint drop to loading writes. The rejected broad scheduler remains
rejected. No long soak, Windows, or new manual visual matrix was run.
