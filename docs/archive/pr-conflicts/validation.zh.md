# PR Conflicts 历史验收档案

本档案按原日期与实施阶段汇集 2026-09-28 至 2026-09-29 的验收记录。
保留当时的命令、成功与失败、未执行范围和截图证据；本次文档合并没有新增手工或构建验收。
当前行为契约见 [PR 冲突查看规格](../../specs/pr-conflicts.md)。

- [2026-09-28：首批只读离线验收](#offline-validation-20260928)
- [2026-09-29：滚动、复制与入口交互修复](#interaction-fixes-20260929)
- [2026-09-29：导航、分栏与首次进入数量加载](#navigation-and-counts-20260929)
- [2026-09-29：文件列表、总统计、加载动画与 Tab 底线](#files-and-totals-20260929)

<a id="offline-validation-20260928"></a>

<!-- archive-section:offline-validation-20260928:begin -->
## PR Conflicts 离线验收记录

日期：2026-09-28。第一批只读代码与离线验收完成；各平台线上权限、部署能力和真实仓库验收尚未执行。线上未验证项不视为通过。

列表布局随后经用户反馈修正；本页旧列表截图仅作为历史记录，不能作为当前布局验收。
当前结果见 [PR 列表布局验证](../ui/pr-list-layout-validation.zh.md)。

### 原生截图

运行的是 `cargo build --locked --features test-fixtures` 生成的 macOS 原生程序，使用独立应用包与 `/private/tmp/usher-conflicts-qa-*` 状态目录，开启 `USHER_BENCHMARK_OFFLINE=1`。数据经过实际 parser、后台文件准备、Slint bridge 与正式组件。截图不是 HTML 复刻或设计稿。

| 场景 | 检查结果 | 截图 |
| --- | --- | --- |
| 英文 1200 x 900 | 两层工具栏居中；文件列表、代码、行号与括号无重叠 | [宽窗详情](screenshots/pr-conflicts-native-en-wide.png) |
| 同一窗口 EN 切换 zh-CN | 标题、状态、只读提示与文件列表立即翻译，文件和滚动位置保留；可选择中英文源码 | [中文详情](screenshots/pr-conflicts-native-zh-wide.png) |
| 非文本、重命名文件 | 保留源/目标路径；空态居中，导航与复制禁用，点击后路径 Tooltip 收起 | [非文本状态](screenshots/pr-conflicts-native-nontext.png) |
| 800 x 600，深色内容主题 | 默认收起文件侧栏；展开后路径省略、代码可横向滚动；前后冲突导航和边界禁用正确 | [窄窗详情](screenshots/pr-conflicts-native-en-dark-files.png) |
| 英文 800 x 600 列表 | 状态随文案自然占宽；PR 编号完整，长作者/分支省略，长标题换行，无相互覆盖 | [窄窗列表](screenshots/pr-list-conflicts-native-en-narrow.png) |
| 中文 1200 x 900 列表 | 同时展示冲突、无冲突、检查中、未知，与评审状态区分 | [宽窗列表](screenshots/pr-list-conflicts-native-zh-wide.png) |

截图验证中修正了工具栏子节点默认位置、状态过度伸展、行号/括号偏移、文件选中标记位置、Tooltip 点击残留，以及窄窗元信息预算导致 PR 编号被省略的问题。括号左竖线为 1px，横线朝右；起始标记的窄槽为黄色，起始行号格中性，其后到结束行的行号格为黄色。

深色 fixture 只切换应用内容的语义主题，不作为 macOS 系统标题栏主题切换验收。实际拖动系统窗口角落未成功改变窗口尺寸，因此分别以 800 x 600 和 1200 x 900 启动验证；测试后端另覆盖 800/1200/1800 宽度和两种语言的工具栏、gutter 几何。

### 自动检查

| 命令 | 结果 |
| --- | --- |
| `cargo fmt --all` | 通过 |
| `cargo check --locked` | 通过 |
| `cargo test --lib --locked --quiet` | 688 通过，2 忽略，0 失败 |
| `cargo test -p usher-i18n --locked` | 7 通过，0 失败 |
| `cargo test --locked --test pr_review_core_journeys --test pr_review_header_ui --test pr_review_status_bar_ui --test pr_list_menu_ui --test pr_list_review_status_fixture` | 共 10 通过，0 失败 |
| `scripts/check-ui-policy.sh` | 通过 |
| `scripts/check-i18n.sh` | 通过 |
| `git diff --check` | 通过 |
| 新模块直接创建 reqwest client 检索 | 无命中；复用共享 HTTP |

状态栏集成测试初次失败于查找英文无障碍标签；按已有 header 测试模式显式选择英文后，独立重跑和完整上述 PR 集成组合均通过。冲突 gutter/i18n/几何、晚到会话失败隔离、Provider 状态语义、Gitee 无写权限内容解码、实际 Git 文本/二进制/modify-delete 冲突及工作区不变均包含在通过的库测试中。

保留既有 Hooks unused-import/dead-code 警告。本轮未运行完整 `cargo test --locked`；仓库 AGENTS 记录的 `src/logger/debug_logger.rs` doctest 已知基线不属于本轮通过声明。

### 复现

按 [规格中的命令](../../specs/pr-conflicts.md#native-fixture--原生验收) 启动。额外的测试专用变量：

- `USHER_PR_CONFLICTS_WIDTH=1200`：1200 x 900 窗口；省略时使用默认 800 x 600。
- `USHER_PR_CONFLICTS_LOCALE=en` 或 `zh-CN`：起始语言。
- `USHER_PR_CONFLICTS_SWITCH_LOCALE=zh-CN`：60 秒后在同一页面切换语言。
- `USHER_PR_CONFLICTS_DARK=1`：深色内容主题。
- 同时设置 `USHER_PR_CONFLICTS_SCENE=list` 与 `USHER_PR_LIST_FIXTURE=review-status-v1`：使用正常列表控制器展示固定状态、长标题和分页数据。

这些入口要求测试 feature、离线模式及独立状态根；生产构建不编译冲突 fixture。

### 尚未执行

- GitHub、GitLab、Gitee v5/v8 的实际认证请求、企业 commitish 解析、私有 fork refs 和部署权限；未记录真实请求数量或声称线上兼容验收通过。
- 复杂共同祖先、超大仓库、限额触发、取消进程树的完整平台矩阵，以及 Windows/Linux 原生截图。
- 完整 Tooltip 20 次路由生命周期矩阵、跨应用粘贴校验、原生加载/错误状态矩阵。当前 fixture 没有完整 ReviewRequest/项目上下文，不能证明刷新和返回真实服务器的整条链路。
- 未执行性能优化 A/B，本轮不宣称性能提升。后续编辑、标记已解决和提交仍按迭代文档实施。

本轮工作区改动均属于本功能及其验证；未撤销用户改动，未修改 vendored Material，未提交 Git commit 或执行远端写入。
<!-- archive-section:offline-validation-20260928:end -->

---

<a id="interaction-fixes-20260929"></a>

<!-- archive-section:interaction-fixes-20260929:begin -->
## PR Conflicts 交互修复验收（2026-09-29）

本次范围：文件复制缺少反馈、详情导航 Conflicts 入口偏上/间距不足、代码滚动崩溃。
保留已有 PR 列表和冲突首批实现，以及工作区其他未提交改动。

### 崩溃证据与修复

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

### 自动验证

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

### 原生验收

已用原生 macOS mock 验证：此前导致退出的上滚/下滚/反向操作、多轮上下到底、
文本/非文本文件切换、冲突块导航、横向滚动、两秒复制提示及文件切换清理。
修复后的 mock 未出现 panic。中文 800px 窗口中同一页面运行时切换为英文后，
复制提示也立即采用英文。
最后的原生 mock 需在桌面会话中运行；沙箱内 GUI 启动曾以 134 退出且无应用日志，
在所需权限下重启后完成截图验收，日志没有 `panic` 或 `Recursion detected`。
临时测试实例已关闭。

- [英文复制成功](screenshots/fix-copy-en.png)
- [中文窄窗复制成功](screenshots/fix-copy-zh.png)
- [运行时切换语言后复制](screenshots/fix-copy-locale-switch.png)
- [多轮滚动与文件切换后的末尾](screenshots/fix-scroll-en.png)
- [宽窗导航入口对齐](screenshots/fix-tabs-en.png)
- [英文窄窗入口换行](screenshots/fix-tabs-narrow-en.png)
- [中文窄窗入口同排](screenshots/fix-tabs-narrow-zh.png)

最终英文窄窗截图确认 Tab 下划线与第二行入口不重叠。没有执行 Windows 原生验收或制造系统级
剪贴板失败；失败和过期完成由单元测试覆盖。
<!-- archive-section:interaction-fixes-20260929:end -->

---

<a id="navigation-and-counts-20260929"></a>

<!-- archive-section:navigation-and-counts-20260929:begin -->
## PR Conflicts 导航与分栏验收（2026-09-29）

本次修复截图中的重复进入触发加载、文件栏不能调宽、Conflicts 入口换行及字体不一致。
本记录中的单行导航取代上一轮验收记录的窄窗第二行方案。

### 实现

- Review 的 open callback 在冲突工作区已激活时立即返回，不改变返回来源。
  ConflictController 同时对当前仓库、PR 和分支上下文做幂等检查；显式 refresh 保持独立。
- 冲突文件栏采用普通 PR 文件树的拖动增量、取消恢复和 9px 命中区域规则。
  偏好宽度独立保存在冲突视图，收起/展开保留，窗口变窄时只约束实际宽度。
- Conflicts 复用 GitHubPullRequestTabButton，保持相同的 14px 字号、前景色和 16px 图标。
  四个入口保持 48px 单行；不足时内部间距由 8px 缩到 4px，仍保留文字、图标和计数。
  极端超宽内容可水平滚动。状态颜色仅作用于冲突图标。

### 自动检查

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

### 原生检查

复用 `USHER_PR_CONFLICTS_FIXTURE=1`、`test-fixtures` 和隔离的 offline 状态目录，
在 macOS 原生构建中检查：

- 1200px 窗口将冲突文件栏从 260px 拖到 380px，源码水平边界同步移动；
  收起再展开保持 380px，垂直源码位置不因分栏改变。
- 滚动到第 29 行后连续两次点击标题栏 Conflicts；当前文件、滚动位置、分栏宽度均未改变。
- 800px 窗口展开应用侧栏，英文四个入口完全可见且保持单行。
- 同一页面运行时从英文切换中文，文案和宽度测量立即更新，四个入口仍同排。
- 临时 mock 均已关闭；没有替换用户安装的应用。

截图：

- [分隔线拖动后的冲突工作区](screenshots/nav-splitter-wide.png)
- [重复点击前](screenshots/nav-reentry-before.png)
- [重复点击后](screenshots/nav-reentry-after.png)
- [宽窗导航](screenshots/nav-tabs-wide-en.png)
- [英文窄窗导航](screenshots/nav-tabs-narrow-en.png)
- [运行时切换中文后的窄窗导航](screenshots/nav-tabs-narrow-zh.png)

mock 仅提供冲突快照，因此返回后的普通 Diff 为空；导航截图用于验证几何和样式。
没有执行 Windows 原生验收或平台线上接口验收。本次没有修改平台取数逻辑。
保留工作区已有 PR 冲突/列表实现及其他未提交改动。

### Conflicts 文件数徽标初版验收（首次进入行为已由下节修正）

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

截图：[英文窄窗](screenshots/count-tabs-narrow-en.png)、
[运行时切换中文](screenshots/count-tabs-narrow-zh.png)。
mock 只提供冲突快照，普通 Diff 为空，不用于验证普通变更文件数量。
日志位于 `/private/tmp/usher-conflict-count-{build,check,lib,integration,i18n,en}.log`。
保留工作区其他未提交改动；已知 Hooks 未使用方法和测试 import 警告未调整。

### 首次进入 Review 的数量加载修正

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
英文截图见 [首次进入 Review](screenshots/count-first-entry-en.png)。
同一实例运行时切换中文后，数量和单行布局保持正确，见
[中文截图](screenshots/count-first-entry-zh.png)。临时实例正常退出（exit 0）。

`cargo fmt --all`、`cargo check --locked`、fixture 构建、UI policy、i18n catalog 和
`git diff --check` 通过。`cargo test --lib --locked` 为 697 通过、0 失败、2 忽略。
`cargo test --locked --test pr_review_status_bar_ui --test pr_list_status_layout_ui`
为 3 通过、0 失败。
新增回归覆盖普通入口/历史入口、后台完成不挂载工作区、未完成与已完成读取复用、
重置后旧成功/失败回写拒绝。保留原有无关改动与 Hooks 未使用方法警告。
日志位于 `/private/tmp/usher-conflict-prefetch-{build,check,lib,ui}.log`。
未执行线上平台和 Windows 原生验收。
<!-- archive-section:navigation-and-counts-20260929:end -->

---

<a id="files-and-totals-20260929"></a>

<!-- archive-section:files-and-totals-20260929:begin -->
## PR 冲突文件列表与总增删统计验收

日期：2026-09-29。

### 交互

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

### 检查结果

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

### 原生截图

全部使用源码构建的 `target/debug/usher`，复制到临时
`/private/tmp/Usher Conflicts.app` 并临时签名；应用标识为
`com.usher.conflicts-validation`。没有启动系统安装版 Usher。
使用隔离离线状态，不访问真实 Provider，也不修改真实仓库。

- [浅色宽窗 PR 总增删统计](screenshots/pr-totals-light-wide.png)
- [切换到中文并返回 Review](screenshots/pr-totals-light-zh.png)
- [英文冲突文件名、路径与右侧图标](screenshots/conflict-files-light-en.png)
- [长路径根层提示](screenshots/conflict-files-path-tooltip.png)
- [中文无文本冲突与重命名路径](screenshots/conflict-files-nontext-zh.png)
- [220px 最小文件栏宽度](screenshots/conflict-files-min-width-zh.png)
- [深色 800px 窄窗固定统计](screenshots/pr-totals-dark-narrow.png)
- [More 与 99+ 徽标](screenshots/pr-totals-dark-more-99plus.png)
- [最终源码版：128 个非文本冲突滚动后选择文件](screenshots/conflict-files-dark-scrolled.png)

原生检查包含运行时英文切中文、点击后 Tooltip 消失、返回 Review、拖动文件栏、
Enter 选择、代码上下滚动及大量文件滚动后的选择。没有复测独立 Settings/Messages
factory、Dialog Tooltip 全矩阵或 20 轮内存增长；本次未改动这些共享宿主。
Windows/Linux 原生、线上 Provider 和已知 debug-logger doctest 未运行。
本次为交互布局改动，不宣称性能改进。
工作区已有的 Hooks、Settings 和此前 PR 改动均保留。

### 复现

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

### 冲突检查动画补充验收

`GitHubConflictStatus` 的 checking 状态复用 `GitHubSpinner`，保持原有 14px
图标占位和文字对齐；状态结束后卸载动画并恢复对应状态图标。PR 列表中的同一
共享状态组件同步生效。本次没有新增用户可见文案。

使用 `cargo build --locked --features test-fixtures` 构建的源码应用，设置
`USHER_PR_CONFLICTS_SCENE=review` 和 `USHER_PR_CONFLICTS_DELAY_MS=60000`，
进入冲突页后验证英文加载状态、运行时中文切换、加载完成和再次刷新。
中文两帧顶部圆弧角度不同，图标占位和文字位置一致；完成后恢复红色冲突图标。

- [英文加载状态](screenshots/spinner-en-a.png)
- [中文动画第一帧](screenshots/spinner-zh-a.png)
- [中文动画第二帧](screenshots/spinner-zh-b.png)
- [加载完成](screenshots/spinner-complete-zh.png)

本次重新执行 `cargo fmt --all`、`cargo check --locked`、
`cargo test --lib --locked`（699 通过、2 忽略）、fixture 构建、完整根 Slint 检查、
UI policy、i18n 和 diff 空白检查，均通过。保留已有未使用代码警告。
未重新验证线上 Provider、Windows/Linux 原生或已知 debug-logger doctest。

### Tab 底线修复

原 PR 导航组件在自身末尾绘制 1px 底线，页面又在下一行添加了 1px
通栏分隔线，导致导航下方为 2px、尾部统计下方为 1px。
现在 PR Tab 组合行在内部底边统一绘制一条 `border-muted` 底线，
嵌套导航通过 `show-bottom-border: false` 关闭重复绘制。选中项的 2px
强调线覆盖同一底边；其他共享导航默认仍绘制自己的底线。

该关系已对照 [Primer 官方共享 Tab 样式](https://github.com/primer/react/blob/main/packages/react/src/internal/components/UnderlineTabbedInterface.module.css)：
容器使用 `inset 0 -1px var(--borderColor-muted)`，选中项的底部伪元素高 2px。
项目用 Slint 矩形表达相同的内嵌绘制关系，没有在下一行追加灰线。

源码构建的离线 mock 原生验收：

- [1200px 英文浅色](screenshots/tab-baseline-wide-en.jpg)
- [800px 中文深色，选中项收纳到 More](screenshots/tab-baseline-narrow-zh.jpg)
- [Tab 行局部](screenshots/tab-baseline-detail.png)

Retina 2x 截图中，宽窗 x=900/1800/2250 的底线均为 y=372..373，
窄窗 x=700/1000/1500 的底线均为 y=436..437：导航区与统计区各取样点
颜色一致，均仅占两行物理像素，即 1px 逻辑高度。More 展开与 Escape 关闭正常。

本次 `cargo fmt --all`、最终 `cargo check --locked`、fixture 构建、完整根
Slint 检查、UI policy、i18n 和 `git diff --check` 通过。完整库测试在解除
本地监听/进程操作的沙箱限制后为 698 通过、1 失败、2 忽略：
`hooks::tests::shutdown_cancels_running_and_queued_work_and_prevents_descendant_effects`
在 `src/hooks/tests.rs:1115` 失败；相同测试单独重跑通过（1 通过）。
完整套件未再次重跑，不能将其记录为全绿；本次没有更改 Hooks 代码。
<!-- archive-section:files-and-totals-20260929:end -->
