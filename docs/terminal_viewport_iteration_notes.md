# Terminal 视口问题迭代记录

日期：2026-04-02

## 回归检查清单

每次改 terminal 后，建议先跑这组最小回归：

1. 基础打开与输入
   - 打开任意项目 terminal
   - 确认 prompt 正常出现，不是只有闪烁光标
   - 输入 `ls`、`ls -al`

2. 同项目 reopen
   - 打开 terminal
   - 关闭 terminal
   - 再打开同一个项目 terminal
   - 确认没有旧 prompt 残留、没有黑色块残留、没有叠屏

3. `fnm` / shell 启动输出
   - 在启用 `fnm` 的项目里打开 terminal
   - 关闭再打开
   - 确认 `Using Node v...` 只出现一次，不叠到旧 prompt 行上

4. `fish`
   - 输入 `fish`
   - 确认欢迎语正常
   - 确认 prompt 路径、右侧 `(main)>` 和光标位置正常

5. `clear` / 控制键
   - 输入 `clear`
   - 确认 buffer 被清空
   - 执行一个长一点的命令后用 `Ctrl+C` 中断

6. `Select` 模式
   - 进入 `Select`
   - 确认可复制
   - 退出后 live terminal 仍能继续输入

如果只改了 terminal 生命周期，优先至少覆盖：

- 基础打开与输入
- 同项目 reopen
- `fnm` / shell 启动输出
- `fish`

## 背景

当前终端是基于 `vt100` parser + PTY + Slint 自绘 UI 的组合实现，不是现成的终端控件。

相关代码主要在：

- `src/terminal/mod.rs`
- `src/terminal/render.rs`
- `src/terminal/session.rs`
- `ui/components/drawer/terminal_drawer.slint`
- `ui/main.slint`

本轮核心目标原本是把终端收回到 JetBrains IDE 风格的固定 viewport 语义：

- 终端始终按真实 viewport 的 `rows/cols` 渲染
- 长输出占满可见高度并进入 scrollback
- 短输出保留正常终端空白区
- 命令输出结束后，提示符/光标行始终可见

但在多轮尝试后，终端这一块已经积累了不少补丁式逻辑和回归。这里把现象、尝试和坑统一记录下来，方便后续继续迭代时少走弯路。

## 当前已知问题

### 1. 终端 viewport 在“关闭后重新打开”路径上不稳定

现象：

- 第一次打开某些项目终端后输入 `ls -al`，显示可能正常
- 关闭终端后，再打开同一项目终端，输入 `ls -al`，提示符位置会异常
- 切换到另一个项目后打开终端，同样可能出现 viewport 错位

典型表现：

- 长输出后，prompt 没有落在底部，而是悬在中间
- 可见区底部保留了一大块空白
- 或者只显示最后几行，看起来像 viewport 行数过少

### 2. “首次打开 / 重新打开”的 rows/cols 同步时机不稳定

现象：

- 如果 session 用了旧的 `rows/cols` 启动，第一次命令输出就可能错误排版
- 手动拉伸窗口/Drawer 后，有时内容会恢复正常，说明问题和 viewport 同步时机强相关

### 3. Select 模式目前只保留第一版

当前结论：

- 第一版 `Select` 模式虽然简单，但至少“除了滚动更多历史外”基本可用
- 后续多次试图把它做成可滚动、可完全无缝切换的方案，都引入了更多 UI/交互回归
- 当前不建议在 live terminal viewport 问题没收稳前，再继续扩 Select 模式

## 已尝试过的方案与结果

### A. 输入事件模型重构

做过的事：

- 把键盘输入拆成“文本输入”和“显式控制 token”
- `Ctrl+C/D/L/Z` 映射为明确的控制字节
- `Cmd+C/V` 走复制粘贴，不和终端控制混用
- `Shift` 不再单独参与终端命令分支

结果：

- 这是成功的
- `Ctrl+C`
- `fish`
- `Shift+字母`
- `Cmd+C / Cmd+V`

这些基础能力相比最初都更接近 macOS 终端预期

结论：

- 这部分逻辑可以保留，不是当前 viewport 问题的根因

### B. 为 fish 增加最小终端应答

做过的事：

- 识别并回复：
  - `CSI 6 n`
  - `CSI c` / `CSI 0 c`

结果：

- `fish` 交互式启动问题基本解决

结论：

- 这部分逻辑也可以保留

### C. 基于鼠标坐标和字符格自己实现拖拽选区

做过的事：

- UI 里自己算 `mouse x/y -> row/col`
- Rust 里自己提取选中区文本
- 后续又尝试用 span `cells`、字符宽度、可见行映射去修正

结果：

- 失败
- 视觉高亮和实际复制文本多次不一致
- 典型错误是看起来选中了某段输出，复制出来却是另一段，比如 prompt 尾部 `"en"`、`"me"` 之类

结论：

- 这条路非常容易进入“视觉坐标系”和“终端逻辑坐标系”不同步的问题
- 不建议继续在 live ANSI 视图里手搓拖拽选区

### D. 用 `TextInput` overlay 接管 Select 模式

做过的事：

- 用一层只读 `TextInput` 覆盖在终端文本之上
- 试图用控件内建选区完成拖拽选择和复制

结果：

- 多次失败
- 常见回归：
  - 选不中
  - 没有高亮
  - 抢走滚动事件
  - 抢走输入焦点
  - 切换到 Select 模式后行距变化，视觉重排明显

结论：

- `TextInput` 适合做简单文本选择，不适合在当前这套 live terminal 自绘结构上做“完全无缝”的 overlay

### E. 固定行列表 + 自绘高亮的 Select 模式

做过的事：

- 试图把 `Select` 模式改成固定行列表
- 自己画选区背景矩形
- 自己按行列提取复制文本

结果：

- 同样没有收稳
- 增加了新的显示层，反而继续放大了 viewport 和文本排版不一致的问题

结论：

- 这是另一个过度设计方向
- 当前不建议继续深入

### F. 为了解决“prompt 看不见”而减少 rows / cols

做过的事：

- 加 `terminal-row-safety-padding`
- 加 `terminal-col-safety-padding`
- 额外减 1 / 2 / 4 行列
- 给右侧多留 gutter

结果：

- 失败，而且副作用非常明显
- 典型副作用：
  - 右侧最后一列被截断
  - `ls` 列数变少，输出高度被拉长
  - 终端滚动时只剩一小块区域显示内容
  - 表面上 prompt 有时能看到，但 viewport 语义被破坏

结论：

- “少报 rows/cols”只能作为临时补丁，不是正确方案
- 不应继续依赖这类安全 padding 做最终实现

### G. 裁剪 `rendered_text()` / `rendered_lines()` 尾部空白行

做过的事：

- 在 `src/terminal/render.rs` 里裁掉尾部 blank rows
- 试图做成“短输出更紧凑、prompt 更靠下”的假象

结果：

- 失败
- 它会把真实终端固定 viewport 语义变成“流式裁剪视图”
- 最终效果就是：
  - 上面不一定放得下
  - 下面却还可能留空
  - 看起来既不像标准终端，也不像 IDE 终端

结论：

- 这条逻辑已经被证明不适合当前目标
- 如果继续做 JetBrains 风格固定终端，就不要再裁 blank rows

### H. 延迟两次 `resize` 同步线程

做过的事：

- session 启动后，延迟 `60ms / 180ms` 再补两次 `session.resize(...)`

结果：

- 不稳定
- 某些场景有帮助，某些场景没有
- 它掩盖了“启动时 viewport 没稳定”的根因，但没有真正解决

结论：

- 这属于典型的时序补丁
- 不建议再继续依赖

### I. 只在 `viewport-resized` 后才启动 session

做过的事：

- 为了避免 session 用错误 rows/cols 启动，曾尝试把 `ensure_session_started(...)` 完全绑定到首次 `viewport-resized(...)`

结果：

- 直接引入新回归：打开终端后只剩一个闪烁光标，看不到 shell 输出

原因：

- `TerminalDrawer` 是条件创建的
- 组件首次创建时，`viewport-resized` 不一定按预期时序触发
- 结果是 session 根本没启动

结论：

- “只靠 resize 回调启动 session” 不可靠
- 必须保留“打开终端时可启动 session”的主路径

### J. 字体切换到中文字体作为主字体

做过的事：

- 尝试把终端主体字体改成 `Hiragino Sans GB`

结果：

- 失败
- 终端排版立即错乱
- 根因很直接：它不是等宽字体

结论：

- 终端主体必须保持等宽字体，例如 `Menlo`
- 中文 fallback 可以做，但不能让比例字体成为终端主字体

## 当前保留的结论

下面这些结论是已经比较明确的：

### 1. live terminal 必须保持固定 viewport

也就是：

- `vt100` 当前 `screen` 有多少行，就按多少行语义渲染
- 不要在 UI 层做“紧凑流式裁剪”

### 2. 主字体必须是等宽字体

当前方向应保持：

- 主体：`Menlo`
- 中文只作为 fallback，而不是主字体

### 3. 终端要 follow-bottom，但不能靠少报 rows 实现

正确目标是：

- 有新 PTY 输出时，把 scrollback 置回 `0`
- prompt 自然出现在 screen 底部

错误做法是：

- 通过额外减行、减列、裁空白来“强行腾 prompt 空间”

### 4. 关闭终端 / 切项目 / 再打开，确实是关键路径

这个问题不是只发生在“切换项目”：

- 关闭同一个项目终端，再重新打开，也会复现

因此后续调试时，必须始终覆盖下面两个场景：

1. 同项目：打开 -> 关闭 -> 再打开 -> `ls -al`
2. 切项目：项目 A 打开 -> 关闭 -> 项目 B 打开 -> `ls -al`

## 当前代码状态备注

当前代码里已经有以下值得注意的点：

- `TerminalDrawer` 新增了 `viewport-ready`
- `TerminalDrawer` 新增了一个轻量 `initial-viewport-sync` 定时器，会在组件 ready 后补一次 `viewport-resized(...)`
- `mod.rs` 已恢复“打开终端时立即 `ensure_session_started(...)`”，避免再次出现“只有光标闪、session 没启动”

但这并不代表 viewport 问题已经彻底解决，只是把系统拉回“可用但未收稳”的状态。

## 当前未完全解决的问题

截至 2026-04-02，这个问题仍然没有完全收稳：

- 同一个项目终端关闭再打开后，`ls -al` 的显示仍可能异常
- 切换到另一个项目后重新打开终端，也仍可能复现
- 现象通常是：
  - 长输出后 prompt 没有贴底
  - 底部留出明显空白
  - 或 visible screen 行数不符合预期

## 已修复问题：关闭后重开导致 prompt / fnm 输出叠屏

这个问题已经确认并修复，单独记录，避免后续 reopen 语义被误改回去。

### 现象

复现路径：

1. 打开某个项目的 terminal
2. 关闭 terminal
3. 再次打开同一个项目 terminal

典型表现：

- prompt 行重复累加
- 光标所在位置出现黑色区块残留/叠加
- 如果 shell 启动时会输出 `Using Node v...`，这行也会重复堆叠
- `Using Node` 甚至可能叠到旧 prompt 行或旧历史输出之上

### 根因

根因不是 `fnm` 本身，也不是 shell 重复打印，而是我们在 close 路径里：

- 只终止了 PTY session
- 没有同步清空本地 `render`
- 没有清空 `lines`
- 没有重置 `cursor` / `scrollback`
- 没有清空 UI 侧复用的 `lines_model` / `span_models`

同时，重新打开同一个项目时，旧逻辑还会因为“同项目”走 `preserve_state` 路径，导致：

- 新 shell 的启动输出继续叠到旧 screen 上
- UI 看起来像是 shell 自己重复输出，实际上是旧 renderer 状态没清干净

### 修复后的正确语义

现在关闭 terminal 后，再打开同一个项目，采用的是：

- **总是启动一个全新的 terminal 会话**

也就是说：

- close 等价于放弃当前 terminal 历史
- reopen 不恢复旧 buffer
- reopen 不恢复旧 prompt
- reopen 不恢复旧 `Using Node ...`

只有在“terminal 当前还开着”的前提下，再次触发打开动作，才允许保持当前会话；一旦已经关闭，再打开就必须 reset。

### 修复点

修复内容包括：

- close 时除了 `terminate_session_locked()`，还同步重置：
  - `render`
  - `lines`
  - `scrollback_offset`
  - `max_scrollback`
  - `cursor`
  - `selection_mode`
  - `selection_output`
  - `running / connected`
- close 后立即把空状态回写 UI，避免旧模型残留到下一轮事件循环
- reopen 同项目时，不再因为“project_key 一样”就复用已关闭会话

### 后续不要再踩的坑

后续如果再调整 terminal reopen 生命周期，不要回到下面这些做法：

1. 不要把“同项目 reopen”默认当成 preserve session
2. 不要只停 PTY，不清本地 renderer/UI 模型
3. 不要把 close 语义做成“只是隐藏 drawer”，除非同时明确支持完整会话恢复

### 这条问题的回归测试

后续每次改 terminal 生命周期，至少手测下面两条：

1. 同项目：
   - 打开 terminal
   - 输入 `ls` 或 `ls -al`
   - 关闭 terminal
   - 再打开同一个项目 terminal
   - 确认只有一份 prompt，没有黑色块残留

2. 启用 `fnm` 的项目：
   - 打开 terminal
   - 关闭
   - 再打开
   - 确认 `Using Node v...` 只出现一次，且不叠到旧 prompt 行上

## 不建议再走的方向

后续迭代时，下面这些方向建议直接避免：

1. 不要再继续加 `row/col safety padding`
2. 不要再裁 `rendered_text()` / `rendered_lines()` 的尾部空白
3. 不要再尝试只靠 `viewport-resized` 启动 session
4. 不要再把比例字体用于 live terminal 主字体
5. 不要在 live ANSI 视图里继续手搓拖拽选区
6. 不要把 `Select` 模式和 live terminal viewport 问题绑在一起一起修

## 建议的下一步

如果后续继续迭代，建议按下面顺序做：

### 第一步：先把“session 启动使用的 viewport”单独收稳

建议做法：

- 在 Rust 侧维护一份“最近一次有效 viewport”
- 只有 drawer 打开且尺寸有效时，才更新这份 viewport
- 新 session 启动时，只用这份最近一次有效 viewport 初始化 PTY
- 如果当前没有有效 viewport，就暂时用默认值，但在首次有效 resize 到来后立刻同步

目标：

- 彻底切断“隐藏态旧 rows/cols 污染新 session”的路径

### 第二步：给 viewport 同步打完整日志

建议新增日志：

- drawer 打开时的 `rows/cols`
- session spawn 时实际使用的 `rows/cols`
- 每次 `viewport-resized` 的 `rows/cols`
- 第一次 PTY 输出到来时的 `rows/cols`

目标：

- 后续再调这个问题时，不需要纯靠截图猜时序

### 第三步：等 live viewport 收稳后，再考虑 Select 模式

当前建议：

- 保持第一版 `Select` 模式不动
- 不要和 live terminal viewport 一起重构

## 一句话结论

这个问题的根因不是单纯的“多减几行”或者“裁几行空白”，而是：

- PTY session 的启动时机
- 有效 viewport 的同步时机
- 本地 parser / UI screen / PTY rows-cols 三者的一致性

只要这三者没有收成一条稳定链路，继续在 UI 层加补丁，只会反复引入新回归。
