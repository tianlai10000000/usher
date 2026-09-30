# Coding Agent Guide / Coding Agent 指南

This file applies to the entire repository. It is intentionally bilingual so both human contributors and coding agents share one source of operational rules.

本文适用于整个仓库。它采用中英双语，确保开发者和 coding agent 使用同一套操作规则。

## Mission / 目标

Keep Usher a responsive desktop application with explicit module ownership, event-loop-safe UI updates, project-scoped state, and centralized cross-cutting infrastructure.

保持 Usher 的桌面响应速度、清晰模块边界、event-loop 安全 UI 更新、项目级状态隔离，以及统一的横切基础设施。

## Read before editing / 修改前必读

1. Read the relevant README and architecture language version.
2. Read `docs/specs/README.md` and any feature spec named there.
3. Inspect `git status --short`; preserve unrelated user changes.
4. Locate both Rust callback bindings and their Slint declaration before changing UI-facing behavior.
5. For terminal work, read `docs/specs/terminal.md`; use the long iteration notes only as history.
6. Before adding or changing any Tooltip, read [Tooltip implementation contract](docs/specs/tooltips.md). Follow its factory-isolation rules and acceptance matrix; a build-only check is insufficient.
7. Before any UI change, read [Internationalization](docs/specs/i18n.md). Every new or changed user-facing string must use the shared semantic catalog, and dynamic Rust projections must refresh immediately when the runtime locale changes.
8. For variable-height lists or unstable scrollbars, read [Exact virtual scrolling](docs/specs/virtual-scrolling.md). Reuse exact extents, shared virtualization and the worker/layout acceptance matrix; do not patch estimated ranges by freezing the thumb.
9. Before changing multiline `Text`, direct `TextInput`, `StyledText`, `TextEdit`, or any wrap/line-height measurement, read [Multiline text layout](docs/specs/text-layout.md). Use the supported native line-height API and keep every fallback, worker measurement and virtual extent on the same layout contract.

1. 阅读对应语言的 README 和架构文档。
2. 阅读 `docs/specs/README.md` 以及对应功能 spec。
3. 先检查 `git status --short`，保留用户无关改动。
4. 修改 UI 行为前，同时定位 Rust callback 绑定和 Slint 声明。
5. Terminal 改动先读 `docs/specs/terminal.md`，长篇迭代记录只作为历史参考。
6. **新增或修改任何 Tooltip 前，必须先阅读 [Tooltip 实现规范](docs/specs/tooltips.md)**，遵守组件工厂隔离规则并完成对应验收矩阵，不能仅凭编译通过交付。
7. **任何 UI 改动前都必须阅读 [国际化规范](docs/specs/i18n.md)**。所有新增或修改的用户可见文案必须进入共享语义 catalog；Rust 动态投影必须在运行时切换语言后立即刷新。
8. 可变行高列表或滚动条不稳定问题先读 [精确虚拟滚动](docs/specs/virtual-scrolling.md)，复用精确 extent、共享虚拟化及 Worker/布局验收矩阵，禁止仅冻结滑块来掩盖估算范围问题。
9. 修改多行 `Text`、直接 `TextInput`、`StyledText`、`TextEdit` 或任何换行/行高测量前，先读 [多行文本排版规范](docs/specs/text-layout.md)。优先使用已支持的原生行高 API，并保证所有 fallback、Worker 测量和虚拟 extent 遵循同一布局契约。

## Canonical references / 主文档

- `README.md` and `README.zh.md`: product scope, setup, support status.
- `docs/architecture.md` and `docs/architecture.zh.md`: ownership and data flow.
- `docs/specs/`: stable behavior contracts.
- `docs/terminal_viewport_iteration_notes.md`: historical investigation, not current truth.
- `material-1.18.1/`: byte-for-byte upstream `ui-libraries/material/src` from
  Slint v1.18.1 (`372cf0ee5577c3dfec309a45e7b778ba4e81b734`); treat it as
  replace-only third-party code.

## Module ownership / 模块边界

- Keep `src/lib.rs` as the composition root. Put business logic in its feature module.
- Keep I/O and process work out of `.slint` files.
- Keep page layout in `ui/views/` and reusable UI in `ui/components/`.
- Keep shared async work on `src/app_runtime.rs`.
- Keep all application-owned HTTP client creation and interception in `src/http/mod.rs`.
- Keep project state in `src/project/`, workflow state in `src/workflow/state.rs`, and terminal session state in `src/terminal/`.

- `src/lib.rs` 只做装配，业务逻辑进入对应 feature module。
- `.slint` 文件不承担 IO 和进程操作。
- 页面布局放 `ui/views/`，复用 UI 放 `ui/components/`。
- 异步任务统一使用 `src/app_runtime.rs`。
- 应用级 HTTP client 创建和拦截统一放在 `src/http/mod.rs`。
- Project、Workflow、Terminal 状态分别归其模块所有。

## Non-negotiable invariants / 硬性约束

### Slint and threading / Slint 与线程

- Only touch Slint components/globals on the Slint event loop.
- From worker threads or Tokio tasks, use `slint::invoke_from_event_loop` with a weak window handle.
- Never block the event loop with HTTP, Git, PTY reads, filesystem scans, or sleeps.
- Avoid creating a Tokio runtime inside feature code.

- 只能在 Slint event loop 操作组件和 global。
- 后台线程/Tokio task 必须通过 weak window + `slint::invoke_from_event_loop` 回写 UI。
- 禁止用 HTTP、Git、PTY 读取、文件扫描或 sleep 阻塞 UI 线程。
- Feature 内禁止自行创建 Tokio runtime。

### HTTP / 网络请求

- Do not call `reqwest::Client::new`, `reqwest::blocking::get`, or build another shared client outside `src/http/mod.rs`.
- Build requests with the shared module and execute through `send`, `send_json`, or `send_blocking`.
- Give every request a meaningful `RequestOptions.operation`.
- Background polling, probes, prefetching, retries, and fallback attempts must use silent feedback.
- User-triggered terminal failures may select toast/notification feedback, but avoid duplicating an existing feature-owned error UI.
- Never log authorization headers, tokens, complete sensitive payloads, or unbounded response bodies.

- 禁止在 `src/http/mod.rs` 外调用 `reqwest::Client::new`、`reqwest::blocking::get` 或创建其他共享 client。
- 使用统一模块构建请求，并通过 `send`、`send_json`、`send_blocking` 执行。
- 每个请求必须设置有意义的 `RequestOptions.operation`。
- 后台轮询、探测、预取、重试和 fallback 必须静默。
- 用户主动操作可以启用 Toast/Notification，但不要与 feature 自有错误 UI 重复提示。
- 不得记录鉴权 header、Token、完整敏感 payload 或无限长响应正文。

### State and lifecycle / 状态与生命周期

- All state shared by multiple routes/pages/status surfaces/background publishers must use the local `crates/usher-state-store` abstraction and a feature-owned facade. Mutations dispatch typed Actions; changed commits auto-notify subscribers; UI bridges never depend on a concrete Store engine.
- Durable shared state must attach automatic commit persistence on a worker. Do not scatter manual save calls across action creators or perform persistence/reducer work on the Slint event loop.
- Scope workflow state by project; never reuse one project's running/log state for another.
- Preserve terminal content only when reopening the same project; switching project must reset session presentation and working directory.
- Long operations need cancellation or duplicate-start protection.
- Keep shutdown registration and priorities intact when adding persistent threads/processes.

- 所有由多个路由/页面/状态展示/后台发布方共享的状态必须使用本地 `crates/usher-state-store` 抽象与 Feature 自有门面。变更统一 dispatch 强类型 Action；有效 commit 自动通知订阅者；UI bridge 禁止依赖具体 Store 引擎。
- 需要持久化的共享状态必须通过 Worker 挂接自动 commit 持久化，禁止把手工 save 分散到各 Action creator，也禁止在 Slint event loop 执行持久化或 reducer 工作。
- Workflow 状态必须按项目隔离。
- 只有同项目重开才保留 Terminal；切换项目必须重置会话展示和 cwd。
- 长任务必须支持取消或防止重复启动。
- 新增长驻线程/进程时必须接入关闭顺序和优先级。

### UI and performance / UI 与性能

- Keep hover hit targets mounted; change opacity/state instead of conditionally creating interactive nodes.
- Do not rebuild large models during scrolling or resize interaction.
- Mount full-window overlays at the root window layer.
- Reuse existing FAB, card-action, modal, toast, notification, and terminal components before adding variants.
- Any new root property/callback must have a consistent name in `ui/main.slint` and Rust.
- Every UI change must preserve full multi-language behavior: no hard-coded user-facing prose, no persisted translated strings, and no stale Rust-projected copy after a locale switch. Validate at least the source locale and one packaged non-source locale.
- For multiline native Slint `Text` and direct `TextInput`, prefer the official `line-height-factor`. Never simulate inter-line leading with padding, injected blank lines or invisible glyphs, or a fixed-height wrapper. On Slint 1.18, `StyledText` and the standard `TextEdit` wrapper require a documented exception and synchronized paint/measurement/virtual-extent metrics.

- Hover 命中节点应常驻，用 opacity/state 控制。
- 滚动和 resize 期间不要重建大型 model。
- 全窗口遮罩必须挂在根窗口层。
- 新建变体前先复用现有 FAB、卡片操作、Modal、Toast、Notification、Terminal 组件。
- 根 property/callback 在 `ui/main.slint` 和 Rust 中必须命名一致。
- 每次 UI 改动都必须保持完整多语言行为：不得硬编码用户可见文案，不得持久化翻译后的字符串，运行时切换语言后 Rust 投影文案不得残留旧语言；至少验证源语言和一种内置非源语言。
- 多行原生 Slint `Text` 和直接 `TextInput` 应优先使用官方 `line-height-factor`。禁止用 padding、注入空行/不可见字符或固定高度容器伪造行间距。Slint 1.18 下的 `StyledText` 与标准 `TextEdit` 包装仍须记录例外，并同步绘制、测量和虚拟 extent 指标。

### Vendored Material / 内置 Material

- Prefer `@material` imports.
- Never edit `material-1.18.1/` or any other vendored Slint Material UI source.
- Implement every application behavior and visual adaptation in application-owned `ui/` code so Material UI can be upgraded by replacement.
- Keep compatibility aliases, input-policy changes, and other upstream adaptations in application-owned wrappers; a Material upgrade must be an upstream directory replacement, not a patch rebase.
- New code must not import private files from the vendored implementation path; use `@material` and project-owned wrappers.

- 优先通过 `@material` 导入。
- 绝对禁止修改 `material-1.18.1/` 或任何其他 vendored Slint Material UI 源码。
- 所有应用行为和视觉适配都必须在项目自有 `ui/` 代码中实现，保证 Material UI 可直接替换升级。
- 兼容别名、输入策略及其他上游适配必须进入项目自有 wrapper；Material 升级必须直接替换上游目录，禁止重新向 vendor 打补丁。
- 新代码禁止引用 vendored 实现路径中的私有文件；必须使用 `@material` 和项目自有包装组件。

## Documentation discipline / 文档同步

- If behavior or ownership changes, update both architecture language versions in the same change.
- If a stable invariant changes, update the relevant spec and its index.
- Do not put ongoing debugging diaries into architecture docs; place them in a clearly marked historical note.
- Keep README commands executable and supported by the current dependency graph.
- When code and prose disagree, code plus tests determine current behavior; then fix the prose.

- 行为或职责变化时，同步更新中英文架构文档。
- 稳定不变量变化时，同步更新对应 spec 和索引。
- 调试过程不要继续堆进架构文档，应放入明确标记的历史记录。
- README 命令必须与当前依赖和支持目标一致。
- 代码与文档冲突时，以代码和测试确认当前行为，再修正文档。

## Validation / 验证

Run the smallest relevant checks during iteration, then before handoff run:

```sh
cargo fmt --all
cargo check --locked
cargo test --lib --locked
git diff --check
```

Additional expectations:

- Every optimization batch requires self-tests and a recorded baseline/candidate A/B comparison before handoff. Tooling-only changes compare gate behavior using positive and negative fixtures; do not claim runtime gains. Runtime/performance changes require controlled Release A/B with raw evidence and the repository's acceptance thresholds. Record failures, unexecuted checks, and the adopt/reject decision; do not advance a failed candidate into the next batch.
- 每个优化批次交付前必须完成自测并记录基线/候选 A/B 对照。纯工具改动使用正反例比较检查行为，不宣称运行时收益；运行时/性能改动必须执行受控 Release A/B，保留原始证据并遵守仓库验收门槛。记录失败、未执行检查及采用/拒绝结论，失败候选不得进入下一批基线。
- HTTP changes: add unit tests for classification/options/interceptor behavior where practical; search for direct reqwest client creation.
- Slint changes: compile the full root UI and manually inspect affected route/layout behavior.
- Terminal changes: follow the regression matrix in `docs/specs/terminal.md`.
- Release changes: run the relevant script under `scripts/` or explain why it was not run.

Known baseline: full `cargo test --locked` currently fails one doctest in `src/logger/debug_logger.rs`. Unit tests must still pass, and new failures must be reported separately.

已知基线：完整 `cargo test --locked` 当前会失败于 `src/logger/debug_logger.rs` 的一个 doctest。单元测试仍必须通过，新增失败必须单独说明。

## Handoff checklist / 交付清单

- State what changed and why.
- List checks run and their exact outcome.
- Call out remaining known failures or manual checks.
- Mention preserved unrelated working-tree changes.
- Do not claim a spec is current unless its referenced code paths were inspected.

- 说明改动内容和原因。
- 列出实际运行的检查及结果。
- 标明剩余已知失败和未执行的手工检查。
- 说明保留的用户无关工作区改动。
- 未核对 spec 对应代码前，不要声称 spec 已是最新。
