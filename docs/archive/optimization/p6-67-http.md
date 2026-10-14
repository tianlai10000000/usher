# P6-B #67: HTTP response budgets / HTTP 响应预算

日期：2026-09-16。#66 候选因最终 footprint 超限已撤回；性能基线为 `47b0892`
（已验收 #65 状态）加双方相同的显式夹具，不包含被拒绝的高亮调度优化。

## 实现与边界

共享 `RequestOptions::response_byte_limit` 按真实 chunk 计账，在返回 chunk 或扩展
缓冲前拒绝超限；`bytes/text/json/chunk/for_each_chunk` 共用入口。零只允许空正文，
Content-Length 不作为可信依据。`ResponseTooLarge` 为非瞬态错误，HTTP 层不自动重试，
不包含正文；失败状态保持，连续读取或切换 reader 不能变成成功，并仅反馈一次。
header 后的 Response 预算只能收紧且保留之前计账。缓冲增长请求不超过输入预算，
避免 Vec 常规翻倍超过上限。非 2xx 错误正文仅收集 8,000 字节前缀再沿用字符截断。

接入远程 Logo 8 MiB（复用本地源预算）、AI 模型列表及 completion JSON 16 MiB（含请求流式
却返回 JSON）、GitLab raw 源码 8 MiB、GitHub/Gitee Base64 JSON 12 MiB。
`src/git/source.rs` 统一后两家解码，归一化 Base64 输入与解码目标均有界，源码最多
8 MiB。SSE 与更新包不套用小响应限制，既有 parser/截止时间/取消策略保持。
Logo driver 仍有自身最多三次尝试与退避；每一次下载都执行同一字节限制。

预算是每请求 payload 上限，不是进程总 RSS 或全局并发预算。传输 chunk 内部分配、
JSON 对象、文本有损转换、图像内嵌资源及消费者仍有额外成本。超过预算的合法大文件
也会失败；后续如需大文件，应该独立设计流式/分页能力，不能直接撤掉预算。

## 自测

- 首轮 `cargo test --lib --locked budget`：13 通过，其中 HTTP 新增 5 项、源码 1 项。
- 初次完整库测试（当时含 #66 候选测试）：626 通过、0 失败、2 忽略，72.87s。
  #66 撤回后复验 622 通过；补齐 `/models` 后最终工作区 623 通过、0 失败、2 忽略，
  75.67s。包含 AI 新增三项真实回环测试：模型列表、JSON 及请求流式后的 JSON
  回退超限，正常 JSON/SSE 完成。
- `cargo check --locked`、fmt、UI policy、i18n、diff 检查通过；脚本回归 27 通过。
- 全仓直接 reqwest client 检索仅命中 `src/http/mod.rs`。
- 结构检查仍为 AI controller 3876/3850、root Slint 3046/3000、root properties
  421/410、PR actions 688/580，数值未增加；未提高门槛。
- Debug 桌面冒烟：3 次超大响应拒绝、6 次正常响应成功，3 轮完成并正常退出，
  零 framebuffer 错误。Debug RSS/增长不作为正式性能结论。

## 正式 A/B 协议

生产默认 features 的 `cargo build --release --locked`，相同 `47b0892` 基线与
`src/project/http_benchmark.rs`。仅显式 batch=67、offline、隔离 state root 同时
开启时运行。1100x700、FemtoVG、2 个 Tokio worker，`caffeinate -di`，AB/BA/AB，
每侧 3 个新进程、每进程 30s，测量时不并行编译/测试。

夹具通过实际远程 Logo 下载函数，回环服务逐块生成响应，避免服务端同时持有完整
大正文。每轮小 PNG -> 无 Content-Length 的 64 MiB chunked 正文 -> 小 PNG，
共 3 轮。B 必须拒绝全部超大正文，同时保留全部 6 次正常下载。计账日志只记录
大小与结果，不记录正文。夹具任务在共享 runtime 执行，每次关闭其本地服务任务。

沿用 #65/#66 门槛：RSS 峰值/稳定及 footprint 增量 <=max(8MiB,5%A)，B 稳定段
增长及增长差 <=8MiB，UI 心跳 p95 增量 <=max(8ms,10%A)，周期中位数增量
<=max(100ms,20%A)；六进程完成/正常退出/样本齐全/零 framebuffer 错误。
周期含大响应早停，不能解读为正常小响应吞吐提高。原始证据：
`memory-benchmark-results/p6-67-http-20260916/`。

## 正式结果与采用结论

三对 AB/BA/AB 全部完成，六个进程正常退出、样本齐全、零 framebuffer 错误，
`valid=true, passed=true`。表内为三进程中位数：

| 指标 | A | B |
| --- | ---: | ---: |
| 采样峰值 RSS MiB | 374.42 | 142.59 |
| 系统生命周期峰值 RSS MiB | 374.45 | 142.62 |
| 稳定 RSS MiB | 373.98 | 142.00 |
| 阶段增长 MiB | -0.44 | -0.45 |
| footprint 峰值 MiB | 379.47 | 147.11 |
| 最终 footprint MiB | 379.00 | 146.49 |
| UI heartbeat p95 ms | 52.179 | 52.188 |
| 每进程周期中位数的中位数 ms | 2545 | 320 |

B 每个进程均拒绝 3 次超大响应且成功下载 6 次正常 PNG，总计 9 次拒绝、
18 次成功；A 同样完成 18 次正常下载，但还下载并缓存了 9 次 64 MiB 正文。
**采用第 67 批实现**。收益限于本批大响应夹具，不外推为普通请求或全应用同幅度
RSS 改善；周期缩短主要来自按预算早停。

构建源复制时保留旧时间戳曾导致 Cargo 跳过候选重编译，已在测量前更新复制文件
时间戳并完整重建；补齐 `/models` 后又构建最终候选。正式测量使用不同二进制：
A SHA256 `078202faac5a79e2ddb0b772f9363783d4730d1b622128fb91e125ceb148c7e6`，
B SHA256 `7bcfb78e5a2d0727813f2c5c0b2baa275207504f09a5e00b7e917be93960365e`。
双方下载夹具 SHA256 相同：
`ad2109f093834ca079cb6329f8200994e180384163d9d54874345202f1f43df6`。
证据目录含 `ab/metadata.json`、`ab/summary.json`、逐进程日志/采样、A/B 二进制、
候选 patch/source 及构建/测试日志。

未执行 Windows、长时 soak、真实外部 Provider、更新包安装或完整 doctest。
既有 compact-unwind 链接警告仍存在；已知 logger doctest 不属于本轮库测试。
起始工作区干净，本轮未覆盖任何用户既存未提交改动。
