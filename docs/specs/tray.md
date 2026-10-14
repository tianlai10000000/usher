# Tray behavior / 托盘行为

## Scope / 范围

This specification owns desktop tray click routing, macOS native menu tracking
and localization, the click-triggered orca jump, static-icon restoration and
teardown. Primary clicks retain the platform-specific window action.

本文负责桌面托盘点击路由、macOS 原生菜单追踪与语言刷新、点击时的虎鲸跳跃、
静止图恢复及清理。主点击保留各平台原有的窗口操作。

## Ownership / 职责

- `src/tray.rs` owns platform tray setup, event routing and native image adapters.
  Its macOS adapter owns temporary menu attachment and native tracking lifetime.
  `src/tray/animation.rs` owns the shared event-loop player and frame timing.
  `src/tray/locale_fixture.rs` owns an explicit offline native locale regression
  under the macOS `test-fixtures` build.
- `scripts/generate-tray-jump.py` derives transparent 64 × 64 PNG poses from the
  application-owned root `logo.png`; `ui/assets/tray-jump/` holds the baked frames.
- `src/lib.rs` keeps the existing high-priority UI shutdown registration for
  `tray::remove_tray` and calls `tray::sync_language` after a successful locale
  change. The macOS Quit item resolves the Rust-only `tray.quit` catalog key;
  no new background animation thread is introduced.

- `src/tray.rs` 负责平台托盘安装、事件路由与原生图片 adapter；
  macOS adapter 负责临时菜单挂载与原生追踪期间的对象生命周期。
  `src/tray/animation.rs` 负责共享的 event-loop 播放器与帧时序。
  `src/tray/locale_fixture.rs` 负责 macOS `test-fixtures` 构建中显式启用的离线
  原生菜单语言回归。
- `scripts/generate-tray-jump.py` 从应用自有根目录 `logo.png` 生成透明的
  64 × 64 PNG 姿态，烘焙帧保存在 `ui/assets/tray-jump/`。
- `src/lib.rs` 保留现有 `tray::remove_tray` 高优先级 UI 关闭注册，并在语言切换
  成功后调用 `tray::sync_language`。macOS Quit 菜单项使用仅供 Rust 消费的
  `tray.quit` catalog key；动画不新增后台线程。

## Invariants / 不变量

1. Only a primary left click starts the jump. macOS dispatches completed left-
   and right-mouse-up events; right-click and Control-left-click open the menu,
   while primary left-click shows the window. Menu commands do not start playback. Non-macOS event
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
   text or persisted state.
8. macOS opens the first completed right-click or Control-click through temporary
   `NSStatusItem.menu` attachment and `NSStatusBarButton.performClick:`. Native
   tracking owns menu presentation; the menu is detached when tracking returns,
   including Escape or outside-click dismissal, restoring primary-click routing.
9. While macOS menu tracking is active, the application-activation observer must
   not reopen the hidden main window. Dock/Cmd-Tab reopen behavior outside menu
   tracking is retained. A tracking guard retains the status item and button
   across AppKit's nested run loop, then detaches the menu and releases those
   references even if tray teardown occurred inside that loop.
10. The macOS Quit title is translated on menu creation and refreshed in place
    after a successful runtime locale switch. It does not retain translated copy
    in durable state or add a generated Slint property.

1. 只有左键主点击启动跳跃。macOS 处理已完成的左键与右键 mouse-up：右键和
   Control-left-click 打开菜单，左键主点击显示窗口；菜单操作不播放动画。
   非 macOS 事件路由保持原样，包括原有的窗口 toggle 行为。
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
   托盘。动画不新增用户可见文案或持久状态。
8. macOS 首次完成右键或 Control-click 后，通过临时挂载 `NSStatusItem.menu`
   并调用 `NSStatusBarButton.performClick:` 进入原生菜单追踪。原生系统负责菜单
   展示；追踪返回后解除菜单挂载，包括 Escape 或点击外部取消，恢复左键主点击路由。
9. macOS 菜单追踪期间，应用激活 observer 不得重新显示已隐藏的主窗口；追踪外
   保留 Dock/Cmd-Tab 的唤起行为。追踪 guard 在 AppKit 嵌套 run loop 期间持有
   status item 与 button，即使该循环内发生托盘清理，返回时仍解除菜单挂载并释放引用。
10. macOS Quit 标题在创建菜单时翻译，并在运行时语言切换成功后原位刷新；
    不把翻译后的文案写入持久状态，也不新增生成的 Slint property。

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
- With Usher active, another application active, the main window hidden and the
  main window minimized, verify the first completed right-click and Control-click
  each open the macOS menu. Dismiss through Escape and outside click; verify the
  next primary click shows/restores the window and starts the jump. Confirm menu
  activation does not reopen a hidden window, then check Dock and Cmd-Tab reopen
  after dismissal. Quit from the native menu and verify safe tracking teardown.
- On Windows, inspect the native tray animation, existing click routing and menu
  actions; cross-compilation or player unit tests do not replace native acceptance.
- Verify idle playback stays stopped; quit during playback without a late update.
- Switch English and one packaged non-source locale without recreating the tray;
  verify the existing macOS menu's Quit title refreshes and click actions remain
  correct. Check switching while the menu is open when the native event loop
  permits it; the animation does not introduce locale copy.
- Run the standard repository formatting, check, library-test and diff gates.

- 执行 `python3 scripts/generate-tray-jump.py --check`，验证烘焙资源可由 `logo.png`
  重现、边缘留白与首尾静止姿态一致。
- 测试首次点击、连续点击、迟到 tick、最终恢复及清理。
- 在 macOS 亮色/暗色菜单栏实际尺寸下检查静止图与动作；确认播放前、播放中与
  播放后均保留原色、眼睛与白色斑纹。确认左键显示窗口、右键/Control-click
  与 Quit 保留原有动作。
- 分别在 Usher 活跃、其他应用活跃、主窗口隐藏及主窗口最小化时，验证首次完成
  右键与 Control-click 都打开 macOS 菜单。通过 Escape 与点击外部取消后，确认
  下一次左键主点击显示/恢复窗口并启动跳跃；菜单激活不得唤起隐藏窗口。取消后
  验证 Dock 与 Cmd-Tab 唤起，再从原生菜单退出并检查追踪期间的安全清理。
- 在 Windows 原生托盘检查动画、原有点击路由及菜单；交叉编译或播放器单测不能替代实机验收。
- 确认静止时 Timer 停止，动画中退出后不发生迟到图片更新。
- 不重建托盘，切换 English 和一种内置非源语言，检查现有 macOS 菜单的 Quit
  标题立即刷新且点击动作正确；原生 event loop 允许时也检查菜单打开期间的语言
  切换。动画不引入语言相关文案。
- 运行仓库标准格式、编译、库单测及 diff 检查。

Run the explicit native locale fixture with disposable offline state:

```sh
tray_fixture_state=$(mktemp -d "${TMPDIR:-/tmp}/usher-tray-locale.XXXXXX")
USHER_BENCHMARK_OFFLINE=1 USHER_BENCHMARK_STATE_ROOT="$tray_fixture_state" \
  USHER_TRAY_LOCALE_FIXTURE=1 cargo run --locked --features test-fixtures
```

The fixture invokes the root language callback used by Settings, reads the same
native menu object's title through English → Chinese → English, and exits after
three `PASS stage` records plus `PASS complete`. A `FAIL` record or missing
completion fails acceptance, regardless of the process exit code.

此夹具调用 Settings 使用的根语言 callback，按英文 → 中文 → 英文读取同一个
原生菜单对象的标题，输出三条 `PASS stage` 和 `PASS complete` 后自动退出。
任何 `FAIL` 或缺少完成记录均表示验收失败，即使进程退出码为零。

The macOS native locale fixture passed; evidence is in
[tray locale validation](../validation/tray-locale-20261014/README.md).
Native acceptance of the macOS pointer-click/menu matrix and Windows tray
behavior remains pending; locale inspection does not establish those results.

macOS 原生菜单语言夹具已通过，证据见[托盘语言验证](../validation/tray-locale-20261014/README.md)。
macOS 指针点击/菜单矩阵与 Windows 托盘行为的原生验收仍待完成；语言检查不能
替代这些验收结果。
