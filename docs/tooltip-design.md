# Tooltip 设计方案纪要 / Tooltip Design Notes

> 文档状态：设计与迁移纪要，不是稳定行为规格。当前产品契约以
> [`docs/specs/ui-design.md`](specs/ui-design.md) 为准；架构归属见
> [`architecture.md`](architecture.md) / [`architecture.zh.md`](architecture.zh.md)。
> 下文保留各方案的取舍与实测结论，便于后续回退或再评估。
>
> **产品决定（2026-08-07）：采用方案 E「锚点式自适应 portal」。** Slint 1.17
> 内置 `Tooltip` 继续不采用；旧 portal 与 Inbox 局部 Rectangle 仅作为失败历史。
> 当前实现复用 Floating UI 的 offset/flip(bestFit)/shift/size 思路，但保留
> Slint ComponentFactory 的独立 global 边界。

日期：2026-08-04，更新于 2026-08-07 · Slint 1.17.1

## 0. 采用状态 / Adoption status

| 方案 | 状态 |
| --- | --- |
| E. 锚点式自适应 portal | **当前采用**；每个 Slint global 图一个根层 host |
| D. Slint 1.17 内置 `Tooltip` + `GitHubTooltipContent` | **暂不采用**（延迟与挡按钮不可接受） |
| B. 旧版坐标式全局 portal | 已被 E 取代；曾解决 clip，但有串文案/坐标回归 |
| A. 局部 Rectangle tip | 仅作历史对照；易被 clip / 叠层截断 |
| C. Material `ToolTip` | 未采用 |

当前方案统一视觉、宿主锚点、自动碰撞处理和可控生命周期；调用点不再计算
Tooltip 表面坐标，也不通过提高局部 `z` 或关闭父级 clip 解决浮层问题。

## 1. 目标 / Goals

Usher 需要统一的 GitHub/Primer 风格 Tooltip，覆盖：

- IconButton、Copy、状态栏、导航、Workflow/PR Review、Inbox 行动作等调用点
- 明暗主题下的视觉一致（`GitHubTheme.tooltip-bg` / `tooltip-fg`）
- 列表、ScrollView、卡片等祖先 `clip: true` 环境
- 独立编译页面（`usher-messages` / `usher-ui-settings`）与主窗口

目标视觉参考 Primer TooltipV2：12px 字、约 18px 行高、4/8px 内边距、6px 圆角、相对宿主约 4px 间距。

## 2. 方案对比 / Approaches compared

### A. 应用自研局部 tip（Rectangle + Text）

**做法：** 在按钮/行内直接画 `Rectangle` + `Text`，用相对 `x`/`y` 定位；Inbox 一度用字面量文案避免列表属性串台。

| 效果 | 弊端 |
| --- | --- |
| 定位可控：可固定在按钮上/下、尾对齐 | 被行、列表、ScrollView 的 `clip` 截断 |
| 无延迟，hover 即显 | 下一行背景会盖住伸出行底的 tip（叠层问题，不只是 clip） |
| 文案可写死在叶子组件，不易串台 | 为“逃逸 clip”反复改列表 `clip`/高度，易破坏外框 |
| 实现简单 | 无法统一窗口级生命周期；各 feature 各写一套 |

**结论：** 适合快速原型，不适合作为全仓共享契约。Inbox 本地 tip 的裁切与边框回归，是该路径的典型失败模式。

### B. 旧版坐标式全局 portal（已被 E 取代）

**做法（迁移前）：**

1. `GitHubTooltip`：透明定位器，发布文案与窗口坐标
2. `TooltipOverlayState`：global（`show` / `renew` / `hide`、token、lease 心跳）
3. `GitHubTooltipOverlayHost`：挂在 `MainWindow` 根层，绘制 `GitHubTooltipSurface`

| 效果 | 弊端 |
| --- | --- |
| 逃逸祖先 `clip`（局部 `z` 做不到） | 独立编译页面不共享主窗口 global，需每页自挂 host 或绕过 |
| 可固定相对宿主：上/下、左/中/右对齐、`overlay-offset` | 坐标换算易偏，曾出现 tip 盖住按钮、间距/对齐错误 |
| 可支持键盘焦点与 `force-tooltip`（如 Copied!） | lease/token 复杂；条件 `if hover:` 销毁无 destructor 时需心跳清残影 |
| 统一 Primer 表面与 token | 多按钮争抢单一 portal 时可能短暂串文案 |
| | 维护成本高，与 Slint 后续内置能力重叠 |

**结论：** 解决了裁切，但引入了 portal 竞态、独立页面边界和定位回归；已被迁移移除。

### C. Slint Material `ToolTip`（未采用）

Vendored `material-1.7.1-fixed` 的 `ToolTip` 是普通局部组件：hover 即显、本地 `tooltip_offset`、无 popup 降级、受祖先 `clip` 限制、Material 色板。仓库策略禁止产品 UI 依赖 Material 私有实现；视觉也不符合 Primer。

### D. Slint 1.17 内置 `Tooltip` + `GitHubTooltipContent`（历史尝试）

**做法：**

```slint
Tooltip {
    GitHubTooltipContent {
        text: "Open terminal";
    }
}
```

- 运行时：内置 `Tooltip` 管理悬停延迟、Popup 生命周期、指针相对定位、视口碰撞、逃逸祖先 `clip`
- 样式：`GitHubTooltipContent` 提供 Primer 色、字号、行盒、内边距、圆角（作为 custom content；Slint 标准外框仍可能存在）
- 调用约定：一宿主至多一个 `Tooltip`；无视觉 tip 的控件（About / dismiss）只保留 `accessible-label`，不挂 `Tooltip`
- 已删除：portal state、overlay host、局部 Rectangle tip、以及 `force-tooltip` / `tooltip-above` / 对齐类 API

| 效果 | 弊端 |
| --- | --- |
| 逃逸列表/ScrollView/卡片 `clip`，不再靠改列表布局 | **延迟硬编码约 500ms，公开 API 不可调** |
| 文案归属宿主，无全局 portal 串台 | **位置跟随指针约 +8px，不可固定上/下/尾对齐；易盖住按钮** |
| 独立编译页面天然可用，无需跨 crate portal | 无键盘焦点触发；无 `force-open`（Copied! 只能靠 hover 时看到文案变化） |
| 生命周期由 runtime 管理，去掉 lease 心跳 | custom content 无法正式关掉标准外框；样式只能近似 Primer |
| 全仓调用点统一，代码量明显下降 | tip 留在主窗口内（Winit 非原生 OS tip）；边缘处可能被窗口夹紧后跳动 |
| | 条件/回收列表里仍需关注 stale popup 风险（runtime 层） |

**结论：** 仓库曾统一到该方案。它解决了裁切与文案门户问题，但**延迟过长**与**挡住宿主**是公开 API 缺口，无法在应用层微调。**产品已决定暂时不采用本方案。**

## 3. 当前代码归属 / Current ownership

| 路径 | 职责 |
| --- | --- |
| `ui/components/github_tooltip.slint` | 透明锚点定位器与统一 Primer surface；长文案按视口宽度换行 |
| `ui/state/tooltip_overlay_state.slint` | 锚点矩形、首选方向/对齐、token/lease 和同步 dismiss claim |
| `ui/components/github_floating_overlay.slint` | 与 surface 内容无关的 offset/flip(bestFit)/shift/size 几何算法 |
| `ui/components/github_tooltip_overlay.slint` | 通用 portal；每个新 claim token 重建文本测量 surface |
| `ui/components/button/github_icon_button.slint` | hover/focus/force-open；点击业务回调前同步 dismiss |
| `ui/main.slint` | 主窗口 global 图的唯一根层 host |
| `ui/state/message_tooltip_overlay_state.slint` | Messages 专用的枚举动作类型、锚点矩形与 lease；禁止承载字符串 |
| `ui/views/messages.slint` | Messages factory-local 专用根层 host |
| `ui/components/messages/github_message_tooltip.slint` | 枚举锚点及三个独立字面量 surface |
| `ui/components/messages/github_message_actions.slint` | 只发布枚举的行动作，禁止动态 delegate label 别名 |
| `ui/views/settings.slint` | Settings factory-local global 图的唯一根层 host |

## 4. 实测问题（迁移后）/ Observed issues after migration

1. **显示延迟过长**  
   内置悬停延迟约 500ms，且无 delay 属性。桌面工具栏/卡片动作感觉“钝”。

2. **Tip 挡住按钮**  
   指针相对放置会使 tip 叠在宿主上（例如 “Open terminal” 盖住终端图标半截），无法改为“固定在宿主上方/下方 + 4px 间距”。

二者均属 Slint 1.17.1 内置行为，**不是应用样式组件能修的**。

## 5. 方案 E：锚点式自适应 portal / Adaptive anchor portal

实现参考 [Floating UI Tutorial](https://floating-ui.com/docs/tutorial)、
[`flip`](https://floating-ui.com/docs/flip)、
[`shift`](https://floating-ui.com/docs/shift)、
[`detectOverflow`](https://floating-ui.com/docs/detectoverflow) 与
[`size`](https://floating-ui.com/docs/size)。仓库不引入 Web/JS 运行时依赖，只移植
适用于 Slint 矩形布局的确定性子集：

1. 定位器发布触发元素的绝对矩形，以及首选 `Top/Bottom/Left/Right` 与
   `Start/Center/End`，禁止发布调用方算好的 Tooltip 左上角。
2. `size` 先把 surface 限制在可视视口 8px 内边距内；长文本按剩余宽度换行。
3. `offset` 在锚点与 surface 之间保留 4px。
4. `flip` 能容纳时保留首选侧；否则尝试相反侧；两侧都不完整时，在四侧中选择
   主轴溢出最少的 `bestFit`。
5. `shift` 最后同时 clamp 两轴，保证完整 surface 不越过可视边界。
6. 定位器常驻，用空文案撤销 claim。共享 IconButton 点击时先同步 dismiss，再执行
   可能卸载锚点的业务回调；token 防止旧 locator 清除新 tip，lease 仅兜底模型直接
   删除导致的异常生命周期。

### ComponentFactory 边界

跨 crate factory 的 global 不共享，因此“全局统一”指同一实现与契约，而不是同一个
global 实例。`MainWindow` 与 Settings factory 各挂一个通用根层
`GitHubTooltipOverlayHost`；通用 host 按 claim token 在两套条件 surface 之间切换，使
上一套先卸载、下一套以当前文案重新测量，不能沿用上一条 preferred-width。Messages
继续遵守此前实测约束：动作图标写死在叶子组件，行内锚点只发布
`MessageTooltipKind` 和矩形，不把任何字符串属性穿过列表 delegate；页面根部唯一的
`GitHubMessageTooltipOverlayHost` 为三个动作分别挂载独立的字面量 surface。所有 host
复用同一个 `GitHubFloatingOverlayHost` 几何算法。这样不会重现属性串台，也不会被行
`clip` 或下一行背景遮挡。

当前独立 factory 清单只有 Settings 与 Messages。以后新增独立生成 crate/page 时，
必须在该 factory 的根组件挂载自己的 portal；普通功能分包继续使用 `MainWindow` 的
portal，禁止为了“全局统一”跨 factory 读取另一个生成图的 global。

## 6. 回归边界 / Regression boundaries

- 窗口四角、四边与窄宽度下，surface 必须自动 flip/shift，完整保留在 8px 边界内。
- ScrollView、列表、卡片和菜单关闭按钮不得通过放开 clip 或局部提高 `z` 绘制 tip。
- 点击会卸载锚点的操作必须在业务回调前同步关闭 tip，禁止闪到窗口原点。
- Messages 反复进入/离开、切换已读状态、删除行时不得串文案或保留旧 tip。
- 在任意通用 factory 内连续 hover 长短不同的 Tooltip 时，每次都必须重新测量宽度。
- Settings/Messages 的 Tooltip 不得依赖 `MainWindow` 的 global 实例。
- 新增独立 factory 时必须同时新增 factory-root portal，并加入此处的独立 factory 清单。

## 7. 相关文档 / Related docs

- 稳定契约：[`docs/specs/ui-design.md`](specs/ui-design.md)（`GitHubTooltip`）
- Messages：[`docs/specs/messages.md`](specs/messages.md)
- Status bar：[`docs/specs/status-bar.md`](specs/status-bar.md)
- 架构：[`docs/architecture.md`](architecture.md)、[`docs/architecture.zh.md`](architecture.zh.md)
