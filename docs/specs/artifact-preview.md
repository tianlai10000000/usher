# Artifact Preview Framework / 文件预览框架

## Scope / 范围

This specification defines the reusable artifact-preview framework used by Chat,
Workflow, Project, and future modules. It covers provider discovery, parsing,
presentation hosts, persistent settings, immediate host migration, lifecycle,
security, and performance acceptance. Markdown is one provider; it is not the
owner of the framework.

本文定义供 Chat、Workflow、Project 及后续模块复用的文件预览框架，涵盖 Provider
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

1. 调用方只提交领域无关 Artifact，不依赖具体 Renderer 或窗口实现。
2. 文件解释方式与展示位置是两条互相独立的策略轴。
3. Provider 独立注册，可在运行期启停，也可在构建期排除；调用方和 Host 无需改变。
4. 只挂载当前活动 Provider/Session；关闭或替换预览时释放大模型和解码像素。
5. 解析、解码和文件 I/O 不得在 Slint event loop 执行。
6. Chat 可用宽度变化不得触发 Markdown 重解析或仅因 resize 重建消息 model。

## Ownership / 职责边界

| Owner | Responsibility |
| --- | --- |
| `crates/usher-preview/` | Framework-neutral artifact descriptors, provider trait, registry/policy, prepared payloads, and built-in Markdown/image/plain-text providers. It must not depend on Chat, `MainWindow`, or Slint. |
| `src/artifact_preview/` | Application facade, worker dispatch, stale-request cancellation, bounded prepared-session ownership, Slint model adaptation, clipboard/external-open actions, and host switching. |
| `ui/components/artifact_preview/` | Shared Primer preview shell, Markdown/text/image surfaces, docked panel, and detached native window. It performs no I/O or parsing. |
| `ui/main.slint` | Root placement of the docked host and the stable Rust-facing preview contract. |
| `src/settings.rs`, `src/settings_page.rs` | Persistent preference and explicit bridge into the independently generated Settings page. |
| Feature callers | Construct `ArtifactSource` and call the facade. They do not select providers or hosts. |

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
Detached 使用同进程的可复用 Slint 原生窗口，不创建浏览器进程。设置变化立即迁移当前
payload，目标 Host 显示后卸载旧 Host，一个 event-loop turn 后不得同时保留两棵重 UI 树。

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
shaping. The shared Preview surface maps
those semantics to official GitHub Primer typography, spacing, colors, heading
dividers, transparent quotes with a 4 px leading rule, table separators, and
bordered monospace code blocks. Theme changes select the already-prepared token
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
共享 Preview Surface 使用 GitHub Primer 的字体层级、间距、语义色、
标题分隔、透明底加 4px 左侧强调线的引用块、表格分隔和带边框等宽代码块。主题切换只
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
