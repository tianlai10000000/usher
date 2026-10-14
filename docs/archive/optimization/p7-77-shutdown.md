# P7 #77：后台关闭屏障与真实失败结果

**最终决定：拒绝；#77 全部代码已撤回，现行代码为已采用 #74+#75+#76。**
本文保留候选设计、失败证据和后续验收要求，不是当前架构契约。

## 预先固定的验收边界

基线 A 为已采用 #74+#75+#76，候选 B 仅叠加 #77。以下为测量前固定的候选要求。
既有正常关闭顺序和优先级保留：原 Slint 线程短停止准入/Timer →
后台等待全部已接受的权威 producer → 后台关闭持久化 writer →
原线程 UI/AppKit 清理 → 后台日志尾部 → 原线程 helper 清理 →
event-loop 返回及 MainWindow 释放确认 → 仅成功时移除 crash sentinel。
各阶段保持 HIGH 优先、同级后注册先执行，共享一个 registration order。

first-frame 尚未发生时也必须履行已存在的 Project/Message/AI 启动恢复义务。
用户工作先登记实际句柄才允许首次 poll；取消 waiter 不能遗失实际 blocking IO。
AI/PR 已接受的终态 Message/Hook 发布在下游准入关闭之前完成。
只停止新外部操作，不能拒绝已有 producer 合法的最后一次提交。
派生展示/Logo/cache 取消不冒充权威写盘或进程回收屏障。

请求、信号、panic 或自然 event-loop 返回的第一步启动同一个绝对总 deadline，
先于日志、registry 锁与回调；阶段超时保持失败并等待仍拥有的实际句柄，
不能让未结束的 producer 穿过 writer close。总时限到则独立 watchdog
非零退出，不争抢 logger 锁；失败、超时、UI dispatch 失败及 Release abort
都保留 sentinel。重复请求共享真实结果，不把取空 registry 算成功。

## A/B 与采用门槛

先执行真实组件正反例：owned waiter 取消/超时、JoinError/panic、准入关闭前后、
Project 初始恢复与 CRUD/Reload 串行事务、Message 首帧前 Stored+Live 合并、
AI 用户提交/终态 publisher、Hook/process 回收与最终写盘。
基线缺少的新 API 明确标记 unsupported，不能计为 A 的产品失败。

原生 Release 使用实际应用入口与相同故障 seam；A 的隔离源码只加入观测/注入，
保留旧退出实现及错误处理。两侧必须先证明同一真实目标接受输入、进入故障，
再比较正常、慢写、写入失败、阶段超时、永久阻塞、UI stop 卡住、Release panic、
首帧前消息、迟到 AI/Hook 发布、logger 锁阻塞与自然 event-loop 返回。
Project 另核对 accepted-before-firstpoll、初始读失败及恢复交错。
真实文件 reload、进程状态/退出码、sentinel、线程与阶段原始记录是判定依据；
外部强杀、注入未进入、准备错误不能当候选 watchdog 成功。

最后串行三对 AB/BA/AB 的 P5 #59 原生回归，每进程 30 秒、两个 Worker、
非激活 800×600/scale 2 Home；Peak/Settled 增量 ≤ max(8 MiB, A 的 5%)，
B growth 及两侧 growth 差 ≤8 MiB，完整 fixture 与八秒内正常退出。
不宣称关闭重构带来 RSS 或启动收益。测量期间不并行编译/测试。

证据目录为 `memory-benchmark-results/p7-77-20261003/`；需保留 A 注入补丁、
B 差异、源码/二进制/锁/依赖/runner hash 与原始记录。实际执行结果、未执行项和
最终采用/撤回决定在验证后补充；当前设计与已编写测试不算通过证据。

### 跨批次恢复门槛（原生测量前补充）

#74 已采用的同值显式重试契约必须在应用退出入口保持成立：实际自动写入与
首次显式 Save 均返回 EIO 后，解除故障，再用同值 replace 和实际 Save callback
重试。仅当业务 revision 未增加、原 UI 成功反馈已发生、实际原子替换已返回且
TOML 全字段重载一致，才请求正常退出。此时退出码应为 0，sentinel 应删除。
历史故障仍留日志；尚未恢复的写入、Join/panic 或未完成工作仍须报告失败。
不能用其他 target 的成功、空 hydrate 或任意一个成功操作清掉未恢复义务。

该场景按 AB/BA/AB 三对执行；两侧使用相同 facade、根 callback、实际保存反馈
观察点、源输入和 rename 注入。故障未进入、少于两次真实 EIO、缺失成功反馈、
revision 或磁盘证明、外部强杀均不是有效恢复对照。

## 实际结果与拒绝原因

相同固定源码目录、Cargo.lock、正式 Release profile（opt-level z、LTO、
codegen-units 1、panic abort）、test-fixtures、两个 Tokio Worker、离线状态和
非激活 winit-femtovg 窗口。A 只加入与 B 完全相同的实际输入和反馈观测；
A 原有关闭顺序、错误处理与退出政策未修复。输入不含凭据，不使用替代 saver。

三对序列为 AB/BA/AB。六个进程均先发生两次真实原子 rename EIO；首次实际
Save 反馈失败后才解除故障。第二次同值 replace 的业务 revision 不变，真实
Save 成功反馈已发生，实际 TOML 的五个字段完整重载一致，然后才发 SIGTERM。
三对共同输入 hash 一致，所有实际输入及恢复 receipt 有效，无外部强杀。

| 对照 | A 退出码 / sentinel | B 退出码 / sentinel | 恢复后的实际磁盘配置 |
| --- | --- | --- | --- |
| 第 1 对 AB | 0 / 已删除 | 1 / 保留 | 两侧正确 |
| 第 2 对 BA | 0 / 已删除 | 1 / 保留 | 两侧正确 |
| 第 3 对 AB | 0 / 已删除 | 1 / 保留 | 两侧正确 |

每个 B 的真实阶段记录都显示：`settings_feedback` 的 producer drain 返回旧
Persistence EIO；之后 `flush_settings` 的真实 worker 正常关闭。最终仅因历史
错误跳过 success finalizer。候选 `SettingsFeedback` 把失败返回给通用
`OwnedTasks`；后者只累加失败 count，后续成功不记录同资源恢复。因此 #74
底层重试并未失效，#77 上层却把已完整恢复的正常退出误判为失败。这是明确的
应用行为回归，不能通过放宽退出期望或删除故障记录制造通过。

Project 的初始读失败后成功 Reload 也采用了历史失败常驻策略；现有候选组件
测试固化了该决定。它是此前设计判断需要修正的地方，独立复核已记录；本次
Settings 原生证据不冒充 Project 原生恢复 A/B。下一候选必须分开记录历史
诊断与当前尚未恢复义务，并用同资源、revision/attempt 与真实 durable receipt
确认恢复；另一 target 的成功、任意 Ok 或空 hydrate 不能清除失败，实际
Join/panic、未完成工作与超时仍必须失败。

### 已执行检查

| 检查 | 实际结果 |
| --- | --- |
| 候选 `cargo fmt --all`、默认 `cargo check --locked`、`git diff --check` | 通过 |
| 候选 `cargo test --lib --locked` | 778 passed、0 failed、2 ignored |
| StateStore 独立测试 / MessageCenter 独立测试 | 19 / 27 passed，均无失败 |
| 关闭默认 feature、仅 `renderer-femtovg` 编译；i18n catalog 检查 | 通过；不等同原生双语言交互矩阵 |
| 候选结构门禁 | 失败：旧 root Slint 3084/3000、属性 422/410；新增 SettingsFeedback 132/110；未抬预算 |
| 正式 Release A、B 构建 | 通过，分别 3m09s / 3m16s；不作为启动或运行时性能证据 |
| 最终注入工具正反例 / reader 工具正反例 | 5 / 4 项通过 |
| 实际恢复 Release AB/BA/AB | 输入与恢复证据 6/6 有效；B 正常退出契约 0/3 通过，拒绝 |
| 回退后的 `cargo fmt --all`、`cargo check --locked`、`git diff --check` | 通过 |
| 回退后的 `cargo test --lib --locked` | 715 passed、0 failed、2 ignored；34.42s |
| 回退后的结构门禁 | 仍失败三项既有债务：root Slint 3084/3000、属性 422/410、PR actions 717/580；新增 SettingsFeedback 超限已随候选撤回 |
| 回退后的 `cargo build --release --locked` | 通过，6m56s，3 项 unused 警告；已替换构建目录里的候选产物，不作为新 A/B 性能证据 |

迭代期编译错误及已修复的夹具准备错误留在原始日志，不计入正式 A/B。
一次 feature-off 命令误用了仓库不存在的 `ai-backend-none`，未执行编译；已按
实际 feature graph 改为 `renderer-femtovg` 并通过。库测试有自动审批连接中断，
命令未执行；在同一审批流程重试后通过，没有绕过审批或删除测试。

因恢复门槛确定失败，其余 12 项原生关闭场景、7 项 Project/Message 原生场景、
本批 P5 内存对照和 Terminal 原生回归不再执行；不是通过项。Release abort、
自然 loop 返回的原线程重入、Windows、完整 Terminal 手工矩阵、P6 全路由、
长时间 soak、完整 logger doctest 和 Clippy 均未作本批交付验证。#74–76 已有
独立通过证据仍保留，但不能替代 #77 的未执行项。不宣称本批运行时收益。

### 撤回与证据

按 `accepted-76-source.tar` 的不可变 hash 及逐文件清单，恢复 39 个既有源文件，
删除 14 个 #77 新增源文件，校验恢复内容与已采用基线逐字节一致。未使用
`git reset`、未恢复到初始 HEAD，#74–76 代码、双语架构/spec 和此前分析文档
保留；#77 只保留本历史记录及提案结论，没有局部候选进入最终代码。
在回退并执行全仓格式化之后，再次核验 460 个版本管理及未忽略的 src/crates/ui
文件均与已采用 #76 基线完全一致；无关的系统忽略文件保留。

原始证据位于 `memory-benchmark-results/p7-77-20261003/`（按仓库惯例忽略），
包括 `recovered-settings/comparison.json` 与六次输入/trace/stdout/stderr/TOML，
源码 tar、候选 diff、A 仅观测补丁、工具链/锁/profile/源码/二进制/注入 hash、
构建/测试日志、注入工具正反例和 `rollback-result.json`。完整候选 source 仍可
从历史快照复查，撤回后的现行文件行号不能用来引用已拒绝实现。
