# 已否决的内存实验

[English](memory-experiments.md) · [当前待办](../memory_optimization_backlog.zh.md)

以下 MEM-14–16 的日期记录在 2026-10-14 文档整理时从内存待办移入。
原实验条件、失败、测量、决策与证据位置均保留。实验基础设施的描述对应记录当时的
批次，不代表新的生产策略；活跃任务与采用门槛继续由当前待办维护。

<a id="mem-14"></a>

### MEM-14 — 重建非活动 Messages 视觉树

状态：候选已拒绝（2026-08-14）

候选保留独立生成的 Messages factory 根，但只在 `Route.Messages` 活动时挂载
ScrollView 与 Tooltip host；基线持续挂载这些视觉节点。两组都使用已有生命周期桥接：
退出时清空投影行 model，重新进入前完成 hydration；Store 订阅以及其他页面上的
Banner 和未读通知表面始终保持活动。

受控 A/B 使用当前持久化的 8 行 MessageCenter 快照、Release/FemtoVG、800×600
逻辑窗口与 2.00 缩放，两轮 Messages 进入/退出，每相位 5 个样本；每个 variant
运行 3 个隔离进程并按轮次交错。6 份日志全部验证了
`Home → Messages → Home → Messages → Home`、两次投影行为 0 的 inactive 生命周期
事件，以及第二次进入时 8 行 hydration。

| 指标 | 稳定视觉树基线 | 仅活动时挂载候选 | 候选 − 基线 |
| --- | ---: | ---: | ---: |
| 第 1 轮离开后稳定 RSS | 120.41 MiB | 121.23 MiB | +0.83 MiB (+0.7%) |
| 第 2 轮离开后稳定 RSS | 120.77 MiB | 121.47 MiB | +0.70 MiB (+0.6%) |
| 第 2 轮 RSS 高于启动值 | 9.88 MiB | 9.50 MiB | -0.38 MiB (-3.8%) |
| 第 2 轮离开后稳定 footprint | 112.56 MiB | 119.58 MiB | +7.02 MiB (+6.2%) |
| 第 2 轮 footprint 高于启动值 | 32.45 MiB | 38.56 MiB | +6.11 MiB (+18.8%) |
| 全程实测 RSS 峰值 | 120.94 MiB | 121.64 MiB | +0.70 MiB (+0.6%) |
| 全程实测 footprint 峰值 | 112.84 MiB | 120.58 MiB | +7.73 MiB (+6.9%) |

结论：**拒绝仅活动时挂载视觉树**。归一化 RSS 方向略有改善，但远低于默认
10 MiB/5% 门槛；3 组配对运行的第 2 轮离开后绝对 RSS 均更高，physical footprint
结果也混合且有两组明显回退。生产行为保留稳定 factory 视觉树与已有的离页行 model
释放。生命周期诊断继续保留，用于直接验证投影所有权。

证据位于 Git 忽略的
`memory-benchmark-results/messages-ui107-ab-20260814/`。冻结的 baseline/candidate
SHA-256 已记录在 `metadata.tsv`，同目录保留原始样本、逐轮摘要、完整日志与生成对比。
基准运行时工作区为脏状态，Preview、Chat 与 macOS 平台的无关修改均原样保留。

<a id="mem-15"></a>

### MEM-15 — Docked Preview 覆盖时卸载 AI Review response delegate

状态：候选已拒绝（2026-08-27）

候选保留 AI Review panel shell、composer、计时器、源 model 与交互状态，仅在 Docked
Preview 占用同一个 PR 侧边表面时条件销毁 formatted response、finding 与 conversation
delegate 子树。同一个固定 Release/FemtoVG 二进制通过
`USHER_AI_REVIEW_DOCKED_LIFECYCLE=retain|unmount` 切换，两组 SHA-256 完全相同：
`be5d5c0d2632469f404c997a5ba5a46b2c74ce54b1bf76a9e3828fe196e6bb99`。
生产默认值为 `retain`。

代表性 fixture 包含 12 条、每条 4,096 bytes 的 assistant response，共 24 行、576 个
block、1,132 个 span、49 个 repository action，以及一个 79-block 的 Docked Preview；
每个 variant 运行 5 个隔离进程。诊断与 UI 测试确认：candidate 隐藏期间源 model 仍为
24 行/576 blocks，实际挂载计数降为 0；重新显示后 delegate 恢复，conversation、
composer 模式、输入文本和滚动位置均保留。

| 代表性指标 | Retain 基线 | Unmount 候选 | 候选 − 基线 |
| --- | ---: | ---: | ---: |
| 外部 Preview 稳定态 RSS | 140.23 MiB | 139.52 MiB | -0.72 MiB (-0.5%) |
| 外部 Preview 稳定态 footprint | 112.86 MiB | 106.52 MiB | -6.34 MiB (-5.6%) |
| 进程内 response → Preview RSS | +11.32 MiB | +11.51 MiB | **+0.19 MiB** |
| 进程内 response → Preview footprint | +11.59 MiB | +12.99 MiB | **+1.40 MiB** |
| 进程内 response → Preview malloc in-use | +0.93 MiB | -5.23 MiB | **-6.16 MiB** |
| 进程内 response → Preview malloc blocks | +6,305 | -95,368 | **-101,673** |

所有权释放是真实的，但没有转化为代表性进程 RSS 降低。64 responses × 8,192 bytes 的
stress fixture 将候选放大到 128 行、1,568 blocks；进程内 response-to-Preview delta
相比基线改善 3.72 MiB RSS、14.94 MiB footprint 和 17.41 MiB malloc in-use。即使在
这个压力场景，外部 RSS 也只下降 2.1%（`159.69 → 156.36 MiB`），仍未通过
10 MiB/5% 采用门槛。3 进程 Detached 负控制始终挂载 delegate，稳定态 RSS/footprint
仅变化 `+0.19/+0.28 MiB`。两轮 open/close cell 未发现 candidate 累积斜率回退，
也没有稳定 RSS 收益。

结论：**拒绝把 delegate unmount 作为生产优化**。代表性进程内 RSS 与 footprint 均
轻微回退，而不同启动批次之间的绝对 footprint 存在波动；因此不能把显著的 live
malloc 下降描述成对 300+ MiB RSS 的有效改善。普通运行保留本批前的生命周期；显式
`unmount` policy、诊断、fixture 与测试仅作为可复现实验基础设施保留。下一批更适合
验证 response delegate 的可见窗口虚拟化，从源头减少活跃 UI work，而不是依赖整树
销毁后 allocator 主动归还页面。

全部 38 份进程日志、raw sample、metadata、生成对比和固定的 27,869,008-byte 二进制
位于 Git 忽略的
`memory-benchmark-results/ai-review-docked-unmount-ab/20260827T064258Z-step2-response-delegate-unmount/`。

<a id="mem-16"></a>

### MEM-16 — AI Review response 可见窗口虚拟化

状态：拒绝设为生产默认（2026-08-27）

候选把 Review 状态、finding、conversation message 与 chat 状态投影成单一 `ListView`
行 model；可见 assistant 行仍复用生产 `GitHubAgentResponse`。源 conversation/block model
完全保留，扁平行只共享 handle，不复制 response payload。同一个固定 Release/FemtoVG
二进制通过 `USHER_AI_REVIEW_RESPONSE_LIST=retain|virtualized` 切换，并显式把已拒绝的
MEM-15 lifecycle 固定为 `retain`。SHA-256 为
`bdb15f0efbd18632f8ef5f397c2e1f735eb0e7283b5b1982a0a32c624256f629`。

代表性 12x4,096-byte fixture 每个 variant 运行 5 个隔离进程，均执行相同的
1280x860 往返滚动准备。Retain 创建全部 24 个 message delegate 和 576 个 block
delegate；Virtualized 保留 24 个源行/576 blocks，并增加一个共享 handle 的 27 行视觉
投影，但实际只创建 4 个 visual delegate、2 个 message delegate 和 48 个 block
reference。稳定态测量位于滚动之后，已覆盖 ListView 使用后的高水位。

| 代表性指标 | Retain | Virtualized | 候选 - 基线 |
| --- | ---: | ---: | ---: |
| 稳定态 RSS | 131.64 MiB | 126.78 MiB | -4.86 MiB (-3.7%) |
| 稳定态 physical footprint | 151.53 MiB | 144.27 MiB | -7.27 MiB (-4.8%) |
| Fixture checkpoint malloc in-use | 30.80 MiB | 25.21 MiB | -5.59 MiB |
| Fixture checkpoint malloc blocks | 182,080 | 89,358 | -92,722 |

64x8,192-byte stress fixture 每个 variant 运行 3 个进程，源 model 为 128 行、1,568
blocks；candidate 初始 checkpoint 仍只有 4 个 visual delegate 与 2 个 message
delegate。稳定态 RSS 从 149.12 降至 131.08 MiB（`-18.05 MiB/-12.1%`），footprint
从 166.22 降至 147.77 MiB（`-18.45 MiB/-11.1%`）；checkpoint malloc in-use
中位数下降 17.21 MiB，live block 数下降 286,659。两组的双周期斜率均为 0。

无 AI 内容的 Home 负控制每个 variant 运行 3 个进程：稳定态 RSS 为
`+0.53 MiB/+0.5%`，footprint 为 `-0.08 MiB/-0.1%`，两组周期斜率同为
`-0.33 MiB`。因为没有挂载 AI panel 或 response model，这属于进程启动噪声，不能
归因于 feature 固定分配。

结论：**生产继续使用 `retain`**。可见窗口虚拟化在 stress 形状下收益显著，也证明
完整 delegate 挂载确实占有 live memory；但代表性 RSS 收益没有通过预先声明的
10 MiB/5% 采用门槛。未设置或非法 policy 继续回退 `retain`；`virtualized`、诊断和
交互测试只保留为显式实验基础设施。除非新的代表性 fixture 通过门槛且完成手工视觉
验收，否则不得把 candidate 变为下一轮生产基线。

raw sample、22 份正式进程日志、对比表、固定二进制及 10 份排除的 pilot 日志位于
Git 忽略的
`memory-benchmark-results/ai-review-response-list-ab/20260827T105430Z-step3-visible-window-virtualization/`。
pilot 因归档路径被标为 `non-release-override` 且 metadata 没有标出 candidate dimension，
不计入正式统计。

