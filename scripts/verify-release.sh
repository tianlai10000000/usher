#!/bin/bash

# 本地验证 release.yml 输出产物的脚本
# 用法: ./scripts/verify-release.sh [target]
# 如果不指定 target，会自动检测当前平台

set -e  # 遇到错误立即退出

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 配置
APP_NAME="Usher"
OUTPUT_DIR="workflow-dist"

# 创建输出目录
mkdir -p "$OUTPUT_DIR"

# 检测当前平台
detect_platform() {
    local arch=$(uname -m)
    case "$arch" in
        arm64|aarch64)
            echo "aarch64-apple-darwin"
            ;;
        x86_64)
            echo "x86_64-apple-darwin"
            ;;
        *)
            echo "Unknown architecture: $arch" >&2
            exit 1
            ;;
    esac
}

# 获取 target
if [ -n "$1" ]; then
    TARGET="$1"
else
    TARGET=$(detect_platform)
fi

echo -e "${GREEN}=== 验证 Release 构建产物 ===${NC}"
echo "Target: $TARGET"
echo "App Name: $APP_NAME"
echo ""

# 检查必要的工具
echo -e "${YELLOW}检查必要的工具...${NC}"
command -v cargo >/dev/null 2>&1 || { echo -e "${RED}错误: cargo 未安装${NC}" >&2; exit 1; }
command -v tar >/dev/null 2>&1 || { echo -e "${RED}错误: tar 未安装${NC}" >&2; exit 1; }
command -v hdiutil >/dev/null 2>&1 || { echo -e "${RED}错误: hdiutil 未安装${NC}" >&2; exit 1; }

# 检查 cargo-bundle
if ! command -v cargo-bundle &> /dev/null; then
    echo -e "${YELLOW}安装 cargo-bundle...${NC}"
    cargo install cargo-bundle
fi

# 检查 create-dmg (可选)
HAS_CREATE_DMG=false
if command -v create-dmg &> /dev/null; then
    HAS_CREATE_DMG=true
    echo "✓ create-dmg 已安装"
else
    echo "⚠ create-dmg 未安装，将使用 hdiutil 作为备选"
fi

echo ""

# 步骤 1: 构建 Release
echo -e "${GREEN}[1/5] 构建 Release 版本...${NC}"
export CARGO_INCREMENTAL=0
export CARGO_NET_GIT_FETCH_WITH_CLI=true
cargo build --release --target "$TARGET" --jobs $(sysctl -n hw.ncpu 2>/dev/null || echo 4)

EXE_PATH="target/$TARGET/release/usher"
if [ ! -f "$EXE_PATH" ]; then
    echo -e "${RED}错误: 可执行文件未找到: $EXE_PATH${NC}" >&2
    exit 1
fi
echo -e "${GREEN}✓ 构建成功: $EXE_PATH ($(du -h "$EXE_PATH" | cut -f1))${NC}"
echo ""

# 步骤 2: 创建 App Bundle
echo -e "${GREEN}[2/5] 创建 App Bundle...${NC}"
cargo bundle --release --target "$TARGET"

BUNDLE_PATH="target/$TARGET/release/bundle/osx/$APP_NAME.app"
if [ ! -d "$BUNDLE_PATH" ]; then
    echo -e "${RED}错误: App Bundle 未找到: $BUNDLE_PATH${NC}" >&2
    exit 1
fi
echo -e "${GREEN}✓ App Bundle 创建成功: $BUNDLE_PATH${NC}"
echo ""

# 步骤 3: 创建 tar.gz 文件
echo -e "${GREEN}[3/5] 创建 tar.gz 文件...${NC}"

# 3a. 创建只包含可执行文件的 tar.gz（用于 self_update）
TAR_EXE_NAME="$OUTPUT_DIR/$APP_NAME-$TARGET.tar.gz"
[ -f "$TAR_EXE_NAME" ] && rm "$TAR_EXE_NAME"

tar -czf "$TAR_EXE_NAME" -C "target/$TARGET/release" usher

if [ ! -f "$TAR_EXE_NAME" ]; then
    echo -e "${RED}错误: tar.gz 文件创建失败${NC}" >&2
    exit 1
fi

TAR_EXE_SIZE=$(du -h "$TAR_EXE_NAME" | cut -f1)
echo -e "${GREEN}✓ tar.gz (可执行文件) 创建成功: $TAR_EXE_NAME ($TAR_EXE_SIZE)${NC}"
echo "  用途: self_update 自动更新"
echo "  内容:"
tar -tzf "$TAR_EXE_NAME" | head -3
echo ""

# 3b. 创建包含整个 .app bundle 的 tar.gz（用于手动安装）
TAR_BUNDLE_NAME="$OUTPUT_DIR/$APP_NAME-$TARGET-bundle.tar.gz"
[ -f "$TAR_BUNDLE_NAME" ] && rm "$TAR_BUNDLE_NAME"

tar -czf "$TAR_BUNDLE_NAME" -C "target/$TARGET/release/bundle/osx" "$APP_NAME.app"

if [ ! -f "$TAR_BUNDLE_NAME" ]; then
    echo -e "${RED}错误: bundle tar.gz 文件创建失败${NC}" >&2
    exit 1
fi

TAR_BUNDLE_SIZE=$(du -h "$TAR_BUNDLE_NAME" | cut -f1)
echo -e "${GREEN}✓ tar.gz (bundle) 创建成功: $TAR_BUNDLE_NAME ($TAR_BUNDLE_SIZE)${NC}"
echo "  用途: 手动安装（解压后可直接使用）"
echo "  内容 (前10项):"
tar -tzf "$TAR_BUNDLE_NAME" | head -10
echo ""

# 3c. 创建只包含可执行文件的 tar.gz（用于调试/查看日志）
TAR_DEBUG_NAME="$OUTPUT_DIR/$APP_NAME-$TARGET-debug.tar.gz"
[ -f "$TAR_DEBUG_NAME" ] && rm "$TAR_DEBUG_NAME"

tar -czf "$TAR_DEBUG_NAME" -C "target/$TARGET/release" usher

if [ ! -f "$TAR_DEBUG_NAME" ]; then
    echo -e "${RED}错误: debug tar.gz 文件创建失败${NC}" >&2
    exit 1
fi

TAR_DEBUG_SIZE=$(du -h "$TAR_DEBUG_NAME" | cut -f1)
echo -e "${GREEN}✓ tar.gz (debug) 创建成功: $TAR_DEBUG_NAME ($TAR_DEBUG_SIZE)${NC}"
echo "  用途: 调试/查看日志（直接运行会显示终端窗口）"
echo "  内容:"
tar -tzf "$TAR_DEBUG_NAME" | head -3
echo ""

# 步骤 4: 创建 DMG
echo -e "${GREEN}[4/5] 创建 DMG 文件...${NC}"
DMG_NAME="$OUTPUT_DIR/$APP_NAME-$TARGET.dmg"

# 清理旧文件
[ -f "$DMG_NAME" ] && rm "$DMG_NAME"

if [ "$HAS_CREATE_DMG" = true ]; then
    echo "使用 create-dmg 创建 DMG..."
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
        "$DMG_NAME" \
        "target/$TARGET/release/bundle/osx/" \
    || {
        echo -e "${YELLOW}create-dmg 失败，使用 hdiutil...${NC}"
        hdiutil create -volname "$APP_NAME" \
            -srcfolder "$BUNDLE_PATH" \
            -ov -format UDZO \
            "$DMG_NAME"
    }
else
    echo "使用 hdiutil 创建 DMG..."
    hdiutil create -volname "$APP_NAME" \
        -srcfolder "$BUNDLE_PATH" \
        -ov -format UDZO \
        "$DMG_NAME"
fi

if [ ! -f "$DMG_NAME" ]; then
    echo -e "${RED}错误: DMG 文件创建失败${NC}" >&2
    exit 1
fi

DMG_SIZE=$(du -h "$DMG_NAME" | cut -f1)
echo -e "${GREEN}✓ DMG 创建成功: $DMG_NAME ($DMG_SIZE)${NC}"
echo ""

# 步骤 5: 验证所有产物
echo -e "${GREEN}[5/5] 验证所有产物...${NC}"
echo ""
echo "=== 构建产物列表 ==="
echo ""

# 检查文件
FILES=("$TAR_EXE_NAME" "$TAR_BUNDLE_NAME" "$TAR_DEBUG_NAME" "$DMG_NAME")
ALL_OK=true

for file in "${FILES[@]}"; do
    if [ -f "$file" ]; then
        SIZE=$(du -h "$file" | cut -f1)
        echo -e "${GREEN}✓${NC} $file ($SIZE)"
        
        # 验证文件命名
        if [[ "$file" == *"$TARGET"* ]]; then
            echo "  命名正确: 包含 target triple"
        else
            echo -e "  ${RED}警告: 文件名不包含 target triple${NC}"
            ALL_OK=false
        fi
        
        # 说明文件用途
        if [[ "$file" == *"-bundle.tar.gz" ]]; then
            echo "  用途: 手动安装（包含完整 .app bundle，解压后可直接使用）"
        elif [[ "$file" == *"-debug.tar.gz" ]]; then
            echo "  用途: 调试/查看日志（只包含可执行文件，直接运行会显示终端窗口）"
        elif [[ "$file" == *.tar.gz ]]; then
            echo "  用途: self_update 自动更新（只包含可执行文件）"
        elif [[ "$file" == *.dmg ]]; then
            echo "  用途: 手动安装（macOS 安装镜像）"
        fi
    else
        echo -e "${RED}✗${NC} $file (未找到)"
        ALL_OK=false
    fi
done

echo ""
echo "=== 文件详细信息 ==="
ls -lh "$OUTPUT_DIR"/*.{tar.gz,dmg} 2>/dev/null || true

echo ""
echo "=== 验证 tar.gz 内容 ==="
echo "可执行文件 tar.gz 内容 (self_update):"
tar -tzf "$TAR_EXE_NAME" || echo "无法读取 tar.gz 内容"
echo ""
echo "Bundle tar.gz 内容 (前10项):"
tar -tzf "$TAR_BUNDLE_NAME" | head -10 || echo "无法读取 tar.gz 内容"
echo ""
echo "Debug tar.gz 内容 (terminal mode):"
tar -tzf "$TAR_DEBUG_NAME" || echo "无法读取 tar.gz 内容"

echo ""
echo "=== 验证 DMG 内容 ==="
echo "DMG 挂载点信息:"
hdiutil info | grep -A 5 "$DMG_NAME" || echo "DMG 未挂载"

echo ""

# 总结
if [ "$ALL_OK" = true ]; then
    echo -e "${GREEN}=== 验证通过！所有产物已正确创建 ===${NC}"
    echo ""
    echo "产物文件已生成到: $OUTPUT_DIR/"
    echo ""
    echo "产物文件:"
    echo "  - $TAR_EXE_NAME (用于 self_update 自动更新)"
    echo "  - $TAR_BUNDLE_NAME (用于手动安装，解压后可直接使用)"
    echo "  - $TAR_DEBUG_NAME (用于调试/查看日志，直接运行会显示终端窗口)"
    echo "  - $DMG_NAME (用于手动安装，macOS 安装镜像)"
    echo ""
    echo "说明:"
    echo "  - $TAR_EXE_NAME: 只包含可执行文件，self_update 会自动替换 .app bundle 内的可执行文件"
    echo "  - $TAR_BUNDLE_NAME: 包含完整 .app bundle，解压后可直接使用，不会显示终端窗口"
    echo "  - $TAR_DEBUG_NAME: 只包含可执行文件，直接运行会显示终端窗口，方便查看运行日志"
    echo "  - $DMG_NAME: macOS 安装镜像，双击安装"
    echo ""
    echo "注意: 这些文件已生成到 $OUTPUT_DIR/ 目录，该目录已添加到 .gitignore"
    echo "GitHub Actions 会在项目根目录生成相同的产物。"
    exit 0
else
    echo -e "${RED}=== 验证失败！部分产物缺失或命名不正确 ===${NC}"
    exit 1
fi
