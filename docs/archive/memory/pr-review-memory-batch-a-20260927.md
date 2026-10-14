# PR Review Batch A Home RSS Reclaim (2026-09-27)

## Decision

**Rejected.** The `release-transient-v2` lifecycle candidate was not adopted.
The production lifecycle changes were removed after the first complete formal
pair failed a predeclared acceptance gate. The deterministic
`test-fixtures`-only workload remains available for future candidates.

中文结论：候选能释放约 6 MiB 的 malloc 在用对象，但释放后的页主要进入
allocator slack，没有形成稳定、可重复的 Home RSS 回收。第一次返回 Home 的
成对 RSS 残留反而回退 0.313 MiB；第二次返回的 3.234 MiB 改善也未达到
10 MiB 或 5% 门槛。因此不能把“对象已析构”描述成“进程 RSS 已回收”。

## Question and candidate

The experiment tested the user-visible lifecycle question, not only peak
allocation while PR Review was open:

1. Start a fresh source-built Release process and settle on Home.
2. Open a deterministic large PR Review and materialize a real highlighted
   diff presentation.
3. Return to Home, settle for 30 seconds, and measure retained RSS/footprint.
4. Re-enter the same review, require a highlighted-cache hit with no grammar
   reload or token regeneration, return to Home again, and measure a second
   30-second plateau.

`release-transient-v2` cleared the root Slint diff-line/metric presentation and
detached the syntax grammar/theme slot when leaving PR Review. Grammar
destruction was transferred to the shared blocking pool so it could not block
the Slint event loop. It intentionally retained the normalized base/range
domain cache, highlighted-file LRU, source LRU, filters, collapsed folders,
selection, and review context so ordinary re-entry could reuse the highlighted
result.

This was an offline, `test-fixtures`-gated runtime policy. It was never enabled
as normal product behavior.

## Frozen build and environment

| Field | Value |
| --- | --- |
| Git HEAD | `181e457b43d944701d434dd2bd86504853de4f21` |
| Build | `cargo build --release --locked --features test-fixtures` |
| Binary | `artifacts/usher-batch-a-v2-181e457-3eeeac8` |
| Binary SHA-256 | `3eeeac8a491b5f0d9df9c7f0533d8e42a855c5c60d3bd49f1453676158d247cf` |
| Source diff SHA-256 | `a017ee58a07fb1f56bcb6aff5cbcd40006fcd00e0938ae7ef28909fbb87a3f4c` |
| Cargo.lock SHA-256 | `b793dad054187c60d1133a8d50b10a100b6e230105a4d372429455e84919ac91` |
| Rust | `rustc 1.97.1`, `cargo 1.97.1`, `aarch64-apple-darwin` |
| Machine | MacBook Pro (Mac14,9), Apple M2 Pro, 32 GB |
| OS | macOS 27.0 (26A428) |
| Window | 800x600 logical, 1600x1200 physical, scale 2.0 |
| Renderer | Existing source-built Release default (FemtoVG/OpenGL) |

Both arms used the same frozen binary and checkout provenance. The only A/B
variable was `USHER_PR_PRESENTATION_LIFECYCLE=retain` versus
`release-transient-v2`. No installed `/Applications/Usher.app` binary was used.

## Fixture and protocol

The synthetic `large-ranges-highlighted-v1` fixture uses production review,
cache, highlighting, and routing paths. It contains:

- 206 files and 27,369 lines in each immutable detail;
- one base detail plus four visited range details, totalling 136,845 owned
  lines and an estimated 20,895,460 heap bytes;
- one selected highlighted diff with 2,932 lines, 17,592 tokens, and an
  estimated 1,325,289 heap bytes;
- no network dependency and isolated per-run benchmark state.

The formal order was predeclared as `A1 B1 B2 A2 A3 B3`, where A is retain and
B is the candidate. Every phase required a 30-second settle followed by five
external samples spaced 0.5 seconds apart. Initial fixture loading and re-entry
were each delayed 60 seconds so their work could not contaminate the preceding
Home capture.

Acceptance required all of the following before any wider matrix:

- all three paired first-return and second-return residuals improve in the same
  direction;
- the median paired saving is at least 10 MiB or 5% for both returns;
- footprint does not regress by more than 5%, candidate RSS stays below
  300 MiB, and the active peak stays within 8 MiB or 5%;
- every Home sampling window has at most 5 MiB RSS and footprint spread;
- compression remains zero, shutdown is clean, and the second candidate return
  grows by at most 5 MiB over the first;
- re-entry is a highlighted-cache hit with zero grammar reloads and zero token
  regeneration.

Passing this stage would only permit multi-PR soak, native AfterRendering p95,
full-file/range, late-I/O, resize, and visual checks. It would not itself adopt
the candidate.

## Pilot

The one-pair, one-second-settle pilot was used only to validate the protocol:

| Metric | Retain | Candidate | Candidate change |
| --- | ---: | ---: | ---: |
| First Home residual | 31.484 MiB | 30.938 MiB | -0.547 MiB |
| Second Home residual | 31.531 MiB | 30.781 MiB | -0.750 MiB |
| Lifetime RSS peak | 155.70 MiB | 155.77 MiB | +0.07 MiB |
| First Home malloc in-use | 42.23 MiB | 36.03 MiB | -6.20 MiB |
| First Home callback | 0.386 ms | 1.916 ms | +1.530 ms |
| Re-entry callback | 0.386 ms | 5.437 ms | +5.051 ms |
| Second Home callback | 0.668 ms | 4.430 ms | +3.762 ms |

The pilot failed the RSS magnitude and footprint gates and was explicitly
marked `pilot_only`; it was not adoption evidence.

## Formal evidence and early rejection

The first formal pair completed cleanly and is sufficient to reject the
candidate because the protocol requires *every* pair to improve. Later pairs
cannot make a positive first-pair delta negative.

| Metric | A1 retain | B1 candidate | Paired interpretation |
| --- | ---: | ---: | --- |
| Initial Home RSS | 123.359 MiB | 123.391 MiB | Fresh-process baseline |
| First returned Home RSS | 155.016 MiB | 155.359 MiB | Candidate +0.344 MiB absolute |
| First RSS residual | +31.656 MiB | +31.969 MiB | **Candidate worse by 0.313 MiB** |
| First footprint | 119.470 MiB | 119.720 MiB | Candidate worse by 0.250 MiB |
| Second returned Home RSS | 156.469 MiB | 153.266 MiB | Candidate -3.203 MiB absolute |
| Second RSS residual | +33.109 MiB | +29.875 MiB | Candidate better by 3.234 MiB |
| Second footprint | 120.548 MiB | 114.360 MiB | Candidate lower, but unstable |
| Second footprint spread | 0.000 MiB | 5.625 MiB | Candidate exceeds 5 MiB stability gate |
| Lifetime RSS peak | 156.56 MiB | 155.66 MiB | No peak regression |
| Re-entry ready | 105 ms | 108 ms | Model-ready polling, not frame p95 |

Candidate navigation callback times were 2.035 ms for the first Home return,
6.073 ms for re-entry, and 3.267 ms for the second Home return. Retain measured
0.290 ms, 1.210 ms, and 0.601 ms respectively. These are callback durations,
not native AfterRendering p95, but they provide no latency justification for a
memory-neutral candidate.

The six-run process was interrupted while B2 was settling its second Home. A1
and B1 have complete results and clean exits; B2 is partial and A2/A3/B3 were
not run. The interrupted data was not combined with any later run. A later
attempt was stopped after A1 when review established that rerunning identical
source and protocol could only create repeat-until-pass bias. Neither
interrupted directory contains a formal summary or is described as a complete
three-pair comparison.

## Why released objects did not reclaim RSS

The candidate did release real Rust allocations. At the first returned Home,
malloc in-use fell from 42.01 MiB to 36.00 MiB. At the second return it fell
from 42.32 MiB to 36.07 MiB. That approximately 6 MiB owner reduction did not
become stable RSS:

- first-return RSS and footprint were slightly higher;
- allocator slack rose from 49.99 to 60.00 MiB on the first return and from
  49.68 to 59.93 MiB on the second return;
- compression was 0.00 MiB in both arms, so compressed-memory pressure did not
  explain the result;
- both peaks stayed near 156 MiB, far below the 300 MiB absolute guard.

The dominant review domain owners also remained reachable by design: five
immutable details account for about 20.9 MiB estimated heap, and the retained
highlight result accounts for about 1.3 MiB. Releasing the root Slint line
projection and grammar cannot reclaim those owners without changing review
navigation semantics. macOS malloc-zone reservation and renderer/backend
high-water allocations can keep freed pages resident, so a successful `Drop`
test or lower malloc in-use is not evidence of lower process RSS.

The approximately 32 MiB return-to-Home residual therefore has two measured
components: intentionally reachable review/domain state and process-level
allocator/renderer residency. This experiment did not prove a leak, and it did
not show that clearing more UI presentation would recover the residual.

## Adopt, rollback, and retained tooling

Decision: **Reject `release-transient-v2`.** The lifecycle policy, asynchronous
grammar reclaim path, and candidate-specific tests were removed. Normal route
hiding retains the current PR Review state exactly as before Batch A.

Retained tooling is limited to the `test-fixtures` feature:

- delayed large highlighted fixture installation, allowing an uncontaminated
  initial Home measurement;
- two Home-return cycles with a real highlighted-cache re-entry;
- owner/model receipts and navigation callback timings;
- the ignored raw runner, frozen binary, provenance, logs, vmmap, footprint,
  and partial-result evidence under
  `memory-benchmark-results/pr-review-batch-a-20260927/`.

The retained fixture does not run in a normal production build and does not
change product lifecycle behavior.

## Next optimization boundary

Do not retry broader lifecycle clearing or allocator purge as the next change.
The next candidate should first separate the persistent approximately 32 MiB
Home residual into domain cache, renderer/graphics, and allocator-residency
budgets using a multi-PR repeated-growth protocol. A production change is only
justified if one owner grows across different PRs or a narrowly scoped owner can
return at least 10 MiB/5% RSS without losing same-review navigation state or
regressing native frame latency.

Full-file, range-diff, late-worker, resize, visual, and multi-PR soak matrices
were deliberately not run after the stage-one hard failure.

## Post-rollback validation

The retained fixture and restored production behavior passed:

- `cargo fmt --all`;
- `cargo check --locked`;
- `cargo check --locked --features test-fixtures`;
- `cargo test --lib --locked`: 677 passed, 0 failed, 2 ignored;
- `cargo test --lib --locked --features test-fixtures workflow::pr_review::controller`:
  34 passed, 0 failed, 1 ignored;
- `cargo test --test pr_review_core_journeys --locked`: 5 passed, 0 failed;
- runner `python3 -m py_compile` and `git diff --check`.

The existing `src/hooks/projection.rs` unused-import warning and
`HookService::events_after` dead-code warning remain unrelated. The full
`cargo test --locked` suite was not rerun; the repository records its known
`src/logger/debug_logger.rs` doctest baseline failure separately. No new test
failure was observed.

## Evidence paths

- Pilot: `memory-benchmark-results/pr-review-batch-a-20260927/pilot-async-v5/`
- Formal interrupted run with complete A1/B1:
  `memory-benchmark-results/pr-review-batch-a-20260927/formal-v2-async-30s-v1/`
- Deliberately stopped duplicate attempt:
  `memory-benchmark-results/pr-review-batch-a-20260927/formal-v2-async-30s-v2/`
- Frozen binary and provenance:
  `memory-benchmark-results/pr-review-batch-a-20260927/artifacts/`
- Machine-readable decision:
  `memory-benchmark-results/pr-review-batch-a-20260927/decision.json`
- Parent all-route source-built Release diagnosis:
  `memory-benchmark-results/all-pages-rss-20260926/report.md`

The evidence directory is intentionally ignored by Git; this dated report is
the tracked decision record.
