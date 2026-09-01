#!/bin/sh

set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

# All translation references are generated from the structured message
# manifest. Handwritten @tr calls would give independently compiled Slint roots
# different bundle indexes, so reject them at the source boundary.
direct_translations=$(rg -n '@tr[[:space:]]*\(' ui --glob '*.slint' || true)
if [ -n "$direct_translations" ]; then
    echo "i18n policy: handwritten @tr() calls are forbidden; add semantic keys to i18n/messages.toml:" >&2
    echo "$direct_translations" >&2
    exit 1
fi

# Production Slint copy must come from the generated I18n global. Keep the
# allowlist deliberately narrow: brands, model names, compact avatar initials,
# environment identifiers, Git terms, and the hidden font-measurement sentinel.
hardcoded_ui=$(rg -n \
    '^[[:space:]]*(text|tooltip-text|placeholder-text|accessible-label|accessible-description):[[:space:]]*"[^"\n]*[A-Za-z][^"\n]*"' \
    ui \
    --glob '*.slint' \
    --glob '!**/showcase/**' \
    --glob '!**/fixtures/**' || true)
hardcoded_ui=$(printf '%s\n' "$hardcoded_ui" | rg -v \
    ':[[:space:]]*(text|tooltip-text|placeholder-text|accessible-label|accessible-description):[[:space:]]*"(Gitee|GitHub|GitLab|DeepSeek|OpenAI|Usher|Ag|U|AI|MMMMMMMMMMMMMMMM|/absolute/path/to/review-rules\.md)";|:[[:space:]]*text:[[:space:]]*"(Commit |FAT |UAT |PROD |v)"[[:space:]]*\+' || true)
if [ -n "$hardcoded_ui" ]; then
    echo "i18n policy: production Slint user copy must use the generated I18n catalog:" >&2
    echo "$hardcoded_ui" >&2
    exit 1
fi

# The crate build validates the locale registry, aliases, message keys, stale
# PO entries, source-text drift, and each locale's configured coverage floor.
cargo check -p usher-i18n --locked

echo "i18n catalog checks passed."
