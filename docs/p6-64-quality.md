# P6 #64：本地质量基线与检查行为 A/B

日期：2026-09-16。源码基线：`209e5c6`。起始仅 `optimizing-proposal.md` 有上一轮审查
的未提交修改，本批在其基础上追加记录。范围是开发检查与格式修复，不改变应用逻辑、
Slint 绘制、依赖或 CI 触发频率，不执行 #65 及后续应用改造。

## 改动与边界

- 使用 `cargo fmt --all` 修复三个 Jobs 文件的导入排序和尾逗号。
- `check-ui-policy.sh` 精确登记 `github_hook_tooltip.slint`。该文件是 Tooltip spec
  已记录的 Settings factory 固定动作 Portal，仍使用共享 `GitHubFloatingOverlayHost`。
  没有对整个 Settings 目录放行，也没有改动 Tooltip 实现。
- 新增 `scripts/check-quality.sh`：顺序运行 fmt、结构、UI、i18n、Python 脚本回归、
  默认 check、库测试、diff check。普通失败后继续执行并汇总，任一失败最终返回 1；
  缺少工具/参数错误返回 2，中断返回 130/143，不启动后续检查。检查不格式化源码。
- 手动 CI 的 Python discovery 同步包含新检查测试，原 workspace/clippy/feature
  检查保持原样；CI 仍仅手动触发，Nightly schedule 仍暂停。
- `AGENTS.md` 固化后续每批的自测和 A/B 要求。工具批次做行为对照；应用性能批次
  做受控 Release 对照与原始采样，不允许把未测量的 RSS 收益写成完成。

统一入口需要 `cargo`、`rg`、`python3`、`git`，通过自身路径定位仓库，不依赖调用者
的当前目录。库测试需要本地 HTTP 监听权限。它不替代 workspace/禁用 feature/
Windows/真实 UI/Release 性能矩阵，也不把已知结构超限当作成功。

## 四项结构超限的职责方案

本批只登记后续所有权，不移动应用代码、不修改结构预算。四项继续阻断统一入口，
因此“第 64 批工具改造完成”不代表“仓库全部检查通过”。

| 检查项 | 基线/本批值 | 原预算 | 后续归属与变更边界 | 验收 |
| --- | ---: | ---: | --- | --- |
| AI controller 行数 | 3876 | 3850 | #71：从 `src/ai_review/controller.rs` 的任务编排中提取响应 presenter/Review Ask 会话；已有 `sessions.rs` 继续拥有 Session I/O，headless crate 不依赖 Slint | 用 typed 输入/输出独立测试，locale/任务切换不回退；不靠搬测试降行数 |
| 根 Slint 行数 | 3044 | 3000 | #71：根窗口保留 route、根浮层挂载和装配；PR/AI 功能展示协议逐项归对应 `ui/features/` contract/global | factory 仍显式桥接；焦点、快捷键、Tooltip、i18n 与路由 A/B 通过 |
| 根 Slint 属性数 | 420 | 410 | 与上一项同批；只迁移 feature 独占属性，根窗口几何与跨 factory 桥接属性保留 | Rust/Slint 名称和类型一致；不把属性搬成另一套隐式权威状态 |
| PR actions 行数 | 688 | 580 | #66/#71：动作捕获 context、远端 mutation、纯 Hook event 构造与 capability presenter 各有明确输入输出；保留 generation 和远端成功后发布事件的顺序 | approve/comment/merge/revert 与快速项目切换测试通过，副作用不被 UI 过期判定吞掉 |

`src/hooks/bridge.rs` 是关联热点，#68/#69/#71 分别承担回放、状态投影及 Settings/editor
边界；本批没有通过新预算豁免它。正式拆分时给新 owner 增加依赖方向和规模检查，
复核总复杂度，不能只让旧文件回到行数门槛内。

## A/B 协议

A 为 `209e5c6` 的原始检查脚本和未格式化应用源码；B 为本批候选。两个版本对相同的
真实 Hook Tooltip 文件及相同反例运行，测试框架在临时目录隔离 fixture。旧脚本通过
`git show` 只读提取，不切换当前分支、不覆盖工作树。生产应用变化只有 rustfmt，
因此另比较 A/B 完整库测试，不进行没有行为变量的 Release RSS 对照，也不宣称内存收益。

可重复命令：

```sh
USHER_TEST_UI_POLICY_BASE_REF=209e5c6 python3 -m unittest scripts.tests.test_quality_checks.PolicyTests -v
python3 -m unittest scripts.tests.test_quality_checks.PolicyTests -v
python3 -m unittest scripts.tests.test_quality_checks.QualityRunnerTests -v
./scripts/check-quality.sh
```

第一条预期返回 1，且只失败于真实 Hook Portal 的接受检查；B 的六项 policy 检查
必须全部通过。反例验证未登记的同目录浮层、已登记路径中的内置 Tooltip、私有
Material 引用、缺 Portal 的独立工厂和重复 Preview 挂载仍被拒绝。

统一入口的隔离测试使用可控命令替身检查调度语义；这不代替最终真实 Cargo 执行。
它验证从其他目录运行、源码只读检查、结构失败后继续执行库测试和 diff，以及中断停止。

## 结果

**采用本批工具和格式修复。** 真实统一入口最终 7 项通过、结构检查失败，退出码 1；
四项结构超限在 A/B 中完全一致，没有静默豁免或提高预算。

| 检查 | A | B | 判断 |
| --- | --- | --- | --- |
| 原工作树 fmt | 失败：3 个 Jobs 文件 | 通过 | 只修导入排序/尾逗号 |
| 原工作树 UI policy | 失败：Hook Portal 误报 | 通过 | 精确 owner 登记生效 |
| 相同六项 policy 正反例 | 5 通过、1 失败（合法 Hook Portal） | 6 通过 | 修复误报，五种违规仍拒绝 |
| 结构门禁 | 4 项超限，exit 1 | 相同 4 项超限，exit 1 | 原预算保持 |
| 完整库测试，允许本地监听 | 602 通过、0 失败、2 忽略，74.09s | 602 通过、0 失败、2 忽略，74.36s | 行为回归通过；时间不用于性能收益判断 |
| 新统一入口隔离调度测试 | 无旧统一入口 | 3 通过 | 顺序/普通失败继续/中断停止均验证 |
| 全部 Python 脚本测试 | 未重新执行旧套件 | 21 通过，3.413s | 含新 9 项回归；门禁负例日志中的模拟失败是测试输入 |
| `cargo check --locked` | 上轮审查通过 | 通过，2.25s | 完整默认 root 编译通过 |
| `./scripts/check-i18n.sh` | 上轮审查通过 | 通过 | 没有变更 UI 字符串 |
| `sh -n scripts/check-quality.sh scripts/check-ui-policy.sh` | 未运行 | 通过 | Shell 语法检查 |
| `git diff --check` | 通过 | 通过 | 包括最终文档更新 |

本批实际先运行 `cargo test --lib --locked` 建立 A 基线，沙箱内因两项 Logo HTTP
夹具的 `TcpListener::bind("127.0.0.1:0")` 被拒绝而失败：600 通过、2 失败、2 忽略。
随后在允许本地监听的环境直接运行该次生成的完整 A 测试程序
`target/debug/deps/usher_lib-ee287357881873a5`，602 项全部通过。B 的完整
`./scripts/check-quality.sh` 在相同允许监听环境运行，其中实际执行
`cargo test --lib --locked`；没有跳过两个 HTTP 测试或 Chat 慢测试。
不以分环境结果抹除第一次失败记录。

原始日志保存在 Git 忽略的
`memory-benchmark-results/p6-64-quality-20260916/`：

- `usher-p6-64-a-fmt.log`、`usher-p6-64-a-ui.log`、`usher-p6-64-a-structure.log`；
- `usher-p6-64-a-policy-tests.log`、`usher-p6-64-b-policy-tests.log`；
- `usher-p6-64-a-lib.log`（沙箱失败）和 `usher-p6-64-a-lib-localhost.log`（完整通过）；
- `usher-p6-64-b-quality.log`（真实统一检查的完整结果）。

测试链接仍有既有 `__eh_frame` compact unwind 警告。未运行远端手动 CI、workspace/
integration/clippy、Windows、disabled-feature、logger doctest 或 Release RSS A/B。
本批没有 Slint 变动，因此未重新执行 Tooltip 原生视觉矩阵；本结论只验证检查工具
接受已登记的实现，不重新宣称其所有视觉场景通过。
