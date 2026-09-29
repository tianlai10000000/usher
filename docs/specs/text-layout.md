# Multiline Text Layout / 多行文本排版

## Scope / 范围

This contract applies whenever Usher changes multiline native Slint text,
editable wrapped text, rich text, or the measurements that determine their
preferred height and virtual extent. It covers native `Text`, direct built-in
`TextInput`, `StyledText`, the standard `TextEdit` wrapper, and feature-owned
worker/layout adapters.

本文适用于 Usher 中所有多行原生 Slint 文本、可编辑换行文本、富文本，以及决定其
preferred height 和虚拟 extent 的测量逻辑。覆盖原生 `Text`、直接使用的内建
`TextInput`、`StyledText`、标准 `TextEdit` 包装与 Feature 自有 Worker/布局适配器。

Terminal cell grids and intentionally fixed diff/log rows may keep their domain
pitch. A real fixed-row contract is not a line-spacing workaround, but its pitch
must remain an explicit paint and measurement input; padding must not stand in
for that pitch.

Terminal 单元格网格与有意固定的 Diff/日志行可以保留领域行距。真实的固定行契约不是
伪造行距，但其 pitch 必须作为明确的绘制与测量输入，不能用 padding 代替。

## Slint 1.18 support boundary / Slint 1.18 支持边界

The repository codes to the compiler/runtime surface it actually builds, not to
a release-summary claim. For the Slint 1.18 upgrade, the verified boundary is:

本仓库以实际编译和运行的组件接口为准，不能把 Release 摘要视为 API 证明。Slint 1.18
升级采用以下已验证边界：

| Surface / 组件 | 1.18 contract / 1.18 契约 | Required use / 使用要求 |
| --- | --- | --- |
| Native `Text` / 原生 `Text` | Exposes `line-height-factor` / 暴露 `line-height-factor` | Use it as the primary multiline spacing control / 作为多行行距的首选控制 |
| Direct built-in `TextInput` / 直接内建 `TextInput` | Exposes `line-height-factor` / 暴露 `line-height-factor` | Use it when changing wrapped editable line spacing; verify caret and selection / 修改可编辑换行行距时使用，并验证光标与选区 |
| `StyledText` | Does not expose the property in the 1.18 surface used here / 本仓库使用的 1.18 接口未暴露该属性 | Keep an explicit, documented fallback; do not silently pretend the property applies / 保留明确记录的 fallback，禁止假定属性已生效 |
| Standard `TextEdit` wrapper / 标准 `TextEdit` 包装 | Does not expose or forward the property in the 1.18 wrapper / 1.18 包装未暴露或转发该属性 | Record an exception or use direct `TextInput` only when editor semantics and tests permit / 记录例外；仅在编辑语义与测试允许时改用直接 `TextInput` |

`line-height-factor` is a unitless multiplier of the font's natural line height.
It is not an absolute pixel line height and must not be converted into a promised
pixel pitch from `font-size` alone. Verify the resulting layout on the active
font collection, renderer and scale factor.

`line-height-factor` 是字体自然行高的无单位倍率，不是绝对像素行高；禁止仅凭
`font-size` 换算并承诺固定像素 pitch。必须在实际字体 collection、renderer 与缩放
倍率下验证最终布局。

Do not replace `StyledText` with `Text`, or `TextEdit` with direct `TextInput`,
solely to gain this property. Rich runs, cursor/selection behavior, scrolling and
accessibility remain part of the component contract.

不得仅为获得该属性就把 `StyledText` 换成 `Text`，或把 `TextEdit` 换成直接
`TextInput`；富文本 run、光标/选区、滚动和可访问性都属于组件契约。

## Invariants / 不变量

1. New or changed multiline native `Text` and direct `TextInput` use
   `line-height-factor` as the primary line-spacing control when the pinned Slint
   API supports it.
2. Never simulate inter-line leading by increasing top/bottom padding, injecting
   extra newline rows, inserting invisible glyphs, duplicating overlay text, or
   fixing/clipping a wrapper height. Padding remains valid for real component
   chrome and fixed containers remain valid for genuine row/viewport contracts;
   neither may be introduced to fake spacing between visual text lines.
3. One layout contract owns font family, size, weight, letter spacing,
   `line-height-factor` or documented fallback pitch, final text width, wrap
   mode, content padding/chrome, glyph safety and line-box rounding.
4. Worker shaping, Slint preferred height, caret/selection/hit testing and exact
   virtual extents must consume that same contract. Do not change only the paint
   factor while retaining old worker extents or source-line estimates.
5. A factor, fallback pitch, font, scale or wrapping change is a layout-key
   change. Cancel stale work, recompute all affected extents and reject results
   prepared under the old key.
6. Visually continuous inline prose that shares one style and baseline must be
   painted by one native text shaping node. Do not split runs merely to attach
   an interaction to a trailing substring; keep one paint node and place a
   measured transparent interaction overlay over that substring instead.

1. 新增或修改多行原生 `Text` 与直接 `TextInput` 时，只要当前固定的 Slint API
   支持，就以 `line-height-factor` 作为首选行距控制。
2. 禁止通过增加上下 padding、注入额外换行、插入不可见字符、叠加重复文本，或固定/
   裁剪外层高度来伪造行间距。Padding 可用于真实组件 chrome，固定容器可用于真实的
   行/视口契约，但两者都不能为了制造视觉文本行距而新增。
3. 字体族、字号、字重、字符间距、`line-height-factor` 或已记录的 fallback pitch、
   最终文本宽度、wrap mode、内容 padding/chrome、glyph 安全余量和 line-box rounding
   必须由同一布局契约统一管理。
4. Worker shaping、Slint preferred height、光标/选区/命中测试及精确虚拟 extent 必须
   使用同一契约。禁止只改绘制 factor，却继续使用旧 Worker extent 或源行数估算。
5. Factor、fallback pitch、字体、缩放或换行变化都属于 layout-key 变化；必须取消旧
   工作、重算全部受影响 extent，并拒绝旧 key 结果。
6. 视觉上连续、共享同一样式与基线的行内文案必须由同一个原生文本 shaping 节点
   绘制。不得仅为给尾部子串添加交互而拆成多个绘制节点；应保留单一绘制节点，并在
   该子串上放置实测的透明交互 overlay。

## Unsupported-surface exceptions / 暂不支持组件的例外

Every multiline `StyledText` or `TextEdit` use that needs non-default line
spacing must record an exception in its feature spec or the relevant architecture
section. The record names the component/path, the semantic reason it cannot move
to a supported primitive, the fallback metric and its single source of truth,
the worker/virtual-extent consumers that must match it, and the condition for
removing the exception.

每个需要非默认行距的多行 `StyledText` 或 `TextEdit` 使用点，都必须在 Feature spec
或相关架构章节记录例外。记录应包含组件/路径、不能迁移到已支持 primitive 的语义原因、
fallback 指标及其唯一事实源、必须与其一致的 Worker/虚拟 extent 使用方，以及移除例外
的条件。

Artifact Preview and Agent Response rich prose are current `StyledText`
surfaces. Their explicit typography and worker measurement remain the supported
fallback until the pinned Slint component compiles and behaves correctly with a
native line-height property. Other call sites must not infer support from those
exceptions.

Artifact Preview 与 Agent Response 富文本是当前 `StyledText` Surface。固定版本
Slint 的对应组件能够以原生行高属性正确编译和运行之前，其显式排版和 Worker 测量仍是
受支持的 fallback；其他调用点不能从这些例外推断已获得支持。

On every Slint upgrade, compile the real component surface and run its behavior
checks before removing an exception. A changelog entry or a property on another
primitive is not sufficient evidence.

每次升级 Slint 都必须实际编译对应组件接口并完成行为验证后才能移除例外；Changelog
条目或其他 primitive 上存在同名属性都不是充分证据。

## Acceptance / 验收

- Exercise mixed CJK, Latin, emoji, long unbroken tokens, explicit blank lines
  and a trailing newline at narrow and wide widths and at supported scale factors.
- Validate the source locale and at least one packaged non-source locale; locale
  changes must not leave stale projected copy or stale extents.
- For direct `TextInput`, verify caret placement, selection, hit testing,
  scrolling and preferred height at the first, middle and final visual lines.
- For variable-height documents, verify stable document/thumb extent during
  ordinary scrolling, exact narrow/wide reflow and reachability of the final line.
- For a `StyledText`/`TextEdit` exception, verify its documented fallback and
  paint/measurement agreement; do not report native line-height support.

- 在窄/宽布局及受支持缩放倍率下，覆盖 CJK、拉丁文、Emoji、连续长 token、显式空行与
  末尾换行。
- 验证源语言及至少一种内置非源语言；切换语言后不得残留旧投影文案或旧 extent。
- 直接 `TextInput` 验证首行、中间行、末行的光标、选区、命中、滚动和 preferred height。
- 可变行高文档验证普通滚动期间文档/滑块 extent 稳定、窄宽重排精确且末行可达。
- `StyledText`/`TextEdit` 例外验证已记录 fallback 与绘制/测量一致；不得宣称原生行高支持。

Variable-height surfaces additionally follow
[Exact virtual scrolling](virtual-scrolling.md). User-facing examples and error
states additionally follow [Internationalization](i18n.md).

可变行高 Surface 还必须遵循[精确虚拟滚动](virtual-scrolling.md)；用户可见示例和错误
状态还必须遵循[国际化规范](i18n.md)。
