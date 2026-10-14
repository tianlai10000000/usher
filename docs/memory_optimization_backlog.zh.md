# 内存优化待办

[English](memory_optimization_backlog.md) · [架构文档](architecture.zh.md) · [规格索引](specs/README.md)

本文记录已测量的内存表现及后续优化任务。它是排查与实施待办，不属于稳定产品规范，也不代表其中每一项都应直接进入生产实现。

## 状态

- 初始基线：2026-08-09；后续测量与决策日期见各条目及[已归档实验](archive/memory-experiments.zh.md)。
- 平台：macOS，Retina scale factor 2.0
- 构建：2026-08-09 静态渲染器 A/B 使用 Release；更早的窗口循环与 Projects 交互证据使用 Debug
- 页面：Home 静态渲染器 A/B；更早的 Home 窗口循环与 Projects 交互对照
- 初始静态结论：WGPU 明显降低静态 Home footprint，并小幅降低 RSS；完整窗口循环、视觉和响应性门槛通过前，不改变生产渲染器

以下数字只描述本次受控运行。Release 构建、其他显示缩放、其他页面以及大型 PR Review 会话必须另行测量。

## 当前诊断与置信度

当前证据支持三个应分开处理的结论：

1. **已确认、高置信度：** 全屏后的最大增量来自 macOS 图形 physical footprint，尤其是 `IOSurface`；Rust malloc 分配变化很小。第二个相同循环已进入平台期，因此本次结果不能证明存在无界泄漏。
2. **已确认、中置信度：** Projects 只挂载当前展示形态后，稳定 RSS 下降约 2.2 MiB；收益真实，但远小于图形高水位。侧栏和原生缩放仍有明显 CPU 采样成本。
3. **静态风险、尚未实测：** PR Review、Chat 和 Terminal 的 Slint 展示边界已有上限，但部分 Rust 所有数据仍可能是“大单项”或随用户主动加载增长。调整缓存容量或生命周期前，必须先按实际负载归因。

### 内存指标边界

- **RSS** 表示当前驻留的映射页，是主要跨平台进程指标，但不能完整反映 macOS 图形压力。
- **Physical footprint** 是 macOS 上更重要的压力指标；Renderer/Window 决策必须与 RSS 同时比较。
- Feature 诊断中的 **owned bytes** 用于解释应用归属，只是估算值，不能替代 RSS 或 footprint。
- **峰值减稳定值**区分瞬时峰值和保留量；**循环间稳定值斜率**区分有界高水位和累积增长。
- 2026-08-02 本地观察到的 Binary 大小及约 19 GiB `target/` 属于磁盘/构建问题，不是运行时 RSS。当前 debug 可执行文件约 325 MiB，并非已 Strip 的 Release 产物，不能据此判断 RSS。

### 静态所有权图

| 区域 | 现有边界 | 待确认问题 | 优先级 |
| --- | --- | --- | --- |
| 渲染器 + macOS 窗口 | 编译期选定渲染器；重复循环已平台化 | 137 MiB `IOSurface` 与 38 MiB 图形增量由哪些渲染器/窗口标志拥有？ | P0 |
| PR Review | 最多 64 行已转换的 Slint 行；高亮缓存 4 项；源码缓存 3 项；范围 Diff 缓存 4 项；有路由清理 | 条目数上限不能限制单个超大 Diff/高亮/源码项；需验证离开后释放与字节归属 | P1 |
| Workflow 可达性 | Router 在有界 Back/Forward 历史仍可返回时保留 Workflow/PR Review | 最后一个 Workflow 历史项被淘汰时，需检测真正的“可达→不可达”边沿；当前事件只依据紧邻上一页 | P1 |
| PR 列表分页 | 每页 10 条远程记录；项目/过滤器/状态变化时清空缓存 | `visited_pages` 会保留同一查询下访问过的每一页；需量化深度分页，并在确有必要时由生命周期所有者清理 | P2 |
| Chat | Slint 最多 56 行；Markdown 缓存 96 项且约 8 MiB；离开路由时清理 cache/UI | Rust `loaded_messages` 及其 O(n) layout prefix 会保留会话中用户主动加载的每一页 | 图形之后的 P1 |
| Terminal | `vt100` scrollback 1000 行；仅把 viewport 行投影到 Slint；切换项目/关闭时重置状态 | 在彩色长输出下量化 Parser、Rust 渲染快照与 Slint Span Model 的同时副本；并保持稳定 Drawer 生命周期 | P2 |
| Project Logo | 解码后栅格最大边长 128×128 | Thread-local Slint Image 缓存没有条目数上限；在增加淘汰策略前，先测量大量不同编辑/远程/base64 Logo key | P3 |
| Workflow Job | Tag 每步日志钳制为 32 KiB，Slint 日志行惰性物化 | 确认没有其他 Job 生产者绕过该上限；空闲态预期影响很低 | P2 |
| Tokio/Runtime | 单一共享多线程 Runtime；观测到的线程栈驻留低于 1 MiB | Worker 数量节省 vs I/O 延迟与关闭行为 | P3 |
| 依赖 | 渲染器已收窄，但仍保留 `tokio/full`、reqwest 默认 feature 与 `image` 默认编解码；当前依赖树同时包含 native TLS 与 rustls | Feature 裁剪可能改善二进制/启动映射，但 RSS 收益尚未证明 | P3 |

## 实测基线

| 场景 | RSS | Physical footprint |
| --- | ---: | ---: |
| 窗口显示前 | 约 75 MiB | 约 19 MiB |
| 小窗口稳定后 | 约 139 MiB | 约 68 MiB |
| 全屏 | 约 140 MiB | 约 253 MiB |
| 退出全屏并缩回小窗口 | 约 141 MiB | 约 243 MiB |
| 再做一次相同循环 | 约 140 MiB | 约 243 MiB |

第二轮没有继续增长。本次测试更像渲染器或窗口 surface 的有界高水位保留，不像每轮缩放都会累积的无界泄漏。

全屏后的主要 footprint 变化为：

- `IOSurface`：约 23 MiB 增至 160 MiB，增加约 137 MiB。
- 应用持有但未映射的图形内存：约 11 MiB 增至 49 MiB，增加约 38 MiB。
- malloc 实际分配：约 11.1 MiB 增至 11.7 MiB，仅增加约 0.6 MiB。
- malloc 驻留保持在约 21 MiB，其中约 10 MiB 为碎片。
- 进程约有 17–18 个线程，线程栈实际驻留低于 1 MiB。

`ps` RSS 没有覆盖 macOS 统计到的全部图形 footprint 增长。后续优化必须同时记录 RSS 与 physical footprint。很大的 VSZ 通常来自地址空间预留和映射，不能单独作为真实内存压力证据。

## 侧栏与原生缩放 A/B

2026-08-02 的 Projects 路由复测保留侧栏运动，但避免逐帧改变路由宽度：页面只采用一次目标宽度，Shell 表面跟随一个可中断弹簧。原生 Live Resize 明确冻结已挂载路由宽度直到拖拽结束。Projects 只挂载当前 List/Grid 表示，PR Review 同时冻结昂贵的内部 Diff 几何；独立的有界原生底面用于消除 GPU Resize 晚一帧时的壁纸色闪块。

受控环境使用固定 1000×700 Projects 窗口、6 秒预热、16 次等间隔侧栏切换或 8 次自动左边缘往返拖拽、100 ms RSS/CPU 采样，并为每个 Variant 独立运行 3 个进程。下表均为每个进程测量值的中位数。

| 侧栏切换方案 | CPU 采样均值 | CPU 采样峰值 | Peak RSS 增量 | 保留 RSS 增量 |
| --- | ---: | ---: | ---: | ---: |
| 一次宽度 + Shell 弹簧 | 51.54% | 65.4% | +2768 KiB | +2528 KiB |
| 原子固定态 + 单一项目树 + 有界原生底面 | 20.65% | 39.0% | +3472 KiB | +3264 KiB |

原子方案量化了 Shell 动画成本，但因为取消了必需的侧栏运动而被否决。只挂载当前 Project 表示的优化独立保留：稳定态绝对 RSS 中位数从约 236.7 MiB 降至 234.5 MiB，Project Model 计数不变。

| 左边缘拖拽方案 | CPU 采样均值 | CPU 采样峰值 | Peak RSS 增量 | 保留 RSS 增量 |
| --- | ---: | ---: | ---: | ---: |
| 冻结路由宽度至拖拽结束 | 55.88% | 74.2% | +2592 KiB | +2304 KiB |
| 未合并的实时路由布局 | 74.80% | 96.0% | +2096 KiB | +1712 KiB |
| 16ms 最新宽度合并 | 65.89% | 89.8% | +1792 KiB | +1424 KiB |

相对未合并实时布局，16ms 方案的采样 CPU 均值下降 11.9%，RSS 峰值增量下降 14.5%，保留增量下降 16.8%，但人工验证右侧内容仍闪，因此被否决。最终产品行为使用整段拖拽冻结路由宽度；Slint/原生不透明底面仍实时贴边，拖拽结束一次应用最终布局。PR Review 继续叠加既有内部稳定几何与 Token 简化。

## 待办

### MEM-01 — 补齐 physical-footprint 增量

状态：已于 2026-08-08 实现

`src/memory_diagnostics.rs` 现在会在同一样本中记录 RSS 与 physical footprint 相对上次/基线的增量、进程生命周期峰值、显式高水位事件、运行元数据，以及最近页面/窗口上下文。Release 诊断仍需显式开启，采样器仍可取消并复用共享 Runtime。

工作：

- 增加 `phys_footprint_delta_last_mib` 与 `phys_footprint_delta_baseline_mib`。
- 在一个进程生命周期内同时保留 RSS 与 footprint 峰值。
- 将高水位变化与 `[Memory][UI]` 物理像素尺寸关联。
- 采样器保持可取消，并继续跑在共享 Runtime 上。

验收：

- 一次缩放/全屏运行中，同一采样同时显示 RSS 与 footprint 增量。
- 日志能标出首次建立新 footprint 高水位的事件。
- Release 诊断仍通过 `USHER_MEMORY_DIAGNOSTICS=1` 显式启用。

### MEM-02 — 建立可重复的 Release 基准

状态：基准框架已于 2026-08-08 实现；代表性 fixture 实跑待完成

`scripts/benchmark-memory.sh` 现在会构建指定 Release 渲染器，默认运行至少三个独立进程，执行固定双循环窗口/全屏协议，并保留原始 TSV、单轮/跨轮汇总、环境元数据与未经改写的应用 stdout 日志。准备 hook 会收到 `PID RUN SCENARIO`，从而无需把自动化逻辑放进应用代码，也能安装固定功能 fixture。

工作：

- 扩展或包装 `scripts/monitor-footprint.sh`，覆盖启动、稳定小窗口、若干固定尺寸、全屏、退出全屏，以及第二次相同循环。
- 每种配置至少跑三轮，记录 RSS/footprint 的中位数与最大值。
- 记录 macOS 版本、硬件、显示缩放、渲染器、构建 profile、页面与已加载 model 计数。
- 覆盖 Home、Workflow PR 列表、一个代表性大型 PR Review，以及打开 Terminal 的场景。

验收：

- 一条命令能为所有渲染器/窗口变体产出可比较结果。
- 结果能区分启动成本、稳态成本、有界高水位保留与累积增长。
- 不以 Debug 专用日志开销作为生产基线。

### MEM-03 — 渲染器 A/B 对比

状态：已完成完整 Home 窗口循环 A/B；WGPU 候选已拒绝（2026-08-09）

受控 Release A/B 使用相同 Home fixture、800×600 逻辑像素、2.0 scale factor、每阶段 5 个样本，以及每个渲染器 3 个独立进程。候选为 FemtoVG/WGPU，并包含其必需的 WGPU/Naga 第三方 Debug 日志过滤；默认 FemtoVG/OpenGL 不会加载这些 target。下表均为 `candidate - baseline`，负值表示候选减少占用。

| 静态 Home 指标 | FemtoVG/OpenGL 基线 | FemtoVG/WGPU 候选 | Candidate − baseline |
| --- | ---: | ---: | ---: |
| 第一稳定段 RSS 中位数 | 104.88 MiB | 100.98 MiB | **−3.89 MiB（−3.7%）** |
| 第一稳定段 footprint 中位数 | 76.91 MiB | 27.06 MiB | **−49.84 MiB（−64.8%）** |
| 第一稳定段 RSS 实测最大峰值 | 105.36 MiB | 101.58 MiB | **−3.78 MiB（−3.6%）** |
| 第一稳定段 footprint 实测最大峰值 | 77.41 MiB | 27.64 MiB | **−49.77 MiB（−64.3%）** |
| 窗口刚显示时 footprint 峰值 | 90.05 MiB | 27.69 MiB | **−62.36 MiB（−69.3%）** |

第二稳定段相对第一段，OpenGL 的 RSS/footprint 分别变化 −0.34/−0.34 MiB，WGPU 分别变化 −0.02/−0.02 MiB；两个静态方案均未出现正向累积。三轮 WGPU 的稳定 RSS 与 footprint 都低于三轮 OpenGL。

上面的静态结果作为历史证据保留，但最终决策以下面的完整窗口循环为准。应用自有的 signal-file 基准控制已经消除了此前的辅助功能权限阻塞。

最终结论：**拒绝 WGPU，生产继续使用 FemtoVG/OpenGL**。已验证的 Home `window-cycle` 使用相同源码指纹与 fixture、2.00 scale factor、`800x600` / `1000x700` / `1280x860` 尺寸、经确认的 `1800x1130` 全屏尺寸、每阶段 5 个样本，以及每个渲染器 3 个隔离 Release 进程（各 180 个原始样本）。

| 代表性窗口循环指标 | FemtoVG/OpenGL | FemtoVG/WGPU | WGPU - OpenGL |
| --- | ---: | ---: | ---: |
| 启动稳定 RSS | 110.97 MiB | 131.39 MiB | **+20.42 MiB（+18.4%）** |
| 启动 RSS 最大峰值 | 111.31 MiB | 132.41 MiB | **+21.09 MiB（+19.0%）** |
| 启动 footprint 最大峰值 | 96.52 MiB | 193.67 MiB | **+97.16 MiB（+100.7%）** |
| 最终 800x600 稳定 RSS | 113.03 MiB | 133.83 MiB | **+20.80 MiB（+18.4%）** |
| 最终 800x600 稳定 footprint | 120.13 MiB | 79.34 MiB | -40.78 MiB（-33.9%） |
| 第 2 轮全屏稳定 RSS | 113.05 MiB | 133.86 MiB | **+20.81 MiB（+18.4%）** |
| 第 2 轮全屏稳定 footprint | 247.36 MiB | 168.80 MiB | -78.56 MiB（-31.8%） |
| 全屏第 2 轮减第 1 轮 footprint 斜率 | +0.53 MiB | +15.02 MiB | **+14.49 MiB** |

WGPU 在大多数缩放/全屏阶段降低了稳定 physical footprint，但所有稳定 RSS 阶段都回退约 20--21 MiB，启动 footprint 峰值翻倍，重复全屏后的 footprint 保留也更差。三个 WGPU 进程均完成协议；其中一次进入全屏时 AppKit 首次请求超时，依靠基准驱动的有限重试才完成。由于结果明显互有得失，并且未通过 RSS、启动峰值与重复全屏门禁，本轮没有修改默认渲染器或任何生产行为。量化结论已经拒绝候选，因此未继续执行视觉、深色主题与打包门禁。原始数据及完整 `candidate - baseline` 表位于 `memory-benchmark-results/batch1-renderer-window-cycle-20260809/opengl-vs-wgpu/`（Git 忽略）。

对比现有编译期渲染器变体：

- 默认 FemtoVG/OpenGL。
- FemtoVG/WGPU。
- Skia。

工作：

- 使用 MEM-02 矩阵，不改应用模型或测试数据。
- 记录 RSS、physical footprint、全屏高水位保留、启动时间、缩放流畅度与视觉回归。
- 明确确认实际选中的后端；不要只根据启用的 Cargo feature 推断。

验收：

- 结果同时给出 RSS 与 physical footprint 的权衡。
- 只有在总内存、响应性、渲染正确性与打包均可接受时，才改变生产渲染器。
- 若生产渲染器决策变化，同步更新 `README.md`、`README.zh.md` 与两份架构文档。

现有架构记录称，之前的 macOS 测试中 WGPU 降低了 physical footprint，但提高了 RSS。后续应使用可重复的 Release 基准重新验证，不能直接将其视为最终选择。

### MEM-04 — 原生 macOS 窗口合成 A/B

状态：A/B 已完成；opaque 候选已拒绝（2026-08-09）

当前 macOS 窗口在 `src/platform/macos.rs` 中启用了透明标题栏，并将原生窗口设为 non-opaque。

工作：

- 在渲染器、尺寸、页面与内容不变的前提下，对比当前配置与 opaque 窗口变体。
- 测量 `IOSurface`、图形 footprint、总 physical footprint 与 RSS。
- 检查标题栏、背景、阴影、圆角、全屏过渡、浅色/深色主题与红黄绿按钮。

验收：

- 仅当 opaque 配置带来可重复的显著节省，且没有不可接受的视觉回归时，才采用。
- 所有适配留在应用自有代码中；禁止修改 vendored Material 源码。

结果（2026-08-09）：**拒绝 opaque，生产默认继续使用 transparent/non-opaque**。

已验证的 Home `window-cycle` A/B 使用同一个 FemtoVG/OpenGL Release 二进制、相同 fixture、2.00 scale factor、`800x600` / `1000x700` / `1280x860` 尺寸，以及经确认的 `1800x1130` 原生全屏尺寸。每个对比单元包含 3 个隔离进程和 180 个 RSS/footprint 原始样本。opaque 单元从两次参数完全相同的调用中合并了 3 个完整隔离 run，来源 run ID 保留在候选元数据中。另有 2 个 opaque 进程因为 AppKit 忽略第二次进入全屏而作为失败证据保留，但未进入内存聚合。transparent 完成 3/3 个进程；opaque 完成 4/6，在该压力协议下进程失败率为 33.3%。

| 代表性指标 | Transparent | Opaque | Opaque - transparent |
| --- | ---: | ---: | ---: |
| 启动稳定 RSS | 110.73 MiB | 110.41 MiB | -0.33 MiB (-0.3%) |
| 最终 800x600 稳定 RSS | 112.58 MiB | 112.58 MiB | +0.00 MiB (+0.0%) |
| 最终 800x600 稳定 footprint | 120.19 MiB | 119.25 MiB | -0.94 MiB (-0.8%) |
| 第 1 轮 large 稳定 footprint | 132.50 MiB | 143.13 MiB | +10.62 MiB (+8.0%) |
| 第 2 轮 fullscreen 稳定 footprint | 248.02 MiB | 246.45 MiB | -1.56 MiB (-0.6%) |
| 第 2 轮 fullscreen RSS 最大峰值 | 112.70 MiB | 112.59 MiB | -0.11 MiB (-0.1%) |

候选的稳定 RSS 节省在最终小窗口完全消失，physical footprint 指标互有得失，重复全屏行为还发生回退。第 2 轮减第 1 轮的 RSS 斜率也基本不变（退出全屏后 `+0.20` 对 `+0.19` MiB；全屏内 `+0.36` 对 `+0.41` MiB）。浅色全屏视觉检查未发现明显标题栏或内容损坏，但在内存与功能结论已拒绝后，没有继续执行深色主题与打包门禁。原始数据及完整的 `candidate - baseline` 表位于 `memory-benchmark-results/ab-window-compositing-window-cycle-20260809-validated/transparent-vs-opaque-final/`（Git 忽略）。

### MEM-05 — 隔离渲染 surface 保留

状态：证据不足；仅用于基准的复现器已撤回（2026-08-09）

工作：

- 将现有 `dpi-flicker-repro/` 改造成最小内存复现器；若改动会混入 DPI 与内存变量，则另建隔离用例。使用相同渲染器与 macOS 窗口标志。
- 验证高水位保留是否在没有 Project 模型、路由、原生托盘集成或功能控制器时也会发生。
- 对比普通缩放、全屏过渡、显示缩放变化与重复循环。
- 若最小复现器确认是框架/后端归属，检索上游修复并准备聚焦的上游报告。
- 未测量闪烁、状态丢失与销毁安全性前，不要在生产中重建 GL context 或渲染 surface。

验收：

- 能将保留内存归因到应用窗口配置、Slint/后端行为，或其更小的可复现交互。
- 任何 workaround 都必须有明确回归测试与基准结果。

结果（2026-08-09）：**证据不足，未采用任何生产优化**。一个仅用于基准的最小 Slint/FemtoVG surface 在不包含 Project model、Router、Tray 集成和 Feature controller 的情况下复现了相同的两轮尺寸/全屏协议。其最终有效单元完成 3/3 个隔离 Release 进程并保留 180 个原始样本。最小进程的全屏循环 RSS 基本持平（`+0.03 MiB`），但稳定全屏 physical-footprint 斜率为 `+18.06 MiB`，说明更小的纯 surface 交互也可能保留图形 footprint。

但是，相同最终源码指纹和协议下的完整 Usher 单元未达到强制三进程门槛。4 个尝试进程中只有 1 个完整完成；一个失败于重复进入全屏，两个失败于外部 `800x600` 尺寸确认，尝试进程失败率为 75%。由于不存在有效的三进程基线，不得把最小复现器数字描述成 `candidate - baseline` 节省或最终归属结论。按照回退原则，已删除仅用于基准的复现器，本轮没有留下 GL context、renderer surface、窗口或其他生产行为改动。通用的 CoreGraphics 全屏尺寸证据和应用变体单变量校验保留在基准基础设施中。成功与失败的原始证据位于 `memory-benchmark-results/mem05-surface-ownership-20260809/`（Git 忽略）。

### MEM-06 — 调整 Tokio worker 数量

状态：Release A/B 后已采用生产默认 2 个 worker（2026-08-10）

共享 Tokio runtime 当前使用默认多线程 worker 数。启动测量中，依赖 runtime 的服务初始化后，线程数从约 5 个增加至约 15 个。

工作：

- 在 HTTP、项目加载、Workflow、Terminal、更新检查与关闭场景下，对 worker 数如 2 和 4 做基准。
- 记录响应性、排队延迟、CPU 利用率、线程数、RSS、footprint 与关闭行为。

验收：

- 仅在并发操作与 UI 响应性不回退时减少 worker。
- 内存节省视为次要收益；基线中线程栈驻留低于 1 MiB。

结果（2026-08-10）：**采用 2 个 Tokio worker 作为生产默认，等待用户对代表性并发操作的验收**。受控静态 Home A/B 使用同一个 FemtoVG/OpenGL Release 二进制，并保持源码指纹、fixture、页面、采样参数和启动服务完全一致。每个单元完成 3 个隔离进程，每阶段 5 个样本。基线通过显式 `default` 恢复 Tokio 自动 worker 选择，候选唯一变化为 `2`。

| 静态 Home 指标 | Tokio default | 2 workers | 2 - default |
| --- | ---: | ---: | ---: |
| 第 1 稳定段 RSS | 111.23 MiB | 104.64 MiB | **-6.59 MiB（-5.9%）** |
| 第 2 稳定段 RSS | 110.98 MiB | 104.48 MiB | **-6.50 MiB（-5.9%）** |
| 第 1 稳定段 footprint | 79.80 MiB | 76.91 MiB | -2.89 MiB（-3.6%） |
| 稳定段 RSS 最大峰值 | 111.33 MiB | 105.59 MiB | **-5.73 MiB（-5.2%）** |
| 窗口刚显示时 footprint 峰值 | 97.49 MiB | 90.52 MiB | -6.97 MiB（-7.1%） |
| 第 2 段减第 1 段 RSS 斜率 | -0.02 MiB | -0.02 MiB | +0.00 MiB |

RSS 在每个稳定阶段都超过默认 5% 采用门槛，候选没有出现静态启动、关闭或循环斜率回退。生产环境在未设置 `USHER_TOKIO_WORKER_THREADS` 时现在选择 2 个 worker；`default` 仅作为显式基准复现开关。静态 fixture 覆盖启动 Project 加载、Chat bootstrap、更新/runtime 服务和有序关闭，但不能替代用户对并发 HTTP、Git、Workflow、Terminal、更新和关闭流程的验收；这些路径一旦出现可重复的延迟或正确性回退，就必须撤回。原始数据及完整表位于 `memory-benchmark-results/mem06-tokio-workers-20260810/default-vs-two/`（Git 忽略）。

### MEM-07 — 评估 malloc 碎片

状态：Backlog，较低优先级

基线中 malloc 碎片约为 10 MiB，明显小于图形内存保留。

工作：

- 在真实导航与 PR Review 负载后的 Release 构建中重新测量碎片。
- 在考虑更换分配器前，先识别大分配所有者。
- 若比较分配器，需包含二进制体积、启动时间、CPU、RSS、footprint 与 macOS 兼容性。

验收：

- 没有可重复的显著改善时，不引入另一套分配器。
- 没有证据时，不要把框架/Objective-C/图形分配归咎于 Rust 分配器。

### MEM-08 — 测量大型 PR Review 占用

状态：Backlog

Home 空闲测量没有覆盖 PR Review。现有缓存虽有容量上限，但大型 Diff 的单项数据仍可能很大。

工作：

- 使用固定大型 PR fixture 与现有 `[Workflow][PR Review][Memory]` 事件。
- 运行现有 `scripts/measure-pr-review-memory-ab.sh` 合成 A/B，作为可重复补充，不能替代真实 Provider fixture。
- 测量打开 Review、选择多个文件、语法高亮、展开/折叠上下文、全文件查看、提交范围切换、路由变化，以及离开 Workflow 生命周期域。
- 确认折叠后的 copy-on-write override 与离开域后的 Review 状态被释放。
- 记录缓存条目数与保留字节，但不记录源码内容或凭证。

验收：

- 反复文件/范围导航仍落在文档约定的缓存容量内。
- 离开 Workflow 生命周期域后，重型 Review 状态被释放。
- 只优化该场景证明的分配；不要从 Home 空闲基线推断。

### MEM-09 — 限制 Chat 已加载会话占用

状态：先测量

Chat UI 窗口和 Markdown Cache 已有边界，但 `ChatSessionService` 会在进程生命周期内保留用户主动加载的每一页历史；Virtual Layout Prefix 也为每条已加载消息保留一个 `f32`。这是用户操作驱动的增长，不是空闲泄漏证据。

工作：

- 增加已加载消息数、源文本字节、layout-prefix 容量、已挂载 Slint 行数与 render-cache 字节的诊断。
- 使用固定大小 fixture 内容，对启动 20 条，以及 100、1,000、10,000 条已加载消息做基准；记录加载前、加载后、离开 Chat 后、重新进入后的 RSS。
- 若保留的 Rust 归属显著，只保留有界内存历史窗口，同时继续以 JSONL 为事实来源并保持稳定分页锚点。不要只是缩小 56 行 Slint 窗口。

验收：

- Slint 行数仍不超过文档约定的 56 行窗口。
- 离开 Chat 后释放 UI/Markdown 分配；任何仍保留的 Rust session 字节都能在诊断中看见。
- 若引入 Rust 历史窗口，回看更早分页仍正确，反复分页不产生重复或丢失新持久化消息。

### MEM-10 — 归因 Terminal 多层副本

状态：先测量，较低优先级

Terminal Scrollback 已有上限，但一个可见帧可能同时存在于 `vt100` Parser、`TerminalDrawerState.lines`、Thread-local 渲染缓存和 Slint Span Model 中。它们是有界副本；没有长输出测量前，不应称为泄漏。

工作：

- 增加 Parser cell 容量、渲染行/span 字符串、缓存 UI 行与选择文本的 owned-byte 估算。
- 在窄/宽 viewport 下运行纯文本、ANSI 密集文本、宽字符文本与连续输出，直到达到 1000 行 scrollback 上限。
- 对比打开、关闭、同项目重开、切换项目与关闭进程。
- 只有完成归属后，才评估减少重复字符串所有权，或只更新变化的可见行。

验收：

- 达到配置的 scrollback 容量后，稳定内存进入平台期。
- 切换项目与关闭 Drawer 遵循稳定 Terminal 生命周期契约。
- 任何优化都保留 ANSI 样式、宽字形对齐、选择、scrollback 与 16 ms 刷新节流。

### MEM-11 — 审核依赖 Feature 占用

状态：Backlog，较低优先级

当前锁定直接依赖树包含较宽的 feature，例如 `tokio/full`、reqwest 默认项加 `rustls-tls`，以及默认 `image` 编解码。解析图同时包含 native-TLS 与 rustls 路径。这证明链接/启动面较宽，不能直接证明稳态 RSS 成比例上升。

工作：

- 使用 `cargo tree -e features --locked` 与 `cargo tree -d --locked` 作为前后依赖记录。
- 在把 `full` 换成最小 feature 列表前，先确认实际用到的 Tokio API。
- 确认应用 reqwest 与 `self_update` 是否都能关闭默认 TLS/feature，而不破坏更新、代理与平台行为。
- 只有在盘点完所有接受的 Project Logo、图标、剪贴板与打包资源格式后，才限制 `image` 编解码。
- 对比干净 Release 二进制体积、启动 RSS/footprint、HTTP/图片/更新路径首次使用 RSS、构建时间与打包。

验收：

- 不能只因为依赖树看起来大就删除 feature。
- HTTP TLS、流式更新、剪贴板图片、Project Logo、托盘资源与打包回归必须通过。
- 将二进制/启动改善与稳态 RSS 分开记录；若只节省可忽略的 RSS 却提高兼容性风险，则拒绝该改动。

### MEM-12 — 分离交互 CPU 与保留 RSS

状态：Backlog

已接受的侧栏与原生缩放行为视觉正确，但采样 CPU 仍然偏高。其保留 RSS 增量相对渲染器 footprint 很小且噪声大，因此 CPU 与内存必须使用不同门禁。

工作：

- 在现有侧栏切换与拖拽基准中增加 frame-time 或 signpost 数据，而不是只依赖进程级采样 CPU。
- 记录均值/p95 frame time、掉帧数、采样 CPU、Peak RSS 增量与稳定 RSS 增量。
- 在改变动画时序或几何所有权前，先分析交互期间的 Slint event loop 与 layout/model 工作。

验收：

- CPU 优化必须保留必需的弹簧运动，并消除此前观察到的闪烁。
- 不能因为 CPU 更低就接受视觉回退变体。
- 不能把瞬时交互 RSS 增量说成稳态内存节省。

### MEM-13 — 验证 Workflow 可达性清理

状态：正确性与测量

产品会在 Back/Forward 历史仍可返回 Workflow 或 PR Review 时有意保留其状态。历史容量为 64 条。当前 `workflow_became_unreachable` 只在“紧邻的上一页属于 Workflow，且转换后历史不再包含 Workflow”时触发；如果后续非 Workflow 导航才淘汰最后一个 Workflow 历史项，真正的“可达→不可达”边沿可能被漏掉。

工作：

- 为直接离开、保留 Back 目标、Back/Forward 移动、清空 Forward 栈，以及在 64 条上限淘汰最后一个 Workflow 项，增加纯 Router 测试。
- 若测试确认存在漏边沿，则在每次历史变更前后跟踪可达性，而不是只从 `previous` 推断。
- 当域变为不可达时，通过各自所属模块清理 PR Review controller 缓存/model、PR 列表 visited pages、highlight resources，以及可重建的 Workflow 细节。在当前稳定行为要求处保留 Tag 执行历史。
- 记录离开前、仍可达时，以及真正不可达后的 `[Workflow][PR Review][Memory]`、根 Slint model 计数、RSS 与 footprint。

验收：

- 打开 Settings/About 后再 Back，能按 PR Review 规格恢复精确的当前 Review。
- 当当前页、Back、Forward 都无法返回 Workflow/PR Review 时，包括有界历史淘汰，清理恰好触发一次。
- 迟到的异步响应仍受序列保护，不能重新填回已清空状态。
- 固定大型 PR fixture 在真正不可达事件后释放重型 owned model/cache；分配器高水位行为与活着的所有权分开报告。

### MEM-14 — 重建非活动 Messages 视觉树

状态：候选已拒绝（2026-08-14）

仅活动时挂载视觉树未达到采用门槛，离页后的绝对 RSS 与 physical footprint
还发生回退。生产继续保留稳定 factory 视觉树与已有的离页行 model 释放。

[完整实验、对照与证据](archive/memory-experiments.zh.md#mem-14)。

### MEM-15 — Docked Preview 覆盖时卸载 AI Review response delegate

状态：候选已拒绝（2026-08-27）

销毁 delegate 降低了活跃分配，但没有转化为代表性进程 RSS 改善；代表性
进程内 RSS 与 footprint 均轻微回退。生产保留本批前的生命周期。

[完整实验、压力与负控制、证据](archive/memory-experiments.zh.md#mem-15)。

### MEM-16 — AI Review response 可见窗口虚拟化

状态：拒绝设为生产默认（2026-08-27）

可见窗口虚拟化改善了压力 fixture，但代表性 RSS 节省 4.86 MiB/3.7% 未通过
预先声明的采用门槛。生产继续使用 `retain`；没有新的代表性证据和视觉验收，
被否决的候选不得成为下一批基线。

[完整实验、排除的 pilot 与证据](archive/memory-experiments.zh.md#mem-16)。

## 当前不优先处理

- Home 空闲 UI 模型：本次测量只有 9 个项目行、10 个标签、没有 PR 行、没有 Review 行，模型文本仅数 KiB。
- 已测空闲 fixture 中的 Project Logo：栅格在转换为 Slint Image 前已限制为 128×128。只有在“大量不同 Logo”场景下才重新考虑，因为进程生命周期 Image 缓存没有条目数上限。
- 线程栈驻留：本次进程低于 1 MiB。
- VSZ：不能单独作为有用的优化目标。

只有代表性场景证明这些项目持有了显著内存时，才重新提高其优先级。

## 建议实施批次

每次只执行一个批次。进入下一批前，当前批次必须记录明确的**采用**、**拒绝**或**证据不足**结论；禁止在同一个 A/B 进程中混入多个推测性改动。

### 强制 A/B 证据门槛

后续每个批次都必须在 fixture 与协议不变的前提下，用当前生产基线对比且只对比一个候选改动。每个单元至少运行三个独立 Release 进程；保留原始采样，并同时报告稳定态中位数、实测最大峰值、physical footprint、RSS 与循环间斜率，所有指标都要给出 `candidate - baseline` 的绝对差值和百分比。统一使用 `scripts/compare-memory-benchmarks.py` 拒绝机器、fixture、commit、页面或采样参数不一致的数据，并生成公共对比表。

只有目标指标可重复改善，且响应性、正确性、视觉行为、打包和关闭流程均不回退时才采用。中性、噪声较大、指标互有得失或明确回退的结果必须记录为**拒绝**或**证据不足**，不得描述为优化。仅诊断类改动若准备在 Release 默认启用，也必须先做开关前后的 overhead A/B。

此门槛同时是后续每个批次的回退与交付契约：

- 如果候选没有带来实质性提升，或者任一量化指标、功能结果变差，必须撤回该批次的全部候选行为；只保留通用测量基础设施、原始证据与明确的拒绝记录。
- 被拒绝的候选不得带入下一批基线，也不得与后续候选叠加，以掩盖中性或负向结果。
- 每批完成测量、决策、必要回退和验证后，必须停下来交给用户实际试用；只有用户确认当前效果正常并明确要求继续时，才能开始下一批。

### Batch 0 — 测量契约

条目：MEM-01、MEM-02

结论（2026-08-08）：**证据不足，暂不关闭本批次**。采用诊断与基准框架；Home 协议已经可执行，但仍需补齐固定 Projects、PR 列表、大型 PR、Chat 历史与 Terminal 输出的准备 hook，并产出各三轮 Release 结果，才能满足退出门槛。

- 扩展诊断，补齐 footprint 相对上次/基线增量与进程生命周期峰值。
- 让 Release 基准输出机器可读原始采样与摘要。
- 冻结 fixture、页面、窗口尺寸、缩放因子、渲染器/后端确认、预热时间、循环次数与 model/cache 计数。
- 每个场景至少跑三个新进程。保留原始运行；报告中位数、最大值与循环间稳定值斜率。

退出门槛：一条命令能复现 Home、Projects、PR 列表、固定大型 PR Review、Chat 历史与 Terminal 输出场景，且不以 Debug 构建作为生产基线。

### Batch 1 — 图形与窗口归属

条目：MEM-03、MEM-04、MEM-05

- 先在不改应用模型的前提下，对比默认 FemtoVG/OpenGL、FemtoVG/WGPU 与 Skia。
- 再对比当前透明/non-opaque AppKit 合成与最小 opaque 变体。
- 仅在两次 A/B 标明渲染器/窗口归属后，再使用最小复现器。

退出门槛：将全屏高水位归因到渲染器、原生窗口合成，或其交互。任何被采用的改动除内存外，还必须通过视觉、缩放、全屏、主题、标题栏、打包与响应性检查。

### Batch 2 — PR Review 大对象预算

条目：MEM-08、MEM-13

- 分别测量 base Diff、范围 Diff、展开 override、高亮、不可变源码、时间线/提交模型、头像与 Slint 行。
- 验证文件/范围导航以及离开 Workflow 生命周期域。
- 同时验证有意的历史保留，以及最后一个 Workflow 路由变为不可达后的清理，包括历史容量淘汰。
- 若条目数有界缓存仍保留显著大项，在保留现有条目上限的前提下增加字节预算，或跳过超预算项缓存。

退出门槛：固定大型 fixture 有可解释的 owned-byte 总量、反复导航平台期、历史仍可达时的恢复验证，以及真正不可达后的释放验证。没有这些证据前，不允许改缓存策略或生命周期。

2026-08-10 生命周期 A/B 结论：**拒绝，生产行为已回滚**。

- Fixture `pr-review-large-ranges-v1` 使用生产 Review 缓存类型，包含 1 份完整 Diff 和 4 份已访问范围 Diff；每份有 206 个文件、27,369 行、每行 112 个内容字节。诊断在清理前归因到 5 份唯一 Detail、136,845 行和 24,052,165 个估算堆字节，清理后均为 0。
- A/B 唯一变量为 `workflow_lifecycle=retain|clear`；两组使用同一个 FemtoVG/OpenGL Release 二进制，各 3 个隔离进程、180 条原始样本，并执行完整的双循环 resize/fullscreen 协议。历史保留和 64 条淘汰均经过真实 Router 与 controller 生命周期路径。
- 最终 800×600 稳定 RSS 变化为 -0.70 MiB（-0.5%），稳定 physical footprint 为 -23.80 MiB（-20.8%）。淘汰后的各相位中，稳定 footprint 改善 23.58–33.77 MiB，而稳定 RSS 只改善 0.16–0.72 MiB。
- 但候选在淘汰后的所有相位都使最大 RSS 峰值增加 0.41–0.64 MiB（+0.3% 到 +0.4%），部分 RSS 循环斜率也略差。按强制无回退原则，即使 footprint 明显下降也不能采用。
- 真正可达性清理只保留在显式内存基准模式中，以便复现实验证据；正常生产路由保持本批前的可达性/保留行为。原始证据与生成表位于 Git 忽略的 `memory-benchmark-results/mem08-lifecycle-20260810/retain-vs-clear/`。

### Batch 3 — 用户加载型状态

条目：MEM-09、MEM-10，以及 Workflow Job 日志核对

- 分别对 Chat 分页增长与 Terminal 长输出做基准。
- 确认 Tag 每步 32 KiB 上限，并审计其他 Job 生产者。
- 测量深度 PR 列表分页与大量不同 Project Logo key；仅在任一所有者显著时增加条目/字节上限。
- 只改变被证明显著的所有者，并保持路由/会话行为。

退出门槛：每个功能都有文档化增长曲线、平台期或释放点，以及回归结果。用户可控的有界缓存不按泄漏处理。

### Batch 4 — Runtime 与依赖面

条目：MEM-06、MEM-11

- 仅在存在代表性并发 HTTP、Git、Terminal、更新与关闭负载后，再调整 Tokio worker。
- 一次只裁剪一个包的依赖 feature，每次改动后重建并重跑同一 Release 矩阵。

退出门槛：只采用具有可重复启动/二进制/RSS 或运维收益，且无兼容性或延迟回退的改动。

### Batch 5 — Allocator 与剩余堆

条目：MEM-07

- 仅在图形与功能归属内存完成归因后，再回头看碎片。
- 仅当 Release 负载仍显示显著分配器归属碎片时，才比较分配器。

退出门槛：分配器实验必须在预先声明的内存阈值上击败现有分配器，且不带来不可接受的 CPU、启动、二进制、打包或平台成本。

### Batch 6 — 交互性能守护

条目：MEM-12

- 依据 frame/layout 证据优化侧栏/缩放 CPU。
- 视觉验收通过后，把已接受基准加入回归例程。

退出门槛：必需动画与视觉正确性保持不变，同时 frame-time/CPU 可重复改善；内存结果单独报告。

### 默认决策阈值

运行实验前应声明场景阈值。未另行记录时，默认要求全部满足：

- 相同循环的稳定 RSS/footprint 持续上升而不是进入平台期时，才按累积增长处理。
- 优化结论至少需要三个新进程，且目标指标稳定值中位数节省达到 10 MiB 或 5%。
- 若 p95 交互时间回退超过 5%、破坏稳定行为契约，或视觉/打包验证失败，则拒绝该改动。
- 低风险小收益可以保留，但必须明确标注，且不能挤占 P0/P1 排查。

### 每批证据记录

每批完成后追加以下字段：

- 日期、commit、脏工作区说明、硬件、macOS 版本、显示/缩放因子。
- 构建 profile、精确命令、启用 feature、已确认渲染器/后端。
- fixture 标识与规模、页面、窗口尺寸、预热、重复次数。
- 原始结果产物路径；RSS 与 footprint 的中位数/最大值；峰值/稳定增量；稳定斜率；owned bytes；相关 CPU/frame-time。
- 假设、单一变更变量、结果、采用/拒绝/证据不足结论、回归与回滚条件。

每项完成后，应追加测试 commit、命令、环境、前后对比表、最终决策和剩余风险。不要覆盖原始基线；后续结果应明确标注日期，以便持续比较回归。

## 相关资料

- `src/memory_diagnostics.rs`：进程 RSS/footprint 采样器。
- `scripts/monitor-footprint.sh`：macOS footprint/RSS 与缩放采样。
- `scripts/measure-pr-review-memory-ab.sh`：合成 PR Review 选中文件内存 A/B。
- `src/lib.rs`：启动检查点与诊断接线。
- `src/platform/macos.rs`：原生 macOS 窗口配置。
- `src/app_runtime.rs`：共享 Tokio runtime。
- `src/workflow/pr_review.rs`：有界 PR Review 缓存与内存诊断。
- `docs/architecture.md`、`docs/architecture.zh.md`：当前渲染器与所有权决策。
- `docs/specs/pr-review.md`：稳定 PR Review 内存边界。
- `docs/archive/memory/pr-review-memory-batch-a-20260927.md`：已拒绝的 PR Review Home RSS
  生命周期候选、源码 Release 证据、归因与回滚记录。
- `docs/archive/memory/settings-memory-batch-b-20260928.md`：已拒绝的 Settings 渲染层候选、
  六进程源码 Release 证据、归因与回滚记录。
- `docs/archive/memory/settings-memory-batch-c-attribution-20260928.md`：Settings 单二进制 owner
  拆分、无效运行控制、Slint 1.18.1 边界，以及不推进新产品候选的结论。
- `docs/archive/memory/terminal-memory-batch-d-attribution-20260928.md`：不激活窗口的
  Workflow-only 与真实 PTY owner 拆分、证据审计限制，以及不修改 Terminal
  生命周期或抽屉渲染的结论。
- `docs/archive/memory/project-list-memory-batch-e-20260928.md`：不激活窗口的 Grid/List owner
  拆分、修正后的长窗口采样、已拒绝的 List 行级外框 A/B，以及 Home RSS 节省未过
  pilot 门槛后的精确产品回滚。
