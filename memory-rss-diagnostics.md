# Usher RSS / Physical Footprint 诊断记录

> 初始记录日期：2026-07-17；本轮诊断与计划更新：2026-09-21。
>
> 第 1–12 节为 Slint 1.17.1 时期的历史证据，保留当时的实验条件与决策；不代表当前源码状态。
>
> 当前 Slint 1.18.0 的小窗口切页诊断、执行顺序与验收门禁见[第 13 节](#13-2026-09-20slint-1180-小窗口切页诊断与新一轮优化批次)。新一轮“批次 0–4”不与历史批次编号混用。
>
> 本轮状态：批次 1 已完成三对 Release 实验，RSS 改善 25.04%，但导航 p95 门禁失败，候选已完整回滚。批次 2 已冻结 `tianlai10000000/usher#1`；2A 字节预算因真实可回收缓存不足而暂缓，2B 会话生命周期候选虽能清空 PR owner，却使三进程末轮 RSS 中位数增加 6.99%，且最终清理 p95 退化 13.34%，已按整包候选完整回滚。批次 3 的 Chat geometry 候选只改善 2.250 MiB / 1.746%，已回滚；独立 Job 日志缓存子批已采用 16 项 + 16 MiB 软淘汰目标，正确性加固后的三对 Release settled RSS 中位数下降 37.594 MiB / 19.930%。未达门禁或被正确性复核作废的改动均未带入生产；原始 400 MB 仍未复现，Job 结果不属于用户当时的路线。
>
> 测试平台：macOS，Apple Retina/高分辨率显示器
>
> 文档性质：运行时诊断、上游调研和优化决策依据；每批是否已实现/采用，以该批明确记录的状态与证据为准。

## 1. 结论摘要

当前 Usher 运行到约 500～600 MiB 不是一个单一的 Rust 业务对象泄漏，而是以下因素叠加：

1. **最主要因素是 Skia 文本/字体路径。** 进入 Chat 后，RSS 一次增加约 449 MiB。`vmmap` 和 `heap` 显示 `Apple Color Emoji.ttc`、`Hiragino Sans GB.ttc` 的映射与两个近似同尺寸的大块堆分配高度吻合。
2. **高物理分辨率是确定存在的放大因素。** Slint 1.17.1 在 Skia 和 FemtoVG 下都会按 backing pixels 分配渲染表面。窗口从 1600×1200 physical 放大到 2800×1800 physical 后，physical footprint 明显增加。
3. **页面和渲染资源表现出“高水位保留”。** Chat 返回 Home、PR 返回列表、窗口缩小后，内存都不会立即回到进入或放大前水平。
4. **当前证据不能证明“每次点击都会无限增长”。** 已确认的是一次性大缓存和历史最大尺寸资源保留；是否存在长期线性泄漏，需要通过多轮自动化循环的增长斜率判断。
5. **PR 业务缓存不是主要矛盾。** 165 个文件、18,853 行 diff 的 Rust 侧已知缓存约 3.8 MiB，而打开 PR 后 footprint 增长接近 200 MiB。

活动监视器的“内存”列更接近 macOS 的 physical footprint，不等同于日志中的 resident-page RSS。macOS 压缩内存后，RSS 可能明显下降，而 footprint 仍维持高位。因此后续验收必须同时记录 RSS、physical footprint 和 VM 分类，不能只看一个数字。

## 2. 测试环境

### 2.1 显示器

测试时连接了两台 2× backing scale 的显示器：

| 显示器 | Logical mode | Backing resolution | Scale |
| --- | ---: | ---: | ---: |
| 内置/显示器 1 | 2056×1329 | 4112×2658 | 2× |
| 外接/显示器 2 | 2560×1440 | 5120×2880 | 2× |

两台显示器的实际 backing resolution 都达到或超过 4K，因此即使 Slint 逻辑尺寸不大，渲染器面对的物理像素数量也会是逻辑像素的四倍。

### 2.2 Slint 配置和实际渲染器

项目同时启用了：

- `renderer-femtovg`
- `renderer-skia`
- `backend-winit`
- `unstable-fontique-010`

虽然 `Cargo.toml` 注释认为默认仍为 FemtoVG，但 Slint 1.17.1 的 winit 默认工厂在 Skia feature 可用时优先创建 Skia renderer。本机运行时也通过 Metal/AGX、`CAMetalLayer`、Skia 类型和 VM region 确认实际默认路径是：

```text
winit -> Skia -> Metal
```

这意味着“同时编译两个 renderer、运行时不显式选择”的行为不可作为稳定的产品配置。

官方文档也说明多个 renderer 可以同时编译，并可通过 `SLINT_BACKEND=winit-skia` 或 `SLINT_BACKEND=winit-femtovg` 显式选择：

- [Slint Backends & Renderers](https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backends_and_renderers/)
- [Slint Cargo features](https://docs.slint.dev/latest/docs/rust/slint/docs/cargo_features/)

## 3. 采样方法

本次使用以下数据源交叉验证：

- Usher `[Memory][Diag]`：RSS、peak RSS、physical footprint、internal/external/reusable/compressed、VM region、线程和 page-in。
- Usher `[Memory][UI]`：路由、逻辑窗口尺寸、物理窗口尺寸和 scale factor。
- Usher `[Workflow][PR Review][Memory]`：PR diff、原始行、高亮和源码缓存的业务侧估算。
- macOS `vmmap`：IOSurface、mapped file、MALLOC zone、graphics region 和具体字体文件。
- macOS `heap`：大块堆分配尺寸。
- 显式 `SLINT_BACKEND=winit-femtovg` 的独立进程，用于 renderer A/B 对照。

采样时读取稳定值，而不是只取路由切换或窗口 resize 后第一帧的瞬时峰值。

## 4. 点击和路由切换记录

以下是默认 Skia + Metal 进程中的主要操作。RSS 会受 macOS 压缩影响而波动，所以判断以 footprint、变化方向和 VM 分类为主。

| 阶段/操作 | 稳定 RSS | 稳定 physical footprint | 观察 |
| --- | ---: | ---: | --- |
| 进程基础状态 | 12.17 MiB | — | 尚未创建主窗口 |
| 创建 MainWindow 后 | 约 72 MiB | 约 21.7 MiB | Slint/窗口/renderer 初始化 |
| 完成业务绑定 | 约 88 MiB | 约 30.4 MiB | 项目模块初始化完成 |
| Home，800×600 logical | 166～167 MiB | 74～76 MiB | 1600×1200 physical 基线 |
| 点击 Chat | 约 587.8 MiB | 约 347.7 MiB | RSS 一次增加约 449.2 MiB；首次合成 transient footprint 达到约 527～534 MiB |
| Chat → Home | 约 588.1 MiB | 约 347.9 MiB | 大部分内存未释放 |
| Home → Project | 约 607.6 MiB | 约 365.6 MiB | 增加约 19.5 MiB |
| 进入 Usher → Workflow | 584～612 MiB | 约 376.4 MiB | RSS 因压缩波动，footprint 继续上升 |
| 点击 PR 子页面 | 约 568 MiB | 约 385.7 MiB | 没有出现 Chat 级别突增 |
| 打开 PR 前 | 约 532.7 MiB | 约 398 MiB | 受压缩影响的即时基线 |
| 打开 165 文件 PR | 约 638.1 MiB | 596～597 MiB | 加载期 footprint 达到约 684～696 MiB |
| 返回 PR 列表 | 约 631 MiB | 约 596.9 MiB | 只回收约 7 MiB RSS，footprint 基本不降 |
| 放大到 1400×900 logical | 约 631.5 MiB | 约 684.9 MiB | 2800×1800 physical；footprint 增加约 88 MiB |
| 缩回 800×600 logical | 后续压缩至约 590 MiB | 约 648.6 MiB | 未恢复到放大前 596.9 MiB |

### 4.1 PR 业务缓存核对

测试 PR：

- 165 个文件
- 18,853 行 diff
- detail heap estimate：1,931,730 bytes
- original heap estimate：1,931,730 bytes
- 首批 highlighted cache：96,650 bytes
- 已知业务缓存合计约 3.8 MiB

这与页面打开后约 200 MiB 的 footprint 增量不在同一数量级。因此优先优化目标应是文本布局、组件生命周期和 renderer 资源，而不是先压缩这几个有界 Rust `Vec`。

## 5. Renderer 和窗口尺寸 A/B

### 5.1 新进程、简单 Home 页面

| Renderer | Logical / Physical | RSS | Physical footprint |
| --- | --- | ---: | ---: |
| Skia + Metal | 800×600 / 1600×1200 | 166.55 MiB | 74.09 MiB |
| Skia + Metal | 1400×900 / 2800×1800 | 约 169 MiB | 104.4 MiB |
| FemtoVG | 800×600 / 1600×1200 | 160.6～161.2 MiB | 92.6～93.1 MiB |

Skia 的简单 Home 页面从 1.92M backing pixels 增加到 5.04M backing pixels 后，footprint 增加约 30.3 MiB。RSS 变化很小，说明新增成本主要进入 IOSurface/GPU/driver accounting，而不是普通 Rust heap。

### 5.2 Chat 页面

| Renderer | Logical / Physical | RSS | Physical footprint | 相对进入 Chat 前 |
| --- | --- | ---: | ---: | --- |
| Skia + Metal | 800×600 / 1600×1200 | 约 588 MiB | 约 348 MiB | RSS 约 +449 MiB |
| Skia + Metal | 1400×900 / 2800×1800 | 约 618.1 MiB | 约 411.5 MiB | 相对同尺寸 Home，RSS 约 +449 MiB、footprint 约 +307 MiB |
| FemtoVG | 800×600 / 1600×1200 | 约 165.45 MiB | 约 128.16 MiB | RSS 约 +4.3 MiB、footprint 约 +35 MiB |
| FemtoVG | 1400×900 / 2800×1800 | 约 167.5 MiB | 约 251.74 MiB | 相对小窗口 Chat，footprint 约 +123.6 MiB |

结论：

- Chat 的约 449 MiB RSS 突增基本与窗口像素数无关，主因不是 framebuffer，而是 Skia 文本/字体路径。
- 高分辨率成本在两种 renderer 中都存在。
- 本机当前工作负载下，FemtoVG 避免了 Skia 的大额字体突增，但 FemtoVG 的 resize/backing-surface 成本更陡。

## 6. VM 和 heap 归因

### 6.1 Skia Chat，2800×1800 physical

`vmmap` 稳定状态：

- physical footprint：约 411.5 MiB
- `MALLOC_LARGE`：约 227.1 MiB
- mapped-file resident：约 224.2 MiB
- owned/unmapped graphics：约 56.3 MiB
- IOSurface：约 58.7 MiB

三个 `CAMetalLayer Display Drawable` 各约 19.6 MiB，合计约 58.7 MiB。其数量和尺寸符合 2800×1800、4 bytes/pixel 的多缓冲渲染表面数量级。

### 6.2 字体

`vmmap`：

- `/System/Library/Fonts/Apple Color Emoji.ttc`：约 183.2 MiB mapped/resident
- `/System/Library/Fonts/Hiragino Sans GB.ttc`：约 22.4 MiB mapped/resident

`heap`：

- 一个约 187,632 KiB（约 183.2 MiB）的单块非对象分配
- 一个约 22,976 KiB（约 22.4 MiB）的单块非对象分配

映射文件与堆块大小高度吻合。没有分配调用栈时不能把“映射与堆块一一对应”写成绝对事实，但结合下列证据，可以把 Skia/fontique 字体数据持有判断为高置信度：

- Chat 历史只有 20 条、约 21 KiB，不可能解释 449 MiB 增量。
- 历史文本包含 `❌` 28 次、`✅` 1 次，会触发 Apple Color Emoji fallback。
- Chat 多处显式使用 `Hiragino Sans GB`。
- Slint 1.17.1 Skia `FontCache` 容量为 64，并在 cache key 中持有字体 blob 的强引用，源码注释明确说明这样做是为了阻止 fontique shared cache eviction。
- Chat 返回 Home 后大块内存保持高水位，符合全局/renderer 字体缓存的生命周期。

相关上游源码：

- [Slint v1.17.1 Skia font cache](https://github.com/slint-ui/slint/blob/v1.17.1/internal/renderers/skia/font_cache.rs)
- [Slint v1.17.1 winit renderer selection](https://github.com/slint-ui/slint/blob/v1.17.1/internal/backends/winit/lib.rs)

## 7. 上游和社区同类反馈

### 7.1 与当前现象最接近：放大窗口导致内存增长

[Slint #5280 — Simply enlarge the window, and the memory will increase exponentially](https://github.com/slint-ui/slint/issues/5280)

- 状态：Open
- 环境：Slint 1.5、macOS 14.5
- 报告内容：仅放大窗口就导致内存快速增长。
- 报告者同样开发 LLM client；随着对话轮数增加，内存快速上升。
- 切换 Skia 后，对话轮数导致的增长基本消失，但窗口缩放导致的内存变化仍然存在。

这与本机测试的“内容/字体成本”和“physical backing surface 成本是两个独立维度”高度一致。renderer 的相对优劣与该旧版本不同，但“缩放成本在切换 renderer 后仍存在”完全一致。

### 7.2 中文字体约 170 MiB 额外内存

[Slint #7434 — Memory usage is excessive or leaking of Chinese font](https://github.com/slint-ui/slint/issues/7434)

- 状态：Open
- 环境：Slint 1.9.2、LinuxKMS、software renderer
- 报告内容：增加中文字体后内存相差约 170 MiB。
- Slint 维护者明确表示这不是设计意图，并确认当时 fontdue 会加载全部 glyph 数据。

该 issue 的 renderer 和平台与本机不同，不能直接证明 Skia/macOS 是同一个实现缺陷；但它证明 Slint 文本栈确实出现过“大字体文件导致百 MiB 级内存”的真实问题，与本机字体 VM 证据方向一致。

### 7.3 新版本内存回归反馈

[Slint #10837 — Significant memory usage regression across versions](https://github.com/slint-ui/slint/issues/10837)

- 状态：Open
- 对比：1.13.1、1.14.1、1.15.1，winit/Skia、Windows
- 至少两名用户观察到 1.14+ 的显著增长。
- 维护者确认 1.14.1 曾存在内存泄漏并在 1.15 修复，但报告者在之后的 master 上仍观察到约 50 MiB，并表示与显式导入字体无关。

这说明近期版本仍有平台相关的内存回归反馈，但它不是 macOS Retina 的直接复现。

### 7.4 其他相邻证据

- [Slint #2789 — drop-shadow + animation 从约 80 MiB 增至 431 MiB](https://github.com/slint-ui/slint/issues/2789)：Open，同时标记 Skia 和 FemtoVG。说明全尺寸阴影/离屏层可能造成很大的渲染缓存，但本次尚未证明它是 Usher 的首要来源。
- [Slint #2768 — FemtoVG text memory leak](https://github.com/slint-ui/slint/issues/2768)：已在后续 master 修复旧 text shaper word cache。维护者把 resize 内存增长视为另一个 renderer/driver 问题，不能把这个已关闭 issue 当作 Usher 当前问题已解决的依据。
- [Slint #10591 — font-weight 导致大内存增长](https://github.com/slint-ui/slint/issues/10591)：已由 commit `30a03cf...` 修复。Slint 1.17.1 的 font cache 为防止该类错误而强持有字体 blob；这解释了字体进入 cache 后为什么不易被逐出，但不代表当前 183 MiB Emoji 字体占用是合理目标值。
- [Slint ScrollView 文档](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/views/scrollview/)：明确说明 ScrollView 会实例化全部 children，长列表应使用只实例化可见项的 ListView。
- [Slint 性能调试文档](https://docs.slint.dev/latest/docs/slint/guide/development/debugging_techniques/)：明确警告 `cache-rendering-hint` 会增加 GPU memory usage 和 memory throughput。

### 7.5 对“Slint 1.17.1 是否仍有高分辨率问题”的严谨判断

可以确认：

- 本项目在 Slint 1.17.1 上可重复观察到 backing pixels 增加导致 footprint 增长。
- Skia 和 FemtoVG 都可重复，且窗口缩小后不完全回落。
- 上游存在一个仍为 Open 的 macOS 放大窗口内存报告 #5280。

不能声称：

- 上游已经正式确认“Slint 1.17.1 macOS Retina 存在某个编号明确的同一 bug”。目前没有找到完全相同版本、平台、renderer、字体内容和指标口径的公开 issue。
- 所有增量都是泄漏。framebuffer、IOSurface、多缓冲和 renderer resource pool 本来就会随物理像素增加；异常点是增长幅度、高水位保留和缺少可控回收边界。

因此最准确的表述是：**Slint 1.17.1 在本项目和本机环境中仍有显著的高物理分辨率内存成本；公开资料存在高度相似的未关闭报告，但尚无完全同构的 1.17.1 上游确认。**

## 8. 优化方案

下面按收益确定性和工程风险排序。每一项都必须通过第 9 节的固定场景验收，不能仅凭活动监视器截图判断。

### P0：固定 renderer 策略，消除运行时歧义

#### 建议

macOS release 构建先选择并只编译一个默认 renderer。基于本次实测，第一候选是 **FemtoVG**，Skia 保留为独立实验构建，而不是同时启用后依赖隐式顺序。

#### 依据

- 当前 Chat 小窗口：Skia RSS 约 +449 MiB；FemtoVG RSS 约 +4.3 MiB。
- 当前 Chat 小窗口：Skia footprint 约 +274 MiB；FemtoVG约 +35 MiB。
- 官方允许构建期和运行期显式选择 renderer。

#### 风险和边界

- FemtoVG 在 2800×1800 Chat 上仍达到约 252 MiB footprint，并不能解决高 DPI surface 成本。
- 必须回归字体观感、代码高亮、阴影、透明层、滚动和拖拽性能。
- 不能根据这一台 Mac 推导 Windows/Linux 也应该统一使用 FemtoVG；renderer policy 应按平台独立确定。

#### 预期收益

在当前 macOS Chat 工作负载下，这是唯一已经用 A/B 数据证明能直接消除约 400 MiB 级突增的方案。

### P0：阻止 Apple Color Emoji 进入正文文本 fallback

#### 建议

将应用生成的状态符号（例如 `❌`、`✅`）从正文字符串中结构化出来，使用现有 SVG/icon 组件渲染；正文保留可复制的文本描述，并为图标提供 accessibility label。

对用户自由输入的真实 Emoji 不做破坏性过滤。优化范围只针对应用自己生成的状态前缀和固定文案。

#### 依据

- Apple Color Emoji 文件及对应大块分配约 183.2 MiB。
- 测试历史中应用生成的 `❌` 出现 28 次、`✅` 出现 1 次。
- 两者移出 Text fallback 后，应不再因为这些固定状态标记加载完整 Emoji 字体。

#### 预期收益

如果 Skia 路径继续保留，理论上可消除本次最大单项字体映射/复制来源。实际收益必须用“完全相同历史，仅替换固定状态符号”的 A/B 确认。

### P0：收敛字体策略，而不是下载另一套完整 CJK 字体

#### 建议

1. 普通 UI/正文统一使用根窗口默认系统 UI font，不在每个 Text 上重复指定 `Hiragino Sans GB`。
2. 代码区单独使用 Menlo 或项目确定的等宽字体。
3. 对 `Hiragino Sans GB` 做 A/B：显式指定、系统默认 fallback、必要时项目字体子集。
4. 只有在许可、字形覆盖和体积都经过审核后，才考虑内置 subset font。

#### 不建议

- 不建议直接下载一套完整 Noto CJK/Emoji 字体替换。完整 CJK/Emoji 字体可能比当前文件更大，且 Slint font cache 会强持有已使用 font blob。
- 不建议只删除 `unstable-fontique-010` 就宣称解决问题。当前 renderer 依赖图本身仍启用 shared fontique/text layout 路径，必须用构建依赖和运行数据确认。

#### 依据

本次 `Hiragino Sans GB.ttc` 映射和对应大块分配各约 22.4 MiB，潜在合计约 45 MiB。

### P1：Chat 改为真正的可见项虚拟化

#### 建议

Chat 当前使用 `ScrollView + for message`，所有消息和内部 Markdown block/span 都会实例化。长期应改成 `ListView` 或等价的 viewport windowing，保留可见窗口和少量 overscan；历史数据继续由 Rust model 分页持有。

#### 依据

Slint 官方文档明确说明：

- ScrollView 总是实例化全部 child。
- ListView 只实例化可见元素，适合长列表并提供稳定性能。

#### 风险和边界

- 当前只有 20 条消息，因此它不是这次 449 MiB 首次突增的解释。
- 可变高度 Markdown、向上加载历史、保持滚动锚点和流式回复需要专项设计，不能简单机械替换。

#### 长期收益

把消息条数、Markdown span 数、text layout 和组件数量从“随会话总量增长”改为“随 viewport 大小有界”，防止 LLM 长会话重新出现线性增长。

### P1：按路由挂载/卸载重页面

#### 建议

Chat 当前常驻根组件，只通过 `visible` 切换。应把聊天数据、输入状态和滚动锚点保留在 Rust/root state 中，而 Chat 可视组件仅在对应路由挂载。PR review 已使用条件挂载，关闭时还应确保大 line model、selected-file highlighted model 和页面专属引用同步清空。

#### 依据

Slint Skia renderer 在 component destroyed 时存在释放 component graphics resources 的路径。常驻但 invisible 的 Chat 不会触发组件销毁，因此页面级 text layout/image/path cache 可能继续保留。

#### 风险和边界

- Skia thread-local font cache 比单个页面生命周期更长，卸载 Chat 不能保证释放 Apple Color Emoji blob。
- 该项用于回收页面级资源、避免后台常驻，不应作为字体问题的唯一修复。

### P1：降低全窗口离屏层和阴影成本

#### 建议

对 PR review、Modal、Drawer 等全窗口区域逐项审计：

- 大面积 `drop-shadow-blur`
- opacity/clip 组合形成的离屏层
- `cache-rendering-hint`
- 不可见但仍挂载的 overlay
- 与窗口等大的临时 image/layer cache

只对静态、边界较小、实测减少 draw calls 的组件启用渲染缓存；禁止把 `cache-rendering-hint` 当作通用性能开关。

#### 依据

- Slint 官方文档明确警告渲染缓存会增加 GPU memory。
- 上游 #2789 显示 drop-shadow + animation 可把内存从约 80 MiB 推高到 431 MiB。
- 本机 post-PR resize 比简单 Home 的 footprint 增量更高，说明复杂页面存在额外全尺寸资源。

### P2：评估 FemtoVG-WGPU/Metal，而不是长期依赖 OpenGL 路径

#### 建议

把 `renderer-femtovg-wgpu` 作为独立实验构建，与当前 FemtoVG OpenGL 和 Skia Metal 做三方矩阵，不直接替换生产配置。

#### 依据

- 官方文档说明 FemtoVG-WGPU 可使用 Metal/Vulkan/Direct3D。
- 上游维护者在 resize 内存报告中建议用 FemtoVG-WGPU 绕开部分 OpenGL driver 行为，但该回复针对 Windows，不能直接推定 macOS 结果。

#### 通过条件

只有在相同 2× backing size 下同时满足以下条件，才考虑替换：

- Chat 首次进入 footprint 明显低于当前 FemtoVG。
- resize 10 个循环后没有正增长斜率。
- 字体质量、PR diff 滚动和拖拽交互没有回退。

### P2：向 Slint 提交最小复现，而不是长期维护不可解释的本地 workaround

建议建立一个最小 Slint 1.17.1 macOS reproducer，至少包含：

- 一个 Home Text 页面和一个 Chat Text/List 页面
- 可切换 `❌/✅` 与纯 ASCII 的同内容测试
- 可切换 `Hiragino Sans GB` 与系统默认字体
- 固定 800×600、1400×900 logical 两种尺寸
- Skia、FemtoVG、FemtoVG-WGPU 三种 renderer
- 每一步 RSS、physical footprint、IOSurface 和字体 mapped region

这份数据可以补充到 #5280，或提交新的 1.17.1/macOS/fontique issue。当前证据已经足以形成比“活动监视器截图”更可操作的上游报告。

## 9. 验收标准

所有优化都使用同一 release/debug profile、同一历史数据、同一窗口尺寸、同一显示器重复测试。

### 9.1 固定场景

1. 冷启动停留 Home 60 秒。
2. Home → Chat → Home，循环 10 次。
3. Chat 中保持相同 20 条历史，分别测试固定 Emoji 状态图标和纯文本版本。
4. 800×600 → 1400×900 → 800×600 logical，循环 10 次。
5. Project → Workflow → PR 列表 → 打开 165 文件 PR → 返回，循环 5 次。
6. 每个阶段等待 30～60 秒，让压缩和异步任务稳定。

### 9.2 指标

- 首次进入 Chat 的 RSS/footprint 增量。
- 第二次到第十次路由循环的线性增长斜率。
- resize 第一次高水位与后续九次是否继续增长。
- `MALLOC_LARGE`、mapped file、IOSurface、graphics region 的变化。
- Apple Color Emoji 和 CJK font 是否出现、各自 resident/heap 大小。
- UI 帧率、拖拽延迟、PR diff 滚动和文本清晰度。

### 9.3 建议门槛

- 路由或 resize 在完成第一次 warm-up 后，后续平均增长不超过 1 MiB/循环。
- 固定 20 条 Chat 历史的首次进入不再出现百 MiB 级 RSS 跳变。
- PR 关闭后允许保留有界 diff cache，但不得随重复打开线性增长。
- 高 DPI 下允许存在由 backing surface 决定的固定成本，但窗口缩小/重复 resize 后不得持续扩大历史高水位。
- renderer 切换不能以明显字体模糊、缺字、滚动卡顿或功能缺失换取内存数字。

## 10. 推荐执行顺序

1. **先做 renderer 确定化的 macOS 实验分支：FemtoVG-only。** 这是当前收益证据最强、改动边界最清晰的方案。
2. **并行把应用生成的 Emoji 状态前缀改为 SVG/icon。** 用 Skia A/B 验证是否消除 Apple Color Emoji 183 MiB 映射和对应大块堆分配。
3. **统一字体策略并做 Hiragino A/B。** 不先下载完整第三方字体。
4. **把 Chat 改成可见项虚拟化并按路由卸载。** 解决长会话的长期可扩展性和页面级高水位。
5. **审计全窗口阴影/离屏缓存。** 重点针对 post-PR resize 增量。
6. **最后评估 FemtoVG-WGPU。** 它是解决 OpenGL/resize 高水位的候选，不是未经测试的默认答案。

这套顺序把“本次已确认的 400 MiB 级问题”放在前面，把“高 DPI renderer/driver 高水位”作为独立问题处理，避免用一个 renderer 切换掩盖字体、列表生命周期和离屏层三个不同层面的成本。

## 11. 分批优化 A/B 记录

### 11.1 第 1 批：生产渲染器确定化

日期：2026-07-17。

改动：默认 Cargo feature 只启用 `renderer-femtovg`；`renderer-skia` 改为显式诊断 feature。常规 `cargo run` 不再链接 Skia，诊断命令为：

```sh
SLINT_BACKEND=winit-skia cargo run --no-default-features --features renderer-skia
```

测试条件：同一台 macOS 2× backing scale 显示器、相同 20 条 Chat 历史、800×600 logical 初始窗口；每个动作后等待 8 秒。A 组是改动前默认选择的 Skia，B 组是改动后的默认 FemtoVG。

| 场景 | A：Skia footprint / RSS | B：FemtoVG-only footprint / RSS | 结果 |
| --- | ---: | ---: | ---: |
| Home 稳定 | 约 75 MiB / 约 166 MiB | 93 MiB / 160.3 MiB | RSS 约 -3%；footprint 口径存在 renderer 差异 |
| 首次进入 Chat | 350 MiB / 588.3 MiB | 128 MiB / 164.5 MiB | footprint -63.4%；RSS -72.0% |
| Chat 返回 Home | 350 MiB / 360.4 MiB | 128 MiB / 164.3 MiB | footprint -63.4%；RSS -54.4% |
| Home 放大到 1400×900 logical | 376 MiB / 360.8 MiB | 194 MiB / 164.4 MiB | footprint -48.4%；RSS -54.4% |
| 恢复 800×600 logical | 364 MiB / 360.9 MiB | 142 MiB / 164.4 MiB | footprint -61.0%；RSS -54.4% |

结论：本批通过。收益来自避免生产路径选中 Skia 以及其 Chat 字体/缓存高水位，不是业务模型释放。默认依赖树已确认不存在 `i-slint-renderer-skia`；FemtoVG 和显式 Skia 两种 feature 均通过 `cargo check --locked`。高 DPI 放大仍产生约 66 MiB 的 footprint 增量，后续必须作为独立的 backing surface/renderer 问题继续测试。

### 11.2 第 2 批：状态 Emoji 改为结构化状态 + SVG

日期：2026-07-17。

改动：Chat 持久化模型新增向后兼容的 `status` 字段；新错误消息保存纯文本和 `error` 状态，UI 使用 `error.svg` / `success.svg`。旧 JSONL 中以 `❌` / `✅` 开头的 assistant 消息在渲染层识别状态并剥离前缀，不改写用户历史文件。

测试条件：显式 Skia 诊断构建、相同 20 条历史、800×600 logical、2× backing scale。选择 Skia 作为本批测试后端，是为了直接隔离并观察 Apple Color Emoji 字体映射；生产默认仍是第 1 批确定的 FemtoVG。

| 指标 | A：Emoji 前缀 | B：结构化状态 + SVG | 改善 |
| --- | ---: | ---: | ---: |
| Chat footprint | 350 MiB | 159 MiB | -54.6% |
| Chat RSS | 588.3 MiB | 217.0 MiB | -63.1% |
| Apple Color Emoji mapped file | 183.2 MiB | 未映射 | -100% |
| Hiragino Sans GB mapped file | 22.4 MiB | 22.4 MiB | 无变化，留给第 3 批 |

结论：本批通过。`vmmap` 已确认候选构建中没有 Apple Color Emoji 映射，证明收益来自去除应用生成的 Emoji 字形路径，而不是随机压缩波动。旧历史兼容和状态前缀剥离均有单元测试；`cargo test --lib --locked` 为 40 passed、0 failed。

### 11.3 第 3 批：系统默认 CJK 字体策略（已回滚）

日期：2026-07-17。

实验改动：Chat 普通文本取消显式 `Hiragino Sans GB`，交给 macOS 系统默认字体回退；代码文本使用 Menlo。测试条件与 11.2 相同。

| 指标 | A：显式 Hiragino | B：系统默认回退 | 变化 |
| --- | ---: | ---: | ---: |
| Chat footprint | 159 MiB | 198 MiB | +24.5% |
| Chat RSS | 217.0 MiB | 346.3 MiB | +59.6% |
| 主要 CJK mapped file | Hiragino 22.4 MiB | PingFang 74.6 MiB | +233% 映射体积 |

结论：本批失败并已回滚。macOS 默认回退在当前 Slint 1.17.1 + Fontique + Skia 组合中选择了完整 PingFang TTC，物理成本明显高于显式 Hiragino。不能仅凭平台字体规范推断内存更优；在引入经过子集化、授权明确的应用字体之前，继续保留 Hiragino 是本机实测更低成本的方案。

### 11.4 第 4 批：Chat 路由卸载实验（已回滚）

日期：2026-07-17。

现状核对：Chat 已实现虚拟窗口渲染，包含 overscan、增量高度前缀缓存、滚动稳定后精确重算和最多 56 条消息的窗口上限；本批不重复替换为另一套列表组件。

实验分两步：

1. 条件创建/销毁整个 Chat 组件。该版本虽然得到 96 MiB 的低 footprint，但截图确认消息未显示，是滚动锚点在 ScrollView viewport 创建前生效导致的假阳性，立即回滚。
2. 保持组件常驻，只在离开 Chat 时清空渲染模型。页面显示正常，但相同 20 条历史下无可测收益。

| 场景 | A：现状 | B：离开时清空模型 | 变化 |
| --- | ---: | ---: | ---: |
| Chat footprint | 153 MiB | 152 MiB | -0.7%，在测量噪声内 |
| Chat RSS | 246.8 MiB | 246.5 MiB | -0.1%，在测量噪声内 |
| 返回 Home footprint | 153 MiB | 152 MiB | -0.7%，字体/renderer 高水位未释放 |
| 返回 Home RSS | 246.8 MiB | 246.6 MiB | -0.1% |

结论：本批无收益并已回滚。当前 20 条消息低于 56 条虚拟窗口上限，模型本身不是本次 RSS 大头；保留模型还能避免重入 Chat 时重复 markdown 解析。只有将来用数千条历史做独立压力测试并观察到模型高水位时，才值得重新评估路由卸载，同时必须先把滚动锚点改成基于实际 viewport 生命周期的显式恢复。

### 11.5 第 5 批：FemtoVG/WGPU（保留为诊断 feature，不设为默认）

日期：2026-07-17。

改动：新增显式 `renderer-femtovg-wgpu` Cargo feature，并将 wasm-bindgen/js-sys 依赖组更新到 WGPU 29 可解析的版本。生产默认 feature 不变，仍是 FemtoVG/OpenGL。

测试条件：相同 800×600 logical 初始窗口、2× backing scale、20 条 Chat 历史；A 为默认 FemtoVG/OpenGL，B 为 FemtoVG/WGPU 29 + Metal。

| 场景 | A：OpenGL footprint / RSS | B：WGPU/Metal footprint / RSS | 结论 |
| --- | ---: | ---: | --- |
| Home | 93 / 160.3 MiB | 70 / 174.0 MiB | footprint -24.7%，RSS +8.6% |
| Chat | 128 / 164.5 MiB | 119 / 186.7 MiB | footprint -7.0%，RSS +13.5% |
| Chat 返回 Home | 128 / 164.3 MiB | 119 / 183.5 MiB | footprint -7.0%，RSS +11.7% |
| Home 放大到 1400×900 | 194 / 164.4 MiB | 151 / 183.1 MiB | footprint -22.2%，RSS +11.4% |
| 恢复 800×600 | 142 / 164.4 MiB | 135 / 182.9 MiB | footprint -4.9%，RSS +11.3% |

结论：不切换生产默认。WGPU/Metal 对高 DPI backing surface 的 footprint 明显更好，但当前稳定 RSS 全场景更差，而本次优化目标是 RSS。保留显式 feature 是为了后续升级 Slint/WGPU 后使用同一矩阵复测。另一个工程风险是项目 Debug logger 会接收 `naga`/`wgpu` 的大量 debug 事件；诊断运行产生了大量 shader 日志，若未来考虑生产启用 WGPU，必须先增加按 target 的日志过滤并独立测量其开销。

### 11.6 第 6 批：PR 详情关闭生命周期

日期：2026-07-17。

触发路径：先进入 Chat，再打开包含 200 个文件、24,843 行 diff 的 PR 详情，随后返回 Home。Activity Monitor 一度显示约 281 MiB。

基线日志揭示两个不同问题：

1. `workflow_pr_review_close` 只隐藏工作区并使异步序列失效，没有调用已经存在的 `reset_review_ui()`，controller 仍保留归一化 diff、原始 diff、高亮和源码缓存。这与 `docs/specs/pr-review.md` 中“关闭审核时清理审核缓存”的契约不一致。
2. Activity Monitor 的“内存”不能等同于即时 resident RSS。返回 Home 后，日志中的 RSS 从约 268.5 MiB 经 macOS 压缩在约 90 秒后降至约 174.9 MiB，而原窗口尺寸下的 physical footprint 仍约 281 MiB。原窗口为 1228×947 logical、2× backing scale，即 2456×1894 physical。随后同一 PID 的一次 `footprint` 采样为 208 MiB（峰值 421 MiB），但采样前用户已经缩小窗口，因此该数值只能证明 IOSurface/backing store 对窗口物理像素尺寸敏感，不能作为“同尺寸下 footprint 自动回落”的 A/B 证据；该次分类中约 81 MiB 为 IOAccelerator、IOSurface 和 graphics-owned footprint，约 110 MiB 为 malloc 区域。

本批改动：关闭 PR 详情时立即取消所有在途请求，清空 active request、单项 diff/原始 diff、筛选状态、折叠状态、4 项高亮 LRU 和 3 项固定源码 LRU，并把所有 Slint PR model/string/state 重置为空。新增 `review_close.before_clear` / `review_close.after_clear` 两个业务内存检查点；候选构建中 after 日志必须显示 detail、original、highlight 和 source 四组持有量均为 0。

本批的验收口径分层处理：

- **确定性 A/B：** A 组关闭后业务缓存仍约 5.2 MiB（两份 diff 各约 2.6 MiB，另有高亮/源码缓存）；B 组关闭后这些 controller 持有量和根 UI models 均归零。
- **进程级 A/B：** 不能承诺 Activity Monitor 立即下降同等数值。malloc arena、字体/glyph cache、当前 Home 窗口 backing surfaces 和驱动资源的生命周期长于页面；以关闭后 10、30、90 秒的 RSS 与 physical footprint 分别比较，并以多轮稳定中位数判断。

候选构建实测：用户重复了 Chat → 同一个 PR → Home。候选窗口日志实际记录为 1169×878 logical、2× backing scale（2338×1756 physical），比旧组的 1228×947 略小，因此下面只作为候选组内部的释放曲线，不能伪装成严格同尺寸百分比 A/B。

| 时间点 | physical footprint | RSS | 说明 |
| --- | ---: | ---: | --- |
| PR 关闭前 | 411.45 MiB | 257.44 MiB | 200 文件、24,843 行；两份 diff 各约 2.62 MiB，高亮约 0.48 MiB |
| 清理后立即 | 411.45 MiB | 257.44 MiB | 四组业务持有量全部为 0，allocator/renderer 尚未变化 |
| Home 约 5 秒 | 288.69 MiB | 256.98 MiB | footprint -122.76 MiB，主要是页面图形资源销毁 |
| Home 约 30 秒 | 288.56 MiB | 256.95 MiB | 基本稳定 |
| Home 约 60 秒 | 288.56 MiB | 253.56 MiB | RSS 仅缓慢下降 |
| Home 约 90 秒 | 288.56 MiB | 253.56 MiB | 未进一步回落 |

90 秒后系统 `footprint` 分类为 289 MiB：IOSurface 79 MiB、IOAccelerator graphics 53 MiB、graphics-owned 40 MiB，图形侧合计约 172 MiB；`MALLOC_SMALL` 90 MiB、`MALLOC_LARGE` 7.5 MiB。说明剩余成本主要是当前高 DPI 大窗口的多缓冲图形表面，以及大量小对象释放后仍由 malloc zone 保留的页，而不是仍有强引用的 PR controller cache。

结论分开判定：

- **生命周期正确性：通过并保留。** 关闭后的业务日志确定性归零，修复了规范与实现不一致，也阻止跨多次 PR 打开继续持有旧数据。
- **进程 RSS 优化：本批未通过。** 业务侧约 5.7 MiB 的明确持有已释放，但 90 秒 RSS 没有出现超出噪声的对应下降；不得把页面本来就会发生的约 123 MiB 图形资源回落归功于 controller cache 清理。

下一批应围绕“降低峰值分配，从而避免 malloc/renderer 建立高水位”设计，而不是在关闭后尝试非可移植的强制 allocator trim。优先审计 PR 行 model 的临时 clone/小字符串分配和全窗口离屏层；图形侧必须继续用固定物理窗口尺寸单独测量 IOSurface 数量与大小。

### 11.7 Home 稳定态的剩余常驻所有者归因

日期：2026-07-17。

复现路径：启用 `MallocStackLogging=1` 启动诊断构建，按 Chat → 同一个 PR → Home 操作；窗口保持 800×600 logical、2× backing scale。该运行只用于分配调用栈归因，不能把 RSS 绝对值与未启用 MallocStackLogging 的生产运行直接比较。

业务所有权日志确认 Home 状态下：

- PR review 的 files、tree items、extension filters 和 lines 均为 0；controller 的 detail、original、highlight、source 四类缓存也均为 0。
- 根 UI 中 Chat、Project、PR list 和 Terminal 的文本量合计仅约 19 KiB；这些 model 不是 200 MiB 级物理占用的解释。
- `leaks` 只报告 288 个节点、18,560 bytes，调用栈属于 macOS AppIntents/NSXPC 循环；没有发现应用规模的不可达泄漏。

该诊断进程稳定后的 `footprint` 为 268 MiB，分类如下：

| 类别 | 大小 | 解释 |
| --- | ---: | --- |
| MALLOC_SMALL | 98 MiB | 应用、Slint、Fontique、Syntect、ColorSync 等大量小对象和 allocator 高水位 |
| MallocStackLogging performance tool data | 63 MiB | 仅诊断运行存在，不属于生产进程 |
| IOSurface | 37 MiB | 2× backing scale 下的窗口/离屏像素表面 |
| IOAccelerator graphics | 28 MiB | 图形驱动资源 |
| graphics-owned physical footprint | 20 MiB | renderer/驱动拥有的未映射物理页 |
| MALLOC_LARGE | 7.5 MiB | 一个长期保留的大分配 |

`heap` 显示 186,402 个 live nodes、约 84.45 MiB 可归因堆对象。已定位的主要应用侧常驻源头如下：

1. **重复注册系统 UI 字体：约 9.32 MiB。** `setup_pr_review_fonts()` 在进程启动时读取并向 Fontique 注册 `/System/Library/Fonts/SFNS.ttf`（7.5 MiB）和 `SFNSItalic.ttf`（1.0 MiB）。分配调用栈为 `std::fs::read -> setup_pr_review_fonts -> fontique::Collection::register_fonts`。Fontique 为后续字形解析长期持有这些 blob，因此与 PR 页面关闭无关。SF Mono 的三张 OTF 合计仅约 316 KiB，不是这一项的大头。
2. **项目 Logo 按原始分辨率解码并永久缓存。** Home 卡片实际绘制 32×32 px，但五张远程 Logo 分辨率分别为 360×360、658×658、474×474、450×340 和 500×500。`load_image_from_bytes()` 直接 `to_rgba8()`，再把原尺寸 `SharedPixelBuffer` 放入线程本地 `PROJECT_LOGO_IMAGE_CACHE`。已观察到至少约 4.3 MiB 的活跃 RGBA buffer。
3. **Logo 解码连带的 ColorSync 常驻：约 13.04 MiB。** 堆中有 60 个 `CMMParaCurveTag::MakeInvertedTRC` 节点合计 10.87 MiB，以及 12 个 `MakeTRC` 节点合计 2.17 MiB。调用时序和远程 Logo 解码一致；这是带色彩配置图片批量解码的进程级缓存成本。归因强度为“高度相关”，不能声称每一字节都由某一张 Logo 单独拥有。
4. **Syntect 默认语法集与正则编译缓存。** 首次进入 PR 后，`SYNTAX_SET: LazyLock<SyntaxSet>` 使用 `SyntaxSet::load_defaults_newlines()` 加载完整默认语法集合；首次高亮还会在 `fancy_regex/regex_automata` 中编译并缓存正则。调用栈中存在 656 KiB 单块及多组 64/48 KiB 分配。由于静态 `LazyLock` 生命周期等于进程，关闭 PR 不会释放。
5. **FemtoVG renderer 命令/字形缓冲。** 约 592 KiB 的可增长 buffer 可直接归因到 `femtovg::Canvas::render_triangles -> append_cmd -> Vec::grow`，其设计是复用峰值容量；图形侧另外保留当前窗口所需的 IOSurface 和驱动资源。它们不是 Slint route model。

因此，“回到 Home 后还有东西占用”这个判断成立，但应准确表述为：PR 业务对象已经释放，剩余的是启动级字体 blob、Home 自身的原尺寸 Logo/ColorSync 资源、首次 PR 激活的进程级 Syntect 正则缓存，以及 renderer/高 DPI 图形高水位。Activity Monitor 不下降不能再归因于 PR route 强引用泄漏。

后续按独立 A/B 批次执行，避免多项同时修改后无法判断收益：

1. **批次 A：Logo 缩略图管线。** 在进入 Slint model 前把 raster Logo 限制到满足 32×32 绘制和 2×/3×显示的合理上限，建议 128×128；保持纵横比，不放大原图。验收同时比较 Logo RGBA live bytes、ColorSync live bytes、Home footprint/RSS 和视觉清晰度。
2. **批次 B：字体注册去重。** 先验证 Fontique/CoreText 能直接解析 `.SF NS`，若成立则删除 8.5 MiB 的 SFNS 手工 blob 注册，仅保留约 316 KiB 的 SF Mono faces。验收必须包含 PR 文件树字体截图对比和 heap 中 `std::fs::read` live bytes。
3. **批次 C：语法高亮生命周期。** 将完整 `SyntaxSet`/主题从进程静态单例改成 PR review session 所有，或构建经过支持扩展白名单裁剪的 syntax dump；关闭最后一个 PR session 后可释放。该批改动面较大，必须以高亮正确性测试、首次打开耗时、重复打开耗时和关闭后 live heap 四项共同验收。
4. **批次 D：renderer 峰值。** 固定 logical size、backing scale 和显示器，对 PR 行渲染临时 clone、离屏层、FemtoVG command capacity 与 IOSurface 数量逐项剖析。不要使用强制 allocator trim 伪造释放效果。

批次 A 和 B 是目前证据最强、风险最低的前两项；批次 C、D 不应在没有独立基线的情况下混入同一提交。

### 11.8 第 7 批：项目 Logo 缩略图管线

日期：2026-07-17。

改动：所有 raster 项目 Logo（远程缓存、本地路径、Base64 和默认图）在进入 Slint `Image` cache 前按比例限制到最大 128×128；小图不放大，SVG 保持矢量。项目卡片实际绘制为 32×32 logical，128 px 覆盖 2×/3× backing scale 并保留 4×余量。

A/B 使用相同的 macOS `MallocStackLogging` + `heap` 工具。A 组为原尺寸解码；B 组为 128 px 候选。由于 A 组完成过 PR 路由、B 组只在 Home，本批不使用二者的总 footprint 差值证明收益，只比较调用栈明确归属于 Logo 的 live pixel buffers；ColorSync 作为辅助观察项。

| 指标 | A：原尺寸 | B：最大 128 px | 结果 |
| --- | ---: | ---: | --- |
| 五张远程 Logo 尺寸 | 360²、658²、474²、450×340、500² | 等比缩至最长边 128 | 符合绘制上限 |
| 可识别 live RGBA buffers | 至少约 4.3 MiB | 约 0.32–0.40 MiB | 约 -91% |
| 最大单张 Logo buffer | 约 1.70 MiB | 约 64–80 KiB | 约 -95% |
| ColorSync 大块 live heap | 约 13.04 MiB | 约 9.78 MiB | 约 -25%，但测量路由不完全相同，只作辅助证据 |

候选堆中原来的约 1.70、1.02、0.90 和 0.62 MiB Logo 大块均消失，替换为五个约 64–80 KiB 的缓冲；这是确定性的应用所有权下降，不依赖 macOS 是否立即归还 malloc page。2× 副屏截图中 128 px 默认 Logo 显示清晰，未观察到缩放锯齿。

结论：本批通过并保留。它不会解决 PR 首次高亮或高 DPI IOSurface 的主要成本，但消除了 Home 页面与项目配置预览中不必要的原图常驻，且没有改变缓存命中和网络下载行为。

### 11.9 第 8 批：macOS 系统 UI 字体注册去重

日期：2026-07-17。

改动：PR 文件树继续指定 `.SF NS`，但不再由应用读取并向 Fontique 重复注册 `SFNS.ttf` 和 `SFNSItalic.ttf`。Fontique 0.10 的 CoreText backend 已明确扫描 Apple 隐藏的 SFNS 文件，并把 `System Font` / `.SF NS` 配置为 `system-ui`；应用先通过 `family_id(".SF NS")` 使用该系统集合，仅在不可用时回退到 `Helvetica Neue`。SF Mono 三张 OTF 保持原实现，避免改变代码区字体。

启动阶段 A/B 使用相同 800×600 logical、2× backing scale、Logo 128 px 候选和 `MallocStackLogging` 环境：

| Home 稳定指标 | A：手工重复注册 | B：系统 Fontique family | 结果 |
| --- | ---: | ---: | --- |
| physical footprint | 107 MiB | 97 MiB | -9.3% |
| live heap | 34.53 MiB | 28.60 MiB | -17.2% |
| `std::fs::read` 字体 blobs | 5 个、9.32 MiB | 不再出现在大分配列表 | 重复 SFNS blobs 消失 |
| `MALLOC_LARGE` | 7.5 MiB | 0 | SFNS 正体大块消失 |

两组 ColorSync 节点数量存在波动，因此不能用总 heap 差值冒充全部字体收益；确定性证据是 SFNS 的 7.5 MiB/1.0 MiB 分配和对应 `setup_pr_review_fonts` 调用栈消失。

PR 页面阶段验收使用同一个 200 文件、24,843 行 PR；首次文件高亮后关闭审核并返回 Home：

- 用户确认 PR 文件树字体显示正常，未观察到 family、字号或字重退化。
- PR controller 的 detail/original/highlight/source 持有量和根 PR models 再次全部归零。
- live heap 为 51.92 MiB；大分配列表中没有 7.5 MiB `MALLOC_LARGE`。
- `std::fs::read` 只剩 3 个 112 KiB 分配、合计 344 KiB，对应保留的三张 SF Mono OTF；SFNS 正体/斜体没有回到应用私有堆。
- `vmmap` 能看到系统按需映射 `SFNS.ttf`：两个 7.55 MiB 虚拟映射的实际 resident 分别约 688 KiB 和 1,568 KiB，合计约 2.2 MiB，属性为共享 alias / copy-on-write，可由系统回收。这不是原实现中不可回收的 8.5 MiB私有 font blob。
- 返回 Home 后诊断进程 physical footprint 为 202 MiB，其中 62 MiB 是 MallocStackLogging performance data；不能把该绝对值当成生产运行内存。

旧 A 组执行过 Chat → PR → Home，新 B 组执行 PR → Home，renderer/highlighter 路径不完全相同，因此不使用旧 268 MiB 与新 202 MiB 的总 footprint 差值宣称字体收益。字体收益以调用栈和所有权严格判定：约 8.5 MiB 应用私有 blob 消失，换成当前约 2.2 MiB 的系统可共享 resident 映射，净物理成本至少降低约 6 MiB，且 Home 启动 footprint 的同条件 A/B 为 -10 MiB。

结论：本批通过并保留。它既不是把成本延迟到首次进入 PR，也没有牺牲 GitHub 风格文件树字体；生产路径继续使用 `.SF NS`，但所有权回归 Fontique/CoreText 系统字体机制。

### 11.10 第 9 批：PR session 级语法高亮资源生命周期

日期：2026-07-17。

改动：删除进程级 `LazyLock<SyntaxSet>` / `LazyLock<Theme>`，改为 `WorkflowPrReviewController` 持有的 `Option<Arc<HighlightResources>>`。首次文件高亮仍在 `spawn_blocking` 中构建，避免阻塞 Slint event loop；同一 PR session 的所有文件共享；打开新 PR、切换项目或关闭审核时清空 owner。正在运行的旧任务可持有临时 `Arc` 到任务结束，但序列号会阻止旧结果回写，任务结束后资源自然释放。

A 组为第 8 批完成后的进程静态 Syntect 资源，执行同一个 PR → Home，关闭后 live heap 51.92 MiB、physical footprint 202.2 MiB。B 组使用 session owner，打开同一 200 文件、24,843 行 PR，连续高亮 4 个文件后关闭并返回 Home：

| 指标 | A：进程静态 | B：PR session owner | 结果 |
| --- | ---: | ---: | --- |
| SyntaxSet/Theme 构建次数 | 首次后进程永久 1 次 | 同一 session 1 次 | 文件切换正常复用 |
| 首次资源构建耗时 | 未单独记录 | 21 ms | 后台执行，可接受 |
| 关闭前资源 owner | 静态、不可清空 | `highlight_resources_loaded=true` | 符合预期 |
| 关闭后资源 owner | 仍常驻 | `highlight_resources_loaded=false` | 确定性释放 |
| 关闭动作即时 allocator 变化 | 无明显变化 | 约 29 MiB 转为 reusable | 资源页可回收 |
| Home live heap | 51.92 MiB | 27.99 MiB | -23.93 MiB，约 -46.1% |
| Home physical footprint | 202.2 MiB | 183.5 MiB | -18.7 MiB，约 -9.2% |

B 组实际高亮了 4 个文件、缓存峰值为 579 行/12,166 tokens/约 0.56 MiB，A 组只高亮 1 个文件；B 组工作量更大但关闭后仍显著更低。首次文件从资源加载日志到高亮应用约 1.32 秒，旧组从 diff 应用到首文件高亮约 1.41 秒；网络、调度和行数会造成波动，因此这里只能判定“没有观察到性能退化”，不能声称固定百分比提速。后续三个文件没有再次输出资源加载日志，证明 session 内没有重复构建 SyntaxSet。

关闭后诊断 RSS 数字没有同步下降，因为释放的约 29 MiB malloc pages 先变为 allocator reusable；physical footprint 和 live heap 已下降。这正是本批目标：释放所有权并使页面高水位可由系统回收，而不是调用非可移植的 allocator trim。

结论：本批通过并保留。现有 Rust/TypeScript/Vue/HTML/CSS/Slint 高亮单元测试全部通过；用户按步骤完成多文件切换和返回 Home，未反馈显示异常。

### 11.11 第 10 批：PR diff 单份基础数据与按文件写时复制

日期：2026-07-17。

问题：旧 `ReviewCache` 同时持有完整 `detail` 和 `original_files`，并且文件选择、hunk 展开和刷新树等路径会临时 clone 整个 cache。即使用户从未展开上下文，同一个 200 文件 diff 也会长期保留两份完整行数据。该设计的成本随 PR 总文件数和总行数增长，而不是随用户实际展开的文件数增长。

改动：缓存改为一份 `Arc<PullRequestDiffDetail>` 不可变基础数据，加一个按原始文件索引存储的 `HashMap<usize, PullRequestDiffFile>` override。普通浏览、筛选、文件切换和高亮只读取基础文件；首次展开某文件时只 clone 该文件，后续继续修改同一 override；`Collapse non-diff lines` 删除该 override 并直接重新使用基础文件。网络结果、cache hit、文件选择、刷新树和完整文件查看均不再 clone 整个 diff。关闭审核、切换项目或打开新 PR 仍会清空基础数据与全部 override。

A/B 条件保持一致：800×600 logical、2.00 scale（1600×1200 physical）、同一 200 文件/24,843 行 PR、连续高亮 4 个文件，均使用 `MallocStackLogging=1`。A 为第 9 批完成后的旧双份缓存，B 为本批写时复制候选版。

| 指标 | A：完整 diff + original files | B：Arc base + per-file override | 结果 |
| --- | ---: | ---: | --- |
| 未展开时完整 diff owner | 2 份 | 1 份 | 结构上减少 50% 的规范化 diff 长期持有量 |
| B 组加载后 owner 日志 | 不适用 | base 200 文件/24,843 行/约 3.05 MiB；override 0 | 未展开不再复制完整数据 |
| 第 4 个文件高亮应用时 footprint | 约 332.1 MiB | 328.25 MiB | -3.85 MiB，约 -1.2%；与移除一份 diff 的量级一致 |
| PR 页面稳定 footprint | 未作为旧批验收指标单独记录 | 210.91 MiB | 仅作 B 组稳定态记录，不虚构百分比 |
| 返回 Home live heap | 27.99 MiB | 27.37 MiB | -0.62 MiB，约 -2.2% |
| 返回 Home physical footprint | 183.5 MiB | 176.24 MiB | -7.26 MiB，约 -4.0% |

B 组第 4 个文件高亮完成时，日志为 `base_files=200`、`base_lines=24843`、`base_heap_estimate_bytes=3195344`、`override_files=0`；高亮缓存为 4 个文件、322 行、3,217 tokens、约 0.15 MiB。关闭前仍只有这一份基础 diff，关闭后 base、override、highlight、source 和 SyntaxSet/Theme owner 全部为 0。返回 Home 后约 32.27 MiB malloc pages 变为 reusable；RSS 不立即下降仍是 allocator 高水位行为，不代表业务对象仍被持有。

自动交互同时验证了两态按钮：点击 `Expand all lines` 后图标和 tooltip 切换为 `Collapse non-diff lines`，完整上下文可见；再次点击恢复原折叠 diff 和 `Expand all lines`。单元测试另外确认只有实际展开的文件进入 override，其他文件继续引用基础数据。

曾有一轮为方便定位控件把窗口切到全屏，导致 IOSurface 增至约 203 MiB 且退出全屏后仍被 renderer 保留；该轮 RSS/footprint 已判定为无效样本，没有混入上表。最终 B 组重新启动进程并全程保持固定小窗口。

结论：本批通过并保留。收益不像语法资源生命周期优化那样大，但它移除了一个随大型 PR 线性翻倍的确定性所有权问题，且展开能力、折叠恢复、文件切换和缓存上限均保持不变。

### 11.12 第 11 批：高亮 token 与 Slint 行模型压缩

日期：2026-07-17。

约束：PR 的语言不可预知，因此本批没有采用扩展名白名单，也没有裁剪 Syntect 的默认语法集合。运行时仍完整调用 `SyntaxSet::load_defaults_newlines()`，语言覆盖与第 10 批一致；优化对象只是高亮结果和 Slint 传输模型。

问题：旧实现把每段空白单独转换成一个 `HighlightedToken` 和一个 Slint `Text` delegate，并在每个 token 上保留 UI 未读取的 `columns` / `indentation`，每行还重复传输未被 UI 读取的 `display-columns`。非 hunk 行同时在 `content` 和 token 文本中保存同一份代码。大型文件因此产生大量小对象、delegate 和重复字符串。

改动：空白并入相邻的可见 token，相邻且颜色、粗体和斜体完全相同的 token 合并；只保留行级最大列数供横向布局使用。删除 Rust/Slint token 中未使用的 `columns`、`indentation` 和行模型中的 `display-columns`；普通代码行不再向 Slint 重复传输 `content`，hunk 行仍保留原文。单元测试逐字符重建原始行，确认缩进、内部空格和尾部空格不丢失。

由于远端 PR 在旧批次后更新过，本批没有复用旧的 3,217-token 数字。严格 A/B 在当前同一远端快照上重新执行：800×600 logical、2.00 scale（1600×1200 physical）、同一 200 文件/24,843 行 PR、相同的前 4 个文件和点击顺序，均启用 `MallocStackLogging=1`。A 仅临时撤销本批压缩，B 为候选实现；两组均保留完整 Syntect 语言资源和前 10 批改动。

| 指标 | A：独立空白 token | B：压缩 token/model | 结果 |
| --- | ---: | ---: | --- |
| 4 文件高亮行数 | 2,808 | 2,808 | 输入完全一致 |
| 高亮文本 bytes | 78,244 | 78,244 | 文本无丢失 |
| 高亮 token | 11,980 | 3,194 | -8,786，约 -73.3% |
| 高亮缓存 heap estimate | 647,338 B | 295,898 B | -351,440 B，约 -54.3% |
| 稳定 live heap | 63,884,056 B | 62,110,824 B | -1,773,232 B，约 -2.8% |
| live heap nodes | 240,525 | 215,698 | -24,827，约 -10.3% |
| 第 4 文件应用瞬时 footprint | 318.72 MiB | 314.50 MiB | -4.22 MiB，约 -1.3% |
| PR 稳定 physical footprint | 约 202 MiB | 约 198 MiB | -4 MiB，约 -2.0% |
| 首文件加载到高亮应用 | 约 1.35 s | 约 1.33 s | 未观察到性能退化 |

候选截图确认 Markdown 标题、链接、列表、中文、缩进和空白位置正常；Rust/TypeScript/Vue/HTML/CSS/Slint 高亮测试继续通过。B 组减少的 live heap 大于 Rust 高亮缓存自身的 0.34 MiB 差值，符合减少 Slint model 字段、字符串复制和约 8,786 个文本 delegate/小对象带来的连带收益。

结论：本批通过并保留。它没有通过减少语言支持换内存，而是在完整语言覆盖不变的前提下，显著降低每个已高亮文件的 token 数和 UI 小对象数量。

### 11.13 第 12 批：PR diff 惰性 Slint 行模型

日期：2026-07-17。

约束：继续完整加载 `SyntaxSet::load_defaults_newlines()`，不按扩展名裁剪语言，也不改变第 11 批的 token 合并规则。本批只优化 Rust 高亮结果到 Slint 行 model 的所有权和物化时机。

问题：旧实现完成高亮后立即把文件的每一行转换成 `WorkflowPrDiffLine`，再放入 `VecModel`。`ListView` 虽然只实例化可见 delegate，但数千行文件仍会一次性复制全部行字符串和 token 子 model；大型文件切换的内存增量随总行数线性增长。

改动：新增只读 `LazyPrDiffLineModel`。完整 `PullRequestDiffFile` 与高亮结果由共享 `Arc` 保持，Slint 请求某行时才调用现有 `to_slint_line`；转换后的行进入 64 项 LRU，快速回滚时复用，超出窗口后立即淘汰。文件切换、hunk 展开和完整文件查看仍通过 event loop 原子替换 model，旧 model 在新高亮完成前保持挂载。单元测试覆盖 128 行顺序访问并确认缓存始终不超过 64 行。

远端 PR 在上一批之后更新，因此本批重新建立同快照 A/B，未复用旧样本。两组均为 800×600 logical、2.00 scale（1600×1200 physical）、同一 206 文件/27,369 行 PR、相同前 4 个文件和点击顺序，并启用 `MallocStackLogging=1`。A 临时切回 eager `VecModel`，B 为惰性候选；四个文件共 3,348 行、3,687 tokens、90,325 文本 bytes，输入完全一致，其中第二个文件约 2,932 行。

| 指标 | A：eager 全行 `VecModel` | B：64 行惰性 LRU | 结果 |
| --- | ---: | ---: | --- |
| 2,932 行文件切换前稳定 RSS | 263.23 MiB | 264.19 MiB | 两进程启动基线有约 0.96 MiB 偏差 |
| 该文件应用瞬时 RSS | 267.19 MiB | 266.61 MiB | 应看组内增量，不直接比较绝对值 |
| 该文件组内 RSS 增量 | +3.96 MiB | +2.42 MiB | -1.54 MiB，约 -38.9% |
| 前 4 文件后稳定 RSS | 272.62 MiB | 270.91 MiB | -1.71 MiB，约 -0.6% |
| 前 4 文件后稳定 physical footprint | 200.19 MiB | 198.33 MiB | -1.86 MiB，约 -0.9% |
| 单文件已转换 Slint 行上限 | 文件全部行 | 64 行 | 大文件 UI 行持有量由线性变为有界 |

`MallocStackLogging` 的 performance-tool pages 仍包含在两组 footprint 中，但两组使用相同诊断配置，因此组内差值可比较；这些页面不属于生产构建成本。候选截图确认 Markdown 标题、链接、列表、中文和 diff 背景正常；在 2,932 行 `Cargo.lock` 上连续发送 25 次、每次 12 行的快速滚动事件后，视口从约第 80 行直接到 2,708 行附近，行号、hunk、增删背景和代码文本均连续，没有空白、错行或卡死。完整语言高亮测试、惰性缓存上限测试和所有 lib 单测均通过。

结论：本批通过并保留。总进程 footprint 改善不大但稳定可重复，而 2,932 行文件切换的新增 RSS 降低约 39%；更重要的是，未来超大文件不再为不可见行预先构造整份 Slint 字符串/token model。

## 12. 剩余值得执行的优化批次

下面只保留有明确 owner、可独立 A/B、且不牺牲任意语言 PR 高亮能力的项目。它们不是已经确认的收益，必须逐批实现、逐批验收；未达到门槛即回滚，不把多项改动混入一个候选版本。

### 12.1 后续 A/B 的统一前提

1. 把当前大型 PR 响应保存为脱敏本地 fixture，固定 206 文件、27,369 行和前 4 个文件顺序。远端 PR 会变化，后续不得再用两个不同远端快照比较。
2. A/B 使用相同构建 profile、renderer、窗口 logical size、backing scale、显示器和操作脚本。每组至少冷启动 3 次，报告中位数和离散范围，不只挑一次最优值。
3. `MallocStackLogging=1` 只用于 owner、live heap 和节点数归因；生产成本另用不带 MSL 的相同场景测 RSS、physical footprint、IOSurface 和 graphics region。两种数据不得交叉比较绝对值。
4. 固定执行 Home → PR → 2,932 行文件 → 快速滚动 → 前 4 文件 → Home，并额外执行 PR 打开/关闭 5 轮。记录首次增量、稳定值和 warm-up 后每轮增长斜率。
5. 除非目标 owner 至少下降 20%，或生产稳定 RSS/footprint 至少改善 1 MiB 且三次方向一致，否则不以噪声为由保留复杂实现。任何文本错误、滚动回退、明显 CPU 增长或功能缺失均直接判失败。

### 12.2 第 13 批：选中文件零拷贝与 Diff 元数据紧凑化（优先级 P1）

原惰性 model 为获得独立生命周期，会把 `cache.file(index)` 完整 clone 后放入新的 `Arc`。因此选中 2,932 行文件时，基础 diff 与当前 model 会短暂或长期各持有一份该文件的行字符串。每行的 `kind` 还是重复 `String`，对 `context`/`addition`/`deletion`/`hunk` 这类有限状态产生不必要的小分配。

该批分成两个可单独回滚的子步骤：

- 13A（2026-07-26 已实现）：基础文件表已改为逐文件 `Arc<PullRequestDiffFile>`。Provider 详情转换时移动文件内容，不深拷贝行；惰性 model、高亮任务、文件树、完整文件查看和未展开状态共享同一文件。只有 hunk 展开创建该文件的 copy-on-write override，收起后恢复基础 `Arc`。单元测试验证未展开选择与基础文件 `Arc::ptr_eq`，展开后才分离，收起后重新共享。`[Workflow][PR Review][Memory]` 同时新增按 `Arc` 去重的 `owned_detail_*` 字段，覆盖完整 PR、当前 range 和 4 项 range LRU；旧的 `base_*` 字段只表示当前激活详情，不能再被误当作全部 diff owner。
- 13B：在 Provider 归一化边界之后，把行类型转换为紧凑 enum，避免为每行分配状态字符串；不要让 Slint 或 Provider adapter 重新承担字符串解析。

13A 使用仓库内进程隔离基准完成了可重复 A/B。固定输入为 206 个文件、27,369 行、每行 112 bytes；A 组精确复现旧实现的“基础 detail + 选中文件深拷贝”，B 组执行当前逐文件 `Arc` 转换和选择。每个单元格启动 5 个独立测试进程，避免 allocator 高水位在 A/B 之间串扰；下表为中位数：

| Selected file | Variant | Median RSS delta | Median physical-footprint delta | Owned heap |
| ---: | --- | ---: | ---: | ---: |
| 2,932 lines | baseline | 6.20 MiB | 5.67 MiB | 5.10 MiB |
| 2,932 lines | candidate | 5.67 MiB | 5.12 MiB | 4.59 MiB |
| 2,932 lines | saved | **0.53 MiB (8.6%)** | **0.55 MiB (9.6%)** | **0.51 MiB (10.0%)** |
| 24,843 lines | baseline | 9.94 MiB | 9.41 MiB | 8.75 MiB |
| 24,843 lines | candidate | 5.56 MiB | 5.02 MiB | 4.59 MiB |
| 24,843 lines | saved | **4.38 MiB (44.0%)** | **4.39 MiB (46.7%)** | **4.17 MiB (47.6%)** |

复现命令：

```sh
scripts/measure-pr-review-memory-ab.sh 5
```

5 轮原始数据中同一单元格的 RSS delta 离散不超过 32 KiB。RSS/footprint 的下降与确定性 owned-heap 下降方向和数量级一致，因此可以排除一次性内存压缩造成的假阳性。该基准证明 13A 的选中文件 owner 优化；它刻意不创建 Slint 窗口或 renderer，不能被用来声称 PR 整页总 RSS 同比例下降。真实 Provider/renderer 的端到端 A/B 仍应使用同一脱敏响应、固定窗口物理尺寸单独执行。必须继续验证关闭 PR 后全部 owner 归零。13B 只有在独立 A/B 有可测收益时才实施，不因“结构更优雅”强行保留。

### 12.3 第 14 批候选：高亮 token 改为源码区间（优先级 P1）

第 11 批已经减少 token 数，但 `HighlightedToken` 仍为每个 token 保存一份 `String`，与 diff 行内容重复。第 12 批只限制 Slint 行物化，没有消除 Rust 侧 4 文件高亮 LRU 中的文本复制。

候选表示为“一份规范化显示行文本 + 若干样式区间”：token 只保存 UTF-8 byte range、颜色和字体样式；Slint 请求可见行时才按合法字符边界生成当前 64 行所需的字符串。Tab 展开应在建立区间前完成，CJK、组合字符、Emoji 和 CRLF 必须有边界测试。Syntect 仍使用完整 `SyntaxSet::load_defaults_newlines()`，不得通过删语言或扩展名白名单获得收益。

A/B 使用高 token 密度的 Rust、TypeScript、Vue、HTML、CSS、Slint 和 Markdown fixture，比较高亮缓存 live bytes、malloc nodes、首次高亮耗时和快速滚动 CPU。通过条件是高亮缓存目标 owner 至少下降 20%，文本逐字符重建完全一致，首次高亮和滚动没有明显退化；否则保留当前字符串 token 方案。

### 12.4 第 15 批候选：Logo 解码期缩略图与 ColorSync 峰值（优先级 P1，先归因后实现）

第 7 批把进入 Slint cache 的 raster Logo 限制为 128×128，已经解决长期 RGBA cache 过大；但当前管线仍可能先完整解码原图，再缩放并丢弃大 buffer，ColorSync 的进程级缓存也可能在这一步建立高水位。该项只在新的 allocation trace 仍能把明显峰值关联到 Logo 解码时继续。

候选方案是在解码器支持时直接请求缩略图尺寸，或在 macOS 使用 ImageIO thumbnail path；磁盘缓存保存尺寸受控的处理结果，SVG 继续保持矢量，不放大小图，也不通过删除 ICC profile 破坏颜色正确性。远程、本地、Base64 和默认 raster 路径必须共用同一入口。

A/B 使用固定的五张原始 Logo，记录解码峰值 RSS、最终 RGBA live bytes、ColorSync live bytes、Home 首次加载耗时和 2×/3×截图清晰度。若 decoder 仍必须完整解码，或 ColorSync/峰值没有下降，则不增加平台专用复杂度。

### 12.5 第 16 批候选：PR 工作区离屏层与高 DPI 图形成本（优先级 P1）

完成业务 model 优化后，剩余大头主要在 IOSurface、IOAccelerator 和 renderer 高水位。`pr_review_workspace.slint` 中存在多个 14–18 px 阴影、clip、opacity overlay 和弹出层；它们不一定都有问题，但需要逐个确认是否形成与窗口尺寸相关的离屏 surface。

按单组件 A/B 审计文件过滤 popup、overflow popup、顶部/浮动卡片和全窗口遮罩：先确认隐藏状态是否仍挂载或分配 layer，再分别测试移除阴影、缩小阴影边界、改用边框、仅在打开时挂载。禁止一次删掉全部视觉效果，也禁止盲目添加 `cache-rendering-hint`；后者可能减少 draw call，却增加 GPU memory。

验收固定 800×600 和 1400×900 logical、2× backing scale，分别记录 PR 打开、popup 开关、窗口放大/缩小 10 轮后的 IOSurface 数量、graphics footprint、稳定 RSS 和截图差异。只有能定位到具体组件、同尺寸 physical footprint 至少下降 5 MiB、并且交互与 GitHub 风格无可见回退时才保留。

### 12.6 第 17 批候选：Slint/renderer 升级矩阵复测（优先级 P2）

当前 FemtoVG/OpenGL 是本项目已测得的较低 RSS 默认；现有 WGPU/Metal feature 虽降低部分高 DPI footprint，但稳定 RSS 更高，因此不能直接切换。后续只有在 Slint、FemtoVG/WGPU 或 macOS 图形栈升级后，才使用同一 fixture 重新跑 OpenGL、WGPU/Metal 和可选 Skia 三方矩阵。

升级批必须先增加 `naga`/`wgpu` target 日志过滤，避免诊断日志自身污染 CPU、磁盘和内存结果。切换生产 renderer 的通过条件不是单看 footprint：Home、Chat、PR、小窗口、大窗口和 10 轮 resize 的稳定 RSS 都不能回退，IOSurface/graphics footprint 要有明确改善，字体、阴影、代码滚动和文件树拖拽必须通过视觉与交互回归。条件不满足时只更新可安全更新的依赖，保持当前 renderer 默认。

### 12.7 暂不重复或仅条件触发的项目

- **Chat 路由卸载：** 当前已有 overscan、增量高度前缀缓存和最多 32 条消息的虚拟窗口；第 4 批清空 model 无可测收益。只有使用数千条历史、反复向上分页后观察到 warm-up 后线性增长，才重新设计 viewport 生命周期和滚动锚点。
- **系统默认 CJK fallback：** 第 3 批已证明当前组合会从 22.4 MiB Hiragino 映射切到约 74.6 MiB PingFang，不能重复尝试同一方案。
- **裁剪 Syntect 语言：** PR 语言不可预知，明确不做；第 14 批只能改变结果表示，不能减少语言覆盖。
- **强制 allocator trim/purge：** 不解决 owner 和峰值，只会制造暂时好看的 RSS；不进入生产方案。
- **立即启用 WGPU/Metal：** 第 5 批稳定 RSS 已回退 8.6%–13.5%，等待依赖或驱动变化后再按第 17 批复测。

推荐顺序为 **13A → 13B → 14 → 15（有调用栈证据才做）→ 16 → 17**。第 13、14 批继续收敛随大型 PR 线性增长的应用侧成本；第 16、17 批处理当前更大的高 DPI 图形成本，但风险更高，必须在业务内存结构稳定后单独测量。

## 13. 2026-09-20：Slint 1.18.0 小窗口切页诊断与新一轮优化批次

本节替代第 10、12 节作为**本轮后续工作的执行顺序**；旧实验及其通过/拒绝结论保留，不追溯改写。本轮批次编号固定写作“2026-09 批次 N”。稳定行为仍以 [Tooltip 规范](docs/specs/tooltips.md)、[架构](docs/architecture.zh.md)与对应 Feature spec 为准；本节是诊断和实验计划，不是已经达成的收益承诺。

### 13.1 已观察事实与尚未复现的边界

用户在普通 `cargo run` 下依次访问项目列表、Hook 设置、PR 详情、Workflow、Chat，小窗口 RSS 超过 400 MB；报告时进程已经关闭。因此无法对原进程事后进行 `vmmap`、`footprint` 或分配调用栈归因。后续第 13.3 节已经用冻结真实 PR/Chat/Hook fixture 完整执行同类路线：三个 Release 进程都低于 300 MiB，辅助 Debug 进程超过 300 MiB 但主要差异来自代码/链接文件映射。**仍未复现原来的 400 MB，也没有证据把它定性为持续泄漏。**

2026-09-20 的隔离诊断使用 `97438f3` 源码、默认 features、Slint 1.18.0、FemtoVG/OpenGL、2 个 Tokio worker，固定 800×600 logical、scale=2（1600×1200 physical）。macOS 27.0（26A428）、Mac14,9、arm64。离线、独立配置及单实例 namespace 下，项目、PR、Chat 和 Hook 配置均为空。实际路线为 Home → Projects → Settings General → Hooks → Chat → Home，Release 再重复 Settings → Chat → Home。

以下为每个有效 profile **一个新进程的阶段末周期样本**，单位 MiB；不是正式优化 A/B，也不是三进程中位数：

| 阶段 | Debug RSS | Debug footprint | Release RSS | Release footprint |
| --- | ---: | ---: | ---: | ---: |
| Home 稳定 | 190.89 | 96.63 | 125.97 | 95.19 |
| Projects 空列表 | 198.30 | 102.13 | 128.77 | 97.86 |
| 首访 Settings 并切到 Hooks | 259.84 | 165.81 | 186.00 | 165.45 |
| 随后空 Chat | 252.02 | 155.77 | 186.78 | 165.83 |
| 第一轮回 Home | 252.11 | 155.74 | 186.83 | 165.81 |
| 第二轮回 Home | 未执行 | 未执行 | 187.02 | 165.99 |

据此可以分开提出以下结论和假设：

- **Debug 基线成本已经实测。** Projects 阶段应用自身 `__TEXT` 与 `__LINKEDIT` 的实际 resident 合计差额约 66 MiB；不是把可执行文件磁盘大小等同 RSS，也不包含 cargo/rustc 的内存。产品验收以 Release 为主，Debug 成本另列。
- **设置首次访问的图形台阶已经实测。** Release 的 IOAccelerator graphics 从 Projects 约 6416 KB 增至 General 约 42 MB、随后 Chat 约 47 MB；IOSurface 始终约 37 MB。`footprint` 分类使用工具原始 MB/KB 舍入值，且采样时刻不同于周期日志，不能逐项相加还原 RSS。
- **Hook Tooltip 是初始诊断提出的具体归因假设。** Settings General 也挂载 Hook portal；5 个固定动作的透明度施加于全窗口 `GitHubFloatingOverlayHost`，内部才是小提示框。Slint 1.18/FemtoVG 对复杂 opacity 子树可能创建离屏 layer。800×600×2²×4 bytes×5≈36.6 MiB 仅是 RGBA 理论规模；同尺寸 stencil 另约 9.15 MiB。第 13.4 节后续 A/B 支持图层面积归因，但候选因交互门禁失败已回滚，当前源码仍使用全窗 host opacity。理论估算不替代实测，也不能把全部 graphics 增量都算给 Tooltip。
- **本次隔离诊断当时没有真实 PR/Workflow 和大 Chat 数据。** 第 13.3 节随后补入冻结的 `tianlai10000000/usher#1`、Chat 历史、Hook 与项目 fixture，并记录了 PR/Chat 离页 owner；它仍没有达到 300/400 MiB。PR 的 count-bounded 高亮/源码/range 缓存、不同离页路径及 Chat 历史不能据此认定为原 400 MB 的主因，释放 owner 也不保证 allocator/renderer 立即等额归还 RSS。
- **重复访问是否线性增长必须单独判断。** 空数据 Release 第二轮已接近平台期，不能由“首次增加且离页后保留”推导出“每轮都会泄漏”。小窗口切页的 graphics 台阶也不能与旧版 resize/IOSurface 高水位混为同一问题。

完整诊断与原始日志保留在本地忽略目录 [rss-diagnosis-20260920](memory-benchmark-results/rss-diagnosis-20260920/analysis.zh.md)。其中 `debug-clean-routes.log`、`release-routes.log`、`*-resident.vmmap`、`*.footprint` 为有效证据；并行编译期间受内存压缩干扰的首轮 Debug 和一次错误的 `footprint -f MB` 输出保留但不纳入比较。该目录不随 Git 分发，因此本节同时保留了关键条件、数值与结论边界。

### 13.2 本轮统一 A/B、交付与回滚门禁

沿用 [memory backlog 的 Mandatory A/B evidence gate](docs/memory_optimization_backlog.md#mandatory-ab-evidence-gate) 与 Default decision thresholds，不使用历史第 12.1 节的较低门槛替代本轮门禁。

1. **一批一个主要变量。** 比较当前已接受生产基线与一个候选；固定源码/补丁身份、二进制 SHA256、features、renderer、机器、显示器、logical/physical size、scale、fixture、日志开关、worker 数、预热和采样协议。A/B 顺序交错，避免把编译负载、系统压缩或温度变化当收益。
2. **每个 A/B 单元至少 3 个独立 Release 进程。** 同一进程内多个周期不能冒充多个独立进程。Debug 与 Release 的差异是构建归因，不是 Tooltip 优化 A/B。所有配置和测试数据必须隔离，不修改用户真实项目、凭据、PR 或消息。
3. **同时报告多种指标。** 保留原始每次采样、失败运行和命令；列出 settled median、离散范围、maximum observed peak、首次访问增量和重复周期斜率，以及 candidate-minus-baseline 的绝对值/百分比。RSS、physical footprint、graphics/IOSurface、malloc in-use 与 owned bytes 分列，不挑单一好看的数字，也不将 malloc reserved/slack 当作等额驻留碎片。
4. **预声明目标和门槛。** 稳定目标指标中位数至少改善 **10 MiB 或 5%**，且至少三次方向一致；交互 p95 不得退化超过 **5%**，不得新增 warm-up 后持续循环增长。目标外指标、启动/关闭、功能、视觉和打包如有实质回退，不能用目标收益抵消。既有 CI 的绝对预算不因新实验而放宽。
5. **测量与功能验收都必须有证据。** 使用适配当前 fixture 的比较器校验协议/元数据；新增测量工具要用正、负 fixture 自测。真实 compiled factory 的原生绘制不能由测试后端或 AX 标签替代；编译通过不等于 Tooltip 通过。声明未执行项目，不把主观“流畅”代替量化 p95。
6. **结论仅为采用、拒绝或证据不足。** 未达收益、噪声/混合结果、视觉/行为错误或关键验收缺失，不宣称优化完成；拒绝候选时回滚本批生产行为，只保留有用诊断、原始证据及拒绝记录。不得把失败候选作为下一批基线。文档/工具正确性改进可单独交付，但不能声称已减少运行内存。
7. **每批结束即交接。** 填写本节的批次记录，运行 `cargo fmt --all`、`cargo check --locked`、`cargo test --lib --locked`、`git diff --check` 及专项检查。交付后先供用户试用；下一批需用户确认当前结果并明确要求继续，不自动扩展范围。

### 13.3 2026-09 批次 0：补齐原现场基线（已完成，Release 低于 300 MiB）

**固定输入与路线：** 使用提交 `b123e2e` 的当前源码构建、默认 features、FemtoVG/OpenGL、2 个 Tokio worker 和原生 800×600@2 小窗口。隔离 fixture 包含 10 个项目、5 个 Hook、99 条 RAG + 24 条 AI Chat 记录、6 个 Logo，以及 `tianlai10000000/usher#1` 的只读冻结回放（213 个文件、26,963 行）。路线固定为 Home → Projects → Hooks → Closed PR list → PR #1 → Back → Chat → Home，再热重复 Hooks → PR #1 → Chat → Home。外部回放不读取用户 Token、不执行写请求；二进制、快照和每次规范化请求均校验 hash。

正式有效组只有 R1、R7、R8，三者使用相同 Release 二进制 SHA256 `23cda3dd7f1b727fdccbb8d1d9dc9369591100ba765c62cdec3e6e202d022ad9`。每进程 12 个阶段、每阶段静置后采 5 点，共 180 个样本；全部 route/尺寸正确、`compressed=0`、正常退出且二进制/快照未变化。下表口径为“三个进程各自阶段五点中位数的中位数”；方括号是三个进程中位数范围，斜线后是该阶段 15 个原始点的最大观测值，单位 MiB：

| 阶段 | RSS 中位数 [范围] / 采样峰值 | footprint 中位数 [范围] / 采样峰值 |
| --- | ---: | ---: |
| `home_start` | 126.000 [125.234–126.031] / 126.031 | 94.126 [94.017–94.751] / 94.751 |
| `projects_cold` | 132.906 [129.469–133.141] / 133.141 | 100.845 [97.142–101.579] / 101.579 |
| `hooks_cold` | 183.703 [182.719–185.141] / 185.141 | 161.954 [160.470–163.814] / 163.814 |
| `workflow_closed` | 203.422 [198.969–204.031] / 204.031 | 179.767 [175.095–181.064] / 181.064 |
| `pr_initial` | 210.094 [208.203–221.563] / 221.563 | 184.626 [182.157–196.017] / 196.017 |
| `workflow_after_pr` | 211.719 [209.891–224.234] / 224.234 | 186.814 [184.439–198.486] / 198.486 |
| `chat_cold` | 211.156 [207.734–213.000] / 213.000 | 183.533 [175.361–185.908] / 185.908 |
| `home_after_cycle1` | 205.922 [205.594–207.766] / 207.766 | 165.657 [164.361–175.220] / 175.220 |
| `hooks_warm2` | 207.750 [205.609–213.141] / 213.141 | 171.767 [165.704–175.157] / 175.157 |
| `pr_warm2` | 227.859 [225.469–231.734] / 231.734 | 186.626 [185.579–199.329] / 199.329 |
| `chat_warm2` | 217.516 [207.031–225.359] / 225.359 | 184.157 [165.329–185.486] / 185.486 |
| `home_after_cycle2` | 206.969 [206.953–217.297] / 217.297 | 165.126 [165.126–174.501] / 174.501 |

进程内诊断记录的 lifetime RSS HWM 分别为 R1 `238.73`、R7 `237.64`、R8 `243.05 MiB`，中位数 `238.73 MiB`；physical-footprint HWM 分别为 `222.11`、`221.55`、`228.45 MiB`。三次 Release 均严格通过 **lifetime RSS HWM <300 MiB** 门槛；最差 R8 仍有 `56.95 MiB` 余量，也没有复现原报告的 400 MB。

两轮最终 Home 的配对结果如下。RSS 变化中位数为 `+1.047 MiB`、范围 `-0.813–+11.703 MiB`，方向混合；graphics 与 IOSurface 三次均无轮间增长，live malloc 也基本不变。R7 的波动对应 `Malloc Large` dirty/reserve 高水位，不能定性为页面 owner 持续存活或持续泄漏：

| 运行 | Home RSS 轮 1→2 | RSS Δ | footprint Δ | graphics dirty Δ | live malloc Δ | Small+Large dirty Δ |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| R1 | 205.922→206.969 | +1.047 | +0.766 | 0.000 | +0.05 | +0.828 |
| R7 | 205.594→217.297 | +11.703 | +8.844 | 0.000 | -0.07 | +8.859 |
| R8 | 207.766→206.953 | -0.813 | -10.094 | 0.000 | -0.10 | -10.156 |

**Debug 辅助归因：** D1 的第一轮最终 Home RSS 为 `306.063 MiB`，相对 Release 中位数高 `100.141 MiB`；footprint 只高 `11.938 MiB`。D1 的 clean `__TEXT + __LINKEDIT` 为 `102.313 MiB`，Release 中位为 `13.563 MiB`，差 `88.750 MiB`；live malloc 高 `9.44 MiB`、graphics 高 `4.484 MiB`。D1 lifetime RSS HWM 为 `342.94 MiB`，11 个已完成阶段中 7 个超过 300 MiB。因此普通 `cargo run` 的 >300 MiB 已复现，但主要是 Debug 代码/链接映射常驻，而不是业务堆等量增加。D1 使用旧回放并被一次连接错误标为 fatal，且缺第二轮最终 Home，只作为描述性归因，不能补入 Release 门禁或循环斜率统计。

**回放合同修正：** `replay.mjs` 现将且仅将 `ERR_HTTP_REQUEST_TIMEOUT + raw_packet_bytes=0 + socket_bytes_read=0` 记为 transport probe，每进程最多允许一次，并单独写 `transport-probes.jsonl`。任何带输入的非法 HTTP、未录制接口、凭据、写请求、其他错误或第二次 probe 仍是 fatal；9 组正/负 fixture 自测通过。R8 有 1 个符合合同的零字节 idle probe，实际 32 个成功请求、2,829,856 bytes 与 R1/R7 的规范化请求集合一致，`fatal/misses/mutations=0`，因此有效。

**无效与中断运行：** D1 为上述不完整 Debug 描述样本；R2 虽完成 12 阶段但旧回放为 fatal；R3/R4 在定位零字节 timeout 时分别只到 6/4 阶段；R5 锁屏后停止、只有首轮 8 阶段；R6 会话中断、只有 4 阶段且无正常 exit/summary，其 stale active marker 已改名保留。它们均未用于正式统计，也没有覆盖失败证据。

末轮 Home 的 graphics dirty 三进程均为 `78.125 MiB`，其中 IOAccelerator graphics `50.047 MiB`、owned-unmapped graphics `28.078 MiB`；IOSurface 另为 `36.641 MiB`。相对 Home 起点，graphics 配对增量中位数 `+59.563 MiB`，IOSurface 不变；live malloc 仅从约 `14.35 MiB` 升至中位 `18.77 MiB`。末轮 `vmmap` 的 malloc allocated 中位约 `19.0 MiB`、resident `53.2 MiB`、fragmentation `24.1 MiB`。这些分类快照与 RSS 不同刻且相互口径不同，不能相加还原 RSS，也不能把 allocator resident/slack 当成等量业务 owner。

原始证据保留在本地忽略目录 `memory-benchmark-results/route-baseline-20260921/`；冻结 API body、Chat/Hook 内容和本机路径不随 Git 分发。结论是 **Release 路线通过 <300 MiB 门槛，Debug 路线越线且主要由调试构建映射解释，原 400 MB 仍未复现**。这不是 renderer、`release-fast` 或 Debug profile 候选的 A/B，也不改变既有 WGPU、opaque window、整页销毁和 allocator purge 的拒绝结论。

### 13.4 2026-09 批次 1：Hook Tooltip 小范围透明层（已实验，拒绝并回滚）

**假设：** 隐藏 Hook Tooltip 的全窗 opacity layer 是设置首次访问 graphics 台阶的重要来源；将透明度选择移至内容尺寸的小 Surface，可减少层面积，同时保持原生工厂文字绑定稳定性。

**已回滚实验候选的唯一变量与边界：**

- 候选仅修改应用自有 `ui/components/settings/github_hook_tooltip.slint` 的 5 个 Hook Tooltip，将 action-kind 对应的 opacity 从全窗定位 host 下移到小 Tooltip Surface。
- 全窗 host 继续负责窗口根定位、flip/shift、像素对齐和跨 ScrollView/Dialog 的裁剪边界；小 Surface 继续使用原测量尺寸、主题与文字。
- 固定 action-kind、每动作独立且常驻的文字/测量节点、claim/lease、按钮命中和 `ComponentFactory` 本地状态隔离全部保持；不能改回共享动态文案、`if` 重建文字或条件重建命中节点。
- 不销毁整个 Settings，不改变 bridge/routing 生命周期，不修改 Material vendor 或引入私有 Material 路径，不捎带处理 PR/Chat。Messages 的 3 个同类 host **不在本批范围**，若后续处理，必须作为独立候选重新建立 A/B。

**前置与性能验收：** 保存改动前 Release 二进制和 SHA256；A/B 使用相同离线空配置、窗口/缩放、两 worker、日志开关及原生 route 脚本。固定小窗口执行 Home → Projects → Settings General → Hooks → Chat → Home，首次访问和后续至少 10 个相同周期分列；另以有 Hook 行和 Dialog 的隔离 fixture 验证交互，不把不同 fixture 的绝对内存混算。RSS 为主目标，footprint/graphics 为归因与不回退指标；按 13.2 的三进程、收益、p95 和斜率门禁判定，不以理论纹理尺寸代替测量。

**Tooltip 专项验收：** 按 [Tooltip acceptance matrix](docs/specs/tooltips.md#acceptance-matrix--交付验收)，在真实 compiled Settings factory 内检查：中文/英文与运行时往返切换；跨行 A → B → A 至少三轮；短 → 长 → 短；图标/提示居中；窄 → 宽 → 窄和四边/底边避让；滚动、切 Tab、离页重入；Dialog 打开/关闭/重开及返回列表；键盘 Tab/Enter/Space、disabled/busy、激活后 dismiss。固定文字、实际绘制、测量宽度和动作身份必须一致，截图和自动回归分工留证。

**拒绝与回滚：** graphics 台阶未按假设下降、RSS 未达门槛、出现截字/串文案/残留/锚点错误或关键验收不足时，记录拒绝或证据不足，恢复本批透明度放置；不得改动 Settings 生命周期或 Material 来掩盖结果。通过本批也只能宣称修复已测 Hook 路径，不能宣布原 400 MB 或 resize 高水位全部解决。

**正式采样协议：** A1 → B1 → B2 → A2 → A3 → B3，每次全新离线进程。固定 `SLINT_BACKEND=winit-femtovg`、800×600 logical、1600×1200 physical、**原生 scale=2**、2 worker，不设置 `SLINT_SCALE_FACTOR`。试运行期间显示环境曾为 1×，强制 2× 还出现自动化命中异常；这些均不用于正式比较。A1 首次采样因仍预期 1× 被门禁拒绝、未写样本；根据真实进程上下文确认原生 2× 后，全部正式 A/B 锁定 2×，不再混用尺寸。收益门槛未放宽。不在 cargo/rustc 运行时采样。人工原生导航到每阶段后，采样器额外静置 2.5 秒，再采 5 次（间隔 0.5 秒）；每进程先算阶段中位数，再比较三个独立进程的中位数和范围。人工调用间隔不计入导航耗时。依次采 Home、Projects、General、Hooks、Chat、Home，再 Settings → Chat → Home；随后额外 10 轮 Settings → Chat → Home，每轮 Home 单独采样，最后采 `after_stress`。RSS 主目标为 `after_stress`；压缩非零、尺寸/路由不符、缺阶段/重复采样、非正常退出或 trace 不完整的运行不纳入有效组。

交互门禁预声明为 **warm Settings 导航 callback 入口 → 目标路由首个 AfterRendering 的 p95**，相对基线不得增加超过 5%；每个进程排除该路由首次导航。这是应用内部 action-to-frame，不包含 OS 输入分发与 GPU present，也不声称测到了完整鼠标到屏幕延迟。其他路由同指标和按路由分组的 BeforeRendering → AfterRendering 耗时作为辅助记录。A/B 共同使用临时 `src/tooltip_benchmark.rs`：精确 fixture marker、offline=1、绝对隔离 state root 三重开关，8192 帧/1024 导航的有界缓冲，退出 event loop 后写文件；默认启动不采样。帧与导航两种 complete 均须为 true，丢样、覆盖或未配对不能按零耗时进入分位数。该模块及装配在最终回滚时一并移除；完全相同的测量源码保留在证据目录 `artifacts/tooltip_benchmark.rs`。

每进程每个 warm 路由为 11 次导航，采用 nearest-rank p95（在 11 个样本时即最大值），再比较三个进程 p95 的中位数；单进程高值不丢弃。相同诊断开关也计入双方动作路径，不能把这一有界小场景推导为所有输入、滚动或真实大数据页面的延迟结论。

**执行结果与拒绝记录：**

| 记录项 | 状态/证据 |
| --- | --- |
| 基线/候选源码、补丁与二进制 SHA256 | 基于 `97438f3`，A/B 均加入相同的有界计时模块；唯一 UI 差异为五处 opacity 的所属节点。二进制及锁文件身份见下。 |
| 命令、硬件、显示器/尺寸、fixture、协议与原始目录 | 两方均 `cargo build --release --locked`；Mac14,9、macOS 27.0、默认 features、FemtoVG、原生 800×600@2、两 worker、空离线配置。原始目录 `memory-benchmark-results/tooltip-layers-20260920/`，不随 Git 分发。 |
| 每组独立 Release 进程数、无效/失败样本 | A1/B1/B2/A2/A3/B3 六个新进程，均 100 个内存样本、37 次完整匹配导航、压缩为零、正常退出。早期 visual/缩放失败保留且不混入正式组。 |
| RSS/footprint median、range、peak、斜率及 A/B delta | v1 RSS 中位数 −43.922 MiB（−25.04%），三对均下降；完整分项见下，不能据此单独采用。 |
| graphics/IOSurface、malloc in-use、owned bytes 归因 | 首访 General 的 IOAccelerator dirty 中位数 41.578→4.844 MiB；IOSurface 均 36.641 MiB；最终 malloc in-use 中位数 17.26→17.21 MiB。支持图层面积归因，不是业务数据回收。 |
| p95 交互指标与 Tooltip 原生验收矩阵 | v1 Settings 导航 p95 中位数 4.815→10.974 ms（+127.91%），超过 +5% 门槛，明确失败。原生功能矩阵见下；长列表滚动因候选被拒绝未继续补测。 |
| 自动检查、未执行项、回归与回滚 | 比较器正例和 22 个拒绝用例通过；v1 compare 因性能门槛失败返回 1。生产 UI、临时计时模块/装配、仅约束该候选的测试与规范改动均已回滚；最终仓库检查结果见收口记录。 |
| 最终采用/拒绝/证据不足 | **拒绝并回滚，不得作为下一批基线。** 内存收益真实可重复，但交互门禁失败。未扣除帧调度等待、放宽门槛或以补测好值替换失败；未启动批次 2。 |

**v1 实测结果（六个独立进程均为有效证据，性能失败不是无效样本）：**

| 进程 | 最终 RSS 中位数 / MiB | 最终 footprint 中位数 / MiB | warm Settings p95 / ms |
| --- | ---: | ---: | ---: |
| A1 | 176.625 | 157.486 | 8.507 |
| B1 | 131.438 | 98.298 | 11.325 |
| B2 | 131.469 | 100.376 | 10.974 |
| A2 | 175.391 | 153.767 | 4.679 |
| A3 | 167.359 | 147.095 | 4.815 |
| B3 | 132.516 | 98.017 | 10.726 |

RSS 三进程中位数为 175.391→131.469 MiB，范围分别 167.359–176.625 与 131.438–132.516 MiB；对应三对变化为 −45.188、−43.922、−34.844 MiB。Footprint 中位数为 153.767→98.298 MiB（−55.469 MiB，−36.07%）。A3 在前段发生无压缩的驻留回落，保留原值，不剔除或拿更高峰值替换。

周期采样的每进程最高 RSS 中位数为 176.62→133.59 MiB，六组最大观测值分别 177.55 与 134.45 MiB；独立报告的 lifetime RSS high-water 中位数为 180.14→136.86 MiB。周期采样最大值不是连续监控峰值。第 3–10 轮 RSS OLS 斜率中位数为 0.00130→0.02288 MiB/轮，平台范围中位数为 0.0156→0.1719 MiB，候选三组范围均不超过 0.204 MiB。该短窗口没有出现数十 MiB 的重复增长，但不足以证明长期无泄漏，微小正斜率也不能单独定性为泄漏。

首访 General 的 dirty graphics 分类合计中位数为 68.109→19.578 MiB（其中包含 owned-unmapped graphics）；后者为 26.531→14.734 MiB，**不能再次相加**。最终 graphics 合计为 74.125→25.656 MiB，IOSurface 在全部组均为 36.641 MiB。这里是异步快照的归属分类，不是 RSS 的可加分解，也不等于项目自有缓存字节；本批没有修改业务数据 owner。

辅助路由同样出现延迟差异：Chat p95 中位数 5.746→11.134 ms，Home 5.152→11.001 ms。慢样本的 action→BeforeRendering 区间常约 8 ms，而渲染通知之间多为 2–3 ms。检查本机 Slint 1.18.0 源码：`i-slint-backend-winit/frame_throttle/apple_display_link.rs` 的 redraw 请求恢复 CADisplayLink，实际 draw 在 display tick；FemtoVG 的部分准备/clear/flush 也发生在 BeforeRendering 之前。因此“输入相位/帧调度影响小样本”是可检验假设，**不是已确认根因**。现有 trace 不能将这个区间全部称作等待；验收指标仍须保留整个 action-to-frame。

证据身份与复核：

- A：`artifacts/baseline-final`，SHA256 `5b18260c656a2150352effc9c5c3685cb32855ee451d36851e8dae469e022300`。
- B：`artifacts/candidate`，SHA256 `982632a74cfea45c7077c5d1e1e26d1bbdb067251302d68501b447cfd8872fd7`。
- 共同计时源码 SHA256 `94d43c443852c32f4a4caf02c4b58e9c530d6da35d80bcf45e388bc5414e6a17`；`Cargo.lock` SHA256 `b793dad054187c60d1133a8d50b10a100b6e230105a4d372429455e84919ac91`。rustc 1.97.1 / cargo 1.97.1。
- `artifacts/` 保留双方 Slint 源码、计时源码和候选 patch；`runs/{A1,B1,B2,A2,A3,B3}/` 保留进程身份、执行副本 hash、应用日志、100 个逐条样本、完整 trace 与 footprint 分类。
- `node memory-benchmark-results/tooltip-layers-20260920/compare.mjs selftest` 校验比较器；`.../compare.mjs compare` 重新生成 `comparison.json` / `comparison.md` 并因本次 p95 失败返回 1。启动使用 `.../probe.mjs run A1 baseline-final` 或 `.../probe.mjs run B1 candidate`，原生导航后逐阶段 `sample ID PHASE:ROUTE`，最终 `stop ID`。已存在 run 拒绝覆盖，不可重跑覆盖失败证据。

**决策与复验边界：** 未采用当前候选。一次更大样本、固定种子输入节拍扰动的 `timing-v2.mjs` 已准备并通过脚本语法及正/负 fixture 自测，但**没有启动、没有性能结果**。独立审阅指出：改变预热/输入节奏是在测试另一调度分布，即使新协议通过，也不能自动推翻 v1 三个候选进程均出现的长尾；应保留“混合证据”，而非不断重测直到通过。因此本轮按原门禁回滚，保留候选二进制/补丁供后续定位。重开批次 1 时先查清 BeforeRendering 前的差异，再预声明新候选和确认性协议；不得直接把这份未通过的改动当作已接受基线。

**原生 Tooltip 验收记录（隔离 fixture，不计入空数据内存组）：**

| 检查 | 实际结果与边界 |
| --- | --- |
| 英文／中文／切回英文 | `visual2` 原生窗口验证五个固定标签：编辑、删除、停止、查看执行、清空历史；运行时语言往返后 Tooltip 文字和宽度更新，无 Tooltip 串文案。 |
| 跨行 A → B → A、短 → 长 → 短 | 编辑第一行／删除第二行至少三轮，两语言均核对实际绘制；另核对短的 Stop／停止与长的 View execution／查看执行流程、Clear execution history／清空执行历史。 |
| 中心与视口 | 编辑提示对准图标；删除和清空历史在右边界内移，完整文字未被裁切；800×600 → 原生 zoom 宽窗口 → 800×600 后 Tooltip 仍正确。界面没有贴左／上边缘的 Hook 动作，未通过改代码伪造这些锚点。 |
| Dialog、busy、动作与 dismiss | 隔离 `/bin/sh` 的 `sleep 45` 测试已实际启动并取消；运行时编辑/保存/查看执行等受禁用约束，Stop 可用；结束后 View execution 正确打开该测试的 Job，返回列表、关闭及重开 Dialog 无 Tooltip 残留。 |
| 键盘与路由 | `visual5` 验证 Tab 到编辑按钮时出现正确提示，Enter 和 Space 分别打开正确编辑 Dialog；关闭、Home 离页和重入 Settings 均无悬空提示。 |
| 滚动／分区切换 | 已检查滚轮输入和 Hooks ↔ General 切换会撤销旧 Tooltip；两行 fixture 未产生真实长列表位移。因性能门禁失败，长列表滚动锚点未继续补测，不标为通过。 |
| 发现的独立既有缺陷 | 宽窄往返后列表行名／图标和部分控件文字偶尔发生绘制复用；`visual3` 的旧 host-opacity 基线同样复现。打开首行仍是正确定义，固定 Tooltip 正确。记录为 [UI-141](bug-history.md)，本批不声称修复该 factory 绘制问题。 |

原生截图保留在本次工具交互记录；对应进程、隔离配置、Hook/Job 日志位于本地 `runs/visual*`。`visual` 第一次在沙箱内启动即 SIGABRT、无应用日志；`visual4` 强制缩放的命中异常不作为验收通过证据。`visual2/3` 期间存在编译/压缩或不同缩放条件，只用于功能观察，不用于 RSS 结论。

**最终检查与收口记录：**

| 检查 | 实际结果 |
| --- | --- |
| 双方 `cargo build --release --locked` | 通过，编译完整 root UI；共同计时版本的 A/B hash 已冻结。 |
| 候选 Slint 结构回归 | 7 项通过；旧 host-opacity 负例被拒绝。候选回滚后移除这份仅约束候选模式的生产测试，不把旧模式伪装成已迁移。 |
| 测量工具 | `probe.mjs selftest`、`compare.mjs selftest` 通过；正式 `compare` 因 p95 门槛失败返回 1。`timing-v2.mjs` 仅语法/fixture 自测通过，未运行真实复验。 |
| `cargo fmt --all`、`cargo check --locked`、`git diff --check` | 回滚后均通过。 |
| `cargo test --lib --locked` | 沙箱内首次 640 passed / 10 failed / 2 ignored；10 项均因本机 mock HTTP 监听端口被禁止而失败。获准在沙箱外重跑后 **650 passed / 0 failed / 2 ignored**。 |
| UI policy、i18n、脚本测试 | 通过；回滚后 Python 脚本测试 37 项通过。候选存在时的 44 项也已通过，但不计作当前 37 项之外的新生产覆盖。 |
| `./scripts/check-quality.sh` | 首轮整体返回 1：架构规模检查失败，库测试受上述 socket 权限限制；后者单独重跑通过，架构问题未修改。不能标注整个 quality gate 通过。 |
| 未执行 | 完整 `cargo test --locked` / doctest、临时计时模块的 8 个 Rust 单元测试、真实长列表滚动与删除确认框专项补测、v2 性能复验。已知 logger doctest 失败未在本轮重新验证；不能用回滚后的库测试冒充临时模块测试。 |

架构规模检查的 5 项既有超限为：`src/ai_review/controller.rs` 3876/3850 行、`ui/main.slint` 3051/3000 行及 421/410 properties、`ui/views/workflow/pr_review_workspace.slint` 4501/4500 行、`src/workflow/pr_review/controller/actions.rs` 688/580 行。通过 `git diff --exit-code HEAD -- <上述文件>` 确认均与本轮开始基线一致，未扩大本批范围修理。编译还保留既有 unused import/dead-code 和 macOS compact-unwind 大小警告，不作为新内存归因。

最终工作树仅保留本诊断文档和 `bug-history.md` 的 UI-140/141 记录；`src/lib.rs`、Hook Tooltip、架构/spec、Material、Cargo manifests/lockfile 均无本轮改动。开始时工作树干净，没有覆盖用户无关修改。所有测试进程已退出，六个正式 A/B 均正常关闭；隔离配置、失败运行、候选源码与二进制保留在本地忽略证据目录。第一批交接时下一批未启动；后续经用户要求启动的第二批见 §13.5。原始用户路线的 400 MB 和 resize 高水位仍未宣称解决。

### 13.5 2026-09 批次 2：PR 字节预算与重数据生命周期（真实回放 pilot 已完成，后续原生验收待解锁）

**当前状态：** 用户已明确要求继续第二批，并要求“先固定真实 PR 复现，再治理大文件缓存和离页引用”。只读 owner、路由和已有测试工具审计后，用户指定 `tianlai10000000/usher` Closed 默认列表第一条；已读取并冻结 PR #1。没有改动缓存上限、生命周期、Material 或 UI，没有启用上一轮失败的 Tooltip 候选，也没有开启旧 `USHER_MEMORY_WORKFLOW_LIFECYCLE=clear`。**冻结快照不等于复现了 400 MB，也不等于完成 Release A/B 或收益验收；后续实测单列记录。**

**目标与范围：** 分开测量完整/范围 Diff、展开 override、高亮、不可变源码、Conversation/Commits 与根 Slint model；按 Arc 去重。仅对实测占用显著的高亮/源码/range 缓存增加总字节预算、LRU 和超大项不入缓存策略，保留现有条目上限。轻量导航/输入状态与可重建重数据分离；离页时是否清重数据必须由明确的 reachability/lifecycle 合同决定，并同步处理根 model 引用。

**前置与验收：** 先完成固定 PR 场景归因，不能仅打开旧 `workflow_lifecycle=clear` 实验开关。验证自有 Back/侧栏/历史前后导航、range/file 切换、全文展开/恢复、取消/过期任务、选区、精确 extent、返回重入与最后可达路由被历史逐出。关闭后的归零/保留必须与合同一致；缓存阈值覆盖单个超大项和多个范围总量。端到端 Release 三进程 A/B 同时量化首次高亮、重入延迟与请求次数。

**拒绝与回滚：** 不删除未提交内容，不减少语言覆盖，不把同一数据反复请求或重新高亮的明显延迟代价藏在内存收益后面。2026-08 的清理实验虽然 footprint 改善，但 RSS/峰值门禁失败；旧失败不是本轮已采用方案。仅 owned bytes 下降却未通过预声明进程指标与交互门禁时，不称为 RSS 优化。

#### 13.5.1 当前源码审计：已确认结构，不等于实测内存主因

| 对象/路径 | 当前行为 | 本轮测量必须区分 |
| --- | --- | --- |
| Diff 高亮、源码、range 缓存 | `src/workflow/pr_review/cache.rs` 的 `BoundedCache` 仅按 4/3/4 条限制并执行 LRU，没有总字节预算。高亮 token 各持 `String`；源码 cache hit 与 insert 会复制 `String`。 | 条目数量、正文/容量估算、实际 unique owner、hit/miss、重高亮和读取次数分别记录。源码共享化与字节预算不能合成一个 A/B 变量。 |
| 单份远端源码 | `src/git/source.rs` 已限制 UTF-8 源码 8 MiB、Base64 JSON 12 MiB。 | 不能称为无限单文件下载；3 个缓存项仍可接近 24 MiB 正文，另有临时副本。实际分布待真实 PR。 |
| 完整 PR、活动 range、range LRU | `all_detail`、`base_detail` 和缓存项共享 `Arc<SharedDiffDetail>`，每个文件又为独立 Arc；活动 range 即使被 LRU 驱逐也可能仍由 `base_detail` 持有。 | 按 detail/file 两层 allocation 去重；缓存 eviction 不等于最后引用释放或 RSS 同额下降。 |
| 展开的文件 | `expanded_files` 为无条目/字节上限的 HashMap；首次展开单文件 copy-on-write，collapse 删除 override。 | 这是用户可见的展开状态，不能直接套 LRU 静默收起；如需释放重数据，须先保留轻量展开/视口状态并独立验证重建。 |
| 活动全文与 UI | 全文投影重新分配每行文本和高亮；全文高亮不进入 Diff 高亮 LRU。`diff_model.rs` 的惰性 model 强持 file/highlight/projection，64 行限制仅适用于转换行缓存。 | 现有 cache 日志漏掉不在缓存中的活动全文、高亮与投影；不能把“只有 64 行 UI”当作完整 PR 内存上限。 |
| Find、Conversation/Commits、geometry | Find 的 key/model/results 可继续持 file/projection；根 model 保留 Conversation/Commits。`text_geometry.rs` 的线程本地布局缓存限 256 项，但不随 PR reset 清空。 | 这些 owner 和在途/待投递 Worker 都不包含在现有 cache 总量中；未量化前不归因为本次 400 MB 的主因。 |

`controller.rs::log_memory_diagnostics` 已提供 base、owned_detail、override、highlight、source 与 `highlight_resources_loaded`，可复用但不能直接相加为 PR 总量。`owned_diff_storage_stats` 只按 detail Arc 去重，且现有 heap estimate 遗漏部分 Arc/File 本体、容器容量和缓存 key 等开销。补统计时按各类 allocation 在同一 snapshot 去重，只输出分类计数/字节，不输出内存地址或源码内容；容量估算必须继续标为 **estimated owned bytes**，不得称为精确 allocator/RSS。

**路由清理不能一概而论：**

| 入口 | 已核对的当前默认行为 |
| --- | --- |
| PR 页内 Back；当前在 PR 时的原生/快捷键 Back | `controller.rs::navigate_back` / `lib.rs` 调同一个 PR Back；除 commit 下钻返回分支外，先失效请求并清 controller/root model，再执行 Router Back。 |
| PR → 侧栏或普通 navigate | PR 子树卸载，但 controller、根 `workflow-pr-review-*` models 与 Find owner 不因此自动清空。 |
| 从其他页面历史 Back 返回 PR | Router 恢复路由，复用保留 model；不是重新从 Provider 打开 PR。 |
| PR Back → Workflow → Forward | PR 已被 Back 清空，Forward 却可能恢复 PR 路由；没有对应 reopen/fetch。空详情风险为静态发现，**尚待真实 fixture 验证**。 |
| 选择项目/打开其他 PR/强制刷新 | 走对应 reset/invalidate 路径；同项目重新选择也会清理 PR。 |
| Workflow/PR 从双历史栈彻底逐出 | `app_lifecycle.rs` 会触发 hook，但 `workflow/mod.rs::release_unreachable_state` 默认直接返回；只有旧实验变量为 `clear` 时才清 PR/list owner。不得把旧实验行为当作默认生产事实。 |

架构中“历史不可达时释放审核”和 PR spec 中“离开详情清理”的概括与上述默认实现并不完全一致。本轮先记录差异，未据此更改清理策略，也未声称这些 spec 已更新。根 history 只存 Route、不存多份 PR 身份；不能承诺能够恢复任意历史 PR。未提交 Review/Conversation 草稿目前是条件挂载页内属性，这一既有设计不能成为新方案丢弃草稿的理由。

另发现两个需要确定性测试的异步风险，**目前仅静态审计，未证明是 RSS 主因**：

- `session_highlight_resources` 在 blocking worker 真正启动时才填共享资源槽。若先 reset 再启动已排队旧 worker，旧任务仍可重新填入同一个槽；完成回写的 generation guard 只保护 UI，不阻止这次 owner 回填。应以 barrier 固定“排队→reset→旧 worker 启动”顺序，区分旧任务暂时存活与新 session 被重新持有。
- 部分 source/expand 完成回调在验证 generation 前清 `code_loading`，可能被旧请求清掉新请求 spinner；生命周期验收应覆盖，但不把该 UI 风险冒充内存归因。

#### 13.5.2 真实快照入口与测量协议准备

仓库现有可运行的 PR 基准均不是可直接复用的真实快照：`measure-pr-review-memory-ab.sh` 是 206 文件/27,369 行的合成 lib 测试，无原生窗口；`large-ranges-v1` 使用合成 base/head；P6 highlight fixture 直接安装人造 controller 数据，部分路径直接设 Route，不能覆盖真实 Router 生命周期。对现有 196 个非 state/runs 的 metadata/manifest 候选盘点，未找到身份完整的真实 PR 冻结记录。历史 §11.13 的 206 文件/27,369 行统计没有 PR 链接、不可变 SHA 或可回放 payload，不能据此宣布已经固定真实 PR。

真实 PR 快照与回放遵循以下前置。不要求用户提供 Token，也不执行任何远端评论、评审、合并或修改：

1. 只读获取并冻结 Provider/repository/PR 身份、base/head/start SHA、采集时间、完整/截断标志；采集结束再次确认 head 及 Provider 提供的 base/start 均未变化，避免混合两个版本。原始内容只放本地忽略目录，不提交凭据、私人对话或源码快照。
2. 保存本次会用到的完整 diff、固定 commit ranges、不可变 base/head 文件、Commits/Conversation，并为各文件记录 SHA256、行数、UTF-8 字节及 token 数；根据真实数据预先确定文件/range顺序，不能事后添加人造大文件使预算触发。
3. 基线优先复放到隔离配置、离线只读 Provider；必须走正常 open/select/range/source callback，缺失的快照读取直接失败，不能静默访问真实网络或直接灌 `ReviewCache` 绕过 owner 路径。记录各类读取和重高亮次数。
4. 固定 Release、机器/renderer、800×600 logical、原生 2×、2 worker；如工具需额外 fixture feature，两方一致并在元数据显式列出。采用三个独立进程的交错 A/B，不在编译或压缩干扰下采样。仪表双方一致，默认生产不启用新增采样。
5. 将原用户路线与缓存压力子场景分列：PR 首次打开、规定文件/range切换、用户显式 Load/View file/展开与收起、PR Back、侧栏离页、历史重入、最后可达目标逐出、关闭后在途任务排空。每阶段同时保留 RSS/footprint、unique owner、读取次数、首次/暖态延迟与重复周期斜率。
6. 交互记录 action callback→目标数据正确且 loading 完成→首个对应 AfterRendering；同时报告计算/读取时间，不能用 50 ms heartbeat 或固定轮询代替 p95。样本数、warm-up、选定主场景与排除条件在采样前冻结，沿用 §13.2 的内存及 +5% 延迟门槛。离线读取不足以反映额外网络 miss 成本时，明确保留该限制，不据此声称在线重入无退化。

#### 13.5.3 候选顺序与必须覆盖的测试

- **2A：单一缓存类别的字节预算。** 先选实测最大且可重建的一类，不同时更改所有缓存；保留现有条目上限，增加 weighted LRU。超预算单项不入缓存但正常显示，不能截断源码、禁用全文或静默收起。昂贵字节成本在生产结果的 Worker 计算一次，不在 event loop 的命中/滚动/resize 路径遍历。预算值根据冻结 fixture 的分布预声明，不先拍阈值再挑数据。
- **2B：离页 owner 与旧任务生命周期。** 待 2A 明确采用或回滚后，独立测量并修复已证实引用路径；同时检查 controller、根 model、Find 和旧 Worker。暂离且仍可返回与显式关闭/替换必须分开，不能 blanket reset 所有导航，也不能直接启用旧 `clear` 方案。
- **可选独立候选：源码共享化。** 只有源码 String 复制确为主要成本时，再单独评估 Arc 共享，不能把它与 2A 的预算收益混算。无收益或未触发预算则如实记录，不扩大合成场景冒充真实现场。

必测：预算恰好/超 1 byte/零值/饱和运算、超大项不驱逐无关小项、同 key 变大/变小/拒绝替换后不返回旧内容、LRU hit提升和 remove/clear记账；detail/file/highlight 多层 Arc 去重；缓存驱逐后活动 model 仍可选择/复制/Find，最后引用释放由 Weak/drop counter 验证；reset 与排队 Worker 的确定顺序；旧响应不得修改新 loading/model；页内 Back、侧栏、历史前后、同/异项目、range、展开、选区与精确 extent、草稿及重入请求次数矩阵。

**身份提供前阶段检查（历史结果，不冒充后续验证）：** `cargo test --lib --locked workflow::pr_review::cache::tests` 1 项通过；`cargo test --test pr_review_core_journeys --locked` 4 项通过；`cargo fmt --all`、`cargo check --locked`、`git diff --check` 通过，`cargo test --lib --locked --quiet` 在允许本机 HTTP 监听的环境下 650 passed / 0 failed / 2 ignored。这些验证现有代码和 callback 契约，不是 controller 生命周期释放、原生绘制或真实 PR 性能验收；未新增针对本节静态风险的回归测试，未重跑完整 quality gate。该阶段未新增/修改 Rust、Slint 或测试代码，未进行 Release A/B；上一批两份未提交文档改动均保留。

#### 13.5.4 用户指定的真实 PR 冻结记录

- 身份：[tianlai10000000/usher#1](https://github.com/tianlai10000000/usher/pull/1)，`Enhance workflows, terminal layout, and UI with new features`，closed/merged；Repository ID `1130209340`。按应用的 created-desc Closed 排序只读查询并核对，不是推测最小 PR 编号。
- base `4fd9e814385588590ebcf2a52dc2621b8ebb6135`，head `54037a9adb96d0a1b75028e120541133882a1fb2`；采集前后核对 identity 一致。共 213 文件、14 commits、2 reviews、1 issue comment、19 timeline items。文件分页 100/100/13，与 detail 的 changed_files 一致，未达截断上限。
- 原始 patch 共 1,164,725 UTF-8 bytes / 26,897 行；93 added、42 modified、78 renamed，66 项没有 patch。这里的原始 patch 行不等于应用规范化行、完整源码行或高亮 token；不能把无 patch 项当作已加载文本。
- 本地私有证据仅存于忽略目录 `memory-benchmark-results/pr-cache-20260920/snapshot/`，不提交原始源码、对话或凭据。`capture-index.json` 保存 35 次响应（含末尾核验）、2 个头像 URL 的原始 bytes/hash/有限响应元数据；源 Token 仅从用户现有 Usher 配置在内存读取并发送给 GitHub API，头像请求不携带鉴权。
- 冻结 index SHA256 `80f5b29fa6544279d432a75f57b2f7e83cf4a1c87cc9fa95cfe93cb6c56c66c4`；预选 workload `scenario.json` SHA256 `2e1fce9c62df311aa4ade29f7470cdc9c6ebf7d7d99478ec15c0d57f946cd631`。记录采集时的原始错误/重试；首次 TLS EOF 未形成有效响应，不作为性能数据。

**在原生测量前确定的文件顺序：** `Cargo.lock` → `ui/main.slint` → `src/git/gitee.rs` → `src/project/mod.rs` → `src/lib.rs` → `ui/views/settings.slint`，分别显式加载大 diff；随后回第三项检查近期命中、回第一项检查重高亮。四个 modified 文件的全文顺序为 Cargo.lock → main.slint → project/mod.rs → lib.rs，再回 main.slint/Cargo.lock 检查 source 命中/重取。展开与 range 阶段置后，避免切 range 清缓存混淆前一阶段。所选最大真实全文为 Cargo.lock head：215,983 bytes / 8,819 行；另外五个 head 分别为 62,692 / 68,359 / 47,515 / 57,274 / 33,272 bytes。冻结了六个文件适用的 11 份不可变 base/head 源码，没有添加人造大文件。

| 范围 | 时间顺序（1-based inclusive） | UI 最新优先索引（0-based inclusive） | base → head | compare 文件 / raw patch bytes |
| --- | --- | --- | --- | ---: |
| R1 | C1…C13 | 1…13 | `4fd9e814…` → `b29e7f3b7f22f49041dc58aef781aff05afd74ea` | 206 / 722,067 |
| R2 | C2…C14 | 0…12 | `0db8e0901eb50d897c24f2888c7e6ead05e61066` → `54037a9a…` | 207 / 760,528 |

Range 顺序为 All → R1 → R2 → All → R1。两个真实 range 覆盖共享 owner/命中回看，但**不能覆盖 4 项 range count 上限的第五项驱逐**；该边界不得标为真实场景已测。

回放通过隔离配置将 GitHub API 指向 `127.0.0.1`，继续走现有 Provider 解码与 controller callback，不向缓存灌人造数据。回放进程只允许精确命中的 GET，缺请求或非 GET 均记录 fatal，不能退回真实网络；逐响应验证原始 SHA，头像改写到同一 loopback 并记录 served SHA。应用不配置 Token，服务也独立拒绝凭据与写操作。`USHER_BENCHMARK_OFFLINE` 本身不是全局网络防火墙，不能只靠这个变量宣称离线；必须检查回放完整性及真实请求路径。首次真实回放预检因 GitHub 原生 Link 使用 `/repositories/1130209340/…` 而非 `/repos/owner/name/…` 被拒绝，未启动监听/应用；只为核对过的该 Repository ID 增加等价分页映射，不允许任意仓库路径。

#### 13.5.5 原生 Release 归因 pilot：结果、边界与下一步

**这不是优化候选的 A/B 验收。** `pilot1` 只运行未修改的基线：HEAD `97438f3f034c39f3b94fa9f0cdaa32aa8a0ed551`，沿用已构建的 Release/default-features 基线二进制，SHA256 `f7236a2bb816a5afabd1d4f8c7bee099456ae13d02c70bf5df13b140d4520e87`；不是上一批被拒绝的 Tooltip 候选，也不是带 Tooltip 计时的版本。本次没有重新构建 Release。运行使用隔离 settings/project registry、空 Token、只读 loopback、FemtoVG、原生 800×600@2、2 Tokio worker；本地仓库只读导航，未执行 Workflow/Terminal/Git 写命令。未启用旧 `workflow_lifecycle=clear` 或 synthetic/test-fixtures 注入。采样时无编译并行；每阶段静置 2.5 秒后采 5 次 RSS/footprint，并校验原生尺寸、PID、路由、诊断时效与压缩为零。表中为各阶段五样本中位数，不是三进程中位数或 p95。

| pilot 阶段 | RSS / MiB | 外部 footprint / MiB |
| --- | ---: | ---: |
| Home 初始 | 120.188 | 90.564 |
| Closed PR 列表 | 146.797 | 114.985 |
| 打开 PR，首文件完成 | 172.406 | 138.064 |
| 六个大 diff 显式高亮 | 181.391 | 140.814 |
| Cargo.lock 全文 | 185.359 | 144.845 |
| 四个文件全文轮换 | 189.469 | 148.923 |
| 两个 range 及回看 | 208.766 | 166.501 |
| Conversation / Commits 加载后返回 Files | 209.328 | 166.892 |
| 侧栏暂离到 Home | 209.281 | 166.798 |
| Workflow 完全历史不可达，Home | 212.078 | 169.517 |
| 重选项目触发既有 reset，再回 Home | 234.047 | 161.548 |

11 阶段共 55 个有效内存样本。外部 footprint 与应用内部瞬时 `phys_footprint_mib` 的采样方式/时点不同，分别保留，不能混作同一列。最后两阶段之间包含项目重选、列表刷新及页面重新挂载，**不是隔离的 clear A/B**；RSS 上升不能全归因于 clear，也不能只挑 footprint 下降宣称优化通过。Mac 锁屏后的周期日志不继续作为原生验收样本。

**缓存归因与 2A 决策：**

| 日志可见的 owner 分类 | 最大 estimated bytes | MiB |
| --- | ---: | ---: |
| 完整 PR detail（含 SHA） | 2,656,351 | 2.533 |
| 完整 PR + R1 + R2，detail Arc 去重 | 6,197,216 | 5.910 |
| 两个 range 相对完整 PR 的增量 | 3,540,865 | 3.377 |
| Diff 高亮缓存 | 2,065,827 | 1.970 |
| 固定源码缓存 | 336,105 | 0.321 |
| 单文件展开 override | 137,341 | 0.131 |

上述是不同阶段的峰值，**禁止直接相加**。同一 snapshot 的“range 增量 + 高亮 cache + source cache”最大约 3.417 MiB；活动 range 仍由 `base_detail` 持有，驱逐对应 LRU 项未必释放它。活动全文及其高亮/投影、Find、完整语法资源等没有完整字节统计，仍为未知而非零。现有统计不含所有容器/Arc 开销，也不是 RSS 的可加分解。

已通过正常 UI 观察六文件 Load diff、近期高亮命中和旧文件重高亮、四文件全文切换、main head source 命中、Cargo head 被逐出后重取、main 全部展开后收起、R1/R2 及 R1 回看。两个 compare 各只读一次。回到全 PR 使用了覆盖全部 14 个 commit 的连续范围，复用原 detail、未新增等价 compare；本次点 All Commits 入口未观察到选中切换，不将该入口标为通过。预选的定向 hunk 和 project/mod.rs 展开尚未完成；不能用 main 的一次全部展开替代其验收。

**本次不实施 2A 字节预算候选。** 该真实场景已触及现有 4 项高亮/3 项源码限制，但当前可选缓存规模不足以支持 ≥10 MiB 或 ≥5% RSS 收益假设。强行压低预算会增加重读/重高亮，而未证明能降低本次大头。此结论仅适用于该 fixture，不否认接近 8 MiB 上限的其他源码可能需要字节预算；不添加合成大文件或其他 PR 来冒充用户现场。

**2B 生命周期已确认的事实：**

1. 侧栏离开 PR 后，Home 根 model 仍为 `pr_review_files=206 / pr_review_lines=168`。历史 Back 能恢复同一 R1，无新增 Provider 请求；这是当前暂离保留语义，不应 blanket reset。
2. 从 PR 直接 Home 后再交替 Project/Home 32 对，共 65 次真实域外导航。23:40:11 Router 明确记录 `Route scope 'workflow' became unreachable`，随后根 model 仍保留 206 文件/168 行。说明 lifecycle 事件已正确触发，而默认 `release_unreachable_state` 跳过清理；不是 Router 没有通知。
3. 23:40:45 重选同一项目时，`project_switch.before_clear` 仍有三个 detail 和语法资源；`after_clear` 的 detail/highlight/source 统计归零、`highlight_resources_loaded=false`，随后根 model 也归零。证明这些数据有正常释放路径，不是不可回收对象；**尚不能称为无限泄漏**。
4. 同一 Project 路由约 8 ms 的相邻 checkpoint：malloc-in-use 71.06→16.95 MiB，RSS 214.17→211.98 MiB，内部 footprint 181.00→154.17 MiB；malloc allocated 仍为 140 MiB，slack/reusable 增大。这符合释放后部分页面仍被 allocator/系统保留的现象，但不能把全部剩余 RSS 归给 allocator。清理同时覆盖 detail、UI model、Conversation/Commits、Find 和语法资源，**不得把约 54 MiB 活跃堆差额精确称为 Syntect 内存**。
5. PR Back/Forward、草稿、迟到 Worker 与 queue→reset→start 屏障测试未完成；本次也未独立证明旧 worker 回填资源槽。2B 仍需分离当前会话与旧任务 owner、补全测量，并以同 fixture 做受控 Release A/B，不能直接启用旧清理开关或把这次顺序观察当收益门禁。

**回放与检查证据：**

- `runs/pilot1/` 保留进程/二进制身份、隔离状态、应用日志、逐样本 ps/footprint 原始输出和阶段记录；`replay-runs/pilot1/` 保留逐请求及 raw→served hash/Link 映射。最终 **34 个请求全部命中，fatal=0 / misses=0 / mutations=0**，回放 summary `complete=true`。响应流量不是 retained heap。
- `analysis-pilot1.json` / `.md` 由 `summarize.mjs` 从原始证据复算并保存输入 hash；汇总脚本语法及时间区/同期求和/缺数据/新鲜 model/截断边界自测通过。以最后外部样本 `2026-09-20T15:41:08.979Z` 为 cutoff，明确排除其后 17 条 periodic/锁屏/退出诊断，不以结束前更晚的数值替换阶段结果。
- 应用正常退出 code=0、确认 `MainWindow dropped`；index hash 在运行前后不变。回放服务已结束，未保留占用端口或用户配置改动。
- `replay.mjs` 语法及 **8 组正/负 fixture 自测通过**：有效 GET 接受，缺页/错 ref/错仓库/hash/头像、非 GET、凭据 header、重复 query 等拒绝。负例 run 精确记录 fatal，而非吞错继续；这只是工具 gate 验证，不是运行时收益 A/B。
- `probe.mjs` 语法及 **6 组自测通过**，拒绝错误 PID/route/尺寸/scale、压缩、过期诊断、已有 run 覆盖及并发应用/构建；footprint 解析覆盖系统真实 `B` 与 `bytes`，拒绝把 MiB 当 bytes。
- 本次停止应用后重跑：`cargo fmt --all`、`cargo check --locked`、`git diff --check` 通过；`cargo test --lib --locked --quiet` 为 **650 passed / 0 failed / 2 ignored（42.96s）**；`cargo test --test pr_review_core_journeys --locked` **4 passed**。保留已有 dead-code、测试 unused-import 及 compact-unwind 警告。未重跑完整 quality gate、doctest；此前记录的 5 项架构规模失败和 logger doctest 基线未宣称修复。采集脚本 `python3 -m py_compile` 通过；曾误用 `node --check` 检查 `.py`，以不支持扩展名失败，改用正确解释器后通过，不是应用回归。
- 当前 Rust/Slint/Material/Cargo 均未修改，保留开始时 `bug-history.md` 和本文件的既有工作区修改。未更新架构/spec 来宣称生命周期已修复。

**当时阻断与恢复点：** 原生工具报告 Mac 已锁屏，测试应用与回放均正常停止。用户随后手动解锁；恢复记录见下一节。锁屏前的 pilot 不因此变成正式 A/B。

#### 13.5.6 解锁后复核与 2B 正式结论（2026-09-21，候选拒绝并回滚）

`pilot2` 沿用同一未修改的 Release/default-features 基线和冻结 index。原生 800×600@2 验证 PR 自有 Back → Workflow → Forward 后，旧标题仍在，但 Files/Commits/Conversation 都为 0，原 merged PR 显示 Open。日志 `runs/pilot2/app.log` 的 `review_close` 与后续路由/model 记录对应；这是候选之前的既有缺陷，不是本轮引入，也不能称 Back/Forward 全部通过。重新从列表打开恢复正常。另已验证 `src/project/mod.rs` 的显式 Load Diff、全部展开（1035→1566 行）、滚动和收起（→1035 行，override 1→0）；Find 键盘入口尝试没有打开面板，不计为通过，定向 hunk/复制/草稿/双语言原生矩阵仍未完成。回放 26 GET 全命中，fatal/misses/mutations=0，应用正常退出，未改用户配置。

确定性 `queue → reset → start` ownership 模型证明旧共享槽会被迟到 Worker 填回；原型正反例 7 项通过，仅为轻量模型而非生产 RSS。候选真实 session-resource façade 的 7 项单测随后通过，使用实际 `ReviewStateFacade` 代际、Barrier 和 Weak 验证复用/隔离/最终释放。旧任务仍可计算并暂持退休槽，`is_loaded=false` 只表示当前槽未初始化，不能冒充全部在途任务排空。

**候选整体变量：** 在 Workflow 域完全历史不可达时结束 PR owned session；保持轻量项目 context、PR 列表、可历史恢复的临时离页状态、Settings 和 Material 不变。结束合同同时要求在排队前捕获 session-local 高亮资源槽、reset 更换槽，且迟到 source completion 先验证代际/文件再触碰新 UI loading。三项作为一个会话结束合同整体 A/B，不分别声称收益；不加入字节预算、缓存容量调整、强制 allocator purge、renderer 或整页设置销毁。候选若失败，整个候选回滚，不进入下一批基线。PR 自有 Back→Forward 的恢复语义另列既有缺陷，不在本候选改变。

**正式采样前冻结的限定负载与门禁：**

- 保持同一 PR/index；采用缩小的生命周期子场景，不冒充 pilot1 全六文件/range路线：正常项目选择 → 打开 PR #1（AGENTS.md，168 行）→ 选择 `src/project/mod.rs` → 显式加载 1035 行 diff → 全部展开 1566 行 → 收起 1035 行 → Home → 历史 Back 恢复 → Home 后 32 对 Project/Home，使第 65 次域外导航触发一次真实 scope-unreachable。
- 正常重新选择项目本来就清理 PR 状态，故两方下一轮都是“进程暖、会话冷”，不是缓存暖命中；不可改成直接调用保留 request 的 open 来制造不对称路径。回放只用于读取成本稳定化，不推论在线网络无退化。
- 双方使用相同 Release/default+test-fixtures、同一个默认关闭且要求隔离/offline 的内部 driver，走正常 callback，不直接 set Route 或灌缓存。相同 bounded receipt/AfterRendering 仪表不保留大 model/owner；缺配对/超时/错误目标无效，不记 0。正式顺序 A1→B1→B2→A2→A3→B3；每进程 3 轮预热、20 轮正式。
- PR open、Load、expand、collapse、historyBack、第 65 次实际清理导航各自统计 callback→正确内容/状态→AfterRendering p95；不把廉价导航混入稀释尾部。逐进程 nearest-rank p95 为 20 点中的第 19 点，比较三进程 p95 中位数，任何主动作 >+5% 则拒绝。20 点尾部估计较粗；保留全点，不能删异常点或反复跑到通过。
- 每轮不可达 Home 后静置 2.5 秒，采 5 次 RSS/footprint，采完 ACK 再推进；主 RSS 为第 20 正式轮的五点中位数。三对方向一致，三进程中位改善 ≥10 MiB 或 ≥5%，且不得新增预热后持续增长；同时报告每轮范围/斜率、首次/重复动作和读取数。尺寸800×600@原生2、压缩为零、PID/route/诊断新鲜度、无并行编译、回放完整、正常关闭均为有效性条件。

**仪表预检记录（不计正式 A/B）：** v1 基线完成一个预热周期，真实 callback/回放/采样通过，但发现既有 120 ms UI 诊断合并可省略最终 Home 的新 model 行，不能用清理前的旧日志证明清理后状态。因此正式采样前升级为 `real-pr-v2`：在同一 event-loop 采样边界额外记录 files/lines/tree/list 四个 O(1) model 行数，holding 期间不推进动作。不得混合 v1 与 v2；v1 源码、基线和 `preflightA1` 全部保留，v1 候选构建主动中止（exit 130）并留存日志。v2 候选 `preflightB2` 的一个预热周期完成，事件精确落在第 65 次导航，PR model 三项归零而 list=1，cache/resource 归零；18 次回放全命中、正常关闭。两次预检都不是 3 对对照，不能从其内存数字推导优化收益。

新增测试实测：session-resource 7 项、controller lifecycle 3 项（各覆盖 en/zh-CN）、v2 driver 7 项均通过。Controller 测试首次因组件创建前切换 Slint locale 而失败 3 项，修正测试初始化顺序后 3/3 通过；仪表接入首次因 RenderingState 所有权移动编译失败，改为借用后通过。Replay 的 8 组正反例在允许 loopback 的环境通过（沙箱首次 EPERM 不能算通过）；probe 6 组、runner 5 组、strict comparator 9 组自测通过。工具自测只证明 gate 行为，不声明运行时收益。

**冻结构建与来源核对：** 基线与候选均由同一 HEAD `97438f3f034c39f3b94fa9f0cdaa32aa8a0ed551`、同一 `cargo build --release --locked --features test-fixtures --bin usher` 和未修改的 Release profile 构建；两份日志都在 3m12s 正常结束。基线二进制 SHA256 为 `0e6bb9392264e6b589ed06e1e3609d8277acc19eb8a1bbc3f8fc20636c22cd58`，候选为 `a726de7000e945484b3254da3a56af6ae96e05b7c8a3d84e419b80b02d2b2776`。双方共同 driver 源 SHA256 均为 `04eb8bafc0545607a8c9ca7c4c2a7696ff698fe12a0e78ada76f081c344168a2`。冻结源码逐文件审计确认：基线去掉 17 行共同测量接点后等于 HEAD；生产差异仅为前述会话结束合同。完整审计见忽略证据目录的 `artifacts/session-v2/source-audit.md`。

**正式证据完整性：** 六个独立进程按 A1→B1→B2→A2→A3→B3 完成；每进程 3 个预热轮、20 个正式轮、23 个真实 lifecycle boundary、1,725 个配对动作、414 个快照请求，均 `fatal=0 / misses=0 / mutations=0`、应用 code=0、回放后停、快照 hash 未变。比较器校验 5,301 个输入文件并判定 `structurally_valid=true`，没有丢样、删异常点或重跑到通过。基线每个最终边界仍为 files=213 / lines=1035 / tree=255 / list=1；候选每轮均为 0 / 0 / 0 / 1，且 detail/override/highlight/source/current-resource 诊断归零。这证明清理合同确实执行，但不是 RSS 收益证明。

| 配对 | 基线末轮 RSS / MiB | 候选末轮 RSS / MiB | 候选变化 |
| --- | ---: | ---: | ---: |
| A1 → B1 | 191.516 | 198.813 | +7.297 MiB / +3.810% |
| A2 → B2 | 186.422 | 199.453 | +13.031 MiB / +6.990% |
| A3 → B3 | 183.625 | 199.563 | +15.938 MiB / +8.679% |
| 三进程中位数 | **186.422** | **199.453** | **+13.031 MiB / +6.990%** |

三对 RSS 方向全部更差，既没有 ≥10 MiB 或 ≥5% 的改善，也直接违反“三对方向一致地改善”。外部 physical footprint 中位数同样从 120.189 增至 127.470 MiB（+7.281 MiB / +6.058%），只能作辅助回退证据，未替代 RSS 门禁。正式轮 RSS OLS 斜率（MiB/轮）为 A1 `+0.130663`、B1 `+0.139603`、A2 `-0.199930`、B2 `+0.214556`、A3 `-0.096652`、B3 `-0.027690`；未预声明斜率数值阈值，因此不从这些波动另造通过/失败线，也不拿单个负斜率挽救已失败的主指标。

| 主动作 | 基线三进程 p95 中位数 / ms | 候选 / ms | 变化 | 门禁 |
| --- | ---: | ---: | ---: | --- |
| 打开 PR | 244.824 | 238.684 | -2.508% | 通过 |
| Load Diff | 245.515 | 247.260 | +0.711% | 通过 |
| 全部展开 | 364.626 | 361.746 | -0.790% | 通过 |
| 收起 | 201.614 | 199.656 | -0.971% | 通过 |
| 历史返回 | 20.486 | 14.221 | -30.582% | 通过 |
| 最终不可达清理 | 16.673 | 18.897 | **+13.339%** | **失败（上限 +5%）** |

最终清理计时包含候选新增的同步 owner 遍历、两次完整 macOS 内存 checkpoint、状态析构、UI 写与到下一帧的等待；两方都开启 diagnostics，但只有候选新增这两次调用，因此这部分开销并不对称。默认生产关闭诊断时会提前返回，不过现有数据不能估算“扣掉日志后”的 p95，也不能推定关诊断就会通过。若以后单独优化清理耗时，应另立冻结的诊断消融/分段计时实验，不能事后修改本次结果。

**采用决策：Reject。** 候选成功断开 Rust/Slint PR owner，但 allocator、渲染器或系统的驻留高水位没有随之形成可采纳的 RSS 回落；数据也不能证明清理本身导致全部上升。由于 RSS 和 final-eviction 两个门禁同时失败，整包候选已回滚：默认历史不可达清理、session highlight slot、迟到 source loading guard、3 个新增模块和全部共同 benchmark 接点均未保留。回滚后 `src/lib.rs`、`src/workflow/mod.rs`、`src/workflow/pr_review.rs`、`controller.rs` 四个 tracked 文件逐字节等于 HEAD，工作树仅保留本诊断文档与 `bug-history.md`；未修改 Slint、Material、Cargo、架构或稳定 spec。资源槽与 source guard 的单测成功只能证明候选内部正确性，不能绕过运行时门禁而拆留部分改动。

正式比较原始结果保留在忽略目录 `memory-benchmark-results/pr-cache-20260920/comparison-session-v2.{json,md}`，含 115 个内存样本/进程、每动作原始点、所有 owner、请求、诊断与输入 hash。PR 自有 Back→Forward 空页仍是 UI-142；本候选没有修复它。因候选已拒绝，未继续执行其定向 hunk、复制/选区、草稿、Find 与双语言完整原生矩阵，也未将其推进为下一批基线。

回滚后的最终检查：`cargo fmt --all`、`cargo check --locked`、`git diff --check` 均通过；`cargo test --lib --locked` 为 **650 passed / 0 failed / 2 ignored**，`cargo test --test pr_review_core_journeys --locked` 为 **4 passed / 0 failed**。仅保留既有的 `events_after` dead-code、测试 `ComponentHandle` unused-import 与 macOS compact-unwind 警告，没有新增失败。未重跑完整 doctest 和既知会报 5 项架构规模超限的 quality budget；这些基线问题与本候选无关，未宣称修复。

### 13.6 2026-09 批次 3：Chat / Job 历史与日志 owner 预算（已完成：Chat 拒绝，Job 采用）

Chat 与 Job 是两个独立子批，共享的只有 §13.2 的验收门禁。不删除持久消息或日志，不将已虚拟化的可见 UI 行当作无界历史，也不将一个子批的收益累加到另一个子批。

#### 13.6.1 真实 Chat 数据与归因边界

只读盘点当前本地两个历史文件，只记录条数和字节，不输出消息内容：

| 文件 | 记录数 | 文件字节 | 正文字节 | 单条最大正文 | 启动最近 20 条正文 |
| --- | ---: | ---: | ---: | ---: | ---: |
| `ai_chat_history.jsonl` | 24 | 6,895 | 4,159 | 635 | 3,891 |
| `chat_history.jsonl` | 99 | 24,392 | 17,539 | 6,506 | 1,478 |

两份真实历史文件合计 **31,287 bytes**，启动尾页正文合计 **5,369 bytes**。这些是序列化文件/正文统计，不等于 allocator 实际分配；但数量级足以排除“当前这两份 Chat 历史正文直接造成 400 MB”的假设。原用户路线没有进入 Job 日志，后续 Job 压力 fixture 的结果也不能追溯归因为原始 400 MB 现场。

#### 13.6.2 Chat owner 审计

当前 Chat 已有最多 32 行的路由 Slint 窗口、96 项 / 8 MiB 渲染缓存，并在离页时清理 model、虚拟高度前缀、行几何与锚点；Rust session 则保留已分页历史。额外发现的是线程本地 `TEXT_GEOMETRY`：最多保留 8 个 layout 及 Parley font/source context，Chat → Home 时没有清理。它是一个有界 owner，不是已证明的无界泄漏。

审计同时记录了四个不在本候选中的后续风险：助手可见行仍持完整原文而显示投影最多 1200 字符；`load_before` 每页扫描整个 JSONL；AI 发送会在 prompt 只取最近 20 条前克隆已加载历史；迟到分页响应缺少 mode/route generation。真实数据未证明它们达到本批内存门槛，所以不捆绑改动。

#### 13.6.3 Chat text geometry 离页释放候选（已实验，拒绝并回滚）

候选只在 Chat 离页时释放线程本地 text geometry owner，不改 session、持久历史、虚拟窗口或 Material。功能单测确认 layout/font context 被释放；性能使用现有 `chat-performance-v2` 的 180 条消息 / 232,641 decoded bytes 压力 fixture。这是生命周期压力证据，**不是上表真实历史的复现**。每个进程固定 800×600 logical @2、FemtoVG、2 worker，执行 10 次 Chat ↔ Home；六进程顺序为 A1 → B1 → B2 → A2 → A3 → B3。

| 指标 | A（当前生产） | B（离页清理） | 变化 |
| --- | ---: | ---: | ---: |
| 第 10 轮回 Home RSS 中位数 | 128.844 MiB | 126.594 MiB | -2.250 MiB / -1.746% |
| 第 10 轮回 Home footprint 中位数 | 93.204 MiB | 90.189 MiB | -3.016 MiB |
| Chat 路由 p95 | 570.998 ms | 568.603 ms | -0.420% |
| Home 路由 p95 | 567.443 ms | 579.643 ms | +2.150% |

三个按时间配对的 B−A RSS 为 **-0.313 / -2.250 / -1.438 MiB**，方向都改善；两个路由 p95 也都符合 +5% 门禁。但主指标只改善 2.250 MiB / 1.746%，未达“≥10 MiB 或 ≥5%”采用门槛，因此结论为 **Reject**。候选已完整回滚；`src/chat/text_selection.rs` 和 `src/chat/interactive.rs` 均与 HEAD 逐字节一致，没有把未达标优化留在生产。

原始证据保留在本地忽略目录 `memory-benchmark-results/history-budget-20260921/comparison-chat-geometry-v1/`，其 `metadata.json` 固定了协议与双方 SHA256，`summary.json` 可重新核对上表数值。

#### 13.6.4 Job viewport 字节预算（独立子批，已采用）

既有 #70 固定 fixture 在 16 个不同大日志同时保留时记录了约 **36,130,816 bytes** 源正文与 **3,276,800 bytes** wrapped range，因此该独立 owner 已经显著，可以测试“原 16 项预算 + retained bytes 预算”候选。候选不裁剪日志，不改精确 extent/可见投影；当前 callback 与所有仍由 Slint `JobLogWindow.rows` 持有的 mounted viewport 必须保留，只有未挂载 viewport 可以被淘汰。

正式协议在运行前固定为：当前接受的 #70 生产二进制为 A，只增加 Job cache 字节预算为 B；默认 Release、FemtoVG、1100×700、2 worker、离线隔离状态，每进程 3 轮 × 16 个 512 行混合 ASCII/CJK/annotation 日志，90 秒/进程，顺序 AB/BA/AB。Runner 使用 `--mode regression` 验证 48 个 layout、共享 snapshot、正常退出和无 framebuffer 错误；历史 `migration` 模式要求旧 A 仍有 measurement 副本，与当前生产不符，不得使用。

采用判定不使用 runner 较宽的“不回退”阈值代替 §13.2：稳定 RSS 中位数必须改善 ≥10 MiB 或 ≥5%，三个同编号 B−A 都须改善，heartbeat p95 中位数不得退化超过 5%，候选不得新增 warm-up 后持续增长。还必须同时通过字节边界、活动超大项、多个 mounted viewport、lease 释放回收、Timer 生命周期和精确 extent 回归，并保留既有 `Viewport::drop` 非阻塞取消契约；失败即只回滚 Job 小批。无论结果如何，都不宣称它解释或修复了原用户的 400 MB 路线。深度 PR 列表翻页和大量不同 Logo 未在本子批执行，仍是条件触发项。

第一版候选只保护一次 layout callback 的“当前项”。它的三对 Release 数字虽为 189.734→150.719 MiB（-39.016 MiB / -20.563%），独立代码复核却发现真实 `GitHubActionsJob` 允许多个 Step 同时展开：两个合计超预算的 mounted 日志可能互相驱逐、取消和重新测量。因此 `comparison-job-cache-v1/` 只保留为“性能方向成立但正确性门禁失败”的诊断证据，**不得作为 Adopt 依据**。加固过程中还发现 repeated Timer 强捕获自身会把 stopped callback/cache 留到 Slint context 销毁；正式 v2 构建前已改为 `Weak<Timer>` 并增加销毁回归。

**已采用 v2 候选：** `src/jobs/log_viewport.rs` 把 16 项和 16 MiB 作为已记账 cache owner 的**软淘汰目标**，不是全局 heap/RSS 硬上限。当前 callback 与所有 mounted rows lease 都会被 pin，组合工作集可在整个挂载期持续超标；仅在超标时启动 250 ms event-loop Timer，lease 释放后淘汰最旧未挂载 viewport，回到目标即停止。淘汰触发 `Viewport::drop` 的非阻塞 Worker 取消；正在运行的任务在下一次取消检查或本次计算返回时释放暂存。

`LogSnapshot` 在生成/更新时缓存正文 retained bytes；每次 prune 只用 O(viewports) 读取当前源计数，能识别已折叠 live log 的增长，不在 event loop 逐行扫描。记账范围包括 `LogModel` 源文本/行槽、generic fallback 的当前 pending snapshot、未共享 measurement 文本、wrapped 内外容器及 measurement/virtual-list extent；源与 measurement 共享快照时只记一次。wrapped/extent/measurement 的线性统计仍在后台测量 Worker 完成。无法观测的 generic 原始 `ModelRc` backing、已取消但未退出的旧 in-flight request、其他 Worker 瞬时分配、allocator/font/GPU 资源和完整进程 RSS 不在该 settled-cache 估算内。日志原文不裁剪，精确 extent、行号和可见投影不变。

| 三进程中位指标 | A（原 16 项缓存） | B（v2 软淘汰目标） | 变化 |
| --- | ---: | ---: | ---: |
| Peak RSS | 189.750 MiB | 161.000 MiB | -28.750 MiB / -15.152% |
| Settled RSS | 188.625 MiB | 151.031 MiB | **-37.594 MiB / -19.930%** |
| Final physical footprint | 177.550 MiB | 136.890 MiB | -40.660 MiB / -22.901% |
| Heartbeat p95 | 51.177 ms | 51.173 ms | -0.004 ms / -0.008% |
| 单轮中位耗时 | 10,983 ms | 10,912 ms | -71 ms / -0.646% |

三个同编号 B−A settled RSS 分别为 **-37.594 / -36.391 / -39.234 MiB**，全部改善且每对都超过 19%。三个 B 进程的 `growth_mib` 为 -9.859 / -9.891 / -9.891 MiB，`rss_slope_mib_min` 为 -12.760 / -12.422 / -12.430 MiB/min，没有新增 warm-up 后持续增长。每对 heartbeat p95 变化为 -0.006% / -0.008% / -1.832%，均未回退。Runner 的 regression summary 为 `valid=true, passed=true`，A/B 共 288 个 layout 全部完成、snapshot 共享、正常退出，framebuffer error 为 0。

基线每轮末的可见诊断字段为 36,130,816 source bytes + 3,276,800 wrapped bytes；v2 在这个逐个替换、无并发展开 lease 的 fixture 中稳定为约 13.55 MB + 1.23 MB，即保留 6 个 viewport。加上诊断日志没有列出的行槽、wrapped 外层和双份 extent 容器，已声明 owner 子集约 14.21 MiB，低于 16 MiB 软目标。这不是完整 owned heap 或 allocator RSS；进程结果另行以上表实测。多个同时 mounted 的真实工作集允许按契约超过该值；owner-level 双 lease 测试和单个 production `JobLogWindow` 测试覆盖 pin/drop 语义，但不用此单展开 fixture 伪称完成真实多 Step 端到端验收。

正式 Adopt 证据保留在本地忽略目录 `memory-benchmark-results/history-budget-20260921/comparison-job-cache-v2/`。A 二进制 SHA256 为 `f7236a2bb816a5afabd1d4f8c7bee099456ae13d02c70bf5df13b140d4520e87`，B 为 `23cda3dd7f1b727fdccbb8d1d9dc9369591100ba765c62cdec3e6e202d022ad9`；`metadata.json`、`summary.json`、6 份 `result.json`、应用日志和 RSS 样本可重新核对协议与结果。被作废的 v1 证据及 candidate hash `c602ed10cf8ad8d19b77a64eeb328f84cb943269e628fd1754638cb255980cd1` 仍保留，不覆盖历史。

**决策：Adopt v2。** 严格 RSS、三对同向、p95、增长门禁和 owner-level 生命周期单测全部通过，因此 Job 软字节预算保留在生产，并同步中英文架构与精确虚拟滚动 spec。这一收益只适用于“切换多个大 Job 日志”的固定负载；用户原路线未打开 Job 日志，不用该结果解释或宣称修复原始 400 MB。深度 PR 列表翻页和大量不同 Logo 未在本子批执行，仍是条件触发项。

最终验证：`cargo build --release --locked`、`cargo fmt --all`、`cargo check --locked` 和 `git diff --check` 通过；Jobs 定向测试为 **36 passed / 0 failed**，覆盖 Unicode/空行/超长行、resize 重排、精确 extent、滚动、稳定滑块、软预算边界、多个 mounted lease、折叠 live growth、generic pending snapshot 与 Timer 销毁；完整 `cargo test --lib --locked` 为 **661 passed / 0 failed / 2 ignored**。保留既有 `events_after` dead-code、测试 `ComponentHandle` unused-import 与 macOS compact-unwind 警告，没有新失败。本小批没有修改 UI 文案或 Slint 布局；未执行 compiled `GitHubActionsJob` 多 Step 同时展开→折叠/离页→Timer 自动停止的端到端矩阵，也未重跑完整 doctest（既知 logger 基线失败）、架构 quality budget、Windows 或长时 soak，不宣称这些项已通过。

### 13.7 2026-09 批次 4：开发构建与渲染剩余项（已完成）

**目标与范围：** 产品内存继续用正式 Release 验收；日常优化构建优先测现有 `release-fast`。仅当 Debug 常驻成本影响开发时，独立比较 generated UI/依赖的 `opt-level=1` 候选，不默认修改全局 dev profile。字体图集、driver/renderer 残留另开归因实验，不把构建 profile 与 renderer 更换合在同一 A/B。

**前置与验收：** profile 比较固定源码/features，记录实际 resident 文件段、启动/RSS、首次及增量编译时间、产物体积、backtrace/调试能力；它是开发体验实验，不替代三个 Release 进程的产品性能门禁。renderer 候选仅在新版本或新的 ownership 证据成立后重开，使用相同小/大窗口、路由/resize 循环，覆盖字体、阴影、拖拽、全屏、主题、标题栏、打包和关闭。

**拒绝与回滚：** 不因一次 footprint 较低就推广 WGPU、opaque window 或整页销毁；这些已有失败/混合结果。无法解释残留归属时保留默认 FemtoVG/OpenGL，不做强制 allocator trim/purge。开发编译显著变慢、调试能力下降或产品 RSS/视觉回退时拒绝对应候选。

#### 13.7.1 `release-fast` 源码构建与路线 A/B（已完成，拒绝作为低内存运行建议）

候选只把现有 Release profile 的 full LTO / 1 CGU / non-incremental 改为 Thin LTO / 8 CGU / incremental；生产源码、features、renderer、fixture、窗口、worker 和路线均不变。候选二进制由当前 `b123e2e` 源码执行 `cargo build --profile release-fast --locked` 构建，SHA256 为 `c5c82c302c42dc447802e9a079e7f3587178900e02e65e30d027427a9f58bc48`。正式 Release 对照仍为同一源码的 `23cda3dd7f1b727fdccbb8d1d9dc9369591100ba765c62cdec3e6e202d022ad9`。

F2/F3/F4 三个新进程各完成与 R1/R7/R8 相同的 12 阶段、60 样本协议，全部 800×600@原生 2×、`compressed=0`、正常退出，二进制/快照未变化。共享的无状态只读回放共收到 96 个请求、8,489,568 bytes，恰为 3 组各 32 请求；`fatal/misses/mutations/transport_probes=0`。F1 因锁屏后等待约 34 分钟才开始采样，仅保留 5 个 Home 点并正常退出，不纳入正式组。

| 指标 | Release R1/R7/R8 中位数 | `release-fast` F2/F3/F4 中位数 | 候选变化 |
| --- | ---: | ---: | ---: |
| Home 起点 RSS | 126.000 MiB | 132.609 MiB | +6.609 MiB / +5.25% |
| Projects 首访 RSS | 132.906 MiB | 140.875 MiB | +7.969 MiB / +6.00% |
| Hooks 首访 RSS | 183.703 MiB | 191.953 MiB | +8.250 MiB / +4.49% |
| PR 首访 RSS | 210.094 MiB | 223.656 MiB | +13.563 MiB / +6.46% |
| 第一轮最终 Home RSS | 205.922 MiB | 214.672 MiB | +8.750 MiB / +4.25% |
| PR 热访 RSS | 227.859 MiB | 234.016 MiB | +6.156 MiB / +2.70% |
| Chat 热访 RSS | 217.516 MiB | 234.422 MiB | +16.906 MiB / +7.77% |
| 第二轮最终 Home RSS | 206.969 MiB | 234.406 MiB | **+27.438 MiB / +13.26%** |
| 第二轮最终 Home footprint | 165.126 MiB | 188.876 MiB | **+23.750 MiB / +14.38%** |
| 全阶段离散样本最大值的进程中位数 | 227.859 MiB | 234.422 MiB | +6.563 MiB / +2.88% |
| lifetime RSS HWM | 238.73 MiB | 243.11 MiB | +4.38 MiB / +1.83% |
| lifetime footprint HWM | 222.11 MiB | 225.66 MiB | +3.55 MiB / +1.60% |

候选的所有原始 RSS 及 lifetime HWM 仍低于 300 MiB，最差 F4 HWM 为 `246.28 MiB`。但是 Home 第一轮→第二轮的 RSS 增量为 F2 `+20.547`、F3 `+18.422`、F4 `+21.703 MiB`，三次同向；Release 对照为 `+1.047`、`+11.703`、`-0.813 MiB`。候选末轮 Home 相对 Release 的 RSS/footprint 均超过 5% 回退门槛，因此不能仅凭绝对值低于 300 MiB 通过。

末轮 Home 的 graphics dirty 与 IOSurface 基本不变：Release 中位 `78.125 / 36.641 MiB`，候选约 `78.125 / 36.641 MiB`。差异主要位于 allocator：Release 的 `Malloc Small + Large` dirty 中位约 `46.047 MiB`，候选约 `59.000 MiB`，且候选三次均保留 `15.75–19.11 MiB` 的 Large dirty；候选 `__TEXT` clean 中位也从 `13.438` 增至 `16.719 MiB`。候选 Mach-O 的 `__TEXT` segment 为 38,797,312 bytes、`__LINKEDIT` 为 10,895,360 bytes；Release 分别为 35,045,376 和 507,904 bytes。磁盘二进制为 52,191,456 对 37,678,464 bytes（+38.5%）。这些数值支持“更少的链接期优化增加代码/allocator 高水位”的方向，但没有分配调用栈，不能进一步归因到具体业务 owner。

构建侧只记录本次实际结果，不把它包装成新的受控编译 A/B：首次 profile 构建 Cargo 报告 `3m37s`；随后一次 generated UI/root rebuild 报告 `1m20s`；最终无改动构建为 `0.96s`。`target/release-fast` 当时占 5.3 GiB，包含增量缓存，不能与单个二进制大小混算。已有 [构建性能文档](docs/build-performance.md) 仍负责历史受控编译对照。

**决策：拒绝把 `cargo run --profile release-fast --locked` 增补为本轮的低内存运行建议。** 现有 `release-fast` profile 与 README 中的“日常优化构建”用途保持不变，也不影响正式发布继续使用 `--release`；本轮不新增运行命令、不修改 profile。下一步仅针对用户实际使用的裸 `cargo run`，独立测试依赖/generated UI 的 Debug `opt-level=1` 候选，根应用 crate 继续保留 opt0 与 line tables；失败则完整回滚。

#### 13.7.2 普通 `cargo run` 的依赖/generated UI opt1 A/B（已完成，采用）

候选只修改 `profile.dev` 的 package override：非 workspace 第三方依赖使用
`opt-level=1`；生成的 `usher-ui`、`usher-ui-settings`、`usher-messages` workspace
crate 使用 `opt-level=1` 且继续 `debug="none"`。根 `usher` 及其余应用业务 workspace
crate 仍为 opt0、`line-tables-only`、incremental。没有改变默认 feature、renderer、
窗口、fixture、运行时代码或 Material vendor；两个 A/B artifact 都早于并发出现的
`pr_review_workspace.slint` 文件树 `y: -1px` 调整，因此该无关改动没有混入任一侧。

固定对照为 D2/D3/D4，SHA256
`66acd9852e772345544e54516247f7cd0a86beac58d144ff65e863586d9ad63c`；候选为
C1/C2/C3，SHA256
`fa68f0b16fa77fd7ccc0ba9eec1f70b017c993763969579563e79c38d19c33e6`。每个进程都
按 13.3 的 12 阶段路线采 60 个 settled 点，使用相同 800x600@原生 2x、
FemtoVG/OpenGL、2 个 Tokio worker、10 Projects、5 Hooks、99 RAG + 24 AI Chat
records、6 Logos 和冻结 `tianlai10000000/usher#1`（213 files / 26,963 lines）。六个
进程都正常退出、`compressed=0`、二进制/快照 hash 未变化，回放无 miss、mutation
或 fatal。

下表为三进程阶段中位数的中位数；方括号为三个进程中位数范围，单位 MiB：

| 阶段 | 原 Debug RSS | 候选 RSS | 候选 footprint |
| --- | ---: | ---: | ---: |
| `home_start` | 198.313 [192.219-199.844] | 159.422 [159.000-161.641] | 99.611 |
| `projects_cold` | 207.234 [203.438-209.391] | 169.000 [166.891-169.031] | 108.532 |
| `hooks_cold` | 266.594 [263.141-268.828] | 223.953 [221.531-224.156] | 167.064 |
| `workflow_closed` | 289.313 [287.938-289.734] | 246.281 [244.734-248.531] | 185.829 |
| `pr_initial` | 302.969 [296.422-303.953] | 270.781 [259.406-272.172] | 203.611 |
| `workflow_after_pr` | 304.922 [298.953-305.969] | 254.484 [254.078-261.781] | 187.236 |
| `chat_cold` | 301.219 [300.906-302.313] | 253.109 [250.938-255.641] | 177.533 |
| `home_after_cycle1` | 301.141 [300.813-302.156] | 253.000 [250.969-255.563] | 177.283 |
| `hooks_warm2` | 301.078 [300.859-302.125] | 253.078 [250.969-255.500] | 177.314 |
| `pr_warm2` | 321.234 [319.344-321.266] | 273.781 [269.953-276.594] | 194.314 |
| `chat_warm2` | 319.500 [300.531-321.766] | 274.219 [270.266-277.156] | 194.533 |
| `home_after_cycle2` | 303.219 [300.438-308.922] | 259.250 [256.906-270.188] | 182.845 |

原 Debug lifetime RSS HWM 为 D2 `330.41`、D3 `330.30`、D4 `328.75 MiB`，三次
全部越过 300 MiB；候选为 C1 `289.39`、C2 `279.78`、C3 `285.12 MiB`，三次全部
严格低于 300 MiB，最差仍有 `10.61 MiB` 余量。候选 lifetime footprint HWM 为
`230.85/227.00/224.17 MiB`。候选相对原 Debug 的中位数改善为：Home 起点
`-19.61%`、首次 PR `-10.62%`、第二轮 PR `-14.77%`、最终 Home `-14.50%`、
lifetime RSS HWM `-13.68%`。候选第二轮 Home 相对第一轮分别为
`+1.344/+17.188/+8.281 MiB`，没有出现每轮重复完整冷启动台阶的证据；仅两轮协议
仍不能证明无限长 soak 的斜率为零。

候选磁盘二进制从 `390,915,112` 降到 `180,791,480 bytes`（-53.75%）。首次候选
源码构建 Cargo 报告 `6m33s`；没有同条件独立 target 的原 Debug 构建时间，因此不
宣称编译提速。验证后的共享 `target/debug` 为 60.80 GiB，其中 incremental 39.01
GiB、deps 16.77 GiB；它混有并发检查和历史 identity，只记录清理压力，不能归为本
候选增量。

调试门禁通过：LLDB 对候选 `src/main.rs:12` 解析出 2 个 breakpoint location；临时
dev-profile panic probe 的 backtrace 输出
`examples/dev_profile_panic_probe.rs:2:5`，验证后已移除。代价是第三方依赖和生成 UI
的调试体验不同：生成 UI 不保留源码级 debug metadata；第三方依赖仍继承 line
tables，但 opt-level 1 可能影响逐步执行与局部变量查看。需要完整应用变量/类型调试时
继续使用隔离的 `dev-full`。

**决策：采用该 dev package override。** 它直接修复用户实际的普通 `cargo run`
路线越过 300 MiB 问题，同时保留应用源码 opt0、行号断点和 panic 定位。产品 Release
门禁和 `release-fast` 的既有用途不变；本批不重开已拒绝的 WGPU、opaque window、
整页销毁、Tooltip 全页 opacity 或 allocator purge 候选，也不修改 Material vendor。
