#!/bin/sh

set -eu

case "${1:-}" in
    --help)
        echo "Usage: $0"
        echo "Run local quality checks without rewriting source; report all failures."
        exit 0
        ;;
    "") ;;
    *) echo "Usage: $0" >&2; exit 2 ;;
esac
if [ "$#" -gt 1 ]; then
    echo "Usage: $0" >&2
    exit 2
fi

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

for dependency in cargo rg python3 git; do
    if ! command -v "$dependency" >/dev/null 2>&1; then
        echo "Quality checks require $dependency on PATH." >&2
        exit 2
    fi
done

trap 'exit 130' INT
trap 'exit 143' TERM
failures=""
passed=0

run_check() {
    label=$1
    shift
    printf '\n[Quality] %s\n' "$label"
    if "$@"; then
        passed=$((passed + 1))
        printf '[PASS] %s\n' "$label"
    else
        result=$?
        # Interrupts must stop the run, not launch the next expensive check.
        case "$result" in
            130|143) exit "$result" ;;
        esac
        failures="${failures} ${label}(${result})"
        printf '[FAIL] %s (exit %s)\n' "$label" "$result" >&2
    fi
}

run_check fmt cargo fmt --all -- --check
run_check architecture sh scripts/check-architecture-structure.sh
run_check ui-policy sh scripts/check-ui-policy.sh
run_check i18n sh scripts/check-i18n.sh
run_check script-tests python3 -m unittest discover -s scripts/tests -p 'test_*.py'
run_check cargo-check cargo check --locked
run_check library-tests cargo test --lib --locked
run_check diff git diff --check

printf '\n[Quality] %s checks passed.\n' "$passed"
if [ -n "$failures" ]; then
    printf '[Quality] Failed:%s\n' "$failures" >&2
    exit 1
fi
echo "[Quality] All checks passed."
