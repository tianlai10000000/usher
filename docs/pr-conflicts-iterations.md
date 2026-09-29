# PR Conflicts 迭代计划

状态：第一批只读代码与离线验收完成，线上平台验收待执行。日期：2026-09-28。
当前实现契约见 [PR 冲突查看规格](specs/pr-conflicts.md)，实际通过项与未执行项见
[验收记录](pr-conflicts-validation.zh.md)；后续写入批次尚未实施。

完整边界见 [PR 冲突查看设计](pr-conflicts-design.zh.md)。第一批仅查看文件与详情，后续三个写入阶段单独验收，不能因文档登记而默认获得实施或远端写入授权。所有批次必须支持共享 i18n 的全部内置语言，多语言不作为延后补齐的功能。

## 第一批 只读查看与列表状态

### A 接口与算法验证

- 用脱敏 fixture 验证 Gitee v8 current_state / conflicts，反向布尔、files 全量内容、refs 解析、不能在线解决仍可查看。
- 验证 GitHub REST、GitLab REST、Gitee v5 的精确信号/未知语义；复用现有摘要请求。
- 在临时 bare repo 中验证现代 merge-tree 的普通冲突、结构/二进制冲突、多个共同祖先、fork、浅历史、取消和工作区隔离。
- 输出每个 profile 的通过/失败/未执行证据；字段/能力不可靠时修改方案而不是沿用错误基线。

### B 领域和解耦

- 新增 Provider-neutral conflict status / snapshot / file / block / read error 与 capability 契约。
- 实现 `src/git/` 只读服务、Workflow 共享轻量状态 façade，以及 PR Review conflicts 自有 State Store/controller/bridge。
- list enrichment 与 session/list/file/layout 代次分开，保持 mutation 独立。

### C 页面和接入

- 列表 10 条当前页的增量状态、非阻塞读取、4 并发预算与未知/检查中/不适用展示。
- Conflicts 子视图、两种进入意图、文件选择、只读源码、块导航和正确返回。
- 原生一次全量内容与按需高亮；统一精确虚拟化、字体/行高、选择/复制契约。
- i18n 全 locale：所有文案进入共享 messages/PO，Slint 使用生成 I18n，Rust 状态保存语义标识/参数；包含状态、错误、计数、Tooltip 和无障碍名称。
- EN/zh-CN 即时往返切换：已挂载行和在途回写不残留旧语言；保留文件/块/选区/滚动锚点，切换不额外请求 Provider 或重跑 Git。验证长翻译、命名参数及布局，登记演练场。

### D 完成门槛

四种 profile 的文本冲突可查看；复杂冲突可以列出并诚实说明限制；list/header/view 状态一致；不会改变本地工作区或远端分支。新 key 在全部内置 locale 达到 manifest 覆盖门槛，`scripts/check-i18n.sh` 与 `cargo test -p usher-i18n --locked` 通过，并完成运行时语言切换和共享 bundle 验证。执行设计文档中的完整验收矩阵，交付记录实际请求数、失败与未执行项，不宣称未测的性能收益。同步两版架构、PR 工作流 spec/index；此时才把 Proposed 改为 Implemented。

## 第二批 在线编辑与逐块选择

加入只读之外的编辑会话、Accept source / Accept target / Accept both，以及局部编辑/撤销/重新载入。草稿只属于当前 PR 与固定 head/target SHA；与普通 Diff 选区和 AI Review 解耦。dirty draft 离开时明确保存/丢弃语义，不能把编辑后的本地内容标为远端已解决。

必须重新验证各平台实际可用的写入方式。GitHub 网页有这些动作不表示 REST 公开了同名 endpoint；Gitee 附件本轮已核实的是 conflicts 读取，不能凭字段名称推导 mark/submit API。不能使用浏览器 Cookie、抓包私有路由作为默认实现。Git 写入 fallback 另行设计和审核。

## 第三批 标记已解决

加入逐文件 Mark as resolved 与取消标记、编辑验证、未解决文件计数；明确区分本地草稿标记与服务器状态。验证 marker 被清除、语法/编码/二进制处理条件以及恢复草稿行为。PR 作者、协作者、推送权限由各 Provider/认证主体能力控制；不能把 Gitee UI 的“非创建者无法提交”规则直接套到 GitHub/GitLab。

标记不得自动提交、自动 Merge 或清理其他用户的修改。固定 head/target 更新后所有旧标记失效，要求重新校验，不允许对新版本继续宣称已解决。

## 第四批 提交解决结果

加入提交信息、提交目标和明确最终操作；从服务器刷新提交前权限及 head/target SHA。若平台没有文档化写接口，可设计隔离 Git 提交和受控 push，但需要明确 token/credential、fork、受保护源分支、并发更新、取消/重试幂等性和冲突失败处理。

Conflict resolution commit 更新来源分支，与 Merge PR 是两个独立命令。提交成功后刷新共享冲突状态、Conversation、Commits、Files changed 与列表；服务器重新判定之前保持 Checking。提交失败不得显示成功，不自动重试非幂等写操作，不绕过保护规则。

## 后续候选

- Split / 三方视图、全文 Find、更丰富复杂冲突浏览。
- 服务端能力成熟后采用原生 GitLab GraphQL conflictFiles；按版本/权限探测，保留只读降级。
- 仅在有明确平台支持或可正确跨页实现时添加冲突筛选/搜索，不能只筛当前页伪装远端全量结果。
- 如出现多 PR 并行编辑、独立路由或脱离 PR 的本地冲突工作台，再评估独立 crate；不为当前只读子视图预先扩大架构。

## 每批交付记录模板

记录范围、实际改动、各平台验收/权限与 SHA fixture、UI/locale 手工结果、取消/工作区隔离、实际请求预算与资源证据、自动检查结果、失败/未执行项、是否采用和下一批前置条件。优化批次必须遵守仓库 Release A/B 门槛，失败候选不得成为下一批 baseline。
