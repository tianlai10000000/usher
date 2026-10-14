# Rejected Memory Experiments

[中文](memory-experiments.zh.md) · [Current backlog](../memory_optimization_backlog.md)

These dated MEM-14–16 records were moved from the memory backlog during the
2026-10-14 documentation cleanup. They preserve the original experiment
conditions, failures, measurements, decisions, and evidence locations. Statements
about experimental infrastructure describe the recorded batch, not a new
production policy. Current tasks and adoption gates remain in the backlog.

<a id="mem-14"></a>

### MEM-14 — Rebuild the inactive Messages visual tree

Status: Candidate rejected (2026-08-14)

The candidate kept the independently generated Messages factory root stable but
mounted its ScrollView and tooltip host only while `Route.Messages` was active.
The baseline kept those visual nodes mounted. Both variants used the same
existing lifecycle bridge: the projected row model is cleared on exit,
rehydrated before re-entry, and the Store subscription plus off-route Banner and
unread notification surfaces remain active.

The controlled A/B used the current persisted eight-row MessageCenter snapshot,
Release/FemtoVG, an 800×600 logical window at scale factor 2.00, two
Messages enter/leave cycles, five samples per phase, and three isolated processes
per variant interleaved by run. All six logs verified
`Home → Messages → Home → Messages → Home`, two inactive lifecycle events with
zero projected rows, and one second-entry hydration with eight rows.

| Metric | Stable visual baseline | Active-only candidate | Candidate − baseline |
| --- | ---: | ---: | ---: |
| Cycle-1 post-leave settled RSS | 120.41 MiB | 121.23 MiB | +0.83 MiB (+0.7%) |
| Cycle-2 post-leave settled RSS | 120.77 MiB | 121.47 MiB | +0.70 MiB (+0.6%) |
| Cycle-2 RSS above startup | 9.88 MiB | 9.50 MiB | -0.38 MiB (-3.8%) |
| Cycle-2 post-leave settled footprint | 112.56 MiB | 119.58 MiB | +7.02 MiB (+6.2%) |
| Cycle-2 footprint above startup | 32.45 MiB | 38.56 MiB | +6.11 MiB (+18.8%) |
| Overall sampled RSS peak | 120.94 MiB | 121.64 MiB | +0.70 MiB (+0.6%) |
| Overall sampled footprint peak | 112.84 MiB | 120.58 MiB | +7.73 MiB (+6.9%) |

Decision: **reject the active-only visual tree**. Its normalized RSS direction was
slightly favorable but far below the default 10 MiB/5% threshold, absolute RSS
was higher in all three paired cycle-2 post-leave runs, and physical-footprint
results were mixed with two material regressions. Production retains the stable
factory visual tree and the already-established off-route row-model release.
Lifecycle diagnostics remain because they directly verify projection ownership.

Evidence is under
`memory-benchmark-results/messages-ui107-ab-20260814/` (Git-ignored). The frozen
baseline/candidate SHA-256 values are recorded in `metadata.tsv`; raw samples,
per-run summaries, exact logs, and the generated comparison are retained beside
it. The benchmark ran from a dirty worktree, with unrelated Preview, Chat, and
macOS platform edits preserved.

<a id="mem-15"></a>

### MEM-15 — Unmount AI Review response delegates behind Docked Preview

Status: Candidate rejected (2026-08-27)

The candidate kept the AI Review panel shell, composer, elapsed timers, source
models, and interaction state mounted, but conditionally destroyed the formatted
response/finding/conversation delegate subtree while a Docked Preview occupied
the same PR-side surface. One fixed Release/FemtoVG binary exposed
`USHER_AI_REVIEW_DOCKED_LIFECYCLE=retain|unmount`; both cells therefore had the
same SHA-256 (`be5d5c0d2632469f404c997a5ba5a46b2c74ce54b1bf76a9e3828fe196e6bb99`)
and differed only in the explicit lifecycle policy. Production defaults to
`retain`.

The representative fixture used 12 assistant responses of 4,096 bytes each,
24 response rows, 576 blocks, 1,132 spans, 49 repository actions, and a Docked
79-block Preview. It ran five isolated processes per variant. Diagnostics and
UI tests confirmed that the source model remained at 24 rows/576 blocks while
the candidate's mounted counts fell to zero, then remounted with conversation,
composer mode, input text, and scroll offset preserved.

| Representative metric | Retain baseline | Unmount candidate | Candidate − baseline |
| --- | ---: | ---: | ---: |
| External Preview-settled RSS | 140.23 MiB | 139.52 MiB | -0.72 MiB (-0.5%) |
| External Preview-settled footprint | 112.86 MiB | 106.52 MiB | -6.34 MiB (-5.6%) |
| In-process response → Preview RSS | +11.32 MiB | +11.51 MiB | **+0.19 MiB** |
| In-process response → Preview footprint | +11.59 MiB | +12.99 MiB | **+1.40 MiB** |
| In-process response → Preview malloc in-use | +0.93 MiB | -5.23 MiB | **-6.16 MiB** |
| In-process response → Preview malloc blocks | +6,305 | -95,368 | **-101,673** |

The ownership result was real but did not become a representative process-RSS
reduction. A 64-response × 8,192-byte stress fixture amplified the candidate to
128 rows and 1,568 blocks: its in-process response-to-Preview delta improved RSS
by 3.72 MiB, footprint by 14.94 MiB, and malloc in-use by 17.41 MiB. Even this
stress-only RSS result was just 2.1% externally (`159.69 → 156.36 MiB`) and did
not pass the 10 MiB/5% adoption threshold. The three-process Detached negative
control kept delegates mounted and changed settled RSS/footprint by only
`+0.19/+0.28 MiB`. A two-open/close-cycle cell found no accumulating candidate
slope, but also no stable RSS gain.

Decision: **reject delegate unmounting as a production optimization**. The
representative in-process RSS and footprint both regressed slightly, while the
absolute footprint result varied between separately launched batches. The
substantial live-malloc reduction therefore cannot be reported as an answer to
the observed 300+ MiB RSS. Normal runs retain the pre-batch lifecycle; the
explicit `unmount` policy, diagnostics, fixtures, and tests remain only as
reproducible experimental infrastructure. A future batch should test visible-
window virtualization of the response delegates, which reduces live UI work
without relying on allocator page return after whole-tree destruction.

All 38 process logs, raw samples, metadata, generated comparisons, and the fixed
27,869,008-byte binary are under
`memory-benchmark-results/ai-review-docked-unmount-ab/20260827T064258Z-step2-response-delegate-unmount/`
(Git-ignored).

<a id="mem-16"></a>

### MEM-16 — Virtualize the visible AI Review response window

Status: Candidate rejected as the production default (2026-08-27)

The candidate flattened review state, findings, conversation messages, and chat
status into one `ListView` row model. Each visible assistant row still reused
the production `GitHubAgentResponse`; source conversation/block models remained
unchanged and were shared by handle rather than copied. One fixed
Release/FemtoVG binary selected `USHER_AI_REVIEW_RESPONSE_LIST=retain|virtualized`
and explicitly held the rejected MEM-15 lifecycle at `retain`. Its SHA-256 was
`bdb15f0efbd18632f8ef5f397c2e1f735eb0e7283b5b1982a0a32c624256f629`.

The representative 12x4,096-byte fixture ran five isolated processes per
variant after the same 1280x860 balanced-scroll preparation. Retain created all
24 message delegates and 576 block delegates. Virtualized preserved those 24
source rows/576 blocks plus a 27-row shared visual projection, while creating a
four-row delegate pool containing two message rows and 48 block references.
The settled measurements include the post-scroll high-water state.

| Representative metric | Retain | Virtualized | Candidate - baseline |
| --- | ---: | ---: | ---: |
| Settled RSS | 131.64 MiB | 126.78 MiB | -4.86 MiB (-3.7%) |
| Settled physical footprint | 151.53 MiB | 144.27 MiB | -7.27 MiB (-4.8%) |
| Fixture-checkpoint malloc in-use | 30.80 MiB | 25.21 MiB | -5.59 MiB |
| Fixture-checkpoint malloc blocks | 182,080 | 89,358 | -92,722 |

The 64x8,192-byte stress fixture used three processes per variant. It retained
128 source rows and 1,568 blocks; the candidate again created only four visual
delegates and two message delegates at the initial checkpoint. Settled RSS fell
from 149.12 to 131.08 MiB (`-18.05 MiB/-12.1%`) and footprint from 166.22 to
147.77 MiB (`-18.45 MiB/-11.1%`). Median checkpoint malloc in-use fell by
17.21 MiB and live block count by 286,659. Both two-cycle slopes were zero.

A three-process Home cell with no AI content measured the fixed-policy overhead:
settled RSS was `+0.53 MiB/+0.5%`, footprint was `-0.08 MiB/-0.1%`, and both
variants had the same `-0.33 MiB` cycle slope. This is launch noise rather than
evidence of feature-owned allocation because no AI panel or response model was
mounted.

Decision: **keep production `retain`**. Visible-window virtualization materially
helps the stress shape and proves that full delegate mounting owns live memory,
but the representative RSS saving does not pass the predeclared 10 MiB/5%
adoption threshold. Unset or invalid policy values therefore remain `retain`;
`virtualized`, its diagnostics, and its interaction tests remain explicit
experimental infrastructure. The candidate must not become the next production
baseline without a representative fixture that clears the gate and a manual
visual trial.

Raw samples, all 22 formal process logs, comparison tables, the fixed binary,
and 10 excluded pilot process logs are under
`memory-benchmark-results/ai-review-response-list-ab/20260827T105430Z-step3-visible-window-virtualization/`
(Git-ignored). The pilot cells are excluded because their archived binary path
was intentionally labelled `non-release-override` and their application-variant
metadata did not identify a candidate dimension.

