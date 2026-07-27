# Usher RSS / Physical Footprint 诊断记录

> 记录日期：2026-07-17
>
> 当前 Slint：1.17.1
>
> 测试平台：macOS，Apple Retina/高分辨率显示器
>
> 文档性质：运行时诊断、上游调研和优化决策依据；本文不表示相关优化已经实现。

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

- **Chat 路由卸载：** 当前已有 overscan、增量高度前缀缓存和最多 56 条消息的虚拟窗口；第 4 批清空 model 无可测收益。只有使用数千条历史、反复向上分页后观察到 warm-up 后线性增长，才重新设计 viewport 生命周期和滚动锚点。
- **系统默认 CJK fallback：** 第 3 批已证明当前组合会从 22.4 MiB Hiragino 映射切到约 74.6 MiB PingFang，不能重复尝试同一方案。
- **裁剪 Syntect 语言：** PR 语言不可预知，明确不做；第 14 批只能改变结果表示，不能减少语言覆盖。
- **强制 allocator trim/purge：** 不解决 owner 和峰值，只会制造暂时好看的 RSS；不进入生产方案。
- **立即启用 WGPU/Metal：** 第 5 批稳定 RSS 已回退 8.6%–13.5%，等待依赖或驱动变化后再按第 17 批复测。

推荐顺序为 **13A → 13B → 14 → 15（有调用栈证据才做）→ 16 → 17**。第 13、14 批继续收敛随大型 PR 线性增长的应用侧成本；第 16、17 批处理当前更大的高 DPI 图形成本，但风险更高，必须在业务内存结构稳定后单独测量。
