# P7 #73：快速/完整质量检查层

## 测量前固定的范围与门槛

基线为已采用 #74+#75+#76，#77 拒绝后的当前工作区；候选仅修改检查工具、
手动 CI 调用及对应文档，不改变业务源码或运行时。保留默认快速入口的八项
检查，新增完整层覆盖 workspace 全目标默认检查、所有成员的单元/集成测试、
严格 Clippy 和根应用关闭 AI feature 的完整编译图。root-only feature-off
与全成员默认检查是两种不同契约，不能把 workspace 中主动选中的独立 AI crate
编译描述成根应用仍然依赖 AI。

完整测试使用 `--workspace --tests`，不包含已单独跟踪的 logger doctest。
原有结构预算和 `-D warnings` 不豁免；普通失败继续执行余下检查并汇总原退出码，
中断立即停止；检查不改写源文件。CI 仍仅手动触发，Windows 独立平台检查保留。

工具正反例 A/B 必须使用同一隔离仓库、相同无副作用工具替身和输入：默认快速
层、完整层、多个失败、中断、未知/冲突参数及路径含空格。A 未有的新入口明确
记录为不支持；B 必须实际调度正确的 workspace/feature argv，不能用打印计划
冒充执行。真实仓库的各个检查再逐项记录，不以替身通过代替真实 Cargo。

本批采用门槛是覆盖与失败汇总准确；真实结构/lint 的已有源代码问题仍返回
非零并列明。路线图已明确修完结构红项不是本批前置。本批不能以“工具采用”
宣称仓库整体全绿，也不能宣称运行时或内存收益。业务源码 hash 必须与冻结
基线一致；失败候选只撤回本批文件，不影响已采用 P1 或既有分析文档。

证据目录：`memory-benchmark-results/p7-73-20261003/`。基线源码 tar、源码/锁/
工具链摘要、正反例输入与原始输出、真实 quick/full 逐项结果和最终决定均保留。
## 实测与决定：采用工具改动

工具 A/B 为 13 组正反例、26 次独立执行，全部通过。默认八项的 argv 与行为
保持一致；旧 A 不支持的新参数如实返回 2，B 的完整层实际执行十项。多项失败
保留各自退出码并继续；INT/TERM 立即停止；依赖缺失、冲突参数、含空格路径与
源码不改写均通过。初次脚本测试有一项 fixture 失败：替身 `dirname` 未消费
`--`，导致未到达缺少 `rg` 的预期检查；只修正测试替身，保留原始失败日志。
修正后的脚本测试为 77 passed。

真实入口结果如下；两种入口都准确返回 1，未把已有红项豁免成成功。

| 检查 | full | quick |
| --- | --- | --- |
| fmt、UI policy、i18n、脚本测试、diff | 通过 | 通过 |
| 结构预算 | 失败，退出 1 | 失败，退出 1 |
| workspace 默认全目标 check | 通过 | 不适用；默认 `cargo check --locked` 通过 |
| workspace 单元/集成测试 | 878 passed、0 failed、4 ignored，30 个测试组 | 根库 715 passed、0 failed、2 ignored |
| 严格 workspace Clippy | 失败，退出 101 | 不适用 |
| 根应用无 AI 全目标 check | 通过 | 不适用 |

结构失败仍为 root Slint 3084/3000、属性 422/410、PR actions 717/580。
Clippy 在 `crates/usher-i18n/src/lib.rs:89` 的既有 `unwrap_or_else` 被
`unnecessary_lazy_evaluations` 拒绝；检查在该 crate 停止，不能声称其余 crate
已通过严格 lint。538 个业务/Material 源文件与本批冻结基线逐字节相同，无新增
业务源码，故这些失败未由本批引入。未借本批修改 i18n 或提高结构预算。

根应用默认 cargo tree 含 `usher-ai-review`，关闭 AI 的根应用 tree 不含它；
该证据与 workspace 主动选中独立 AI member 的默认全成员检查分别记录。
`cargo fmt --all`、默认 check、根库 test 和 diff 检查已实际执行并通过。
完整 logger doctest 未执行，仍单独作为已知基线跟踪。工具批次没有运行时改动，
未运行原生/UI 性能测量，也不宣称内存或速度收益。手动 CI 触发策略未改变。

决定：采用七个工具/CI/对应文档文件的改动。采用依据为检查覆盖与准确失败报告，
不是仓库全绿；quick 为 7 pass/1 fail，full 为 8 pass/2 fail。保留原有 P1 和
此前分析文档改动。证据含 `tooling-ab/`、`73-full.log`、`73-quick.log`、
`business-source-equality.json`、源码/工具链摘要和最终决定。
