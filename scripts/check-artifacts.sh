#!/bin/bash

# 快速检查 release 产物脚本
# 用法: ./scripts/check-artifacts.sh [target]

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

APP_NAME="Usher"
OUTPUT_DIR="workflow-dist"

detect_platform() {
    local os
    local arch
    os=$(uname -s)
    arch=$(uname -m)

    if [ "$os" = "Darwin" ]; then
        case "$arch" in
            arm64|aarch64) echo "aarch64-apple-darwin" ;;
            x86_64) echo "x86_64-apple-darwin" ;;
            *) echo "Unknown architecture: $arch" >&2; exit 1 ;;
        esac
    else
        echo "x86_64-pc-windows-gnu"
    fi
}

target_kind() {
    local target="$1"
    case "$target" in
        *apple-darwin) echo "macos" ;;
        *windows-gnu|*windows-msvc) echo "windows" ;;
        *) echo "unsupported" ;;
    esac
}

print_file_summary() {
    local path="$1"
    local usage="$2"
    if [ -f "$path" ]; then
        local size
        size=$(du -h "$path" | cut -f1)
        echo -e "  ${GREEN}✓${NC} $path ($size) - $usage"
        return 0
    fi

    echo -e "  ${RED}✗${NC} $path (未找到)"
    return 1
}

check_macos_target() {
    local target="$1"
    local found=false
    local tar_exe="$OUTPUT_DIR/$APP_NAME-$target.tar.gz"
    local tar_bundle="$OUTPUT_DIR/$APP_NAME-$target-bundle.tar.gz"
    local tar_debug="$OUTPUT_DIR/$APP_NAME-$target-debug.tar.gz"
    local dmg="$OUTPUT_DIR/$APP_NAME-$target.dmg"

    echo -e "${YELLOW}检查 $target (macOS):${NC}"
    print_file_summary "$tar_exe" "用于 self_update" && found=true
    print_file_summary "$tar_bundle" "用于手动安装" && found=true
    print_file_summary "$tar_debug" "用于调试/查看日志" && found=true
    print_file_summary "$dmg" "用于手动安装" && found=true

    if [ -f "$tar_exe" ]; then
        echo "    tar.gz 内容:"
        tar -tzf "$tar_exe" 2>/dev/null | sed -n '1,3p' | sed 's/^/      /' || true
    fi
    echo ""

    [ "$found" = true ]
}

check_windows_target() {
    local target="$1"
    local found=false
    local zip_exe="$OUTPUT_DIR/$APP_NAME-$target.zip"
    local zip_bundle="$OUTPUT_DIR/$APP_NAME-$target-bundle.zip"
    local zip_debug="$OUTPUT_DIR/$APP_NAME-$target-debug.zip"

    echo -e "${YELLOW}检查 $target (Windows):${NC}"
    print_file_summary "$zip_exe" "用于 self_update" && found=true
    print_file_summary "$zip_bundle" "用于手动分发" && found=true
    print_file_summary "$zip_debug" "用于调试/查看日志" && found=true

    if command -v unzip >/dev/null 2>&1 && [ -f "$zip_exe" ]; then
        echo "    zip 内容:"
        unzip -l "$zip_exe" 2>/dev/null | sed -n '1,8p' | sed 's/^/      /' || true
    fi
    echo ""

    [ "$found" = true ]
}

TARGET="${1:-$(detect_platform)}"
TARGETS=("aarch64-apple-darwin" "x86_64-apple-darwin" "x86_64-pc-windows-gnu" "x86_64-pc-windows-msvc")

echo -e "${BLUE}=== 检查 Release 产物 ===${NC}"
echo "Target: $TARGET"
echo "检查目录: $OUTPUT_DIR/"
echo ""

FOUND_ANY=false

for CHECK_TARGET in "${TARGETS[@]}"; do
    case "$(target_kind "$CHECK_TARGET")" in
        macos)
            if check_macos_target "$CHECK_TARGET"; then
                FOUND_ANY=true
            fi
            ;;
        windows)
            if check_windows_target "$CHECK_TARGET"; then
                FOUND_ANY=true
            fi
            ;;
    esac
done

echo -e "${BLUE}=== 当前目标检查 ===${NC}"
case "$(target_kind "$TARGET")" in
    macos)
        if check_macos_target "$TARGET"; then
            echo -e "${GREEN}✓ 当前目标产物已就绪${NC}"
        else
            echo -e "${YELLOW}⚠ 当前目标产物不完整${NC}"
        fi
        ;;
    windows)
        if check_windows_target "$TARGET"; then
            echo -e "${GREEN}✓ 当前目标产物已就绪${NC}"
        else
            echo -e "${YELLOW}⚠ 当前目标产物不完整${NC}"
        fi
        ;;
    *)
        echo -e "${RED}不支持的 target: $TARGET${NC}"
        exit 1
        ;;
esac

echo ""
echo -e "${BLUE}=== 命名规则检查 (self_update 兼容) ===${NC}"
for CHECK_TARGET in "${TARGETS[@]}"; do
    for candidate in \
        "$OUTPUT_DIR/$APP_NAME-$CHECK_TARGET.tar.gz" \
        "$OUTPUT_DIR/$APP_NAME-$CHECK_TARGET-bundle.tar.gz" \
        "$OUTPUT_DIR/$APP_NAME-$CHECK_TARGET-debug.tar.gz" \
        "$OUTPUT_DIR/$APP_NAME-$CHECK_TARGET.zip" \
        "$OUTPUT_DIR/$APP_NAME-$CHECK_TARGET-bundle.zip" \
        "$OUTPUT_DIR/$APP_NAME-$CHECK_TARGET-debug.zip" \
        "$OUTPUT_DIR/$APP_NAME-$CHECK_TARGET.dmg"; do
        if [ -f "$candidate" ]; then
            if [[ "$candidate" == *"$CHECK_TARGET"* ]]; then
                echo -e "${GREEN}✓${NC} $candidate 命名包含 target triple"
            else
                echo -e "${RED}✗${NC} $candidate 命名未包含 target triple"
            fi
        fi
    done
done

echo ""
if [ "$FOUND_ANY" = true ]; then
    echo "如需重新生成，请执行:"
    echo "  ./scripts/verify-release.sh $TARGET"
    exit 0
else
    echo -e "${RED}未找到任何产物文件${NC}"
    exit 1
fi
