# Reusable workflow jobs / 可复用 Workflow Job

## Scope / 范围

This specification defines the reusable Job boundary used by Tag and future CD,
branch-build, and other deterministic workflows. It covers immutable step input,
runtime snapshots, ownership, presentation, and observable lifecycle state.

本文定义 Tag 以及后续 CD、分支构建等确定性流程共用的 Job 边界，覆盖不可变步骤输入、
运行时快照、职责归属、展示以及可观察的生命周期状态。

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

## Ownership / 职责

- `src/workflow/jobs/model.rs`: reusable definition, runtime snapshot, lifecycle,
  and compact duration formatting.
- `ui/components/jobs/job.slint`: reusable GitHub Actions-style Job and Step log
  presentation.
- Feature controllers such as `src/workflow/tag/controller.rs`: domain
  orchestration and side effects.
- `src/workflow/state.rs`: project-scoped ownership of feature Job snapshots.

- `src/workflow/jobs/model.rs`：通用定义、运行时快照、生命周期和紧凑耗时格式。
- `ui/components/jobs/job.slint`：可复用的 GitHub Actions 风格 Job/Step 日志展示。
- `src/workflow/tag/controller.rs` 等 Feature controller：领域编排与副作用。
- `src/workflow/state.rs`：按项目隔离持有各 Feature 的 Job 快照。

## Regression checklist / 回归清单

- Run `cargo test --lib --locked workflow::jobs`.
- Compile the complete root Slint UI.
- Verify queued → in-progress → completed transitions and all conclusions.
- Verify total duration is readable independently of step rows.
- Verify argv boundaries survive spaces in paths and values.
- Verify multiple steps can remain expanded and multiline logs preserve one
  source line number per logical line.

- 运行 `cargo test --lib --locked workflow::jobs`。
- 编译完整 Slint 根 UI。
- 验证 queued → in-progress → completed 以及全部 conclusion。
- 验证外部无需读取 Step 行即可取得总耗时。
- 验证路径和参数含空格时 argv 边界不变。
- 验证多个 Step 可同时展开，多行日志按逻辑行保留一个源行号。
