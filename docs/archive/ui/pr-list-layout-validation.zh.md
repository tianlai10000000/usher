# PR 列表布局验证

日期：2026-09-28。本记录替代早期冲突验收中的列表截图。

## 实际参考与实现

直接查看 [GitHub cli/cli PR 列表](https://github.com/cli/cli/pulls)，分别记录
[默认宽度](../pr-conflicts/screenshots/github-pr-list-reference-wide.png) 与
[640px](../pr-conflicts/screenshots/github-pr-list-reference-narrow.png)。
GitHub 将标题置于首行，作者/时间、评审和 CI 检查自然排列在下方，使用分隔点，
窄窗口各段按剩余空间换行。CI 检查不是代码冲突状态；本次观察没有把 GitHub
列表宣称为存在独立冲突列的依据。Usher 保留用户要求的冲突入口和分支信息，
沿用这种紧凑元信息排列。

评审与冲突标签按实际文案占宽，元信息统一 24px 高和 16px 分隔区间；无评审时
不预留空列。分支使用 20px 胶囊与 regular 11px 等宽字，居中跟随最后一段；
剩余空间不足时整组换行。英文系统字体与等宽字体的可见字形中心比同行文本低
1px，因此通过 locale manifest 提供 `-1px` 紧凑文字补偿；中文保持 `0px`。
运行时切换语言会立即重投影该数值，组件不判断 locale id 或翻译文本。
长标题按原生 Text 的统一 `line-height-factor` 绘制/测量契约增长。评论计数按
文本测量预留宽度，覆盖 18、99、128 等多位数字。
冲突图标沿用详情页评审勾的光学对齐方式，从控件几何中心向上补偿 1px，
匹配 12px 文字的可见字形中心。状态入口位于整行点击层之上，避免误进 Diff。

## 复现

```sh
./tests/run-pr-list-review-status-fixture.sh
USHER_PR_LIST_WIDTH=800 USHER_PR_LIST_LOCALE=zh-CN USHER_PR_LIST_PROVIDER=gitee ./tests/run-pr-list-review-status-fixture.sh
```

复用原命令，自动创建临时状态目录并离线进入 PR 列表。36 条数据包含 24 条打开、
12 条关闭/合并，覆盖长标题、长分支、中文作者、缺失分支、无评审与四种冲突状态。
可使用正常查询、筛选和分页。此 fixture 不具备真实项目/ReviewRequest 上下文，
点击详情不作为服务端冲突读取链路验收。

macOS 截图工具需要应用包身份，截图运行通过 Cargo runner 把该命令构建的同一
二进制复制到临时测试包；离线模式、数据与自动导航均由原 mock 命令提供。

## 原生截图

- [英文 1200px，侧栏展开；计数/图标最后修正前](../pr-conflicts/screenshots/pr-list-inline-en-wide.png)
- [英文 1200px，侧栏收起；计数/图标最后修正前](../pr-conflicts/screenshots/pr-list-inline-en-wide-full.png)
- [英文 800px，GitHub](../pr-conflicts/screenshots/pr-list-inline-en-narrow.png)
- [最终换行状态图标对齐](../pr-conflicts/screenshots/pr-list-icons-aligned.png)

设置中运行时由中文切换英文，返回列表后元信息、相对时间、状态标签和布局均更新。
原生验证采用分别指定启动尺寸及侧栏切换，未将系统窗口拖拽作为已通过项。
最终英文与中文 800px Gitee 均使用当前源码构建的独立临时应用包现场检查：英文
元信息前缀与分支标签应用 `-1px` 补偿后和时间/状态的可见基线一致；中文保持
`0px`。元信息段共用 24px 中线，20px 分支胶囊在该中线上居中。
验证包使用独立 Bundle ID 与临时状态目录，未打开或绑定系统安装版 Usher。

## 自动检查

`tests/pr_list_status_layout_ui.rs` 覆盖三种平台展示、中英反复切换、侧栏两种状态、
800/1000/1200/1800px 宽度、缺失评审/分支、关闭行、长标题及多位评论计数。
验证同排分隔距离、换行左边缘、纵向区间、末行底部间距，以及评审、冲突、标题
分别进入 Tab 0/3/2。现有菜单定位与 PR Review 集成检查继续运行。

执行结果：`cargo fmt --all`、`cargo check --locked`、UI policy、i18n catalog
与 shell 语法检查通过；`cargo test --lib --locked` 为 699 通过、2 忽略；
六组 PR 集成测试共 11 通过。新增点击测试曾检出整行命中层覆盖状态入口，修复
层级后重新执行通过。保留既有 Hooks unused-import/dead-code 警告。
图标补偿后重新执行 `cargo check --locked`、列表布局/入口测试、UI/i18n gate
均通过；fixture 过滤/分页单元测试 2 通过。`git diff --check` 通过。

本次没有改变平台适配和网络语义；线上接口、Windows/Linux 原生布局仍未验证。
保留既有工作区 Conflicts 与 Hooks/runtime 等改动，未提交 Git commit。
