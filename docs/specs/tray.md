# Tray behavior / 托盘行为

## Scope / 范围

This specification owns the desktop tray's click-triggered orca jump, static-icon
restoration and animation teardown. It preserves the existing platform-specific
window and menu actions.

本文负责桌面托盘点击时的虎鲸跳跃、静止图恢复及动画清理，保留现有各平台窗口
与菜单操作。

## Ownership / 职责

- `src/tray.rs` owns platform tray setup, event routing and native image adapters.
  `src/tray/animation.rs` owns the shared event-loop player and frame timing.
- `scripts/generate-tray-jump.py` derives transparent 64 × 64 PNG poses from the
  application-owned root `logo.png`; `ui/assets/tray-jump/` holds the baked frames.
- `src/lib.rs` keeps the existing high-priority UI shutdown registration for
  `tray::remove_tray`; no new background animation thread is introduced.

- `src/tray.rs` 负责平台托盘安装、事件路由与原生图片 adapter；
  `src/tray/animation.rs` 负责共享的 event-loop 播放器与帧时序。
- `scripts/generate-tray-jump.py` 从应用自有根目录 `logo.png` 生成透明的
  64 × 64 PNG 姿态，烘焙帧保存在 `ui/assets/tray-jump/`。
- `src/lib.rs` 保留现有 `tray::remove_tray` 高优先级 UI 关闭注册；动画不新增后台线程。

## Invariants / 不变量

1. Only a primary left click starts the jump. macOS right-click and Control-click
   retain the menu path; menu commands do not start playback. Non-macOS event
   routing remains unchanged, including its existing window toggle behavior.
2. A click starts one 24-frame jump with a 30 ms frame interval and a 720 ms
   timeline. The first serviced tick at or beyond the deadline restores the
   actual static image selected at startup; the generated final rest pose is
   only a reference.
3. Repeated clicks during playback do not queue or restart animation. Every
   click still performs its existing window or menu action independently.
4. Frames and platform image objects are prepared once during tray setup. Clicks
   and ticks do not read image files, decode PNGs, sleep or perform process work.
5. The Slint Timer selects the frame from monotonic elapsed time. A delayed tick
   skips missed frames instead of extending playback or replaying a backlog.
   Native image changes and player teardown run on the event loop.
6. macOS uses 22 pt original-colour, non-template images for both the static icon
   and animation frames. The orca's eye and white markings remain visible;
   Light/Dark themes must not replace the artwork with a tinted silhouette.
   Other platforms preserve the 32 px `tray-icon` image size. All poses retain
   transparent canvas margins.
7. `remove_tray` stops and drops the player before releasing native tray resources.
   A late tick must not access a removed tray. The animation adds no user-facing
   text and does not alter menu localization or persisted state.

1. 只有左键主点击启动跳跃。macOS 右键与 Control-click 保留菜单路径，菜单操作
   不播放动画。非 macOS 事件路由保持原样，包括原有的窗口 toggle 行为。
2. 一次点击启动 24 帧跳跃，帧间隔 30 ms、时序长 720 ms。达到或超过截止时间后
   首次执行的 tick 恢复启动时实际选定的静止图片；生成的最终静止姿态只作为对照。
3. 播放中的重复点击不排队、不重新开始动画，每次点击仍独立执行原有窗口或菜单动作。
4. 帧与平台图片对象只在托盘安装时准备一次；点击和 tick 不读取图片文件、解码 PNG、
   sleep 或执行进程操作。
5. Slint Timer 根据单调 elapsed 时间选帧；迟到 tick 跳过错过的帧，不延长动画或补播。
   原生图片替换与播放器清理均在 event loop 执行。
6. macOS 静止图与动画帧统一使用 22 pt 原色、非 template 图片，保留虎鲸的眼睛
   与白色斑纹；亮色/暗色主题不得把图案变成染色剪影。其他平台保留 32 px
   `tray-icon` 图片尺寸，所有姿态均保留透明画布边距。
7. `remove_tray` 先停止并释放播放器，再释放原生托盘资源；迟到 tick 不得访问已移除的
   托盘。动画不新增用户可见文案，不改变菜单语言处理或持久状态。

## Failure behavior / 失败行为

An unavailable static logo or animation preparation failure keeps the existing
static icon or platform fallback and leaves playback disabled.
An image-update failure is logged; it does not change existing tray actions or
extend the playback deadline. Completion still attempts to restore the selected
static image. No animation error dialog is added, and teardown must remain safe.

静止 Logo 不可用或动画准备失败时保留现有静止图或平台回退，并禁用播放。
图片更新失败只记录诊断，不改变现有托盘动作、
不延长播放截止时间，结束时仍尝试恢复选定的静止图。不新增动画错误弹窗，清理须安全。

## Regression checklist / 回归清单

- Run `python3 scripts/generate-tray-jump.py --check` to verify baked assets
  reproduce from `logo.png`, keep clear borders and have identical static endpoints.
- Test first click, repeated clicks, delayed ticks, final restoration and teardown.
- On macOS, inspect the static icon and jump at menu-bar size in Light/Dark;
  verify original colours, the eye and white markings remain visible before,
  during and after playback. Verify left-click reveals the window while
  right-click/Control-click and Quit retain their actions.
- On Windows, inspect the native tray animation, existing click routing and menu
  actions; cross-compilation or player unit tests do not replace native acceptance.
- Verify idle playback stays stopped; quit during playback without a late update.
- Switch English and one packaged non-source locale; check affected tray actions
  retain their existing behavior and the animation does not introduce locale copy.
- Run the standard repository formatting, check, library-test and diff gates.

- 执行 `python3 scripts/generate-tray-jump.py --check`，验证烘焙资源可由 `logo.png`
  重现、边缘留白与首尾静止姿态一致。
- 测试首次点击、连续点击、迟到 tick、最终恢复及清理。
- 在 macOS 亮色/暗色菜单栏实际尺寸下检查静止图与动作；确认播放前、播放中与
  播放后均保留原色、眼睛与白色斑纹。确认左键显示窗口、右键/Control-click
  与 Quit 保留原有动作。
- 在 Windows 原生托盘检查动画、原有点击路由及菜单；交叉编译或播放器单测不能替代实机验收。
- 确认静止时 Timer 停止，动画中退出后不发生迟到图片更新。
- 切换 English 和一种内置非源语言，检查托盘操作保留原有行为、动画不引入语言相关文案。
- 运行仓库标准格式、编译、库单测及 diff 检查。
