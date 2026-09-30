#!/bin/bash

# macOS DMG 安装程序构建脚本
# 使用方法: ./scripts/build-dmg.sh

set -e

APP_NAME="Usher"
CARGO_BUNDLE_VERSION="0.12.0"
VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')

echo "🔨 Building ${APP_NAME} v${VERSION}..."

# 检查是否安装了 create-dmg
if ! command -v create-dmg &> /dev/null; then
    echo "📦 Installing create-dmg..."
    brew install create-dmg
fi

# 检查是否安装了发布流程锁定的 cargo-bundle 版本
if [ "$(cargo-bundle --version 2>/dev/null || true)" != "cargo-bundle v${CARGO_BUNDLE_VERSION}" ]; then
    echo "📦 Installing cargo-bundle v${CARGO_BUNDLE_VERSION}..."
    cargo install cargo-bundle --version "$CARGO_BUNDLE_VERSION" --locked
fi

# 构建 Release 版本
echo "🔨 Building release..."
cargo build --release

# 创建 App Bundle
echo "📦 Creating app bundle..."
cargo bundle --release --format osx

# 创建输出目录
mkdir -p dist

# 删除旧的 DMG 文件
rm -f "dist/${APP_NAME}-${VERSION}.dmg"

# 创建 DMG 安装程序
echo "💿 Creating DMG installer..."
create-dmg \
    --volname "${APP_NAME}" \
    --window-pos 200 120 \
    --window-size 600 400 \
    --icon-size 100 \
    --icon "${APP_NAME}.app" 175 178 \
    --hide-extension "${APP_NAME}.app" \
    --app-drop-link 425 178 \
    --no-internet-enable \
    "dist/${APP_NAME}-${VERSION}.dmg" \
    "target/release/bundle/osx/" \
|| {
    echo "⚠️  create-dmg failed, using hdiutil fallback..."
    hdiutil create -volname "${APP_NAME}" \
        -srcfolder "target/release/bundle/osx/${APP_NAME}.app" \
        -ov -format UDZO \
        "dist/${APP_NAME}-${VERSION}.dmg"
}

echo ""
echo "✅ DMG 安装程序已创建: dist/${APP_NAME}-${VERSION}.dmg"
echo ""
echo "📝 安装说明:"
echo "   1. 双击 ${APP_NAME}-${VERSION}.dmg 打开"
echo "   2. 将 ${APP_NAME}.app 拖到 Applications 文件夹"
echo "   3. 从 Applications 或 Launchpad 启动 ${APP_NAME}"
