#!/bin/bash

# Windows 构建入口脚本
# 用法:
#   ./scripts/build-windows.sh [target]
# 默认 target: x86_64-pc-windows-gnu

set -euo pipefail

TARGET="${1:-x86_64-pc-windows-gnu}"

case "$TARGET" in
    *windows-gnu|*windows-msvc)
        ;;
    *)
        echo "错误: 仅支持 Windows target，当前为 $TARGET" >&2
        echo "示例: ./scripts/build-windows.sh x86_64-pc-windows-gnu" >&2
        exit 1
        ;;
esac

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_ROOT"
./scripts/verify-release.sh "$TARGET"
