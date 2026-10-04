# P7 #76：Ask / Session 真实提交屏障

## 预先固定的验收边界

A 为已采用 #74+#75，B 只叠加 #76。先使用真实 Session/atomic_write 的
受控 Release 组件交错夹具，三对独立进程 AB/BA/AB；再用同一 P5 #59
桌面协议确认整应用成本不回退。组件直接针对 Session 事务，桌面普通场景
不能替代取消/慢 Session 写入专项，不声明 RSS 或启动优化收益。

组件固定旧写 rename barrier、新 Ask 和实际重载：取消等待必须不能让新 Ask
释放准入后被迟到旧 whole-session 覆盖；用户提交必须在可取消网络工作之外
等实际 receipt；同一操作重试一次落盘，不同操作即使正文/时间相同仍保留。
旧 assistant attempt、已退休 task 的迟到输出必须拒绝。Review 结果提交必须
在串行事务内读/合并当前 chat；对账必须不能删除仍在执行的 atomic 临时文件。
通过门槛是已接受消息在最终 reload 中完整/不重复、陈旧响应拒绝且所有实际
已准入 IO 在 drain 前完成。不同 task 允许并行，不引入永久 per-task registry。

基线只加同样的可控时序/故障 seam，不修复原行为。固定生产源码/hash、
锁中 registry 版本/checksum、Release profile 与 runner；若准备夹具出错
须记录并排除，不能把夹具错误算作产品证据。

原生对照沿用 #75 的隔离、离线、非激活 Home 窗口、两个 Worker，
30 秒 × 三对 AB/BA/AB，遵守原 P5 #59 门槛：Peak/Settled RSS 增量
不超过 max(8 MiB, A 中位数的 5%)；B growth/growth delta 不超过 8 MiB，
完整四轮落盘/UI/偏好检查且八秒内正常退出。测量期间不并行编译/测试。

## 实施与结果

**采用，保留代码。** 证据目录：
`memory-benchmark-results/p7-76-20261003/`。
Session drain receipt 的边界为实际已准入 Session IO；AI controller 顶层
future 的终态 Task dispatch/Message publish/Hooks emit 屏障属于 #77，
本批不提前宣称已实现全应用退出排空。

### 已完成的组件与自测

三对 Release AB/BA/AB，七场景共 42 个独立进程记录。B 21 项全部通过；
A 的 overwrite/cleanup/retirement 三种真实缺陷各复现三次，promotion 两侧
兼容通过；identity 为新增契约比较，A 的 stale-assistant/drain 六个记录是
unsupported，单列且不计为通过。真实最终 JSON 独立核对用户条数/身份、
孤儿文件不存在、迟到 assistant 不出现，以及关闭前接受的用户实际保存。

B 的实际 blocking driver 与原子写入被延后时，Stop/drain 均等待真实完成；
对账保持活跃临时文件，放行后保存成功。稳定票据重试不重复，两个相同正文/
毫秒时间的提交保留两条独立 ID；20 个真实 IO 错误归集 failure_count=20、
最多 16 份有界失败样本。释放票据后 Weak task owner 活跃数为零。

两侧直接包含实际生产源码与原 Controller cancel helper，使用同一 toolchain、
Cargo.lock 与精确 Release serde/json/StateStore/Tokio/log rlib，B 仅使用项目
既有 rand；共同 opt-z/LTO/codegen1/panic-abort/strip/test-fixtures。
同一 interposer 二进制只在精确目标首次已 armed rename 处等待，并实际 renameat；
源码/二进制/依赖/hash/原始 stdout/stderr/最终 JSON 保留。受注入的耗时不作收益声明。

首次候选编译的遗漏参数、非 Send 锁作用域，以及收束旧入口时误删诊断方法
均已修正，最后诊断方法与 HEAD 逐字相同，初次失败日志保留。组件 B 首次准备
有一次 field/method 拼写错误，runner 有一次 Python 缺括号，均为夹具错误，
未冒充产品结果；修正后源码一致性与完整 42 项对照通过。
早期 Debug interposer 的 dlsym 递归崩溃同样只算夹具失败，正式证据使用
已修正的 renameat 版本，不混入该无效样本。

默认编译通过；AI Review 定向 44 项、领域 crate 22 项、应用根库
715 passed/0 failed/2 ignored。涉及陈旧终态与初始加载回写的三项 UI 操作
夹具调用真实 MainWindow 函数，英文/zh-CN 动态文案已覆盖；这些是人工延迟
闭包的单元验证，不是原生队列/完整 PR 路由手工验收。

### 桌面 Release A/B 与最终决定

最终文件布局重新执行完整 42 项组件对照，随后冻结十份候选源码/hash、
同锁 Release 二进制；原生六次运行全通过。A/B 的 Peak RSS 中位数为
114.953125 / 115.234375 MiB，Settled 为 114.453125 / 114.78125 MiB，
growth 均为 -0.359375 MiB；差值在预先固定门槛内。
phys_footprint_peak 中位数 92.78 / 93.20 MiB，B 最长退出 138.632 ms。
六次均为非激活 800×600、scale 2 窗口，四轮 revision
501/1002/1503/2004、五次偏好写入、Inbox UI 更新及正常退出完整，
framebuffer 错误为零。原始 result/samples/app.log、summary 与额外核对在
`native/`、`native-extra.json`；不把此短时 Home 场景解释为 Session 性能收益。

最终 `cargo fmt --all`、`cargo check --locked`、`cargo test --lib --locked`
（715 passed/0 failed/2 ignored）、`git diff --check` 均通过；
关闭默认 features 后的 `renderer-femtovg` 编译及 i18n 门禁通过。
结构门禁仍有根 Slint 两项与 PR actions 一项既有失败。候选初期 AI controller
超行数/函数数预算，经把四项新增会话测试及两项原有测试逐字移入专属测试模块，
最终 3843/3850 行通过，预算未提高。默认编译中三项待 #77 接入的 API
unused 警告已记录；feature-off 两项条件 import 警告保留。

当前证据支持采用真实 Session 提交与取消边界，未验证完整 PR 路由人工操作、
六路 P6 长场景、十五分钟 soak、Windows、完整 doctest 或 Clippy。
已声明的 logger doctest 基线未在本批重跑。#76 成为 #77 基线。
