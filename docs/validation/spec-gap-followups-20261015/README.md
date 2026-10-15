# Spec gap follow-ups / 剩余实现缺口修复

本批继续处理 [源码核对记录](../spec-audit-20261014/README.md) 中已确认的实现缺口，
并修复消息最终关闭时的 UI 阻塞边界。Preview 文件预算与第一批 UI reducer 修复见
[前批记录](../spec-gap-fixes-20261015/README.md)。本记录只覆盖下列代码路径和实际
执行的回归，不代表所有原生平台、Provider 或视觉验收均已完成。

后续 UI-120 宿主分页焦点修复与 macOS 原生对照见
[专项记录](../ui120-macos-focus-20261015/README.md)；下文保留本批结束时的历史验收范围。

## 修复范围 / Adopted behavior

| 项目 | 当前实现及验证边界 |
| --- | --- |
| Messages hydration | 一次成功初始化由并发调用共享；先合并、安装自动 observer，再比较规范化磁盘基线。纯磁盘加载和空 revision 0 不写盘；初始化期间业务变化仍持久化，重复调用不复活已删消息。见 [详细证据](messages-hydration.md)。 |
| 最终关闭 | 根窗口先关闭输入；async drain 等待完整发布方，原线程 pre 停止其余发布方，后台阶段关闭 MessageCenter/Project/请求偏好及最终 Settings writer，再回原线程清理 UI/原生资源。阶段和 finalizer 共享完成屏障，保留优先级/注册逆序。 |
| AI Review 发布方 | permit 覆盖 spawn 前准入、真实 Session receipt、终态 Task/Message 和 awaited Hook continuation；首帧前退出完成同一保留恢复任务。先完成发布方，再关闭 Hooks/消息 writer。失败后成功显式重试不会被历史错误累计判定为退出失败。见 [详细证据](ai-review-producers.md)。 |
| Diagnostics 归档 | 使用私有临时句柄完整完成 tar/gzip/sync 后无覆盖发布；导出和 prune 串行，每次成功返回时最多 5 份匹配的普通归档。保留当前返回路径；清理失败返回错误并删除本次新文件。 |
| StatusBar 已展开详情 | 浮层仅保存语义身份，标题/详情保持 live binding；切语言和状态变化立即更新。快捷键阶段、下载阶段/字节/百分比按语义状态重投影；失效条目关闭浮层。 |
| Workflow 共享状态 | legacy Mutex 业务变更迁入 feature StateStore 门面、强类型 Action 和 FIFO Worker；64 个所有权许可覆盖排队、任务及未消费 UI 回调。自动 revision 投影，项目/代际隔离，完整 pending 草稿保护；Project 匹配 commit 后才初始化并导航。 |
| Tag 初始化与草稿 | 初始化命令有 30 秒截止时间和取消 token，使用共享 ProcessTree 清理后代；正常路径仍按 argv 调 Git。Description 通过真实编辑回调保存完整草稿，Add/Remove 及路由重挂保留。Stop/reset/项目替换使旧运行终态和导航失效。 |

中英文架构、相关 feature spec 和 spec 索引已随实现同步；新 owner 的结构规模和依赖
门禁已加入原检查及正反夹具，未调高原预算。Tag 控制器拆分转移状态、执行、初始化、
进度和任务所有权职责，UI bridge 不依赖具体 Store engine。

## 回归结果 / Executed checks

| 检查 | 实际结果与原始输出 |
| --- | --- |
| `cargo fmt --all` | PASS，exit 0；[日志](cargo-fmt-final.log) |
| `cargo check --locked` | PASS，exit 0；包含完整根 UI；[日志](cargo-check-complete.log) |
| `cargo check --no-default-features --features renderer-femtovg --locked` | PASS，exit 0，无 warning；[日志](core-feature-check-final.log) |
| `cargo test --lib --locked` | PASS，874 passed / 0 failed / 7 ignored，35.50s；允许 localhost 夹具后执行；[日志](cargo-test-lib-complete.log) |
| `cargo test --doc --locked` | PASS，1 passed / 0 failed / 1 existing ignored；Logger 示例执行通过；[日志](cargo-test-doc-final.log) |
| `cargo test -p usher-messages --locked` | PASS，34 passed / 0 failed / 0 ignored；[日志](messages-package-final.log) |
| `cargo test -p usher-i18n --locked` | PASS，7 passed / 0 failed；[日志](i18n-package.log) |
| `cargo test --lib --locked status_bar_tests` | PASS，2 passed；保持已挂载节点的 en/zh 更新和操作结束卸载；[日志](status-bar-focused-final.log) |
| compiled Project/Tag UI 定向回归 | PASS，1 parent test，实际执行 en/zh × grid/list 四格；[日志](project-tag-ui-focused.log) |
| `cargo test --lib --locked workflow::tag::initialization::tests` | PASS，4 parent tests；before 逃逸、after 超时/取消及正常 Git 控制；[日志](tag-process-focused.log) |
| `python3 -m unittest discover -s scripts/tests` | PASS，105 tests，50.874s；[日志](python-fixtures-final.log) |
| 结构 / UI / i18n policy | 全部 PASS，exit 0；[结构](architecture-gate-final.log)、[UI](ui-policy-final.log)、[i18n](i18n-policy-final.log) |
| Markdown 链接/锚点、源码引用和 25 份 spec 索引 | PASS；[导航检查](navigation-check.txt) |
| `git diff --check` | PASS，exit 0；[日志](diff-check.log) |

Root 的 7 个 ignored 中，2 个是原有线上 AI Provider 和 opt-in PR memory A/B；其余
5 个是必须由普通父测试在隔离进程执行的 helper（一个 Project event loop，四个
Tag 子进程/真实 Git 入口），本轮父测试均实际调用并通过，未用 ignore 跳过验收。
Python 日志中的 `runner exited 7`/`passed: false` 是检查器故障注入的预期负例输出，
最终 unittest 结果为 `OK`。

## 对照与修复过程 / Controls and corrected failures

- hydration 行为对照临时恢复 HEAD 的旧无条件提交算法，仅替换同一方法并在结束后
  恢复候选实现。[旧算法差异](hydration-legacy-algorithm.patch)及
  [原始回归](hydration-legacy-regression.log)记录 3 个预期失败（磁盘加载不写、空
  revision 0 不创建文件、早期新增后移除回到基线不写），以及 2 个通过的正常业务控制。
  最终候选的 package 回归另行记录，未保留旧算法开关。
- 最初编译轮次暴露迁移中残留签名、三个下载状态回调类型、测试可见性及 TextInput
  accessibility role 错误；均已修正。对应 `*-initial.log`、`*-second.log` 和
  `cargo-check-final.log` 为失败的迭代记录，文件名不表示最终通过。
- 第三轮完整库测试实际执行：871 passed / 2 failed / 7 ignored。
  [原始日志](cargo-test-lib-third.log)中的两项失败分别是 inline Slint harness
  未打包翻译及真实时钟与 mock 点击混用。修复为同一翻译 bundle 和一致 mock clock，
  保留原有语言/行为断言。随后的 StatusBar 定向测试还暴露 deferred `changed` 未推进
  UI turn 的夹具时序问题，见 [日志](status-bar-focused.log)；修正后增加浮层节点
  卸载断言。最终完整回归为上表的 874 passed / 0 failed。
- Tag 的 [进程定向回归](tag-process-focused.log)在同一 parent/descendant 夹具中
  验证旧 direct-child-only 方式回收父进程后后代仍可写入；随后验证生产 timeout/cancel
  终止整组、后代不能写入。正常 Git 控制使用临时本地/裸 origin，包含中文、空格和
  shell 元字符路径；所有测试隔离 Git 配置、Hooks 和签名，不读取工作站配置。
- 翻译 harness 修复的后续核对确认根 build script 也固化了编译时 checkout 路径。
  现改为读取本次调用的 `CARGO_MANIFEST_DIR`。同一二进制的两个带空格目录夹具中，
  [旧版按预期失败](root-i18n-path-baseline.log)，[修复候选通过](root-i18n-path-candidate.log)；
  最终常规 Cargo 构建及 Python 全集也包含该夹具。它只比较 bundle 位置、完整文件与
  catalog 一致性，不声称性能收益。
- 这批按功能正确性、资源预算和生命周期契约采用，不宣称吞吐、延迟、RSS 或 footprint
  收益；未执行 Release 性能 A/B，不将单测作为运行时性能证据。

## 保留边界 / Remaining acceptance

- 真实 compiled Slint/Store/进程/磁盘夹具覆盖本批竞态，但未手工完成原生窗口退出、
  全路由视觉、原生通知权限和托盘点击矩阵；Messages 的 UI-120 原生分页焦点仍保留。
- ProcessTree 后代取消在当前 Unix/macOS 测试；Windows job assignment 在 spawn
  之后的窗口及 Windows 原生进程/交互验收未测，未声称跨平台完整认证。
- 既有退出日志/状态策略、历史 P7 #77 的 deadline/abort/累计错误策略未在本批替换。
  `PersistentStore::shutdown` 的底层 void/Action 准入边界及 Chat 旧文本兼容仍按 spec
  描述；没有将其误报为本次已修复的 checked-close/语义 Schema。
- 未运行线上 Provider、Release 打包或性能 benchmark。

保留此前文档整理与第一批修复。无关 staged 删除
`ui/assets/.DS_Store`、`ui/assets/fonts/.DS_Store`、
`ui/assets/fonts/Noto_Sans_SC/.DS_Store` 未改写、未取消暂存；未创建 Git commit。
