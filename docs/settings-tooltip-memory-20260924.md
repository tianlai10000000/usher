# Settings Hook Tooltip memory investigation (2026-09-24)

The reported screenshots show Home before the first Settings visit (image 2)
and Home after returning from Settings (image 1). The Settings factory stays
mounted after its first visit to preserve Slint factory bindings. In the old
implementation, each of five Hook actions had a full-window positioning host
whose `opacity` was zero while inactive. On Slint 1.18/FemtoVG this could retain
offscreen graphics layers even after leaving Settings. A higher steady RSS after
one visit is therefore expected from that implementation; it does not by itself
show growth on every later visit.

All native checks here used Usher binaries built from this source tree, never
the system-installed application. Each controlled Release comparison used six
fresh processes, isolated offline empty state, Winit/FemtoVG, 800x600 native logical
size at scale 2, and two Tokio workers. Each process visited Settings once,
returned Home, then completed 20 Settings-to-Home cycles. Eight phases had five
RSS/physical-footprint samples each after settling. The route and PID were
verified after native clicks; each valid trace had 42 matched navigations.
Warm Settings latency was callback entry to the first matching AfterRendering,
with the first Settings/Home actions excluded and nearest-rank p95 calculated
per process before taking the three-process median.

## Candidate decisions

The 2026-09-20 content-sized opacity experiment in the historical
`memory-rss-diagnostics.md` section 13.4 remains **rejected**: three Release
pairs reduced settled RSS, but warm Settings p95 rose from 4.815 to 10.974 ms.
The first 2026-09-24 candidate changed the five full-window hosts from opacity
to visibility. Its three Release pairs reduced final Home RSS median from
166.172 to 128.297 MiB, but Settings p95 rose from 18.269 to 20.368 ms
(+11.49%). It was **rejected** under the predeclared +5% latency gate and was
not used as the next baseline.

The second 2026-09-24 candidate uses one permanently instantiated full-window
positioning host, hidden when no Hook tip is active, and five separate fixed
text/measurement surfaces selected by visibility. The active surface's own
measured dimensions feed the shared floating placement. Action identity,
localized labels, leases, the stable factory and vendored Material are unchanged.
The baseline was the original five-host opacity implementation. Both sides had
identical opt-in timing instrumentation. Formal order: A4, C1, C2, A5, A6, C3.

| Measure | Baseline median | Single-host median | Difference |
| --- | ---: | ---: | ---: |
| Final Home RSS | 174.344 MiB | 130.766 MiB | -43.578 MiB (-25.00%) |
| Final Home physical footprint | 152.157 MiB | 98.032 MiB | -54.125 MiB |
| First Settings visit minus initial Home RSS | 47.969 MiB | 2.969 MiB | -45.000 MiB |
| Warm Settings navigation p95 | 16.841 ms | 14.779 ms | -12.24% |

The paired final Home RSS differences were -44.094, -43.609 and -35.000 MiB.
Every process had 40 valid memory samples and 42 matched navigations, zero
compressed memory and a normal exit. Candidate cycle 5/10/15/20 RSS slopes
were 0.0053, 0.0113 and 0.0088 MiB per cycle. One baseline run naturally
lost resident memory late in the sequence; it remains in the comparison.
The single-host candidate passed the predeclared quantitative gates: at least
10 MiB or 5% median RSS benefit, all three pairs improving, no physical
footprint regression and no more than 5% Settings p95 regression.

The source-built native Tooltip visual and interaction matrix is still in
progress. Native V1 verified English Edit/Delete tooltips on two rows, switching
back and forth and reentering Settings after Home. Painted labels, measured
widths and positioning above each button were correct; Home had no lingering
tip. Switching from English to Chinese also repainted both rows' Edit/Delete
labels with new measured widths and centered placement, without stale English
text. Switching back to English restored both labels and their measured widths,
with no Chinese text left on either tooltip. The source-built V1 process exited
normally (code 0); screenshots are in the ignored `runs/V1/` directory,
including `tooltip-{edit,delete}-en-after-zh.png`. Other actions and short/long
labels, viewport edges, scrolling, Dialog actions and keyboard/busy states
remain to be checked before calling the change fully accepted. UI-141's separate
factory row-paint defect remains open. This Release, empty-state, fixed-window
result does not establish other routes, long-term leak freedom or the earlier
400 MiB report. One isolated final-source Debug `cargo run --locked` smoke run
at the same window size measured 145.30 MiB RSS on Home before Settings,
152.70 MiB in Settings and 152.89 MiB after returning Home (+7.59 MiB);
it is not a paired Debug A/B estimate. The temporary timing
instrumentation was removed from production source after the comparison.

The local, Git-ignored raw evidence is in
`memory-benchmark-results/tooltip-visible-20260924/`: `protocol.md` and
`comparison.json` for the rejected five-host candidate;
`single-host-protocol.md`, `comparison-single.json` and `runs/` for the
single-host result. Instrumented baseline and single-host binary SHA256 values
were respectively `7825a6fe3b82fc72910fe714f56003f3220468a65f1d64ce6ff7ad9e23722f82`
and `090900b54dc4cac9905a1838c4d520783a0471c30bd6b533dc19d466abe1a26f`.
