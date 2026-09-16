# Terminal Behavior Specification / Terminal 行为规格

## Scope / 范围

This is the stable contract for the project-aware terminal drawer. Historical experiments and abandoned approaches remain in `docs/terminal_viewport_iteration_notes.md`.

本文是项目感知 Terminal Drawer 的稳定契约。历史实验和放弃方案保留在 `docs/terminal_viewport_iteration_notes.md`。

## Ownership / 所有权

- Rust controller, session, and PTY state: `src/terminal/mod.rs`, `session.rs`, and `render.rs`.
- Input admission and asynchronous session retirement: `src/terminal/io.rs` and `lifecycle.rs`.
- Pure keyboard mapping, diagnostic formatting, and refresh throttling policy:
  `src/terminal/input.rs`, `diagnostics.rs`, and `refresh.rs`.
- Root bridge and docking: `ui/main.slint`.
- Drawer presentation: `ui/components/drawer/terminal_drawer.slint`.
- Project entry point: `src/project/mod.rs`.
- Workflow entry point: root shell-drawer callback and
  `GitHubPageScrollActions`.

- Rust controller、Session 与 PTY 状态：`src/terminal/mod.rs`、`session.rs`、`render.rs`。
- 输入准入与异步会话回收：`src/terminal/io.rs`、`lifecycle.rs`。
- 纯键盘映射、诊断格式和刷新节流策略：`src/terminal/input.rs`、
  `diagnostics.rs`、`refresh.rs`。

## Invariants / 不变量

1. `open_terminal_for_project` is the canonical project-page entry point.
2. Reopening the same project preserves PTY/session content and current working directory.
3. Opening another project resets output/input presentation, exits fullscreen, restores dock height, and changes to the new repository directory.
4. Live terminal rendering uses a fixed viewport derived from the visible terminal content area.
5. PTY rows/columns change only after a validated viewport update; do not fake bottom visibility by subtracting rows.
6. The terminal follows bottom during live output without trimming semantic terminal content.
7. Closing the drawer clears fullscreen state so the FAB can return.
8. Fullscreen hides the page scroll view and occupies the work area below the title/breadcrumb region.
9. Docked width and padding align with the page content area.
10. PTY reads, writes, flushes and process waits never run on the Slint event loop.
11. Terminal chrome reuses shared GitHub Button/IconButton focus, tooltip,
    selected, disabled, and loading contracts. Terminal cells keep the
    documented monospace font and PTY-provided ANSI colors.
12. Session shutdown terminates the PTY process group, waits for graceful exit,
    escalates when required, and reaps the child. Descendant commands must not
    survive a project switch as orphan processes.
13. Closing or toggling the drawer hides it without terminating the current
    project session. Hidden sessions keep their Rust render state current but
    defer Slint model refreshes until the drawer reopens. Project replacement
    detaches the old session before terminating it on a worker so late exit or
    refresh events cannot affect the new project.
14. Output admission is bounded to 64 chunks of at most 8 KiB (512 KiB queued,
    plus an 8 KiB read buffer, one pending chunk and one bridge chunk). A full queue backpressures
    the PTY; it never drops live bytes. Only detached/stopping sessions discard
    output. The 1,000-line parsed scrollback is a separate budget.
15. Keys, paste and protocol replies share one ordered Writer. Input admission
    never waits for PTY I/O, accepts an entire input or rejects it, and charges
    in-flight data against both 1 MiB and 128-item limits. Rejection uses the
    existing localized input-failure message; the next accepted user input clears
    it. Writer failure or a rejected protocol reply stops the session explicitly.
16. Full-queue sends wait for notification or an independent cancellation channel;
    they never sleep-poll queue capacity. Shutdown closes
    input admission and discards pending input, stops publishing output, drains
    the kernel PTY while reaping the child, then stops and joins both I/O workers.
    Joining or waiting for child exit must never hold the presentation-state lock.
17. Application shutdown closes new-session admission and awaits active and retired
    sessions in the asynchronous drain, before synchronous UI cleanup. Project-switch
    retirements must remain owned until completed or joined by that barrier.

1. `open_terminal_for_project` 是 Project 页面唯一标准入口。
2. 同项目重开保留 PTY/Session 内容和 cwd。
3. 切换项目时重置输出/输入展示、退出全屏、恢复 Dock 高度并进入新仓库目录。
4. Live Terminal 使用由可见内容区计算出的固定 viewport。
5. 只有收到有效 viewport 更新后才修改 PTY rows/columns；禁止通过少报 rows 伪造底部可见。
6. Live 输出应 follow-bottom，但不能裁剪有语义的终端内容。
7. 关闭 Drawer 时必须清除 fullscreen，保证 FAB 恢复。
8. Fullscreen 隐藏页面 ScrollView，占据标题/面包屑以下工作区。
9. Docked 宽度和 padding 与页面内容区对齐。
10. PTY 读取、写入、flush 和进程等待不得运行在 Slint event loop。
11. Terminal 外壳复用共享 GitHub Button/IconButton 的焦点、Tooltip、选中、
    禁用与 loading 契约。终端单元格继续使用约定的等宽字体和 PTY 提供的 ANSI
    色。
12. Session 关闭时终止 PTY 进程组，等待正常退出，必要时升级终止并回收 Child；
    切换项目后子命令不得作为孤儿进程继续运行。
13. 关闭或切换 Drawer 只隐藏界面，不终止当前项目 Session。隐藏期间继续更新 Rust
    渲染状态，但延迟 Slint model 刷新直到重新打开；替换项目时先摘下旧 Session，再由
    Worker 终止，迟到的退出或刷新事件不得影响新项目。
14. 输出队列最多容纳 64 个、不超过 8 KiB 的块（排队 512 KiB，另有 8 KiB 读取
    缓冲、一个待入队块和一个 Bridge 在途块）。队列满时向 PTY 施加背压，不丢弃活动会话字节；
    只有已摘下或正在关闭的 Session 可以丢弃输出。解析后的 1,000 行 scrollback
    是另一项独立预算。
15. 按键、粘贴和协议应答共用单个有序 Writer。输入准入不等待 PTY IO，整次接受
    或拒绝；在途写入计入 1 MiB、128 项双重上限。拒绝复用已有本地化输入失败
    文案，下次用户输入成功入队后清除；Writer 失败或协议应答无法入队时明确停止会话。
16. 满队列发送等待容量通知或独立取消通道，不以 sleep 轮询队列空位。
    关闭输入准入、丢弃待写输入，停止发布输出，在回收
    Child 期间继续排空内核 PTY，再停止并 join 两个 IO Worker。join 或等待进程
    退出时不得持有展示状态锁。
17. 应用退出先禁止创建新会话，在异步排空阶段等待活动与已摘下 Session，完成后
    才进入同步 UI 清理。切项目产生的回收任务必须持有到完成或由该屏障等待结束。

## Project switching matrix / 项目切换矩阵

| Action | Expected state |
| --- | --- |
| Open project A first time | Start/attach session for A, set cwd to A, show drawer. |
| Close then reopen A | Preserve output/session/cwd and restore drawer presentation. |
| Open B while A exists | Reset presentation, select/start B, set cwd to B. |
| Close in fullscreen | Hide drawer and clear fullscreen. |
| Return from another route | Do not resurrect stale fullscreen/layout state. |

## Viewport synchronization / Viewport 同步

- The Slint drawer reports content width/height after layout.
- Rust converts physical dimensions to terminal columns/rows using the active font metrics.
- Ignore zero, negative, or duplicate viewport values.
- Session startup must use the latest stable viewport; late resize events may refine it.
- Diagnostic logs should include project key, source, route, rows, columns, and viewport dimensions when verbose terminal logging is enabled.

## Input and selection / 输入与选择

- Normal mode forwards keyboard/paste/input to the PTY.
- Selection behavior must not replace the live terminal model or alter PTY viewport semantics.
- Any selection redesign requires separate regression checks for Unicode width, wrapped lines, scrollback, copy, and returning to live input.

## Regression checklist / 回归检查

- First open shows prompt without clipping.
- Close/reopen preserves prompt and does not duplicate shell initialization output.
- Rapid close/open toggles keep the final requested visibility, reuse one session,
  and apply hidden output when reopened.
- A -> B -> A project switching selects the correct session and cwd.
- Resize narrow/wide repeatedly without stale rows, overlapping output, or feedback loops.
- Docked/fullscreen transitions preserve output and restore FAB/page layout.
- Long output follows bottom while manual scrollback remains usable.
- Chinese and wide glyphs keep column alignment with the configured monospace font.
- Application shutdown terminates or detaches PTY processes according to shutdown policy.
- Slow stdin plus a full output queue: input admission stays responsive; cancellation
  completes with both workers joined and the child reaped, even while the receiver lives.
- Backpressured output is byte-for-byte ordered through EOF; partial input writes and
  interrupted/nonblocking retries preserve packet order without duplication.
- Input-failure copy is visible in live mode and refreshes immediately between English
  and a packaged non-source locale without remounting the drawer.
