# Tooltip 设计方案纪要 / Tooltip Design Notes

> 文档状态：设计与迁移纪要，不是稳定行为规格。当前产品契约以
> [`docs/specs/ui-design.md`](specs/ui-design.md) 为准；架构归属见
> [`architecture.md`](architecture.md) / [`architecture.zh.md`](architecture.zh.md)。
> 下文保留各方案的取舍与实测结论，便于后续回退或再评估。
>
> **产品决定（2026-08-04）：暂时不采用本纪要中的「Slint 1.17 内置 `Tooltip` +
> `GitHubTooltipContent`」方案。** 原因是延迟约 500ms 不可调，且指针相对定位
> 会挡住宿主按钮。该方案的代码/文档迁移可保留作参考，但产品交互在另行恢复
> 可控 tip 之前不应继续以此为最终交付目标。

日期：2026-08-04 · Slint 1.17.1

## 0. 采用状态 / Adoption status

| 方案 | 状态 |
| --- | --- |
| D. Slint 1.17 内置 `Tooltip` + `GitHubTooltipContent` | **暂不采用**（延迟与挡按钮不可接受） |
| B. 应用自研全局 portal | 已废弃；曾解决 clip，但有串文案/坐标回归 |
| A. 局部 Rectangle tip | 仅作历史对照；易被 clip / 叠层截断 |
| C. Material `ToolTip` | 未采用 |

后续若恢复可控体验，优先重新评估「统一视觉 + 宿主相对定位/可调延迟」路径，
而不是继续打磨内置 `Tooltip` 的公开缺口。

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

### B. 应用自研全局 portal（已废弃）

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

### D. Slint 1.17 内置 `Tooltip` + `GitHubTooltipContent`（当前）

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

> 下列路径描述的是「内置 Tooltip 迁移后」的代码快照，便于回退对照。因产品暂不采用该方案，实际交付应以恢复后的可控 tip 实现为准，勿把本节当作长期契约。

| 路径 | 职责 |
| --- | --- |
| `ui/components/github_tooltip.slint` | 仅 `GitHubTooltipContent` 样式表面 |
| `ui/components/button/github_icon_button.slint` | 共享 IconButton：有文案且 `show-tooltip` 时挂内置 `Tooltip` |
| `ui/components/button/github_copy_button.slint` | Copy→Check；“Copied!” 仅在仍悬停时由 tip 文案体现 |
| `ui/components/messages/github_message_actions.slint` | Inbox 动作：每动作一个内置 `Tooltip` + 字面量文案 |
| `ui/components/showcase/github_ui_components_gallery.slint` | UI Components 中的 Tooltip 示例（动态文案、裁切宿主） |
| ~~`ui/state/tooltip_overlay_state.slint`~~ | 已删除 |
| ~~`ui/components/github_tooltip_overlay.slint`~~ | 已删除 |

## 4. 实测问题（迁移后）/ Observed issues after migration

1. **显示延迟过长**  
   内置悬停延迟约 500ms，且无 delay 属性。桌面工具栏/卡片动作感觉“钝”。

2. **Tip 挡住按钮**  
   指针相对放置会使 tip 叠在宿主上（例如 “Open terminal” 盖住终端图标半截），无法改为“固定在宿主上方/下方 + 4px 间距”。

二者均属 Slint 1.17.1 内置行为，**不是应用样式组件能修的**。

## 5. 若需恢复可控体验 / If product needs controllable tips again

产品已暂不采用内置方案。可选路径：

1. **恢复应用自研 tip（当前倾向）**  
   保留统一的 `GitHubTooltipContent` 视觉；行为改回宿主相对定位 + 可选短延迟。裁切仍需 portal 或抬高 hover 行 z，并严格避免列表属性串台。

2. **维护 Slint runtime / fork**  
   给内置 `Tooltip` 暴露 delay、placement、offset 等 API；升级成本高。

3. **长期再评估内置 `Tooltip`**  
   仅当上游提供可调延迟与宿主相对定位后，再考虑重新采用。

## 6. 相关文档 / Related docs

- 稳定契约：[`docs/specs/ui-design.md`](specs/ui-design.md)（`GitHubTooltipContent`）
- Messages：[`docs/specs/messages.md`](specs/messages.md)
- Status bar：[`docs/specs/status-bar.md`](specs/status-bar.md)
- 架构：[`docs/architecture.md`](architecture.md)、[`docs/architecture.zh.md`](architecture.zh.md)
