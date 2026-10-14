# Project List Batch E Returned-Home RSS (2026-09-28)

## Decision

**The List owner was confirmed, but the product candidate was rejected and
rolled back.** A full Project List exposure added **13.672 / 13.438 MiB** and
**13.297 / 11.219 MiB** stable returned-Home RSS over Grid-only in two fresh
process pairs. The dominant category was a **10.797 MiB** `Malloc Large`
region, matching the full-document rounded List clip.

The single-variable `project-list-row-owned-frame-v1` candidate removed that
large active layer and reduced active List RSS peaks by **24.656 / 25.656 MiB**.
However, after returning Home and settling for 30 seconds, the paired RSS
savings were only **1.750 MiB (1.43%) / 3.828 MiB (3.12%)**. Both rounds missed
the predeclared 5 MiB pilot gate, so the formal six-process adoption matrix was
not run and the product UI delta was exactly reversed.

中文结论：Batch E 证实 Project List 的整文档圆角裁剪会产生显著 active 峰值，
但它不是一个可稳定转化为“回到 Home 后 RSS 节省”的候选。候选将 List 打开期间的
RSS 峰值降低约 25 MiB，但回到 Home 30 秒并继续观察 20 秒后，只节省
**1.750 / 3.828 MiB**，未达到预先冻结的 5 MiB pilot 线。产品改动已回滚；
没有用 active 峰值改善冒充用户要求的 Home 回收改善。

## Scope and architecture boundary

This batch intentionally kept Project registry state, Logo projections, search
state, factories, route ownership, and all durable data unchanged. It did not
clear models on route exit, reset FemtoVG, force allocator pressure relief, or
destroy the stable Project factory.

The renderer boundary was checked before implementation:

- Slint 1.18.0 FemtoVG sends a non-zero rounded `clip` through `render_layer`,
  while a zero-radius clip uses rectangular scissoring;
- the layer target allocates a physical-size GPU render target and associated
  framebuffer resources;
- the relevant Slint 1.18.1 renderer, item-cache, compiler clip, and FemtoVG
  dependency sources are byte-identical to 1.18.0, as recorded in
  `docs/archive/memory/settings-memory-batch-c-attribution-20260928.md`;
- dropping the item/layer releases ownership, but neither Rust deallocation nor
  the system/driver allocator promises an immediate RSS reduction.

This supported a measured List-only candidate. It did not justify changing
Grid cards, Logo clips, model lifecycle, or the renderer globally.

## Corrected attribution protocol

The first Project waterfall found an important harness defect: its five
“stable” samples covered only about 2.4 seconds after the 30-second settle. One
run released a 10.8 MiB empty large allocation roughly two seconds after the
last sample. Batch E v2 therefore retained the 30-second point and spread five
samples over the following 20 seconds. The last three points were reported as
the stable tail, while full-window spread and delayed reclamation remained
visible.

The old v1 probe was restored byte-for-byte after the audit (SHA-256
`ac14d322893876d4b12a1fd62289d9f094a9e95f914903dec57cdc87a12e516e`),
and the split work moved to a new evidence directory and protocol version.

All split arms used one frozen source-built Release artifact:

| Field | Value |
| --- | --- |
| Git HEAD | `181e457b43d944701d434dd2bd86504853de4f21` |
| Build | `cargo build --release --locked --features test-fixtures` |
| Artifact SHA-256 | `93d9da597c2fa40ddbadf8a9c8eebe0e023d393c2ddcc8fcc5c8d9efb9bac52b` |
| Cargo.lock SHA-256 | `2f87849fe4319e952d1af35509bd232852d724adaa96956e7929cfb7447118a8` |
| Backend | Winit/FemtoVG, two Tokio workers |
| Window | 800x600 logical at scale 2.00 |
| Fixture | 10 projects, six fixed local Logos, offline isolated state |

The three arms were orthogonal:

- `home-only/grid-only`: execute the same model/search callbacks without
  mounting Project;
- `six-logos/grid-only`: mount Project but never mount List;
- `six-logos/grid-list`: add only the second-2 through second-6 List exposure.

Each arm ran twice in the interleaved order `H1 G1 L1 L2 G2 H2`. Every run had
a clean shutdown, zero compression, exact state/artifact hashes, valid
`10 -> 1 -> 10` projection checks, stable Home tails, and a passing foreground
monitor.

## Grid/List owner result

Positive values are treatment-minus-control returned-Home residual contrasts.

| Pair | Round 1 stable RSS | Round 2 stable RSS | Decision |
| --- | ---: | ---: | --- |
| H1 -> G1, Grid route exposure | +1.891 MiB | +2.250 MiB | Below 5 MiB screen |
| H2 -> G2, Grid route exposure | +2.594 MiB | +4.828 MiB | Below 5 MiB screen |
| G1 -> L1, additional List exposure | **+13.672 MiB** | **+13.438 MiB** | Pass |
| G2 -> L2, additional List exposure | **+13.297 MiB** | **+11.219 MiB** | Pass |

The List pairs had positive physical-footprint contrast in all four rounds.
Their `Malloc Large` contrasts were `+10.797 / +10.797 MiB` and
`+10.797 / +8.594 MiB`; graphics dirty was much smaller. Grid was not proven
to be zero-cost—the second Grid pair came within 0.172 MiB of the screen—but it
was not authorized for this candidate.

Static geometry explained the List fingerprint. Ten 136px rows sit below one
6px rounded `clip`; at 2x scale its full-document RGBA target is in the same
size class as the observed large allocation. The candidate therefore reused
the repository's established Messages pattern: rectangular scissoring for
middle rows, bounded rounded clipping for first/last rows, and a continuous
row-owned outline. Loading/error/empty retained their original bounded frame.

## Frozen candidate A/B

The A/B harness added explicit active-List phases and increased each Home phase
window from 25 to 35 seconds, leaving at least ten seconds after the final timed
sample for categorized footprint and `vmmap`. A and B contained the identical
harness and Cargo.lock. The only product difference was the frozen patch with
SHA-256 `78e436897bfc995a86980d2715d627b77596ee671b35f38584587092abaf2099`.

| Variant | Frozen artifact | SHA-256 |
| --- | --- | --- |
| A, original full-list rounded clip | `baseline-5af19a76` | `5af19a765ae611ce77b96db4e97f3e5f52dc907c4ef9e78f3275ebe1b0cf491f` |
| B, row-owned frame | `candidate-077c8b19` | `077c8b198b807ea0915b5dc27e9102307d8b4483ce36eed3a810cf4f8ef65a76` |

The pilot pair had excellent initial balance: 0.344 MiB RSS and 0.203 MiB
physical-footprint difference. All Home sample spreads were zero and both
processes exited cleanly.

### Returned Home: primary decision metric

| Round | A residual | B residual | RSS saving | Footprint saving | Gate |
| ---: | ---: | ---: | ---: | ---: | --- |
| 1 | +4.578 MiB | +2.828 MiB | **1.750 MiB / 1.43%** | 1.516 MiB | Fail `< 5 MiB` |
| 2 | +4.828 MiB | +1.000 MiB | **3.828 MiB / 3.12%** | 1.016 MiB | Fail `< 5 MiB` |

The candidate was safe in this pair—returned footprint improved, candidate
round two did not grow, and returned RSS stayed well below the cap—but both
rounds missed the pilot screen. The formal `A1 B1 B2 A2 A3 B3` set was therefore
correctly skipped instead of rerunning until allocator timing produced a more
favorable baseline.

### Active List: secondary evidence only

| Round | A RSS peak | B RSS peak | Change | A footprint peak | B footprint peak | Change |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 146.813 MiB | 122.156 MiB | **-24.656 MiB** | 142.142 MiB | 110.126 MiB | **-32.016 MiB** |
| 2 | 145.922 MiB | 120.266 MiB | **-25.656 MiB** | 142.220 MiB | 105.751 MiB | **-36.469 MiB** |

This confirms the rounded List layer mechanism and shows that the candidate is
effective for active peak allocation. It does not satisfy the requested
returned-Home objective. In A, the large allocation had already been purged by
the 30-second Home point in this fresh process, leaving too little persistent
residual for B to save repeatably.

## Foreground and input safety

No run used AX actions, `CGEvent`, AppleScript activation, system mouse
movement, or OS keyboard injection. Navigation, view switching, and search ran
only through Slint event-loop callbacks. The native window used Accessory
activation policy, disabled automatic activation, stayed always-on-bottom, and
ignored cursor hit testing. An independent `NSWorkspace` helper sampled the
frontmost PID every 100 ms; all eight valid split/pilot processes exited with
monitor code 0. These tests did not take the user's mouse or foreground app.

## Rollback and validation boundary

The exact candidate patch was reversed after the valid pilot failure. A clean
`git diff` for both Project UI files and a successful forward patch check prove
that no Batch E product UI remains. The measurement fixture remains behind the
non-default `test-fixtures` feature and explicit environment selectors.
The exact Rust fixture source used for both artifacts is frozen under the A/B
evidence directory with its recorded `a9d36729...` hash. The tracked copy only
received a post-run `cfg(test-fixtures)` import guard to avoid a default-build
unused-import warning; that import is present in the measured feature build and
does not change fixture timing or behavior.

The candidate had passed Slint compilation and UI policy checks before the
pilot. The expanded visual matrix was not run because the memory gate failed
first; an unadopted candidate is not promoted based on visual work. Final
repository results after rollback:

- `cargo fmt --all`: passed;
- `cargo check --locked`: passed, with only the existing `events_after`
  dead-code warning;
- `cargo check --locked --features test-fixtures`: passed with the same warning;
- `cargo test --lib --locked`: 677 passed, 0 failed, 2 ignored;
- targeted `home_return_tests` with `test-fixtures`: 3 passed, 0 failed;
- UI policy, four probe/comparator self-test suites, and `git diff --check`:
  passed;
- Project UI files and `material-1.18.0/`: no working-tree difference.

The test build also reports the repository's existing
`src/hooks/projection.rs` test-only unused import warning. No new test failure
was introduced.

## Evidence

- Split protocol and raw runs:
  `memory-benchmark-results/project-home-batch-e-split-20260928/`
- Machine-readable split comparisons:
  `grid-owner-pair-{1,2}.json` and `list-owner-pair-{1,2}.json`
- A/B protocol, build manifest, frozen artifacts, and raw pilot:
  `memory-benchmark-results/project-home-batch-e-ab-20260928/`
- Machine-readable pilot decision: `pilot-comparison.json`

The evidence directories are intentionally Git-ignored. This tracked report is
the durable decision record.
