# PR Conflicts 导航与分栏验收（2026-09-29）

本次修复截图中的重复进入触发加载、文件栏不能调宽、Conflicts 入口换行及字体不一致。
本记录中的单行导航取代上一轮验收记录的窄窗第二行方案。

## 实现

- Review 的 open callback 在冲突工作区已激活时立即返回，不改变返回来源。
  ConflictController 同时对当前仓库、PR 和分支上下文做幂等检查；显式 refresh 保持独立。
- 冲突文件栏采用普通 PR 文件树的拖动增量、取消恢复和 9px 命中区域规则。
  偏好宽度独立保存在冲突视图，收起/展开保留，窗口变窄时只约束实际宽度。
- Conflicts 复用 GitHubPullRequestTabButton，保持相同的 14px 字号、前景色和 16px 图标。
  四个入口保持 48px 单行；不足时内部间距由 8px 缩到 4px，仍保留文字、图标和计数。
  极端超宽内容可水平滚动。状态颜色仅作用于冲突图标。

## 自动检查

- `cargo fmt --all`：通过。
- `scripts/ui-check.sh`：完整根 UI 通过。
- `cargo build --locked --features test-fixtures`：通过。
- `cargo check --locked`：通过。
- `cargo test --lib --locked`：最终 693 通过、0 失败、2 忽略。
- `cargo test --locked --test pr_review_status_bar_ui --test pr_list_status_layout_ui`：3 通过。
- `cargo test -p usher-i18n --locked`：7 通过。
- `scripts/check-ui-policy.sh`、`scripts/check-i18n.sh`、`git diff --check`：通过。

新增会话重复进入和真实指针分隔线拖动测试，导航测试改为中英文
800/1200/1800px 以及 800px 展开应用侧栏时同排显示、边界和间距断言。
拖动测试覆盖 260→380px、最小/最大值、收起恢复及窗口缩小。
已有滚动、复制及行号区域测试继续通过。

日志位于 `/private/tmp/usher-conflicts-nav-lib-final.log`、
`/private/tmp/usher-conflicts-nav-tests.log` 和 `/private/tmp/usher-conflicts-nav-check.log`。
保留原有 Hooks 未使用方法与测试未使用 import 警告；本次未调整这些无关代码。

## 原生检查

复用 `USHER_PR_CONFLICTS_FIXTURE=1`、`test-fixtures` 和隔离的 offline 状态目录，
在 macOS 原生构建中检查：

- 1200px 窗口将冲突文件栏从 260px 拖到 380px，源码水平边界同步移动；
  收起再展开保持 380px，垂直源码位置不因分栏改变。
- 滚动到第 29 行后连续两次点击标题栏 Conflicts；当前文件、滚动位置、分栏宽度均未改变。
- 800px 窗口展开应用侧栏，英文四个入口完全可见且保持单行。
- 同一页面运行时从英文切换中文，文案和宽度测量立即更新，四个入口仍同排。
- 临时 mock 均已关闭；没有替换用户安装的应用。

截图：

- [分隔线拖动后的冲突工作区](designs/pr-conflicts/validation/nav-splitter-wide.png)
- [重复点击前](designs/pr-conflicts/validation/nav-reentry-before.png)
- [重复点击后](designs/pr-conflicts/validation/nav-reentry-after.png)
- [宽窗导航](designs/pr-conflicts/validation/nav-tabs-wide-en.png)
- [英文窄窗导航](designs/pr-conflicts/validation/nav-tabs-narrow-en.png)
- [运行时切换中文后的窄窗导航](designs/pr-conflicts/validation/nav-tabs-narrow-zh.png)

mock 仅提供冲突快照，因此返回后的普通 Diff 为空；导航截图用于验证几何和样式。
没有执行 Windows 原生验收或平台线上接口验收。本次没有修改平台取数逻辑。
保留工作区已有 PR 冲突/列表实现及其他未提交改动。

## Conflicts 文件数徽标初版验收（首次进入行为已由下节修正）

Conflicts Tab 复用其他 Tab 的数量徽标，数值来自成功读取的冲突快照，
统计文件数（包含非文本文件），不统计冲突块数。未知时隐藏徽标，确认空快照时显示 0。
从冲突工作区返回普通 Review 后保留数量；刷新、切换 PR/项目、离开 Review 或平台失效时清除。
首次读取冲突文件后才显示数量，本次没有增加平台请求或预先执行 Git 分析。

验证结果：

- `cargo fmt --all`、`cargo check --locked`、`git diff --check`：通过。
- `scripts/ui-check.sh`：完整根 UI 编译通过。
- `cargo build --locked --features test-fixtures`：通过。
- `cargo test --lib --locked`：694 通过、0 失败、2 忽略。
- `cargo test --locked --test pr_review_status_bar_ui --test pr_list_status_layout_ui`：3 通过。
- `scripts/check-ui-policy.sh`、`scripts/check-i18n.sh`：通过。
- 文件数生命周期测试覆盖未知值、含非文本文件的快照、返回保留、退出清除和空快照。
- 两位数徽标 17 的几何测试覆盖中英文 800/1200/1800px，以及 800px 展开应用侧栏。
- macOS 原生 mock：800px 窗口展开应用侧栏，加载两个冲突文件后返回 Review，显示 `Conflicts 2`。
  运行时切换为中文后显示 `冲突 2`；四个 Tab 始终同排，徽标没有裁切或重叠。
- 临时 mock 正常退出（exit 0），未替换已安装应用。Windows 原生与平台线上接口未重新验收。

截图：[英文窄窗](designs/pr-conflicts/validation/count-tabs-narrow-en.png)、
[运行时切换中文](designs/pr-conflicts/validation/count-tabs-narrow-zh.png)。
mock 只提供冲突快照，普通 Diff 为空，不用于验证普通变更文件数量。
日志位于 `/private/tmp/usher-conflict-count-{build,check,lib,integration,i18n,en}.log`。
保留工作区其他未提交改动；已知 Hooks 未使用方法和测试 import 警告未调整。

## 首次进入 Review 的数量加载修正

初版只在打开冲突工作区之后得到数量，遗漏了普通 Review 首次进入的触发。
现在普通 Review 打开和路由历史重新进入都会启动可取消的后台读取。
读取复用现有 Provider adapter 及有界 Git fallback，HTTP 反馈保持静默；
完成后只投影数量和状态，不挂载冲突文件/源码模型，也不切换当前 Tab。
随后打开 Conflicts 会复用正在读取或已完成的结果。离开 Review 或重置 PR
立即取消任务，并通过代际检查拒绝晚到的回写。读取失败仍隐藏未知数量，不伪造 0。

源码版本验证命令：

```sh
cargo build --locked --features test-fixtures
USHER_BENCHMARK_OFFLINE=1 \
USHER_BENCHMARK_STATE_ROOT=/private/tmp/usher-conflict-first-entry \
USHER_PR_CONFLICTS_FIXTURE=1 \
USHER_PR_CONFLICTS_SCENE=review \
USHER_PR_CONFLICTS_LOCALE=en \
USHER_PR_CONFLICTS_SWITCH_LOCALE=zh-CN \
USHER_PR_CONFLICTS_WIDTH=800 \
./target/debug/usher
```

macOS UI 自动化实际运行的是上述源码产物复制到临时 bundle
`/private/tmp/Usher Conflicts.app/Contents/MacOS/usher` 的版本，未启动系统安装的 Usher。
mock 通过异步 loader 提供两个冲突文件，直接进入普通 Review，没有预先打开冲突工作区。
英文截图见 [首次进入 Review](designs/pr-conflicts/validation/count-first-entry-en.png)。
同一实例运行时切换中文后，数量和单行布局保持正确，见
[中文截图](designs/pr-conflicts/validation/count-first-entry-zh.png)。临时实例正常退出（exit 0）。

`cargo fmt --all`、`cargo check --locked`、fixture 构建、UI policy、i18n catalog 和
`git diff --check` 通过。`cargo test --lib --locked` 为 697 通过、0 失败、2 忽略。
`cargo test --locked --test pr_review_status_bar_ui --test pr_list_status_layout_ui`
为 3 通过、0 失败。
新增回归覆盖普通入口/历史入口、后台完成不挂载工作区、未完成与已完成读取复用、
重置后旧成功/失败回写拒绝。保留原有无关改动与 Hooks 未使用方法警告。
日志位于 `/private/tmp/usher-conflict-prefetch-{build,check,lib,ui}.log`。
未执行线上平台和 Windows 原生验收。
