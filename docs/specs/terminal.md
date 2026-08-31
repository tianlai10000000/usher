# Terminal Behavior Specification / Terminal 行为规格

## Scope / 范围

This is the stable contract for the project-aware terminal drawer. Historical experiments and abandoned approaches remain in `docs/terminal_viewport_iteration_notes.md`.

本文是项目感知 Terminal Drawer 的稳定契约。历史实验和放弃方案保留在 `docs/terminal_viewport_iteration_notes.md`。

## Ownership / 所有权

- Rust controller, session, and PTY state: `src/terminal/mod.rs`, `session.rs`, and `render.rs`.
- Pure keyboard mapping, diagnostic formatting, and refresh throttling policy:
  `src/terminal/input.rs`, `diagnostics.rs`, and `refresh.rs`.
- Root bridge and docking: `ui/main.slint`.
- Drawer presentation: `ui/components/drawer/terminal_drawer.slint`.
- Project entry point: `src/project/mod.rs`.
- Workflow entry point: root shell-drawer callback and
  `GitHubPageScrollActions`.

- Rust controller、Session 与 PTY 状态：`src/terminal/mod.rs`、`session.rs`、`render.rs`。
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
10. PTY reads and process waits never run on the Slint event loop.
11. Terminal chrome reuses shared GitHub Button/IconButton focus, tooltip,
    selected, disabled, and loading contracts. Terminal cells keep the
    documented monospace font and PTY-provided ANSI colors.
12. Session shutdown terminates the PTY process group, waits for graceful exit,
    escalates when required, and reaps the child. Descendant commands must not
    survive a project switch as orphan processes.

1. `open_terminal_for_project` 是 Project 页面唯一标准入口。
2. 同项目重开保留 PTY/Session 内容和 cwd。
3. 切换项目时重置输出/输入展示、退出全屏、恢复 Dock 高度并进入新仓库目录。
4. Live Terminal 使用由可见内容区计算出的固定 viewport。
5. 只有收到有效 viewport 更新后才修改 PTY rows/columns；禁止通过少报 rows 伪造底部可见。
6. Live 输出应 follow-bottom，但不能裁剪有语义的终端内容。
7. 关闭 Drawer 时必须清除 fullscreen，保证 FAB 恢复。
8. Fullscreen 隐藏页面 ScrollView，占据标题/面包屑以下工作区。
9. Docked 宽度和 padding 与页面内容区对齐。
10. PTY 读取和进程等待不得运行在 Slint event loop。
11. Terminal 外壳复用共享 GitHub Button/IconButton 的焦点、Tooltip、选中、
    禁用与 loading 契约。终端单元格继续使用约定的等宽字体和 PTY 提供的 ANSI
    色。
12. Session 关闭时终止 PTY 进程组，等待正常退出，必要时升级终止并回收 Child；
    切换项目后子命令不得作为孤儿进程继续运行。

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
- A -> B -> A project switching selects the correct session and cwd.
- Resize narrow/wide repeatedly without stale rows, overlapping output, or feedback loops.
- Docked/fullscreen transitions preserve output and restore FAB/page layout.
- Long output follows bottom while manual scrollback remains usable.
- Chinese and wide glyphs keep column alignment with the configured monospace font.
- Application shutdown terminates or detaches PTY processes according to shutdown policy.
