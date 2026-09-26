# Hook Dialog Memory Investigation (2026-09-26)

## Scope and status

This note records the Home -> Settings/Hooks -> Add/Edit -> scroll JS/TS ->
Home memory investigation. Every native run launched a frozen Release artifact
built from this repository. No run launched `/Applications/Usher.app` or another
installed Usher binary.

The root cause is confirmed and the final quantitative Release acceptance is
complete. The application replaces application-owned target-sized live shadows
with centralized fixed-source elevation profiles and removes rounded clipping
around the Hook document tree. Native visual acceptance is recorded below.

## Controlled baseline

The protocol separates three arms in fresh processes:

- `L`: visit the Hooks list and return Home.
- `E`: open the editor without scrolling, close it, and return Home.
- `S`: open the editor, scroll the JS/TS source, close it, and return Home.

The original source-built Release measured:

| Arm | Home start RSS | Settled Home RSS | Start-to-Home | Malloc Large after editor |
| --- | ---: | ---: | ---: | ---: |
| L1 | 122.67 MiB | 131.69 MiB | +9.02 MiB | 2.203 MiB |
| E1 | 122.36 MiB | 150.97 MiB | +28.61 MiB | 9.047 MiB |
| S1 | 123.27 MiB | 143.75 MiB at 30 s | +20.48 MiB | 9.047 MiB at return |

Opening the editor added about 19.55 MiB beyond the Hooks-list arm. Scrolling
itself added only 0.97 MiB RSS and 1.25 MiB physical footprint. JS/TS source,
syntax runs, and scroll activity were therefore not plausible explanations for
the reported approximately 30 MiB jump.

## Rejected lifecycle-only candidate

The first candidate gave the editor a real conditional owner tied to Settings
route activity and Add/Edit session activity. It destroys the editor's
`ScrollView`, `TextInput`, `StyledText`, Timer, layout, and per-item graphics
owners on close or route exit. That ownership is correct and remains in the
application, but it did not lower the measured plateau:

| Arm | Candidate start-to-Home RSS | `Malloc Large` |
| --- | ---: | ---: |
| L3 | +9.27 MiB | 2.203 MiB |
| E3 | +28.77 MiB | 9.047 MiB |
| S3 | +29.08 MiB | 9.047 MiB |

This candidate was rejected as the memory fix. Component destruction clears
per-item renderer state, but it does not clear the renderer-wide box-shadow
cache. Hiding or destroying more feature UI cannot solve that cache owner.

## Root cause

`GitHubDialog` drew a 24px live shadow around an approximately 680x568 logical
Hook Dialog. At 2x scale the final RGBA shadow texture is:

```text
(680*2 + 2*24*2) * (568*2 + 2*24*2) * 4
= 7,175,168 bytes
= 6.842773 MiB
```

That byte count matches the measured `Malloc Large` increase from 2.203125 to
9.046875 MiB within measurement precision. FemtoVG also renders source,
horizontal-blur intermediate, and result textures, explaining the larger RSS
and graphics-dirty change. Slint's renderer-wide `BoxShadowCache` retains the
final image independently of an individual Dialog item's lifetime; item cache
and OpenGL allocator high-water behavior can retain more memory.

Setting only `drop-shadow-blur: 0px` is not a valid fix: a non-zero offset and
non-transparent color can still allocate a target-sized texture. The shared
Dialog must not enter the box-shadow path at all.

## Causal Release experiment

An isolated Release candidate removed all three `drop-shadow-*` declarations
from the shared Dialog and left the editor unchanged. Its SHA256 was
`81e20f785fb49c449268d971bb53c8f2bac0997cd17c3ebe37adc892f9ba4df3`.

| Arm/phase | RSS | Footprint | Graphics dirty | Malloc Large |
| --- | ---: | ---: | ---: | ---: |
| E start | 123.16 MiB | 90.63 MiB | 18.55 MiB | 2.20 MiB |
| E editor | 134.91 MiB | 103.20 MiB | 24.48 MiB | 2.20 MiB |
| E Home 10 s | 137.39 MiB | 106.34 MiB | 27.58 MiB | 2.20 MiB |
| S start | 122.34 MiB | 90.67 MiB | 18.69 MiB | 2.20 MiB |
| S editor | 134.03 MiB | 103.97 MiB | 25.55 MiB | 2.20 MiB |
| S scrolled | 134.48 MiB | 104.41 MiB | 25.55 MiB | 2.20 MiB |
| S Home 30 s | 136.92 MiB | 107.53 MiB | 28.64 MiB | 2.20 MiB |

Compared with E3/S3, settled Home RSS fell 14.31/15.20 MiB, footprint fell
16.22/16.25 MiB, graphics dirty fell 9.13/8.67 MiB, and the 6.84 MiB
`Malloc Large` jump disappeared. Scrolling itself added 0.45 MiB RSS and did not
change graphics dirty or `Malloc Large`.

Raw JSON, footprint reports, screenshots, logs, and executable identity records
are under
`memory-benchmark-results/hook-editor-scroll-20260925/causal-dialog-no-shadow-20260926/`.

## Adopted architecture

- `GitHubDialogElevation` owns one generated 145x145 PNG alpha mask.
- `nine-slice(72 72 72 72)` stretches the center and edges without creating a
  Dialog-sized source texture.
- `Image.colorize` applies `GitHubTheme.shadow-floating-dialog`; source plus
  colorized RGBA textures are about 164 KiB and independent of Dialog geometry.
- The elevation sibling owns no input, accessibility, layout, `Layer`, opacity,
  clipping, tiling, or live box shadow. Narrow fullscreen Dialogs omit it.
- The Hook frame keeps its radius, border, background, and focus state, while
  the inner ScrollView performs rectangular clipping.
- The route/session editor owner remains because it correctly bounds text and
  layout owners, even though it was not the primary RSS fix.

This is a shared boundary change. It covers all nine current `GitHubDialog`
call sites: Hook, Project, Update, enhanced generic Dialog, and five PR review
flows.

The same central module owns a 75x75 Floating profile with
`nine-slice(37 37 37 37)` and a 55x55 Compact profile with
`nine-slice(27 27 27 27)`. Each generated profile keeps a single center pixel
for stretching and declares a minimum target size (40px Floating, 32px Compact),
below which elevation is omitted so fixed caps cannot overlap. They preserve the
ActionMenu and compact-surface elevation while keeping decoded source size
independent of each target.

## Repository audit and prevention

The repository-wide follow-up removed every application-owned raw
`drop-shadow-*` declaration. Dialogs use `GitHubDialogElevation`; ActionMenu uses
`GitHubFloatingElevation`; floating IconButtons, Toasts, Notifications, the PR
code-rendering indicator, and the compact navigation strip use
`GitHubCompactElevation`. Breadcrumb's shallow edge uses direct gradient paint,
and the unused floating Terminal shadow path was removed. There is no live-shadow
allowlist: `scripts/check-ui-policy.sh` rejects every raw declaration under
`ui/` and verifies the shared profiles, generated assets, all nine registered
`GitHubDialog` instances, the reviewed backdrop owners, and sole ownership of
`GitHubDialogElevation`. Named Dialog/Modal wrappers must structurally own one
shared Dialog shell.

Large rounded clips remain a separate renderer-risk inventory in Project list,
Agent Response code blocks, PR list, Job Run, and Terminal. They are not claimed
as leaks without route-specific Release A/B. The confirmed Hook document clip
was removed in this batch; the remaining owners need independent fixtures before
changing visuals or interaction behavior.

The policy tests cover:

- all three fixed-source elevation profiles and generated-asset freshness;
- repository-wide rejection of every raw `drop-shadow-*` under `ui/`;
- exact expected shared-profile consumers and the Breadcrumb direct-paint
  contract;
- the complete Dialog consumer/backdrop registry, named Dialog/Modal wrapper
  ownership, and rejection of direct Dialog elevation use outside the shell;
- route/session conditional ownership of `HookCodeEditor`;
- rejection of rounded clipping on the Hook document frame.

## Acceptance

The final architecture candidate was frozen as the repository-built Release
artifact with SHA-256
`e0df0388aa5f5a10f8f1dc2af02baddb61826a6272e6960e0ec729595a8034a6`.
The L2, E2, and S2 processes directly spawned the copied executable under the
repository-owned probe bundle; none launched `/Applications/Usher.app`. All
three processes exited normally, and the copied executable and frozen artifact
had the same SHA-256.

| Arm | Home start RSS | Settled Home RSS | Start-to-Home RSS | `Malloc Large` in every phase |
| --- | ---: | ---: | ---: | ---: |
| L2 | 122.39 MiB | 129.31 MiB at 10 s | +6.92 MiB | 2.203125 MiB |
| E2 | 122.59 MiB | 136.11 MiB at 10 s | +13.52 MiB | 2.203125 MiB |
| S2 | 123.17 MiB | 134.98 MiB at 30 s | +11.81 MiB | 2.203125 MiB |

The 6.842773 MiB `Malloc Large` allocation did not recur. Within S2, scrolling
changed RSS from 132.28125 to 132.56250 MiB (+0.28125 MiB) and physical
footprint from 99.78235 to 100.03235 MiB (+0.25 MiB), while graphics dirty and
`Malloc Large` remained unchanged. Using the immediate start-to-Home-return
contrast, S2 was 1.703125 MiB lower in RSS and 1.359375 MiB lower in footprint
than E2, so there is no retained scroll-specific increment. The final E2 and S2
settled RSS values were respectively 1.28125 and 1.93750 MiB below the isolated
no-shadow causal result.

`probe.mjs verify 2` passed process identity, artifact integrity, sample-count,
screenshot-integrity, scroll-change, reverse-scroll pixel-return, and
normal-exit checks. The memory thresholds were evaluated from `comparison.json`;
the verifier itself checks evidence integrity rather than acceptance thresholds.
Raw JSON, footprint reports, screenshots, logs, and executable identity records
are under `memory-benchmark-results/hook-editor-final-20260926/`.

The automated flow covers editor open, downward and reverse scrolling, Cancel,
and return Home. Its light/English screenshots show the Settings ActionMenu,
Hook list, Dialog, scrolled editor, closed Dialog, and returned Home without
clipping, overlap, or dark bands.

A separate source-built Release with the `ui-components` feature (SHA-256
`7238ebdb5dfce85f8b9b10f2d26f920fb2eff65a6c6e1237f7eaed9fcfc26ffa`)
was executed directly from an isolated probe bundle for dark/zh-CN acceptance.
It covered Toast, Notification Banner, notification overflow ActionMenu, Hook
Dialog, editor scrolling, Cancel/reopen scroll reset, Escape cancellation,
Terminal FAB, and Back-to-top FAB. No elevation cap overlap, clipping, dark
band, malformed edge, or interaction regression was observed; the process
exited normally. Screenshots are retained under
`/private/tmp/usher-elevation-visual-qa-20260926/screenshots/`.

After restoring default features, the final memory-accepted binary itself was
run directly with the repository's offline batch-66 fixture. Its
`GitHubCodeRenderingIndicator` was observed in the live `Rendering code` state
without clipping, overlap, or malformed elevation, and the process exited
normally. The executable hash was again
`e0df0388aa5f5a10f8f1dc2af02baddb61826a6272e6960e0ec729595a8034a6`.
No installed Usher binary was used for either visual run.

The macOS application cannot enter narrow-fullscreen Dialog mode because its
800px minimum window width is above the Dialog's 544px breakpoint. The compact
navigation strip is intentionally disabled on macOS. Those two platform/layout
combinations remain covered by the compiled contracts and policy tests rather
than native macOS screenshots.
