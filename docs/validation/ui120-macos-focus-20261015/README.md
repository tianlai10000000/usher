# UI-120 消息中心宿主分页焦点（2026-10-15）

范围：修复原生 macOS ComponentFactory 消息页点击分页后仍由侧栏处理键盘的问题，
并核对分页键盘链、禁用方向焦点环、运行时语言切换与路由生命周期。
这是行为正确性修复，不宣称运行时性能收益，不以 Debug 构建速度作为 Release A/B。

## 实现与决策

- `src/message_center/focus.rs` 管理真实 MainWindow adapter 的焦点；
  `focus/navigation.rs` 遍历实际挂载树，承接 Tab、Shift+Tab 和独立 Backtab。
  过滤隐藏/禁用、不可交互省略号及屏幕外透明侧栏，保留 Flickable 揭示能力。
  赋焦点前按目标真实矩形最小滚入所属视口，避免行 Clip 拒绝反向目标后固定向前回退。
- 显式指针信号区分鼠标/键盘分页。鼠标保持回顶部行为，转交中性 page scope，
  Return/Space 不导航、不标已读；键盘保留原分页链接及可见 footer。
- 共享 Pagination 的键盘激活不再调用私有 factory `.focus()`；禁用方向立即清除
  绘制状态并拒绝再次激活。模态/隐藏页阻止背景键，离开路由清除页面持有焦点。
- 保留固定 20 行与九个页码槽位；没有新增根窗口代理、用户文案或 vendor 修改。
  新 focus owner 有独立规模和 UI-only 依赖门禁，未上调既有结构预算。

## 原生 before/candidate 对照

环境与构建身份见 [environment](native-environment.txt)；完整输入步骤、隔离方式与
观察结果分别见 [baseline](native-baseline.md) 和 [candidate](native-candidate.md)。
使用完整生产应用、离线模式、独立临时 Settings/messages、1000 条合成通知与同样
的 800 × 600 logical 前置，不读取/改写工作站配置，也不访问 Provider。

| 对照 | 观察及采用决策 |
| --- | --- |
| baseline | Home 蓝环 → 单击 page 4 → Return 确实导航 Home。失败，保留原始日志/SHA。 |
| 中间候选 | 相同前置保持 Notifications、首条 0940、未读 1000；但键盘 Next 到末页再 Tab 出现双环。拒绝交付这一候选，继续修复。 |
| 最终候选 | 核心同前置通过；逐键 Tab、键盘分页、末页单焦点环、中英文切换、zoom/restore、路由重入、0/1 页过滤与恢复通过。采用；原生反向工具限制单列，不宣称全平台验收。 |

原生 CUA Shift+Tab 在普通 Home → Chat 的宿主对照中也前进，不能作为反向验收。
真实反向键盘仍保留手工复核项；完整 compiled UI 对实际宿主的 Shift+Tab/Backtab
覆盖单独列出，不冒充 native 结果。Windows、读屏未测。原生最小窗口 800 × 600；
强制 640px 的方向 fallback 只属于 compiled 组件边界测试。

## 自动回归与失败记录

| 检查 | 实际结果 / 原始日志 |
| --- | --- |
| baseline 完整 native build | 成功，3m51s；[log](baseline-build.log)。 |
| 中间 native build | 成功，19.15s；[log](candidate-build-initial.log)。 |
| 最终完整 build | 成功；[禁用环/Backtab 构建](candidate-build-final.log) 3m27s，[精确 reveal 最终构建](candidate-build-reveal.log) 9.02s。 |
| 完整 compiled UI 定向 | **4 passed / 0 failed**，含 en/zh、真实宿主完整反向/Backtab、模态/隐藏、实际描边、1000 条/50 页、强制窄布局与固定 model；[log](focused-reveal-after.log)。 |
| `cargo fmt --all` | exit 0；[log](cargo-fmt.log)。 |
| `cargo check --locked` | exit 0；[log](cargo-check.log)。 |
| `cargo test --lib --locked` | **876 passed / 0 failed / 7 ignored**；[log](cargo-test-lib.log)。 |
| `cargo test --doc --locked` | **1 passed / 0 failed / 1 ignored**，logger doctest 实际通过；[log](cargo-test-doc.log)。 |
| `cargo test -p usher-messages --locked` | **34 passed / 0 failed**，doc 0；[log](messages-tests.log)。 |
| `scripts/ui-check.sh` | exit 0，完整 root UI + 开发功能 alias 编译；[log](ui-check.log)。 |
| 架构 / UI policy | exit 0；[architecture](architecture-gate.log)、[UI](ui-policy.log)。 |
| `git diff --check` | exit 0；[log](diff-check.log)。 |
| 文档导航 / 源码引用 | 38 份 Markdown、25 项 spec 索引核对通过；[log](navigation-check.txt)。 |
| Python 全套 | 107 passed；[log](python-fixtures.log)。含新增隔离 native bundle 工具正反例。 |
| 架构 gate 夹具 | 10 passed；[log](architecture-fixtures.log)。新 owner 规模/依赖负例纳入既有 suite。 |
| native 工具独立夹具 | 2 passed；[log](native-fixture-tool-tests.log)。路径含 Unicode/空格/shell 字符，环境覆盖、非法输入和禁止覆盖。 |
| i18n catalog gate | passed；[log](i18n-policy.log)。无新增产品字符串；原生 en → zh-CN 另测。 |

根 lib 的 7 项 ignored 中，5 项是由父测试启动的独立 event-loop/子进程夹具，并由父
测试实际验证；另 2 项是原有真实在线 AI 与 PR memory Release A/B，本批未运行。
本批不修改这两项策略，不将它们计作运行通过。Doctest 的 ignored 为既有关闭优先级示例。

保留以下中途失败，不将失败候选当作新基线：

1. [首次编译](focused-initial.log)：三处 ItemTreeRc 比较类型错误；改用 ptr_eq 后编译通过。
2. [第二轮](focused-second.log)：3 passed / 1 failed。裸窗口夹具遗漏生产侧栏 motion
   callback，侧栏被 spring 默认 progress 0 移出窗口；补真实初始化及展开断言。
   Messages factory 缺 debug metadata 的 warning 与此失败根因分开记录。
3. [第三轮](focused-third.log)：3 passed / 1 failed。短页翻页后的坐标点击未可靠产生
   期望私有焦点；使用真实生成的 neutral `.focus()` 路径显式建立残留私有焦点，同时
   强断 host 仍持有 page 3，保留后续真实 Shift+Tab/Space 判别断言。
4. [第四轮](focused-fourth.log)：3 passed / 1 failed。裸窗口遗漏生产 DialogManager
   callback 和延迟显示 timer；接入真实 setup_dialog/settle。
5. [第五轮](focused-fifth.log)：4 passed / 0 failed。上述夹具初始化修正后通过。
6. [禁用环 before](disabled-outline-before.log)：0 passed / 1 failed，实际 paint
   border width 2px，期望 0；原生亦复现 disabled Next 与 breadcrumb 双环。
7. [native 工具初版](native-fixture-tool-initial.log)：临时路径 `/var` 与 `/private/var`
   别名断言不一致；工具统一 resolve 后，独立夹具和全 Python suite 均通过。
8. [Backtab 首轮](focused-final.log)：3 passed / 1 failed。反向遍历未离开 factory；
   [诊断 trace](backtab-trace-before.log) 为 0 passed / 1 failed，同一可见 Mark as read
   重复 30 次、scroll_y 固定 -1216。精确揭示目标后 [after](focused-reveal-after.log)
   4 passed / 0 failed，没有删除或弱化完整反向断言。

原生重启的 crash-sentinel 对照及最终重新隔离步骤见 candidate 记录。基线/中间/最终
二进制及输入结果均独立保存；采用最终通过的候选，没有把失败候选滚入基线。

## 文档与工作区

已同步 [Messages spec](../../specs/messages.md)、[spec 索引](../../specs/README.md)、
[英文架构](../../architecture.md)、[中文架构](../../architecture.zh.md) 与
[UI-120 历史](../../../bug-history.md)。保留此前 spec-gap 批次的历史状态，另加后续链接。
此前所有未提交修改和三项无关 staged `.DS_Store` 删除均保留；本批不创建提交。
