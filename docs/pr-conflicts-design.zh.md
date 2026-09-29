# PR 冲突查看设计方案

状态：用户已授权按最终方案实施。设计日期：2026-09-28。当前实现契约与验收边界
见 [PR 冲突查看规格](specs/pr-conflicts.md)；本文保留设计决策与研究依据。

本方案将 Conflicts 设计为 PR Review 内部可独立维护的子模块。第一批同时覆盖 PR 列表的逐行冲突状态，以及冲突文件列表和只读详情。在线编辑、标记已解决和提交列入[后续迭代](pr-conflicts-iterations.md)。实施契约已同步到中英文架构文档与 PR 工作流规格，线上平台验收限制按实际记录保留。

视觉基线为[当前程序 UI 设计图 v5](designs/pr-conflicts/README.md)：保留程序框架与入口，修正括号方向、线宽、行号背景和起始行括号槽漏绘，用户已授权按最终结果实施。它是 Usher 只读适配稿，不是 GitHub 原生截图。全套 UI 必须满足本方案的 i18n 多语言契约，图中示例语言不限制程序语言支持。

## 1 范围与建议决策

第一批交付五项：

1. 每条 Open PR 显示明确的冲突状态，区分有冲突、无冲突、检查中、未知。Closed/Merged 的当前合并冲突标为不适用。
2. 列表状态和详情页的 Conflicts 都可进入同一个冲突查看视图。
3. 文件列表、单文件只读代码、冲突块导航、加载和失败状态完整可用。
4. GitHub、GitLab、Gitee 公有 v5、Gitee 企业 v8 共用展示协议，各自保留独立语义和取数方式。企业部署按其 API 能力降级。
5. PR 列表冲突状态和 Conflicts 子视图完整接入共享 i18n，支持全部内置语言、运行时即时切换，以及后续通过 catalog 扩展语言；多语言是第一批完成条件。

Conflicts 使用独立页内子视图。入口与 Conversation、Commits、Files changed 复用同一 Tab 组件并保持单行，窄窗压缩内部间距；进入后内容区与局部工具栏切换为独立冲突工作区，标题、PR 编号、分支上下文继续存在。已进入时重复点击顶部入口保留当前内容，刷新使用专用按钮。文件列表支持拖动分隔线调宽。返回恢复来源 Tab 或列表。第一批不呈现 Accept、Mark as resolved、Commit merge 等写入按钮，也不加入冲突搜索语法或筛选器。

## 2 已核实事实与研究限制

### GitHub 的参考边界

[GitHub 官方冲突流程](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/resolving-a-merge-conflict-on-github)在 PR 合并状态区列出冲突文件，通过 Resolve conflicts 进入左侧文件列表、右侧代码编辑区；支持逐文件和逐冲突定位。[2025-10-02 官方更新](https://github.blog/changelog/2025-10-02-one-click-merge-conflict-resolution-now-in-the-web-interface/)展示了逐块接受双方修改的操作，属于后续写入范围。

2026-09-28 再次核对该更新中的[官方编辑器原图](https://github.com/user-attachments/assets/4a235221-12c2-44f3-802e-90c46f9126bf)，可以确认：大标题下用一行描述分支关系；左侧平铺冲突文件；右侧文件栏集中呈现路径、冲突数量和 Prev/Next；代码保持白底，冲突在行号栏用浅黄色和括号标记，逐块操作位于代码内。官方文档的 Mark as resolved 截图也显示类似文件工具栏。这是所引用版本的截图证据，不代表所有部署或当前登录态均相同。

此前适配稿中的多层分支说明、文件夹树、重复 Read only 提示和整行绿/蓝来源底色是自行设计，不能称为 GitHub 原样复刻。重绘保留 Usher 现有 Shell、组件字体和入口，冲突内容区采用以下规则：分支关系只保留一处；文件列表默认平铺；单一文件工具栏；白底代码与局部冲突 gutter 标记；首批移除所有写入操作。只读状态仅提示一次，短 SHA/诊断来源按需放入辅助信息，不占固定额外横条。

2026-09-28 实际查看了未登录的 [rust-lang/rust PR 列表](https://github.com/rust-lang/rust/pulls)：可以确认 GitHub 的标题行、元信息行、PR 生命周期图标、筛选与列表结构；这次查看没有核实到适用于每条 PR 的通用冲突徽标。不能据此声称 GitHub 在所有版本、登录态中都没有这种徽标，也不能把 CI 失败图标解释为代码冲突。

因此本方案的“列表逐条显示冲突”是用户要求的 Usher 功能，采用现有 GitHub/Primer 风格组件和紧凑状态布局；不是宣称逐像素复刻某个已核实的 GitHub 冲突列表。正式实施前再以当前登录态页面核对可参考的状态细节，不将未经核实的样式当作平台事实。

### Gitee 企业版附件

来源：用户提供的 `gitee企业版代码托管平台API接口清单.docx`。按原文章节“PR通过条件状态展示接口”“查看项目的合并请求记录”“更新合并请求”“冲突文件信息查询”核对。文档内容作为接口证据，不作为执行其示例写操作的授权。

| 文档接口/字段 | 已核实语义 | 设计使用方式 |
| --- | --- | --- |
| `GET /projects/:id/merge_requests/:iid/current_state` | `conflict_result=true` 无冲突，`false` 有冲突；`can_merge` 是整体合并资格 | 独立提取冲突状态，不根据 `can_merge=false` 推断冲突 |
| `GET /projects/:id/merge_requests` | 提供 `merge_status`、`sha`、`diff_refs` 等；该章节没有完整约定全部 `merge_status` 枚举 | 明确可识别的冲突值才直接使用；缺失/不确定时查询 current_state |
| MR 详情/更新响应示例 | 包含 `show_conflict`、`has_conflict`、`can_operation_conflict`、`can_resolved_conflict_files`、`resolved_conflict_files_count` | 作为部署验证项；不假设所有列表响应都带这些字段，不把写权限作为查看权限 |
| `GET /projects/:id/merge_requests/:iid/conflicts` | 返回分支、提交引用及 `files`；原文要求不能在线解决时仍返回冲突文件 | 企业 v8 原生查看数据源 |
| `files.their_path` / `files.our_path` | 文档分别标为源分支路径/目标分支路径 | 保留双方路径，正确表达重命名，不仅取一个显示路径 |
| `files.content`、`type`、`encoding`、`our_mode`、`updated` | 冲突内容、文件类型、编码、目标侧模式、解决相关标志 | 第一批解析只读内容；`updated` 不映射为本地“已解决”状态 |
| `commit_sha` / `target_commit_sha` | 文字说明为 SHA，但示例实际是 `refs/pull/2/head` / `refs/heads/master` | 视为 commitish，先解析固定提交，再建立快照；不能把 ref 字符串伪装为 SHA |

该接口一次返回 files 及其 content。UI 可以按选中文件延迟解析和投影，不能声称附件提供了逐文件懒加载网络接口。字段缺失、空 files、非文本内容和只读权限用户的响应需要实际部署验收；本轮没有连接内部服务、使用 Token 或验证线上返回。

## 3 现有代码与扩展位置

已检查的代码：

- `src/git/model.rs` 的 `PullRequestItem` 只有评审状态，没有列表冲突字段。
- `src/git/provider.rs` 的 `GitProvider` / `ProviderCapabilities` 已隔离平台请求，但还没有冲突列表/详情契约。
- `src/git/github.rs` 已为当前列表页并发加载 PR summary，且限制为 4；summary 中已有 PR detail 可复用，不应为冲突重复发同一请求。
- `src/git/gitee/readiness.rs` 已正确把 `conflict_result=false` 映射为冲突；公有 v5 的非 mergeable 仅映射为 Blocked，没有擅自判为冲突。
- `src/git/gitlab.rs` 已读取 `has_conflicts` / `detailed_merge_status`，但列表评审投影和整体 readiness 不等于独立冲突状态。
- `src/workflow/pr_list.rs` 的页大小是 10；列表投影由 `to_ui_item` 附近逻辑生成，已有 page/filters 返回上下文。
- `src/workflow/pr_review.rs` 为稳定 façade，`controller.rs` 管导航，`controller/actions.rs` 管能力和远端命令，`state.rs` 管独立请求代次。
- Slint 列表契约在 `ui/components/workflow/github_pr_list_components.slint`，行布局在 `github_pr_flow.slint`；PR 公共协议在 `ui/features/pr_review/contract.slint`，完整页面在 `ui/views/workflow/pr_review_workspace.slint`，就绪入口在 `ui/components/pr_review/shell.slint`。

以下为拟新增组织，路径尚不存在，不要求新增 crate：

```text
src/git/conflicts/
  mod.rs                  Provider-neutral 服务入口与只读结果
  model.rs                冲突状态、文件、块、快照、能力、错误类型
  merge_tree.rs           隔离 Git 分析、结构化输出解析、进程取消
src/git/gitee/conflicts.rs 企业 v8 原生请求与解码
src/workflow/pr_conflict_status.rs
                          列表/页头/冲突视图共享的轻量状态 façade
src/workflow/pr_review/conflicts/
  mod.rs                  独立 controller façade
  controller.rs           加载、选择、导航、失效编排
  state.rs                强类型 State Store 和 Actions
  projection.rs           行号、冲突块、高亮和可见行投影
  bridge.rs               Slint event-loop 绑定
ui/components/pr_review/conflicts/
                          文件列表、只读代码、状态和导航控件
ui/views/workflow/pr_review_conflicts.slint
                          冲突页面布局，复用现有 PR Shell
ui/features/pr_review/conflicts_bridge.slint
                          无状态的公开 callback / model 协议
```

`src/git/` 负责平台 I/O 和 Git 对象分析；PR Review 子模块不接收 Provider JSON/Token。共享状态 façade 位于 Workflow feature 边界，列表不 import Review 内部 controller；Review 也不直接修改列表 ModelRc。

允许复用文件路径显示、高亮算法、只读选区、滚动与虚拟化基础设施。不能复用普通 Diff 的业务状态、所选文件索引、Find 结果或高亮请求代次；冲突文件用稳定 file id/路径组合标识。确需抽取复用逻辑时提供不可变数据接口，不把整个 review controller 作为依赖传进去。

## 4 页面与导航

### PR 列表

保留行左侧 Open/Draft/Merged/Closed 图标；冲突不改变 PR 生命周期图标。第二行元信息继续显示编号、作者、时间和分支，紧接分支放一个紧凑的“图标＋冲突状态文字”槽位，与评审状态分开。

| 状态 | 源语言 / 中文建议 | 操作 |
| --- | --- | --- |
| Conflicting | Conflicts / 存在冲突 | 打开该 PR 的 Conflicts 子视图 |
| Clear | No conflicts / 无冲突 | 状态展示；不暗示满足 Merge 条件 |
| Checking | Checking conflicts / 检查冲突中 | 状态展示；可进入详情等待结果 |
| Unknown | Conflict status unknown / 冲突状态未知 | 可进入详情查看原因并显式重试 |
| NotApplicable | 不显示冲突徽标，辅助信息标不适用 | Closed/Merged 不发冲突探测 |

冲突使用红色语义；Clear 使用绿色或 muted success 图标，但文字始终为“No conflicts”，不能写“可合并”。Checking 为中性色，Unknown 为明确的未知图标和文字，不能用绿色默认值。来源为隔离 Git 时使用“本地分析”限定说明，不能冒充服务器结论。原型中的状态和代码为合成样例，不代表附件中 PR 的实际查询结果。

宽屏尽量保持现有两行密度；宽度不够时元信息自然换行。冲突状态完整可辨，不以 hover 为唯一说明，不挤掉评论/AI 操作。标题保持已有正常色/hover 链接色，状态点击是独立焦点目标，点击不触发父行的普通打开动作。Closed 列表里再次打开的 PR 按其真实 Open 状态判断。

当前列表显示后再做状态 enrichment，仅更新发生变化的行，不等全部探测完成才显示列表，不在滚动期间重建模型。第一批不新增 `is:conflict` 等查询词，不改变远端分页、筛选和总数。

### PR 详情入口

当页头现有 readiness 恰为 Conflicts 时，点击它直接打开冲突子视图。其他 readiness 仍保持原有行为；若 Draft 等优先状态掩盖了代码冲突，在现有就绪菜单内提供“View conflicts”，使用独立冲突快照判断。避免重复显示两个相同状态徽标。

列表的 Conflicts 点击携带 `OpenIntent::Conflicts`；普通标题打开保持现有默认 Tab。冲突入口只需加载轻量 PR 上下文、状态和冲突数据，不应先加载完整 Files changed、高亮和 Conversation 后才能进入。

### 冲突工作区

上方保留当前程序标题、编号和 Open/Draft 等 PR 上下文，源/目标 Branch 关系只在页头说明一次。内容区局部导航提供“返回”及低强调的刷新/只读提示；不再另加 Source/Target 工具条或整宽蓝色分支横条。冲突文件 N 只在左侧列表标题出现；短 SHA 和查看来源作为按需辅助信息。N 仅在完整文件列表返回后显示；未知或部分结果不能伪装成准确总数。

主体左侧为平铺冲突文件列表，复用现有文件图标与选择行为；完整路径可换行，不用多层文件夹节点增加稀疏列表高度。双方路径不同时显示重命名关系；文本冲突可显示块数量，二进制/结构冲突只显示类型。右侧只有一条文件工具栏，集中呈现选中文件路径、冲突块序号及上一处/下一处，下方是带行号的只读代码，marker 的上下文标明源分支和目标分支。不使用含糊的“ours/theirs”作为用户标签，也不重复逐行贴分支标签。

采用官方截图参考的单列冲突标记视图，保留上下文行：代码主体以白底/对应暗色 canvas 为主，冲突范围在 gutter 用主题化浅色和括号形边界标记，不用普通 Diff 的整行新增/删除底色区分双方，也不加固定颜色图例。来源由 marker 与语义标签说明。所有源码均只读，可以选中复制；没有编辑框、逐块 Accept、已解决勾选或提交区。行号是“冲突展示行号”，不得伪装为两侧原始行号；只有数据确实提供时才另显示源/目标侧原始行号。

gutter 细节按用户补充的 GitHub 原图核对：括号使用 `[` 方向，竖线在左，上、中、下短横线向右伸向代码，中间短横线对齐分隔 marker；设计线宽为 1 个逻辑像素，实施时按设备像素对齐。窄括号槽的浅黄色背景必须从起始 `<<<<<<<` 行连续覆盖至结束 `>>>>>>>` 行，包含起始、分隔和结束行，不能从下一行才开始。数字格与括号槽分开描述：浅色主题数字保持灰色；所提供截图的起始行数字格是白底，其后直到结束行的数字格为浅黄。这是该截图的可见效果，不作为 GitHub 所有版本的通用规则。背景与线条使用主题语义 token，暗色主题需独立验证可辨识度。v5 示例的括号槽覆盖 69–73、79–83 行，数字格 70–73、80–83 行浅黄、69 和 79 行白底；不得将起始数字格白底误解为起始行没有黄色高亮，也不得把背景规则误作数字字体颜色规则。

在共享 small 断点以下（现有设计系统为准），文件列表进入可展开的选择区域，右侧代码占据剩余宽度；不额外发明固定窗口尺寸阈值。长源码默认不换行、支持横向滚动，沿用 PR 源码字体和布局契约。若后续支持软换行，必须同步实测 extent、原生 line-height、选区和 Worker 测量。

### 返回契约

从详情 Tab 进入：Back 先恢复原 Tab、所选 diff 文件和 viewport；再一次 Back 执行原有退出 PR 行为。从列表直接进入：Back 回到原项目、筛选、页码和列表滚动位置，不绕经 Files changed。Conflicts 内切文件不是新的 Router 历史条目。离开项目、显式关闭审核和刷新使旧请求失效；Sidebar Home 隐藏仍遵守现有“历史可达则保留当前会话”契约。

## 5 独立状态模型

建议引入 `PullRequestConflictState = Checking | Conflicting | Clear | Unknown | NotApplicable`，不从 `PullRequestReadinessKind` 反向派生：Draft、Required reviews、CI、权限都可能遮蔽冲突。

`ConflictStatusSnapshot` 保存 RepoKey、PR IID、head/target SHA（可空）、状态、来源、checked_at、freshness、语义错误。RepoKey 必须含 API host、Provider/profile、远端项目身份和认证配置代次，禁止仅用 `#166` 或 repo_project_id 跨 host 共用缓存。

`ConflictSession` 保存固定 base/head/target revision、文件元数据、可用性、选中文件 id、冲突块、presentation 状态和来源。文件包括双方路径、类型、编码、可选只读内容句柄与块索引。错误/消息用结构化类型与语义 catalog key，不存翻译后的 prose；Provider 原始 message 只作有界诊断，不直接进 UI。

能力契约分开表达 `status_read`、`native_details_read`、`isolated_git_details`；未来再加入 `online_edit`、`mark_resolved`、`commit_resolution`。部署支持、当前用户权限和当前快照可用性是三个维度，不能用一个 `can_resolve` 布尔决定整个页面是否可见。第一批只暴露读取命令，不预先绑定未实现的写入 callback。

共享轻量状态通过 feature 自有 façade 和 `usher-state-store` 更新，强类型 Actions 如 ObserveStatus、MarkStale、InvalidateRepository、RemoveProject；不同读取方订阅同一提交。冲突子模块再拥有独立的 session façade/Actions：Open、LoadStarted、ListLoaded、SelectFile、FileLoaded、SelectBlock、Fail、Refresh、Close。Reducer 和大数据处理运行在 Worker；UI bridge 只接收不可变快照/行增量，不依赖具体 Store。

List、File、Highlight/Layout 使用各自请求代次。回写前校验 RepoKey、PR、session generation、固定 SHA、file id 和 layout key；刷新或切 PR 后的旧成功/旧失败都不得更改新 loading 状态。状态 enrichment 不使 review/merge mutation 失效。重复点击同一在途请求合并，不重复启动。

状态是临时观测，本批不持久化到项目配置。共享缓存限当前项目与有界的已访问条目；完整冲突内容只归当前审核会话，不能按所有访问过的 PR 无限积累。原生接口同时返回多文件内容时保留一份有界原文/临时内容存储，按选择生成高亮，避免同一字符串复制多份。

## 6 平台兼容与取数策略

| 平台/profile | 状态证据 | 文件/详情来源 | 降级与限制 |
| --- | --- | --- | --- |
| GitHub.com / GitHub Enterprise | PR detail `mergeable=false/true/null`；可复用现有 summary | 文档化 REST 没有专门 conflicts 内容 API；按需隔离 Git 分析 | null=Checking；认证/refs 不可得时明确不可用，不调用 cookie 页面接口 |
| GitLab.com / Self-managed | `detailed_merge_status=conflict` 或可信 `has_conflicts=true`；完成判定后才能解释 false | 第一批用同一隔离 Git 分析 | checking/unchecked/preparing 不能误报 Clear；新版本实验 GraphQL conflictFiles 不作第一批必需依赖 |
| Gitee Enterprise v8 | current_state 的反向布尔；已验证 `has_conflict` 可辅助 | 附件 `/conflicts` 原生接口，失败后按条件提供隔离分析 | `can_operation_conflict=false` 不阻断查看；端点 404 不等于空冲突列表 |
| Gitee Public v5 | 已文档化/验证的明确冲突字段优先；`mergeable=false` 本身仅说明 Blocked | 按需隔离 Git 分析 | 无精确信号时 Unknown；不能复用 v8 字段/接口/token |

[GitHub REST](https://docs.github.com/en/rest/pulls/pulls#get-a-pull-request)明确 `mergeable=null` 是异步计算中。[GitLab REST](https://docs.gitlab.com/api/merge_requests/#merge-status)说明列表状态可能未及时重算、`has_conflicts=false` 依赖 merge_status；`with_merge_status_recheck=true` 请求重算但不保证完成。各 adapter 应使用已知状态白名单和独立测试，不把任何“不能合并”都映射为冲突。

GitHub 的 `mergeable_state` 是现有实现使用的补充信息，公共 REST 文档中未完整保证全部枚举；第一批以文档化 `mergeable` 为主。相互矛盾的服务器字段、native 内容和固定 SHA 需要重读/标记 stale，不能随意选一个绿色值。

Gitee list 先用已有响应能明确识别的字段；缺少可靠证据的当前页 Open PR 最多增加一次 current_state 请求。详情可以复用已经加载且未过期的 current_state/detail 结果。禁止为列表加载 conflicts 内容或为每条 PR 下载 Git 对象。native 接口不支持或内容受限时才考虑隔离分析；401/403 先报告认证/权限原因，不把 fallback 用作绕过读取权限。

第一批的“多平台兼容”验收要求上述四种 profile 的普通文本冲突均能查看；二进制/结构冲突能列出且说明详情限制。如果某 profile 因私有 fork 权限、refs 获取或部署缺失接口无法查看，需要报告具体失败并显示不可用，不能用打开网页按钮代替完成验收。

## 7 隔离 Git 分析

这是第一批兼容 GitHub/GitLab/Gitee 公有版的必要补充，仅在用户打开 Conflicts 时触发。它属于读取与推导，不是在线解决。

1. 从平台读取源/目标仓库身份、PR refs 与固定 head/target SHA。源分支来自 fork 时按源仓库/PR head ref 取对象，不能把目标仓库同名分支当作源分支。
2. 在应用控制的临时 bare object store 中取得所需对象；可以只读复用已有对象，但所有新增对象/refs 留在隔离目录，不写用户仓库的工作树、index、HEAD、分支或 remote-tracking refs。
3. 使用支持现代模式的 Git，执行 `merge-tree --write-tree -z --messages <head-sha> <target-sha>`。这个方向表示把目标并入来源，匹配冲突解决的源分支上下文；UI 永远明确标签。Git 自动处理共同祖先/多个 merge base，不以 PR diff 的 base_sha 或手工选择一个 merge base 替代。
4. 按 NUL 分隔的结构化输出解析冲突路径、stage/blob 和冲突关联；退出 1 表示冲突，其他失败码为分析失败。不能通过扫描整个结果树或仅搜索 `<<<<<<<` 判定所有冲突，也不能把“冲突路径为空”判为 Clear。目录级冲突允许没有可显示文件。
5. 从结果 tree/blob 读取文本标记内容；按类型对 modify/delete、rename、mode、binary、submodule、file/directory 给出元数据和两侧内容可用性。不是每类冲突都有两个文本块。
6. 后台超时、取消、最大响应/对象/内容大小受限。Git 能力启动前探测；旧 Git 不运行旧版 trivial merge 冒充同等结果。浅历史按有界策略加深；共同祖先仍不可得则明确失败。

[Git 官方文档](https://git-scm.com/docs/git-merge-tree)确认现代模式不更改工作树/index、不生成 commit，但会写 tree/blob 对象，因此仍必须隔离对象写入。实现应验证参数能力，而不是只比较版本字符串。结构化 conflict type 可映射语义错误；自由文本诊断不作稳定机器协议。

对象传输由统一 Git 凭据边界控制：API Token 与 Git HTTPS/SSH 身份可能不同，不能假设 API 能读就一定能 fetch。不得把 Token 放在命令行/URL/日志/临时持久配置中；禁用后台交互认证，返回可理解的认证错误。只允许经仓库上下文验证的 host/protocol，fork 目标需要校验 host 和凭据作用域；不因 Provider 返回任意 URL 而向其发送 Token。所有应用 HTTP 仍走 `src/http/mod.rs`。不执行仓库自定义 hook、filter 或未验证 merge driver；采用可控的标准分析环境，遇到平台策略差异明确标注结果来源和限制。

服务器是状态主来源。隔离分析只提供该 SHA 对的文件/内容；在没有服务器精确状态的平台可给出带“本地分析”标签的推导结论。服务器 Conflicting 而本地结果 Clear 时保留服务器状态并展示不一致说明；绝不由本地计算启用 Merge。旧快照或服务器策略差异不能显示“已解决”。

## 8 请求预算与刷新

当前页 10 条。列表 enrichment 最多 4 个轻量请求并发，单次入页最多每条一个状态读；复用已有 detail 请求。GitHub/GitLab 返回计算中时最多再做两轮有界延迟重读，仍计算中就保留 Checking 并提供显式刷新。延迟和重试总预算统一计数，HTTP transient retry 不能叠加成无界请求。翻页/筛选/离开项目取消未启动项，已完成旧页结果只有身份仍匹配时才进入有界缓存，不写新页。

建议轻量状态缓存 30 秒，保存 checked_at；超期回页显示 Checking 并重新获取，不继续以旧 Clear 作为当前结论。没有 head/target SHA 的观测也只能限时复用；手动刷新、目标/源分支已知更新、token/profile 改变立即失效。列表和详情观察到的新状态通过共享 façade 同步，不由子视图直接操作父页面。

详情原生列表只按需读一次，单文件投影/高亮串行最新优先；隔离 Git 分析同时只允许一个重任务，重复点击合并。刷新前先取消旧分析，再启动新快照。路由隐藏不自动取消远端 mutation，不改变现有 PR 生命周期规则。

不做常驻轮询；仅当前交互的异步判定允许上述有限补查。后台探测和 fallback 用 silent HTTP feedback，界面以行状态或局部错误呈现，避免每条 PR 失败都弹 Toast。操作名分别为 Load PR conflict status / Load PR conflicts / Prepare conflict file 等明确用途。

## 9 边界状态与安全展示

| 情形 | 期望展示 |
| --- | --- |
| native 内容全量且可解析 | 文件列表、正确块数、首个文件，逐块定位 |
| 确认无冲突 | No conflicts 空状态；不自动离开页面或启用 Merge |
| native 空 files 但状态 Conflicting | 冲突详情暂不可用/结构冲突说明，不能报无冲突 |
| 二进制、LFS、编码失败、超大文件 | 文件仍在列表；类型、双方路径/版本、无法显示文本的原因；不显示伪造代码 |
| 文件总数或内容被截断 | 明确“不完整”提示；块数/总数标未知或部分，不能标全部 |
| 源分支删除、fork 不可读、历史不足 | 保留服务器状态，说明读取失败并提供刷新/查看平台页 |
| 刷新中 branch 推送 | 旧结果不作为新 SHA 结果；重读失败则标 stale |
| 401/403/404/429/离线/超时 | 分类语义信息；429 遵守 Retry-After，403 区分权限/限流，404 不断言端点一定不支持 |
| native 未承诺内容顺序对应分支 | 保留原始 marker label；通过 refs/blob 证据映射，无法确认不猜源/目标 |
| malformed marker、diff3/zdiff3、无末尾换行 | 完整保留原文；解析器识别可确认的块，失败不丢代码，不把文件内原有 marker 字符串直接判为合并冲突 |

源码、路径与远端消息均是非指令内容。HTML-like 源码作为纯文本绘制，不执行 markup。不能把 Gitee 响应 content 当成 Provider 的页面 HTML 执行。文件路径含空格、中文、Tab、换行等时用稳定 id 与参数数组/NUL 协议，不拼接 shell。

### i18n 多语言契约

第一批遵循 `docs/specs/i18n.md`，复用现有 `usher-i18n` 和 `src/i18n.rs`，不另建 Conflicts 翻译器或语言选择器。语言来自全局设置；当前 manifest 注册 `en` 和 `zh-CN`，两者覆盖率门槛均为 100%。新文案必须同步到 `i18n/messages.toml` 和每个内置 locale 的 `usher.po`；未来新增语言只扩展 manifest/PO，不添加语言枚举或 locale 分支。

翻译范围包括列表徽标、页面入口/标题、文件与冲突计数、源/目标 Branch 标签、只读提示、刷新与返回、上一处/下一处、来源与过期状态、文件类型、加载/空/错误/降级提示、Tooltip、键盘提示和无障碍名称。未来在线解决、标记和提交的每一批同样遵守这套契约。

建议语义键如下，实施时优先复用已有完全等义的 key；这些是待加入 catalog 的设计，不代表本轮已经添加翻译：

| 拟用语义键 | English 源文案 | 简体中文文案 |
| --- | --- | --- |
| `pr_list.conflict_conflicting` | Conflicts | 存在冲突 |
| `pr_list.conflict_clear` | No conflicts | 无冲突 |
| `pr_list.conflict_checking` | Checking conflicts | 检查冲突中 |
| `pr_list.conflict_unknown` | Conflict status unknown | 冲突状态未知 |
| `pr_conflicts.files_count` | Conflicting files: {count} | 冲突文件：{count} |
| `pr_conflicts.block_position` | Conflict {current} of {total} | 第 {current} / {total} 处冲突 |
| `pr_conflicts.source_branch` | Source Branch: {branch} | 来源 Branch：{branch} |
| `pr_conflicts.target_branch` | Target Branch: {branch} | 目标 Branch：{branch} |
| `pr_conflicts.back_to_list` | Back to PR list | 返回 PR 列表 |
| `pr_conflicts.back_to_tab` | Back to {tab} | 返回 {tab} |
| `pr_conflicts.read_only` | Read-only | 只读 |
| `pr_conflicts.error.load_failed` | Could not load conflict details. Try refreshing. | 无法加载冲突详情，请刷新重试。 |

所有计数、Branch、路径和 Tab 使用命名占位符；翻译可以重排，但必须保留相同的参数名集合。返回 Tab 的名称由已有 catalog 解析，不保存英文或中文标签，也不拼接固定顺序的句子。产品/工程术语按仓库规范保留 English；路径、Branch 名、SHA、协议值、远端原始 marker 和代码保持原文。Provider 原始错误不作机器翻译，也不作为 UI prose，映射为结构化错误后再解析语义文案；日志保持 English。

Slint 使用共享生成的 `I18n` properties，不手写 `@tr()`、硬编码文案或工厂局部翻译 bundle。Rust 长期状态保存错误种类、语义 key/`LocalizedText` 和原始参数，翻译仅发生在 presentation boundary；纯 Rust 文案按规范设置 `slint = false`，不改变 UI bundle 索引。

语言切换无需重启、刷新页面或重新打开 PR：已挂载的列表行、页头、文件元信息、状态提示、打开的菜单/Tooltip 和无障碍描述立即更新。冲突子模块明确接入现有 locale 刷新链路；异步结果回写使用当时的活动 locale，旧语言生成的投影由 locale/layout 代次阻止覆盖新语言。切换仅重投影受影响文案及必要布局，不重新请求 Provider、不重跑 Git 分析、不重新解析源码；保留当前文件、冲突块、选区、返回上下文和滚动锚点。

长翻译按实际字体测量，允许按钮/状态区重排，不通过缩小字体或 locale 条件布局掩盖裁剪。文案换行引起的文件行高、滚动 extent 和定位使用共享精确测量契约。Tooltip 和键盘提示遵循现有工厂隔离/根 Portal 规则，固定动作的命中节点保持挂载。

精确虚拟化复用 `usher-virtual-list`、LatestCompute 与现有 PR 字体/源码测量契约；源码解析、高亮、reducer、Git、文件扫描都离开 event loop。滚动仅变更有界可见窗口；不能按滚动重复生成全文件 model。新增持久任务/子进程注册取消与 shutdown。大内容以显式大小上限、部分展示和取消控制，不承诺未测量的运行时性能收益。

## 10 验收矩阵与实施门槛

实现前先完成原生接口和隔离分析的正反例 spike，决定字段优先级和具体大小/超时阈值；失败 profile 不作为已完成推进。测试至少覆盖：

| 分组 | 必须覆盖 |
| --- | --- |
| 状态语义 | GitHub true/false/null；Draft＋conflict；CI failed＋Clear；Gitee conflict_result 反向布尔；v5 false 不误判；GitLab checking＋false；未知枚举/缺字段/矛盾字段 |
| 文件内容 | 单/多文件，多冲突块，中文路径/代码，重命名、删除、二进制、目录冲突、submodule、模式冲突、超大/截断内容、无最终换行、diff3/异常 marker |
| 平台 | 四种 profile 的文本冲突/无冲突/无写权限，企业 host 配置，GitHub/GitLab fork，缺 refs/旧 Git/浅历史/权限不足 |
| 生命周期 | 列表直接进入，三 Tab 下钻返回，连续切文件/PR/项目，刷新时推送，关闭时 Worker 完成，状态失败不破坏 review mutation，token/locale 切换 |
| 界面 | EN＋zh-CN，亮/暗，窄/短窗口，高 DPI，键盘 Enter/Esc/Tab，代码复制，Tooltip 串文案与边缘裁剪，真实滚动/resize 稳定 extent |
| i18n | 全部内置 locale 新 key 覆盖率达标；命名参数一致；在列表、冲突页、加载/失败状态、打开菜单/Tooltip 时往返切换 EN/zh-CN；已有 Rust 行立即更新；在途结果不恢复旧语言；切换不触发额外 HTTP/Git；选中文件/块/选区/滚动锚点保持；长文案、计数 0/1/多项和中文路径不裁剪或误翻译 |
| 数据共享 | 列表/页头/子视图一致；只更新目标行；项目/host/PR 同号不串用；本地结果不提升 Merge 能力；Closed/Merged 不发探测 |
| 工作区隔离 | 以干净、dirty、未追踪文件、已有本地 merge 状态、只读仓库测试；前后 HEAD/index/worktree/refs/config 保持一致；临时分析取消可清理 |
| 请求与资源 | 当前页请求预算、4 并发、1 重任务、取消、429、预算耗尽；反复跨 PR 增长有界；无 credential 泄漏；关闭进程无残留 |

实施交付必须运行 `scripts/check-i18n.sh`、`cargo test -p usher-i18n --locked`，验证共享 bundle 与已挂载动态投影；再运行 `cargo fmt --all`、`cargo check --locked`、`cargo test --lib --locked`、`git diff --check`，完整 root Slint 编译与真实路由人工检查；新增 UI 同步演练场登记。每个内置 locale 都人工查看受影响路由，EN 与至少一种非源语言必须覆盖即时切换与在途请求。已知完整 doctest 基线单独报告。若实施包含性能优化批次，另按仓库规范保留 Release A/B 原始数据和采用/拒绝结论，不以功能自测代替性能证据。

## 11 本轮交付与审核点

本轮仅新增本设计及迭代计划，应用 Rust/Slint、配置、依赖和现行规格不改动。已有 Cargo、memory、PR Review 等工作区修改保留。

建议批准的关键选择是：独立页内子视图；列表常显独立冲突状态；企业 v8 原生读取；其他 profile 按需隔离 Git 分析；只读第一批；在线写入另批审核。接口文档支持不等于线上已验收，尤其 SHA/ref、内容双方顺序、只读权限和超大文件响应仍需实施阶段验证。
