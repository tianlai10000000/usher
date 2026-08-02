# Application status bar / 应用状态栏

## Scope / 范围

This specification defines the persistent bottom utility bar owned by
`ui/components/status_bar.slint`: placement, item geometry, status semantics,
tooltips, anchored overlays, and the state supplied by shortcut, notification,
network, version, and update features. Route pages must not recreate this
surface.

本文定义由 `ui/components/status_bar.slint` 持有的常驻底部工具栏，包括位置、
条目几何、状态语义、Tooltip、锚定浮层，以及快捷键、通知、网络、版本和更新功能
提供的状态。各路由页面不得重复实现该 Surface。

## Ownership and data flow / 所有权与数据流

- `ui/components/status_bar.slint` owns presentation, pointer/keyboard
  interaction, compact item geometry, popup selection, and popup anchoring.
- `ui/components/label/github_counter_label.slint` owns the shared Primer
  CounterLabel schemes, limit formatting, compact badge geometry, and
  hide-if-zero behavior used by the notification trigger.
- `ui/main.slint` mounts one status bar at the root shell, below ordinary route
  content and above route-local floating controls. Its x/width follow the live
  main-content edge while the docked sidebar moves.
- `src/shortcuts/` owns shortcut initialization stages, failure/readiness, and
  the binding detail shown by the shortcut popup.
- `src/notification.rs` and `NotificationManager` own the notification queue.
- `src/network.rs` owns the connectivity probe and publishes
  `AppState.network-online`.
- `AppState.version` is the single displayed application version value.
- Update/download state remains owned by the updater and is only presented by
  the status bar.

- `ui/components/status_bar.slint` 负责展示、指针/键盘交互、紧凑条目几何、
  当前浮层选择及浮层锚点。
- `ui/components/label/github_counter_label.slint` 持有通知入口复用的 Primer
  CounterLabel Scheme、上限格式、紧凑徽标几何与零值隐藏行为。
- `ui/main.slint` 在根 Shell 只挂载一个状态栏；它位于普通路由内容之下、路由局部
  浮动控件之上，并在固定侧栏移动时跟随主内容区实时 x/width。
- `src/shortcuts/` 持有快捷键初始化阶段、失败/就绪状态，以及快捷键浮层显示的
  键位详情。
- `src/notification.rs` 与 `NotificationManager` 持有通知队列。
- `src/network.rs` 持有网络探测并发布 `AppState.network-online`。
- `AppState.version` 是唯一的应用版本展示值。
- 更新/下载状态仍由 Updater 持有，状态栏只负责展示。

## Geometry and order / 几何与顺序

1. The bar is 28 px high and remains attached to the bottom edge of the main
   content column.
2. Update checks, downloads, and shortcut initialization progress occupy the
   transient left status area.
3. After shortcut initialization settles, the persistent utility cluster is
   anchored directly to the status bar's right edge with an 8 px inset. It is
   not a child of a width-distributing layout.
4. Persistent items appear in this fixed order: shortcut status, notification,
   network, version.
5. Shortcut, notification, and network use 28 px icon hit targets. Version uses
   a stable 104 px text target. Adjacent items have a 4 px gap. The complete
   cluster is 200 px wide.
6. Opening the 240 px sidebar or resizing down to the 800 px application
   minimum must not clip, overlap, reorder, or hide the version item.
7. The notification trigger is icon-only. A top-right numeric badge displays
   unread count from 1 through 99 and caps larger values at `99+`; count 0
   mounts no badge. Tooltip and popup retain the complete count. A persistent
   `Notifications N` label is not rendered.

1. 状态栏高度为 28px，并始终贴在主内容列底边。
2. 更新检查、下载与快捷键初始化进度使用左侧临时状态区域。
3. 快捷键初始化结束后，常驻工具组以 8px 内缩直接锚定到状态栏右边缘，不得再作为
   会分配宽度的 Layout 子节点。
4. 常驻条目固定顺序为：快捷键状态、通知、网络、版本。
5. 快捷键、通知和网络使用 28px 图标命中区；版本使用稳定的 104px 文字命中区；
   相邻条目间距为 4px，完整工具组宽度为 200px。
6. 打开 240px 侧栏或把窗口缩小到应用 800px 最小宽度时，不得裁切、重叠、重排或
   隐藏版本号。
7. 通知入口只显示铃铛图标；右上角数字徽标显示 1–99 条未读数量，更大值显示
   `99+`，数量为 0 时不挂载徽标。Tooltip 与浮层保留完整数量，不显示常驻
   `Notifications N` 文字。

## Status semantics / 状态语义

Green is reserved for an explicitly healthy or ready state. Color must not be
the only source of meaning; every icon has a tooltip and popup detail.

| Item | Normal/idle | Positive | Failure/attention |
| --- | --- | --- | --- |
| Shortcut | Initialization uses progress and accent color | Ready keyboard icon is green | Failed keyboard icon is red and remains visible |
| Notification | Empty bell is muted and has no badge | Not applicable; notifications are not a health signal | Unread queue uses an accent bell and numeric badge, not green |
| Network | Not applicable | Online broadcast icon is green | Offline alert icon is red |
| Version | Muted text | Not applicable | Not applicable |

绿色只用于明确的“健康/就绪”状态。颜色不得成为唯一信息来源；每个图标均需提供
Tooltip 与浮层详情。

| 条目 | 正常/空闲 | 正向状态 | 失败/注意状态 |
| --- | --- | --- | --- |
| 快捷键 | 初始化期间显示进度和强调色 | 就绪后的键盘图标为绿色 | 失败后的键盘图标为红色并保持可见 |
| 通知 | 空队列铃铛为弱化灰色且无徽标 | 不适用；通知不是健康状态 | 有未读通知时使用强调色铃铛与数字徽标，不使用绿色 |
| 网络 | 不适用 | 在线广播图标为绿色 | 离线警告图标为红色 |
| 版本 | 始终使用弱化文字色 | 不适用 | 不适用 |

## Tooltip and popup behavior / Tooltip 与浮层行为

1. Persistent right-side item tooltips align their trailing edge to the item
   and expand leftward. They must stay inside the window at the minimum width.
2. Tooltip text names the function or current state: shortcut readiness,
   notification count/empty state, connectivity, or version.
3. Clicking an item toggles one shared anchored popup. Only one status popup is
   active at a time.
4. Shortcut detail may use a wider 360 px popup; ordinary utility popups use
   220 px. Shared viewport collision handling constrains either width when the
   window is narrower.
5. Popup viewport and item anchors use the status bar's parent coordinate
   space exactly once. Implementations must not subtract the status-bar x
   twice.
6. Popups open above the bottom anchor, scroll internally if needed, close on
   Escape, and close when the user clicks outside. Opening a popup must not
   remove or cover the version item.

1. 右侧常驻条目的 Tooltip 尾边与条目尾边对齐，并向左展开；在最小窗口宽度下也
   必须留在窗口内。
2. Tooltip 文案说明功能或当前状态：快捷键就绪状态、通知数量/空状态、网络连接或
   版本。
3. 点击条目切换同一个共享锚定浮层；同一时间只能激活一个状态栏浮层。
4. 快捷键详情可使用 360px 宽浮层，普通工具浮层使用 220px；窗口更窄时由共享
   viewport 碰撞逻辑约束宽度。
5. 浮层 viewport 与条目 anchor 只在状态栏父坐标系中换算一次，禁止重复减去
   状态栏 x。
6. 浮层从底部锚点向上打开，内容过高时内部滚动；支持 Escape 与外部点击关闭。
   打开浮层不得移除或遮挡版本项。

## Failure behavior / 失败行为

- Shortcut initialization failures remain visible in red and the popup keeps
  stage/error detail; the UI must never report ready prematurely.
- A failed connectivity probe publishes offline/red without blocking the event
  loop or creating duplicate notification feedback.
- An empty notification queue is a valid idle state, not a success or failure.
- Missing version data hides only the version item; it must not move other
  items outside the right edge.

- 快捷键初始化失败以红色保持可见，浮层保留阶段/错误详情；不得提前报告就绪。
- 网络探测失败发布离线/红色状态，不得阻塞 event loop，也不得重复创建通知反馈。
- 空通知队列是合法空闲状态，不是成功或失败。
- 版本数据缺失时只隐藏版本项，不得把其他条目推出右边界。

## Regression checklist / 回归清单

- Verify all four persistent items at 800 px and at a wide window, with the
  sidebar both open and closed.
- Verify the version remains visible while every status popup is open.
- Verify notification uses a bell icon with muted empty and accent unread
  states; count 0 has no badge, positive counts have a numeric badge, and no
  persistent notification text is rendered.
- Verify shortcut ready/failed and network online/offline use the semantic
  colors above.
- Hover every right-side item and verify its tooltip expands leftward without
  clipping.
- Open each popup and verify anchor position, Escape, outside click, and
  internal scrolling.
- Run the full Slint compile and the repository standard checks.

- 在 800px 与宽窗口下分别检查侧栏展开/收起时的四个常驻条目。
- 打开每一种状态栏浮层时确认版本号仍可见。
- 确认通知只使用铃铛图标：空队列为弱化色且不显示数字徽标，未读为强调色并显示
  数字徽标，不显示常驻通知文字。
- 确认快捷键就绪/失败、网络在线/离线符合上述语义色。
- 悬停全部右侧条目，确认 Tooltip 向左展开且不被裁切。
- 打开每个浮层，检查锚点、Escape、外部点击与内部滚动。
- 编译完整 Slint 根并运行仓库标准检查。

## Design references / 设计参考

- [VS Code 1.21 notification UI](https://code.visualstudio.com/updates/v1_21)
  introduced a status-bar bell that indicates the number of new notifications.
- [Current VS Code layout guidance](https://code.visualstudio.com/docs/configure/custom-layout)
  keeps the bell as the Notification Center entry in the status bar by default.
- [WebStorm notification guidance](https://www.jetbrains.com/help/webstorm/notifications.html)
  uses a compact Notifications entry and a separate unread/importance
  indicator rather than persistent status text.
- [Primer Counter](https://primer.style/product/getting-started/rails/components/counter_label)
  defines counters for navigation/actions, a primary scheme, upper limits, and
  `hide_if_zero` behavior.
- [Primer IconButton accessibility](https://primer-docs-preview.github.com/product/components/icon-button/accessibility/)
  requires a recognizable icon, descriptive accessible name, at least a 24 px
  target, and meaning beyond color alone.
- [Primer degraded experiences](https://www.primer.style/product/ui-patterns/degraded-experiences/)
  hides notification indicators when count data is unavailable.

- [VS Code 1.21 通知界面](https://code.visualstudio.com/updates/v1_21)明确引入
  状态栏铃铛，并用它表示新通知数量。
- [当前 VS Code 布局指南](https://code.visualstudio.com/docs/configure/custom-layout)
  默认继续把铃铛作为状态栏中的通知中心入口。
- [WebStorm 通知指南](https://www.jetbrains.com/help/webstorm/notifications.html)
  使用紧凑通知入口，并用独立未读/重要性标记代替常驻状态文字。
- [Primer Counter](https://primer.style/product/getting-started/rails/components/counter_label)
  定义导航/操作计数、Primary Scheme、上限与 `hide_if_zero` 行为。
- [Primer IconButton 无障碍规范](https://primer-docs-preview.github.com/product/components/icon-button/accessibility/)
  要求图标可识别、提供描述性无障碍名称、命中区至少 24px，且不能只靠颜色传达含义。
- [Primer 降级体验](https://www.primer.style/product/ui-patterns/degraded-experiences/)
  要求通知数量数据不可用时隐藏 Indicator。
