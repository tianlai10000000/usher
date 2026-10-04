# P7 #74：失败最新状态的显式重试

## 预先固定的验收边界

A 为 HEAD `de643e4` 的冻结源码；B 只叠加 #74。两侧使用同一
`Cargo.lock`、Release policy 和显式 `test-fixtures`，FemtoVG、两个 Tokio
worker、离线独立状态目录及 `winit-background-v1` 非激活窗口。

正确性 A/B 直接使用真实 `LatestPersistence`：注入写失败，再恢复介质并请求
同值保存；记录业务 revision、写入次数、flush 结果及重载结果。B 必须在
不增加业务 revision 的情况下完成重试。还须证明旧失败 A 不覆盖新 B、最多
一个活动和一个待写状态、重复 flush 不忙循环、并发 retry/flush/shutdown
等待正确尝试，以及关闭后拒绝提交。错误不能通过伪 Action 掩盖。

原生回归复用 P5 #59 的四轮慢写、偏好保存和 Inbox 投递，三对独立进程
AB/BA/AB，每进程至少 30 秒；使用原脚本门槛：peak/settled RSS 增量不超过
`max(8 MiB, A 中位数的 5%)`，B growth 和 growth delta 不超过 8 MiB，
每轮落盘版本为 501/1002/1503/2004，fixture 完成且八秒内正常退出。
此回归证明该固定场景成本不回退，不代表新的 Session 或原生通知故障验收。
测量期间不并行编译或运行测试，不据此宣称普遍运行时收益。

## 实施与结果

**采用。** 业务 revision 与写入 attempt 分离；失败的最新状态以共享 `Arc`
保持 dirty，显式 `retry_failed()` 只安排一次尝试。Hook 配置保存/开关与
AI Agent Settings 显式保存请求恢复，普通 flush/shutdown 不自动重试。
旧失败回调即使 dispatch 新 B，也不覆盖 B；关闭后拒绝重试。

原始证据：`memory-benchmark-results/p7-74-20261003/`，包含冻结 A/B
二进制及 hash、代码差异、组件源码/二进制/六次输出、构建与测试日志、
六个原生进程的日志/采样/最终文件和汇总。A 从 `git archive de643e4`
构建；B 在同一构建路径只替换本批六个源文件，避免路径变化引起不同编译图。

组件 Release A/B 六次全部完成：两侧 1,001 次 64 KiB 提交只保留一个
待写内容 owner，最终写入 `[1, 1001]`；100 次失败 flush 不产生新写入。
恢复介质后，A 同 revision 恢复为 false、总计一次写入；B 为 true、两次写入。
两侧旧 A/新 B 最终写入 `[1, 2]`。组件对照只证明行为，不声称延迟或 RSS 收益。

原生窗口日志核对为 800×600 logical、scale 2、Home、FemtoVG；六次均确认
`active=false/cursor_hittest=false`。原 P5 #59 门禁通过：

| 中位数 | A | B |
| --- | ---: | ---: |
| Peak RSS MiB | 123.625 | 124.016 |
| Settled RSS MiB | 123.141 | 123.547 |
| RSS growth MiB | -0.359 | -0.359 |
| Peak footprint MiB（诊断日志） | 97.410 | 97.860 |

六次均完成四轮写盘/偏好/Inbox 校验，落盘版本均为
501/1002/1503/2004；每次五次偏好写入，候选最大退出耗时 148.340 ms。
本批证明所测场景成本不回退，没有一般内存优化收益结论。

实际检查：`cargo fmt --all`、`cargo check --locked`、
`cargo test -p usher-state-store --locked`（18 passed）、
`cargo test --lib --locked`（701 passed、0 failed、2 ignored）、
`git diff --check` 均通过。首次沙箱根库测试 10 项因为本机 bind 被拒绝失败，
允许回环测试服务器后完整重跑通过；原失败日志保留。
结构检查仍为原有四项超限，本批未新增超限、未提高预算。

未运行全 workspace/Clippy、关闭 AI feature、Windows、logger doctest、
完整 P6 六场景及 15 分钟持续负载，未补签这些验收。
