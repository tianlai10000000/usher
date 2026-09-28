# Settings Batch C Home RSS Owner Attribution (2026-09-28)

## Decision

**Attribution-only batch; no product optimization adopted.** The complete
Settings operation still leaves a material returned-Home high-water mark, but
the controlled owner splits did not identify one clean owner that passed the
predeclared 5 MiB screening boundary in both rounds. Product UI and lifecycle
code were therefore left unchanged.

中文结论：完整 Settings 操作相对“只进入 General”在两轮返回 Home 后多保留
**13.453 / 16.078 MiB RSS**，问题真实存在；但拆分后的 ActionMenu、字段密集
sections、Tooltip、Dialog 都没有单独达到两轮 5 MiB 筛选线。最接近的 Hooks
上界实验后来确认混入了 ActionMenu，不能归因给 Hooks。修正后的对称 Hooks
实验连续两次被约 8.8 MiB 的瞬时 graphics physical-footprint 台阶判为无效，
因此没有继续“重跑到通过”，也没有据此盲改产品代码。

## Why this batch did not modify the product

Batch B proved that removing rounded control layers changes real graphics
residency, but its six-process product candidate missed the frozen `10 MiB or
5%` adoption gate and was rolled back. Batch C therefore changed the question
from “which visual code can be simplified?” to “which independently exercised
owner explains the returned-Home residual?”

The batch used the unchanged, rolled-back source tree and changed only native
interaction exposure between control and treatment. It did not unload the
stable Settings factory, clear Hook domain state, reset the renderer, invoke
allocator pressure relief, or alter any application paint/lifecycle behavior.

## Frozen build and environment

| Field | Value |
| --- | --- |
| Git HEAD | `181e457b43d944701d434dd2bd86504853de4f21` |
| Build | `cargo build --release --locked --features test-fixtures` |
| Frozen artifact | `baseline-c6a0a96` |
| Artifact SHA-256 | `c6a0a96f5ad037bdb5b81d45bf23b4380eee8552f64a5bcc541fcc5fb78883a9` |
| Cargo.lock SHA-256 | `b793dad054187c60d1133a8d50b10a100b6e230105a4d372429455e84919ac91` |
| Rust | `rustc 1.97.1`, `cargo 1.97.1` |
| Backend | Winit/FemtoVG, two Tokio workers |
| Window | 800x600 logical at scale 2.00 |
| Fixture | English, offline, isolated state, three fixed Hooks |

The previous `target/release/usher` was still the rejected Batch B candidate
(`fbd8216...`). It was never measured. A fresh full-LTO source build reproduced
the rolled-back baseline hash `c6a0a96...`, which was copied to an immutable
absolute artifact path before the first run. Installed application bundles
were forbidden by the probe.

## Protocol

Every valid arm was a fresh process. It settled untouched Home for at least 30
seconds, retained five RSS and physical-footprint samples plus categorized
footprint and `vmmap`, exercised its assigned Settings exposure, returned Home,
settled for at least 30 seconds, and repeated the cycle once. The primary value
is:

```text
(treatment returned Home - treatment initial Home)
  - (control returned Home - control initial Home)
```

Both arms used the same binary and tool hashes. Initial Home RSS had to differ
by no more than 5 MiB; every sampled RSS and footprint phase had a 5 MiB spread
limit; processes could not overlap; geometry, PID identity, zero compression,
step schema, raw samples, normal shutdown, and artifact hashes were retained.

The pairs deliberately controlled known confounders:

- field-dense sections and their control opened the section Select the same
  number of times;
- Dialog control and treatment both rendered the Hook Edit Tooltip before only
  the treatment opened and closed the real editor;
- Tooltip control and treatment both entered Hooks;
- the full-flow pair is an end-to-end bound, not an owner attribution.

These are pilot attribution pairs, not product-candidate A/B runs. No pilot
result is allowed to satisfy the repository's optimization adoption gate.

## Valid pilot results

Positive treatment-minus-control values mean that the extra exposure left more
memory after returning Home.

| Exposure | Round 1 RSS | Round 2 RSS | Round 1 graphics dirty | Round 2 graphics dirty | Decision |
| --- | ---: | ---: | ---: | ---: | --- |
| ActionMenu: select General vs leave General | +4.219 MiB | +2.266 MiB | +2.344 MiB | +2.406 MiB | Below screen |
| Field-dense sections vs equal-count General/Diagnostics control | +1.563 MiB | +3.906 MiB | +5.859 MiB | +5.859 MiB | Real graphics owner; RSS below screen |
| Hook Edit Dialog after equal Tooltip warm-up | +3.031 MiB | +3.094 MiB | +1.141 MiB | +0.016 MiB | Below screen; round-two graphics signal absent |
| Hook Edit Tooltip vs Hooks control | +0.406 MiB | -1.906 MiB | -0.172 MiB | -0.172 MiB | No positive owner signal |
| Full Settings flow vs General-only route | **+13.453 MiB** | **+16.078 MiB** | **+11.438 MiB** | **+11.438 MiB** | Confirms the aggregate residual only |

The full-flow treatment itself retained 15.016 MiB after the first cycle and
17.703 MiB after the second. Its General-only control retained 1.563 and 1.625
MiB. This agrees with Batch B's independent baseline residuals closely enough
to confirm that the split protocol still observes the target phenomenon.

The result is fragmented rather than contradictory: ActionMenu and field-heavy
sections both contribute graphics residency; Dialog adds a smaller mostly
malloc-backed first-round increment; Tooltip does not matter. Cross-pilot sums
are not treated as a formal decomposition because each pair has an independent
process baseline and different control content.

## Hooks confound and invalid attempts

An initial `hooks-route-v1` pilot reported +4.859 / +4.781 MiB RSS and +3.016
MiB graphics dirty. Audit caught that its control stayed on General without
opening the section Select, while treatment opened ActionMenu and selected
Hooks. This is only an upper bound for `ActionMenu + Hooks`, not a Hooks result.
A formal A1 process was stopped as soon as the confound was found; it is marked
invalid and is not combined with any other run.

The corrected `hooks-section-v2` gives both arms one real Select operation per
round: control reselects General and treatment selects Hooks. It did not produce
a valid pair:

1. attempt 1 PA completed, but its second returned-Home footprint spread was
   8.8125 MiB; PB was stopped;
2. attempt 2 PA already had an 8.828125 MiB initial-Home footprint spread and
   was stopped.

Raw RSS stayed nearly flat while physical footprint stepped by about 8.8 MiB,
consistent with transient graphics-surface residency. The frozen protocol
requires both metrics to be stable, so neither attempt can be used. A third
unchanged rerun would be repeat-until-pass bias and was not attempted.

Static review also found no plausible large Hooks layer to justify acting in
the absence of a valid pair. The 528x218 category card has rounded painting but
no rounded clip, shadow, explicit layer, or cache hint. The only clear composite
layers in the three-Hook empty-history fixture are disabled-control opacity
surfaces with a raw upper bound of roughly 0.31 MiB. Small icons are tens of
KiB. FemtoVG may grow renderer-wide 512x512 RGBA glyph-atlas pages, but item-tree
unmounting would not release that global atlas and the current evidence does
not prove Hooks created a new page.

## Slint 1.18.1 boundary

Before Batch C, official Slint 1.18.1 source packages were compared against the
project's 1.18.0 packages. The relevant FemtoVG renderer files, core item-cache
files, compiler clip passes, and FemtoVG 0.27 dependency were byte-identical.
The 1.18.1 renderer change was an image buffer-length safety assertion, and the
documented native-menu leak fix concerned a shutdown reference cycle not used
by Usher's Settings route. Slint was therefore not upgraded: 1.18.1 does not
contain a change that can explain or reclaim this route-return high-water mark.

The source boundary remains important:

- non-zero rounded clips use a render layer, while rectangular clips use a
  scissor path;
- component unregister clears per-item caches, but merely hiding an ancestor
  does not visit and release every prior child layer;
- Rust `Drop`/deallocation and FemtoVG texture deletion do not promise that the
  system allocator or macOS immediately lowers process RSS.

These facts support the measured graphics mechanism, but they do not authorize
destroying stable route state or resetting the renderer.

## Decision and next boundary

No individual clean owner passed the 5 MiB two-round screening rule. The only
combined Settings layer candidate already received a six-process Batch B A/B
and failed the stricter product adoption gate. Adding an estimated 0.31 MiB
disabled-control tweak to retry that candidate would be threshold gaming, not
a new architectural hypothesis.

Accordingly:

- do not change the stable Settings factory lifecycle;
- do not change the single lifecycle-owned Tooltip host;
- do not simplify rounded cards that do not create a clip layer;
- do not clear Hook state or transiently reset the renderer;
- do not rerun the rejected Batch B bundle unchanged;
- do not promote Batch C attribution runs as product gains.

Further Settings work should wait for either a stable symmetric footprint
environment plus stronger active-section checkpoints, or an upstream renderer
change that exposes/reclaims a separately measurable cache owner. The next
optimization batch should otherwise move to a different page whose clean owner
already exceeds the screen, rather than accumulating sub-threshold Settings
tweaks.

## Validation and evidence

Tool self-tests passed for every frozen scenario namespace and for the summary
math. Valid pairs retained exact PID/start identity, five samples per phase,
raw `ps`, `footprint`, categorized footprint, `vmmap`, step lists, and clean
exit. Invalid and interrupted runs carry explicit `invalid.json` markers and
are never merged.

- Protocol: `memory-benchmark-results/settings-batch-c-20260928/protocol.md`
- Provenance and frozen binary:
  `memory-benchmark-results/settings-batch-c-20260928/artifacts/`
- Valid raw pairs:
  `memory-benchmark-results/settings-batch-c-20260928/runs/`
- Machine-readable summaries:
  `memory-benchmark-results/settings-batch-c-20260928/*-attribution.json`

The evidence directory is intentionally ignored by Git. This report is the
tracked decision record.
