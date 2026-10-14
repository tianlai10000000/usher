# 第 71 批：Hook 文案与错误语义

日期：2026-09-17。范围限定为 Hook 宿主错误、验证错误及相关展示边界；没有
为行数预算机械拆分 PR/AI controller。#72 的冻结二进制保留作独立对照输入，
其普通生产行为仍基于 `b5f2f23`；本批不能覆盖或宣称解决 #72 的未完成长跑。

## 实现

- `errors.rs` 定义宿主失败和表单验证类别。运行摘要新增可选 `failure_kind`，
  持久版本比较包括此字段；旧 JSON 可读取，未知未来类别安全降级。
- `presentation.rs` 在展示边界映射状态、错误、退出码及信号。Settings 历史
  不再显示 `start_failed/timed_out` 等协议值；Job 和测试反馈不再把 OS 错误
  或脚本 stderr 当成宿主文案。原始诊断和外部输出仍保留日志原文。
- 历史、测试结果、表单错误、Job 摘要/触发事件/步骤及过期链接 Toast 即时
  随语言重投影；异步 Job 投影携带语义文本，语言切换不重建日志 model。
- 新 Hook 通知持久化带版本的语义元数据，在 Inbox/Banner/系统通知发送时解析。
  已挂载 Banner 可切换语言；旧通知和外部通知保持原样，已发出的系统通知不重发。
- 所有新增文案进入共享 catalog，源语言和简体中文齐全。配置操作的未分类技术
  错误使用友好通用提示并记诊断日志，不通过任意英文字符串猜测错误类别。

## 自测

`memory-benchmark-results/p6-71-hook-semantics-20260917/` 保存证据。
完整库测试 **648 通过、0 失败、2 忽略**，i18n crate **7 通过**，脚本 **37 通过**。
fmt、cargo check、UI policy、i18n coverage、diff 通过；结构门禁只有原有四项超限。
`cargo check --no-default-features --features renderer-femtovg --locked` 通过。
新增测试覆盖类型化错误、数值上下文、旧/未来 JSON、持久版本、已挂载双语言
历史/结果/Job/Banner/Toast、新草稿清理，以及语言切换保留日志 model 身份。

首次专项测试 43 通过、1 失败：新增测试在组件创建前选择 Slint bundle；修正
测试初始化顺序后完整检查通过。该失败日志保留，并非被忽略的产品失败。

## 独立 Release A/B

默认 Release 构建通过。A 为冻结的 `usher-72`，B 为本批 `usher-71`，B SHA-256：
`b552a98131230570abf4a673ab633ed9d63693e9c25becafc4c72580e94613c5`。
以 #69 Hook 输出/回放夹具运行三对 AB/BA/AB，每进程 90 秒；相同离线状态、
1100x700 窗口、FemtoVG 和两个 Tokio worker，采样期间未编译或运行测试。
`ab/metadata.json`、六份日志/采样及 `ab/summary.json` 保留完整证据。

| 中位数 | A | B |
| --- | ---: | ---: |
| 峰值 RSS MiB | 158.828 | 158.891 |
| 稳定 RSS MiB | 158.203 | 158.297 |
| RSS 后段增长 MiB | 0 | 0 |
| 峰值 footprint MiB | 172.19 | 172.66 |
| 最终 footprint MiB | 151.06 | 151.24 |
| 心跳 p95 ms | 52.136 | 52.168 |
| 单轮耗时 ms | 429 | 430 |

六个进程均完成三轮、正常退出，逐轮终态回放、配置共享和纯输出零写盘均通过。
原有相对/绝对门槛全部通过，没有放宽阈值。B 第三次出现分配器回收造成的
RSS 下跌，原始样本保留；不将它或组中位数波动解释为性能收益。

## 桌面与采用状态

独立离线数据目录、800x600 桌面窗口：先检查英文失败历史、Job 摘要及原始诊断，
在同一进程通过 Settings 切换到简体中文后再次打开同一记录。历史显示
“Could not start / 无法启动”，摘要显示各自语言的解释器启动失败提示，
触发事件及测试标记正确翻译，诊断路径保持原文，无文字重叠或持续闪烁。
截图在 `manual-en-job.png`、`manual-zh-history.png`、`manual-zh-job.png`；
`manual.log` 记录语言切换，应用退出码为 0。已挂载 Banner/Toast 与表单错误
的即时切换由上述自动化测试覆盖，未逐项做桌面重放。

**采用本批 Hook 语义改造：自测、独立 Release A/B 和上述桌面检查通过；不宣称
RSS 改善。** #72 的长跑取消及原重复性失败记录保持不变。Windows/macOS 原生
通知实际送达、Windows 执行及完整 doctest 不在本次已验证范围内；已知 logger
doctest 基线限制仍保留。起始工作区干净，本次没有覆盖其他用户改动，未提交 Git。
