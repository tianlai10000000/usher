# Settings Batch B Home RSS Reclaim (2026-09-28)

## Decision

**Rejected.** The final `settings-rounded-control-layers-v1` candidate was not
adopted. It reduced returned-Home RSS in all three formal pairs and removed a
repeatable graphics allocation, but it failed both predeclared magnitude gates.
All candidate product and test changes were mechanically reversed with the
frozen delta; the Batch A fixture and evidence were preserved.

中文结论：该候选不是“没有效果”。它把第一次和第二次返回 Home 后的 RSS
残留中位数分别降低了 **6.781 MiB** 和 **6.609 MiB**，三对实验方向完全
一致；graphics dirty 残留也稳定减少 **8.031 MiB**。但两轮只达到基线
Home RSS 的 **4.981%** 和 **4.767%**，都没有满足预先冻结的“至少 10 MiB
或 5%”采用门槛。不能把 4.981% 四舍五入成通过，也不能用 pilot 的更好
结果替代 formal。因此 Batch B 没有进入生产代码。

## Architecture boundary

Batch B targeted memory that remained after returning from Settings, rather
than only the peak while Settings was visible. Static and testing-backend
review established an important ownership constraint before the final
candidate was frozen:

- the Settings factory, selected section, and scroll geometry are route-local
  state that must remain mounted across ordinary Home/Settings navigation;
- destroying the complete factory would change navigation semantics and add
  reconstruction churn, so it was not accepted as an optimization strategy;
- transient dialog/editor item trees already unmount on close or route exit;
  tests retain this lifecycle boundary without unmounting stable Settings
  content.

The final candidate therefore changed rendering cost, not route ownership. It
replaced rounded child clipping in Settings text fields with a same-size
rectangular content clip while leaving rounded border/background painting on
the field chrome. It also added a default-off ActionMenu option used only by
the Settings section Select; every other ActionMenu kept the existing rounded
clip behavior. No global menu viewport wrapper or factory remount was included.

## Candidate progression

Each unsuccessful idea was rolled back before the next baseline was frozen:

| Candidate | Stage | Returned-Home RSS saving | Decision |
| --- | --- | ---: | --- |
| Settings visibility/remount | Pilot | 1.094 MiB in both rounds | Reject; below the approximately 5 MiB pilot screen and wrong lifecycle direction |
| Rounded text-field content clip | Pilot | 4.453 / 4.203 MiB | Reject; graphics mechanism worked but both rounds missed 5 MiB |
| Rounded controls bundle | Pilot | 8.828 / 8.750 MiB | Advance to formal only |
| Rounded controls bundle | Formal, 3 pairs | 6.781 / 6.609 MiB | Reject; both formal magnitude gates failed |

The first two candidates are retained only as ignored evidence under
`memory-benchmark-results/settings-batch-b-20260927/`; neither was carried into
the final production baseline.

## Frozen build and environment

| Field | Value |
| --- | --- |
| Git HEAD | `181e457b43d944701d434dd2bd86504853de4f21` |
| Build | `cargo build --release --locked --features test-fixtures` |
| Baseline binary | `baseline-rounded-controls-bundle-c6a0a96` |
| Baseline SHA-256 | `c6a0a96f5ad037bdb5b81d45bf23b4380eee8552f64a5bcc541fcc5fb78883a9` |
| Candidate binary | `candidate-rounded-controls-bundle-fbd8216` |
| Candidate SHA-256 | `fbd8216a754794fcff04fced80cc89bbb42778e586e175ca06c3b87a5118541b` |
| Candidate delta SHA-256 | `bbc6461bd85990c9a3118867900eb4d0a03626ee9a879c40a880204cbbf1546b` |
| Cargo.lock SHA-256 | `b793dad054187c60d1133a8d50b10a100b6e230105a4d372429455e84919ac91` |
| Rust | `rustc 1.97.1`, `cargo 1.97.1` |
| Machine | MacBook Pro (Mac14,9), Apple M2 Pro, 32 GiB |
| OS | macOS 27.0 (26A428) |
| Window | 800x600 logical at scale 2.00, native frame `(100,100,800,600)` |
| Backend | Winit/FemtoVG, two Tokio workers |
| Fixture | English, offline mode, three fixed Hooks, isolated state per process |

Both executables came from the repository source and were copied to immutable
absolute paths before measurement. `/Applications/Usher.app` and any other
installed binary were excluded by the runner and comparator.

## Protocol

The formal order was frozen as `A1 B1 B2 A2 A3 B3`. Every run was a fresh
process and performed two complete cycles:

1. settle untouched Home for at least 30 seconds, then collect five RSS and
   footprint samples plus categorized footprint and `vmmap -summary`;
2. enter Settings and traverse General, Code hosting, AI Agent, AI Review,
   Diagnostics, and Hooks through the real section Select;
3. hover the Hook Edit action until its Tooltip paints, open the real Edit Hook
   Dialog, close it, return Home, and settle for at least 30 seconds;
4. repeat the complete Settings operation and returned-Home measurement.

The runner validated exact PID/start identity, source and binary hashes,
geometry, route diagnostics, fixture/final-state hashes, zero compression,
normal exit, and non-overlap. Each phase retained raw `ps`, `footprint`, and
`vmmap` evidence. The exact comparison command was:

```sh
USHER_SETTINGS_BENCHMARK_RUN_SET=rounded-controls-v1 \
  node memory-benchmark-results/settings-batch-b-20260927/compare.mjs formal
```

The comparator returned exit status 1 because the result was valid but failed
the frozen acceptance gates. Its SHA-256 was
`dc427cbd8f3185b071496f607db6ca6bc243a2cce66af393bef508be6415d2cc`;
the generated comparison SHA-256 was
`70864e1583335771e6696848ac80d3e209dc836966a0f1124342e2650521ef48`.

## Formal result

| Metric | Baseline median | Candidate median | Candidate improvement |
| --- | ---: | ---: | ---: |
| First returned-Home RSS | 136.141 MiB | 129.266 MiB | 6.875 MiB absolute |
| First returned-Home residual | 13.625 MiB | 6.844 MiB | **6.781 MiB** |
| First saving / baseline Home RSS | - | - | **4.981%** |
| Second returned-Home RSS | 138.656 MiB | 131.828 MiB | 6.828 MiB absolute |
| Second returned-Home residual | 15.922 MiB | 9.313 MiB | **6.609 MiB** |
| Second saving / baseline Home RSS | - | - | **4.767%** |
| First returned-Home footprint | 102.673 MiB | 94.860 MiB | 7.813 MiB |
| Second returned-Home footprint | 104.923 MiB | 96.282 MiB | 8.641 MiB |
| Graphics-dirty residual, both rounds | 10.563 MiB | 2.531 MiB | **8.031 MiB** |
| Owned-unmapped graphics residual | 2.203 MiB | 0.328 MiB | 1.875 MiB |

All paired RSS residual changes had the same direction:

| Pair | First-return saving | Second-return saving |
| --- | ---: | ---: |
| 1 | 6.781 MiB | 6.609 MiB |
| 2 | 6.281 MiB | 6.078 MiB |
| 3 | 7.656 MiB | 7.578 MiB |

Candidate lifetime RSS peaks were 131.62-132.22 MiB versus 138.48-139.03 MiB
for baseline. The candidate's second returned-Home RSS grew 2.469-2.594 MiB
over its first return, inside the 5 MiB guard. Every Home sampling spread was
below 5 MiB, candidate RSS stayed below 300 MiB, active Settings did not
regress, footprint did not regress, and all processes exited cleanly.

The only failed gates were the two target magnitude gates. Formal acceptance
required each round's median residual saving to be at least 10 MiB or 5% of the
corresponding baseline returned-Home RSS. A consistent sub-threshold result is
useful attribution evidence, but it is not an accepted optimization under the
repository contract.

The lower 5% alternatives were 6.807 MiB for round one and 6.933 MiB for round
two. The measured savings missed them by 0.026 MiB and 0.323 MiB respectively.
The very small first-round shortfall is not grounds to round the result up or
rerun an unchanged candidate until it passes.

## Attribution

The result supports the rendering-layer hypothesis. In both formal rounds,
graphics dirty residual fell by 8.031 MiB, including 1.875 MiB of
owned-unmapped graphics. IOSurface, Malloc Large, and mapped-file changes were
symmetrical. Malloc Small offset the graphics benefit by 0.594 MiB in round
one and improved by only 0.188 MiB in round two.

The graphics categories overlap in the system report, so owned-unmapped
graphics is not added to total graphics dirty. The evidence shows a real
FemtoVG/macOS graphics residency reduction; it does not prove that every dirty
graphics byte must appear one-for-one in process RSS. The six-process RSS result
remains the adoption metric.

## Functional evidence and unexecuted gates

Before freezing the candidate, the following checks passed:

- 15 Settings tests;
- five transient-input tests;
- the production text-field drag-selection test;
- default ActionMenu and Select regression tests;
- `scripts/check-ui-policy.sh`, candidate Release build, formatter check, and
  `git diff --check`;
- probe/comparator self-tests, including valid namespace and invalid namespace
  fixtures.

The final candidate did **not** run the separate same-instrumentation
AfterRendering p95 matrix or the complete English/zh-CN native visual and
interaction matrix. Those were downstream adoption gates and were deliberately
stopped after the formal memory gate failed. Baseline coordinate calibration
visibly covered English Home, all Settings sections, the Tooltip, and the real
Edit Hook Dialog, but it is not claimed as candidate visual proof.

Post-measurement architecture review also found three unresolved adoption risks:

- `GitHubTextField` is shared by Project Add/Edit dialogs, so the field-level
  clip change would require native placeholder, validation, loading, focus,
  selection, and rounded-border coverage on those routes as well as Settings;
- the new ActionMenu property was public, but a constrained-height scrolling
  menu can move the 4px row inset away from the rounded edge and place the
  scroll thumb close to the paint-only corner. The public mode lacked a
  native overflow pixel matrix;
- the materially changed public ActionMenu/Select property had no matching
  component-gallery contrast, so the shared visual contract was incomplete.

These issues did not affect the rejection decision because the memory gate had
already failed. They are recorded so a future experiment cannot reuse the
candidate unchanged and claim that only timing remains.

## Rollback proof

The frozen candidate delta passed `git apply --reverse --check` against the
measured working tree and was then reversed exactly. After rollback, the six
core source hashes matched the baseline provenance byte for byte, including
the Settings text fields, AI Review rule field, ActionMenu, Select popup,
Settings view, and Rust Settings bridge. Candidate-only transient-input and UI
tests disappeared from `git diff --name-only`.

The remaining working-tree changes are the independent Batch A benchmark
fixture, its stable Settings lifecycle test strengthening, and Batch A
documentation. No Batch B product behavior remains.

## Post-rollback validation

The restored baseline and retained Batch A tooling passed:

- `cargo fmt --all`;
- `cargo check --locked`;
- `cargo check --locked --features test-fixtures`;
- `cargo test --lib --locked`: 677 passed, 0 failed, 2 ignored;
- `cargo test --lib --locked --features test-fixtures workflow::pr_review::controller`:
  34 passed, 0 failed, 1 ignored;
- `scripts/check-ui-policy.sh`;
- Settings comparator positive/negative fixture self-tests;
- decision JSON parsing and `git diff --check`.

The existing `HookService::events_after` dead-code warning and
`src/hooks/projection.rs` test-only unused import remain unrelated. The full
`cargo test --locked` suite was not rerun; the repository records its known
`src/logger/debug_logger.rs` doctest failure separately. No new failure was
observed.

## Next optimization boundary

Do not rerun this identical candidate hoping that the 4.981% result crosses
5%; that would create repeat-until-pass bias. The rectangular-clip result may
be reused only as mechanism evidence in a newly frozen batch that identifies a
separate material graphics owner. Such a batch must compare a fresh current
baseline against one coherent candidate and rerun the full RSS, p95, locale,
visual, interaction, and packaging gates.

The stable Settings factory should remain mounted unless a future product
spec explicitly changes route-state restoration. The remaining baseline
returned-Home residual is approximately 13.625 MiB after one cycle and
15.922 MiB after two; the next investigation should attribute that residual
before introducing another lifecycle policy.

## Evidence paths

- Frozen protocol: `memory-benchmark-results/settings-batch-b-20260927/protocol.md`
- Final pilot: `memory-benchmark-results/settings-batch-b-20260927/rounded-controls-v1-pilot-comparison.json`
- Formal comparison: `memory-benchmark-results/settings-batch-b-20260927/rounded-controls-v1-comparison.json`
- Machine-readable decision: `memory-benchmark-results/settings-batch-b-20260927/rounded-controls-v1-decision.json`
- Raw formal runs: `memory-benchmark-results/settings-batch-b-20260927/runs/rounded-controls-v1/`
- Frozen binaries, source patches, delta, and provenance:
  `memory-benchmark-results/settings-batch-b-20260927/artifacts/`
- Text-field-only rejection:
  `memory-benchmark-results/settings-batch-b-20260927/rounded-clip-rejection.md`
- Visibility/remount archive:
  `memory-benchmark-results/settings-batch-b-20260927/archive/settings-visibility-rejected-3eeeac8-3b571f2/`

The evidence directory is intentionally ignored by Git; this dated report is
the tracked decision record.
