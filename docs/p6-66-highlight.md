# P6-B #66: PR highlighting / PR 高亮调度

后续只修 loading 回写的独立尝试已通过验收，见
[p6-66-correctness.md](p6-66-correctness.md)。下文仍保留宽范围候选的拒绝结论。

日期：2026-09-16；基线 `47b0892`，起始工作区干净。

## 被拒绝的候选实现与边界

本节描述候选快照，不是当前生产实现。正式 A/B 的最终 footprint 门槛失败，
已撤回本批运行时改动，仅保留显式开启的复现夹具和检查脚本。

`controller/highlight.rs` 收拢 Diff/完整源码高亮的调度与回写，复用共享
`LatestCompute`，一个活动计算、一个最新输入、一个输出。缓存命中、延迟文件、
reset/close 也取消。完整源码行投影移到 blocking pool；源码逐行、语法逐行及
Vue hunk 推断协作取消。资源初始化不持有 UI 清理锁，在锁内校验取消后才发布。
结果先验证 generation、index/path、Diff/全文模式，再清 loading、写 cache/model。
未改变 Slint 几何、token 样式或同文件滚动锚点。

取消不等于硬中断：单行 syntect 与库资源加载仍是不可分的调用；本批不声称对病态
超长单行有硬时限。保留现有语法失败文案与语义 catalog，不新增 root 属性。

## 自测

高亮筛选测试 15 通过，含新增的旧结果不能清 loading（英文/中文）、最新请求数据
替换、关闭后不能写回 cache、取消资源初始化、完整源码/Vue 扫描中途退出。
完整库测试 618 通过、0 失败、2 忽略，73.15s；check、fmt、UI policy、i18n 通过。
脚本测试当时 26 通过。基线构建前夹具首次 check 发现两个领域结构未实现
Default，已显式填写字段后重新构建，没有为夹具改动领域类型。

## A/B 协议

基线源码快照包含 HEAD 与双方一致的 `controller/highlight_benchmark.rs`，不包含
调度改动。正式 `cargo build --release --locked`，生产默认 features、FemtoVG、
2 个 Tokio worker、1100x700、离线隔离状态。三对新进程，AB/BA/AB，每进程 30 秒，
`caffeinate -di` 固定空闲显示状态。采样 100ms RSS/CPU、系统 RSS 高水位、footprint、
50ms UI 心跳、最终正确 model 的完成延迟及正常退出。

夹具通过真实控制器完成三轮：8 文件快速选择（7x12000 行、最终 3000 行）；
Diff/完整源码交替（最终 3001 行）；执行中关闭、重新安装会话并选择最终文件。
只在路径、行数、全文模式及 loading 同时正确时记录完成；不会仅凭 loading 判断。

测前门槛沿用 #65：内存峰值/稳定及 footprint 增量 <= max(8MiB, 5%A)，稳定段
增长及增长差 <=8MiB，心跳 p95 增量 <= max(8ms,10%A)，周期中位数增量
<=max(100ms,20%A)。六进程必须完整、正常退出、样本齐全、零 framebuffer 错误；
B 不能出现 premature_loading_clear。原始证据目录：
`memory-benchmark-results/p6-66-highlight-20260916/`。未运行长时间 soak/Windows。

## 正式结果与拒绝结论

三对进程全部完成、正常退出、样本齐全、零 framebuffer 错误。表内为三进程中位数：

| 指标 | A | B |
| --- | ---: | ---: |
| 采样峰值 RSS MiB | 414.44 | 149.59 |
| 系统生命周期峰值 RSS MiB | 414.44 | 149.62 |
| 稳定 RSS MiB | 413.98 | 148.95 |
| 阶段增长 MiB | 76.16 | -0.59 |
| footprint 峰值 MiB | 389.00 | 160.58 |
| 最终 footprint MiB | 125.69 | 140.22 |
| UI heartbeat p95 ms | 51.170 | 51.183 |
| 每进程周期中位数的中位数 ms | 2909 | 1102 |

A 的过早清 loading 计数为 64/78/83，B 均为 0。RSS、峰值 footprint、周期及
正确性改善，但最终 footprint 增加 14.53 MiB，超过原 8 MiB 门槛；汇总
`valid=true, passed=false`，**本批候选拒绝，不进入 #67 基线**，门槛没有放宽。

关闭后的 malloc in-use 约 A 13.84–13.93 MiB、B 13.66–13.76 MiB；B internal
为 62.39–64.88 MiB，A 两轮约 50 MiB，另一次 184 MiB，且 A reusable 页明显更多。
证据指向分配器保留/回收差异，而非已确认的活动对象泄漏；本批没有用全局强制回收
或增大阈值掩盖回退。后续应独立验证高亮资源/结果释放位置与分配碎片，再重做相同
自测和 A/B。单独的 loading 正确性修复也应建立自己的验收，不视为本轮已交付。

证据目录含 `ab/summary.json`、六份进程日志/采样、`usher-A/B`、
`candidate.patch` 和 `candidate-source/`（含新增高亮模块及测试）。保留复现夹具
`controller/highlight_benchmark.rs`。A 二进制 SHA256 为
`202374516bfb7430c198f36479f9ebf5120dce19c54604ece98f4abe49891f7d`，B 为
`785d9c0fcdace7ee7b5d644512d3529b9e2b68ead861985ca0b393e26f116883`。
