# Diagnostic Logging Specification / 诊断日志规格

## Scope / 范围

This specification defines user-controlled application log destinations, optional high-volume diagnostics, crash-run detection, retention, and privacy-bounded diagnostic export. Explicit benchmark-only environment outputs remain developer tooling and are outside ordinary Settings behavior.

本文定义用户可控制的应用日志输出目标、可选高频诊断、异常运行检测、保留策略与隐私有界的诊断包导出。由显式环境变量启用的基准专用输出仍属于开发工具，不属于普通设置行为。

## Controls / 控制项

The five diagnostic settings are stored under `[diagnostics]` in `settings.toml`.
Legacy root keys are accepted on load as per-field fallbacks; explicit section
values win. Saving emits only the section keys and preserves existing values
and defaults. Update preferences `auto_update` and `update_channel` use `[update]`
with the same migration rules.

五个诊断设置统一保存在 `settings.toml` 的 `[diagnostics]` 下。加载时旧顶层键作为
逐字段回退，分类内显式值优先；保存只写分类内键，保留已有值与默认值。
更新设置 `auto_update` 和 `update_channel` 使用 `[update]`，迁移规则相同。

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

## Severity / 日志级别

Expected macOS native-view/window readiness retries use `DEBUG`; ordinary shutdown
signals, requests, cleanup, termination and event-loop exit use `INFO`. A successful
native startup and orderly exit must not emit `WARN` solely for those lifecycle steps.

Real AppKit installation failures and degraded fallbacks retain `WARN`. If native
Chrome is still unavailable at the 1200 ms startup reveal deadline, revealing the
visible fallback emits one warning. Shutdown dispatch failure retains `ERROR`;
quit-event-loop failure, skipped UI/AppKit cleanup and forced-exit timeout retain
`WARN`. Severity changes must not change installation, reveal or shutdown ordering,
and all ordinary records still respect the existing destination switches.

macOS 原生 View/Window 暂未就绪的预期重试使用 `DEBUG`；普通关闭信号、请求、清理、
终止及 event-loop 退出使用 `INFO`。成功的原生启动与有序退出不得仅因这些正常
生命周期步骤产生 `WARN`。

真实 AppKit 安装失败与降级 fallback 保留 `WARN`。1200 ms 启动显示截止时间到达后，
若原生 Chrome 仍未就绪，显示可见 fallback 时记录一次警告。关闭 dispatch 失败保留
`ERROR`；quit-event-loop 失败、跳过 UI/AppKit 清理及强制退出超时保留 `WARN`。
调整级别不得改变安装、显示或关闭顺序，普通记录仍遵循既有输出目标开关。

## Invariants / 不变量

1. Turning off console and both file destinations stops ordinary application log output immediately without restarting.
2. Turning every control off produces no ordinary `log` or `tracing` output and no Terminal or memory diagnostic records.
3. Re-enabling a destination at runtime resumes output; the application file is opened lazily on its first enabled write.
4. Every value persists in `settings.toml`. Legacy settings receive build-compatible defaults: console and tracing enabled, Terminal verbose disabled, and application-file/memory diagnostics enabled only for debug builds.
5. Logger filtering is thread-safe and never performs reducer, persistence, or filesystem work on the Slint event loop except the existing deferred Settings save trigger.
6. `USHER_MEMORY_DIAGNOSTICS=1` may force memory diagnostics for controlled benchmarks even when the UI preference is off.
7. Pre-logger startup failures and explicit environment-controlled benchmark tools may write directly to the process console; these are not normal application logging destinations.
8. The global panic hook always appends one valid JSON panic record to the configured tracing path, or its default path when tracing was never initialized, even when structured tracing is disabled. It then requests shutdown; ordinary tracing emission still respects the runtime switch.
9. The application file sink uses a 64 KiB `BufWriter`. Warn/error records flush
   immediately; ordinary write activity flushes at least once per second, and
   registered shutdown flushes the remaining buffer. The logger's
   `flush_application_log` directly flushes the real application sink after the
   cleanup-pipeline summary and final `MainWindow`-drop record, including the
   cleanup tail on background fallback. The existing post-shutdown flush task
   and its priority remain intact. No long-lived background flush thread is created.
10. Startup writes a crash sentinel under application data before UI setup. An
    orderly shutdown removes it; finding it on the next launch produces one
    bounded warning and never includes user content.
11. Date-partitioned log directories older than 14 days are removed on a worker.
    Diagnostic archives are retained newest-first with a maximum of five.
12. Settings can reveal the log directory and export a `.tar.gz` containing only
    regular `.log` files plus a non-sensitive runtime manifest. Symlinks, files
    larger than 8 MiB, and content beyond a 64 MiB aggregate budget are skipped.
    Settings, tokens, messages, chat histories, repository content, and other
    user data are never collected.
13. The scheduled macOS performance gate runs the Release Home fixture in three
    isolated processes with two settled cycles. It must observe the expected
    route, keep RSS and physical-footprint peaks at or below 256 MiB, and keep
    the median cycle-two growth for both metrics at or below 16 MiB. Raw logs,
    summaries, and the threshold report are retained as workflow artifacts.

1. 关闭 Console 与两个文件输出后，普通应用日志立即停止，无需重启。
2. 关闭全部控制项后，不再产生普通 `log`/`tracing` 输出，也不再产生 Terminal 或内存诊断记录。
3. 运行时重新启用输出目标后恢复输出；应用日志文件只在首次启用写入时按需打开。
4. 所有值持久化到 `settings.toml`。旧配置采用与构建类型兼容的默认值：Console 与 tracing 开启、Terminal verbose 关闭，应用文件与内存诊断仅在 Debug 构建默认开启。
5. Logger 过滤必须线程安全；除既有延迟 Settings 保存触发外，不得在 Slint event loop 执行 reducer、持久化或文件系统工作。
6. 受控基准可通过 `USHER_MEMORY_DIAGNOSTICS=1` 强制启用内存诊断，即使 UI 偏好已关闭。
7. Logger 初始化前的启动失败与显式环境变量控制的基准工具可以直接写进程控制台；它们不属于普通应用日志输出目标。
8. 全局 panic hook 无论 structured tracing 是否关闭，都必须向已配置 tracing 路径追加一条有效 JSON panic 记录；tracing 尚未初始化时使用默认路径。落盘后请求关闭；普通 tracing 事件仍遵循运行时开关。
9. 应用文件 sink 使用 64 KiB `BufWriter`；warn/error 立即 flush，普通写入活动至少每秒
   flush 一次，注册的 shutdown 任务负责冲刷剩余缓冲。Logger 的 `flush_application_log`
   在清理管线总结及最后的 `MainWindow` 释放记录之后直接 flush 实际应用 sink，
   也覆盖后台 fallback 的清理日志尾部。既有 post-shutdown flush 任务与优先级保留，
   不得为此新增常驻 flush 线程。
10. UI 装配前在应用数据目录写入崩溃哨兵；正常关闭时删除。下次启动发现哨兵时只显示一条
    有界警告，且哨兵不包含用户内容。
11. 在 Worker 上删除早于 14 天的日期日志目录；诊断归档按时间保留最新文件，最多 5 个。
12. Settings 可打开日志目录并导出 `.tar.gz`；归档只包含普通 `.log` 文件和不敏感的运行
    manifest。跳过符号链接、单文件超过 8 MiB 或累计超过 64 MiB 的内容；不得收集 Settings、
    Token、消息、Chat 历史、仓库内容或其他用户数据。
13. 定时 macOS 性能门禁使用 Release Home fixture 运行 3 个隔离进程和两个稳定周期；必须
    观察到预期路由，将 RSS 与 physical footprint 峰值分别限制在 256 MiB 以内，并将两项
    指标第二周期相对第一周期的中位增长限制在 16 MiB 以内。原始日志、汇总和阈值报告作为
    workflow artifact 保留。

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
- With valid configuration and no independent failure, verify normal native
  startup and orderly signal/menu shutdown emit no lifecycle `WARN`; expected
  native-handle retries remain `DEBUG` and shutdown steps remain `INFO`.
- Exercise native-Chrome reveal deadline and shutdown failure/fallback paths;
  verify the deadline emits one warning, fallback/timeout warnings remain visible,
  and dispatch failure remains an error when its destination is enabled.
- Load a legacy settings file without the five granular fields and verify defaults.
- Confirm Settings works in both regular and compact layouts.
- Write ordinary, warn, and error records; verify urgent records are flushed
  immediately and shutdown leaves no buffered tail behind. With application-file
  logging enabled, verify the final INFO cleanup summary and `MainWindow`-drop
  record reach the file; background fallback must retain its final cleanup summary.
- Simulate one unclean run and verify the next launch warns once; complete a
  clean shutdown and verify the following launch does not warn.
- Export diagnostics with unrelated files and symlinks present under the log
  tree; inspect the archive and verify only bounded `.log` files and the
  manifest are included. Create more than five exports and verify only the five
  newest remain.
- Run `python3 -m unittest scripts/tests/test_memory_benchmark_thresholds.py`
  and verify both the passing fixture and intentional over-budget fixture are
  classified correctly. Dispatch the nightly workflow and inspect its retained
  `thresholds.md` plus raw samples.

- 分别切换每个输出目标，确认只有对应 sink 变化。
- 先单独开启部分行，再使用 `Disable all`；确认所有开关都显示为关闭、批量操作变为 `Enable all`，并且 Console、`usher.log`、`usher_tracing.log`、Terminal verbose 与周期内存记录停止。
- 全部关闭时触发 Tooltip 与更新弹窗交互，确认 Slint 诊断不会绕过 logger 直接写标准输出。
- 使用 `Enable all`，确认所有行开启且无需重启即可恢复输出。
- 重启后确认五项值从 `settings.toml` 恢复。
- 在配置有效且没有其他失败时，确认正常原生启动与信号/菜单有序关闭不产生生命周期
  `WARN`；原生句柄预期重试仍为 `DEBUG`，关闭步骤仍为 `INFO`。
- 验证原生 Chrome 显示截止时间及关闭失败/fallback 路径：截止时间只警告一次，
  fallback/超时警告保留，dispatch 失败在输出目标开启时仍记录为 error。
- 加载不含五项细分字段的旧配置并确认默认值。
- 确认 Settings 在普通与紧凑布局下均可用。
- 写入普通、warn 与 error 记录，确认紧急记录立即 flush；应用文件输出开启时，
  关闭后的文件必须保留最后的 INFO 清理总结及 `MainWindow` 释放记录，后台
  fallback 也必须保留最后的清理总结，不得留下未冲刷的日志尾部。
- 模拟一次异常退出，确认下次启动只警告一次；随后正常关闭，确认再启动时不再警告。
- 在日志树中放入无关文件和符号链接后导出，检查归档仅含有界 `.log` 与 manifest；连续创建
  超过 5 个归档，确认只保留最新 5 个。
- 运行 `python3 -m unittest scripts/tests/test_memory_benchmark_thresholds.py`，确认正常 fixture
  与故意超限 fixture 分别判定为通过和失败；手动触发 nightly workflow，检查保留的
  `thresholds.md` 与原始采样。
