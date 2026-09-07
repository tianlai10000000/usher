# 代码质量与可读性优化记录

记录日期：2026-09-07  
状态：待实施；本轮仅分析与记录，未修改业务代码。

## 范围与结论

本文补充 `claude-proposal.md`，关注代码表达、类型设计、局部重复、函数接口和可维护性，不重复展开架构、性能、发布与安全专项。

结论来自对 Git、Tag、Chat、Job、Toast 和文本几何代码的抽查，不代表全仓逐行审计。证据基于检查时的工作区，包含当时尚未提交的修改；行号可能随后续开发变化，应优先按文件和符号定位。

主要提升方向是：让类型承载更多语义，减少需要调用者记住的约定，并让同一业务规则只有一个实现。项目已有 `RepoType`、`PullRequestReviewStatus`、`LocalizedText`、`PlannedCommand` 等可沿用的范例。

## CQ-01：命令返回值混合业务数据与展示日志

**位置**

- `src/workflow/tag/controller.rs`：`run_command()`、`run_command_blocking()`。
- 同文件：`inspect_latest_remote_tag_live()`（检查时约 1846 行）、`inspect_latest_remote_tag()`（约 2066 行）。

**现状与问题**

异步命令执行返回展示日志字符串，其中混合 stdout、stderr、`Run ...` 前缀，并经过日志截断。`inspect_latest_remote_tag_live()` 又从这个 `list_log` 解析远程 Tag。同步实现则通过返回元组中的 stdout 单独解析，两条路径的输入语义不同。

展示日志的格式和截断策略因此会影响业务结果：如果最大语义版本所在记录被截掉，它就无法参与选择。这是由代码路径推导出的风险，本轮没有进行大规模 Tag 仓库的端到端复现。

此外，异步执行使用 `Result<String, String>`，取消、启动失败、执行失败最终都落到字符串；调用者还需要查询取消标志来解释失败原因。

**建议**

- 返回具名的命令结果，明确区分 stdout、stderr 和退出状态；展示日志独立生成。
- 解析 Tag 时消费原始 stdout 或专用增量解析结果，不能消费用于展示的截断日志。
- 用局部错误枚举区分取消、启动失败和非零退出，避免调用者根据外部标志重新推断结果。
- 原始输出的资源上限需要单独定义；不要通过无限保留输出解决日志与业务数据混用的问题。

**验收关注点**

- 改变日志前缀、展示格式或截断阈值，不影响 Tag 选择结果。
- stdout、stderr 混合输出时，只有 stdout 中的引用记录参与解析。
- 覆盖最大版本不在日志尾部、空结果、取消、启动失败和非零退出。

## CQ-02：有限集合使用字符串，缺少编译期约束

**位置**

- `src/git/model.rs`：`PullRequestDiffLine.kind`、`PullRequestDiffFile.change_type`（约 97、105 行）。
- `src/ai_review/controller.rs`：diff 文本构造处同时匹配 `"addition" | "added"`、`"deletion" | "removed"`。
- `src/workflow/tag/version.rs`：`next_semver_for_mode()`。
- `src/workflow/cd.rs`：`deployment_snapshot()`、`rollback_snapshot()`。

**现状与问题**

这些字段和参数表示有限且已知的集合，却以 `String` 或 `&str` 在业务逻辑中传递。拼写错误和新增分支无法获得穷尽性检查。版本递增把所有未知 mode 当作 patch；CD 阶段使用 `_ => {}` 忽略未知值。

**建议**

- 在 Rust 内使用 `DiffLineKind`、`FileChangeKind`、`VersionBump`、`DeploymentStage` 等枚举。
- Provider 输入边界负责别名归一化，Slint 输出边界负责转换到现有 UI 协议。
- 明确定义未知输入的处理策略：拒绝、显式 Unknown，或有文档依据的回退。
- 保留自由文本字段的字符串形式，例如标题、正文和搜索词；按实际语义选择类型。

**验收关注点**

- Provider 别名在输入边界统一，业务代码不再到处兼容不同拼写。
- 新增枚举成员时，遗漏处理的匹配能够被编译器发现。
- 现有 Slint 协议、持久化格式及有效输入行为保持兼容；修改回退行为时同步相应 spec。

## CQ-03：状态字段之间的关系依赖调用者维护

**位置**

- `src/chat/session.rs`：`ChatRequestState`（约 19 行）。
- `src/jobs/model.rs`：`JobStepData`（约 83 行）、`JobSnapshot`。
- `src/workflow/tag/controller.rs`：步骤运行、成功、失败、取消时的字段赋值。

**现状与问题**

Chat 为每种模式同时维护 `sending: bool` 和 `started_at: Option<Instant>`，现有方法始终同步修改二者，因此 `sending` 可以从开始时间是否存在推导。

Job 则分别保存 status、conclusion 和字符串 duration，调用方需要同步赋值。类型本身允许 `Queued + Success` 等无意义组合；耗时还混合空字符串、运行中的省略号和完成后的格式化文本。

这是维护风险；本轮没有发现 Chat 已经出现字段不一致的运行实例。

**建议**

- Chat 去掉冗余布尔值，通过 `Option<Instant>` 判断是否发送中。
- Job 先提供 `start()`、`succeed()`、`fail()`、`cancel()` 等方法，集中维护字段关系。
- 后续视需要采用 `Completed { conclusion, duration }` 等枚举变体，使完成结果与完成状态绑定。
- 内部保存 `Duration` 等语义值，UI 投影时再格式化；运行提示由状态决定。

**验收关注点**

- 覆盖开始、成功、失败、取消、再次运行的转换。
- 保持现有 Job 展开状态、日志请求状态、耗时显示和 Tag running 闩锁语义。
- 测试行为与状态约束，避免仅断言实现内部的字段赋值顺序。

## CQ-04：Toast 的大元组、长参数列表和重复回调

**位置**

- `src/toast.rs`：`parse_width()`（约 35 行）、`create_toast_with_defaults()`（约 57 行）、`apply_toast()`（约 132 行）、`setup_toast()`（约 185 行）。
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

## CQ-05：Tag 计划和解析规则存在重复及位置耦合

**位置**

- `src/workflow/tag/commands.rs`：`create_tag_plan()`、`remove_tag_plan()`。
- `src/workflow/tag/controller.rs`：`default_remove_tag_steps()`（约 1300 行）、`run_local_remove_tag_flow()`（约 1556 行）。
- 同文件：异步与同步远程 Tag 检查中的引用解析和最大版本选择循环。

**现状与问题**

create 返回包含名称、argv 和说明的 `PlannedCommand`，remove 却返回 `Vec<Vec<String>>`。删除流程通过 `zip` 将命令填入另一份手写步骤表，再按索引访问步骤。命令与步骤描述的对应关系依赖两个列表保持相同顺序。

远程 Tag 的引用解析和最大版本选择算法也在同步、异步路径各实现一次，修改规则时容易只改一处。

**建议**

- create/remove 统一返回 `PlannedCommand`，可执行步骤从计划生成。
- 明确区分额外的刷新、完成步骤，避免隐藏的位置约定。
- 抽取远程引用解析和最大版本选择的纯函数，由同步与异步路径共同调用。
- 配合 CQ-01，使共享解析器只接收语义一致的输入。

**验收关注点**

- 插入或重排命令时，名称、说明与执行命令仍对应。
- 保持 Git argv 参数边界与现有命令顺序，尤其是带空格的路径和描述。
- 统一覆盖 `v`/`V` 前缀、预发布/build qualifier、无合法 Tag、相同版本核心等情况，遵守当前 Tag spec 的版本规则。

## CQ-06：文本几何接口反复传递同一组位置参数

**位置**

- `src/workflow/pr_review/text_geometry.rs`：`with_layout()`、`with_layout_options()`、`hit_test()`（约 178 行）、wrapped hit test 和 range geometry 系列函数。

**现状与问题**

字体、字号、字重、斜体、缩放、宽度和多个坐标沿调用链反复传递，多处需要允许 `clippy::too_many_arguments`。多个相邻 `f32` 含义不同，交换顺序也可能通过编译；新增选项需要同步调整多层函数签名。

**建议**

- 按已有语义组织为 `TextStyle`、`LayoutOptions`、`Point` 等小结构。
- 将文本样式与布局约束分开，避免形成承载所有参数的大对象。
- 保留底层排版算法，通过具名字段改善调用处的可检查性。

**验收关注点**

- 既有 mixed-script、软换行、缩放、选区与命中测试通过。
- 样式和布局选项仍完整参与缓存身份计算。
- 涉及 Slint 回调适配时，手工检查选区、Find 高亮及换行布局。

## 建议实施顺序

| 顺序 | 项目 | 原因 | 状态 |
| --- | --- | --- | --- |
| 1 | CQ-04 Toast 结果对象与重复回调 | 范围小、边界明确，适合作为低风险整理起点 | 待处理 |
| 2 | CQ-01 命令输出与展示日志分离 | 已涉及业务正确性，应在 Tag 后续改动前处理 | 待处理 |
| 3 | CQ-05 Tag 计划和解析统一 | 可接续命令结果整理，减少位置约定及重复规则 | 待处理 |
| 4 | CQ-02 有限集合枚举化 | 按 feature 分批完成，逐步收敛输入和输出边界 | 待处理 |
| 5 | CQ-03 状态关系封装 | 先集中状态转换，再评估是否进一步改为枚举 | 待处理 |
| 6 | CQ-06 文本几何参数分组 | 依靠已有几何测试保护行为，独立整理函数接口 | 待处理 |

以上是实施便利性排序。CQ-01 的正确性风险优先于纯可读性清理；如果正在修改 Tag，应先处理该项。

## 分析时的验证记录

以下结果来自 2026-09-07 的代码分析轮次，不代表未来实施后的验收结果：

- `cargo fmt --all -- --check`：通过；使用检查模式，未格式化或改动用户文件。
- `cargo check --locked`：通过。
- `git diff --check`：通过。
- `cargo test --lib --locked`：502 通过、2 失败、2 忽略。两项失败均为 `project::logo_loader::tests` 在沙箱内绑定 `127.0.0.1:0` 被拒绝，报 `PermissionDenied / Operation not permitted`。
- 沙箱外定向重跑 `cargo test --lib --locked project::logo_loader::tests -- --nocapture`：2 项均通过。未在沙箱外重新执行全部库测试。
- 测试链接器报告 `__eh_frame` 超过 compact unwind 编码大小限制的警告。
- 未执行手工 UI 验证和完整 doctest；仓库约定中已有的 logger doctest 问题不属于本轮验证结论。

本轮保留全部已有工作区修改。后续实施应遵循 `AGENTS.md`，按涉及的功能运行相关回归检查；行为或所有权变化时同步中英文架构文档，稳定约束变化时同步 spec 及索引。
