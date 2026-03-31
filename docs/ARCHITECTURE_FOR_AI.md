# Usher 架构文档（面向后续 AI 对话）

## 1. 文档目标

这份文档用于让后续 AI 在对话中快速识别项目结构、边界和历史坑位，减少重复探索和误改。

适用场景：
- 新增页面/功能时快速定位改动点
- 性能优化与卡顿排查
- 修复 UI 闪烁、滚动抖动、弹窗错位等历史问题
- 排查 Workflow / Chat / Project 的状态与数据问题

---

## 2. 技术栈与运行形态

- 语言：Rust
- UI：Slint（Material 组件库，项目内有 `material-1.0-fixed`）
- 桌面目标：macOS / Windows（当前 macOS 适配更深）
- 异步：Tokio 共享 runtime（`src/app_runtime.rs`）
- 日志：tracing + 文件日志（`src/logger`）

入口：
- 二进制入口：`src/main.rs`
- 主装配入口：`src/lib.rs`
- Slint 主窗口：`ui/main.slint`

---

## 3. 目录与职责

### 3.1 Rust 后端

- `src/lib.rs`
  - 应用总装配（router、chat、project、workflow、update、toast、dialog、network 等）
  - 平台相关逻辑（如 macOS 主题与窗口交互）

- `src/router`
  - 路由策略与页面切换映射

- `src/chat`
  - 聊天业务、消息渲染、markdown 解析、持久化、会话状态
  - 关键文件：
    - `src/chat/session.rs`
    - `src/chat/persistence.rs`
    - `src/chat/interactive.rs`

- `src/project`
  - 项目列表管理、配置读写、搜索过滤、logo 处理、PR 概览统计
  - 项目配置文件路径：`data_local_dir/project/projects.json`

- `src/workflow`
  - 工作流核心（Branch / Tag / PR / CD）
  - `state.rs` 提供按项目隔离的视图状态
  - `bindings.rs` 负责 project->workflow 上下文切换
  - `tag.rs` / `branch.rs` / `pr.rs` / `cd.rs` 分别绑定各 tab 逻辑

- `src/update`
  - 更新检查、下载与安装流程

- `src/logger`
  - tracing logger / debug logger / error handling

### 3.2 Slint 前端

- `ui/main.slint`
  - 主窗口布局与全局 in/out property
  - 页面路由显示条件
  - 对 Rust callback 的桥接

- `ui/views/*`
  - 页面级视图（chat / project / workflow / settings / about ...）

- `ui/views/workflow/*`
  - Workflow 各 tab 视图拆分：`branch.slint`、`tag.slint`、`pr.slint`、`cd.slint`

- `ui/components/*`
  - 可复用组件（标题栏、状态栏、弹窗、通知、按钮等）
  - 当前已经沉淀出几条可复用体系：
    - 卡片操作按钮体系
    - 页面浮动操作体系
    - TerminalDrawer 终端体系

---

## 4. 核心数据与状态模型

### 4.1 Project 数据

配置项（`projects.json`）：
- `project_name`
- `project_alias`
- `local_repo`
- `remote_repo`
- `logo`
- `project_tags`
- `is_pr_auto_update`

UI 展示模型由 Rust 转换并注入 `project-items`，支持：
- 搜索过滤（名称/别名）
- 远程 logo 预取
- PR 概览异步填充

### 4.2 Workflow 状态

`src/workflow/state.rs` 的 `WorkflowStore`：
- `current_project`
- `projects: HashMap<project_id, WorkflowViewState>`
- `cd_running`

要点：
- 以项目维度隔离状态，避免跨项目串数据
- `set_project` 会切换上下文并清理部分历史视图状态
- `initialize_current_ui` 统一把当前状态刷到 Slint property

### 4.4 Terminal 状态

`src/terminal/mod.rs` 提供独立于 Workflow 的终端服务：
- `setup_terminal(ui)`
  - 启动时绑定全局 shell drawer 状态与事件
- `reset_terminal_for_project(ui, project_key, cwd, preserve_state)`
  - 项目切换时重置/保留终端状态
- `open_terminal_for_project(ui, project_name, project_key, cwd)`
  - 页面级入口，Project / Workflow / 后续页面都应优先复用它

当前终端状态由 `TerminalDrawerState` 持有：
- `project_key`
- `cwd`
- `output`
- `running`

行为约束：
- 同项目重复打开：
  - 保持终端内容、cwd、输入状态
- 切到新项目再打开：
  - 清空输出
  - 清空输入
  - 退出 fullscreen
  - 重置高度
  - 自动执行 `cd <local_repo>`

### 4.3 Chat 历史与分页

`src/chat/persistence.rs` 使用 JSONL 落盘：
- 路径：`data_local_dir/im/chat_history.jsonl`（与 logs 同级目录体系）
- 新消息 append 写入
- 启动只加载尾部最近 N 条（默认 20）
- 向上加载历史按批次（默认每次 10）

---

## 5. 主要数据流（AI 快速识别版）

### 5.1 Project -> Workflow

1. 在 Project 页点击项目卡片
2. 触发 `workflow_open_project(...)`
3. `src/workflow/bindings.rs` 组装 `WorkflowProjectContext`
4. 切换 `WorkflowStore.current_project`
5. `initialize_ui(...)` 刷新 Workflow 页头部和 tab 状态

### 5.2 Tag Run（本地模式）

1. UI 点击 Run -> `on_workflow_tag_run`
2. `src/workflow/tag.rs` 做输入与状态校验
3. 异步串行执行 git 命令步骤
4. 每步写回 step 状态 + log
5. 完成后更新 summary/current version

### 5.3 Chat 历史加载

1. 启动：`load_recent(startup_load)`
2. 上滑触底：`load_before(before_id, page_size)`
3. UI 维持滚动锚点，避免加载后跳动

### 5.4 页面 Terminal 打开链路

Project 页：
1. `ProjectCardActions -> TerminalActionButton`
2. 触发 `open-terminal(project_name, project_alias, local_repo)`
3. `ui/main.slint` 转发到 `project-open-terminal(...)`
4. `src/project/mod.rs` 调用 `open_terminal_for_project(...)`
5. `src/terminal/mod.rs` 处理复用/重置/默认 cd
6. `TerminalDrawer` 在页面底部 docked 打开

Workflow 页：
1. `PageScrollActions -> TerminalFab`
2. 触发 `shell-drawer-toggle()`
3. 若当前 drawer 关闭，则执行初始化命令（通常为 `cd 当前 workflow 项目路径`）
4. drawer 以 JetBrains IDE 风格占用页面底部空间

### 5.5 页面浮动操作链路

当前页面级浮动操作已经沉淀为统一组件：
- `ui/components/fab/floating_circle_button.slint`
  - 最基础圆形 FAB
- `ui/components/fab/back_to_top_fab.slint`
  - 回到顶部按钮
- `ui/components/fab/terminal_fab.slint`
  - 页面右下角终端入口
- `ui/components/fab/page_scroll_actions.slint`
  - 页面级组合容器，统一管理 `TerminalFab + BackToTopFab`

接入原则：
- 新页面如果只需要回到顶部：
  - 优先接 `PageScrollActions`
  - `enable-to-top: true`
  - `enable-terminal: false`
- 新页面如果既需要 terminal 又需要 toTop：
  - 同样优先接 `PageScrollActions`
  - 避免在页面里再次手写 FAB 布局

统一常量位于：
- `ui/layout_constants.slint`

当前已沉淀的 FAB 常量：
- `page-fab-right-offset`
- `page-fab-bottom-offset`
- `page-fab-stack-spacing`
- `to-top-show-threshold`

### 5.6 Project 卡片操作链路

当前 Project 卡片右上角操作区已组件化：
- `ui/components/button/terminal_action_button.slint`
  - 提供：
    - `IconActionButton`
    - `TerminalActionButton`
- `ui/components/button/project_card_actions.slint`
  - 统一封装：
    - Terminal
    - Settings
    - Delete

接入原则：
- 新增卡片级图标按钮时，优先复用 `IconActionButton`
- Project 卡片操作区的 hover / tooltip / 尺寸不要再回到页面里内联实现
- 三颗按钮的顺序、显隐和 hover 命中统一由 `ProjectCardActions` 维护

---

## 6. 改动原则（给后续 AI 的硬约束）

- 高内聚低耦合：
  - 页面逻辑优先进对应模块（如 project 逻辑放 `src/project`）
  - 共享状态统一收敛到 state 层，不在多个 callback 各自维护副本

- 单一职责：
  - `bindings.rs` 做桥接，不承载复杂业务
  - IO、网络、UI 状态更新分层处理

- 可维护性：
  - 所有新 property/callback 在 `ui/main.slint` 统一声明并命名一致
  - 异步流程必须可中断或可防重入（尤其滚动加载、workflow run）
  - 页面级浮动操作优先走 `PageScrollActions`
  - 页面级终端入口优先走 `open_terminal_for_project(...)` 或 `shell-drawer-toggle()`
  - 卡片操作区优先走 `ProjectCardActions / IconActionButton`

- 性能优先级：
  - 避免在滚动过程中触发重布局/重算
  - 避免 hover 条件创建/销毁节点（改为常驻 + opacity）

---

## 7. 历史“闪烁/抖动”问题经验库（重点）

以下是本项目已经踩过并验证过的典型问题，后续改 UI 必须对照检查。

### 7.1 Hover 图标闪烁

- 症状：鼠标移到卡片右上角操作图标时，图标反复出现/消失
- 根因：`if has-hover` 条件渲染导致节点频繁创建销毁，hover 命中区域丢失
- 有效做法：图标容器常驻，仅切 `opacity`；可见性由父 hover + 图标 hover 联合决定
- 禁忌：直接在 `if hover` 块里挂交互关键节点

### 7.2 “加载更多”重复触发

- 症状：一次加载后连续触发下一批，甚至连触两次
- 根因：缺少 `is_loading` 防重入 + 顶部判定阈值过宽 + 加载后仍停留触发区
- 有效做法：
  - 触发前先判 `is_loading` / `has_more`
  - 首批加载后滚动锚点定位到新批次底部，脱离触发阈值
  - 分页批次固定（如 10 条）

### 7.3 双 loading 同时出现

- 症状：列表顶部出现两个“Loading more”
- 根因：两处状态源同时驱动 loading UI
- 有效做法：单一 loading source of truth，渲染层只读一个状态

### 7.4 滚动卡顿（尤其 markdown 消息）

- 症状：滚动几条后出现“阶梯式”顿挫
- 根因：滚动中频繁触发 markdown 精细重算与重绘
- 有效做法：
  - 先粗略布局，滚动停止后再精细重算（节流/防抖）
  - 视口虚拟化，只渲染可见窗口消息
  - 历史加载异步化，主线程只做最小 UI 更新

### 7.5 窗口拖动瞬时卡顿

- 症状：消息等待期，拖动标题栏开始瞬间卡顿
- 根因：动画相位 + 高频轮询/重算与窗口交互争用主线程
- 有效做法：
  - 保留交互必要动画时，压缩每帧计算量
  - 将重计算移到后台或空闲时段
  - 避免在拖动起始瞬间启动额外昂贵任务

### 7.6 弹窗遮罩覆盖不全/错位

- 症状：遮罩未覆盖状态栏，顶部或底部留缝
- 根因：弹窗挂载层级错误（挂在内容区而不是主窗口层）
- 有效做法：Modal 挂在 MainWindow 顶层（高 z），覆盖完整窗口

### 7.7 窄屏布局挤压/重叠

- 症状：workflow tabs 和信息条在窄宽度重叠错位
- 根因：固定宽度 + 缺少断点布局策略
- 有效做法：按宽度断点切换列数/换行/最小宽度，避免硬编码单行布局

### 7.8 面包屑不左对齐

- 症状：顶部 breadcrumb 视觉上漂在中间，没有和页面正文左边界对齐
- 根因：
  - `Breadcrumb` 实例在 `main.slint` 中没有显式占满父容器
  - 只写 `horizontal-stretch` 在普通 `Rectangle` 容器里不生效
- 有效做法：
  - 在实例层显式设置 `width: parent.width; height: parent.height;`
  - 组件内部再做左对齐布局
- 禁忌：
  - 只调整 breadcrumb 内部布局，而忽略实例本身宽度

### 7.9 Terminal fullscreen 顶部黑条/滚动条残留

- 症状：TerminalDrawer fullscreen 后顶部出现黑色条带
- 根因：上方 `page-scroll-view` 虽然高度被压缩，但滚动条仍残留
- 有效做法：
  - fullscreen 时直接隐藏 `page-scroll-view`
  - fullscreen 高度按 breadcrumb 以下的工作区计算

### 7.10 Terminal fullscreen 关闭后 FAB 消失

- 症状：Terminal fullscreen 状态下关闭 drawer，右下角终端 FAB 不再出现
- 根因：关闭时只改了 `shell-drawer-visible`，没有同步清掉 `shell-drawer-fullscreen`
- 有效做法：
  - drawer 关闭时同步重置 fullscreen 状态

### 7.11 Docked Terminal 未按页面内容区对齐

- 症状：TerminalDrawer 在页面底部显示时，视觉上未与正文内容区水平对齐
- 根因：dock 区域以整个右侧工作区宽度为基准，而正文内容列存在额外 padding
- 有效做法：
  - docked Terminal 宽度应扣除与正文一致的左右 padding
  - 左侧菜单栏变化应影响工作区宽度，但正文 padding 也必须被纳入

---

## 8. 常见故障排查手册（快速）

### 8.1 Workflow 执行失败

优先检查：
1. `local_repo` 是否真实存在且可访问
2. 命令执行层是否被 fd/权限问题影响（看 `workflow::tag` 日志）
3. 当前项目上下文是否正确切换（`workflow_project_id`）

### 8.2 项目名中文显示乱码

优先检查：
1. 该 Text 是否显式设置中文字体（如 `Hiragino Sans GB`）
2. 字体是否在该页面对应 Text 节点遗漏
3. 跨页面组件是否混用不同字体回退策略

### 8.3 Add/Edit/Delete 弹窗布局异常

优先检查：
1. Dialog 高度是否依赖内容模式（delete 模式应更紧凑）
2. Action 区高度与 padding 是否过大
3. 分隔线是否独立于内容区并全宽

### 8.4 TerminalDrawer 行为异常

优先检查：
1. 当前 route 是否在 `shell-drawer-docked-route` 范围内
2. `shell-drawer-visible / shell-drawer-fullscreen / shell-drawer-height` 是否互相冲突
3. 是否通过 `open_terminal_for_project(...)` 打开，而不是旁路直接改 UI property
4. 日志里的：
   - `route`
   - `source`
   - `project_key`
   - `cwd`
   是否符合预期

当前日志 source 约定：
- `project-card`
- `workflow-fab`
- `terminal-close-button`
- `shell-toggle` / `project-terminal-toggle`

---

## 9. 推荐后续文档协作方式

后续每次修复后建议在此文档追加一条：
- `问题现象`
- `触发条件`
- `根因`
- `最终修复`
- `回归用例`

这样下一轮 AI 对话可以直接“按经验库命中”，减少试错。
