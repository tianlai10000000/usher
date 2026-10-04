# P7 #75：消息最新状态持久化与独立通知

## 预先固定的验收边界

A 为已采用 #74，B 只叠加 #75。先用真实 MessageCenter 核心的受控 Release
组件对照，再用同一 P5 #59 原生回归检查整应用成本。组件无 Slint 窗口，不能
把其队列 payload bytes 等同整应用 RSS；原生回归也不能替代慢 MessageCenter
Writer/Notifier 的专项证据。

组件固定 64 条 × 2 KiB 消息和 1,000 次已读变更；首写使用确定性 barrier。
记录实际持久化排队 owner/UTF-8 payload bytes、写入次数、flush outcome 和
最终重载。分别阻塞 Notifier，验证持久化 flush 不被拖住，已准入原生事件
按 FIFO 投递；原生队列包括在途消息，最多 64 条/1 MiB UTF-8 payload，
单条最多 128 KiB，满载记录拒绝且 Inbox 发布/持久化仍成功。
注入写失败、恢复、同 revision retry，B 必须报告真实失败再正确重载。
两侧使用同一固定依赖与 Release policy，三对独立进程 AB/BA/AB。
基线仅加入相同故障注入 seam 与计数，不修复排队/flush 行为；保留其源码差异。

原生对照沿用 #74 的隔离、离线、两个 Worker、非激活窗口、30 秒 × 三对
AB/BA/AB 与 P5 #59 原门槛：Peak/Settled RSS 增量不超过
`max(8 MiB, A 中位数的 5%)`，B growth/growth delta 不超过 8 MiB，
完整四轮落盘/UI/偏好校验且八秒内正常退出。测量时不并行编译或测试。

## 实施与结果

**采用，保留代码。**单测、组件与原生对照均通过。原始证据路径：
`memory-benchmark-results/p7-75-20261003/`。关闭仍由兼容同步 façade
完成真实排空，移出事件循环属于 #77，不提前宣称已完成该生命周期批次。

### 已完成的组件对照

三对独立进程 AB/BA/AB 全部断言通过。每侧均真实读取/序列化/原子写入，
两侧统一冻结 registry 版本/checksum、同一 fixture lock、`opt-level=z`、LTO、
单 codegen unit、panic=abort、strip。B-final 的五份生产源 SHA 与候选一致。

| 指标（三次均相同） | A | B |
| --- | ---: | ---: |
| 64 × 2 KiB + 1,000 已读变更的实际写入次数 | 1,001 | 2 |
| 持久快照峰值 owner | 1,001 份私有 Vec（不含当前 Store） | 2 个共享 State Arc（含当前 Store） |
| 对应 UTF-8 payload bytes | 133,957,824 | 267,648 |
| 首次注入写失败的 flush | 错报成功 | 返回实际错误 |
| 同 revision 介质恢复后的最终重载 | 未恢复 | retry 后恢复，revision 不变 |
| 慢通知：100 条 compact 通知准入 | 100 | 64，拒绝 36 |
| 慢通知：20 条大 metadata 准入 | 20 | 12，拒绝 8 |

B 大 metadata 在途峰值为 1,004,796 bytes，低于 1 MiB；
单条 131,116 bytes 的私有 seam 被拒绝（该项不经过 Inbox normalization，
不作 Inbox 发布声明）。两种通知压力下，已准入事件 FIFO 完整，所有 Inbox
发布/最终重载成功，B durable flush 在 notifier 被阻塞时完成。
owner 口径不同已如表注明，payload 不含 Rust 对象、allocator、序列化缓冲或 RSS；
不据此宣称应用 RSS 降幅或一般场景耗时收益。

`cargo test -p usher-messages --lib --locked`：27 passed（17 项新增专项）；
`cargo test --lib --locked`：701 passed、0 failed、2 ignored；
`cargo fmt --all`、`cargo check --locked`、`git diff --check` 通过。
组件派生 lock 首次 `--locked` metadata 因依赖 closure 需裁剪失败，未构建该输入；
离线刷新 A 后核对每个 registry 版本/checksum 与仓库锁一致，冻结同份 lock 给 B，
两侧重新 `--locked` 验证及 Release 构建通过。失败准备日志保留，不计为产品回归。
独立审查发现的初版订阅者关闭重入死锁已修正并补回归，最终 B 不含该初版。

### 原生对照与决定

同一固定构建路径编译，冻结两侧实际 Release 二进制/hash/source diff。
六次窗口均为 Home、800×600 logical、scale 2、FemtoVG，
确认 active=false/cursor_hittest=false，无 framebuffer 错误。预设 P5 #59 门禁通过：

| 中位数 | A（已采用 #74） | B（#74+#75） |
| --- | ---: | ---: |
| Peak RSS MiB | 123.438 | 114.906 |
| Settled RSS MiB | 122.984 | 114.453 |
| RSS growth MiB | -0.359 | -0.359 |
| Peak footprint MiB | 97.330 | 92.780 |

六次均完成四轮，版本 501/1002/1503/2004、每次五次 Settings 写入，
Inbox UI 更新与终值检查成功，候选最长退出 146.618 ms。
这证明该受控场景的成本通过且 RSS 下降，不推广为所有消息量/OS 通知场景收益。
组件直接测试 MessageCenter Writer/Notifier；P5 原生压力的慢写对象是共享
Store fixture，不能把两组证据混称原生慢 MessageCenter/真实 OS 投递测试。

未运行完整 workspace/Clippy、关闭 AI feature、Windows、logger doctest、
完整 P6 六场景/15 分钟长跑或真实 OS 慢通知。现有 Hooks 未使用 import 警告保留。
结构预算未放宽；#75 未触及现有四项超限文件。未完成矩阵不冒充通过。
