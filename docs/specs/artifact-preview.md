# Artifact Preview Framework / 文件预览框架

## Scope / 范围

This specification defines the reusable artifact-preview framework used by Chat,
AI Review, Workflow, Project, and future modules. It covers provider discovery, parsing,
presentation hosts, persistent settings, immediate host migration, lifecycle,
security, and performance acceptance. Markdown is one provider; it is not the
owner of the framework.

本文定义供 Chat、AI Review、Workflow、Project 及后续模块复用的文件预览框架，涵盖 Provider
发现与解析、展示 Host、持久化设置、Host 即时迁移、生命周期、安全边界和性能验收。
Markdown 只是一个 Provider，不拥有整个框架。

## Design goals / 设计目标

1. Callers submit a domain-neutral artifact and never depend on a renderer or
   window implementation.
2. File interpretation and presentation location are independent strategy axes.
3. Providers are independently registered, runtime-enabled/disabled, and may be
   excluded at build time without changing callers or hosts.
4. Only the active provider/session is mounted. Closing or replacing a preview
   releases its heavy model and decoded pixels.
5. Parsing, decoding, and file I/O never run on the Slint event loop.
6. Chat resize never reparses Markdown or rebuilds its message model merely
   because available width changed.
7. Compact agent responses may reuse Markdown structure without paying the
   syntax-highlighting initialization cost of a full Preview.

1. 调用方只提交领域无关 Artifact，不依赖具体 Renderer 或窗口实现。
2. 文件解释方式与展示位置是两条互相独立的策略轴。
3. Provider 独立注册，可在运行期启停，也可在构建期排除；调用方和 Host 无需改变。
4. 只挂载当前活动 Provider/Session；关闭或替换预览时释放大模型和解码像素。
5. 解析、解码和文件 I/O 不得在 Slint event loop 执行。
6. Chat 可用宽度变化不得触发 Markdown 重解析或仅因 resize 重建消息 model。
7. 紧凑 Agent 回复可以复用 Markdown 结构，但不得承担完整 Preview 的语法高亮初始化成本。

## Ownership / 职责边界

| Owner | Responsibility |
| --- | --- |
| `crates/usher-preview/` | Framework-neutral artifact descriptors, provider trait, registry/policy, prepared payloads, and built-in Markdown/image/plain-text providers. It must not depend on Chat, `MainWindow`, or Slint. |
| `src/artifact_preview/` | Application facade, worker dispatch, stale-request cancellation, bounded prepared-session ownership, Slint model adaptation, clipboard/external-open actions, host switching, and handle-verified containment/read for explicitly activated repository Markdown. |
| `src/agent_response/` | Bounded compact projection of agent Markdown and opaque repository-file action tokens. It does not read files, trust model paths, or own a host. |
| `ui/components/artifact_preview/` | Shared Primer preview shell, Markdown/text/image surfaces, docked panel, and detached native window. It performs no I/O or parsing. |
| `ui/main.slint` | Root placement of the docked host and the stable Rust-facing preview contract. |
| `src/settings.rs`, `src/settings_page.rs` | Persistent preference and explicit bridge into the independently generated Settings page. |
| Feature callers | Construct `ArtifactSource` and call the facade. They do not select providers or hosts. |

The AI Review controller owns token lookup and task binding; the agent-response
layer identifies and normalizes candidate relative Markdown references but can
expose them only as opaque actions. The Preview facade alone verifies and reads
the selected repository file. / AI Review Controller 负责 token 查表与 task 绑定；Agent
回复层识别并归一化候选相对 Markdown 引用，但只能暴露不透明 action；只有 Preview
门面验证并读取最终仓库文件。

## Two-axis strategy / 双策略轴

```text
ArtifactSource
    -> PreviewRegistry -> enabled Provider -> PreparedArtifact
                                              |
                         PresentationMode ----+
                            |                 |
                            +-> DockedHost    +-> DetachedHost
```

The provider axis answers “what is this and how is it decoded?”. The host axis
answers “where is the already prepared payload shown?”. Changing one axis must
not invoke the other again. In particular, switching Docked -> Detached moves the
current prepared payload without reparsing or retaining two copies.

Provider 轴回答“它是什么、如何解码”；Host 轴回答“已准备好的内容在哪里展示”。切换
任一轴不得重新执行另一轴。尤其 Docked -> Detached 必须迁移同一个已准备 payload，
不得重新解析或同时保留两份内容。

## Core contracts / 核心契约

`ArtifactSource` contains a stable id, title, optional path, declared MIME type,
and either UTF-8 text or bytes. File-backed sources are bounded before loading.

Every `PreviewProvider` exposes:

- a stable provider id and human-readable label;
- `probe(&ArtifactDescriptor) -> MatchScore`, using magic bytes/MIME before the extension;
- `prepare(ArtifactSource, CancellationToken) -> PreparedArtifact`;
- declared capabilities such as copy, search, zoom, or external-open.

`PreviewRegistry` filters disabled providers before probing, selects the highest
score deterministically, and returns Unsupported when no provider accepts the
source. A disabled provider must not read, parse, decode, or cache the artifact.
Provider failure must not silently route sensitive content through a lower-score
provider.

`ArtifactSource` 包含稳定 id、标题、可选路径、声明 MIME，以及 UTF-8 文本或字节；
文件来源在读取前必须执行大小限制。`PreviewProvider` 提供稳定 id、探测分值、可取消
prepare 和能力声明。Registry 先过滤已禁用 Provider，再确定性选择最高分；禁用后的
Provider 不得读取、解析、解码或缓存内容。高优先级 Provider 失败后不得静默把敏感
内容交给低优先级解析器。

Repository Markdown opening is an explicit action contract, not another probe
source. The caller supplies a task-bound project root and an opaque action whose
relative path remains in Rust. Both projection and facade reject unsafe syntax;
the facade accepts only `.md`/`.markdown` and rejects absolute/root/prefix/parent
components. On Unix a worker opens the canonical root once and traverses every
relative component from that directory descriptor with no-follow semantics. On
Windows it checks the final paths of the opened root and target handles. The
verified regular-file handle enforces `MAX_SOURCE_BYTES`, performs the bounded
read, and then validates UTF-8 before constructing `ArtifactSource`. Failure never falls back to
an external opener, a remote fetch, or another project's root.

打开仓库 Markdown 是显式 action 契约，而不是另一种 probe 来源。调用方提供与 task
绑定的项目根目录，以及相对路径仍只保留在 Rust 的不透明 action。门面先归一化相对
路径，只接受 `.md`/`.markdown`，拒绝绝对/root/prefix/父目录 component。Unix Worker
只打开一次 canonical 根目录，然后从该目录 descriptor 逐 component 以 no-follow
语义遍历；Windows 检查已打开根目录与目标句柄的最终路径。已验证的普通文件句柄执行
`MAX_SOURCE_BYTES`、有界读取与 UTF-8 验证后再构造 `ArtifactSource`。
失败不得回退到外部打开、远程获取或其他项目根目录。

## Built-in providers / 内置 Provider

| Provider | Recognition | Prepared payload | Default |
| --- | --- | --- | --- |
| Markdown | `text/markdown`, `.md`/`.markdown`, or explicit Chat Markdown | Width-independent block model parsed by `pulldown-cmark` | Enabled |
| Image | PNG/JPEG/GIF/WebP magic or matching image MIME/extension | Decoded bounded RGBA pixels plus dimensions | Enabled |
| Plain text | explicit text MIME or UTF-8 text file after stronger providers decline | Bounded UTF-8 text | Enabled |

Provider source modules are individually Cargo-feature gated. Runtime settings
control registered providers without restarting. A build that excludes a feature
contains neither that provider nor its optional parser dependency.

首版注册 Markdown、图片和纯文本三个独立 Provider。源码分别受 Cargo feature
控制；运行时设置无需重启即可控制已注册 Provider。构建时排除 feature 后，对应
Provider 和可选解析依赖都不得进入安装包。

## Presentation behavior / 展示行为

`PresentationMode::Docked` is the default. It mounts one application-level panel
at the right edge of `MainWindow`, outside every route. The panel is therefore
available to Chat, Workflow, Project, and future routes. On wide windows it
reserves content width; on narrow windows it overlays the route instead of
compressing it below its usable minimum. Its surface starts at the root-window
top edge and a continuous one-pixel hairline separates it from route content.
Opening and closing drive both panes from one 260ms animated progress value: a
complete, right-anchored panel is revealed or clipped at the moving divider while
the route reserves or reclaims the same width. Preview and Chat document layout
widths stay stable during that transition, so neither tree reflows on every
animation frame. The feature-owned prepared session is released immediately on
close; its read-only Slint projection may remain mounted for at most 280ms to
paint the exit, then blocks, decoded pixels, and other visual payload state are
cleared. The docked Markdown layout bridge clears its projected rows, block
extents, projection guards, and font/layout caches at the same boundary while
retaining the stable Slint model identity registered by the root callback. The
optimized native host applies the shared exact-extent virtual-list
facade to Markdown and instantiates only a bounded visible/overscan block window
at rest. During the docked shell transition, it retains the complete
prepared model but suspends document painting because FemtoVG otherwise replays
every descendant when the ancestor clip moves. Header, background, divider, and
route surfaces remain painted through the complete transition. The lightweight
loading spinner and its current parser label remain visible; document-paint
suspension applies only after a payload is prepared. The
leading hairline has a wider invisible resize hit target. Hover emphasizes the
line and shows a column-resize cursor; direct dragging changes the panel width
within bounded panel/main-content limits. The preferred width survives close,
reopen, mode changes, route changes, and temporary window clamps for the current
application session, but is not durable user configuration.
While open, the docked host owns the trailing title-bar region and paints above
ordinary shell title-bar actions, so shell controls cannot overlap preview
controls. Header actions form one right-aligned compact group: Close is always
last, optional External Open and Copy precede it, and hidden actions reserve no
space.

`PresentationMode::Detached` shows the same prepared payload in one reusable
native Slint window belonging to the Usher process. It is modeless and does not
create a browser process. Opening another artifact replaces the current session.
On macOS the native window title is the only visible document title: the shared
in-content Header is removed, and a trailing native title-bar accessory owns the
same compact Copy, External Open, and Close actions. Optional actions are
reflowed only when preview capabilities change, while Close remains last.
Platforms without this native accessory retain the shared in-content Header so
the actions are never lost.
Because that window owns an independent Slint component tree, the Preview facade
must explicitly synchronize the resolved effective dark mode when creating or
reusing it and whenever the application theme changes; its local theme and
standard-widget palette must never fall back to their default Light state.

Changing the setting is immediate:

1. persist and publish the new mode;
2. if no preview is open, update policy only;
3. if a preview is open, mount the destination host from the current prepared payload;
4. reveal the destination, then unmount and clear the old host;
5. never leave both heavy surfaces mounted after the event-loop turn.

`PresentationMode::Docked` 默认启用，在 `MainWindow` 右侧根层挂载，不属于任何
路由；宽窗口预留内容宽度，窄窗口改为覆盖，避免把页面压缩到不可用。
右侧面板从根窗口顶边开始，并以连续 1px 分隔线与路由内容分开。开启和关闭时，面板与
主内容共享一个 260ms 动画 progress：完整宽度的面板始终靠右，只在移动分割线处揭示或
裁剪，主内容同步预留或回收完全相同的宽度；Preview 与 Chat 文档树在过渡期间保持稳定
布局宽度，不得逐帧重排。关闭时 Feature 持有的 prepared session 立即释放，只读 Slint
视觉投影最多保留 280ms 绘制退出动画，随后清除 block、解码像素及其他视觉 payload；
停靠 Markdown layout bridge 同时清空 projected rows、block extent、projection guard
及字体/layout 缓存，但保留根 callback 注册的稳定 Slint model 身份。
优化后的原生 Host 使用共享精确 extent 虚拟列表门面，稳定时只实例化有界的
可见/overscan Markdown block。停靠 Shell 过渡期间保留完整准备后 model，但暂停文档绘制，因为 FemtoVG 会在祖先
clip 移动时逐帧重放所有后代；Header、背景、分隔线和路由 Surface 在完整动画期间继续绘制。
轻量 loading spinner 与当前解析器文案保持可见；文档绘制暂停只作用于准备完成后的 payload。
分隔线
拥有更宽的透明 resize 命中区；hover 时强化线条并显示列缩放光标，直接拖拽在面板和主内容
的安全宽度范围内调整尺寸。用户偏好宽度在当前应用会话内跨关闭、重开、模式切换、路由切换
和临时窗口 clamp 保留，但不作为持久设置保存。
停靠面板打开时接管标题栏右侧区域，并绘制在普通 Shell 标题栏操作之上，Shell 控件不得
覆盖预览控件。Header 操作使用一个右对齐的紧凑操作组：关闭始终在最右侧，可选的外部
打开与复制依次位于其左侧；隐藏操作不得保留空白槽位。
Detached 使用同进程的可复用 Slint 原生窗口，不创建浏览器进程。macOS 只显示一次
原生文档标题：共享内容 Header 不再挂载，
同一组复制、外部打开和关闭操作由标题栏右侧的原生 accessory 承载。可选操作仅在
Preview capability 变化时重新紧凑排列，关闭始终位于最右侧；不具备该原生 accessory
的平台继续保留共享内容 Header，确保操作入口不会丢失。由于该窗口拥有独立的
Slint component tree，Preview facade 必须在创建、复用窗口以及应用主题变化时显式同步
解析后的 effective dark mode；其本地 Theme 与标准控件 Palette 不得回退到默认浅色。
设置变化立即迁移当前 payload，目标 Host 显示后卸载旧 Host，一个 event-loop turn 后不得
同时保留两棵重 UI 树。

Both hosts separate live bounds from prepared-content layout. While the docked
divider or detached native window is actively resizing, the host follows the
pointer/window edge and clips the shared surface, but the complete prepared
document tree retains its last settled width. Gesture end applies the exact
final content width once. Provider implementations and prepared payloads must
not depend on host-specific resize state. Detached native notifications are
scoped to that window and must not reuse `MainWindow.window-live-resizing`.
Backends that do not surface native begin/end notifications use a bounded,
window-local width debounce; it must not affect other windows.
The document scrollbar is hidden while its content width is intentionally frozen
and returns at the settled trailing edge; the affected route scrollbar follows
the same rule during a docked open/close transition. A root content scrollbar that extends
under a macOS full-size title bar must place its draggable track below the native
title-bar inset so AppKit cannot turn a thumb drag into a window drag.

两种 Host 都必须分离实时可见边界与准备后内容布局。固定分割线或独立原生窗口处于
resize 时，Host 跟随指针/窗口边缘并裁剪共享 Surface，但完整准备后文档树保持上次
稳定宽度；手势结束后只应用一次精确最终内容宽度。Provider 实现及准备后 payload
不得依赖 Host 专有 resize 状态。独立窗口原生通知必须按该窗口隔离，不得复用
`MainWindow.window-live-resizing`。

内容宽度被冻结期间隐藏文档滚动条，稳定后只在最终右边缘恢复。根内容滚动区延伸到
macOS 全尺寸标题栏下方时，可拖动 scrollbar track 必须从原生标题栏 inset 下方开始，
避免 AppKit 把滚动条拖动识别为窗口拖动。

## Markdown presentation / Markdown 展示

Markdown remains native: `MarkdownParserLoader` owns a `pulldown-cmark` parser,
while `CodeParserRegistry` owns independently registered language loaders.
`ParserLoadStrategy::OnDemand` is the production default; explicit
`PreloadAll` initializes every registered loader. A cold loader is initialized
once and its parser is cached by normalized fence language. TypeScript/TSX uses
the dedicated `TsParserLoader` backed by Tree-sitter; JavaScript has a dedicated
loader and other languages use `SyntectParserLoader`. The latter two share one
lazy `syntect` repository with `two-face`'s bat-curated syntax set, avoiding
duplicate complete syntax repositories. Unknown or omitted languages fall back
to plain monospace text. Slint 1.17 `StyledText` owns inline emphasis/link
shaping. The shared Preview surface maps those semantics to the same Codex-style
tokens used by compact AI Review responses: a 14 px body in a 22 px line box,
24/20/17/15 px H1-H6 steps, 20 px between adjacent paragraphs, 10 px between
prose and a list group, and no extra space between consecutive list items. It
uses a 24 px hanging-list gutter with structured ordered/task markers, Primer
semantic colors, draws no automatic heading rules, and keeps transparent
quotes with a 4 px leading rule, table separators, and subtle bordered code
surfaces using the platform-registered code font. Fenced code uses a 10 px top
inset without a language label, a compact 32 px label-and-content inset when a
language exists, and 10 px bottom padding. Text blocks include explicit
glyph-height safety. For prose, the Parley extent bridge mirrors the settled
width, font family, size, inline styles, fixed 22 px body rhythm, and the same
safety allowance, so CJK/Latin/bold fallback runs cannot be clipped or diverge
from the virtual extent. Theme changes select the already-prepared token
projection and must not reparse Markdown or re-run highlighting. Code tokens are
rendered directly as native Slint `Text` rows with explicit foreground colors;
they are never round-tripped through Markdown or HTML markup. A feature-owned
Parley adapter measures exact block extents only when the
document revision or settled content width changes. The shared
`usher-virtual-list` facade maps those extents into a bounded visible/overscan
projection. The Preview `Flickable` and exactly one application-owned Primer
scrollbar consume that same stable content extent; neither may derive its range
from Slint `ListView.viewport_height`. Thumb height must remain constant while
scrolling, and 25/50/75/100 percent thumb travel must map to the same normalized
document offsets, with bottom content reached only at 100 percent. It does not
add Wry, WebView, HTML/CSS injection, or a browser helper process. Provider
parsing remains independent from this replaceable visual theme.

Markdown 保持原生实现：`MarkdownParserLoader` 持有 `pulldown-cmark` 解析器，
`CodeParserRegistry` 持有彼此独立注册的语言 loader。生产默认使用
`ParserLoadStrategy::OnDemand`，显式 `PreloadAll` 会初始化所有已注册 loader；冷 loader
只初始化一次，并按规范化围栏语言缓存 parser。TypeScript/TSX 使用 Tree-sitter 支撑的
专用 `TsParserLoader`；JavaScript 使用独立 loader，其他语言由
`SyntectParserLoader` 处理。后两者共享一份惰性 `syntect` Repository 与 `two-face`
维护的 bat 语法集合，避免重复完整语法库。未知或未声明语言自动回退为纯等宽文本。
Slint 1.17 `StyledText` 负责行内强调和链接排版。
共享 Preview Surface 延续紧凑 AI Review 回复使用的 Codex 排版 token：14px 正文与
22px 行盒、24/20/17/15px 的 H1-H6、相邻段落 20px、正文与列表组 10px、连续列表项
不额外加间距。列表使用 24px 悬挂 gutter，并结构化显示有序/任务 marker。Surface
使用 Primer 语义色、不自动为标题绘制分隔线，并保留透明底加 4px 左侧强调线的引用块、
表格分隔，以及使用平台注册代码字体的低对比带边框代码 Surface。文字 block 保留显式
字形高度安全量。无语言标签的围栏代码顶部为 10px，有语言标签时标签与正文共用紧凑的
32px 顶部空间，底部为 10px。对于正文，Parley extent bridge 同步稳定宽度、字体、字号、
行内样式、固定 22px 正文节奏及相同安全余量；CJK/Latin/粗体 fallback 不得裁切，且
渲染高度不得偏离虚拟 extent。主题切换只
选择已经准备好的 token 投影，不得重新解析 Markdown 或重新执行高亮。代码 token
必须以带显式前景色的原生 Slint `Text` 行直接绘制，禁止再经 Markdown 或 HTML 标记
二次转换。不得为该
视觉主题引入 Wry、WebView、HTML/CSS 注入或浏览器辅助进程。Feature 自有 Parley
适配器只在文档 revision 或稳定内容宽度变化时测量精确 block extent；共享
`usher-virtual-list` 门面据此生成有界可见/overscan 投影。Preview `Flickable` 与唯一的
应用自有 Primer scrollbar 必须消费同一个稳定 content extent，禁止再从 Slint
`ListView.viewport_height` 推导范围。滚动全程滑块高度必须不变；滑块位于
25%/50%/75%/100% 时必须映射到相同的文档归一化 offset，并且只有 100% 才到达正文底部。
Provider 解析继续与可替换视觉主题解耦。

The parser core also exposes a structure-only entry for inline agent response
projection. It runs `pulldown-cmark` with the same block/inline semantics but
does not construct, initialize, or query `CodeParserRegistry`, Tree-sitter,
`syntect`, or `two-face`. Fenced blocks retain language and source text and
leave `code_spans` empty for the compact renderer. This entry never changes the
full Markdown Provider: opening the original response in Preview still uses the
configured loader strategy and produces prepared light/dark highlighting on a
worker.

Both parser entries preserve H1-H6, ordered/unordered/task-list markers,
emphasis, strikethrough, links, inline code, quotes, tables, rules, and fenced
code. A residual model/transport escape layer before ASCII Markdown punctuation
is removed after a non-fenced block is assembled, including inline-code
identifiers; fenced-code source and link targets remain byte-preserved.
Paired `**` runs that survive parsing as ordinary prose are treated as malformed
model formatting residue and hidden; a single literal run remains visible.

Parser core 另行提供行内 Agent 回复投影使用的纯结构入口。它以相同 block/inline
语义运行 `pulldown-cmark`，但不构造、初始化或查询 `CodeParserRegistry`、Tree-sitter、
`syntect` 或 `two-face`。围栏 block 保留语言和源码，紧凑 renderer 使用的
`code_spans` 为空。该入口绝不改变完整 Markdown Provider：在 Preview 中打开原始
回复时，仍按配置的 loader 策略在 Worker 生成准备好的亮/暗色高亮。

两个 parser 入口都保留 H1-H6、有序/无序/任务列表 marker、强调、删除线、链接、行内
代码、引用、表格、分隔线与围栏代码。非围栏 block 组装完成后，会清理模型/传输层在
ASCII Markdown 标点前残留的一层转义（包括行内代码 identifier）；围栏代码源码与链接
target 继续逐字节保留。
解析后仍以普通正文成对残留的 `**` 视为模型格式残渣并隐藏；单个字面量仍保留显示。

## Chat integration / Chat 集成

Chat no longer owns Markdown block rendering or width-dependent wrapping. A
Markdown assistant message keeps its readable plain-text projection inline so
the response is available without a click, and adds a compact artifact title
action that opens the fully formatted document. Opening the action submits the
original Markdown to the shared preview facade.

The Chat render cache key contains message identity, not window width. Native
text layout may reflow during resize, but resize must not run `pulldown-cmark`,
replace `chat-messages`, or rebuild the complete history model.

Chat 不再拥有 Markdown block 渲染或宽度相关手工换行。Markdown assistant 消息
仍以内联纯文本投影展示实际内容，用户无需点击才能读取；同时提供紧凑标题入口，点击
后把原始 Markdown 交给通用预览门面。Chat cache key 只包含消息身份，不包含窗口
宽度；resize 可触发原生文本排版，但不得运行 `pulldown-cmark`、替换
`chat-messages` 或重建完整历史 model。

## AI Review integration / AI Review 集成

An AI Review assistant row keeps a bounded structured projection inline. Its
leading heading is the full-response action; only a response without a leading
heading appends a bounded fallback title. Repository Markdown
links are opaque actions resolved against the repository bound to that review
task; the Slint action target carries only the opaque token, never the trusted
relative path. User rows and the
same-binary `baseline` variant remain raw text. The `formatted` variant is the
production default.

AI Review Assistant 行内保留有界结构投影；leading heading 本身就是完整回复 action，
只有没有 leading heading 的回复才追加有界 fallback title。
仓库 Markdown link 是相对该 Review task 绑定仓库解析的不透明 action；Slint action
目标只携带不透明 token，绝不接收可信相对路径。User 行和同一二进制的 `baseline`
变体继续显示原始文本；生产
默认使用 `formatted` 变体。

When such an artifact opens in Docked mode on `Route.PrReview`, Preview occupies
the existing AI Review trailing slot and width. It replaces only the visible AI
surface by moving the one application Preview host into that slot; no second
Markdown surface is mounted. The conversation model stays mounted, PR content reserves the slot once,
the existing divider controls both surfaces, and PR Dialogs remain above them.
Closing Preview restores the intact AI surface. Detached mode uses the independent
native window and leaves AI Review visible; it never participates in the slot
replacement.

在 `Route.PrReview` 以 Docked 模式打开这类 Artifact 时，Preview 占用既有 AI Review
trailing slot 与宽度，并把唯一的应用 Preview host 移入该 slot，只替换可见 AI Surface；
不会挂载第二棵 Markdown Surface。conversation model 保持挂载，PR 内容只
预留一次 slot，既有分隔线控制两种 Surface，PR Dialog 始终位于其上。关闭 Preview
后恢复完整 AI Surface。Detached 模式使用独立原生窗口并让 AI Review 保持可见，绝不
参与 trailing slot 替换。

Closing the AI panel or leaving PR Review clears its response projection,
opaque-action map, and task repository binding. It dismisses the active Preview
only when that Preview has PR-side context and is currently Docked. An already
Detached Preview remains open and owns its prepared payload independently.
Closing only Preview never clears the AI conversation or cancels background
Review/Ask work. Closing the AI panel or leaving the route does cancel the current
Ask response, while the independently scheduled Review task continues.

关闭 AI 面板或离开 PR Review 会清空其回复投影、不透明 action 映射和 task 仓库绑定；
仅当活动 Preview 属于 PR 侧上下文且当前为 Docked 时，才同时关闭 Preview。已经
Detached 的 Preview 继续打开并独立拥有 prepared payload。只关闭 Preview 绝不清空
AI conversation，也不取消后台 Review/Ask 工作。关闭 AI 面板或离开路由会取消当前 Ask
回复，但独立调度的 Review task 继续运行。

## Development fixtures / 开发样例

The opt-in UI Components playground provides Markdown, PNG image, plain-text,
JSON-text, and unsupported-binary fixtures. Its UI emits only a fixture id; a
development-only application adapter constructs `ArtifactSource` values and
submits them through the same `ArtifactPreviewHandle`, registry, worker, and
active host used by production callers. The playground feature includes the
fixture bridge, while normal and Release builds compile neither the fixture
payloads nor their visible controls. A `test-fixtures` build may set
`USHER_ARTIFACT_PREVIEW_FIXTURE` to a fixture id to open that payload through
the production facade at startup for repeatable visual regression capture.

可选的 UI Components 演练场提供 Markdown、PNG 图片、纯文本、JSON 文本和不支持
二进制样例。
UI 只发出 fixture id；仅开发构建存在的应用适配器负责构造 `ArtifactSource`，并通过
生产调用方共用的 `ArtifactPreviewHandle`、Registry、Worker 和当前 Host 打开。
演练场 feature 自动包含 fixture bridge；普通构建和 Release 构建均不编译样例数据，
也不显示对应控件。`test-fixtures` 构建可通过 `USHER_ARTIFACT_PREVIEW_FIXTURE` 指定
fixture id，在启动时经生产 facade 自动打开对应 payload，用于可重复的视觉回归截图。

## State, lifecycle, and concurrency / 状态、生命周期与并发

- Preview mode and provider enable flags are durable application settings.
- The application facade owns one monotonically increasing request generation;
  completion from an older generation is discarded.
- Preparing a new artifact cancels the previous preparation when possible.
- A cold language-loader cache miss reports `ParserLoading` then `ParserReady`;
  a cache hit reports neither. Cancellation before cache insertion reports no
  `ParserReady` and leaves no parser entry.
- The UI progress bridge checks the request generation before and on the Slint
  event loop, so stale progress cannot replace a newer Preview's loading label.
- Worker completion enters Slint only through `slint::invoke_from_event_loop`
  with weak window handles.
- Host close clears the active feature session immediately and clears the visual
  payload/decoded image plus docked virtual-layout caches after the bounded
  docked exit transition.
- The facade retains exactly one prepared session. Replacing or closing it drops
  the previous block model or decoded pixels; there is no multi-entry prepared
  artifact cache.
- Image decoding rejects dimensions or decoded size beyond the documented budget.

## Failure and security behavior / 失败与安全行为

- Unsupported or disabled content shows metadata and an optional system-open action;
  it is not interpreted by a fallback provider.
- Remote URLs embedded in Markdown and images are never fetched automatically.
- File paths are opened only from an explicit application/user action.
- Parser/decode errors are bounded and user-safe; logs never dump full content.
- Disabling the provider used by the active session closes it and clears its cache entries.

## Performance acceptance / 性能验收

The implementation is accepted only with release-build A/B measurements on the
same machine and fixed fixture. Report raw/median values and commands for:

1. release executable size;
2. Chat projection/model construction time for a fixed large Markdown history;
3. repeated 800×600, 1000×700, 1280×860, fullscreen, and exit-fullscreen cycles;
4. settled RSS and macOS physical footprint for Chat, docked Markdown, detached
   Markdown, and after close;
5. proof that width-only resize does not increment the Chat model-commit or
   Markdown-parse counter;
6. close/reopen memory behavior and stale-request cancellation.
7. isolated docked-divider and detached-window width-only frame traces, with at
   least three Release processes per variant and a same-binary A/B switch.
8. isolated Preview wheel-scroll frame traces with the same prepared document,
   including projection-commit count and mounted-row peak; the comparison must
   also exercise the same optimized resize path to detect interaction regressions.

No claim of improvement may be based only on debug builds or subjective visual
inspection. Regressions must be reported rather than averaged away.

AI Review response integration additionally follows the same-binary Release
protocol in `ai-code-review.md`: runtime `baseline|formatted` selection, the
non-persistent `markdown-v1` fixture, at least three isolated processes per cell,
a fixed 1280x860 window and balanced scroll input with an application-consumed
marker, and retained settled RSS, sampled-window RSS maximum, physical-footprint, raw-sample, metadata, log, and
candidate-minus-baseline evidence. Unit instrumentation must also prove that a
structure-only projection does not initialize a code loader, emits no
parser-loading progress, and leaves compact code spans empty; opening the title
may initialize highlighting on the full Preview worker.

AI Review 回复集成还必须遵循 `ai-code-review.md` 的同一 Release 二进制协议：运行时
选择 `baseline|formatted`、使用不持久化的 `markdown-v1` fixture、每个 cell 至少 3 个
隔离进程、固定 1280x860 窗口和带应用消费 marker 的相同往返滚动，并保留稳定 RSS、
采样窗口 RSS 最大值、physical
footprint、原始样本、metadata、日志及 candidate-minus-baseline 证据。单元级插桩还
必须证明纯结构投影不初始化代码 loader、不产生 parser-loading progress，且紧凑代码
span 为空；点击标题后完整 Preview 可以在其 Worker 上初始化并执行高亮。

The dated Release measurements, commands, raw samples, binary-size delta, and
known retained-memory caveat are recorded in
[`docs/artifact-preview-performance.md`](../artifact-preview-performance.md).

带日期的 Release 测量、复现命令、原始样本、二进制体积差值和已知内存残留说明见
[`docs/artifact-preview-performance.md`](../artifact-preview-performance.md)。

## Regression checklist / 回归清单

- Old settings load with Docked and all built-ins enabled.
- Switching Docked <-> Detached while content is open moves it immediately and leaves one host.
- Hover and drag the docked divider at narrow and wide sizes; verify cursor,
  bounded width, cancel restoration, and session-width restoration after a clamp.
- Markdown, PNG/JPEG, and UTF-8 text select their expected providers.
- Runtime disabling prevents provider prepare and closes an active session.
- Chat shows readable content plus a title action using the global selected host.
- Resize Chat repeatedly and verify no width-driven message-model replacement.
- Project an AI response containing fenced code through the structure-only entry;
  verify block/inline semantics and language survive, `code_spans` stay empty,
  and no code-highlighting loader initializes before full Preview opens. /
  通过纯结构入口投影包含围栏代码的 AI 回复；确认 block/inline 语义和语言保留、
  `code_spans` 为空，且打开完整 Preview 前没有代码高亮 loader 初始化。
- Activate safe root/nested repository Markdown actions and reject absolute,
  parent, non-Markdown, oversized, invalid-UTF-8, and symlink-escape targets. /
  激活安全的根目录/嵌套仓库 Markdown action，并拒绝绝对路径、父目录、非 Markdown、
  超限、非 UTF-8 和 symlink 逃逸目标。
- In PR Review, open a response in Docked mode and verify Preview replaces the
  trailing AI surface without a second width reservation or conversation-model
  clear; close Preview and verify the same conversation returns. / 在 PR Review
  以 Docked 模式打开回复；确认 Preview 替换右侧 AI Surface 时不重复预留宽度或清空
  conversation model，关闭 Preview 后同一对话恢复。
- Repeat in Detached mode, then close the AI panel and leave the route. Verify
  the AI action/repository binding clears while the detached prepared Preview
  stays open and the background task remains independent. / 在 Detached 模式
  重复操作后关闭 AI 面板并离开路由；确认 AI action/仓库绑定清空，而 Detached 的
  prepared Preview 保持打开，后台 task 生命周期独立。
- Resize both Preview hosts and verify the block model remains mounted once,
  live bounds remain responsive, and final-width reflow occurs only at release.
- Open and close a docked Preview over Chat; verify both panes animate, Markdown
  and messages do not reflow per frame, and the visual payload clears after exit.
- Drag Chat's scrollbar thumb at its topmost macOS position; it must scroll Chat
  and must not move the native window.
- During Preview resize/transition, verify no stale scrollbar remains inside the
  document and the settled scrollbar returns at the trailing edge.
- On heterogeneous Markdown, record thumb height at top/middle/end and verify it
  is constant; drag to 25/50/75/100 percent and verify normalized document offset
  matches, with document bottom reached only at 100 percent.
- Continuously wheel-scroll a large Markdown document and verify the stable row
  model is reused while the viewport remains inside its exact overscan guard;
  crossing the guard incrementally removes/appends only boundary rows, mounted
  rows remain bounded, and the same optimized divider-resize path does not regress.
- Replace and close a large image and verify decoded pixels are not retained by both hosts.
- Leave Chat while preparation is running and verify hidden Chat is not remounted.
- Run repository validation and the release performance protocol.
