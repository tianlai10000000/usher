# PR 冲突文件列表与总增删统计验收

日期：2026-09-29。

## 交互

冲突文件栏参考 GitHub 冲突编辑器的平铺列表，而非普通 Files changed 的目录树：
文件名为主行，路径为次行，右侧独立保留红色冲突图标的位置。行高固定为 56px，
长路径省略，完整路径使用现有根层 Tooltip；重命名保留两侧路径。
点击、Enter、Space 仍选择同一文件，激活时关闭提示。
无文本预览、没有冲突标记块的文件同样属于未解决冲突，不显示绿色已解决状态。
在线编辑、标记已解决与提交仍属于后续迭代。

Tab 行最右端固定显示 PR 总增删行数与现有五格比例条，使用共享组件的实际宽度；
空间不足时左侧 Tab 收入 More，不再隐藏统计。总数来自缓存的整个 PR，
选择 Commit 比较范围不会将其替换为范围内的行数。
数字沿用现有共享组件格式，本次未新增千位分隔格式。

参考：[GitHub 官方冲突操作说明](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/resolving-a-merge-conflict-on-github)、
[官方冲突列表截图](https://docs.github.com/assets/cb-198808/images/help/pull_requests/merge-conflict-commit-changes.png)，
以及用户提供的深色、浅色 PR Tab 行截图。

## 检查结果

| 检查 | 结果 |
| --- | --- |
| `cargo fmt --all` | 通过 |
| `cargo check --locked` | 通过；保留已有 Hooks `events_after` 未使用警告 |
| `cargo test --lib --locked` | 699 通过，0 失败，2 忽略 |
| `cargo test --locked --test underline_nav_ui --test pr_review_status_bar_ui` | 3 通过 |
| `cargo build --locked --features ui-components` | 通过；最终 mock 组合参数修正后再次构建通过 |
| 完整根 Slint `slint-viewer --check` | 通过，包含 PR Review、AI Review 与组件演练包 |
| `scripts/check-ui-policy.sh` | 通过 |
| `scripts/check-i18n.sh` | 通过 |
| `git diff --check` | 通过 |

新增/扩展回归覆盖中英文文件名/路径/状态列的几何边界、根目录和中文路径、
非文本冲突、点击及 Enter/Space 选择，800/1200/1800px 与主侧栏组合下的
大位数统计边界，以及 Commit 比较范围切换时保留 PR 总数。

## 原生截图

全部使用源码构建的 `target/debug/usher`，复制到临时
`/private/tmp/Usher Conflicts.app` 并临时签名；应用标识为
`com.usher.conflicts-validation`。没有启动系统安装版 Usher。
使用隔离离线状态，不访问真实 Provider，也不修改真实仓库。

- [浅色宽窗 PR 总增删统计](designs/pr-conflicts/validation/pr-totals-light-wide.png)
- [切换到中文并返回 Review](designs/pr-conflicts/validation/pr-totals-light-zh.png)
- [英文冲突文件名、路径与右侧图标](designs/pr-conflicts/validation/conflict-files-light-en.png)
- [长路径根层提示](designs/pr-conflicts/validation/conflict-files-path-tooltip.png)
- [中文无文本冲突与重命名路径](designs/pr-conflicts/validation/conflict-files-nontext-zh.png)
- [220px 最小文件栏宽度](designs/pr-conflicts/validation/conflict-files-min-width-zh.png)
- [深色 800px 窄窗固定统计](designs/pr-conflicts/validation/pr-totals-dark-narrow.png)
- [More 与 99+ 徽标](designs/pr-conflicts/validation/pr-totals-dark-more-99plus.png)
- [最终源码版：128 个非文本冲突滚动后选择文件](designs/pr-conflicts/validation/conflict-files-dark-scrolled.png)

原生检查包含运行时英文切中文、点击后 Tooltip 消失、返回 Review、拖动文件栏、
Enter 选择、代码上下滚动及大量文件滚动后的选择。没有复测独立 Settings/Messages
factory、Dialog Tooltip 全矩阵或 20 轮内存增长；本次未改动这些共享宿主。
Windows/Linux 原生、线上 Provider 和已知 debug-logger doctest 未运行。
本次为交互布局改动，不宣称性能改进。
工作区已有的 Hooks、Settings 和此前 PR 改动均保留。

## 复现

在仓库根目录复用现有的 mock 启动方式：

```sh
env USHER_BENCHMARK_OFFLINE=1 \
  USHER_BENCHMARK_STATE_ROOT=/private/tmp/usher-conflict-files-manual \
  USHER_PR_CONFLICTS_FIXTURE=1 \
  USHER_PR_CONFLICTS_FILES=long-paths \
  USHER_PR_CONFLICTS_SCENE=review \
  USHER_PR_CONFLICTS_LOCALE=zh-CN \
  USHER_PR_CONFLICTS_WIDTH=1200 \
  cargo run --locked --features ui-components
```

进入 Review 后点击冲突 Tab。省略 `USHER_PR_CONFLICTS_SCENE` 可直接打开冲突页。
增加 `USHER_PR_CONFLICTS_DARK=1`、`USHER_PR_CONFLICTS_WIDTH=800` 检查深色窄窗；
增加 `USHER_PR_CONFLICTS_COUNT=128` 检查 99+ 和大量文件滚动，该参数覆盖长路径
样例列表并生成非文本冲突。`USHER_PR_CONFLICTS_SWITCH_LOCALE=en` 在 60 秒后
切换语言。此命令通过源码构建运行，不使用系统安装应用。

## 冲突检查动画补充验收

`GitHubConflictStatus` 的 checking 状态复用 `GitHubSpinner`，保持原有 14px
图标占位和文字对齐；状态结束后卸载动画并恢复对应状态图标。PR 列表中的同一
共享状态组件同步生效。本次没有新增用户可见文案。

使用 `cargo build --locked --features test-fixtures` 构建的源码应用，设置
`USHER_PR_CONFLICTS_SCENE=review` 和 `USHER_PR_CONFLICTS_DELAY_MS=60000`，
进入冲突页后验证英文加载状态、运行时中文切换、加载完成和再次刷新。
中文两帧顶部圆弧角度不同，图标占位和文字位置一致；完成后恢复红色冲突图标。

- [英文加载状态](designs/pr-conflicts/validation/spinner-en-a.png)
- [中文动画第一帧](designs/pr-conflicts/validation/spinner-zh-a.png)
- [中文动画第二帧](designs/pr-conflicts/validation/spinner-zh-b.png)
- [加载完成](designs/pr-conflicts/validation/spinner-complete-zh.png)

本次重新执行 `cargo fmt --all`、`cargo check --locked`、
`cargo test --lib --locked`（699 通过、2 忽略）、fixture 构建、完整根 Slint 检查、
UI policy、i18n 和 diff 空白检查，均通过。保留已有未使用代码警告。
未重新验证线上 Provider、Windows/Linux 原生或已知 debug-logger doctest。

## Tab 底线修复

原 PR 导航组件在自身末尾绘制 1px 底线，页面又在下一行添加了 1px
通栏分隔线，导致导航下方为 2px、尾部统计下方为 1px。
现在 PR Tab 组合行在内部底边统一绘制一条 `border-muted` 底线，
嵌套导航通过 `show-bottom-border: false` 关闭重复绘制。选中项的 2px
强调线覆盖同一底边；其他共享导航默认仍绘制自己的底线。

该关系已对照 [Primer 官方共享 Tab 样式](https://github.com/primer/react/blob/main/packages/react/src/internal/components/UnderlineTabbedInterface.module.css)：
容器使用 `inset 0 -1px var(--borderColor-muted)`，选中项的底部伪元素高 2px。
项目用 Slint 矩形表达相同的内嵌绘制关系，没有在下一行追加灰线。

源码构建的离线 mock 原生验收：

- [1200px 英文浅色](designs/pr-conflicts/validation/tab-baseline-wide-en.jpg)
- [800px 中文深色，选中项收纳到 More](designs/pr-conflicts/validation/tab-baseline-narrow-zh.jpg)
- [Tab 行局部](designs/pr-conflicts/validation/tab-baseline-detail.png)

Retina 2x 截图中，宽窗 x=900/1800/2250 的底线均为 y=372..373，
窄窗 x=700/1000/1500 的底线均为 y=436..437：导航区与统计区各取样点
颜色一致，均仅占两行物理像素，即 1px 逻辑高度。More 展开与 Escape 关闭正常。

本次 `cargo fmt --all`、最终 `cargo check --locked`、fixture 构建、完整根
Slint 检查、UI policy、i18n 和 `git diff --check` 通过。完整库测试在解除
本地监听/进程操作的沙箱限制后为 698 通过、1 失败、2 忽略：
`hooks::tests::shutdown_cancels_running_and_queued_work_and_prevents_descendant_effects`
在 `src/hooks/tests.rs:1115` 失败；相同测试单独重跑通过（1 通过）。
完整套件未再次重跑，不能将其记录为全绿；本次没有更改 Hooks 代码。
