# PR Conflicts 交互修复验收（2026-09-29）

本次范围：文件复制缺少反馈、详情导航 Conflicts 入口偏上/间距不足、代码滚动崩溃。
保留已有 PR 列表和冲突首批实现，以及工作区其他未提交改动。

## 崩溃证据与修复

使用现有 `USHER_PR_CONFLICTS_FIXTURE=1` 离线 fixture，在原生 macOS mock
中先上滚，再连续下滚/反向上滚，修复前进程退出码 134。
隔离日志：`/private/tmp/usher-conflicts-scroll-before/default/run-0/logs/2026-09-29/usher_tracing.log`。
首个 panic 为 Slint `properties.rs:641` 的 `Recursion detected`；堆栈包含
`FlickableDataInner::flick_limits`、`ConstantDeceleration` 和
`PrReviewConflicts` 的 `PrConflictWindow` 属性绑定。

原先 `content-height -> window.total -> content-y -> flick_limits -> content-height`
形成递归。现在文档总高度通过独立的 `layout(row_count, 0, 1)` 获取精确 extent，
可见窗口继续使用滚动位置。仍使用共享 VirtualList 和有界惰性源码模型。

复制继续在 Worker 写入剪贴板，成功回写前检查会话和文件选择代际；使用共享
`GitHubCopyButton` 的绿色勾、两秒确认 Tooltip 和中英文文案。失败只显示本地化
错误；切文件清除旧反馈。共享组件新增异步成功确认入口，同步调用方行为不变。

Conflicts 导航入口放入整行高的容器并垂直居中，两侧各留 12px 空间。
原生窄窗发现 800px 窗口展开 240px 应用侧栏后，英文入口会超出可见边界；
根据 Tab、计数和冲突标签的实际测量宽度，在空间不足时将入口移到导航第二行。
Tab 行显式固定在导航顶部，避免默认居中使选中下划线与第二行入口重叠。

## 自动验证

- Conflicts 专项测试 6 项通过，包括滚动/选区、复制成功/失败/过期完成、
  中英文 800/1200/1800px 导航几何和现有 gutter 布局。
- `cargo fmt --all`、`cargo check --locked`、UI policy、i18n catalog 与
  `git diff --check` 已通过；`cargo build --locked --features test-fixtures` 成功。
- 最终 `cargo test --lib --locked`：691 通过、0 失败、2 忽略。
- `pr_review_status_bar_ui` 2 项、`pr_list_status_layout_ui` 1 项全部通过。
- 首次全库测试：681 通过、10 失败、2 忽略。10 个失败均为 HTTP/Logo fixture
  监听本地端口时收到沙箱 `PermissionDenied`；允许本地监听后重跑全部通过。
- 最终日志：`/private/tmp/usher-conflicts-wrap-lib.log`、
  `/private/tmp/usher-conflicts-wrap-check.log`、`/private/tmp/usher-conflicts-wrap-ui.log`。
  构建仍有已有 `hooks::events_after` 未使用警告。

## 原生验收

已用原生 macOS mock 验证：此前导致退出的上滚/下滚/反向操作、多轮上下到底、
文本/非文本文件切换、冲突块导航、横向滚动、两秒复制提示及文件切换清理。
修复后的 mock 未出现 panic。中文 800px 窗口中同一页面运行时切换为英文后，
复制提示也立即采用英文。
最后的原生 mock 需在桌面会话中运行；沙箱内 GUI 启动曾以 134 退出且无应用日志，
在所需权限下重启后完成截图验收，日志没有 `panic` 或 `Recursion detected`。
临时测试实例已关闭。

- [英文复制成功](designs/pr-conflicts/validation/fix-copy-en.png)
- [中文窄窗复制成功](designs/pr-conflicts/validation/fix-copy-zh.png)
- [运行时切换语言后复制](designs/pr-conflicts/validation/fix-copy-locale-switch.png)
- [多轮滚动与文件切换后的末尾](designs/pr-conflicts/validation/fix-scroll-en.png)
- [宽窗导航入口对齐](designs/pr-conflicts/validation/fix-tabs-en.png)
- [英文窄窗入口换行](designs/pr-conflicts/validation/fix-tabs-narrow-en.png)
- [中文窄窗入口同排](designs/pr-conflicts/validation/fix-tabs-narrow-zh.png)

最终英文窄窗截图确认 Tab 下划线与第二行入口不重叠。没有执行 Windows 原生验收或制造系统级
剪贴板失败；失败和过期完成由单元测试覆盖。
