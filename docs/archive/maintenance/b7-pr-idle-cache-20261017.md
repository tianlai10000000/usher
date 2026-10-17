# B7 PR idle resource lifecycle — 2026-10-17

Status: **B7 is not adopted; complete regression failed the Hook footprint
slope gate.** The candidate passes the PR-specific paired and sustained gates.
Original A remains the formal control. Rejected B5 and B6 results remain intact.

## Reason and scope

[B6](b6-pr-soak-20261017.md) demonstrated that repeated builtin syntax-graph
reinitialization contributes to the PR workload's allocator-residency growth.
Keeping that graph for the controller's entire lifetime passed the individual
900-second PR criteria but failed original-A paired final footprint: 89.14
versus 80.41 MiB, a new 8.73 MiB cost above the unchanged 8 MiB allowance.
B6 was rejected and its full P6 was not run.

B7 coalesces idle confirmation onto the next event-loop turn. A normal review
declares interest before asynchronous metadata/provider work; a highlight
request captures its own immutable cache-slot lease before enqueueing. A true
close retires an unclaimed slot and releases it on the existing shared blocking
pool. One coalesced weak UI check and one release-drain worker avoid accumulating
cleanup jobs. Session cancellation, typed generations and completion identity
remain separate authorities.

This is an event-loop-operation policy, not a timed human-reopen grace. The
single current graph's lazy grammar/regex caches are not a byte bound, and old
admitted work may temporarily retain its independent retired slot. Ordinary
Home hiding, commit-tab Back checkpoints, remote mutations and final CPU-only
controller teardown keep their existing lifecycle. Full-source preparation
is unchanged.

## Fixed controls and required validation

Release remains opt-level `z`, one codegen unit, full LTO, panic abort, symbol
strip and incremental off, with the default AI/FemtoVG features. Original A,
all prior failures, workloads and every threshold are preserved. All 46 new
native runs used 2× display scaling and 1100×700 logical fixture geometry;
the earlier 1× evidence remains a separate cohort. No allocator purge or
workload reduction is used.

The first native check compared rejected B6 with B7 in three fresh 90-second
pairs solely to test the closed-footprint intervention. It passed without
promoting B6 to an accepted baseline. Formal regression then used original A/B7:
six fixtures in three fresh AB/BA/AB pairs plus four candidate-only 900-second
soaks, including PR #66. All 40 full-protocol runs completed. This is 114 minutes
of requested full-protocol workload before overhead, in addition to the nine
minutes of causal paired work.

## Exact build and preservation

The isolated build changed exactly 11 allowed paths against the frozen rejected
B6 source reference. Three complete owners now separate session slots, idle
cache state and bounded highlighting, with independent size/dependency gates.
The new async UI test owner has a 220-line limit. Existing budgets were not
increased. No Slint/vendor/Release/dependency file changed in this batch;
715 unrelated initial source/script/config files retain their hashes.
The user's `src/platform/macos.rs` remains unchanged, SHA-256
`ef28417899936c7e877414aea0104cf6b94092f2b0db580206190a776180609b`.

`cargo build --locked --offline --timings --message-format=json -vv --release`
completed in 214.137 seconds; two fresh confirmations took 0.911 and 0.464
seconds, all exit 0 with identical executable hashes. Actual root library and
binary compiler commands confirm the fixed Release parameters. These are
provenance timings, not a compilation-speed A/B claim.

The frozen B7 executable is 39,547,536 bytes, mode `0500`, SHA-256
`d157e95f51a27c23596f734603c0d7a5df44bc729e3f14763f9f5dd6b2a811d1`.
The build summary SHA-256 is
`d909bb1f0ed93c1055d80f167650abc7d868e6c9deab3ee90e50f859e12dcad0`.
The frozen source has 1005 files; two generated catalogs are preserved
separately. Original A and all B5/B6 frozen artifacts, source and rejection
records were verified before and after the build. Only the designated mutable
workbench/target were reused.

## Self-tests completed before native measurement

| Check | Observed result |
| --- | --- |
| `cargo fmt --all`; final `--check` | exit 0 |
| `cargo check --locked --offline` | exit 0 |
| PR library filter | 135 passed, 1 ignored |
| Final `cargo test --lib --locked --offline` | 982 passed, 10 ignored, no failures |
| `cargo test --doc --locked --offline` | 1 passed, 1 ignored; logger example passed |
| Six PR UI integration targets | 46 passed, no failures |
| Architecture gate and UI policy | exit 0 |
| Architecture positive/negative fixtures | 46 existing tests passed; new independent async-test-owner size control passed |
| `git diff --check` | exit 0 |

The new blocking parent test runs six dedicated child processes: English and
simplified Chinese, each with actual review open, bridge force Reload and close.
Its ignored child entry point is executed explicitly by that parent, with a
bounded timeout and process reaping. These controls use the real event-loop
queue and shared asynchronous metadata-error chain, with no registered Project
Store, workstation configuration or HTTP. Open/Reload preserve the warm graph
through the first queued check; a current configuration error or explicit close
then retires its slot and the shared worker releases the graph. Semantic error
copy matches the active locale.

Pure cache/worker tests additionally cover reset/claim/reset coalescing, old-slot
isolation, stale error tickets, pending/completed cancellation and a blocked
destructor draining a second confirmed slot on the same worker. These tests do
not establish native RSS/footprint acceptance or successful delayed Provider
responses. The old physical-keyboard Cmd-F check remains deferred.

## Controlled native results

Every series used isolated offline state, two Tokio workers, default Release,
FemtoVG and English. No compiler, unit test, intrusive profiler or archive
compression ran during native measurement. The preparation awake guard was
reaped before measurement; each supervisor reaped its suite and awake guard
without a cleanup error or forced app kill.

| Three causal PR pairs, same 2× cohort | Rejected B6 | B7 |
| --- | ---: | ---: |
| Peak RSS median, MiB | 171.093750 | 164.812500 |
| Settled RSS median, MiB | 170.359375 | 163.296875 |
| Final footprint median, MiB | 155.20 | 118.28 |
| Heartbeat p95 median, ms | 51.241 | 51.243 |
| Cycle-duration median, ms | 1101 | 1112 |

All six generic/strict PR runs and relative checks pass. The canonical paired
driver and raw auditor both exit 0. This establishes the idle-retirement effect
in these windows; B6 remains a rejected causal reference, not an accepted baseline.

| Formal original-A/B7 PR paired medians, same 2× cohort | Original A | B7 |
| --- | ---: | ---: |
| Peak RSS, MiB | 433.406250 | 164.937500 |
| Settled RSS, MiB | 431.921875 | 164.343750 |
| Final footprint, MiB | 143.53 | 129.99 |
| Heartbeat p95, ms | 51.214 | 52.209 |
| Cycle duration, ms | 2909 | 1100 |

The final-footprint change is -13.54 MiB, passing the unchanged relative gate.
Actual 70–80-second periodic closed-window observations also remain in raw
evidence: A per-run footprint medians are `143.53 / 135.52 / 145.42` MiB and B7
are `129.83 / 130.28 / 115.49` MiB. These are finite observed windows, not an
unlimited plateau or invented post-process-exit memory. The soak's final
boundary checkpoint remains a different observation.

All six paired fixture summaries (#65–#70, 36 fresh runs) pass generic
correctness and relative gates. The four actual candidate-only soaks are:

| 900-second workload | Cycles | Peak RSS, MiB | RSS growth, MiB | RSS slope, MiB/min | Footprint slope, MiB/min | Result |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| Terminal #65 | 1112 | 148.546875 | 0.375000 | 0.045688 | 0.029578 | pass |
| PR #66 | 572 | 170.265625 | 1.453125 | 0.183433 | 0.272095 | pass |
| Hook #69 | 2077 | 278.500000 | -10.906250 | -0.292113 | **0.979011** | **fail: footprint slope > 0.5** |
| Job #70 | 80 | 170.921875 | 0.437500 | 0.052785 | 0.178810 | pass |

PR loaded syntax resources exactly once during its 572 cycles. Its lifetime RSS
peak is 173.33 MiB, heartbeat p95 52.169 ms, shutdown 194.742 ms; no framebuffer
error occurred. Raw auditing confirms **all candidate PR criteria**, including
strict RSS below 300 MiB, growth/slopes, resources and paired relative checks.
The unchanged strict PR partition still contains **12 original-A errors, zero
B7 errors and zero global strict errors**. These historical A-only failures
remain joint failures and do not erase the separate new Hook failure.

The full canonical driver and actual raw-auditor CLI both exit **1**. The auditor
confirms integrity and completion of all 40 runs, without pending cases or
integrity errors; the only generic failed case is Hook #69 footprint slope.
The raw-audit SHA-256 is
`2ea5cdf53d47e15a01f8a5afd782d0b1edebd06f6cd704a3f2208f070acc67ae`.
The canonical driver checks actual `Popen.returncode == 0` for each application;
separate integer app-OS-exit records unavailable in this suite are not
reconstructed. Supervisor integer exits and reaping records are preserved.

## Remaining Hook blocker and handoff decision

The Hook failure is confirmed under the unchanged checker. Its post-warmup set
contains 139 footprint points: 136 periodic plus three lifecycle/closing points.
The periodic-only slope is 1.396328 MiB/min, so closing points lowered rather
than manufactured the canonical 0.979011 slope. This diagnostic subset does
not replace the canonical result or make the gate pass.

Hook #69 did not load syntax resources. Thirteen Hook/Job/sampling source paths
have identical hashes in frozen B5/B6/B7; the new release helper's sole production
caller is PR lifecycle. Those facts do not establish that B7 is unrelated:
shared binary layout/allocator effects and the common Hook path still need
controlled runtime evidence. The fixture retained 4154 Hook journal files,
141,435,392 bytes, matching two new paths per cycle despite bounded in-memory
history. File growth does not identify the owner of process footprint. An early
diagnostic count included two empty IM journals; v1 is preserved and v2 corrects
the scope. The old 1×/new 2× near-fourfold slope ratio is only a display-condition
hypothesis, not a causal comparison.

**Do not adopt B7 or advance it into the next accepted baseline.** The PR target
meets its scoped gates, but complete regression does not pass. At the user's
explicit choice, this batch hands off the complete current record; no additional
same-2× original-A/B7 900-second Hook diagnostic was run, and the Hook footprint
owner remains unresolved. Original A and all prior failed records remain intact.
No workload, Release parameter or threshold changed to obtain acceptance.

The [artifact manifest](../../validation/b7-pr-idle-cache-20261017/artifact-preservation/manifest.json)
preserves the exact build/source/tests/tooling;
the [rejected-runtime manifest](../../validation/b7-pr-idle-cache-20261017/runtime-rejected/manifest.json)
preserves all 46 runs, original samples/logs, audits, exact CLI exits and cleanup.
The [final handoff manifest](../../validation/b7-pr-idle-cache-20261017/final-handoff/manifest.json)
joins both sealed packages with the final spec/architecture snapshots, historical
preservation checks and the final validation outcome. These local evidence
directories are ignored by Git; this repository record retains the decision and
the relevant hashes.
Neither file-only auditing nor an initial unlock/power observation proves
continuous OS inactivity or replaces unexecuted manual native UI/platform checks.
