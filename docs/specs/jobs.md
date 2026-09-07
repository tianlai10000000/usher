# Reusable job runs / 可复用 Job Run

## Scope / 范围

This specification defines the application-wide Job Run boundary used by Tag
and future executable features. It covers immutable step input, runtime
snapshots, source-independent routing, ownership, presentation, and observable
lifecycle state.

本文定义 Tag 与后续可执行 Feature 共用的应用级 Job Run 边界，覆盖不可变步骤输入、
运行时快照、来源无关路由、职责归属、展示以及可观察的生命周期状态。

GitHub Actions is the reference model: a workflow contains jobs; a job contains
ordered steps; both jobs and steps expose status, conclusion, start time, and
completion time. A `run` step is a process and may contain multiple command
lines. GitHub reuses step sequences through composite actions and reuses complete
job/workflow orchestration through reusable workflows.

GitHub Actions 是参考模型：Workflow 包含 Job，Job 包含有序 Step；Job 与 Step
都暴露 status、conclusion、开始时间和完成时间。`run` Step 是进程且可包含多行
命令；GitHub 通过 Composite Action 复用步骤序列，通过 Reusable Workflow
复用完整 Job/Workflow 编排。

References:

- [Workflow jobs REST model](https://docs.github.com/en/rest/actions/workflow-jobs)
- [Workflow syntax: jobs and steps](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax)
- [Creating a composite action](https://docs.github.com/en/actions/tutorials/create-actions/create-a-composite-action)
- [Reusing workflow configurations](https://docs.github.com/en/actions/reference/workflows-and-actions/reusing-workflow-configurations)
- [Workflow commands and log annotations](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-commands)
- [Using workflow run logs](https://docs.github.com/en/actions/how-tos/monitor-workflows/use-workflow-run-logs)
- [Primer semantic color usage](https://primer.style/product/getting-started/foundations/color-usage/)

## Contract / 契约

1. The immutable input is an ordered `Vec<JobStepDefinition>`. Every definition
   contains only a display name and a command argument vector. Commands must not
   be flattened into a shell string for execution.
2. Runtime data is separate from definitions. `JobStepData` adds status,
   conclusion, detail, bounded log output, measured duration, and the initial
   disclosure state.
3. `JobSnapshot` is the single source of truth for one execution. It owns job
   status, optional conclusion, total measured duration, and ordered step
   snapshots.
4. Status follows GitHub's lifecycle boundary: `queued`, `in_progress`,
   `completed`. A completed job or step carries `success`, `failure`, or
   `cancelled`; an unfinished item has no conclusion.
5. `ui/components/jobs/job.slint` is presentation-only. It performs no process,
   filesystem, network, or timing work. Its public `execution-status`,
   `conclusion`, and `total-duration` properties let the owner bind or read the
   complete workflow result without inspecting rows.
6. A feature controller owns domain planning, command execution, cancellation,
   log bounds, and progress publication. It converts a `JobSnapshot` to Slint
   values only on the event loop. Tag, CD, and branch-build controllers may
   differ without changing the reusable Job component.
7. Adding a new status presentation or step metadata extends the generic model
   and presenter. Adding a new workflow creates definitions and a feature
   controller; it must not copy the Job UI or add domain branches to it.
8. Step disclosure state is local presentation state. Disclosures are
   independent, matching GitHub Actions; opening one step does not close another.
9. Raw process output remains backend-owned. The presenter requests a step's
   lazy Slint line model only on its first expansion. A failed step may opt into
   `default_expanded`, which also materializes its log. Once requested, the
   latch survives progress and completion snapshots for that project.
10. Process stdout and stderr are consumed while the process is running.
    Controllers publish bounded snapshots at a throttled cadence, and an already
    loaded disclosure receives those lines without waiting for step completion.
11. Log semantics follow GitHub workflow commands. `notice`, `warning`, `error`,
    and `debug` map to Primer accent, attention, danger, and muted presentation.
    Conventional `warning:`, `error:`, `fatal:`, `notice:`, and `debug:` process
    prefixes use the same mapping. Warning, error, and notice rows retain their
    semantic muted background; ordinary rows use the neutral hover wash. Every
    source line uses one fixed line-number column and one text block; semantic
    prefixes are measured and wrapped with their message so all rows stay aligned.
12. An in-progress step uses a continuously rotating attention-colored spinner.
    A queued step has an empty status ring but no disclosure, pointer cursor, or
    hover wash. Completed/running steps are interactive only when output can be
    shown.
13. CD may adapt its project-scoped stage snapshot to `JobStepItem` for the
    common visual presenter, but the presenter must not mutate deployment state
    or start callbacks. Branch validation is not an executable Job; it must not
    fabricate running Steps until a controller owns real execution state.
14. `Route.JobRun` is an application-level route, not a child of Workflow and
    not a member of the Workflow lifecycle domain. Any page may navigate to it.
    Global Router history returns Back to the exact source route.
15. A source publishes a `JobRunDescriptor` and `JobSnapshot` through
    `src/jobs::apply_job_run`. The descriptor provides a stable source key,
    title, workflow/process label, context label, selected job name, and trigger
    detail; the run page must not read properties owned by a particular feature.
16. Source-specific execution state, process ownership, cancellation, and log
    retention remain in the source feature. `src/jobs/` only owns the normalized
    model, UI projection, and source-keyed action dispatch for Stop and lazy log
    loading.
17. The run layout follows GitHub Actions: run status and trigger metadata in
    the header, a Jobs navigation region, and the selected Job's ordered Step
    log surface. Wide layouts use a bounded 216 px Jobs region and a flexible log
    pane inside the 1280 px wide-page limit. On narrow viewports the Jobs region
    stacks above the log without changing execution state or rebuilding the model.
18. A configuration page must not embed the Job Run surface. After a valid start
    is accepted, it publishes the run and navigates to `Route.JobRun`; it may
    expose a secondary action that reopens the current/latest run.

1. 不可变输入为有序 `Vec<JobStepDefinition>`，每项只包含展示名称与命令 argv；
   执行时不得把命令压平成 shell 字符串。
2. 运行时数据与定义分离。`JobStepData` 在定义之外增加 status、conclusion、
   detail、有界日志、实测耗时和初始展开状态。
3. `JobSnapshot` 是单次执行的唯一事实来源，统一持有 Job status、可选
   conclusion、总实测耗时和有序 Step 快照。
4. Status 遵循 GitHub 生命周期：`queued`、`in_progress`、`completed`。
   已完成 Job/Step 的 conclusion 为 `success`、`failure` 或 `cancelled`；
   未完成项没有 conclusion。
5. `ui/components/jobs/job.slint` 只负责展示，不执行进程、文件、网络或计时。
   外部通过公开的 `execution-status`、`conclusion`、`total-duration` 绑定或
   读取完整流程结果，无需检查每一行。
6. 具体 Feature controller 负责领域计划、命令执行、取消、日志上限和进度发布，
   并且只在 event loop 把 `JobSnapshot` 转为 Slint 数据。Tag、CD、分支构建可以
   使用不同 controller，不需要修改通用 Job 组件。
7. 新增通用状态展示或 Step 元数据时扩展通用 model/presenter；新增业务流程只创建
   definitions 与对应 controller，不得复制 Job UI，也不得向 Job 中添加业务分支。
8. Step 展开状态属于本地展示状态；与 GitHub Actions 一致，各 Step 独立展开，
   打开一个 Step 不会关闭其他 Step。
9. 原始进程输出由后端持有。展示层仅在 Step 第一次展开时请求 Slint 懒加载行
   model；失败 Step 可通过 `default_expanded` 自动构造日志。请求标记在同项目的
   progress 与 completion 快照替换时必须保留。
10. 进程运行期间持续读取 stdout/stderr。Controller 以受控频率发布有界快照，
    已加载的展开区无需等待 Step 完成即可看到新日志。
11. 日志语义遵循 GitHub workflow command：`notice`、`warning`、`error`、
    `debug` 分别映射 Primer accent、attention、danger、muted；常见进程前缀
    `warning:`、`error:`、`fatal:`、`notice:`、`debug:` 使用相同映射。
    Warning、Error、Notice 行始终保留对应的 muted 背景，普通行才使用中性 hover。
    每条源日志统一使用固定行号列和单一正文块；语义前缀与消息共同测量和换行，保证
    所有日志行严格对齐。
12. 运行中 Step 使用持续旋转的 attention 色 Spinner。queued Step 显示空心
    状态环，但不得出现 disclosure、手型光标或 hover 背景；只有确实可展示输出的
    running/completed Step 才可交互。
13. CD 可以把项目级 Stage Snapshot 适配为 `JobStepItem` 交给共用视觉展示，但
    Presenter 不得修改部署状态或启动 callback。Branch 校验不是可执行 Job；在
    Controller 尚未持有真实执行状态前，不得伪造运行中的 Steps。
14. `Route.JobRun` 是应用级路由，不是 Workflow 子页面，也不属于 Workflow
    生命周期域；任何页面都可进入，Back 由全局 Router 历史精确返回来源路由。
15. 来源 Feature 通过 `src/jobs::apply_job_run` 发布 `JobRunDescriptor` 与
    `JobSnapshot`。描述符提供稳定 source key、标题、流程名、上下文名、选中 Job
    名称与触发信息；运行页不得读取某个具体 Feature 的属性。
16. 领域执行状态、进程所有权、取消和日志保留仍归来源 Feature；`src/jobs/` 只拥有
    归一化 model、UI 投影，以及按 source key 分发 Stop 和懒加载日志动作。
17. 页面布局遵循 GitHub Actions：页头展示运行状态和触发元数据，Jobs 区展示当前
    Job，主区展示选中 Job 的有序 Step 日志；宽屏在 1280 px 宽页上限内使用固定
    216 px Jobs 区与自适应日志区；窄屏把 Jobs 区堆叠到日志上方，不得改变执行状态
    或重建 model。
18. 配置页面不得内嵌 Job Run。合法启动被接受后，先发布运行数据再导航到
    `Route.JobRun`；配置页可提供次级操作重新打开当前或最近一次运行。

## Ownership / 职责

- `src/jobs/model.rs`: reusable definition, runtime snapshot, lifecycle, and
  compact duration formatting.
- `src/jobs/mod.rs`: source-independent descriptor, UI projection, and
  source-keyed Stop/log action dispatch.
- `ui/components/jobs/job.slint`: reusable `GitHubActionsJob` and Step log
  presentation.
- `ui/components/jobs/github_job_run.slint`: GitHub Actions-style run layout.
- `ui/views/job_run.slint`: application-level `Route.JobRun` composition.
- Feature controllers such as `src/workflow/tag/controller.rs`: domain
  orchestration and side effects.
- `src/workflow/state.rs`: project-scoped ownership of feature Job snapshots.

- `src/jobs/model.rs`：通用定义、运行时快照、生命周期和紧凑耗时格式。
- `src/jobs/mod.rs`：来源无关描述符、UI 投影和按 source key 分发 Stop/日志动作。
- `ui/components/jobs/job.slint`：可复用的 `GitHubActionsJob` 与 Step 日志展示。
- `ui/components/jobs/github_job_run.slint`：GitHub Actions 风格运行页布局。
- `ui/views/job_run.slint`：应用级 `Route.JobRun` 装配。
- `src/workflow/tag/controller.rs` 等 Feature controller：领域编排与副作用。
- `src/workflow/state.rs`：按项目隔离持有各 Feature 的 Job 快照。

## Regression checklist / 回归清单

- Run `cargo test --lib --locked jobs`.
- Compile the complete root Slint UI.
- Verify queued → in-progress → completed transitions and all conclusions.
- Verify total duration is readable independently of step rows.
- Verify argv boundaries survive spaces in paths and values.
- Verify multiple steps can remain expanded and multiline logs preserve one
  source line number per logical line.
- Verify the first expansion materializes existing output and subsequent output
  appears live without reconstructing unopened logs.
- Verify workflow-command and conventional warning/error prefixes receive the
  correct Primer semantic row treatment.
- Verify the running spinner rotates when animations are enabled and queued
  steps have no hover or click behavior.
- Verify entry from Workflow and UI Components, then Back to each exact source.
- Verify Stop and lazy log loading dispatch only to the active source key.

- 运行 `cargo test --lib --locked jobs`。
- 编译完整 Slint 根 UI。
- 验证 queued → in-progress → completed 以及全部 conclusion。
- 验证外部无需读取 Step 行即可取得总耗时。
- 验证路径和参数含空格时 argv 边界不变。
- 验证多个 Step 可同时展开，多行日志按逻辑行保留一个源行号。
- 验证第一次展开才构造已有日志，之后的输出实时出现，未展开日志不被构造。
- 验证 workflow command 与常见 warning/error 前缀采用正确的 Primer 语义行样式。
- 验证动画启用时运行 Spinner 持续旋转，queued Step 没有 hover 或点击行为。
- 分别从 Workflow 与 UI Components 进入并验证 Back 精确返回各来源。
- 验证 Stop 与日志懒加载只分发给当前 source key。

## Output and viewport budgets / 输出与视口预算

- Worker log mutations prepare semantic lines incrementally, reusing unchanged
  lines; the event loop only converts requested visible rows. Step/log model
  identities remain stable across progress, append and tail eviction.
- Slint conversion cache: at most 128 rows. Expanded log panes: at most 420 px,
  using public ListView and the existing GitHub scrollbar. Source numbers,
  wrapping and semantic colors remain; long wrapped rows scroll inside the pane.
- Tag transport: two 4 KiB readers, a shared queue of 16 blocks, at most three
  UTF-8 carry bytes per stream, and a 32 KiB combined tail including truncation.
  Overflow discards oldest display bytes; it does not grow the queue or line buffer.
- Continue consuming while readers complete. Cancellation aborts and joins both
  readers; inherited pipes after child exit have a two-second drain deadline.
  Deadline expiry returns a failure with an explanatory bounded log.
- Regression: split UTF-8/CRLF, short-line bursts, multi-MiB lines, both streams,
  queue backpressure, exit/Stop, first expansion, collapse/reopen and scrolling.

- Worker 日志变更增量准备语义行并复用未变化行；event loop 只转换请求的可见行。
  Progress、追加与尾部淘汰保持 Step/log model 身份。
- Slint 转换缓存最多 128 行，展开面板最高 420px，复用公开 ListView 与现有
  GitHub 滚动条。保留源行号、换行和语义色；较长的换行行在面板内滚动。
- Tag 两个 Reader 每次读取 4 KiB，共用 16 块队列，每流 UTF-8 跨块残留最多 3 字节；
  合并尾日志连同截断标记最多 32 KiB。超限淘汰最旧展示字节，不扩大队列或整行缓冲。
- 等待 Reader 时继续消费。取消必须 abort/join 两个 Reader；直接子进程退出后
  继承管道排空限时 2 秒，超时返回失败及有界解释日志。
- 回归覆盖 UTF-8/CRLF 拆分、短行突发、数 MiB 单行、双流、背压、退出/Stop、
  首次展开、折叠重开及滚动。
