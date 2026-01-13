#!/bin/bash

# 快速检查 release 产物脚本
# 用法: ./scripts/check-artifacts.sh [target]

set -e

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

APP_NAME="Usher"
OUTPUT_DIR="workflow-dist"

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

echo -e "${BLUE}=== 检查 Release 产物 ===${NC}"
echo "Target: $TARGET"
echo "检查目录: $OUTPUT_DIR/"
echo ""

# 检查所有可能的 targets
TARGETS=("aarch64-apple-darwin" "x86_64-apple-darwin")
FOUND_ANY=false

for CHECK_TARGET in "${TARGETS[@]}"; do
    TAR_EXE_NAME="$OUTPUT_DIR/$APP_NAME-$CHECK_TARGET.tar.gz"
    TAR_BUNDLE_NAME="$OUTPUT_DIR/$APP_NAME-$CHECK_TARGET-bundle.tar.gz"
    TAR_DEBUG_NAME="$OUTPUT_DIR/$APP_NAME-$CHECK_TARGET-debug.tar.gz"
    DMG_NAME="$OUTPUT_DIR/$APP_NAME-$CHECK_TARGET.dmg"
    
    echo -e "${YELLOW}检查 $CHECK_TARGET:${NC}"
    
    if [ -f "$TAR_EXE_NAME" ]; then
        SIZE=$(du -h "$TAR_EXE_NAME" | cut -f1)
        echo -e "  ${GREEN}✓${NC} $TAR_EXE_NAME ($SIZE) - 用于 self_update"
        
        # 验证 tar.gz 内容
        echo "    内容:"
        tar -tzf "$TAR_EXE_NAME" 2>/dev/null | head -3 | sed 's/^/      /' || echo "      (无法读取)"
        
        FOUND_ANY=true
    else
        echo -e "  ${RED}✗${NC} $TAR_EXE_NAME (未找到)"
    fi
    
    if [ -f "$TAR_BUNDLE_NAME" ]; then
        SIZE=$(du -h "$TAR_BUNDLE_NAME" | cut -f1)
        echo -e "  ${GREEN}✓${NC} $TAR_BUNDLE_NAME ($SIZE) - 用于手动安装"
        
        # 验证 tar.gz 内容
        echo "    内容 (前3项):"
        tar -tzf "$TAR_BUNDLE_NAME" 2>/dev/null | head -3 | sed 's/^/      /' || echo "      (无法读取)"
        
        FOUND_ANY=true
    else
        echo -e "  ${RED}✗${NC} $TAR_BUNDLE_NAME (未找到)"
    fi
    
    if [ -f "$TAR_DEBUG_NAME" ]; then
        SIZE=$(du -h "$TAR_DEBUG_NAME" | cut -f1)
        echo -e "  ${GREEN}✓${NC} $TAR_DEBUG_NAME ($SIZE) - 用于调试/查看日志"
        
        # 验证 tar.gz 内容
        echo "    内容:"
        tar -tzf "$TAR_DEBUG_NAME" 2>/dev/null | head -3 | sed 's/^/      /' || echo "      (无法读取)"
        
        FOUND_ANY=true
    else
        echo -e "  ${RED}✗${NC} $TAR_DEBUG_NAME (未找到)"
    fi
    
    if [ -f "$DMG_NAME" ]; then
        SIZE=$(du -h "$DMG_NAME" | cut -f1)
        echo -e "  ${GREEN}✓${NC} $DMG_NAME ($SIZE) - 用于手动安装"
        FOUND_ANY=true
    else
        echo -e "  ${RED}✗${NC} $DMG_NAME (未找到)"
    fi
    
    echo ""
done

# 检查当前平台的产物
echo -e "${YELLOW}当前平台 ($TARGET) 的产物:${NC}"
TAR_EXE_NAME="$OUTPUT_DIR/$APP_NAME-$TARGET.tar.gz"
TAR_BUNDLE_NAME="$OUTPUT_DIR/$APP_NAME-$TARGET-bundle.tar.gz"
TAR_DEBUG_NAME="$OUTPUT_DIR/$APP_NAME-$TARGET-debug.tar.gz"
DMG_NAME="$OUTPUT_DIR/$APP_NAME-$TARGET.dmg"

if [ -f "$TAR_EXE_NAME" ] && [ -f "$TAR_BUNDLE_NAME" ] && [ -f "$TAR_DEBUG_NAME" ] && [ -f "$DMG_NAME" ]; then
    echo -e "${GREEN}✓ 所有产物已就绪${NC}"
    echo ""
    echo "文件列表 (位于 $OUTPUT_DIR/):"
    ls -lh "$TAR_EXE_NAME" "$TAR_BUNDLE_NAME" "$TAR_DEBUG_NAME" "$DMG_NAME"
    echo ""
    echo "这些文件应该与 GitHub Actions 生成的产物一致。"
    echo "注意: 本地产物在 $OUTPUT_DIR/ 目录，GitHub Actions 产物在项目根目录。"
else
    echo -e "${YELLOW}⚠ 部分产物缺失${NC}"
    echo ""
    echo "运行以下命令生成产物:"
    echo "  ./scripts/verify-release.sh"
    echo ""
    echo "产物将生成到 $OUTPUT_DIR/ 目录"
fi

# 检查文件命名是否符合 self_update 的要求
echo ""
echo -e "${BLUE}=== 验证文件命名 (self_update 兼容性) ===${NC}"

for CHECK_TARGET in "${TARGETS[@]}"; do
    TAR_EXE_NAME="$OUTPUT_DIR/$APP_NAME-$CHECK_TARGET.tar.gz"
    TAR_BUNDLE_NAME="$OUTPUT_DIR/$APP_NAME-$CHECK_TARGET-bundle.tar.gz"
    TAR_DEBUG_NAME="$OUTPUT_DIR/$APP_NAME-$CHECK_TARGET-debug.tar.gz"
    
    for TAR_NAME in "$TAR_EXE_NAME" "$TAR_BUNDLE_NAME" "$TAR_DEBUG_NAME"; do
        if [ -f "$TAR_NAME" ]; then
            echo "检查: $TAR_NAME"
            
            # 检查是否包含 target triple
            if [[ "$TAR_NAME" == *"$CHECK_TARGET"* ]]; then
                echo -e "  ${GREEN}✓${NC} 文件名包含 target triple: $CHECK_TARGET"
            else
                echo -e "  ${RED}✗${NC} 文件名不包含 target triple"
            fi
            
            # 检查格式
            if [[ "$TAR_NAME" == *.tar.gz ]]; then
                echo -e "  ${GREEN}✓${NC} 格式正确: .tar.gz"
            else
                echo -e "  ${RED}✗${NC} 格式不正确，应该是 .tar.gz"
            fi
            
            echo ""
        fi
    done
done

if [ "$FOUND_ANY" = true ]; then
    exit 0
else
    echo -e "${RED}未找到任何产物文件${NC}"
    exit 1
fi
