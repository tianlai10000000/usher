# 代码质量与可读性优化记录

首次记录：2026-09-07

最近复核：2026-09-08；实施基线 `53e1fd4`，CQ-B2 开始 HEAD `b8adf7a`

状态：CQ-B1 至 CQ-B8 八个执行批次均已完成。每批使用行为刻画 A/B；涉及现有生产 fixture 的批次另做 Release 二进制 A/B。

## 范围与结论

本文补充 `optimizing-proposal.md`，关注代码表达、类型设计、局部重复、函数接口和可维护性，不重复展开架构、性能、发布与安全专项。

结论来自对 Git、Tag、Chat、Job、Toast 和文本几何代码的抽查，不代表全仓逐行审计。2026-09-08 复核开始时工作区干净；实施按下方批次记录持续更新。上一轮的测试结果保留在历史记录中，不作为已完成批次的验收结果。

主要提升方向是：让类型承载更多语义，减少需要调用者记住的约定，并让同一业务规则只有一个实现。项目已有 `RepoType`、`PullRequestReviewStatus`、`LocalizedText`、`PlannedCommand` 等可沿用的范例。

## 本轮复核变化

| 建议 | 当前结论 | 相对上一轮需要更新的内容 |
| --- | --- | --- |
| CQ-01 命令结果 | CQ-B2 已完成 | 命令终态已类型化；远程 Tag 在 stdout 合并和截断前增量解析；同步入口复用同一选择规则 |
| CQ-02 字符串集合 | CQ-B3、CQ-B6、CQ-B7 已完成 | Tag bump、CD stage/status 及 Diff line/change type 均已类型化，字符串只保留在外部边界 |
| CQ-03 状态关系 | CQ-B4、CQ-B5 已完成 | Job 生命周期集中维护语义耗时；Chat 每模式以单一可选计时起点表达发送状态 |
| CQ-04 Toast | CQ-B1 已完成 | 配置解析、宽度类型和五类回调已收敛，Rust/Slint 时长语义已对齐 |
| CQ-05 Tag 计划 | CQ-B2、CQ-B3 已完成 | 引用解析已统一；create/remove 共用 `PlannedCommand` 和 Step 投影，刷新/完成位置由计划长度派生 |
| CQ-06 几何参数 | CQ-B8 已完成 | 四个 Rust 几何门面改用 `TextGeometryOptions`；`controller/diff_bindings.rs` 在 `PrDiffBridge` callback 边界组装具名字段，Slint 声明保持兼容 |

以下是已经实现的前提，不再列为本轮待办：

- `src/workflow/tag/command_output.rs`：每流 4 KiB 分块、共享 16 块队列、每流最多 3 字节 UTF-8 残留、含截断标记的 32 KiB 展示尾日志；取消 abort/join Reader，子进程退出后管道排空限时 2 秒。
- `src/jobs/log_model.rs`、`presenter.rs`：`LogSnapshot` 复用未变化行，Slint 转换缓存最多 128 行，Step/log model 在同来源进度更新中保持身份；Job UI 已接入日志虚拟化。
- `src/workflow/pr_review/controller/diff_bindings.rs`、`ui/features/pr_review/diff_bridge.slint`：Diff 服务已归无状态 `PrDiffBridge`；Find 后台扫描与取消由 `find_worker.rs`、`find_bridge.rs` 承担。
- `src/chat/viewport_policy.rs`、`message_model.rs` 和 `src/ai_review/sessions.rs` 已承接提取后的职责。本轮只处理请求状态和 Diff 类型相关调用，不重新划分这些模块。

这些前提已核对相应代码；P5 的完整性能结论与手工验收范围仍以 `optimizing-proposal.md` 第 18–20 节及对应性能记录为准。CQ-B2 已按相同 P5-B #58 生产 fixture 重跑专项 A/B，结果见文末实施记录。

## CQ-01：命令返回值混合业务数据与展示日志

**实施状态**：CQ-B2 已完成。以下“现状与问题”保留实施前分析，当前结果与验证数据以文末 CQ-B2 记录为准。

**位置**

- `src/workflow/tag/command_output.rs`：`run_command()`、`Output`、`read_stream()`、`Decoder`。
- `src/workflow/tag/controller.rs`：重新导出 `run_command()`；仍拥有 `run_command_blocking()`、`inspect_latest_remote_tag_live()`、`inspect_latest_remote_tag()`。
- `src/workflow/tag/benchmark.rs`：通过 controller 导入命令入口，并按 `Ok/Err` 中的日志检查 fixture 结果。

**现状与问题**

异步命令执行返回展示日志字符串，其中混合 stdout、stderr、`Run ...` 前缀，并经过日志截断。`inspect_latest_remote_tag_live()` 又从这个 `list_log` 解析远程 Tag。同步实现则通过返回元组中的 stdout 单独解析，两条路径的输入语义不同。

展示日志的格式和截断策略因此会影响业务结果：如果最大语义版本所在记录被截掉，它就无法参与选择。这是由代码路径推导出的风险，本轮没有进行大规模 Tag 仓库的端到端复现。

此外，异步执行使用 `Result<String, String>`，取消、启动失败、执行失败最终都落到字符串；调用者还需要查询取消标志来解释失败原因。

P5-B 的模块提取和有界传输已经完成，但没有消除上述语义混用。现有 `Output::Bytes` 用 `0/1` 区分 stdout/stderr；流身份在拼成展示日志后丢失。实现业务解析时应在合并之前消费 stdout。

**建议**

- 返回具名的命令完成结果，将退出结果与展示尾日志分开；Tag 引用通过 stdout 专用的有界增量解析器处理，保留当前最大版本即可，不要求所有命令收集完整 stdout/stderr。
- 流事件用 `Stdout`、`Stderr` 等具名类型标识；业务解析必须发生在双流合并和展示截断之前，并正确处理跨块记录。
- 用局部错误枚举覆盖取消、启动失败、等待失败、读取失败、排空超时和非零退出；保留有界诊断日志，调用者从结果判断失败类别。
- 保持现有块大小、队列、UTF-8 残留、日志和排空预算。给业务解析中的半条记录设置独立上限及显式失败策略，不能把无界整行缓冲引回来。
- 同步入口消费同一个引用解析器，格式化日志与读取 stdout 的职责保持清晰。同步/异步执行机制不必为消除语法重复强行统一。

**验收关注点**

- 改变日志前缀、展示格式或截断阈值，不影响 Tag 选择结果。
- stdout、stderr 混合输出时，只有 stdout 中的引用记录参与解析。
- 覆盖最大版本不在日志尾部、空结果、取消、启动失败和非零退出。
- 覆盖引用记录跨块、stderr 在 stdout 两块之间插入、读取失败和子进程退出后管道未关闭。
- 既有分块/背压/排空测试继续通过；`tag/benchmark.rs` 的尾部检查、取消检查和指标仍走真实生产入口。

## CQ-02：有限集合使用字符串，缺少编译期约束

**位置**

- `src/git/model.rs`：`PullRequestDiffLine.kind`、`PullRequestDiffFile.change_type`。
- `src/git/diff.rs`、GitHub/GitLab/Gitee adapter、`src/workflow/pr_review/`：构造、转换和消费上述字段。
- `src/ai_review/controller.rs`：`unified_diff_from_detail()` 同时匹配 `"addition" | "added"`、`"deletion" | "removed"`。
- `src/workflow/tag/version.rs`：`next_semver_for_mode()`。
- `src/workflow/cd.rs`：`deployment_snapshot()`、`rollback_snapshot()`。

**现状与问题**

这些字段和参数表示有限且已知的集合，却以 `String` 或 `&str` 在业务逻辑中传递。拼写错误和新增分支无法获得穷尽性检查。版本递增把所有未知 mode 当作 patch；CD 阶段使用 `_ => {}` 忽略未知值。

**建议**

- 在 Rust 内使用 `DiffLineKind`、`FileChangeKind`、`VersionBump`、`DeploymentStage` 等枚举。
- Provider 输入边界负责别名归一化，Slint 输出边界负责转换到现有 UI 协议。
- 明确定义未知输入的处理策略：拒绝、显式 Unknown，或有文档依据的回退。
- 保留自由文本字段的字符串形式，例如标题、正文和搜索词；按实际语义选择类型。
- 分别处理 Tag bump、CD stage/status、Diff kind/change_type；不要将 RepoType、PR 状态、日志级别等额外领域一次性纳入本项。
- 类型化过程中保留当前有效输入及回退语义。尤其 Tag 的无效当前版本回退和预发布/build qualifier 核心比较来自已有 spec，不能顺便切换为不同的 SemVer 排序规则。

**验收关注点**

- Provider 别名在输入边界统一，业务代码不再到处兼容不同拼写。
- 新增枚举成员时，遗漏处理的匹配能够被编译器发现。
- 现有 Slint 协议、持久化格式及有效输入行为保持兼容；修改回退行为时同步相应 spec。

## CQ-03：状态字段之间的关系依赖调用者维护

**实施状态**：Job 部分已由 CQ-B4 完成，Chat 部分已由 CQ-B5 完成。以下“现状与问题”保留实施前分析，当前结果与验证数据以文末 CQ-B4、CQ-B5 记录为准。

**位置**

- `src/chat/session.rs`：`ChatRequestState`，调用方为 `src/chat/interactive.rs`。
- `src/jobs/model.rs`：`JobStepData`、`JobSnapshot`；关联 `src/jobs/log_model.rs`、`presenter.rs`。
- `src/workflow/tag/controller.rs`：步骤运行、成功、失败、取消时的字段赋值。

**现状与问题**

Chat 为每种模式同时维护 `sending: bool` 和 `started_at: Option<Instant>`，现有方法始终同步修改二者，因此 `sending` 可以从开始时间是否存在推导。

Job 则分别保存 status、conclusion 和字符串 duration，调用方需要同步赋值。类型本身允许 `Queued + Success` 等无意义组合；耗时还混合空字符串、运行中的省略号和完成后的格式化文本。

这是维护风险；本轮没有发现 Chat 已经出现字段不一致的运行实例。

当前 `JobSnapshot` 已有 `start()`、`finish()`、`replace_steps()` 和日志请求保留逻辑，不能记录为完全缺少状态方法。新增的 `JobStepData::set_log()` 已同时更新 `log` 与 `parsed_log`；这些是需要沿用的入口，主要剩余工作是 Step 生命周期及耗时表达。

**建议**

- Chat 去掉冗余布尔值，通过 `Option<Instant>` 判断是否发送中。
- Job 先提供 `start()`、`succeed()`、`fail()`、`cancel()` 等方法，集中维护字段关系。
- 后续视需要采用 `Completed { conclusion, duration }` 等枚举变体，使完成结果与完成状态绑定。
- 内部保存 `Duration` 等语义值，UI 投影时再格式化；运行提示由状态决定。

**验收关注点**

- 覆盖开始、成功、失败、取消、再次运行的转换。
- 保持现有 Job 展开状态、日志请求状态、耗时显示和 Tag running 闩锁语义。
- 测试行为与状态约束，避免仅断言实现内部的字段赋值顺序。
- 保持 `set_log()` 与 `LogSnapshot` 的配合、presenter 的稳定 Step/log model 身份；修改状态接口时同步 Tag benchmark 的构造与完成路径。
- Chat 的状态简化单独提交，不改 `viewport_policy`、`message_model` 和 JSONL 格式；当前分页测试失败单列为基线，不借本项重写视口算法。

## CQ-04：Toast 的大元组、长参数列表和重复回调

**实施状态**：CQ-B1 已完成。以下“现状与问题”保留实施前分析，当前结果与验证数据以文末 CQ-B1 记录为准。

**位置**

- `src/toast.rs`：`parse_width()`、`create_toast_with_defaults()`、`apply_toast()`、`setup_toast()`。
- UI 对应声明：`ui/components/toast/toast_manager.slint`。

**现状与问题**

默认值解析返回八项元组，包含多个 `String` 和两个相邻 `bool`；调用方拆开后，再传给十参数的 `apply_toast()`。success、error、info、loading、show 回调重复默认值解析、克隆和应用过程。

`parse_width()` 返回 `(bool, f32, f32)`，需要额外注释解释哪些字段有效、哪些必须为零。

**建议**

- 引入具名的 `ResolvedToast`，统一解析默认消息、时长、位置、宽度等选项。
- 使用 `apply_toast(manager, toast)`，各回调只负责传入类型和 options。
- 宽度采用 `Auto | Pixels(f32) | Percent(f32)` 表达互斥关系。
- 在选项统一解析后，清理不必要的中间克隆和失效的注释代码。

**验收关注点**

- 各便捷回调与通用 show 的同类输入得到一致配置。
- 保持负数时长使用默认值、零时长常驻、正数自动关闭的当前 Rust 行为。
- 保持默认消息、宽度回退和 Toast 世代校验行为；修改 UI-facing 接口时同时检查 Rust 与 Slint 声明。
- `ToastOptions.duration` 的 Slint 注释同时写了 `<= 0` 使用默认值和 `0` 常驻，示例还称省略 duration 使用默认值。该批应校正注释并验证省略字段的实际行为；沿用当前 Rust 的负数/零/正数语义，不通过本次重构暗改默认时长。

## CQ-05：Tag 计划和解析规则存在重复及位置耦合

**实施状态**：CQ-B2 完成引用解析，CQ-B3 完成计划与步骤投影。以下“现状与问题”保留实施前分析，当前结果与验证数据以文末对应批次记录为准。

**位置**

- `src/workflow/tag/commands.rs`：`create_tag_plan()`、`remove_tag_plan()`。
- `src/workflow/tag/controller.rs`：`default_remove_tag_steps()`、`run_local_remove_tag_flow()`、`run_local_tag_flow()`。
- `src/workflow/state.rs`：`default_tag_steps()` 及待执行计划的重置入口。
- 同文件：异步与同步远程 Tag 检查中的引用解析和最大版本选择循环。

**现状与问题**

create 返回包含名称、argv 和说明的 `PlannedCommand`，remove 却返回 `Vec<Vec<String>>`。删除流程通过 `zip` 将命令填入另一份手写步骤表，再按索引访问步骤。命令与步骤描述的对应关系依赖两个列表保持相同顺序。

远程 Tag 的引用解析和最大版本选择算法也在同步、异步路径各实现一次，修改规则时容易只改一处。

**建议**

- create/remove 统一返回 `PlannedCommand`，可执行步骤从计划生成。
- 明确区分额外的刷新、完成步骤，避免隐藏的位置约定。
- 抽取远程引用解析和最大版本选择的纯函数，由同步与异步路径共同调用。
- 配合 CQ-01，使共享解析器只接收语义一致的输入。
- 共享引用解析器属于 CQ-B2；本项在 CQ-B3 只继续完成计划统一，避免两个批次各建一份 helper。

**验收关注点**

- 插入或重排命令时，名称、说明与执行命令仍对应。
- 保持 Git argv 参数边界与现有命令顺序，尤其是带空格的路径和描述。
- 统一覆盖 `v`/`V` 前缀、预发布/build qualifier、无合法 Tag、相同版本核心等情况，遵守当前 Tag spec 的版本规则。

## CQ-06：文本几何接口反复传递同一组位置参数

**实施状态**：CQ-B8 已完成。以下“现状与问题”保留实施前分析，当前结果与验证数据以文末 CQ-B8 记录为准。

**位置**

- `src/workflow/pr_review/text_geometry.rs`：`with_layout()`、`with_layout_options()`、`hit_test()`、wrapped hit test 和 range geometry 系列函数。
- Rust 绑定：`src/workflow/pr_review/controller/diff_bindings.rs`；关联 `selection.rs` 的几何适配。`find_bridge.rs` 属于 Find 行为回归范围，不是几何算法的所有者。
- UI 声明：`ui/features/pr_review/diff_bridge.slint` 中的 `PrDiffBridge`，消费方为 PR Review workspace。

**现状与问题**

字体、字号、字重、斜体、缩放、宽度和多个坐标沿调用链反复传递，多处需要允许 `clippy::too_many_arguments`。多个相邻 `f32` 含义不同，交换顺序也可能通过编译；新增选项需要同步调整多层函数签名。

**建议**

- 按已有语义组织为 `TextStyle`、`LayoutOptions`、`Point` 等小结构。
- 将文本样式与布局约束分开，避免形成承载所有参数的大对象。
- 保留底层排版算法，通过具名字段改善调用处的可检查性。
- 首批只重组 Rust 内部参数，继续在 `PrDiffBridge` 边界接收现有 Slint 参数并组装具名结构。不要把 callback 搬回 `ui/main.slint` 或重新扩大 composition root。

**验收关注点**

- 既有 mixed-script、软换行、缩放、选区与命中测试通过。
- 样式和布局选项仍完整参与缓存身份计算。
- 涉及 Slint 回调适配时，手工检查选区、Find 高亮及换行布局。
- 保持 `GeometryKey` 对字体、字重、斜体、scale 和 wrap width 的完整覆盖，以及 Find 后台任务的取消/空查询语义。

## 执行批次

保留 CQ-01 至 CQ-06 作为问题编号，用 CQ-B1 至 CQ-B8 标识可交付批次。每批单独提交和登记检查结果；八个批次均已完成，模块提取或关联性能优化不等于完成对应批次。

| 建议顺序 / 批次 | 对应问题 | 交付范围 | 前置依赖 | 状态 |
| --- | --- | --- | --- | --- |
| 1 / CQ-B1 | CQ-04 | Toast 配置对象、宽度类型、五类回调收敛 | 无 | 已完成 |
| 2 / CQ-B2 | CQ-01、CQ-05 解析部分 | 命令结果/错误类型、stdout 有界解析、同步异步共用引用规则 | 无；正确性优先，可先于 B1 | 已完成 |
| 3 / CQ-B3 | CQ-05 计划部分、CQ-02 Tag 部分 | 创建/删除计划统一、VersionBump 及其 UI 转换 | B2 | 已完成 |
| 4 / CQ-B4 | CQ-03 Job 部分 | Step 生命周期、Job 耗时及调用方迁移 | B3，先稳定计划再封装步骤转换 | 已完成 |
| 5 / CQ-B5 | CQ-03 Chat 部分 | 双模式请求状态去冗余 | 无，可提前 | 已完成 |
| 6 / CQ-B6 | CQ-02 CD 部分 | 部署/回滚阶段和状态枚举 | 无，可提前 | 已完成 |
| 7 / CQ-B7 | CQ-02 Diff 部分 | Provider 到 PR/AI Review 的 Diff 类型贯通 | 无；单独控制跨模块范围 | 已完成 |
| 8 / CQ-B8 | CQ-06 | 几何参数对象与新 PrDiffBridge 调用适配 | 建议 B7 后；属编辑范围重叠，并非算法依赖 | 已完成 |

明确依赖链为 **B2 → B3 → B4**。B1、B5、B6 可独立交付；B7/B8 顺序执行以减少同一 Diff 代码的反复修改。若从 Tag 开始，直接先做 B2，不必等待 Toast。

### CQ-B1：Toast 配置归一化

- **修改范围**：`src/toast.rs`；仅在注释或边界验证需要时涉及 `toast_manager.slint`。
- **完成标准**：一个 `ResolvedToast` 承接所有默认值；`apply_toast` 接收配置对象；五类回调只选择类型和传递选项；宽度用互斥类型表达。
- **专项验证**：默认消息、各类默认时长、负数/零/正数及省略字段；auto/px/%/非法宽度；连续 Toast 不被旧定时器关闭。新增有意义的配置测试，并手工检查显示和关闭行为。
- **边界**：只整理内部接口和错误注释，保留现有 Slint 对外字段及计时语义。

### CQ-B2：命令结果与引用解析

- **修改范围**：`tag/command_output.rs`、`controller.rs`、现有 `version.rs` 或专用小型引用解析模块、`benchmark.rs`。
- **完成标准**：业务解析不再读取合并/截断日志；命令终态使用类型表示；同步/异步共用同一引用选择规则；半条 stdout 记录也有预算。
- **专项验证**：`cargo test --lib --locked workflow::tag`；新增跨块/双流交错、最大版本在输出前部、空记录、错误类型和超限输入回归。沿用背压、取消和排空测试；触及传输时复跑现有 P5-B #58 Job 基准，命令选项与预算见 `docs/archive/optimization/p5-b-performance.md` 和对应脚本。
- **边界**：保持已完成的 4 KiB / 16 块 / 32 KiB / 2 秒约束，不恢复无界输出或整行读取；benchmark 继续检查真实生产路径。

### CQ-B3：Tag 计划及版本模式

- **修改范围**：`tag/commands.rs`、`version.rs`、`controller.rs`、`workflow/state.rs`；检查 `ui/main.slint` 的 Tag 属性/callback 及 `ui/components/workflow/` 下对应控件。
- **完成标准**：create/remove 计划均由 `PlannedCommand` 描述；待执行和实际执行步骤采用一致的计划映射；刷新/完成步骤显式追加；Rust 内以 `VersionBump` 表示模式，仅在 UI 边界转换。
- **专项验证**：Tag 单元测试；argv 边界、创建/删除顺序、版本核心规则、模式切换重置、运行页 Back、无效输入回退。
- **边界**：不再次实现 B2 的引用解析，不改变分支命令、Remote 不可用语义及当前 Tag spec 的版本比较规则。

### CQ-B4：Job 生命周期及耗时

- **修改范围**：`jobs/model.rs`、Tag controller/benchmark、`workflow/state.rs` 的相关调用；关联检查 `jobs/presenter.rs`、`log_model.rs`。
- **完成标准**：Step 转换由方法集中维护 status/conclusion/detail/duration；沿用已有 `JobSnapshot::start/finish`；耗时保留语义值并在投影时格式化；生产调用方不再手工拼出矛盾的生命周期状态。
- **专项验证**：`cargo test --lib --locked jobs` 和 Tag 测试；状态转换、再次运行、取消、失败自动展开、日志请求保留、Step/log model 身份、`set_log()` 的原文与解析结果一致性。
- **边界**：不因状态对象改造而重建全量日志或绕过 `set_log()`；不把 Feature 进程所有权放入 Jobs。涉及 presenter/日志更新时复验现有 Job 基准。

### CQ-B5：Chat 请求状态简化

- **修改范围**：`chat/session.rs`，必要时调整 `interactive.rs` 的请求状态调用。
- **完成标准**：每种模式以 `Option<Instant>` 或等价的单一状态表达发送过程；维持 `start/finish/is_sending/elapsed_seconds` 的有效行为。
- **专项验证**：`cargo test --lib --locked chat::session`；AI/RAG 独立发送、完成耗时、重复 finish、再次发送；手工检查模式切换与发送/取消控件。
- **边界**：不修改历史文件、视口算法及消息 model；分页布局已有失败须与新增失败分开记录，不删除或放宽原断言。

### CQ-B6：CD 阶段类型化

- **修改范围**：`workflow/cd.rs` 及 `workflow/state.rs` 的 CD 快照消费；核对根 CD 属性/callback 与 `github_cd_flow.slint`。
- **完成标准**：部署阶段、回滚阶段和 Step 展示状态有明确类型；必要时用两个阶段枚举表达不同的合法集合；UI 字符串仅在投影边界生成。
- **专项验证**：增加针对每个部署/回滚阶段的快照转换测试；检查终态 running、各步骤状态和 `LocalizedText` 文案；手工检查当前 CD 流程。
- **边界**：保持当前 CD 的模拟阶段推进方式，不扩展为真实部署引擎；仅验证枚举不够，须验证生成的完整阶段状态。

### CQ-B7：Diff 领域类型贯通

- **修改范围**：`git/model.rs`、`diff.rs`、三个 Provider adapter、`workflow/pr_review/` 的构造/投影/消费点，以及 `ai_review/controller.rs::unified_diff_from_detail()` 和相关 fixture。
- **完成标准**：`kind`、`change_type` 在 Rust 领域内为枚举，Provider 别名在边界归一化，Slint model 的现有字符串协议由统一转换生成。UI 专用合成行与 Provider 源 diff 行按各自语义处理，不机械替换全仓 `kind`。
- **专项验证**：Git、PR Review、AI Review 相关单元测试；`cargo test --test pr_review_core_journeys --locked`；关闭默认功能的 check；覆盖 addition/deletion/context/hunk/meta、文件新增/删除/重命名及 AI unified diff 行号。
- **边界**：不改变 diff 内容、哨兵行号、缓存身份、文件/选区定位和 Findings 导航。此批跨模块，不能只让 `model.rs` 编译就标完成。

### CQ-B8：几何参数对象

- **修改范围**：`text_geometry.rs` 和 `controller/diff_bindings.rs`，按实际调用检查 `selection.rs`、`find_bridge.rs`。
- **完成标准**：重复样式与布局参数以小型具名结构传递；Slint 的位置参数只保留在 `PrDiffBridge` 边界；`GeometryKey` 继续完整表达布局身份。
- **专项验证**：`cargo test --lib --locked workflow::pr_review`、PR journey 集成测试；手工覆盖中英文混排、软换行、缩放、跨行选区、Find 高亮/清空/上下条导航。涉及 Find 调度或计算路径时复验对应基准。
- **边界**：不修改 Parley 算法或把后台 Find 扫描搬回 event loop；保留 P5-D 的无状态 global 和结构门禁。Find 快捷键/导航的既有手工缺口不能用几何单测代替。

### 每批交付规则

1. 开始前记录 HEAD 与 `git status --short`，复核本批涉及的 spec 和 Rust/Slint 边界。
2. 实施和专项验证完成后，运行 `cargo fmt --all`、`cargo check --locked`、`cargo test --lib --locked`、`git diff --check`；涉及结构或 UI 时运行现有结构/UI 门禁，根 UI 变化编译并手工检查。
3. 更改几何、日志或输出链路时使用上面的针对性回归与现有基准，不修改原预算来掩盖回归。纯内部配置整理不强制全场景性能 A/B。
4. 已有失败与新增失败分开记录；不能把当前已知失败概括成所有测试通过，也不能把新失败归入旧基线。
5. 将本表状态更新为“已实现/待验收”或“已完成”，追加提交号、实际检查结果及剩余手工项。只有该批完成标准与专项验证满足时才能标“已完成”。
6. 行为或所有权变化时同步中英文架构文档；稳定约束变化时同步 spec 及索引。单纯参数/私有类型整理没有这些变化时，不做无关文档重写。

## 实施前验证基线（2026-09-08）

本轮在 `53e1fd4` 实际执行，未改业务代码：

| 检查 | 实际结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过；本轮文档任务采用检查模式 |
| `cargo check --locked` | 通过 |
| `./scripts/check-architecture-structure.sh` | 通过 |
| `./scripts/check-ui-policy.sh` | 通过 |
| `cargo test --lib --locked`（沙箱外） | 523 通过、1 失败、2 忽略；两项 Logo loader 测试通过 |

失败项为 `chat::interactive::pagination_tests::real_layout_prepend_and_bidirectional_scrolling`，断言位于 `src/chat/pagination_tests.rs:174`：`Ai: viewport must contain a real message at 9984`。与 `optimizing-proposal.md` 第 20 节的 P5-D 记录一致，属于开始本轮优化前可复现的基线失败；本轮仅记录，未修复或忽略该测试。

库测试还报告 `__eh_frame` 过大的链接器警告，与上一轮同类；未作为本次代码质量批次的新优化项。

本轮未执行 workspace 全目标/全 feature CI、完整 doctest、远端 CI、性能 A/B 或手工 UI 验证。P5-D 已记录开发功能下 `ai_review_response_ui` 夹具的 `@i18n` 映射问题，以及 Find 快捷键/导航手工缺口；这些本轮未重测，不能作为本轮新确认的失败或已通过项目。

本轮最终文档 diff 检查结果见文末更新记录。

## 历史验证记录（2026-09-07）

以下结果来自 2026-09-07 的代码分析轮次，不代表未来实施后的验收结果：

- `cargo fmt --all -- --check`：通过；使用检查模式，未格式化或改动用户文件。
- `cargo check --locked`：通过。
- `git diff --check`：通过。
- `cargo test --lib --locked`：502 通过、2 失败、2 忽略。两项失败均为 `project::logo_loader::tests` 在沙箱内绑定 `127.0.0.1:0` 被拒绝，报 `PermissionDenied / Operation not permitted`。
- 沙箱外定向重跑 `cargo test --lib --locked project::logo_loader::tests -- --nocapture`：2 项均通过。未在沙箱外重新执行全部库测试。
- 测试链接器报告 `__eh_frame` 超过 compact unwind 编码大小限制的警告。
- 未执行手工 UI 验证和完整 doctest；仓库约定中已有的 logger doctest 问题不属于本轮验证结论。

2026-09-07 分析轮次保留了当时的全部工作区修改。该轮结果仅用于历史追溯，后续验收使用上方 2026-09-08 基线。

## 更新记录

- 2026-09-07：建立 CQ-01 至 CQ-06 初始问题清单。
- 2026-09-08：基于 `53e1fd4` 重新核对六项，更新命令传输/Job 日志/Diff bridge 所有者和约束，拆成 CQ-B1 至 CQ-B8，补充批次依赖、完成标准与当前测试基线；本轮只修改本文。
- 2026-09-08：完成 CQ-B1 Toast 配置归一化和 CQ-B2 命令结果/远程 Tag 引用解析，批次结果见下方实施记录。
- 2026-09-08：完成 CQ-B3 Tag 计划统一与版本模式类型化，并同步 Tag spec 及中英文架构文档。
- 2026-09-08：完成 CQ-B4 Job Step 生命周期与耗时语义化，并同步 Job spec 及中英文架构文档。
- 2026-09-08：完成 CQ-B5 Chat 双模式请求状态去冗余，并同步 Chat spec 及中英文架构文档。
- 2026-09-08：完成 CQ-B6 CD 部署/回滚阶段及 Step 状态类型化，并同步 Job spec 及中英文架构文档。
- 2026-09-08：完成 CQ-B7 Diff 领域类型从 Provider 到 PR/AI Review 的贯通，并同步 PR Review spec 及中英文架构文档。
- 2026-09-08：完成 CQ-B8 文本几何参数对象及 `PrDiffBridge` Rust 调用适配，并同步 PR Review spec 及中英文架构文档。
- 初始文档复核轮次的 `git diff --check`：通过；该轮最终工作区变更仅为 `code-quality-proposal.md`。

### CQ-B1 实施记录（2026-09-08）

- **提交**：`b8adf7a refactor(toast): normalize resolved toast configuration`。
- **实现**：`src/toast.rs` 新增 `ToastWidth` 和 `ResolvedToast`；默认消息、位置、loading 类型、宽度和时长只解析一次；`apply_toast()` 改为单配置参数；五类回调统一走 `show_toast()`。`toast_manager.slint` 的时长注释已修正为当前 `< 0` 默认、`0` 常驻、`> 0` 毫秒语义。
- **A/B 行为刻画**：生产重构前补入并运行三项行为测试，A 为 3/3 通过；生产重构后以相同测试覆盖默认消息、宽度和默认/显式时长，B 为 3/3 通过。随后增加 Slint manager 回调投影测试，B 最终为 4/4 通过。
- **批次验证**：`cargo check --locked`、结构门禁、`./scripts/ui-check.sh`、UI policy 和 `git diff --check` 通过。沙箱外完整库测试为 525 通过、1 个既有 Chat 分页失败、2 忽略；相对基线新增的两项 Toast 测试通过，未出现本批新增失败。
- **Clippy**：`cargo clippy --lib --locked -- -D warnings` 在未涉及的 `crates/usher-i18n/src/lib.rs:83` 被已有 `unnecessary_lazy_evaluations` 阻断；本批代码未产生可见 Clippy 诊断。该失败不计作本批通过，也不在 CQ-B1 中顺手修改。
- **性能 A/B 范围**：Toast 没有持续负载 fixture；本批采用同一行为契约的修改前/后 A/B，未用无关 Home/Project 场景声称性能结论。运行时路径减少了重复 clone 和参数搬运，但没有据此宣称可测性能提升。

### CQ-B2 实施记录（2026-09-08）

- **提交**：`e552ae7 refactor(tag): separate command outcomes from output parsing`。
- **实现**：`tag/command_output.rs` 以 `CommandSuccess`、`CommandFailure` 和 `CommandFailureKind` 表达命令终态，以 `OutputStream` 区分 stdout/stderr，并为 stdout 提供合并/截断前的 consumer；读取、消费、排空、spawn、wait、退出和取消失败均保留有界日志。`tag/version.rs` 新增 8 KiB 单记录预算的 `RemoteTagParser`；异步路径跨块增量选择最大合法 Tag，同步路径通过 `latest_remote_tag()` 复用相同规则。controller 和生产 benchmark 已迁移到具名结果。
- **A/B 行为刻画**：A 为重构前 `workflow::tag` 12/12 通过；B 使用相同既有测试并加入跨块、最大版本位于输出前部、空结果、超限记录及类型化失败回归，最终 15/15 通过。既有 UTF-8/CRLF、双流排空、背压、取消和实时日志测试均继续通过。
- **Release 性能 A/B**：A 二进制 SHA-256 为 `1da0583d2f0f76c93c641a89fba5d1bb324b19a9ad55becf31f3e109bad24629`，最终 B 为 `c1725335a87602c591cbe491e032a59774d5076df32616812485a3f7e8e5d799`。P5-B #58 按 AB/BA/AB 各三轮运行，`summary.json` 判定 `passed: true`；A/B 的 peak 中位数为 135.47/136.83 MiB，settled 为 134.89/136.36 MiB，growth 均为 -0.359 MiB。B 三轮 peak queue 均为 15（预算 16）、tail 均为 32768 B、取消为 256–260 ms（预算 2000 ms），fixture 均完整且进程 clean exit。结果保存在 `/tmp/usher-cq-b2-ab-final-stream-type/summary.json`。
- **批次验证**：`cargo fmt --all`、`cargo check --locked`、`./scripts/check-architecture-structure.sh`、`./scripts/check-ui-policy.sh`、`./scripts/ui-check.sh` 和 `git diff --check` 通过。沙箱外完整库测试为 529 通过、1 个既有 Chat 分页失败、2 忽略；失败仍是 `chat::interactive::pagination_tests::real_layout_prepend_and_bidirectional_scrolling` 在 `src/chat/pagination_tests.rs:174`，没有本批新增失败。
- **Clippy 与手工范围**：`cargo clippy --lib --locked -- -D warnings` 仍只在未涉及的 `crates/usher-i18n/src/lib.rs:83` 被已有 `unnecessary_lazy_evaluations` 阻断。未执行真实远程仓库的交互式 Tag 操作；本批用命令 fixture、同步/异步解析测试和生产 Job fixture 验证内部重构，不改变 UI 协议或 Tag 行为。

### CQ-B3 实施记录（2026-09-08）

- **实现**：create/remove 均返回包含名称、argv 和说明的 `PlannedCommand`，并通过同一映射生成启动前发布与实际执行使用的 queued steps；remove 的 refresh/complete 和 create 的 complete 显式追加，索引由计划长度派生。`VersionBump` 在 Rust state 中表达 Patch/Minor/Major，未知 UI 输入按既有行为回退 Patch，字符串只在 Slint callback/投影边界转换。顺手消除了 CQ-B2 command transport 中 `is_some()+unwrap()` 的 Clippy 诊断。
- **A/B 行为刻画**：A（HEAD `e552ae7`）为 Tag 15/15、workflow state 2/2、根 UI 编译通过；B 为 Tag 18/18、workflow state 3/3、根 UI 编译通过。新增回归覆盖 create/remove 计划到步骤的名称、argv、说明一致性，refresh/complete 顺序，版本模式三值往返与非法输入回退，重置后全部步骤恢复 queued/折叠状态，以及 `VersionBump::Major -> "major"` 的真实 Slint 投影。
- **批次验证**：`cargo fmt --all`、`cargo check --locked`、`./scripts/check-architecture-structure.sh`、`./scripts/check-ui-policy.sh`、`./scripts/ui-check.sh` 和 `git diff --check` 通过。完整库测试为 533 通过、1 个既有 Chat 分页失败、2 忽略；失败仍是 `chat::interactive::pagination_tests::real_layout_prepend_and_bidirectional_scrolling` 在 `src/chat/pagination_tests.rs:174`，没有本批新增失败。
- **Clippy**：标准 `cargo clippy --lib --locked -- -D warnings` 仍先被 `crates/usher-i18n/src/lib.rs:83` 的既有 lint 阻断。继续豁免已确认的 `unnecessary_lazy_evaluations`、`collapsible_if` 和 `field_reassign_with_default` 基线 lint 后，全库 lib Clippy 通过；CQ-B3 范围没有剩余诊断。
- **文档与 A/B 范围**：已同步 `docs/specs/tag.md`、`docs/architecture.md` 和 `docs/architecture.zh.md` 的计划/版本类型所有权。此批不更改命令传输、日志模型或几何热路径，因此使用行为与 UI 协议 A/B，没有用 P5-B 输出 fixture 声称性能变化。未执行会真实创建/删除 Git Tag 的交互式操作；argv 边界、顺序、状态重置、Job Run/Back 路由基础行为和 Slint 协议由单元测试、完整库测试及根 UI 编译覆盖。

### CQ-B4 实施记录（2026-09-08）

- **实现**：`JobStepData::start/finish` 集中维护 status、conclusion、detail、duration 与初始展开状态；`JobSnapshot::start/finish` 继续维护 Job 生命周期。Step 和 Job 总耗时改为 `Option<Duration>`/`Duration`，queued 与未计时状态使用 `None`，空串、运行中的 `…` 和完成后的紧凑耗时只在 Slint 投影边界生成。Tag 创建、删除、取消、失败、刷新与生产 benchmark 均改用生命周期入口，日志仍只经 `set_log()` 更新。
- **A/B 行为刻画**：A（HEAD `eabe6b1`）为 Jobs 7/7、Tag 18/18；B 为 Jobs 9/9、Tag 17/17、workflow state 3/3，根 UI 均编译通过。B 新增 Step queued/running/completed/cancelled 投影、Job 再次运行清理旧结论/耗时，以及 `set_log()` 原文与解析快照一致性回归；controller 内重复的 duration 测试归并到 Jobs 模型所有者，所以 Tag 数量减少 1。既有日志懒加载、请求 latch、失败自动展开、Tag 取消/失败路径和 Step/log model 身份继续通过。
- **批次验证**：`cargo fmt --all`、`cargo check --locked`、结构门禁、UI policy、`./scripts/ui-check.sh` 和 `git diff --check` 通过。沙箱外完整库测试为 534 通过、1 个既有 Chat 分页失败、2 忽略；失败仍是 `chat::interactive::pagination_tests::real_layout_prepend_and_bidirectional_scrolling` 在 `src/chat/pagination_tests.rs:174`，没有本批新增失败。沙箱内另有两项 Logo loader 因禁止绑定 `127.0.0.1:0` 失败，沙箱外定向复跑 2/2 通过。
- **Clippy 与 A/B 范围**：标准 `cargo clippy --lib --locked -- -D warnings` 仍先被未涉及的 `crates/usher-i18n/src/lib.rs:83` 既有 lint 阻断；豁免既有的 `unnecessary_lazy_evaluations`、`collapsible_if` 和 `field_reassign_with_default` 后，全库 lib Clippy 通过。本批不改变 command transport、日志 parser、presenter 更新算法或输出预算，采用生命周期与 UI 协议的修改前/后 A/B；P5-B #58 仍由 CQ-B2 的 Release A/B 覆盖，未用同一传输 fixture 重复声称 CQ-B4 性能变化。未执行真实创建/删除 Tag 的交互式操作。

### CQ-B5 实施记录（2026-09-08）

- **实现**：`ChatRequestState` 删除可从计时起点推导的 `ai_sending`/`rag_sending`，每个模式只保留一个 `Option<Instant>`。`is_sending()` 由起点是否存在决定，`start()` 写入起点，`finish()` 取走起点并返回实测耗时；`interactive.rs` 的调用协议和 UI 属性保持不变。
- **A/B 行为刻画**：A（HEAD `b714b0f`）为 `chat::session` 4/4；B 为 5/5。相同测试继续覆盖 AI/RAG 状态和历史隔离，新增回归覆盖完成耗时、重复 finish 返回 `None`、完成后 elapsed 归零及再次 start 使用新起点。
- **批次验证**：`cargo fmt --all`、`cargo check --locked`、结构门禁、UI policy、根 UI 编译和 `git diff --check` 通过；豁免三项已登记基线 lint 后全库 lib Clippy 通过。沙箱外完整库测试为 535 通过、1 个既有 Chat 分页失败、2 忽略；失败仍是 `chat::interactive::pagination_tests::real_layout_prepend_and_bidirectional_scrolling` 在 `src/chat/pagination_tests.rs:174`，没有本批新增失败，也未修改或放宽其断言。
- **A/B 范围**：本批只删除互相依赖的冗余状态，不改变历史文件、请求传输、消息 model、viewport 或持续负载路径，因此采用状态行为与 UI 编译 A/B，不运行无关的 Chat 滚动性能 fixture，也不据此声明性能变化。未执行 Chat 模式切换与发送/取消的手工 UI 操作。

### CQ-B6 实施记录（2026-09-08）

- **实现**：`DeploymentStage` 和 `RollbackStage` 分别限定部署五阶段与回滚三阶段，`CdStepStatus` 表达 pending/running/success/rolled_back；`CdSnapshot` 不再存储自由字符串，只有 `apply_cd_snapshot()` 在 Slint 边界投影既有协议。异步阶段序列直接使用枚举，未知 stage 静默生成无效 running 快照的入口已消失。
- **A/B 行为刻画**：A（HEAD `7415936`）在 `workflow::cd` 下没有专项测试；B 为 3/3。新增表驱动回归逐一验证 Build/Test/Deploy/Verify/Done 与 Deploy/Verify/Done rollback 的 running 终态、四个 Step 状态及 `LocalizedText` 说明，并以真实 `MainWindow` 验证 success/rolled_back/running 字符串投影。
- **批次验证**：`cargo fmt --all`、`cargo check --locked`、结构门禁、UI policy、根 UI 编译、`git diff --check` 和基线 lint 豁免后的全库 lib Clippy 通过。沙箱外完整库测试为 538 通过、1 个既有 Chat 分页失败、2 忽略；失败仍位于 `src/chat/pagination_tests.rs:174`，没有本批新增失败。
- **A/B 范围**：本批只改变 Rust 阶段/状态表达和边界投影，不改变 700 ms 模拟推进、通知、项目隔离或 Slint 属性协议，因此采用完整阶段快照与真实 Slint 投影 A/B，不运行无关性能 fixture。未执行 CD 页面部署/回滚动画的手工 UI 操作。

### CQ-B7 实施记录（2026-09-08）

- **实现**：`DiffLineKind` 统一 addition/deletion/context/hunk/meta，`DiffChangeType` 统一 new/deleted/renamed/modified；GitHub、GitLab、Gitee 公有/企业 decode 在输入边界归一化各自别名。统一 diff parser、缓存统计、hunk/展开、搜索、高亮、文件树、lazy model 与 AI unified diff 全部消费枚举；只有 Slint 文件/行投影生成既有字符串。UI 专用合成行、左右列 kind 与 Conversation/AI 视觉行保持原字符串协议。
- **A/B 行为刻画**：A（HEAD `2016ef2`）为 Git 122/122、PR Review 79 通过/1 忽略、AI Review controller 15/15、核心旅程 4/4；B 为 Git 123/123、PR Review 79 通过/1 忽略、AI Review controller 15/15、核心旅程 4/4。新增别名回归覆盖 Gitee new/old/match、GitHub/GitLab added/removed/renamed/modified，并继续覆盖 unified diff 行号、文件新增/删除/重命名、AI 行前缀和 Slint Unified/Split 投影。
- **批次验证**：`cargo fmt --all`、`cargo check --locked`、结构门禁、UI policy、根 UI、diff check 和基线 lint 豁免后的全库 lib Clippy 通过。沙箱外完整库测试为 539 通过、1 个既有 Chat 分页失败、2 忽略，没有本批新增失败。CI 对应的 `cargo check --workspace --all-targets --no-default-features --features renderer-femtovg --locked` 通过，仅保留 `settings_page.rs` 两组既有条件导入 warning；直接省略 renderer 的 `--no-default-features` 命令按 Slint 约束无法构建。
- **A/B 范围**：本批改变枚举大小与比较方式，但不改变 diff 内容、缓存身份、行物化上限、选区/Find 算法或滚动协议；使用完整领域行为与 Slint 投影 A/B，不运行无对应阈值的内存 fixture，也不据此声明性能变化。未执行真实 Provider PR 的手工审核操作。

### CQ-B8 实施记录（2026-09-08）

- **实现**：新增 `TextGeometryOptions`，以具名字段统一承载 text、rendered width、line pitch、font family/size/weight/italic 和 scale factor。`hit_test()`、`hit_test_wrapped()`、`range_geometry()`、`wrapped_range_geometry()` 四个 Rust facade 从最多九个位置参数收敛为参数对象加操作特有坐标/范围；`controller/diff_bindings.rs` 在既有 `PrDiffBridge` callback 边界组装对象。Slint callback 声明、内部 Parley 数值函数、`GeometryKey` 字段和 256 项 LRU 均保持不变。
- **A/B 行为刻画**：A（HEAD `01d7939`）为 text geometry 9/9、selection 10/10、PR Review 核心旅程 4/4；B 使用相同测试同样为 9/9、10/10、4/4。覆盖 mixed-script shaping、字体样式、scale、软换行命中/范围、选择投影和真实审核旅程，没有用只镜像参数字段赋值的测试扩充数量。
- **批次验证**：`workflow::pr_review` 为 79 通过、1 个性能采样测试按设计忽略；`cargo fmt --all -- --check`、`cargo check --locked`、结构门禁、UI policy、根 UI 编译、`git diff --check` 和基线 lint 豁免后的全库 lib Clippy 通过。沙箱外完整库测试为 539 通过、1 个既有 Chat 分页失败、2 忽略；失败仍是 `chat::interactive::pagination_tests::real_layout_prepend_and_bidirectional_scrolling` 在 `src/chat/pagination_tests.rs:174`，没有本批新增失败。本地 HTTP fixture 在沙箱外正常运行。
- **Clippy**：标准 `cargo clippy --lib --locked -- -D warnings` 仍先被未涉及的 `crates/usher-i18n/src/lib.rs:83` 既有 `unnecessary_lazy_evaluations` 阻断；继续豁免已登记的 `unnecessary_lazy_evaluations`、`collapsible_if` 和 `field_reassign_with_default` 后全库 lib Clippy 通过，CQ-B8 范围没有剩余诊断。
- **A/B 范围**：本批只整理 Rust facade 接口，不改变排版、缓存、后台 Find、viewport、虚拟化或 Slint 协议，因此采用几何/选区/审核旅程行为 A/B，不运行无对应阈值的性能 fixture，也不声明性能提升。选区、Find 高亮与换行布局的手工 UI 操作不由这些自动测试替代。
