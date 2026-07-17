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
