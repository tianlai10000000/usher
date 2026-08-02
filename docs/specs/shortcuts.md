# Application keyboard shortcuts / 应用快捷键

## Scope / 范围

This specification defines Usher's application-wide keyboard-command boundary.
It covers raw key delivery, platform normalization, declarative bindings,
command scope, conflict handling, dispatch, tooltip labels, and extension rules.
Text editing and component-local navigation remain owned by their controls.

本文定义 Usher 的应用级键盘命令边界，覆盖原始按键传递、平台归一化、声明式绑定、
命令作用域、冲突处理、分发、Tooltip 标签与扩展规则。文本编辑和组件内部导航仍由
对应控件负责。

The initial map follows observable Codex desktop shell conventions:
Command/Ctrl+B toggles the sidebar, Command/Ctrl+J toggles the bottom panel,
Command/Ctrl+[ and Command/Ctrl+] navigate history, and Command/Ctrl+, opens
Settings.

首批键位对齐 Codex 桌面端可观察的 Shell 菜单约定：Command/Ctrl+B 切换侧栏，
Command/Ctrl+J 切换底部面板，Command/Ctrl+[ 与 Command/Ctrl+] 前后导航，
Command/Ctrl+, 打开设置。

## Ownership / 职责归属

- `ui/main.slint` owns one full-window ancestor `FocusScope`. It forwards raw
  key text and modifier flags through `app-shortcut-key-pressed` and accepts an
  event only when Rust reports that a command actually ran. On macOS, the
  platform adapter forwards physical `NSEvent.keyCode` values through the same
  Rust resolver because Slint's logical text varies with the active input
  method.
- `src/shortcuts/mod.rs` owns stable `CommandId` values, default bindings,
  platform-independent modifier normalization, scopes, conflict validation,
  command availability, dispatch, display labels, and the single visible
  startup initialization entry.
- Feature modules own the action behind a command. The shortcut dispatcher
  invokes existing root callbacks; it does not duplicate Router, terminal, or
  feature state.
- `src/platform/` may render native menu/button labels from the registry, but
  it must not define a second executable binding table.
- Component-local editing commands such as Copy, Paste, Select All, and the PR
  diff find interaction remain in their focused components unless promoted to
  a named application command with explicit scope and conflict tests.

- `ui/main.slint` 只拥有一个覆盖完整窗口祖先层的 `FocusScope`，通过
  `app-shortcut-key-pressed` 转发原始按键和修饰键；只有 Rust 确认命令已执行时
  才接受事件。macOS 会由平台适配层把物理 `NSEvent.keyCode` 交给同一个
  Rust resolver，避免 Slint 逻辑文本随当前输入法改变。
- `src/shortcuts/mod.rs` 统一拥有稳定 `CommandId`、默认键位、跨平台修饰键归一化、
  作用域、冲突校验、命令可用性、分发、展示标签与唯一的可见启动初始化入口。
- 具体 Feature 仍拥有命令动作。快捷键模块只调用既有根 callback，不复制 Router、
  Terminal 或业务状态。
- `src/platform/` 可以读取注册表生成原生菜单/按钮标签，但不得维护第二份可执行键位表。
- Copy、Paste、Select All 和 PR Diff Find 等编辑型命令继续由获得焦点的组件处理；
  若提升为应用命令，必须提供明确作用域及冲突测试。

## Default bindings / 默认键位

| Command | macOS | Windows/Linux | Scope |
| --- | --- | --- | --- |
| Toggle sidebar | `⌘B` | `Ctrl+B` | Application |
| Toggle terminal bottom panel | `⌘J` | `Ctrl+J` | `Route.Workflow` and `Route.Project` |
| Back | `⌘[` | `Ctrl+[` | Application, when history is available |
| Forward | `⌘]` | `Ctrl+]` | Application, when history is available |
| Open Settings | `⌘,` | `Ctrl+,` | Application, except when already on Settings |

## Invariants / 不变量

1. There is one root application shortcut listener. Pages and native chrome do
   not independently implement application commands.
2. A binding consists of a stable command, exact key, exact modifiers, and a
   scope. Extra modifiers never activate a simpler binding.
3. Application scope overlaps every route scope for conflict detection. Two
   bindings with the same key/modifiers and overlapping scopes make registry
   construction fail during the visible initialization stage.
4. The primary modifier means Command on macOS and Control on Windows/Linux.
   Backend-specific flag names are normalized at the Slint/Rust boundary and
   must not leak into feature code. Punctuation bindings must be independent of
   the active input method or keyboard layout.
5. A modal overlay owns input. Application shortcuts reject while one is
   visible, allowing the focused dialog to handle Escape and editing keys.
6. A disabled command rejects instead of swallowing the event. Back/Forward
   require Router history; Settings rejects on Settings itself.
7. Unmatched events return to the focused child. Standard text editing and
   accessibility keyboard behavior must continue to work unchanged.
8. Tooltips and future menu accelerators obtain display labels from the same
   registry as execution. A literal shortcut may not be appended separately.
9. A new application or route command extends `CommandId`, the declarative
   binding table, the dispatcher, and resolver tests in one change. A feature
   command must reuse an existing root callback or add a consistently named
   Slint/Rust callback pair.
10. The shortcut dispatcher performs no I/O and runs synchronously on the Slint
    event loop.
11. Shortcut initialization begins after the window is shown. The bottom status
    bar reports registry/callback and platform-bridge progress. `Shortcuts
    ready` is published only after every required input boundary is installed;
    failures remain visible and shortcuts are never reported ready prematurely.
    After completion, progress collapses into a persistent success/failure icon.
    Activating it opens the shared status-bar overlay with each initialization
    stage and registered command's binding, scope, and load state.

1. 应用级快捷键监听器只能有一个；页面与原生 Chrome 不得各自重复实现应用命令。
2. 每个绑定由稳定命令、精确按键、精确修饰键和作用域组成；附加修饰键不得误触发
   更简单的绑定。
3. 冲突检测中 Application 作用域与所有 Route 作用域重叠；同键同修饰键且作用域
   重叠时，注册表必须在可见初始化阶段构造失败。
4. Primary 修饰键在 macOS 表示 Command，在 Windows/Linux 表示 Control；Slint
   后端字段差异只能在 Slint/Rust 边界归一化，不得泄漏到 Feature。标点快捷键
   必须与当前输入法或键盘布局无关。
5. Modal Overlay 拥有输入；显示期间应用快捷键全部拒绝，让当前 Dialog 继续处理
   Escape 和编辑键。
6. 不可用命令必须拒绝而不是吞键；Back/Forward 依赖 Router 历史，Settings 页面
   自身不重复执行 Open Settings。
7. 未匹配事件返回当前焦点控件，文本编辑与无障碍键盘行为必须保持不变。
8. Tooltip 与未来菜单 accelerator 必须从执行注册表读取展示标签，不得另写字面量。
9. 新增应用级或路由级命令时，必须在同一改动中扩展 `CommandId`、声明式绑定表、
   dispatcher 和 resolver 测试；Feature 命令应复用既有根 callback，或新增命名一致的
   Slint/Rust callback 对。
10. 快捷键分发不执行 I/O，并在 Slint event loop 上同步完成。
11. 快捷键初始化在窗口显示后开始，底部状态栏展示注册表/callback 与平台输入
    桥接进度。只有所有必需输入边界安装成功才发布 `Shortcuts ready`；失败状态
    必须保持可见，不得提前宣告可用。完成后进度收成常驻成功/失败图标；激活后
    通过共享状态栏 Overlay 展示各初始化阶段，以及每个命令的键位、作用域与载入状态。

## Failure behavior / 失败行为

- Invalid duplicate defaults fail the visible startup stage with both command
  IDs, scope, and key in the log; the status bar remains failed.
- Unknown keys, unavailable commands, route mismatches, and modal blocking are
  normal rejections and produce no user-facing error.
- If the window has already been destroyed, the callback rejects safely.
- Chords, user remapping, and persisted overrides are not part of the first
  implementation. They must extend the registry layer rather than introduce a
  parallel listener.

- 默认键位重复时可见启动阶段失败，日志包含两侧命令、作用域和按键，底栏保持失败状态。
- 未知按键、不可用命令、路由不匹配及 Modal 阻断属于正常拒绝，不显示用户错误。
- 窗口已销毁时 callback 安全拒绝。
- 首版不包含组合序列、用户改键或持久化覆盖；未来必须扩展注册表层，不得增加平行监听器。

## Regression checklist / 回归清单

- Verify every default binding from Home, Chat, Project, Workflow, and Settings.
- Verify the bottom status bar reaches `Shortcuts ready` only after the native
  bridge is installed, then collapses into a persistent status icon whose
  overlay lists every stage and binding.
- Verify `⌘/Ctrl+B` works while a page text field has focus and the typed text is
  unchanged.
- Verify `⌘/Ctrl+J` toggles the terminal on Workflow and is rejected elsewhere.
- Verify unavailable Back/Forward does not swallow the key.
- Verify application shortcuts are blocked by modal overlays and dialog Escape
  still works.
- Verify PR diff `⌘/Ctrl+F`, Copy, Select All, and ordinary text-field editing
  continue to use their component-local handlers.
- Verify macOS native title-bar Tooltip labels and Windows/Linux Slint Tooltip
  labels match the registry.
- Run shortcut unit tests, compile the complete Slint root, and run the standard
  repository validation commands.

- 在 Home、Chat、Project、Workflow 和 Settings 分别验证全部默认键位。
- 验证底部状态栏只在原生输入桥接安装成功后进入 `Shortcuts ready`，随后收成
  常驻状态图标，且 Overlay 列出全部阶段与键位。
- 文本框获得焦点时验证 `⌘/Ctrl+B` 仍可切换侧栏，且输入文本不被改写。
- 验证 `⌘/Ctrl+J` 只在 Workflow 切换 Terminal，其他路由拒绝。
- Back/Forward 不可用时不得吞键。
- Modal Overlay 显示时阻断应用快捷键，同时 Dialog Escape 仍然有效。
- PR Diff 的 `⌘/Ctrl+F`、Copy、Select All 以及普通文本框编辑继续走组件本地处理。
- macOS 原生标题栏 Tooltip 与 Windows/Linux Slint Tooltip 必须和注册表一致。
- 运行快捷键单测、完整 Slint 根编译及仓库标准验证命令。
