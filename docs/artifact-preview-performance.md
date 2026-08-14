# Artifact Preview Performance Report / 文件预览性能报告

Date: 2026-08-12. This report measures the implementation defined by
[`docs/specs/artifact-preview.md`](specs/artifact-preview.md). All latency and
memory claims below use Release builds; Debug runs were used only for visual QA.

日期：2026-08-12。本文量化
[`docs/specs/artifact-preview.md`](specs/artifact-preview.md) 对应实现。延迟和内存
结论全部来自 Release 构建；Debug 仅用于视觉验收。

## Environment / 环境

| Item | Value |
| --- | --- |
| Machine | Mac14,9, arm64 |
| OS | macOS 26.6.1 (25G76) |
| Display | 3024 x 1964 Retina; measured Slint scale factor 2.00 |
| Renderer | Winit + FemtoVG/OpenGL |
| Release profile | `opt-level=z`, LTO, one codegen unit, stripped, panic abort |
| Final binary SHA-256 | `da2a3f4df2934b29096ab6632e65a8b08dc899ea6f8ff499dc521b55730f0f45` |

Memory values are medians of the final three samples in each phase. Each state
uses three isolated processes, five samples per phase, and a 0.5-second sample
interval. `footprint(1)` supplies physical footprint; `ps` supplies RSS.

内存值取每阶段最后三个样本的中位数。每种状态使用三个隔离进程，每阶段五次采样，
间隔 0.5 秒；physical footprint 来自 `footprint(1)`，RSS 来自 `ps`。

## Installed Size / 安装体积

| Binary | Bytes | SHA-256 |
| --- | ---: | --- |
| Baseline before this feature | 20,524,096 | `d5a67faae7f93b7eed1dbdec9adcd1a7b6a423eab09bb2b3cd24dcfc164ba6d3` |
| Final candidate | 20,774,928 | `da2a3f4df2934b29096ab6632e65a8b08dc899ea6f8ff499dc521b55730f0f45` |
| Delta | **+250,832 bytes (+245.0 KiB, +1.22%)** | |

No Wry/WebView dependency or browser helper process was added. The size delta
contains the provider registry, `pulldown-cmark`, image/text adapters, native
Slint hosts, settings, and diagnostics.

未引入 Wry/WebView 或浏览器辅助进程。体积增量包含 Provider Registry、
`pulldown-cmark`、图片/文本适配器、Slint 原生 Host、设置与诊断代码。

## Docked Open/Close Transition A/B / 停靠面板开关动画 A/B

The same final Release binary ran three isolated processes per variant. Each
process closed and reopened the docked panel once with the fixed 256 KiB,
371-block Markdown fixture. Baseline paints the complete block tree while the
ancestor clip moves. Optimized retains the same model, suspends document
painting during the shell motion, and restores only visible delegates through
the shared exact-extent virtual-list facade.

同一最终 Release 二进制为每个 variant 运行 3 个隔离进程；每个进程使用固定 256KiB、
371-block Markdown fixture 执行一次关闭和一次开启。Baseline 在祖先 clip 移动时绘制
完整 block 树；Optimized 保留同一 model，在 Shell 移动期间暂停文档绘制，并通过 Slint
共享精确 extent 虚拟列表门面只恢复有界的可见/overscan delegate。

| Metric | Baseline | Optimized | Change |
| --- | ---: | ---: | ---: |
| Recorded frames / run, median | 4 | 108 | **+2600.0%** |
| Frame gap p50 | 238.90 ms | 8.33 ms | **-96.5%** |
| Frame gap p95 | 255.60 ms | 15.42 ms | **-94.0%** |
| Frame gap max | 259.44 ms | 37.80 ms | **-85.4%** |
| Render p50 | 122.42 ms | 2.34 ms | **-98.1%** |
| Render p95 | 134.57 ms | 4.13 ms | **-96.9%** |
| Render max | 139.96 ms | 29.57 ms | **-78.9%** |
| Frames over 33.33 ms | 100.00% | 0.32% | **-99.7%** |

Every trace retained exactly 371 blocks. The optimization changes presentation
only: it performs no provider prepare, parse, model replacement, or duplicate
payload allocation during either transition. Raw data is under
`artifact-preview-transition-benchmark-results/20260812T024919Z/`.

所有 trace 全程保持 371 blocks。优化只改变展示，不在动画期间执行 Provider prepare、
解析、model 替换或重复 payload 分配。原始数据位于
`artifact-preview-transition-benchmark-results/20260812T024919Z/`。

## Preview Resize Interaction A/B / 预览宽度调整 A/B

The same Release binary selected baseline/optimized behavior only through
`USHER_PREVIEW_PERF_VARIANT`. Each host/variant used three isolated processes
and the fixed 256 KiB Markdown fixture (371 blocks). Docked input was a real
divider drag (`+180/-180 px`, 160 steps per leg, 8 ms). Detached input used
continuous macOS native `AXSize` width commits (`920 -> 560 -> 920`, fixed outer
height, 80 steps per leg, 16 ms), avoiding WindowServer mouse-event coalescing
while exercising the same Slint window-width/render path.

同一 Release 二进制只通过 `USHER_PREVIEW_PERF_VARIANT` 切换 baseline/optimized。
每种 Host/variant 使用 3 个隔离进程和固定 256 KiB Markdown fixture（371 blocks）。
Docked 使用真实分割线拖动（每段 160 steps、8ms）；Detached 使用连续 macOS 原生
`AXSize` 宽度提交（`920 -> 560 -> 920`，外框高度固定，每段 80 steps、16ms），避免
WindowServer 合并合成鼠标事件，同时经过相同的 Slint 窗口宽度与渲染路径。

| Host | Metric | Baseline | Optimized | Change |
| --- | --- | ---: | ---: | ---: |
| Docked divider | Frame gap p50 | 261.27 ms | 10.06 ms | **-96.1%** |
| Docked divider | Frame gap p95 | 331.71 ms | 16.77 ms | **-94.9%** |
| Docked divider | Render p50 | 130.16 ms | 7.00 ms | **-94.6%** |
| Docked divider | Render p95 | 131.50 ms | 8.45 ms | **-93.6%** |
| Docked divider | Frames >33.33 ms | 100.00% | 0.78% | **-99.2%** |
| Detached window | Frame gap p50 | 127.52 ms | 16.32 ms | **-87.2%** |
| Detached window | Frame gap p95 | 236.30 ms | 22.87 ms | **-90.3%** |
| Detached window | Render p50 | 122.88 ms | 2.54 ms | **-97.9%** |
| Detached window | Render p95 | 125.02 ms | 2.94 ms | **-97.6%** |
| Detached window | Frames >33.33 ms | 100.00% | 0.00% | **-100.0%** |

All twelve traces retained exactly 371 blocks throughout the measured gesture;
resize did not replace or reparse the prepared model. The optimized path keeps
the complete document tree at its settled width while only the host bounds and
clip follow live input. Release applies the exact final width once.

12 份 trace 在完整手势期间始终保持 371 blocks，resize 没有替换或重新解析准备后的
model。优化路径在实时输入期间保持完整文档树的稳定布局宽度，只让 Host 边界和裁剪
跟随；松手后只应用一次精确最终宽度。

The docked optimized max frame gap remains 366.91 ms versus 385.63 ms baseline,
but its corresponding render maximum is 15.65 ms rather than 131.71 ms. The gap
is an idle settle-tail interval without required redraw, not renderer blocking.
Detached optimized max frame gap is 25.04 ms and max render is 9.64 ms. These
maxima are reported rather than averaged away.

Docked optimized 最大 frame gap 仍为 366.91ms，baseline 为 385.63ms，但对应的
render max 已从 131.71ms 降到 15.65ms；该 gap 是无需重绘的 settle-tail 空闲间隔，
不是 renderer 阻塞。Detached optimized frame gap max 为 25.04ms、render max 为
9.64ms。报告保留这些 max，不用平均值隐藏。

### Stable scrollbar extent follow-up / 稳定滚动范围复测

After replacing the incrementally estimated Slint `ListView.viewport_height`
with the shared exact Markdown extent, the resize benchmark was repeated with
the same Release binary protocol, three isolated processes per host/variant,
and the same 256 KiB, 371-block fixture. This checks that stable thumb geometry
and exact end-position mapping do not reintroduce resize-time layout work.

将 Slint `ListView.viewport_height` 的增量估算替换为 Markdown 共享精确 extent 后，
使用相同 Release 协议、每种 Host/variant 3 个隔离进程及同一个 256 KiB、371-block
夹具重新执行 resize A/B，以确认滑块高度稳定和精确终点映射不会重新引入 resize
期间的布局开销。

| Host | Metric | Baseline | Exact-extent optimized | Change |
| --- | --- | ---: | ---: | ---: |
| Docked divider | Render p95 | 130.42 ms | 7.94 ms | **-93.9%** |
| Docked divider | Frames >33.33 ms | 100.00% | 0.78% | **-99.2%** |
| Detached window | Render p95 | 124.19 ms | 2.75 ms | **-97.8%** |
| Detached window | Frames >33.33 ms | 100.00% | 0.00% | **-100.0%** |

All 12 traces retained exactly 371 blocks. Docked optimized max render was
18.96 ms; its 375.10 ms max frame gap was an idle settle-tail interval rather
than renderer work. Deterministic layout tests additionally verify that top,
middle, and end projections return one identical content extent, and that
25/50/75/100% thumb positions map to the same normalized document offsets with
the bottom clamp occurring only at 100%.

12 份 trace 始终保持 371 blocks。Docked optimized render max 为 18.96ms；
375.10ms 的 frame-gap max 属于无渲染任务的 settle-tail 空闲区间。确定性布局测试
另外验证 top/middle/end 投影返回完全相同的 content extent，并验证滑块位于
25/50/75/100% 时映射到相同归一化文档偏移，且只在 100% 位置触发底部 clamp。

## Preview Wheel Scroll A/B / 预览滚轮滚动 A/B

The same final Release binary ran three isolated processes per projection
policy with the fixed 256 KiB, 371-block Markdown fixture. Each process first
performed the same optimized docked-divider resize gesture and then sent 420
pixel-wheel events at 8 ms intervals inside Preview. Baseline rebuilt a Slint
row model for every scroll callback. Optimized keeps one stable `VecModel`,
reuses the community virtualizer's exact three-block overscan as a guard, and
incrementally removes/appends only boundary rows when crossing it.

同一最终 Release 二进制针对每种投影策略运行 3 个隔离进程，使用固定 256 KiB、
371-block Markdown 夹具。每个进程先执行同一个优化版右侧分割线 resize 手势，再在
Preview 内以 8ms 间隔发送 420 个像素滚轮事件。Baseline 每次滚动 callback 都重建
Slint 行 model；Optimized 保持一个稳定 `VecModel`，把社区 virtualizer 的精确
3-block overscan 直接作为安全区，跨区时只增量删除/追加边界行。

| Metric | Per-event model | Incremental model | Change |
| --- | ---: | ---: | ---: |
| Frame gap p50 | 42.66 ms | 14.25 ms | **-66.6%** |
| Frame gap p95 | 63.99 ms | 25.11 ms | **-60.8%** |
| Frame gap max | 74.77 ms | 27.64 ms | **-63.0%** |
| Render p50 | 20.70 ms | 10.62 ms | **-48.7%** |
| Render p95 | 31.45 ms | 18.98 ms | **-39.6%** |
| Render max | 32.46 ms | 19.62 ms | **-39.6%** |
| Frames >33.33 ms | 79.79% | 0.00% | **-100.0%** |
| Mounted rows max | 15 | 15 | **no increase** |
| Projection commits | 408 | 58 | **-85.8%** |

The resize phase used the identical optimized Preview and differed only in the
scroll projection policy. Render p95 was unchanged at 11.26 ms; frame-gap p95
changed from 16.78 ms to 16.76 ms. This isolates the wheel-scroll improvement
without hiding a resize regression. The benchmark uses an explicit diagnostic
instance-lock namespace, so it can run beside a developer-owned Usher process;
normal application launches retain the original global single-instance lock.

Resize 阶段两组都使用完全相同的优化 Preview，只切换滚动投影策略。render p95
保持 `11.26ms`，frame-gap p95 从 `16.78ms` 变为 `16.76ms`，因此滚动收益没有
掩盖 resize 回退。基准使用显式诊断实例锁 namespace，可与开发者自己的 Usher
进程并行；普通应用启动仍保持原来的全局单实例锁。

## CPU Microbenchmarks / CPU 微基准

### Provider preparation

Fixed Markdown source: 1,029,616 bytes, 15 Release runs.

- Median: **23.732 ms**
- P95: **43.911 ms**
- Raw microseconds:
  `23286, 23352, 23396, 23414, 23473, 23579, 23708, 23732, 23850, 23947, 24152, 24283, 25710, 29635, 43911`

### Chat projection

96 Markdown messages, 1,065,984 source bytes, 15 Release runs.

| Path | Median | P95 |
| --- | ---: | ---: |
| Cold projection | **27.087 ms** | **37.400 ms** |
| Warm bounded-cache hit | **1.737 ms** | **2.316 ms** |

Cold raw microseconds:
`26828, 26859, 26921, 26961, 26967, 27026, 27073, 27087, 27146, 27182, 27259, 27304, 27804, 30922, 37400`.

Warm raw microseconds:
`1637, 1665, 1675, 1700, 1700, 1707, 1728, 1737, 1751, 1768, 1770, 1944, 1973, 2028, 2316`.

## Chat Resize A/B / Chat Resize 对比

Three isolated baseline and candidate processes ran two cycles of 800x600,
1000x700, 1280x860, fullscreen, exit fullscreen, and final 800x600. The fixture
contained 20 mounted rows and one Markdown projection.

| Phase | Candidate minus baseline settled RSS |
| --- | ---: |
| Startup visible | -0.91 MiB (-0.8%) |
| 800x600, cycle 2 | -1.00 MiB (-0.8%) |
| 1000x700, cycle 2 | -1.06 MiB (-0.9%) |
| 1280x860, cycle 2 | -1.05 MiB (-0.8%) |
| Fullscreen, cycle 2 | -0.78 MiB (-0.6%) |
| Exit fullscreen, cycle 2 | -0.81 MiB (-0.7%) |
| Final 800x600 | -1.06 MiB (-0.9%) |

Candidate peak RSS was lower in every phase by 1.56-1.95 MiB (1.3-1.6%).
Physical footprint was noisy and mixed: settled deltas ranged from -5.33 MiB to
+56.39 MiB. Therefore this report makes no physical-footprint improvement claim
for resize. Application counters remained at `chat_model_commits=2` and
`chat_markdown_projections=1` after initial stabilization through every resize
and fullscreen transition. Width-only resize performed no projection or model
commit.

候选版所有阶段的峰值 RSS 均降低 1.56-1.95 MiB（1.3-1.6%）。physical footprint
噪声较大且有升有降，settled 差值范围为 -5.33 MiB 到 +56.39 MiB，因此不声称
resize 的 physical footprint 得到改善。首次稳定后，所有 resize/fullscreen 阶段
计数器保持 `chat_model_commits=2`、`chat_markdown_projections=1`，宽度变化未重新投影
或提交消息 model。

## Preview State Memory / 预览状态内存

The preview fixture is 256 KiB Markdown, prepared as 371 blocks and an
approximately 800,726-byte payload. The table uses settled phase 2 so every
state has completed preparation and stabilization.

预览夹具为 256 KiB Markdown，准备后为 371 blocks、约 800,726 字节 payload。
下表使用 settled phase 2，确保所有状态均已完成准备和稳定。

| State | Settled RSS | Delta vs Chat | Settled footprint | Delta vs Chat |
| --- | ---: | ---: | ---: | ---: |
| Chat, never opened | 111.03 MiB | baseline | 79.64 MiB | baseline |
| Docked panel open | 118.78 MiB | **+7.75 MiB (+7.0%)** | 84.50 MiB | **+4.86 MiB (+6.1%)** |
| Detached window open | 121.94 MiB | **+10.91 MiB (+9.8%)** | 151.66 MiB | **+72.02 MiB (+90.4%)** |
| After docked close | 119.56 MiB | **+8.53 MiB (+7.7%)** | 84.72 MiB | **+5.08 MiB (+6.4%)** |

Detached minus docked is +3.16 MiB RSS and +67.16 MiB physical footprint. The large
physical delta is the cost of a second native 920x720 Retina window and its
compositor backing surfaces, not a second prepared payload: diagnostics reported
exactly one Host and the same 371-block payload in every detached run.

独立窗口相对右侧面板的 RSS 增加 3.16 MiB，physical footprint 增加 67.16 MiB，
主要来自第二个 920x720 Retina 原生窗口及合成 backing
surface，并非重复 payload：所有 detached run 都只报告一个 Host 和同一份
371-block payload。

Closing clears feature-session ownership immediately. The read-only Slint visual
projection may remain for the bounded 280ms exit transition and is then cleared;
all three settled close runs reported
`preview_host=none`, `preview_host_count=0`, `preview_blocks=0`, and
`preview_payload_bytes=0`. RSS and physical footprint did **not** immediately
return to the never-opened control because the process allocator and renderer
retain their high-water pages. The two post-close settled phases changed by only
-0.02 MiB, so there was no continuing growth during the observation window, but
the retained 8.53 MiB RSS / 5.08 MiB footprint is a real residual cost and not
claimed as fully released OS memory.

关闭会立即清除 Feature Session 所有权；只读 Slint 视觉投影可为退出动画保留最多
280ms，随后清除。三次稳定后的 close run 都为 `host=none`、`host_count=0`、
`blocks=0`、`payload_bytes=0`。但 allocator 与 renderer 保留高水位页面，RSS 和
footprint 没有立即回到“从未打开”控制组。关闭后的两个 settled 阶段仅变化
-0.02 MiB，观察期内没有持续增长；不过残留的 8.53 MiB RSS / 5.08 MiB footprint
是真实成本，不能表述为操作系统内存已完全释放。

## Reproduction / 复现

```sh
cargo test --release -p usher-preview --locked provider_prepare_release_benchmark -- --ignored --nocapture
cargo test --release --lib --locked chat_projection_release_benchmark -- --ignored --nocapture

scripts/benchmark-memory.sh \
  --runs 3 \
  --scenarios chat-preview-docked,chat-preview-detached,chat-preview-after-close \
  --protocol static \
  --route Chat \
  --fixture artifact-preview-markdown-256k-371-blocks-v1 \
  --prepare-hook scripts/prepare-chat-preview-memory-benchmark.sh \
  --prepare-delay 2 --samples 5 --interval 0.5 \
  --application-variant preview-candidate-codex-pane-final \
  --binary target/release/usher --no-build \
  --output memory-benchmark-results/artifact-preview-codex-pane-states

scripts/benchmark-artifact-preview-interaction.sh --runs 3 --no-build
scripts/benchmark-artifact-preview-scroll.sh --runs 3 --no-build
bash scripts/benchmark-artifact-preview-transition.sh 3
```

Raw local result roots:

- Chat A/B: `memory-benchmark-results/artifact-preview-ab/20260811T020644Z-preview-candidate-final-femtovg-normal-chat-resize-settled/`
- Preview states: `memory-benchmark-results/artifact-preview-virtualized-final/20260812T042534Z-preview-candidate-virtualized-transition-final-femtovg-normal-chat-preview-docked_chat-preview-detached_chat-preview-after-close/`
- Chat control: `memory-benchmark-results/artifact-preview-virtualized-control/20260812T042902Z-preview-candidate-virtualized-transition-control-femtovg-normal-chat-preview-control/`
- Preview resize A/B: `artifact-preview-interaction-benchmark-results/20260812T043154Z/`
- Docked transition A/B: `artifact-preview-transition-benchmark-results/20260812T024919Z/`

Visual QA also covered the top-aligned docked pane and continuous leading
hairline, 800x600 overlay and 1280x860 reserved-width layouts, a frame captured
about 80 ms into the 220 ms right-edge close transition, the fully reclaimed
Chat width after the transition, and the detached native window. Benchmark-only
fixtures are activated only when `USHER_MEMORY_DIAGNOSTICS=1` and an exact
preview scenario name is present; they do not persist presentation mode or
provider settings.
