# P6-B #65: Terminal bounded I/O / Terminal 有界 IO

日期：2026-09-16。基于 `209e5c6` 工作区及已完成的 #64；保留已有未提交改动。

结论：保留通知/取消版有界 IO 与异步退出屏障。最终自测通过，固定显示条件的
三对正式 Release A/B 通过预定门槛；RSS 基本持平，不能宣称显著降低。
首版吞吐失败和修订首次 footprint 失败均保留，详见下文。完整手工矩阵尚未闭合，
Windows 和长时间 soak 未验证；本次没有推进 #66/#67。

## 实现

- `session.rs` 使用 64 项有界输出队列，每次读取最多 8 KiB，队列容量为 512 KiB。
  此外存在固定读取缓冲、一个待入队块、一个 Bridge 在途块；解析器、scrollback、
  UI model 和内核 PTY 缓冲另行占用内存，不将队列预算等同于总 RSS。
- `io.rs` 统一输入准入和短写重试，按键、粘贴与协议应答共用一个 Writer。
  1 MiB/128 项上限包含正在写入的数据，整次接受或拒绝，不发送半次超预算粘贴。
  UI callback 不再同步执行 PTY write/flush。调用方原有的剪贴板 String 不在队列预算内。
- Writer 失败或协议应答无法入队时停止会话；普通用户输入拒绝通过现有语义
  `terminal.write_failed` 显示，下一次成功准入清除。新增一个 root bool 投影，
  通过 Slint catalog binding 支持已经挂载提示的即时语言切换。
- 关闭控制不依赖队列空位，先关闭输入准入、停止输出发布，Reader 继续排空
  内核 PTY，Child 回收后再停止并 join 两个 IO Worker。退出处理也移出展示锁。
  Unix 使用非阻塞 master；Windows 使用原生同步 IO 取消，尚未在 Windows 运行验证。
- 同项目重开、跨项目 Session ID 隔离、隐藏时继续解析、viewport 和 1,000 行
  scrollback 语义保持。Shared Runtime 继续拥有后台回收，既有 shutdown 优先级保持。

## 自测与发现

证据目录：`memory-benchmark-results/p6-65-terminal-20260916/`（Git 忽略）。

- Terminal 专项：19 通过、0 失败，包含输出背压的逐字节完整性、满队列取消、
  Receiver 断开、输入原子准入及在途计账、短写/Interrupted/WouldBlock 顺序、
  空闲 Writer 唤醒、真实 PTY 不读 stdin 的关闭，以及英中已挂载错误文案切换。
- 完整 `cargo test --lib --locked`：610 通过、0 失败、2 忽略，75.45 秒。
  测试使用允许本地 HTTP 监听的环境；保留原有 `__eh_frame` 链接警告。
- `cargo check --locked`、UI policy、i18n catalog 与 `git diff --check` 通过。
  Python A/B 驱动语法检查通过。
- 结构门禁仍失败，预算未提高：AI controller 3876/3850、PR actions 688/580，
  root Slint 从 3044 增至 3046/3000 行、property 从 420 增至 421/410。
  增量来自输入拒绝提示的必要根桥接，不能写成“原超限完全不变”。

本批测试先发现了两项问题并修复：

1. 首版关闭先停 Reader，再 wait Child，在持续输出且 stdin 不被读取的真实 PTY
   中超过 3 秒。定位到 Child wait，取消沙箱限制后仍复现。修改为关闭发布但继续
   排空内核 PTY，再回收 Reader/Writer 后，原测试通过；没有放宽超时断言。
2. 最初本地化测试未切换到 Project 路由，Drawer 没有挂载。修正测试和两侧相同的
   压测夹具入口后通过；早期未包含路由修正的 Release 构建不进入 A/B。

独立 Debug 应用完成三轮输出及同项目重开、A -> B -> A 流程。通过桌面工具确认
按键输入和 Drawer 全屏切换保留内容。桌面工具的 clipboard paste 超时，Unicode
键盘注入也未完整送达，故不宣称粘贴和中文手工验收通过；宽字符已有渲染单测。
窗口拖动后的工具截图不够可靠，窄宽 resize、选择复制和错误提示视觉矩阵仍需补验。
本地验收包最初被自动审批拒绝；用户要求重新申请后已获准打开，不再是权限阻塞。

## Release A/B 协议

使用 `scripts/benchmark-p6-terminal.py`，默认生产 features、正式 `--release`
（`opt-level=z`、LTO、单 codegen unit），固定 `winit-femtovg`、2 个 Tokio worker、
1100 x 700 逻辑窗口、独立离线配置。每侧三个新进程，顺序 AB/BA/AB，每次至少
30 秒。每轮运行 150,000 行固定文本输出，共三轮，覆盖同项目重开和跨项目切换。
UI 每 50ms 记录心跳，退出使用应用已有 SIGTERM 关闭协议。

基线包含 #64 及与候选相同的诊断夹具，不包含 #65 行为改动：

- A 二进制 SHA-256：`1c055d58a91c8ebc0277fff63eb4572ad407ca5934be07bc37f3a14ad8ce1535`。
- `Cargo.lock` SHA-256：`afcf0d528160625b7235a4e7e12c69b13727ed919b42cd76a1899cdbc19f1996`。
- 夹具 SHA-256：`98fad4e20f8c8a123b357cc3c383f1fad2fb42bb5b729822e71b81dbd8c21132`。

门槛在测量前确定：峰值/稳定 RSS 与 footprint 的 B-A 不超过 max(8 MiB, 5% A)；
B 的稳定段增长及增长差不超过 8 MiB；UI 心跳 p95 增量不超过 max(8ms, 10% A)。
各进程都必须完成三轮并正常退出，不能缺少 footprint/心跳样本或出现 framebuffer
错误。原始日志、100ms RSS/CPU 样本、逐进程结果、hash 和汇总都保留。

### 首版拒绝与修订

首版正式 Release A/B 已完成，原始证据保留在 `ab/`。RSS 峰值中位数
129.33 -> 128.69 MiB，稳定 RSS 129.08 -> 128.48 MiB，心跳 p95
51.151 -> 51.089ms，但固定输出用时从 A 约 300ms 增至 B 约 500–800ms。
旧驱动未检查吞吐，因此其 `passed: true` 不能作为采用结论：**首版拒绝**。
首版 B SHA-256：`c93f5d2cc486c97a32191dfde7803fd368a6ae46a4540ad5ecfd7dd43d4ba9d2`。

修订将满队列的 2ms sleep 轮询替换为 Crossbeam bounded channel 的容量通知与
独立取消通道 select；关闭时断开取消发送端，唤醒所有阻塞生产者，保持 Reader
排空内核的协议。直接声明已锁定的 `crossbeam-channel 0.5.16`，未升级其他依赖。
驱动增加吞吐门槛：逐进程三轮中位数，再取三进程中位数，B-A 不超过
max(100ms, 20% A)；100ms 对应夹具观察完成的粒度。缺失/无效周期不通过。
该门槛在修订测量前加入，首版原始汇总不改写。修订 Terminal 自测 19 通过，
脚本测试 25 通过，含内存与心跳良好但吞吐回退时必须失败的独立回归测试。
修订完整库测试 613 通过、0 失败、2 忽略（76.89 秒，包含保留的 Tag 改动）；
编译、UI policy、i18n、diff 检查通过，结构门禁仍为上述四项超限。
新驱动同时读取系统 RSS 生命周期高水位，并应用相同内存门槛，减少抽样漏峰风险。

补充桌面检查确认选择模式可以高亮回显文字；工具发送复制/粘贴快捷键时未正确
传递修饰键，搜索框只出现普通 `v`，故复制粘贴链路仍不能判定通过。
窗口边缘拖动未改变截图尺寸，也不作为 resize 验收证据。隔离 Debug 应用已关闭，
避免影响后续正式 A/B。

修订候选使用 `/private/tmp/usher-p6-65-candidate-v2/` 隔离快照：从基线复制，
仅覆盖 #65 Terminal、两项 UI 与 Cargo 清单，排除并行出现的 Tag/i18n 用户改动。
退出调用链复核又发现原 pre-shutdown 在 UI 线程直接 wait PTY，已改为只调度。
新增 `lifecycle.rs` 保存尚未完成的回收任务，共享异步退出阶段先拒绝新 Session，
再摘下活动 Session 并等待所有活动/已摘下 Session。保留原关闭优先级；新增测试
验证退出确实等待已摘下任务、等待期间不持有展示锁、重复排空可完成。
退出屏障最终版本：Terminal 20 项通过；完整库测试 614 通过、0 失败、2 忽略
（76.07 秒）；`cargo check --locked` 与 `git diff --check` 通过。

最终修订 Release 构建完成，增量编译/链接 4m27s（工具报告时间）；最终 B
SHA-256：`1776dda4caa5ca1aed38fbff8bc94ccf9644ebaacc819c921483eed54df645e7`。
修订 `Cargo.lock` SHA-256：`8a4f51dfe435ec4af220dd87823b44a1653ce7dbfbdc7b0f5a34f5236a6d2a51`。
新一轮原始数据目录为 `ab-v2/`；运行期间使用 `caffeinate -i` 防止空闲休眠，
结束自动释放，不修改系统持久设置。启动前已确认没有 Cargo/rustc 或手工验收
应用进程。

### 修订首轮结果与环境复核

`ab-v2/` 六进程均完整运行、正常退出，汇总 `valid=true, passed=false`：

| 指标（中位数） | A | B |
| --- | ---: | ---: |
| 输出周期 ms | 301 | 305 |
| 抽样 RSS 峰值 MiB | 125.41 | 125.03 |
| 系统 RSS 生命周期峰值 MiB | 125.44 | 125.06 |
| 稳定 RSS MiB | 125.39 | 124.81 |
| 心跳 p95 ms | 52.184 | 51.195 |
| footprint 峰值 MiB | 137.02 | 137.06 |
| 最终 footprint MiB | 124.08 | 136.74 |

吞吐回退已收敛，但最终 footprint 增量 12.66 MiB 超过 8 MiB 门槛，本轮不能作为采用依据。
核查原始日志发现 A-2 在 20s -> 25s 时 footprint 136.88 -> 124.22 MiB，
RSS 都为 125.39、malloc in-use 都为 13.88 MiB、线程数都为 20；A-3/B-3
又同时降至较低占用。该变化提示显示/合成器状态可能未保持一致，**尚非已证实根因**。
因此保留原数据不改写，以相同二进制、相同三对顺序和门槛做环境复核；仅将
临时休眠抑制从 `caffeinate -i` 改为 `-di`，覆盖显示器空闲休眠。
复核目录 `ab-v2-display-awake/`，不能仅凭 RSS 较小或一次成功掩盖本轮失败。

### 固定显示条件的最终结果

相同 A/B 二进制哈希、相同工作负载、相同 AB/BA/AB 三对与全部门槛，使用
`caffeinate -di`。六进程均完成三轮、正常退出、零 framebuffer 错误；汇总为
`valid=true, passed=true`，原始汇总与逐进程日志位于 `ab-v2-display-awake/`。

| 指标（中位数） | A | B |
| --- | ---: | ---: |
| 输出周期 ms | 299 | 306 |
| 抽样 RSS 峰值 MiB | 126.05 | 125.97 |
| 系统 RSS 生命周期峰值 MiB | 126.08 | 125.97 |
| 稳定 RSS MiB | 125.77 | 125.67 |
| 稳定段增长 MiB | -0.031 | -0.031 |
| 心跳 p95 ms | 52.178 | 52.178 |
| footprint 峰值 MiB | 137.14 | 136.84 |
| 最终 footprint MiB | 136.86 | 136.50 |
| 正常退出耗时 ms（观察项） | 663.0 | 143.4 |

采用结论限定于 macOS 此固定条件：消除首版吞吐回退，内存/响应门槛通过，
保留有界输入输出、不会等待 PTY 的 UI 输入路径、以及完整会话回收屏障。
RSS 差异很小，主要收益是积压上限与生命周期正确性。B 个别输出周期仍约
400ms，夹具完成观察粒度为 100ms，不将 7ms 汇总差解释为精确延迟差。
显示环境因素仍是推断，前次 footprint 失败不抹除；后续批次固定显示唤醒并
继续保留 RSS、系统高水位和 footprint 三种证据。

最终验证：`cargo fmt --all`、`cargo check --locked`、UI policy、i18n、脚本测试
25 项及 `git diff --check` 通过；`cargo test --lib --locked` 为 614 通过、
0 失败、2 忽略。原有链接器 `__eh_frame` 警告仍存在，结构门禁四项失败及新增
root 2 行/1 property 债务如上，未提高预算。保留已有 #64、Jobs 和 Tag/i18n 改动。
本批没有运行 15–30 分钟 soak、Windows、完整 doctest 或 workspace 全目标测试。
