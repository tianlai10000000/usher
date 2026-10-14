# Documentation / 文档导航

日常开发从本页和 [规格索引](specs/README.md) 开始。当前架构负责模块归属与数据流，
规格负责行为契约；历史实验和验收结果集中在 [归档索引](archive/README.md)。
归档中的“当前”“待实施”等措辞保留记录当时的语境，不代表今天的产品状态。

Start here and in the specification index for current development. Architecture
describes ownership and data flow; specifications define behavior. Archived
experiments and acceptance reports retain their original historical context.

## 日常入口 / Current references

| 文档 / Document | 用途 / Purpose |
| --- | --- |
| [项目 README](../README.zh.md) · [English](../README.md) | 产品范围、环境与支持的命令 / Product, setup and supported commands |
| [架构](architecture.zh.md) · [English](architecture.md) | 模块归属、状态与生命周期 / Ownership, state and lifecycle |
| [规格索引](specs/README.md) | 稳定行为契约与专项验收矩阵 / Behavior contracts and acceptance matrices |
| [构建性能与产物策略](build-performance.md) | 构建、IDE、profile 与清理命令 / Build, IDE, profiles and cleanup |
| [P6 回归协议](p6-regression.md) | 重复负载、A/B 门禁与原始证据要求 / Repeated-load protocol and evidence gates |
| [内存待办](memory_optimization_backlog.zh.md) · [English](memory_optimization_backlog.md) | 基线、活跃任务及采用阈值 / Baselines, active tasks and acceptance thresholds |
| [UI 迁移计划](ui-github-migration-plan.md) | 尚待实施或验收的批次 / Remaining implementation and acceptance work |
| [PR 冲突后续迭代](pr-conflicts-iterations.md) | 编辑、标记与提交的后续阶段 / Planned editing, resolution and commit stages |
| [本地归档恢复](local-archives.md) | 被清理的基准输出和旧发布产物 / Restoring local benchmark and release evidence |
| [Coding Agent 规则](../AGENTS.md) · [旧入口](ARCHITECTURE_FOR_AI.md) | 仓库操作规则 / Repository rules |

根目录的 [优化方案](../optimizing-proposal.md)、[代码质量方案](../code-quality-proposal.md)
和 [RSS 诊断](../memory-rss-diagnostics.md) 保留在原位置。缺陷登记见
[bug-history.md](../bug-history.md)。方案中的实验状态与结果按各自日期阅读，
不能用旧候选或未通过验收的方案替代当前规范。

The three root proposal/diagnostic documents remain in place. Read experimental
decisions by date; a proposal or failed candidate does not supersede a specification.

## 目录职责 / Directory roles

| 目录 / Directory | 内容 / Contents |
| --- | --- |
| [specs/](specs/README.md) | 当前行为契约 / Current contracts |
| [designs/](designs/pr-conflicts/README.md) | 设计参考入口与采用的视觉基线 / Design references and adopted visual baselines |
| [validation/](validation/) | 原始测试输出、正反例与基线/候选证据 / Raw checks and baseline/candidate evidence |
| [archive/](archive/README.md) | 完成或否决的实验、设计研究与历次验收 / Completed or rejected experiments and historical acceptance |

`archive/` 随 Git 保存；`.local-archives/` 是本机忽略的大型证据归档，恢复方式见
[local-archives.md](local-archives.md)。当前文档引用历史结论时直接链接归档，
历史报告不再散落于 `docs/` 顶层。新增规范仍须同步规格索引；新的排查过程和
验收记录应放入对应历史目录，并明确日期、采用决策和未测范围。

`archive/` is tracked in Git. `.local-archives/` holds larger local-only evidence.
Keep current contracts separate from dated investigations, decisions and limitations.
