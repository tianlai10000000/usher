# Chat Interaction Performance A/B

## Outcome

The accepted Chat path passes the same-binary, three-run-per-variant release
gate for consecutive near-limit messages, continuous width resize, and a full
top-and-back history scroll. Every optimized run met all acceptance limits:

- commit-adjacent frame-gap max: `29.59 / 33.78 / 32.56 ms` (`<= 50 ms`);
- stable virtual blank max: `0 / 0 / 0 px` (`<= 1 px`);
- commits in the 800 ms after scrolling stops: `0 / 0 / 0`;
- mounted-row max: `13 / 13 / 13` (`<= 32`);
- startup bottom-distance max across the initial size settle: `0 / 0 / 0 px`;
- drag tracking/reverse/post-release jump: `0 / 0 / 0 px` in every run;
- wheel reverse jump: `0 / 0 / 0 px`, backed by `394 / 390 / 383`
  Flickable direction samples.

Commit-adjacent p95 fell from `1729.71 ms` to `21.69 ms` (`-98.7%`), and its
maximum fell from `1733.90 ms` to `32.56 ms` (`-98.1%`). Render p95 fell from
`8.19 ms` to `4.27 ms` (`-47.9%`), and render max fell from `13.89 ms` to
`6.37 ms` (`-54.2%`). Resize frame-gap p95 fell from `298.30 ms` to `21.03 ms`
(`-92.9%`), with no optimized resize frame over `33.33 ms`.

The optimized scroll frame-gap p95 is slightly higher than baseline (`20.95 ms`
versus `18.39 ms`) because the bounded model performs frequent small commits while the
baseline stalls on a few complete 56-row replacements. The accepted result
prioritizes the user-visible worst case: baseline produced 1.7-second stalls
and exposed up to 680 px of stable blank, while optimized stayed below 50 ms
for every commit-adjacent frame, exposed no blank, and recorded no reverse
movement for either direct input path.

## Accepted Design

- One route-owned `VecModel` keeps its identity; window changes mutate only
  non-overlapping prefix/suffix rows.
- The product window mounts at most 32 single-role rows. A normal scroll commit
  introduces at most two new rows, and same-event-loop requests are coalesced
  before the latest viewport position is rendered.
- A 2,400 px mounted-edge guard moves the window before the viewport reaches a
  spacer. The width-keyed prefix is computed before scrolling from the bounded
  projection with Parley/Slint-equivalent text shaping and complete bubble
  section/spacing extents, so document height does not feed back from mounted rows.
- Normal optimized window shifts never write viewport anchor corrections;
  Flickable is the sole wheel/touchpad position owner. History prepend and
  settled resize retain exact stable-ID correction.
- Assistant inline projection is capped at 1,200 Unicode scalar values; the
  complete response remains available through Artifact Preview.
- Programmatic bottom navigation pins the exact Flickable bottom for at most
  800 ms while initial measurements settle. Wheel input or scrollbar pointer
  input cancels the pin immediately. A resize that starts at the bottom keeps
  that intent; a resize elsewhere preserves a visible message anchor.
- macOS live resize keeps the complete message viewport at its last stable
  width and commits one final layout after interaction ends.
- Direct scrollbar drag freezes its maximum, thumb geometry, and pointer grab
  offset until release. Flickable supplies direction samples for wheel/touchpad
  continuity without a second scroll-position write path.

## Method

- Build: full-LTO `cargo build --release --locked`.
- Binary SHA-256: `f2206b969e8af9d597cb0abc9f43aa0f8b894226e47345a2ffdf82f5333f4f8c`.
- Source commit: `d6ad5c28efb4e2ec6df1f700859eb3ce2d2d73e2`; the worktree was dirty, so the
  binary hash is the authoritative build identity.
- Machine: Mac14,9, macOS 26.6.1 (25G76), `winit-femtovg`.
- Fixture: 180 messages with three consecutive near-limit Assistant rows per
  User row.
- Runs: three isolated baseline/optimized process pairs using one release
  binary.
- Resize: `1280x860 -> 800x860 -> 1280x860`, 80 events per leg at 8 ms.
- Scroll: `+320/-320 px`, 3,200 events per leg at 8 ms.
- Instrumentation: Slint `BeforeRendering`/`AfterRendering`, model mutation and
  mutation-to-render timings, changed-row counts, mounted rows, virtual blank,
  scroll position, exact Flickable bottom distance, frozen-thumb pointer drift,
  post-release position, anchor-write count, and Flickable wheel direction.

Raw traces and the generated report are at
`/tmp/usher-chat-final-ab-v1/20260814T024640Z/` on the benchmark machine.

## Median Results

| Phase | Metric | Baseline | Optimized | Change |
| --- | --- | ---: | ---: | ---: |
| Resize | Frame gap p95 | 298.30 ms | 21.03 ms | -92.9% |
| Resize | Frame gap max | 299.02 ms | 22.00 ms | -92.6% |
| Resize | Render p95 | 6.96 ms | 2.37 ms | -65.9% |
| Resize | Mounted rows max | 56 | 3 | -94.6% |
| Scroll | Frame gap p50 | 8.38 ms | 8.69 ms | +3.7% |
| Scroll | Frame gap p95 | 18.39 ms | 20.95 ms | +13.9% |
| Scroll | Render p95 | 8.19 ms | 4.27 ms | -47.9% |
| Scroll | Render max | 13.89 ms | 6.37 ms | -54.2% |
| Scroll | Commit-adjacent p95 | 1729.71 ms | 21.69 ms | -98.7% |
| Scroll | Commit-adjacent max | 1733.90 ms | 32.56 ms | -98.1% |
| Scroll | Mounted rows max | 56 | 13 | -76.8% |
| Scroll | Stable virtual blank max | 680 px | 0 px | -100.0% |
| Scroll | Post-stop commits (800 ms) | 0 | 0 | unchanged |
| Direct input | Drag reverse/post-release max | 0 px | 0 px | unchanged |
| Direct input | Wheel reverse max | 0 px | 0 px | unchanged |

## Reproduction

```sh
scripts/benchmark-chat-interaction.sh --runs 3
```

The script builds one release binary unless `--no-build` is supplied, runs the
interleaved A/B, rejects incomplete history traversal, and applies every
per-run gate before writing `summary.md`, metadata, logs, and TSV traces.
