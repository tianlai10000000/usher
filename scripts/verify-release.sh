#!/bin/bash

# 本地验证 release 产物脚本
# 用法:
#   ./scripts/verify-release.sh [target]
# 示例:
#   ./scripts/verify-release.sh x86_64-pc-windows-gnu

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

APP_NAME="Usher"
CARGO_BUNDLE_VERSION="0.12.0"
CREATE_DMG_VERSION="1.3.0"
OUTPUT_DIR="workflow-dist"

mkdir -p "$OUTPUT_DIR"

require_cmd() {
    local cmd="$1"
    command -v "$cmd" >/dev/null 2>&1 || {
        echo -e "${RED}错误: 缺少命令 $cmd${NC}" >&2
        exit 1
    }
}

detect_platform() {
    local os
    local arch
    os=$(uname -s)
    arch=$(uname -m)

    if [ "$os" = "Darwin" ]; then
        case "$arch" in
            arm64|aarch64) echo "aarch64-apple-darwin" ;;
            x86_64) echo "x86_64-apple-darwin" ;;
            *)
                echo "Unknown architecture: $arch" >&2
                exit 1
                ;;
        esac
    else
        echo "无法自动推断目标平台，请显式传入 target，例如 x86_64-pc-windows-gnu" >&2
        exit 1
    fi
}

target_kind() {
    local target="$1"
    case "$target" in
        *apple-darwin) echo "macos" ;;
        *windows-gnu|*windows-msvc) echo "windows" ;;
        *)
            echo "unsupported"
            ;;
    esac
}

collect_mingw_runtime_dlls() {
    local target="$1"
    local output_dir="$2"

    if [[ "$target" != *windows-gnu ]]; then
        return 0
    fi

    if ! command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
        echo -e "${YELLOW}⚠ 未找到 x86_64-w64-mingw32-gcc，无法自动收集 MinGW 运行时 DLL${NC}"
        return 0
    fi

    local dll
    local dll_path
    for dll in libgcc_s_seh-1.dll libstdc++-6.dll libwinpthread-1.dll; do
        dll_path=$(x86_64-w64-mingw32-gcc -print-file-name="$dll")
        if [ -n "$dll_path" ] && [ -f "$dll_path" ]; then
            cp "$dll_path" "$output_dir/"
            echo "  + 打包依赖: $dll"
        else
            echo -e "${YELLOW}  ⚠ 未找到依赖: $dll${NC}"
        fi
    done
}

verify_created_files() {
    local all_ok=true
    local file
    echo ""
    echo "=== 构建产物列表 ==="
    for file in "$@"; do
        if [ -f "$file" ]; then
            local size
            size=$(du -h "$file" | cut -f1)
            echo -e "${GREEN}✓${NC} $file ($size)"
        else
            echo -e "${RED}✗${NC} $file (未找到)"
            all_ok=false
        fi
    done

    if [ "$all_ok" != true ]; then
        echo ""
        echo -e "${RED}=== 验证失败！部分产物缺失 ===${NC}"
        exit 1
    fi
}

build_macos() {
    local target="$1"
    local exe_path="target/$target/release/usher"
    local bundle_path="target/$target/release/bundle/osx/$APP_NAME.app"
    local tar_exe="$OUTPUT_DIR/$APP_NAME-$target.tar.gz"
    local tar_bundle="$OUTPUT_DIR/$APP_NAME-$target-bundle.tar.gz"
    local tar_debug="$OUTPUT_DIR/$APP_NAME-$target-debug.tar.gz"
    local dmg_name="$OUTPUT_DIR/$APP_NAME-$target.dmg"
    local has_create_dmg=false

    require_cmd cargo
    require_cmd tar
    require_cmd hdiutil

    if [ "$(cargo-bundle --version 2>/dev/null || true)" != "cargo-bundle v${CARGO_BUNDLE_VERSION}" ]; then
        echo -e "${YELLOW}安装 cargo-bundle v${CARGO_BUNDLE_VERSION}...${NC}"
        cargo install cargo-bundle --version "$CARGO_BUNDLE_VERSION" --locked
    fi

    if [ "$(create-dmg --version 2>/dev/null || true)" = "create-dmg ${CREATE_DMG_VERSION}" ]; then
        has_create_dmg=true
        echo "✓ create-dmg ${CREATE_DMG_VERSION} 已安装"
    else
        echo "⚠ 未找到 create-dmg ${CREATE_DMG_VERSION}，将使用 hdiutil 作为备选"
    fi

    echo -e "${GREEN}[1/5] 构建 Release...${NC}"
    export CARGO_INCREMENTAL=0
    export CARGO_NET_GIT_FETCH_WITH_CLI=true
    cargo build --release --target "$target" --jobs "$(sysctl -n hw.ncpu 2>/dev/null || echo 4)"
    [ -f "$exe_path" ] || { echo -e "${RED}错误: 未找到可执行文件 $exe_path${NC}" >&2; exit 1; }

    echo -e "${GREEN}[2/5] 创建 App Bundle...${NC}"
    cargo bundle --release --format osx --target "$target"
    [ -d "$bundle_path" ] || { echo -e "${RED}错误: 未找到 App Bundle $bundle_path${NC}" >&2; exit 1; }

    echo -e "${GREEN}[3/5] 打包 tar.gz...${NC}"
    rm -f "$tar_exe" "$tar_bundle" "$tar_debug"
    tar -czf "$tar_exe" -C "target/$target/release" usher
    tar -czf "$tar_bundle" -C "target/$target/release/bundle/osx" "$APP_NAME.app"
    tar -czf "$tar_debug" -C "target/$target/release" usher

    echo -e "${GREEN}[4/5] 创建 DMG...${NC}"
    rm -f "$dmg_name"
    if [ "$has_create_dmg" = true ]; then
        create-dmg \
            --volname "$APP_NAME" \
            --volicon "ui/slint-logo.svg" \
            --window-pos 200 120 \
            --window-size 600 400 \
            --icon-size 100 \
            --icon "$APP_NAME.app" 175 178 \
            --hide-extension "$APP_NAME.app" \
            --app-drop-link 425 178 \
            --no-internet-enable \
            "$dmg_name" \
            "target/$target/release/bundle/osx/" \
        || {
            echo -e "${YELLOW}create-dmg 失败，回退到 hdiutil${NC}"
            hdiutil create -volname "$APP_NAME" -srcfolder "$bundle_path" -ov -format UDZO "$dmg_name"
        }
    else
        hdiutil create -volname "$APP_NAME" -srcfolder "$bundle_path" -ov -format UDZO "$dmg_name"
    fi

    echo -e "${GREEN}[5/5] 验证产物...${NC}"
    verify_created_files "$tar_exe" "$tar_bundle" "$tar_debug" "$dmg_name"
    echo ""
    echo "可执行文件 tar.gz 内容:"
    tar -tzf "$tar_exe" | head -3
    echo ""
    echo "Bundle tar.gz 内容 (前10项):"
    tar -tzf "$tar_bundle" | head -10
    echo ""
    echo "Debug tar.gz 内容:"
    tar -tzf "$tar_debug" | head -3
}

build_windows() {
    local target="$1"
    local exe_path="target/$target/release/usher.exe"
    local self_zip="$OUTPUT_DIR/$APP_NAME-$target.zip"
    local bundle_zip="$OUTPUT_DIR/$APP_NAME-$target-bundle.zip"
    local debug_zip="$OUTPUT_DIR/$APP_NAME-$target-debug.zip"
    local package_root
    package_root="$(mktemp -d)"
    local bundle_dir="$package_root/manual-bundle"

    require_cmd cargo
    require_cmd zip
    require_cmd unzip

    echo -e "${GREEN}[1/3] 构建 Windows Release...${NC}"
    export CARGO_INCREMENTAL=0
    export CARGO_NET_GIT_FETCH_WITH_CLI=true
    cargo build --release --target "$target"
    [ -f "$exe_path" ] || { echo -e "${RED}错误: 未找到可执行文件 $exe_path${NC}" >&2; exit 1; }

    echo -e "${GREEN}[2/3] 打包 Windows 产物...${NC}"
    rm -f "$self_zip" "$bundle_zip" "$debug_zip"

    zip -j "$self_zip" "$exe_path" >/dev/null
    zip -j "$debug_zip" "$exe_path" >/dev/null

    mkdir -p "$bundle_dir"
    cp "$exe_path" "$bundle_dir/"
    [ -f "logo.png" ] && cp "logo.png" "$bundle_dir/"
    collect_mingw_runtime_dlls "$target" "$bundle_dir"
    cat > "$bundle_dir/README.txt" <<'EOF'
Usher Windows Bundle
====================
1) 直接运行 usher.exe
2) 如果是 GNU 目标，保持 DLL 与 usher.exe 在同一目录
3) 若托盘图标异常，请确认 logo.png 存在
EOF
    (cd "$bundle_dir" && zip -r "$OLDPWD/$bundle_zip" . >/dev/null)

    echo -e "${GREEN}[3/3] 验证产物...${NC}"
    verify_created_files "$self_zip" "$bundle_zip" "$debug_zip"
    echo ""
    echo "Self-update zip 内容:"
    unzip -l "$self_zip" | sed -n '1,8p'
    echo ""
    echo "Bundle zip 内容:"
    unzip -l "$bundle_zip" | sed -n '1,20p'

    rm -rf "$package_root"
}

TARGET="${1:-$(detect_platform)}"
KIND=$(target_kind "$TARGET")

echo -e "${BLUE}=== 验证 Release 构建产物 ===${NC}"
echo "Target: $TARGET"
echo "App Name: $APP_NAME"
echo "Output: $OUTPUT_DIR/"
echo ""

case "$KIND" in
    macos)
        build_macos "$TARGET"
        ;;
    windows)
        build_windows "$TARGET"
        ;;
    *)
        echo -e "${RED}错误: 暂不支持 target $TARGET${NC}" >&2
        exit 1
        ;;
esac

echo ""
echo -e "${GREEN}=== 验证通过！产物已生成到 $OUTPUT_DIR/ ===${NC}"
