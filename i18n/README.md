# Usher translations

The application has one translation source of truth:

- `manifest.toml` registers shipped locales, native display names, aliases,
  and the minimum accepted catalog coverage.
- `messages.toml` owns stable semantic keys and English source text for
  Slint-visible messages. Rust may consume these keys too.
- `rust-messages.toml` owns messages used only at a Rust presentation boundary;
  each sets `slint = false`.
- `locales/<locale>/LC_MESSAGES/usher.po` translates `messages.toml`.
  The adjacent `usher-rust.po` translates `rust-messages.toml`.

To add any number of target languages, add one `[[locales]]` entry and both
independent PO catalogs per language. Source English comes from the registries
and needs no PO. No Settings, routing, Rust enum, or match
statement may change: the manifest is the extension interface and drives the
picker, locale resolution, Rust catalogs, and every Slint bundle. Use a native
language name in the manifest and declare generic aliases only when that locale
is the intentional fallback for the whole language family. English is the
source-copy locale, not a limit on which target languages may be packaged.

The catalog-free `usher-i18n-build` helper owns validation and code generation;
it has no embedded catalog or build script. The `usher-i18n` runtime build uses
it to merge both scopes, reject duplicate keys/property names across scopes,
and enforce the configured coverage threshold over all messages. Each PO must
contain only its own scope's contexts. Misplaced `slint` flags, source-text drift,
and placeholder drift fail validation.

UI build scripts use the helper directly to generate one typed Slint `I18n`
global and stage the same complete Slint-visible catalog for every independently
compiled UI root. Their catalog source watches cover only `manifest.toml`,
`messages.toml`, and registered target locales' `usher.po`; Rust-only registry/PO edits update the runtime
catalog without invalidating these UI units. Manifest or Slint-visible copy
changes still invalidate all roots. Settings language pickers use the helper's
validated locale metadata. All inputs are resolved from the caller's current
checkout, including when Cargo reuses a build-script executable.

The four static UI roots compile through `usher-ui-build`, which preserves real
Slint/resource/environment inputs and excludes their current `OUT_DIR` outputs
from rebuild triggers. The original catalog inputs above remain watched; a
generated translation bundle must not invalidate the next unchanged build.

Do not add `@tr()` outside that generated global: separate Slint compilation
units share one runtime translation bundle
and therefore must keep identical translation indexes.

Business code consumes semantic keys. Translators should only need to edit PO
files; source keys and English text change only when product copy changes.

文案按消费者分成两组：`messages.toml` / `usher.po` 负责 Slint 可见文案（Rust 也可
使用）；`rust-messages.toml` / `usher-rust.po` 负责仅 Rust 使用的文案，必须声明
`slint = false`。新增非源语言时提供两份 PO；源语言英文直接使用 registry，无需 PO。
`usher-i18n-build` 不内嵌 catalog，UI 构建只监听共享 manifest 和 Slint 输入；
运行时 `usher-i18n` 合并两组并统一校验 key、属性名与总覆盖率。每个 UI 根始终编译
同一份完整 Slint catalog；仅 Rust 文案变化不使 UI 失效，共享 manifest 或 Slint
文案变化仍须使全部根失效。完整契约及回归入口见
[国际化规范](../docs/specs/i18n.md)。
四个静态 UI 根通过 `usher-ui-build` 编译，保留真实 Slint/资源/环境输入监听，
排除当前 `OUT_DIR` 生成物；原始 catalog 输入继续监听，生成翻译 bundle 不应再让
下一次无修改构建失效。

## Translation rules

- Keep product and engineering terms in English when translating them. This
  includes GitHub, GitLab, Gitee, Provider, AI Agent, AI Review, Pull Request,
  PR, Review, Commit, Branch, Tag, Diff, SHA, Workflow, Job, Terminal, Git,
  Markdown, Token, API, and URL.
- Placeholders are named (`{version}`, `{count}`, `{code}`), never positional.
  A translation may reorder them, but it must preserve the source placeholder
  names exactly. The catalog build rejects missing, added, or malformed names.
- Do not store a translated `String` in long-lived application state. Store a
  semantic key, a structured domain value, or `src/i18n.rs::LocalizedText`, and
  resolve it only where the current UI projection is built.
- Provider, PTY, filesystem, and protocol details belong in logs. User-visible
  errors use stable catalog copy and must not expose raw technical payloads.
- Existing Chat history can contain application errors rendered by an older
  locale. Its renderer recognizes those messages against every registered
  catalog and re-projects the semantic key in the active locale. This recovery
  rule is for application-authored errors only, never arbitrary provider text.

Production `.slint` copy must use the generated `I18n` global. The
`scripts/check-i18n.sh` gate rejects handwritten `@tr()` calls and literal
user-facing text outside its narrow list of intentional brands, compact labels,
environment identifiers, and measurement/example sentinels. Showcase and
fixture copy is outside the production hardcoding gate.

The optional static UI Components factory uses the same full ordered UI bundle.
Its build script also excludes Rust-only registry/PO inputs. 可选静态演练场同样使用
完整有序 UI bundle，构建脚本不读取或监听 Rust-only registry/PO。
