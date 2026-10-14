#!/bin/sh

set -eu

usage() {
    echo "Usage: $0 [--quick|--full|--help]"
    echo "Default/--quick: local gates, root check, library tests and doctests."
    echo "--full: local gates, workspace check/tests/lint, no-AI check and root doctests."
    echo "Checks do not rewrite source; ordinary failures are collected and remain blocking."
}
if [ "$#" -gt 1 ]; then
    usage >&2
    exit 2
fi
mode=quick
case "${1:-}" in
    --help) usage; exit 0 ;;
    ""|--quick) ;;
    --full) mode=full ;;
    *) usage >&2; exit 2 ;;
esac

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

for dependency in cargo rg python3 git; do
    if ! command -v "$dependency" >/dev/null 2>&1; then
        echo "Quality checks require $dependency on PATH." >&2
        exit 2
    fi
done

# The full/default and disabled-feature Slint graphs share a target directory.
# Match manual CI's existing protection against incremental fingerprint reuse.
if [ "$mode" = full ]; then
    CARGO_INCREMENTAL=0
    export CARGO_INCREMENTAL
fi

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

printf '[Quality] Mode: %s\n' "$mode"
run_check fmt cargo fmt --all -- --check
run_check architecture sh scripts/check-architecture-structure.sh
run_check ui-policy sh scripts/check-ui-policy.sh
run_check i18n sh scripts/check-i18n.sh
run_check script-tests python3 -m unittest discover -s scripts/tests -p 'test_*.py'
if [ "$mode" = full ]; then
    run_check workspace-check cargo check --workspace --all-targets --locked
    # Workspace unit/integration tests and root doctests are separate blocking
    # gates so either failure still allows the remaining checks to run.
    run_check workspace-tests env SLINT_EMIT_DEBUG_INFO=1 cargo test --workspace --tests --locked
    run_check workspace-lint cargo clippy --workspace --all-targets --locked -- -D warnings
    # Selecting the root package excludes its optional AI core dependency;
    # --workspace would still select usher-ai-review as a workspace member.
    run_check no-ai-check cargo check -p usher --all-targets --no-default-features --features renderer-femtovg --locked
else
    run_check cargo-check cargo check --locked
    run_check library-tests cargo test --lib --locked
fi
run_check doctests cargo test --doc --locked
run_check diff git diff --check

printf '\n[Quality] %s checks passed.\n' "$passed"
if [ -n "$failures" ]; then
    printf '[Quality] Failed:%s\n' "$failures" >&2
    exit 1
fi
echo "[Quality] All checks passed."
