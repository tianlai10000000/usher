# Diagnostic Logging Specification / 诊断日志规格

## Scope / 范围

This specification defines user-controlled application log destinations and optional high-volume diagnostics. Explicit benchmark-only environment outputs remain developer tooling and are outside ordinary Settings behavior.

本文定义用户可控制的应用日志输出目标与可选高频诊断。由显式环境变量启用的基准专用输出仍属于开发工具，不属于普通设置行为。

## Controls / 控制项

- Console output controls standard application log output.
- Application file logs control plain-text `usher.log` writes.
- Structured tracing controls `usher_tracing.log` writes.
- Terminal verbose diagnostics control input, PTY output, viewport, and render-detail records. Normal terminal lifecycle and error records still follow the enabled application destinations.
- Memory diagnostics control periodic process-memory, UI-model, and feature memory records.
- Settings presents every control independently and provides GitHub-style `Enable all` / `Disable all` bulk actions without hiding individual state.

- Console output 控制应用标准输出日志。
- Application file logs 控制纯文本 `usher.log` 写入。
- Structured tracing 控制 `usher_tracing.log` 写入。
- Terminal verbose diagnostics 控制输入、PTY 输出、viewport 与渲染细节；普通 Terminal 生命周期和错误记录仍跟随已启用的应用输出目标。
- Memory diagnostics 控制周期进程内存、UI model 与 feature 内存记录。
- Settings 独立展示每项状态，并提供 GitHub 风格的 `Enable all` / `Disable all` 批量操作，不用父开关隐藏子项真实状态。

## Invariants / 不变量

1. Turning off console and both file destinations stops ordinary application log output immediately without restarting.
2. Turning every control off produces no ordinary `log` or `tracing` output and no Terminal or memory diagnostic records.
3. Re-enabling a destination at runtime resumes output; the application file is opened lazily on its first enabled write.
4. Every value persists in `settings.toml`. Legacy settings receive build-compatible defaults: console and tracing enabled, Terminal verbose disabled, and application-file/memory diagnostics enabled only for debug builds.
5. Logger filtering is thread-safe and never performs reducer, persistence, or filesystem work on the Slint event loop except the existing deferred Settings save trigger.
6. `USHER_MEMORY_DIAGNOSTICS=1` may force memory diagnostics for controlled benchmarks even when the UI preference is off.
7. Pre-logger startup failures and explicit environment-controlled benchmark tools may write directly to the process console; these are not normal application logging destinations.
8. The global panic hook always appends one valid JSON panic record to the configured tracing path, or its default path when tracing was never initialized, even when structured tracing is disabled. It then requests shutdown; ordinary tracing emission still respects the runtime switch.

1. 关闭 Console 与两个文件输出后，普通应用日志立即停止，无需重启。
2. 关闭全部控制项后，不再产生普通 `log`/`tracing` 输出，也不再产生 Terminal 或内存诊断记录。
3. 运行时重新启用输出目标后恢复输出；应用日志文件只在首次启用写入时按需打开。
4. 所有值持久化到 `settings.toml`。旧配置采用与构建类型兼容的默认值：Console 与 tracing 开启、Terminal verbose 关闭，应用文件与内存诊断仅在 Debug 构建默认开启。
5. Logger 过滤必须线程安全；除既有延迟 Settings 保存触发外，不得在 Slint event loop 执行 reducer、持久化或文件系统工作。
6. 受控基准可通过 `USHER_MEMORY_DIAGNOSTICS=1` 强制启用内存诊断，即使 UI 偏好已关闭。
7. Logger 初始化前的启动失败与显式环境变量控制的基准工具可以直接写进程控制台；它们不属于普通应用日志输出目标。
8. 全局 panic hook 无论 structured tracing 是否关闭，都必须向已配置 tracing 路径追加一条有效 JSON panic 记录；tracing 尚未初始化时使用默认路径。落盘后请求关闭；普通 tracing 事件仍遵循运行时开关。

## Troubleshooting workflow / 问题排查流程

Open **Settings → Diagnostics** before reproducing a problem and explicitly check the current rows; persisted values and build-compatible defaults mean a debug build alone does not guarantee that a particular output is enabled. Prefer the smallest useful combination:

| Problem | Enable | Result |
| --- | --- | --- |
| General application, network, update, or workflow behavior | `Console output` for a live run, or `Application file logs` for a persistent capture | Ordinary application records in the process terminal or `logs/<date>/usher.log` |
| Structured event or crash-path investigation | `Structured tracing file`; also enable an ordinary destination when ordinary `log` records are needed | Structured events in `logs/<date>/usher_tracing.log` |
| Terminal input, PTY output, viewport, or render behavior | `Terminal verbose diagnostics` plus `Console output` and/or `Application file logs` | Additional `[Terminal][Diag]` records in the selected ordinary destination |
| Memory growth, window-size correlation, or retained UI models | `Memory diagnostics` plus `Console output` and/or `Application file logs` | `[Memory][Diag]`, `[Memory][UI]`, and feature-owned memory records |
| Unclear cross-cutting reproduction | Use `Enable all` only for the shortest practical reproduction | Both file destinations, console output, and all detailed diagnostics |

Record the reproduction start time, actions, affected project/route, and expected versus actual result so file records can be correlated. After collecting enough evidence, turn off the temporary rows or use `Disable all`; do not leave high-volume diagnostics enabled as an accidental steady-state configuration. Terminal verbose records may include bounded input and PTY-output previews, so do not enter secrets during capture and review logs before sharing them. Never attach `settings.toml`, credentials, authorization headers, or unrelated user content to a diagnostic report.

复现问题前先打开 **Settings → Diagnostics**，明确核对每一行的当前状态；设置会持久化，且旧配置使用与构建类型兼容的默认值，因此不能仅凭 Debug 构建就假定某项输出已经开启。优先选择最小够用的组合：

| 问题类型 | 开启项 | 结果 |
| --- | --- | --- |
| 普通应用、网络、更新或 Workflow 行为 | 实时观察开启 `Console output`；需要留存则开启 `Application file logs` | 进程终端中的普通应用记录，或 `logs/<date>/usher.log` |
| 结构化事件或崩溃路径 | 开启 `Structured tracing file`；如果还需要普通 `log` 记录，同时开启一个普通输出目标 | `logs/<date>/usher_tracing.log` 中的结构化事件 |
| Terminal 输入、PTY 输出、viewport 或渲染问题 | 开启 `Terminal verbose diagnostics`，并同时开启 `Console output` 和/或 `Application file logs` | 所选普通输出目标中的额外 `[Terminal][Diag]` 记录 |
| 内存增长、窗口尺寸关联或 UI model 滞留 | 开启 `Memory diagnostics`，并同时开启 `Console output` 和/或 `Application file logs` | `[Memory][Diag]`、`[Memory][UI]` 及 Feature 自有内存记录 |
| 暂时无法判断归属的跨模块问题 | 仅在最短可行复现期间使用 `Enable all` | 两个文件目标、控制台及全部详细诊断 |

采集时记录复现开始时间、操作步骤、涉及的项目/路由，以及预期与实际结果，便于和文件日志对齐。证据足够后应关闭临时开启项或使用 `Disable all`，不要把高频诊断意外保留为长期配置。Terminal verbose 可能包含有界的输入与 PTY 输出预览，采集期间不要输入密钥，分享前必须检查日志；诊断报告不得附带 `settings.toml`、凭据、鉴权 Header 或无关用户内容。

## Regression checklist / 回归检查

- Toggle each destination independently and verify only the named sink changes.
- Toggle individual rows on, then use `Disable all`; verify every switch renders off, the bulk action changes to `Enable all`, and console, `usher.log`, `usher_tracing.log`, Terminal verbose records, and periodic memory records stop.
- Exercise Tooltip and update-dialog interactions while all outputs are disabled; verify Slint diagnostics do not bypass the logger and write directly to standard output.
- Use `Enable all`; verify every row is on and output resumes without restart.
- Restart and verify all five values restore from `settings.toml`.
- Load a legacy settings file without the five granular fields and verify defaults.
- Confirm Settings works in both regular and compact layouts.

- 分别切换每个输出目标，确认只有对应 sink 变化。
- 先单独开启部分行，再使用 `Disable all`；确认所有开关都显示为关闭、批量操作变为 `Enable all`，并且 Console、`usher.log`、`usher_tracing.log`、Terminal verbose 与周期内存记录停止。
- 全部关闭时触发 Tooltip 与更新弹窗交互，确认 Slint 诊断不会绕过 logger 直接写标准输出。
- 使用 `Enable all`，确认所有行开启且无需重启即可恢复输出。
- 重启后确认五项值从 `settings.toml` 恢复。
- 加载不含五项细分字段的旧配置并确认默认值。
- 确认 Settings 在普通与紧凑布局下均可用。
