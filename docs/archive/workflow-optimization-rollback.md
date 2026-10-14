# Workflow optimization rollback record / Workflow 优化回滚记录

Status: historical, rejected, and rolled back on 2026-08-04. This document is
not a current architecture contract.

状态：历史记录；实验已于 2026-08-04 否决并回滚。本文不是当前架构契约。

## Requested checkpoint / 用户指定恢复点

The code was restored to the state immediately after the Settings page bottom
content spacing was reduced in response to “间距太大了点”. The later Workflow
compile-boundary experiment and all subsequent Workflow menu/tooltip work were
removed.

代码已恢复到用户反馈“间距太大了点”并完成 Settings 页面底部内容间距调整后的状态。
之后开始的 Workflow 编译边界实验，以及随后所有 Workflow 菜单和 Tooltip 改动均已撤销。

Retained work / 保留内容：

- development/test profile and artifact-size optimizations;
- UI validation and opt-in live preview scripts;
- production-disabled `ui-components` feature and its existing playground;
- dependency updates completed before the checkpoint;
- independently compiled Settings page, its single-scroll-owner fix, original
  canvas color, no horizontal scrolling, and the accepted smaller bottom space.

保留开发/测试 profile 与产物优化、UI 校验和可选 live preview、生产关闭的
`ui-components` feature、恢复点之前完成的依赖更新，以及 Settings 独立编译、单一
滚动所有者、原背景色、禁止横向滚动和最终较小底部间距。

Rolled-back work / 已撤销内容：

- `usher-ui-workflow`, `src/workflow_page.rs`, and the temporary Workflow model;
- Workflow `ComponentFactory` properties, callbacks, state mirroring, viewport,
  theme, scroll-offset, and overlay bridge protocol;
- route-local Workflow scroll/tooltip hosts introduced for that boundary;
- all later Workflow filter ordering, repository popup, icon, tooltip, and
  UI Components tooltip-demo edits;
- the later shared tooltip token/lease/anchor protocol experiments.

## Measurements / 对比数据

All timings below were measured on the same 10-core Apple M2 Pro with 32 GiB
RAM unless stated otherwise. They are historical observations, not promises.

| Stage | Generated root | Warm UI edit | Decision |
| --- | ---: | ---: | --- |
| Original 2026-08-03 regression baseline | 48,976,640 bytes / 523,456 lines | 66.7 s end-to-end; generated `usher-ui` compile 43.20 s | Root cause confirmed: one large serial generated crate |
| Repeater-reduced showcase | 44,968,820 bytes / 481,343 lines | 28.62 / 28.16 / 27.95 s; median 28.16 s | Retained |
| UI Components disabled by default | 39,131,311 bytes / 420,331 lines | Showcase edits do not invalidate default UI | Retained; -12.98% bytes vs feature-enabled root |
| Settings split | 34,969,310 bytes / 374,472 lines | 9.26 / 6.94 / 6.50 s; median 6.94 s | Retained; root bytes -10.64% |
| Workflow split experiment | 26,914,095 bytes / 286,604 lines; separate root 8,561,867 bytes / 94,364 lines | 6.84 / 6.14 / 5.90 s; median 6.14 s | Rejected and rolled back despite root bytes -23.04% |

Other measured trade-offs / 其他实测权衡：

- Removing the unused `cdylib` improved the warm Rust median from 5.12 s to
  3.92 s (23.4%); retained.
- Disabling debug information for generated `usher-ui` improved the warm UI
  median from 46.27 s to 41.61 s (10.1%) and reduced its rlib from 792,121,424
  to 482,158,224 bytes (39.1%); retained.
- Direct Tooltip anchor publication reduced generated output by only 10,672
  bytes (0.022%) and five item trees while expanding the global protocol; the
  original prototype and the later reintroduction were both removed.
- Live preview reloads avoided Cargo/rustc but increased RSS by roughly 300 MiB
  per reload in the diagnostic run, reaching about 1,011 MiB after two reloads;
  it remains a short-lived development tool.

## Problems encountered / 改动中遇到的问题

### Settings extraction and scrolling

1. The shell Flickable and the extracted Settings viewport both attempted to
   own wheel input. At different iterations the outer Settings container moved
   while inner content did not, or the scrollbar existed but could not reach
   the final content.
2. Adding padding at both shell and page levels created visible gutters on all
   sides. Allowing unconstrained content width introduced a horizontal scroll
   strip that the original page did not have.
3. A transparent extracted root exposed the shell's gray canvas instead of the
   original Settings background.
4. The shell's ordinary 4 px bottom reservation became an empty strip above the
   fixed status bar. Correcting it first removed all content breathing room;
   restoring the intended content bottom space then overshot and had to be
   reduced to the user-accepted size.
5. These defects showed that “a scrollbar is visible” is not acceptance. Top,
   bottom, wheel ownership, background, horizontal overflow, and compact/wide
   layouts all require separate inspection.

### Workflow compile-boundary experiment

1. `ComponentFactory` created a separate Slint global graph. Theme, callbacks,
   project state, PR list models, viewport dimensions, scroll offsets, Back to
   top, and transient-overlay dismissal all required explicit bridging.
2. The bounded PR list needed conversion between generated model types. This
   improved compile isolation but enlarged the ownership surface and made UI
   behavior harder to reason about.
3. Workflow's embedded terminal, full-window PR Review, page scrolling, and
   responsive filters did not share one simple viewport/overlay hierarchy after
   extraction. Compile success did not prove route behavior.
4. Slint 1.17 live preview could dynamically generate MainWindow but a factory
   expected a static `VWeak`; the independent build scripts had to ignore
   `SLINT_LIVE_PREVIEW` for their products.

### Workflow menu and Tooltip regressions

1. Tooltips were clipped, crossed separators, appeared at an old screen
   position after controls disappeared, or remained visible after menus closed.
2. Hovering one action repeatedly displayed another action's text, most often
   `More filters` or `Close menu`. Shared payload ownership and conditional
   component teardown were changed several times without a reliable visual
   acceptance gate.
3. Repository popup rows temporarily showed unrelated values/icons; copy,
   folder, close, selected-check, and kebab visuals were observed in the wrong
   contexts during iterations.
4. Responsive filter ordering and available width regressed: options were cut
   off, spacing grew, and the repository popup approached the right viewport
   edge too closely.
5. The attempted fixes accumulated token, lease, heartbeat timer, local surface,
   root portal, viewport clamp, and direct-anchor variants. This was excessive
   for the behavior and made failures harder to isolate.
6. The final always-mounted locator attempt affected component geometry/hit
   regions and still republished stale text during layout changes. Compilation
   and 179 passing unit tests did not detect the visual regression.

## Process failures and future gate / 过程问题与后续门禁

- The work continued after repeated user-visible failures instead of returning
  to the last accepted checkpoint early.
- Several handoffs claimed completion based on compile/tests while manual hover,
  menu-switching, compact-width, and popup-edge checks had not passed.
- A tiny Tooltip generated-size saving was allowed to influence runtime design
  even though the measured 0.022% reduction was immaterial.
- Workflow optimization is paused. Do not reintroduce a Workflow crate,
  ComponentFactory bridge, route-local tooltip protocol, or PR Review split
  without a new explicit user request and a reversible branch/checkpoint.
- Any future prototype must keep production code untouched until it passes a
  fixed visual matrix: wide/compact layouts, every filter order, repository
  popup, each icon/tooltip label pair, menu switching, scrolling, terminal,
  PR Review entry/return, and UI Components demonstrations.
