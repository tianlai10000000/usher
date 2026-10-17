# PR Review runtime follow-up — 2026-10-17

This record follows the rejected [B3 compilation split](mainwindow-build-split-20261016.md).
Original A and B3 sources, binaries, failed results and thresholds remain intact.
The current B4/B5 experiments are not an accepted performance baseline. Results
below distinguish intrusive attribution from formal A/B without external native
capture or added frame tracing. Scoped macOS native behavior validation is now
complete; it does not certify the full earlier B3 matrix.

## Fixed controls

- Original A: `/private/tmp/usher-mainwindow-split-20261016/target-A/release/usher`,
  SHA256 `1ee4f31b9290763eb5f699258cf817f841eee27fe38a34764c398075b42cfa4b`.
- Original B3: the same root's `target-B/release/usher`, SHA256
  `f41f06948961be193ab0ef327eba2c3d9886022e5ad49f946829087412434770`.
- New private experiment: `/private/tmp/usher-b3-runtime-20261017` (0700).
  Frozen candidate sources and binaries are separate from its mutable workbench.
- Default features only: `ai-code-review,default,renderer-femtovg`. Release stays
  `opt-level=z`, one codegen unit, full LTO, abort, no incremental, symbol stripping.
- Keep PR RSS strictly below 300 MiB, relative memory 5%/8 MiB, heartbeat 10%/8 ms,
  cycle 20%/100 ms and every existing correctness/shutdown check. Do not purge
  the allocator or replace the original workload with a smaller fixture.
- Batch 66 starts at 800×600@2x and resizes to 1100×700@2x two seconds after the
  first frame; three workload cycles clear the review and return to Project.
  A 90-second batch is not full P6 or a 15-minute soak.
- No cloud workflow, push, Release profile change or workstation-settings write.

## Attribution before changes

The old B3 formal median final footprint was 134.24 → 145.74 MiB (+11.50), while
both arms had roughly 434 MiB RSS. Live malloc was slightly lower in B3, and
internal/external/reusable counters did not explain an extra 11.50 MiB of live
Rust objects. Original A-3 already reached the same higher residual footprint
state. Neither observation changes the original failed verdict.

An intrusive original-A/B3 pair used the exact old binaries and workload, with
`vmmap` at 60 seconds and `footprint` at 65 seconds. It is excluded from formal
acceptance because those tools can suspend or perturb the target.

| 65-second category | Original A | Original B3 | B3−A |
| --- | ---: | ---: | ---: |
| Total footprint | 139,429,112 B | 153,634,088 B | +14,204,976 B |
| IOSurface dirty | 49,479,680 B | 61,849,600 B | +12,369,920 B |
| Malloc Small dirty | 44,171,264 B | 45,875,200 B | +1,703,936 B |
| Owned unmapped graphics | 22,872,064 B | 22,872,064 B | 0 B |

B3's full region detail identifies three AppKit surfaces shared with
WindowServer and two `IOGPUSurfaceMTL` buffer 0/1 surfaces. Each is
2200×1400 BGRA, mapped size 12,369,920 B (11.796875 MiB). A supplemental original-A
process also has exactly these five full-size surfaces. Read-only metadata
regions vary independently; IOSurface region count is not drawable count.
This locates the large difference in native surface accounting, but does not
prove which Rust/Slint object owns AppKit's backing surfaces or establish a
B3-only leak. The pair and supplemental process are n=1 diagnostics, not a new
formal median or a replacement for the failed B3 result.

The common RSS mechanism is separate. Batch 66 submits 33 highlight requests
with 351,001 total line inputs; 9,001 lines belong to the three applied final
results. Previous code rejects superseded results only after complete worker
calculation. Logs show 54–56 threads and roughly 273–280 MiB live malloc after
returning to Project, then about 15 MiB live malloc but about 280 MiB reusable
pages and over 430 MiB RSS. The number of fully executed obsolete lines was not
instrumented; scheduled input counts must not be reported as measured work.

Raw original attribution and exact capture hashes are in
`/private/tmp/usher-b3-runtime-20261017/attribution-original-v2.json`.

## Candidate ownership

B4 changed the native solid canvas only. A layer-hosting NSView with an explicitly
assigned CALayer replaces NSBox painting. It owns opaque CGColor background,
frame updates, disabled implicit actions, native retains, failure cleanup and
main-thread shutdown. It does not replace the renderer or cap/purge its buffers.
Its effect on AppKit surface allocation was measured rather than inferred from
the CALayer API. The intrusive original-A/B4 pair completed cleanly, but both
arms still had three AppKit and two MTL full-window surfaces (61,849,600 B).
It did not establish the proposed backing-store reduction. B4 is rejected;
its source, binary and raw evidence remain frozen. Native code and its added
gates were reverted, restoring the exact original user macOS file SHA256
`ef28417899936c7e877414aea0104cf6b94092f2b0db580206190a776180609b`.
No formal B4 acceptance or small total-footprint difference is claimed as a gain.

B5 contains only a per-controller LatestCompute owner for PR highlighting:
one executing input, one replaceable pending input and at most one completed
output. Request identity and the session lease are captured before scheduling;
obsolete work is rejected before grammar initialization and at each logical
line. Existing final generation/index/path/mode checks, cache boundaries and
ordinary Home retention remain. One Syntect line and resource initialization
are indivisible. Conflicts retains its production non-cancellable wrapper and
its separate existing compute owner.

It starts from the original B3 source with seven explicit source/policy overlays;
it inherits no native B4 changes and never uses B4 as an accepted baseline.
The original user change reserving `92.0 + TRAFFIC_LIGHT_X_OFFSET` in the native
accessory remains verbatim; the pre-change file and patch are saved privately.

## Build provenance and automated regression

The copied candidate cache deliberately reran build scripts whose old absolute
output paths pointed into original target-B. Its first build reused old library
fingerprints; the first confirmation then rebuilt eleven direct dependency
leaves because regenerated build-script timestamps were newer. This attempt
failed the all-fresh confirmation check and remains preserved as `build-B4`.
It is not a compile-speed result. A long wall-clock gap with much less compiler
CPU time is also excluded; its cause was not established.

`build-B4-recovery` reparses the last successful actual Release command, verifies
unchanged source and then requires two additional strictly fresh, zero-Running
confirmations with identical binary bytes. Both passed. B4 is 39,547,552 bytes,
SHA256 `41a050b4824291eb8f946fdb1d41b530125ec5a71d751303b071748988202e33`.
Recovery establishes artifact eligibility only, not runtime adoption.

B5's default Release build and two fresh confirmations passed without recovery.
Only root library/binary compilation ran; no UI package rebuilt. The first
invocation took 219.11 s, followed by 0.881/0.492 s fresh controls. These are
artifact-provenance timings, not a controlled compile-speed gain. Its binary is
39,547,488 B, SHA256
`cd6e459b19e3133bc6be6a1c54acc4e44e18f840eb59871723d1f8af57e6d3e9`;
source aggregate `dbfe37467cfbb43e933b479121bbecc17138f2a0f90aad64271d35d81e1413ed`.
It retains the original Release policy and exactly the seven declared overlays.

Completed automated checks include formatting (`--check`), default
`cargo check --locked --offline`, four new focused highlight tests and the actual
architecture/UI policy gates, all with exit 0. The retained worker-dependency
and extracted-owner-size positive/negative policy groups passed (15.92 s).
The earlier four-group experiment included the now-reverted native gate and
must not be counted as four current policy groups. Seven `test-fixtures`
integration groups have **48 passed, 0 failed**. The latest complete root library regression
has **970 passed, 0 failed, 9 ignored (41.39 s)**; root doctests have **1 passed,
0 failed, 1 ignored**. Initial fixture integer-type and production-wrapper cfg
mistakes were corrected, with failed logs retained. Private diagnostic, build,
protocol, recovery and final-audit tools have positive/negative self-tests;
the final runtime audit adds seven controls, including an exact-300 MiB failure
and preservation of baseline/global strict errors.

## Completed controlled Release A/B

The unchanged batch-66 driver completed three fresh AB/BA/AB pairs at 90 seconds
per process: **exit 0, `valid=true`, `passed=true`**. Both arms used the same
cleaned environment, built-in memory diagnostics, offline fixture, two Tokio
workers and default FemtoVG/OpenGL Release policy. No external `vmmap`/`footprint`
capture or extra BeforeRendering trace ran in this formal cohort.

| Run / order | Peak RSS (MiB) | Settled RSS (MiB) | Final footprint (MiB) | Heartbeat p95 (ms) | Median cycle (ms) | Shutdown (ms) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| [A-1 / AB](/private/tmp/usher-b3-runtime-20261017/formal-B5-v1/A-1/result.json) | 439.859375 | 438.578125 | 147.89 | 51.213 | 2717 | 130.627 |
| [B5-1 / AB](/private/tmp/usher-b3-runtime-20261017/formal-B5-v1/B-1/result.json) | 166.718750 | 166.234375 | 131.78 | 51.232 | 1104 | 195.281 |
| [B5-2 / BA](/private/tmp/usher-b3-runtime-20261017/formal-B5-v1/B-2/result.json) | 170.531250 | 170.031250 | 129.61 | 52.211 | 1073 | 181.757 |
| [A-2 / BA](/private/tmp/usher-b3-runtime-20261017/formal-B5-v1/A-2/result.json) | 437.125000 | 435.843750 | 146.16 | 51.189 | 2857 | 184.641 |
| [A-3 / AB](/private/tmp/usher-b3-runtime-20261017/formal-B5-v1/A-3/result.json) | 439.359375 | 437.812500 | 148.86 | 51.206 | 2906 | 184.156 |
| [B5-3 / AB](/private/tmp/usher-b3-runtime-20261017/formal-B5-v1/B-3/result.json) | 167.281250 | 166.718750 | 132.75 | 51.232 | 1059 | 189.145 |

The unchanged relative gates compare per-arm medians:

| Median metric | Original A | B5 | B5−A | Allowed increase | Relative gate |
| --- | ---: | ---: | ---: | ---: | --- |
| Peak RSS (MiB) | 439.359375 | 167.281250 | −272.078125 | 21.967969 | Pass |
| Lifetime peak RSS (MiB) | 439.73 | 167.52 | −272.21 | 21.9865 | Pass |
| Settled RSS (MiB) | 437.812500 | 166.718750 | −271.093750 | 21.890625 | Pass |
| Peak footprint (MiB) | 422.80 | 171.28 | −251.52 | 21.14 | Pass |
| Final footprint (MiB) | 147.89 | 131.78 | −16.11 | 8.00 | Pass |
| RSS growth (MiB) | 0 | 0 | 0 | B5 and delta each ≤8 | Pass |
| Heartbeat p95 (ms) | 51.206 | 51.232 | +0.026 | 8.000 | Pass |
| Cycle duration (ms) | 2857 | 1073 | −1784 | 571.4 | Pass |

Peak RSS falls by 61.93% and median cycle duration by 62.44% in this fixture.
The input workload was not reduced: every run retained the eight-file,
87,000-line base fixture and the same 33 scheduled inputs/351,001 input lines;
the final Diff/full-file/Diff results still pass the existing identity, mode and
line-count completion guards. B5 cancels obsolete calculations instead of fully
executing them. It neither purges the allocator nor changes Release parameters.
Scheduled input lines are not a measurement of how many cancelled lines executed.

### Separate strict RSS result

The unchanged strict checker exited **1**, with **`passed=false` and 12 errors,
all belonging to original A**. For each A run, peak, lifetime peak, settled RSS
and raw RSS sampling violate the existing strictly-below-300 MiB contract.
There are **zero B5 errors and zero global/protocol errors**. This is a failed
joint strict result with an unchanged negative control; it is not a waived gate.
The generic 512 MiB P6 ceiling does not replace the PR requirement.

B5's **2,329 raw RSS samples** (773/778/778) all satisfy `0 < RSS < 300 MiB`.
Its per-run lifetime peaks are 167.05/170.77/167.52 MiB, and all peak/settled
metrics also pass the same strict limit. Original A has 713/710/709 raw samples
at or above 300 MiB. Preserve those failures and the original B3 +11.50 MiB
footprint failure; neither cohort is rewritten by the new candidate result.

All six runs completed exactly three ordered cycles, exited cleanly and had zero
framebuffer errors or premature loading clears. Shutdown was 130.627–195.281 ms,
below eight seconds. Median peak resource counts were threads **55 → 29**, Mach
ports **434 → 353**, VM regions **709 → 491**. Process CPU p95 medians were
928.9% → 32.7% (the `ps` metric includes multiple cores). Resource counts and
90-second slopes are observations, not a completed soak acceptance matrix.

The last UI-model observation is Project with all PR model counts zero after
explicit fixture cleanup. Settled RSS/final footprint therefore mainly describe
post-cleanup Project, not normal Home hiding with retained review caches.
Heartbeat records 504–514 approximately-50 ms timer intervals per run over the
active fixture window; it is not input-to-paint latency or 90 seconds of continuous
heartbeat. This batch is **not full P6 or a 15-minute soak**.

### Evidence and provenance

- [Formal summary](/private/tmp/usher-b3-runtime-20261017/formal-B5-v1/summary.json)
  retains driver `valid=true/passed=true`; each run keeps its raw log and samples.
- [Unchanged strict result](/private/tmp/usher-b3-runtime-20261017/formal-B5-strict-result.json)
  retains `passed=false` and all twelve original-A errors.
- [Independent final audit](/private/tmp/usher-b3-runtime-20261017/final-B5-runtime-audit.json),
  SHA256 `bfc2357aa49a161b9d36630f3ec6d98ac48c40553bbe3b1f8bf7b890e3feffd5`,
  recomputes all six raw sample/log metrics, the driver gates and the unchanged
  strict checker. It joins exact binary hashes, all B5 before/after source
  manifests, the frozen 1,002-file B5 source, original A's full source/manifest
  and the unchanged failed B3 summary.
- [Rejected B4 audit](/private/tmp/usher-b3-runtime-20261017/attribution-B4-rejected.json),
  SHA256 `e167a7bc543f5a83fb741bc1b4a38b90fccd17b8f207b23c09fef405d7d834ba`,
  preserves native surface and BeforeRendering evidence without a formal B4 verdict.

The formal result supports B5's candidate RSS and relative runtime criteria in
this scoped fixture. It does not establish an accepted whole-batch performance
baseline or ownership of AppKit/NSBox buffers. Native behavior evidence has a
separate scope below.
The original 11.796875 MiB IOSurface diagnostic difference remains an accounting
observation also reachable by original A, rather than proof of a removed B3 FBO.

The [durable raw-evidence manifest](../../validation/pr-review-runtime-20261017/manifest.json)
and [hash verification](../../validation/pr-review-runtime-20261017/verification.json)
retain the repository copy of the captured evidence.

## Scoped native regression

The current-root debug build with `test-fixtures` completed with exit 0. Its
normal MainWindow payload has SHA256
`2b34bf192e0915267738db6505f7e6ddc4e0e1b818dc3c6b61b7aa1f657c35b8`.
Every launch is isolated and offline, with private settings/state and a unique
instance namespace. Only the EN bundle was launched; Chinese was checked by
switching locale in that same process. The prepared ZH bundle was not launched.
This functional fixture is `large-ranges-highlighted-v1` with **206 files**,
separate from the formal batch-66 eight-file fixture.

| Native case | Observed result |
| --- | --- |
| EN Home/Back into PR | File 000 restored in the normal PR workspace. |
| EN rapid file 001 → 002 → 003 → 004 | Selected file 004, content `00:004` and ordinal `5/206` agree; no loading spinner observed. |
| EN cached 000 → 004, then Home/Back | File 004 and matching content remain selected. |
| Settings runtime `en → zh-CN`, then Back | Settings/sidebar/history labels update immediately; PR date `8月10日`, status `开启`, `全部 Commits` and `筛选文件` update, with file 004 retained. |
| Chinese rapid 003 → 002 → 001 | Selected file 001, content `00:001` and ordinal `2/206` agree; no stale replacement or spinner observed. |
| Chinese scrolling | Rows 7/8 paint the matching file 001 content. |
| Chinese Home/Back | File 001 is retained without stuck loading or a stale overwrite. |

These pointer, content, locale and reentry observations pass the scoped native
checks. They do not establish every real-code syntax colour or exhaust all
pressure/race interleavings. The four new unit tests separately check completion,
cancellation and token payloads, including Chinese text.

The first CUA `getApp` took **418.697 s** despite its requested timeout; this is
tool time, excluded from application/performance measurements. The app's own
single-run debug Startup Timeline records its first frame at **465 ms** from
the internal origin. That observation is not a controlled startup comparison.

Native close hides the window. Cleanup therefore matched PID **72020**, kernel
birth and the exact payload argv before sending SIGTERM. The log records signal
15, a five-millisecond cleanup pipeline and MainWindow/native-resource release;
the final process check confirms the process exited. Cleanup selected the test
application by exact process identity; workstation settings were untouched.

- [Native observations](/private/tmp/usher-b3-runtime-20261017/native-functional-observations.json)
  retains the CUA cases and their limits; AX/screenshots were observed live in the
  conversation, not fabricated as screenshot files.
- [Native build/source proof](/private/tmp/usher-b3-runtime-20261017/native-build-proof.json)
  and [launch metadata](/private/tmp/usher-b3-runtime-20261017/native-functional-v1/runs/en-b1zfze_w/launch.json)
  identify the payload, source and isolated fixture.
- [Native cleanup](/private/tmp/usher-b3-runtime-20261017/native-cleanup-result.json)
  retains the exact process/cleanup checks.

Physical Cmd-F remains deferred at the user's request. This follow-up did not
rerun the entire old B3 native matrix, full P6, a 15-minute soak, or Windows/Linux
validation. The new 206-file native fixture is functional evidence and must not
be substituted for the eight-file formal performance workload.

## Final decision and adoption boundary

Retain the B5 bounded-highlight fix candidate: its scoped candidate runtime
criteria and the native functional checks above pass, with the source unchanged.
B4 remains rejected. The unchanged joint strict result still fails with twelve
original-A errors; do not change that checker, swap out A, waive the RSS threshold
or promote the whole compilation-split batch to a new accepted baseline.
Original A and all old B3 failures remain preserved. Scoped B5 success does not
retroactively turn the historical B3 +11.50 MiB result green.

正式原 A/B5 三对 AB/BA/AB 及本轮限定 macOS 原生功能回归已完成。B5 的 2,329 个
RSS 原始采样及 peak/lifetime/settled 指标均严格低于 300 MiB，相对门槛通过；
英文快速选文件、缓存与 Home/Back、同进程实时切换中文、中文快速选择/滚动/重入
观察通过。206 文件原生夹具仅作功能观察，不替代正式八文件性能夹具或宣称全面压力
race/语法配色覆盖。联合 strict 仍 `passed=false`，12 项全来自原 A，不能改 checker、
更换对照或晋升整批基线。保留 B5 修复候选，B4 拒绝；原 B3 失败表和旧 A 证据不变。
物理 Cmd-F 仍按用户要求延期，旧完整原生矩阵、完整 P6/15 分钟 soak、Windows/Linux
均未在本轮重跑。
