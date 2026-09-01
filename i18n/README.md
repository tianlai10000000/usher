# Usher translations

The application has one translation source of truth:

- `manifest.toml` registers shipped locales, native display names, aliases,
  and the minimum accepted catalog coverage.
- `messages.toml` owns stable semantic keys and English source text.
- Messages used only at a Rust presentation boundary set `slint = false`.
  They remain translated and coverage-checked but are not emitted into `I18n`.
- `locales/<locale>/LC_MESSAGES/usher.po` contains only translations.

To add any number of languages, add one `[[locales]]` entry and one independent
`usher.po` catalog per language. No Settings, routing, Rust enum, or match
statement may change: the manifest is the extension interface and drives the
picker, locale resolution, Rust catalogs, and every Slint bundle. Use a native
language name in the manifest and declare generic aliases only when that locale
is the intentional fallback for the whole language family. English is the
source-copy locale, not a limit on which target languages may be packaged.

The `usher-i18n` build step validates duplicate keys, locale aliases, stale PO
contexts, source-text drift, and the configured coverage threshold. It also
generates one typed Slint `I18n` global and stages the same catalog for every
independently compiled UI crate. Do not add `@tr()` outside that generated
global: separate Slint compilation units share one runtime translation bundle
and therefore must keep identical translation indexes.

Business code consumes semantic keys. Translators should only need to edit PO
files; source keys and English text change only when product copy changes.

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
