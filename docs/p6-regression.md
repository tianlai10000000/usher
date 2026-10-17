# P6 regression protocol

The on-demand entry point is:

```sh
cargo build --release --locked
python3 scripts/benchmark-p6-regression.py --binary target/release/usher --output memory-benchmark-results/p6-local
```

It runs all six fixtures (#65-#70) in three fresh AB/BA/AB pairs, then actual
15-minute continuous Terminal, Hook-state and Job-layout workloads. Optional
`--baseline PATH` compares two current implementations; without it, both sides
have the same SHA-256 and measure repeatability. The hosted workflow remains
manual-only. Run on an unlocked macOS desktop with consistent display/renderer,
power and window conditions; do not compile or run unit tests during measurement.
Other operating systems have not been validated by this protocol.

`--baseline` applies to the paired comparisons; sustained runs use the candidate
`--binary` only. The default sustained selection remains `65 69 70`. For a PR
highlighting change, `--soak-batches 65 66 69 70` additionally runs the existing
PR fixture continuously for the same default 900 seconds; it does not create an
A soak or alter the defaults, workload or thresholds.

`benchmark-p6-terminal.py --mode migration` preserves historical old/new tests.
`--mode regression` checks both sides against today's correctness invariants;
it does not require old-A Hook snapshot copies, output-only writes, or copied
Job measurement text. Historical migration results are not ongoing regression
baselines. Use a new output directory for every invocation; failures are retained.

Each run uses isolated state, offline fixtures, two Tokio workers, default
Release and FemtoVG at 1100x700 logical pixels. Pair gates retain the established
5%/8 MiB memory, 10%/8 ms heartbeat and 20%/100 ms cycle regression allowances.
Absolute regression ceilings are 512 MiB RSS/footprint, 100 ms heartbeat p95,
8 MiB RSS growth and shutdown under 8 seconds. These are conservative detection
ceilings, not claims about idle product memory usage.
The generic 512 MiB ceiling does not replace a stricter feature contract:
[PR Review](specs/pr-review.md) requires RSS strictly below 300 MiB. That RSS
ceiling and all growth/slope gates apply independently; passing one does not
waive another.

Continuous mode (`--soak`) repeats the actual workload until its requested
duration, finishing on a complete cycle boundary. It must complete at least
three ordered cycles, record progress for every cycle and never leave over
60 seconds between them. The driver grants at most 60 seconds for the final
cycle. Heartbeats continue for the workload lifetime. Terminal repeats same-
project reopen and A/B/A replacement; PR repeats diff/full-file/stale-selection
paths; Hooks repeat journal replay or concurrent output with deep retained
history; Job repeats all sixteen source layouts. Missing correctness markers,
timeouts, early exits and forced kills fail even when memory looks healthy.

After the first 25% warmup, least-squares RSS and footprint slopes must not
exceed 0.5 MiB/min. Diagnostic resource counts compare the first/last quartiles
of post-warmup samples: threads +4, Mach ports +32, VM regions +256. Raw logs,
100 ms RSS/CPU samples, per-run results, hashes, protocol and aggregate outcomes
remain in the output. SIGINT/SIGTERM stop the active process, preserve available
evidence and produce a failing cancellation result; failures do not silently
skip remaining independent gates. OS notifications already delivered cannot
be used as evidence of a mounted UI refresh.

This tooling batch makes no claim of runtime savings. Its A/B is the old/new
checker on positive and negative fixtures, supplemented by Release execution
of the real fixtures. Individual performance changes still need a separate
baseline/candidate comparison and must not adopt a rejected candidate.
