# Memory Optimization Backlog / 内存优化待办

This document records measured memory behavior and follow-up optimization work. It is an investigation backlog, not a stable product specification or a claim that every item should be implemented.

本文记录已测量的内存表现及后续优化任务。它是排查与实施待办，不属于稳定产品规范，也不代表其中每一项都应直接进入生产实现。

## Status / 状态

- Last measurement / 最近测量：2026-07-28
- Platform / 平台：macOS, Retina scale factor 2.0
- Build / 构建：debug, default FemtoVG/OpenGL renderer
- Route / 页面：Home
- Current decision / 当前结论：prioritize renderer and native-window A/B measurements; do not optimize small business models based on the idle sample

All numbers below describe this single controlled run. Release builds, other display scales, other routes, and large PR review sessions require separate measurements.

以下数字只描述本次受控运行。Release 构建、其他显示缩放、其他页面以及大型 PR Review 会话必须另行测量。

## Measured baseline / 实测基线

| Scenario / 场景 | RSS | Physical footprint |
| --- | ---: | ---: |
| Before showing the window / 窗口显示前 | about 75 MiB | about 19 MiB |
| Small window, settled / 小窗口稳定后 | about 139 MiB | about 68 MiB |
| Fullscreen / 全屏 | about 140 MiB | about 253 MiB |
| Exit fullscreen and return to a small window / 退出全屏并缩回小窗口 | about 141 MiB | about 243 MiB |
| Repeat the same resize/fullscreen cycle / 再做一次相同循环 | about 140 MiB | about 243 MiB |

The second cycle did not continue growing. In this run the behavior looked like a bounded renderer/window-surface high-water mark, not a per-cycle unbounded leak.

第二轮没有继续增长。本次测试更像渲染器或窗口 surface 的有界高水位保留，不像每轮缩放都会累积的无界泄漏。

The main footprint changes after fullscreen were:

- `IOSurface`: about 23 MiB to 160 MiB, an increase of about 137 MiB.
- Owned unmapped graphics memory: about 11 MiB to 49 MiB, an increase of about 38 MiB.
- Allocated malloc memory: about 11.1 MiB to 11.7 MiB, an increase of only about 0.6 MiB.
- Resident malloc memory stayed around 21 MiB, including about 10 MiB of fragmentation.
- The process used about 17–18 threads; resident thread stacks were below 1 MiB.

全屏后的主要 footprint 变化为：

- `IOSurface`：约 23 MiB 增至 160 MiB，增加约 137 MiB。
- 应用持有但未映射的图形内存：约 11 MiB 增至 49 MiB，增加约 38 MiB。
- malloc 实际分配：约 11.1 MiB 增至 11.7 MiB，仅增加约 0.6 MiB。
- malloc 驻留保持在约 21 MiB，其中约 10 MiB 为碎片。
- 进程约有 17–18 个线程，线程栈实际驻留低于 1 MiB。

`ps` RSS does not include all of the graphics-footprint growth seen by macOS. For optimization decisions, always record both RSS and physical footprint. Very large VSZ values are mostly address-space reservations and mappings and are not, by themselves, evidence of real memory pressure.

`ps` RSS 没有覆盖 macOS 统计到的全部图形 footprint 增长。后续优化必须同时记录 RSS 与 physical footprint。很大的 VSZ 通常来自地址空间预留和映射，不能单独作为真实内存压力证据。

## Backlog / 待办

### MEM-01 — Extend physical-footprint deltas / 补齐 physical-footprint 增量

Status / 状态：Backlog

`src/memory_diagnostics.rs` currently logs absolute physical footprint but calculates previous/baseline deltas only for RSS.

`src/memory_diagnostics.rs` 当前会记录 physical footprint 绝对值，但相对上次和启动基线的增量只针对 RSS。

Work / 工作：

- Add `phys_footprint_delta_last_mib` and `phys_footprint_delta_baseline_mib`.
- Retain both RSS and footprint peaks for one process lifetime.
- Correlate high-water changes with `[Memory][UI]` physical pixel dimensions.
- Keep the sampler cancellable and on the shared runtime.

Acceptance / 验收：

- A resize/fullscreen run shows RSS and footprint deltas in the same sample.
- Logs identify the first event that establishes a new footprint high-water mark.
- Release diagnostics remain opt-in through `USHER_MEMORY_DIAGNOSTICS=1`.

### MEM-02 — Establish a repeatable release benchmark / 建立可重复的 Release 基准

Status / 状态：Backlog

Work / 工作：

- Extend or wrap `scripts/monitor-footprint.sh` to capture startup, settled small window, several fixed sizes, fullscreen, exit fullscreen, and a second identical cycle.
- Run at least three iterations per configuration and record median and maximum RSS/footprint.
- Record macOS version, hardware, display scale, renderer, build profile, route, and loaded model counts.
- Include Home, Workflow PR list, one representative large PR review, and Terminal-open scenarios.

Acceptance / 验收：

- One command produces comparable output for all renderer/window variants.
- The result distinguishes startup cost, steady-state cost, bounded high-water retention, and cumulative growth.
- Debug-only logging overhead is not used as the production baseline.

### MEM-03 — Renderer A/B comparison / 渲染器 A/B 对比

Status / 状态：Backlog

Compare the existing compile-time renderer variants:

- Default FemtoVG/OpenGL.
- FemtoVG/WGPU.
- Skia.

Work / 工作：

- Use the MEM-02 matrix without changing application models or test data.
- Record RSS, physical footprint, fullscreen high-water retention, launch time, resize smoothness, and visual regressions.
- Confirm the selected backend explicitly; do not infer it only from enabled Cargo features.

Acceptance / 验收：

- Results show the trade-off for both RSS and physical footprint.
- A production renderer change is made only if total memory, responsiveness, rendering correctness, and packaging are acceptable.
- Update `README.md`, `README.zh.md`, and both architecture documents if the production renderer decision changes.

Existing architecture notes say the WGPU path reduced physical footprint but increased RSS in earlier macOS tests. Treat that as a hypothesis to revalidate against the repeatable release benchmark, not as a final selection.

现有架构记录称，之前的 macOS 测试中 WGPU 降低了 physical footprint，但提高了 RSS。后续应使用可重复的 Release 基准重新验证，不能直接将其视为最终选择。

### MEM-04 — Native macOS window compositing A/B / 原生 macOS 窗口合成 A/B

Status / 状态：Backlog

The current macOS window enables a transparent titlebar and sets the native window to non-opaque in `src/lib.rs`.

当前 macOS 窗口在 `src/lib.rs` 中启用了透明标题栏，并将原生窗口设为 non-opaque。

Work / 工作：

- Compare the current configuration with an opaque-window variant while keeping renderer, dimensions, route, and content constant.
- Measure `IOSurface`, graphics footprint, total physical footprint, and RSS.
- Inspect titlebar, background, shadows, corners, fullscreen transitions, light/dark themes, and traffic-light controls.

Acceptance / 验收：

- Adopt an opaque configuration only when it produces a meaningful repeatable saving without unacceptable visual regressions.
- Keep all adaptation in application-owned code; do not modify vendored Material sources.

### MEM-05 — Isolate renderer surface retention / 隔离渲染 surface 保留

Status / 状态：Backlog

Work / 工作：

- Build a minimal Slint window reproducer with the same renderer and macOS window flags.
- Verify whether the high-water retention occurs without project models, routing, native tray integration, or feature controllers.
- Compare normal resize, fullscreen transition, display-scale change, and repeated cycles.
- If the minimal reproducer confirms framework/backend ownership, search for an upstream fix and prepare a focused upstream report.
- Do not recreate the GL context or renderer surface in production without measuring flicker, state loss, and teardown safety.

Acceptance / 验收：

- The retained memory is attributed to either application-owned window configuration, Slint/backend behavior, or a smaller reproducible interaction between them.
- Any workaround has an explicit regression test and benchmark result.

### MEM-06 — Tune Tokio worker count / 调整 Tokio worker 数量

Status / 状态：Backlog, lower priority / 较低优先级

The shared Tokio runtime currently uses the default multi-thread worker count. Startup measurements showed the process growing from about 5 to about 15 threads when the runtime-backed services initialized.

共享 Tokio runtime 当前使用默认多线程 worker 数。启动测量中，依赖 runtime 的服务初始化后，线程数从约 5 个增加至约 15 个。

Work / 工作：

- Benchmark explicit worker counts such as 2 and 4 under HTTP, project loading, Workflow, Terminal, update checks, and shutdown.
- Record responsiveness, queueing latency, CPU utilization, thread count, RSS, footprint, and shutdown behavior.

Acceptance / 验收：

- Reduce workers only when concurrent operations and UI responsiveness do not regress.
- Treat any memory saving as secondary; resident thread stacks were below 1 MiB in the baseline.

### MEM-07 — Evaluate malloc fragmentation / 评估 malloc 碎片

Status / 状态：Backlog, lower priority / 较低优先级

The baseline showed roughly 10 MiB of malloc fragmentation. This is much smaller than graphics-memory retention.

基线中 malloc 碎片约为 10 MiB，明显小于图形内存保留。

Work / 工作：

- Re-measure fragmentation in a release build after realistic navigation and PR review workloads.
- Identify large allocation owners before considering a different allocator.
- If comparing allocators, include binary size, startup time, CPU, RSS, footprint, and macOS compatibility.

Acceptance / 验收：

- Do not introduce another allocator without a repeatable material improvement.
- Do not attribute framework/Objective-C/graphics allocations to the Rust allocator without evidence.

### MEM-08 — Measure large PR review ownership / 测量大型 PR Review 占用

Status / 状态：Backlog

Idle Home measurements do not exercise PR review. Existing caches are bounded, but their retained values can still be large for a large diff.

Home 空闲测量没有覆盖 PR Review。现有缓存虽有容量上限，但大型 Diff 的单项数据仍可能很大。

Work / 工作：

- Use a fixed large PR fixture and the existing `[Workflow][PR Review][Memory]` events.
- Measure opening the review, selecting several files, syntax highlighting, expanding/collapsing context, full-file view, commit-range switching, route changes, and leaving the Workflow lifecycle domain.
- Confirm that collapsed copy-on-write overrides and out-of-domain review state are released.
- Record cache entry counts and retained bytes without logging source content or credentials.

Acceptance / 验收：

- Repeated file/range navigation stays within documented cache capacities.
- Leaving the Workflow lifecycle domain releases heavy review state.
- Optimize only allocations demonstrated by this scenario; do not infer them from the idle Home baseline.

## Items not currently worth prioritizing / 当前不优先处理

- Idle Home UI models: the measured run had 9 project rows, 10 tags, no PR rows, no review lines, and only a few KiB of model text.
- Project logos: raster images are already limited to 128×128 before conversion to Slint images.
- Thread stack residency: below 1 MiB in the measured process.
- VSZ: not a useful standalone optimization target.

These items should be reconsidered only when a representative scenario attributes material retained memory to them.

只有代表性场景证明这些项目持有了显著内存时，才重新提高其优先级。

## Suggested implementation batches / 建议实施批次

1. Measurement foundation / 测量基础：MEM-01, MEM-02.
2. Primary graphics investigation / 主要图形排查：MEM-03, MEM-04, MEM-05.
3. Feature workload validation / 功能负载验证：MEM-08.
4. Secondary runtime tuning / 次要运行时优化：MEM-06, MEM-07.

After completing an item, append the tested commit, command, environment, before/after table, decision, and any remaining risk. Do not replace the original baseline; keep later results clearly dated so regressions remain comparable.

每项完成后，应追加测试 commit、命令、环境、前后对比表、最终决策和剩余风险。不要覆盖原始基线；后续结果应明确标注日期，以便持续比较回归。

## Related references / 相关资料

- `src/memory_diagnostics.rs`: process RSS/footprint sampler.
- `scripts/monitor-footprint.sh`: macOS footprint/RSS and resize sampling.
- `src/lib.rs`: native macOS window configuration and startup checkpoints.
- `src/app_runtime.rs`: shared Tokio runtime.
- `src/workflow/pr_review.rs`: bounded PR review caches and memory diagnostics.
- `docs/architecture.md`, `docs/architecture.zh.md`: current renderer and ownership decisions.
- `docs/specs/pr-review.md`: stable PR review memory bounds.
