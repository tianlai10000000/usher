# Usher 项目结构审计（2026-08-29）

> 本文是基于当前工作树的阶段性结构审计，不是稳定行为规格。稳定约束仍以
> `docs/specs/` 为准，模块所有权仍以中英文架构文档为准。

> 2026-09-16：第 64 批已核查当前四项结构超限，职责方案与验证记录见
> [P6 #64](p6-64-quality.md)。以下日期章节保留历史语境；当前 CI 仅手动触发，
> 不再由 PR/push 自动运行。第 64 批不提高结构预算，超限继续返回失败。

## 结论

项目不需要推倒重来。现有架构已经具备四条正确主干：

1. Slint 只负责展示，Rust 负责 I/O、状态和编排。
2. HTTP client、Tokio runtime、关闭流程等横切能力已经集中管理。
3. Workflow 与 Terminal 的关键状态按项目隔离。
4. `usher-ui` 已形成生成 UI API 的独立编译边界。

当前主要风险仍是少数热点文件拥有过多“变化原因”，但 7 月完成拆分后，新增功能
已经让多个热点明显回流。此前“结构治理可以停止”的结论不再成立。后续仍应保持
公开 façade，以可独立测试的状态、纯逻辑和适配器为单位渐进迁移，避免一次性改动
模块 API、异步时序和 Slint callback。

## 审计方法

本次检查了：

- 中英文 README、架构文档和全部 spec 索引；
- Rust/Slint 文件规模、函数与测试分布；
- feature 之间的直接依赖；
- reqwest client 和 Tokio runtime 的创建位置；
- `src/lib.rs` 的启动顺序和 callback 装配；
- Project、Workflow、Git Provider、Terminal、Chat 的状态所有权。

2026-08-29 复核时 `cargo check --locked` 可通过。PR CI 已覆盖 fmt、workspace
tests、clippy 和 disabled-feature check；没有发现 `src/http/mod.rs` 外创建应用级
reqwest client，也没有发现 `src/app_runtime.rs` 外创建 Tokio runtime。

## 热点与判断

| 热点 | 当前规模 | 判断 | 优先级 |
| --- | ---: | --- | --- |
| `ui/views/workflow/pr_review_workspace.slint` + `ui/components/pr_review/` | 页面编排 4379 行 | 页面根仍持有大量跨区域状态；独立 package contract 已存在，但生产页面尚未形成独立生成 crate | P3 继续治理 |
| `src/workflow/pr_review.rs` + `pr_review/` | façade 13 行；controller 4451 行 | cache/diff/avatar 等已有边界，但控制器新增 action、fixture 和 UI 编排后再次增长 | P3 继续治理 |
| `src/git/gitee/` | façade/用例编排 3715 行；conversation 960 行 | transport/decode/readiness/revert 边界仍有效，但 Provider 用例和会话逻辑明显回流 | P3 继续治理 |
| `src/workflow/tag/` | façade 7 行；controller 1698 行；commands 101 行；version 49 行 / 新增 4 个纯逻辑测试 | UI/取消编排与 SemVer、Git argv 计划分离；命令参数不经 shell 拼接 | 已完成 P1 |
| `src/project/` | façade 13 行；controller 928 行；logo 421 行；model 125 行；storage 78 行；overview 50 行 | 保存/UI 编排保留在 controller；模型、JSON 存储、Logo 和 overview 任务选择已独立 | 已完成 P2 |
| `src/git/overview.rs` | 约 120 行；3 个纯逻辑测试 | Project/Workflow 共用 Git façade；分页聚合只消费归一化 PR model，旧 `project/pr_stats.rs` 已删除 | 已完成收敛 |
| `src/git/mod.rs` + `src/git/{model,locator,diff,json,provider}.rs` | façade 约 900 行；共享 JSON helper 与 Provider adapter 已独立 | `RepoContext`、对象安全 `GitProvider` 和 `ProviderCapabilities` 已统一读写分发；Provider endpoint/解码仍归各 adapter | 已完成 P3 #27 |
| `src/chat/interactive.rs` + `virtual_layout.rs` | UI/请求编排 2104 行 | 虚拟布局边界仍有效，但持久化、请求与 UI 编排继续集中增长 | 门禁守护 |
| `src/terminal/` | controller 1078 行；input 92 行；diagnostics 78 行；refresh 47 行，另有 render/session | PTY/session 时序保持原位；输入字节、诊断格式和刷新节流策略可独立测试 | 已完成 P2 |
| `src/router/mod.rs` + `src/app_lifecycle.rs` | Router 249 行；lifecycle hook 34 行 | Router 只拥有 transition/history/reachability；跨 feature 清理由 composition root 注入 | 已完成 P2 |

文件长度本身不是拆分依据。P1 文件的问题是同时包含三种以上独立变化原因，
并且这些变化由不同测试策略验证。

## 2026-08-29 回流结论与门禁

7 月审计记录的 `src/lib.rs` 635 行、PR Review controller 3630 行、Gitee façade
2263 行和 Chat controller 871 行，现分别为 1065、4451、3715 和 2104 行。
这不是一次单纯的代码搬迁：PR/AI Review、会话归一化、虚拟化与持久化功能持续进入
原控制器，说明原先只靠文档约束无法守住边界。

`scripts/check-architecture-structure.sh` 现在把当前热点行数与函数/根 UI contract
规模记录为带少量余量的可执行预算，并继续守护 HTTP client 与 Tokio runtime 的
唯一创建位置。PR CI 执行该脚本。预算触发时应优先拆出已有独立变化原因；确需增长时，
必须在同一改动中说明所有权并更新本审计基线，不能静默抬高数字。

本轮 P3 已完成共享 Provider JSON helper 与对象安全 Provider adapter；其 PR Review 六类
state-store generation、AI Review enabled/disabled 公共 contract、生成体积工具的
`@ai-review` 注册，以及任务驱逐后的 session 文件清理。独立 PR Review 生成 crate、
完整 objc2 迁移和剩余传递依赖去重仍是独立高风险阶段，必须分别建立回归护栏后推进。
P4-A 随后把远端 action-state 读取与 mutation 拆分，使 PR Review 当前使用七类独立
generation，避免只读刷新作废已提交的远端写操作。

## 2026-09-08 P5-D 职责收敛

在 `8b29a8f` 上，PR Review controller 已回涨到 5315 行、Chat 2650 行、
AI Review 3933 行、根 Rust 1117 行，根 Slint callback 为 149 个。
#62 按远端动作、Diff event-loop 绑定、诊断、Chat 视口策略/稳定消息 model、
AI Session I/O 和 Settings 保存反馈提取已有职责后，分别为
4287、2165、3846、1034 行及 135 个 callback。既有测试保留原位，原门禁预算
不变；提取模块增加独立规模和反向 controller 依赖检查。远端 action 方法仍是
controller 子模块实现，以保留现有七类 generation 与生命周期。

#63 为 `main` push 接入原 PR 质量门禁，并将真实 Project、Job、Preview、Find
夹具纳入 Nightly。所有权以中英文架构与对应 spec 为准，实测及验收缺口见
[`p5-d-performance.md`](p5-d-performance.md)，后续完成状态以
[`claude-proposal.md`](../claude-proposal.md) 第 20 节为准。以下 7 月路线是历史
记录，不再作为停止结构治理的当前依据。

## 2026-07 已完成的结构调整（历史基线）

截至路线图第 18 批，已完成以下行为不变的边界调整：

1. 新建 `src/platform/`。跨平台 façade 位于 `mod.rs`，macOS AppKit 外观、
   标题栏附件、窗口交互通知和字体注册位于 `macos.rs`。
2. 把根窗口 Slint model 占用统计从 `src/lib.rs` 移入
   `src/memory_diagnostics.rs`。
3. 保留 `WorkflowPrReviewController` façade，把固定源码对齐/hunk 展开与本地
   文件树/筛选分别移入 `src/workflow/pr_review/hunk.rs` 和
   `src/workflow/pr_review/file_tree.rs`。
4. 把 Unified/Split 行投影与 hunk 滚动锚点移入
   `src/workflow/pr_review/diff_projection.rs`，把选择命中、显示列切片和复制文本
   移入 `src/workflow/pr_review/selection.rs`；相关单元测试随职责移动。
5. 把只读 Slint 惰性行 model、64 行转换缓存和 Diff/token UI 适配移入
   `src/workflow/pr_review/diff_model.rs`，Controller 只提交构建好的 model。
6. 把 Conversation/Commit model、行高、连接/分组规则和 commit range 映射移入
   `src/workflow/pr_review/timeline.rs`；头像下载和 UI 原地展开仍由 Controller 编排。
7. 把 PR Review 的 bounded cache、共享 Diff 所有权、range/源码缓存和内存统计移入
   `cache.rs`，有界头像下载/解码移入 `avatar.rs`；`pr_review.rs` 收口为稳定 façade。
8. 把 PR Review 的公开 Slint struct、Header/Summary/Actions 与可复用的 File tree、
   Diff 行、工具栏、过滤器和大文件占位组件移入 `ui/components/pr_review/`；页面根
   保留必须共享的选择状态、滚动锚点和全窗口 overlay 编排。
9. 把 Git Provider-neutral 领域模型、remote/SSH 定位和统一 diff 解析分别移入
   `src/git/model.rs`、`locator.rs`、`diff.rs`。
10. 把 Gitee provider 收口为目录模块，认证与 endpoint 构造、JSON/diff 解码、
    会话/提交映射、merge readiness 和 revert capability 分别由 `transport.rs`、
    `decode.rs`、`conversation.rs`、`readiness.rs`、`revert.rs` 所有。
11. 把 Tag façade、UI/取消控制器、SemVer 规则和 Git argv 计划拆开，并为版本规则
    和命令顺序/参数边界补充单元测试。
12. 把 Project 收口为稳定 façade；模型、JSON 存储、Logo 和 PR overview 任务选择
    分别归入 `model.rs`、`storage.rs`、`logo.rs`、`overview.rs`。
13. 把 Chat 高度估算、prefix cache、增量 prepend/append 和窗口选择移入
    `virtual_layout.rs`，补空列表、长历史和 resize 宽度边界测试。
14. 把 Terminal 输入映射、诊断格式和刷新节流策略移入独立纯模块；实际
    event-loop 调度、PTY/session 和 shutdown 顺序保持不变。
15. 增加由 composition root 注入的轻量 route lifecycle hook；Router 不再直接
    清理 Workflow state、Project Logo 或 Slint feature 字段。
16. 把 Project/Workflow 的 PR 环境计数并入 Git façade；删除独立的阻塞式
    `project/pr_stats.rs`，由 `git/overview.rs` 聚合归一化分页并复用 Provider
    定位、持久化 metadata、鉴权与请求路径。

`src/lib.rs` 从 1476 行降到约 635 行，只保留启动顺序、跨模块 callback 编排、
生命周期和 shutdown 注册。总代码量不会因为搬迁显著下降，但 composition root
的变化原因已经减少，Windows/Linux 原生适配也有了明确扩展点。

## 批次路线图

路线图不再以“凑满 20 批”为目标。重新检查当前依赖方向、重复实现和测试收益后，
第 17 批已经构成结构治理基线；随后完成了唯一有明确重复证据的收敛项。截至当前：

- **已完成：18 批**
- **建议执行：0 批**
- **可选守护：1 批**
- **P1：已全部完成**；
- **P2：已全部完成**；
- 原第 18 批“全面注入 `RepositoryContext`”从必做路线移除，改为满足触发条件后
  再立项，不作为 PR overview 收敛的前置。

批次是交付和验收单位，不等同于文件数量。一个批次可以同时移动多个高度相关的
模块，但不得同时改变稳定行为、Provider 协议或 Slint 公共 contract。

| 批次 | 状态 | 优先级 | 范围 | 主要交付 |
| ---: | --- | --- | --- | --- |
| 1 | ✅ 已完成 | P1 | Composition root 与平台边界 | 新建 `src/platform/`；原生 macOS 集成离开 `src/lib.rs`；Slint model 内存统计归入 `memory_diagnostics.rs` |
| 2 | ✅ 已完成 | P1 | PR Review Hunk 与文件树 | 新建 `hunk.rs`、`file_tree.rs`；固定源码对齐、展开/收起、树构建和本地筛选独立测试 |
| 3 | ✅ 已完成 | P1 | PR Review 投影与选择 | 新建 `diff_projection.rs`、`selection.rs`；Unified/Split 映射、滚动锚点、选择命中和复制文本独立测试 |
| 4 | ✅ 已完成 | P1 | PR Review Diff UI adapter | 新建 `diff_model.rs`；Slint lazy model、64 行缓存和 Diff/token 转换离开 Controller |
| 5 | ✅ 已完成 | P1 | PR Review Timeline | 新建 `timeline.rs`；Conversation/Commit model、行高、连接/分组和范围映射独立测试 |
| 6 | ✅ 已完成 | P1 | PR Review cache 与共享所有权 | 新建 `cache.rs`；迁移 `BoundedCache`、`ReviewCache`、`SharedDiffDetail`、range diff、expanded file 和内存统计 |
| 7 | ✅ 已完成 | P1 | PR Review 头像与 Controller 收口 | 新建 `avatar.rs`，隔离有界下载/解码；请求序列、异步编排和 UI 回写收口到 `controller.rs`，`pr_review.rs` 只保留 façade |
| 8 | ✅ 已完成 | P1 | PR Review Slint 外壳 | 公开 struct、Header、Summary、Actions 等稳定组件进入 `shell.slint`；根 property/callback 和 overlay 层级不变 |
| 9 | ✅ 已完成 | P1 | PR Review Slint 内容区 | File tree、Diff toolbar/line/view、过滤器等复用组件进入 `content.slint`；跨区选择、ListView identity 与滚动锚点仍由页面根编排 |
| 10 | ✅ 已完成 | P1 | Git 核心领域与定位 | 从 `src/git/mod.rs` 提取 `model.rs`、`locator.rs`、`diff.rs`；provider-neutral façade 不变 |
| 11 | ✅ 已完成 | P1 | Gitee 传输层 | Gitee 迁入目录模块；公有 v5/企业 v8 共用的认证、URL 与请求构建进入 `transport.rs`，继续使用统一 HTTP pipeline |
| 12 | ✅ 已完成 | P1 | Gitee 语义适配 | Diff/HTML/base64 解码、Conversation/Commit 归一化、merge readiness、revert capability 分别进入 `decode.rs`、`conversation.rs`、`readiness.rs`、`revert.rs`；既有 21 个 Gitee 测试通过 |
| 13 | ✅ 已完成 | P1 | Tag Workflow | `tag.rs` 收口为 façade；新增 `controller.rs`、`commands.rs`、`version.rs`，补 4 个 SemVer/argv 计划测试 |
| 14 | ✅ 已完成 | P2 | Project | `project/mod.rs` 收口为 façade；提取 model、storage、logo、overview，保持 project-scoped 状态与保存事务语义 |
| 15 | ✅ 已完成 | P2 | Chat 虚拟布局 | 高度估算、prefix cache、增量更新和可见窗口进入 `virtual_layout.rs`，新增空列表、长历史和 resize 宽度测试 |
| 16 | ✅ 已完成 | P2 | Terminal Controller | 提取 input mapping、refresh throttle policy 和 diagnostics；保持 PTY/session 所有权、event-loop 调度及关闭优先级 |
| 17 | ✅ 已完成 | P2 | Router 生命周期 | composition root 注入单一 lifecycle hook；Router 只产生 transition/reachability，不再调用 Workflow/Project 内部清理 |
| 18 | ✅ 已完成 | 收敛 | PR overview 统一 | 新增 `src/git/overview.rs` 和 provider-neutral open PR 计数用例；Project/Workflow 只消费归一化结果，删除 `project/pr_stats.rs` 的重复定位、认证和 Provider 请求分支，并补分支映射/分页终止测试 |
| 19 | ◇ 可选 | 守护 | 依赖边界检查 | 增加可本地执行的轻量脚本，检查新增 runtime/client 创建和 `codereview/api` 反向依赖；接入 PR CI 时再增加基于 diff 的 vendored UI 修改检查 |

### 推荐停止点

- 完成第 **13 批**：P1 热点完成，适合暂停结构治理并回到产品功能开发。
- 完成第 **17 批**：主要中型热点和生命周期耦合完成；这已经是当前的通用重构
  停止点，不应继续按文件长度拆分 Controller 或 Provider。
- 完成第 **18 批**：已消除最后一处确认存在的重复 Provider 知识；应回到功能开发。
- 第 **19 批**：不阻塞第 18 批，也不代表必须新增完整 CI 体系；仅在团队准备维护
  PR 检查流程或继续扩展 Provider 时执行。

批次顺序允许因业务需求调整，但必须遵守依赖关系：第 6～7 批先于 PR Review
Controller 最终收口；第 10 批先于第 11～12 批。第 18 批现已完成；若执行可选的
第 19 批，应以当前结构作为边界检查基线。

### 2026-07-28 路线重评

这次调整不是完成状态更新，而是对原方案的三个前提重新判断：

1. **全面 `RepositoryContext` 注入目前收益不足。** 重评时的真实重复集中在
   `src/project/pr_stats.rs`，现已由第 18 批消除；主 Git façade 通过 `locator.rs`
   读取 Settings 虽然不是最纯粹的依赖方向，但尚未形成两套行为，也没有多账户、
   项目级凭据或 headless 调用需求。现在修改所有 Provider 方法签名仍会扩大变更
   面，却没有对应的现实收益。
2. **PR overview 可以独立收敛。** 原 `pr_stats.rs` 同时实现 remote 解析、
   Provider 判断、token/profile 回退、API 请求和 branch 过滤，而 `src/git/`
   已拥有对应 Provider adapter。第 18 批已新增窄的计数 use case，复用现有定位、
   持久化 Provider metadata、认证和归一化分页，不引入新的上下文对象。
3. **边界守护应匹配现有工程成熟度。** 仓库目前只有 release workflow。先提供
   可本地运行、基线可通过的静态检查更合适；“vendored UI 不得修改”需要比较 PR
   diff，不能用扫描当前文件树的脚本可靠判断，因此只在建立 PR CI 时接入。

`RepositoryContext` 保留为条件式设计选项。仅在出现以下任一需求时重新立项：

- 同一进程需要多账户或项目级凭据；
- Provider adapter 需要脱离全局 Settings 做契约测试；
- 一次操作必须固定 Settings 快照，避免执行期间配置变化；
- 新增 CLI/headless 入口，需要在没有 UI 全局状态时调用 repository service。

届时应只在 Git façade 边界构建不可变上下文，再传给 adapter；不要把 Settings、
token 或通用 service locator 传播到 UI 和 Workflow。

## 建议目标结构

```text
src/
  lib.rs                         # composition root，只决定装配顺序
  app_lifecycle.rs               # route transition 后的跨 feature 清理 hook
  platform/
    mod.rs                       # 跨平台 façade 与 fallback
    macos.rs                     # AppKit
    windows.rs                   # 需要时新增，不提前创建空实现
  workflow/
    pr_review.rs                 # 稳定 façade
    pr_review/
      controller.rs              # request sequence、异步编排、UI 回写
      cache.rs                   # bounded cache 与共享 diff 所有权
      avatar.rs                  # 有界下载、解码与 send-safe 像素
      diff_projection.rs         # Unified/Split 纯行投影和滚动锚点
      diff_model.rs              # Slint lazy model 与领域行到 UI 行适配
      hunk.rs                    # context expand/collapse
      selection.rs               # hit test 与 copy text
      file_tree.rs               # tree/filter/extension
      timeline.rs                # Conversation/Commits view model
    tag.rs                       # 稳定 callback façade
    tag/
      controller.rs              # 流程状态、取消、重复启动保护
      commands.rs                # 纯 Git argv 计划
      version.rs                 # semver 解析和递增
  git/
    mod.rs                       # Provider-neutral façade
    model.rs                     # 领域记录和 capability
    locator.rs                   # remote URL/SSH 解析和 Provider 校验
    diff.rs                      # Unified diff 解析
    overview.rs                  # open PR 环境计数与归一化分页聚合
    github.rs
    gitlab.rs
    gitee/
      mod.rs                     # Public v5 / Enterprise v8 分发和用例编排
      transport.rs               # 鉴权、endpoint 与请求构建
      decode.rs                  # JSON/HTML/base64 -> domain
      conversation.rs            # Conversation/Commit 归一化
      readiness.rs               # Provider 独立合并语义
      revert.rs                  # 回退 capability 语义
  project/
    mod.rs                       # setup_project façade
    controller.rs
    model.rs
    storage.rs
    logo.rs
    overview.rs
  chat/
    interactive.rs               # request、timer 与 UI 编排
    virtual_layout.rs            # 高度估算与有界窗口
  terminal/
    mod.rs                       # controller、PTY/UI 调度
    input.rs                     # 键盘到 PTY bytes
    diagnostics.rs               # 纯诊断格式
    refresh.rs                   # 帧节流策略
```

Slint 对应建议：

```text
ui/views/workflow/pr_review_workspace.slint  # 只做页面编排和根 contract
ui/components/pr_review/
  shell.slint                    # 公开 struct、Header/Summary/Actions
  content.slint                  # File tree、Diff、过滤器等复用内容组件
```

不要按每个小组件建文件。只有能独立描述输入 property、输出 callback、视觉职责
和测试场景的区域才拆分。

## 关键耦合问题

### 1. Router 知道 Workflow 和 Project 的内部清理细节

已解决。`src/router/mod.rs` 只计算 history、transition 与 Workflow reachability；
`src/lib.rs` 注入 `src/app_lifecycle.rs` 的单一 hook，由 composition 层协调
Workflow heavy-state 和相关 UI 清理。没有引入全局事件总线。

### 2. Provider façade 仍从全局 Settings 拉取凭据

`src/git/locator.rs` 和各 Provider adapter 仍通过统一 helper 读取 Settings。
这会限制未来的多账户和脱离 UI 的契约测试，但当前只有一套配置来源，尚未导致
主调用链出现重复实现。现阶段不应为了理想依赖方向全面修改 Provider 签名。
保持读取入口集中；只有出现本节路线重评列出的触发条件时，才引入不可变
`RepositoryContext`。

### 3. PR overview 与完整 PR repository service 存在重复 Provider 知识

已解决。原 `src/project/pr_stats.rs` 的 Provider 请求与认证分支已经删除。
`src/git/overview.rs` 通过窄的 repository service use case 聚合 open PR：
GitLab/Gitee 复用归一化分页，GitHub 复用 search 的精确总数；Project 和 Workflow
只提供持久化 repository metadata 并消费 `fat`/`uat`/`prod` 计数。

### 4. 根 UI contract 很宽

Feature adapter 大量通过 crate 根重新导出的 Slint 类型和 `MainWindow` 读写 property。
当前不应引入抽象 trait 全面包裹 Slint；那只会产生镜像接口。更有效的做法是让
每个 feature 暴露一个 controller/setup façade，并把纯算法移到不依赖
`MainWindow` 的模块，使核心逻辑可直接单测。

## 分阶段执行

### P1：先降低最高变更冲突

对应路线图第 1～13 批，现已全部完成。最高冲突文件均具备稳定 façade、明确的
内部所有权和对应单元测试；专项结构治理可在此暂停，后续按业务需求进入 P2。

### P2：清理中等热点

对应路线图第 14～17 批，现已全部完成。拆分止于明确的模型、纯算法和生命周期
边界；未继续把 controller 机械切碎，也未引入通用 service/trait/event bus。

### 后续：按证据收敛，不再扩大 P3

第 17 批后不再设置一组必须完成的扩展性重构。第 18 批已消除 PR overview
重复；第 19 批仍是可选守护，不要求为此先搭建完整 CI。除此之外，Provider、
PR Review Controller、Tag Controller 和 Slint 页面根即使仍然较大，也应等待新的
独立变化原因、测试阻塞或复用需求再拆。

后续批次仍只做职责迁移、测试补齐和可见性收紧，不同时修改交互或 Provider
协议。若业务改动本身需要改变行为，应单独立项，不借结构批次混入。

## 拆分验收标准

每个拆分 PR 应满足：

- 对外 façade 和 Slint callback/property 名称不变；
- async sequence guard、取消和 project scope 不变；
- 纯逻辑模块不依赖 `MainWindow`、全局 Settings 或 HTTP；
- 测试随被测逻辑移动，不通过 `pub` 扩大内部 API；
- `cargo fmt --all`、`cargo check --locked`、`cargo test --lib --locked`、
  `git diff --check` 全部通过；
- PR Review/Terminal 等交互按对应 spec 做手工回归。

不建议设定机械的“单文件不得超过 N 行”。更有效的门槛是：一个文件若出现三个
以上独立变化原因，或纯逻辑必须依赖 UI/全局单例才能测试，就应进入拆分队列。
