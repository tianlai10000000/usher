# Sidebar Motion A/B / 侧栏动画 A/B

Date / 日期: 2026-08-10

Decision / 结论: **adopt the display-linked closed-form spring / 采用显示帧同步的闭式弹簧**.

## Algorithm provenance / 算法来源

- [Slint's official animation documentation](https://docs.slint.dev/latest/docs/slint/guide/language/coding/animation/)
  defines `animation-tick()` as the clock for frame-synchronized custom animation.
- [Apple's animation guidance](https://developer.apple.com/videos/play/wwdc2024/10145/)
  describes retargeting a running spring while preserving continuous velocity
  instead of restarting or queueing it.
- The coefficient solver is adapted from Charm's production-used
  [Harmonica spring](https://github.com/charmbracelet/harmonica/blob/master/spring.go),
  a framework-independent closed-form damped harmonic oscillator based on Ryan
  Juckett's derivation. Usher retains source attribution next to the implementation.

- Slint 官方动画文档把 `animation-tick()` 定义为自定义逐帧动画的同步时钟；Apple
  动画指南要求重定向运行中的弹簧并保持连续速度，而不是重新启动或排队；系数求解器
  来自成熟的 Harmonica 闭式阻尼振子实现，源码旁保留归属说明。

## Controlled protocol / 受控协议

- One FemtoVG/OpenGL Release binary for both cells, SHA-256
  `3612bc6280bccc6b2edbc572dd84c999aa8fcec3757a87d00d6af48fc1dac2c2`.
- Baseline: the previous 16 ms `Timer` with a fixed-step Euler integrator.
- Candidate: Slint `animation-tick()` plus the actual render-frame delta and a
  closed-form damped harmonic oscillator. Spring stiffness and damping are
  unchanged, so the frame source/integrator is the only candidate variable.
- Three isolated Release processes per cell on `Mac14,9`, macOS `26.6.1`.
- Each process received 24 real mouse clicks, 50 ms apart, on the native macOS
  sidebar toggle. Every run observed all 24 target reversals.
- The existing rendering notifier captured timestamp, motion progress, target,
  and active state before every rendered frame. Trace collection is disabled
  unless `USHER_SIDEBAR_MOTION_TRACE` is set.

- 两组使用同一个 FemtoVG/OpenGL Release 二进制，SHA-256 如上。
- 基线是原有 16ms `Timer` 与固定步长欧拉积分；候选改为 Slint
  `animation-tick()`、真实渲染帧间隔和闭式阻尼振子。弹簧刚度与阻尼保持不变，
  因此唯一候选变量是帧源/积分器。
- 每组 3 个隔离 Release 进程；每个进程向 macOS 原生侧栏按钮发送 24 次、间隔
  50ms 的真实鼠标点击。6 次运行均识别到完整 24 次目标反转。
- 逐渲染帧记录时间戳、运动进度、目标与 Active 状态；未设置诊断环境变量时不收集轨迹。

## Median result / 中位数结果

| Metric / 指标 | Fixed timer / 固定定时器 | Display-linked / 显示帧同步 | Candidate change / 候选变化 |
| --- | ---: | ---: | ---: |
| Repeated active frames / Active 重复静止帧 | 50.43% | 0.00% | -50.43 pp (-100.0%) |
| Motion update median / 位移更新间隔中位数 | 16.66 ms | 8.33 ms | -8.33 ms (-50.0%) |
| Motion update p95 / 位移更新间隔 P95 | 17.66 ms | 9.33 ms | -8.33 ms (-47.2%) |
| Per-frame progress step p95 / 单帧位移 P95 | 0.0667 | 0.0322 | -51.8% |
| Maximum progress step / 最大单帧位移 | 0.1096 | 0.0550 | -49.9% |
| Settle after final click / 最后点击后收敛 | 698.84 ms | 656.91 ms | -41.93 ms (-6.0%) |
| Render frame interval p95 / 渲染帧间隔 P95 | 9.29 ms | 9.33 ms | +0.04 ms (+0.4%) |

The old timer still rendered at the display cadence while changing position
only every other frame. The candidate eliminates those repeated frames and
halves both the update interval and visible per-frame displacement. Its p95
render interval is effectively unchanged; one isolated maximum-frame outlier
was worse (`12.69 ms` versus `9.51 ms`), but it did not repeat at p95 and the
three candidate runs delivered the same 8.33 ms median cadence. Collapsed and
expanded Home end states were manually inspected after the stress run with no
geometry, title-bar, sidebar-content, or shutdown regression.

旧实现虽然仍按显示刷新率渲染，却只隔帧更新位置，因此约一半 Active 帧完全静止。
候选消除了重复帧，将有效位移更新时间与肉眼可见的单帧跳变同时减半。渲染帧间隔 P95
基本不变；候选出现一次孤立的最大帧间隔离群值（`12.69 ms` 对 `9.51 ms`），但没有在
P95 重复，且三个候选进程的中位刷新节拍均为 8.33ms。压力测试后已人工检查 Home 的
展开/收起最终态，未发现几何、标题栏、侧栏内容或关闭流程回退。

Raw traces and the complete per-run table are in
`sidebar-motion-benchmark-results/ab-display-link-20260810/` (Git-ignored).
Reproduce with `scripts/benchmark-sidebar-motion.sh`; set
`USHER_SIDEBAR_MOTION=timer` only to select the legacy A/B baseline.

原始轨迹及逐进程完整表位于
`sidebar-motion-benchmark-results/ab-display-link-20260810/`（Git 忽略）。使用
`scripts/benchmark-sidebar-motion.sh` 可复现；`USHER_SIDEBAR_MOTION=timer` 仅用于选择旧 A/B 基线。
