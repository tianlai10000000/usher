# PR Conflicts 离线验收记录

日期：2026-09-28。第一批只读代码与离线验收完成；各平台线上权限、部署能力和真实仓库验收尚未执行。线上未验证项不视为通过。

列表布局随后经用户反馈修正；本页旧列表截图仅作为历史记录，不能作为当前布局验收。
当前结果见 [PR 列表布局验证](pr-list-layout-validation.zh.md)。

## 原生截图

运行的是 `cargo build --locked --features test-fixtures` 生成的 macOS 原生程序，使用独立应用包与 `/private/tmp/usher-conflicts-qa-*` 状态目录，开启 `USHER_BENCHMARK_OFFLINE=1`。数据经过实际 parser、后台文件准备、Slint bridge 与正式组件。截图不是 HTML 复刻或设计稿。

| 场景 | 检查结果 | 截图 |
| --- | --- | --- |
| 英文 1200 x 900 | 两层工具栏居中；文件列表、代码、行号与括号无重叠 | [宽窗详情](designs/pr-conflicts/validation/pr-conflicts-native-en-wide.png) |
| 同一窗口 EN 切换 zh-CN | 标题、状态、只读提示与文件列表立即翻译，文件和滚动位置保留；可选择中英文源码 | [中文详情](designs/pr-conflicts/validation/pr-conflicts-native-zh-wide.png) |
| 非文本、重命名文件 | 保留源/目标路径；空态居中，导航与复制禁用，点击后路径 Tooltip 收起 | [非文本状态](designs/pr-conflicts/validation/pr-conflicts-native-nontext.png) |
| 800 x 600，深色内容主题 | 默认收起文件侧栏；展开后路径省略、代码可横向滚动；前后冲突导航和边界禁用正确 | [窄窗详情](designs/pr-conflicts/validation/pr-conflicts-native-en-dark-files.png) |
| 英文 800 x 600 列表 | 状态随文案自然占宽；PR 编号完整，长作者/分支省略，长标题换行，无相互覆盖 | [窄窗列表](designs/pr-conflicts/validation/pr-list-conflicts-native-en-narrow.png) |
| 中文 1200 x 900 列表 | 同时展示冲突、无冲突、检查中、未知，与评审状态区分 | [宽窗列表](designs/pr-conflicts/validation/pr-list-conflicts-native-zh-wide.png) |

截图验证中修正了工具栏子节点默认位置、状态过度伸展、行号/括号偏移、文件选中标记位置、Tooltip 点击残留，以及窄窗元信息预算导致 PR 编号被省略的问题。括号左竖线为 1px，横线朝右；起始标记的窄槽为黄色，起始行号格中性，其后到结束行的行号格为黄色。

深色 fixture 只切换应用内容的语义主题，不作为 macOS 系统标题栏主题切换验收。实际拖动系统窗口角落未成功改变窗口尺寸，因此分别以 800 x 600 和 1200 x 900 启动验证；测试后端另覆盖 800/1200/1800 宽度和两种语言的工具栏、gutter 几何。

## 自动检查

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

## 复现

按 [规格中的命令](specs/pr-conflicts.md#native-fixture--原生验收) 启动。额外的测试专用变量：

- `USHER_PR_CONFLICTS_WIDTH=1200`：1200 x 900 窗口；省略时使用默认 800 x 600。
- `USHER_PR_CONFLICTS_LOCALE=en` 或 `zh-CN`：起始语言。
- `USHER_PR_CONFLICTS_SWITCH_LOCALE=zh-CN`：60 秒后在同一页面切换语言。
- `USHER_PR_CONFLICTS_DARK=1`：深色内容主题。
- 同时设置 `USHER_PR_CONFLICTS_SCENE=list` 与 `USHER_PR_LIST_FIXTURE=review-status-v1`：使用正常列表控制器展示固定状态、长标题和分页数据。

这些入口要求测试 feature、离线模式及独立状态根；生产构建不编译冲突 fixture。

## 尚未执行

- GitHub、GitLab、Gitee v5/v8 的实际认证请求、企业 commitish 解析、私有 fork refs 和部署权限；未记录真实请求数量或声称线上兼容验收通过。
- 复杂共同祖先、超大仓库、限额触发、取消进程树的完整平台矩阵，以及 Windows/Linux 原生截图。
- 完整 Tooltip 20 次路由生命周期矩阵、跨应用粘贴校验、原生加载/错误状态矩阵。当前 fixture 没有完整 ReviewRequest/项目上下文，不能证明刷新和返回真实服务器的整条链路。
- 未执行性能优化 A/B，本轮不宣称性能提升。后续编辑、标记已解决和提交仍按迭代文档实施。

本轮工作区改动均属于本功能及其验证；未撤销用户改动，未修改 vendored Material，未提交 Git commit 或执行远端写入。
