#!/bin/bash

# AuroraDB 完全离线手动打包 AppImage
# 直接使用 linuxdeploy，不依赖 Tauri bundler

set -e

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo "========================================"
echo "  AuroraDB 离线手动打包 AppImage"
echo "========================================"
echo ""

# 检查离线工具
OFFLINE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/.tauri-tools"

if [ ! -d "$OFFLINE_DIR" ]; then
    echo -e "${RED}错误: 未找到离线工具目录${NC}"
    echo ""
    echo "请先运行: ./download-offline.sh"
    exit 1
fi

# 检查必需的工具文件
MISSING=0
[ ! -f "$OFFLINE_DIR/linuxdeploy-x86_64.AppImage" ] && echo -e "${RED}✗ 缺少: linuxdeploy-x86_64.AppImage${NC}" && MISSING=1
[ ! -f "$OFFLINE_DIR/linuxdeploy-plugin-appimage-x86_64.AppImage" ] && echo -e "${RED}✗ 缺少: linuxdeploy-plugin-appimage-x86_64.AppImage${NC}" && MISSING=1
[ ! -f "$OFFLINE_DIR/AppRun-x86_64" ] && echo -e "${RED}✗ 缺少: AppRun-x86_64${NC}" && MISSING=1

if [ $MISSING -eq 1 ]; then
    exit 1
fi

echo -e "${GREEN}✓ 离线工具检查通过${NC}"
echo ""

# 设置工具路径
LINUXDEPLOY="$OFFLINE_DIR/linuxdeploy-x86_64.AppImage"
LINUXDEPLOY_PLUGIN="$OFFLINE_DIR/linuxdeploy-plugin-appimage-x86_64.AppImage"
APPRUN="$OFFLINE_DIR/AppRun-x86_64"

# 确保可执行
chmod +x "$LINUXDEPLOY"
chmod +x "$LINUXDEPLOY_PLUGIN"
chmod +x "$APPRUN"

# 获取脚本所在目录
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# 构建前端
echo -e "${YELLOW}[1/3] 构建前端...${NC}"
cd src-ui
if [ ! -d "node_modules" ]; then
    npm install
fi
npm run build
echo -e "${GREEN}  ✓ 前端构建完成${NC}"

# 构建 Rust
cd "$SCRIPT_DIR"
echo -e "${YELLOW}[2/3] 构建 Rust 可执行文件...${NC}"
cd src-tauri

# 只构建 release 二进制，不打包
cargo build --release
echo -e "${GREEN}  ✓ Rust 构建完成${NC}"

# 创建 AppDir 结构
echo -e "${YELLOW}[3/3] 手动打包 AppImage...${NC}"
cd "$SCRIPT_DIR"

TARGET_DIR="$SCRIPT_DIR/src-tauri/target/release"
APPDIR="$TARGET_DIR/AppDir"

echo "  TARGET_DIR: $TARGET_DIR"
echo "  APPDIR: $APPDIR"

# 检查二进制文件是否存在
if [ ! -f "$TARGET_DIR/auroradb" ]; then
    echo -e "${RED}错误: 未找到二进制文件 $TARGET_DIR/auroradb${NC}"
    ls -la "$TARGET_DIR/" | head -20
    exit 1
fi

# 清理旧目录
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/lib"
mkdir -p "$APPDIR/usr/share/applications"
mkdir -p "$APPDIR/usr/share/icons/hicolor/256x256/apps"

# 复制二进制文件
echo "  复制二进制文件..."
cp "$TARGET_DIR/auroradb" "$APPDIR/usr/bin/"
ls -la "$APPDIR/usr/bin/"

# 复制前端资源
mkdir -p "$APPDIR/usr/share/auroradb"
cp -r src-ui/dist/* "$APPDIR/usr/share/auroradb/" 2>/dev/null || true

# 创建 .desktop 文件
cat > "$APPDIR/usr/share/applications/auroradb.desktop" << 'EOF'
[Desktop Entry]
Name=AuroraDB
Comment=A MySQL database client
Exec=auroradb
Icon=auroradb
Type=Application
Categories=Development;Database;
Terminal=false
X-AppImage-Integrate=true
EOF

# 创建符号链接
ln -sf "usr/share/applications/auroradb.desktop" "$APPDIR/auroradb.desktop"

# 创建图标（使用 256x256.png）
if [ -f "src-tauri/icons/256x256.png" ]; then
    cp "src-tauri/icons/256x256.png" "$APPDIR/usr/share/icons/hicolor/256x256/apps/auroradb.png"
    echo "  ✓ 已复制图标 256x256.png"
else
    echo -e "${YELLOW}警告: 未找到 src-tauri/icons/256x256.png${NC}"
    touch "$APPDIR/usr/share/icons/hicolor/256x256/apps/auroradb.png"
fi
ln -sf "usr/share/icons/hicolor/256x256/apps/auroradb.png" "$APPDIR/auroradb.png"

# 复制 AppRun
cp "$APPRUN" "$APPDIR/AppRun"

# 创建启动脚本
cat > "$APPDIR/usr/bin/auroradb-wrapper" << 'WRAPPER'
#!/bin/bash
# AppImage 启动脚本

# 获取 AppImage 所在目录
APPDIR="$(dirname "$(readlink -f "$0")")/../.."

# 设置环境变量
export APPDIR="$APPDIR"
export PATH="$APPDIR/usr/bin:$PATH"

# 启动应用
exec "$APPDIR/usr/bin/auroradb" "$@"
WRAPPER

chmod +x "$APPDIR/usr/bin/auroradb-wrapper"

# 使用 linuxdeploy 打包
echo "  运行 linuxdeploy..."

# 设置环境变量让 linuxdeploy 使用离线插件
export PATH="$OFFLINE_DIR:$PATH"
ln -sf "$LINUXDEPLOY_PLUGIN" "$OFFLINE_DIR/linuxdeploy-plugin-appimage" 2>/dev/null || true

# 验证文件存在
echo "  检查 AppDir 内容..."
ls -la "$APPDIR/usr/bin/auroradb" || echo "二进制文件不存在!"
ls -la "$APPDIR/usr/share/icons/hicolor/256x256/apps/auroradb.png" || echo "图标不存在!"
ls -la "$APPDIR/AppRun" || echo "AppRun 不存在!"

# 运行 linuxdeploy (保持在 SCRIPT_DIR，使用绝对路径)
"$LINUXDEPLOY" \
    --appdir "$APPDIR" \
    --output appimage \
    2>&1 | grep -v "^Downloading" || true

# 查找生成的 AppImage
# linuxdeploy 可能在当前目录或 target 目录生成
cd "$SCRIPT_DIR"
APPIMAGE_FILE=$(find . -maxdepth 1 -name "*.AppImage" -type f 2>/dev/null | head -n 1)
if [ -n "$APPIMAGE_FILE" ]; then
    APPIMAGE_FILE="$SCRIPT_DIR/${APPIMAGE_FILE#./}"
fi

if [ -n "$APPIMAGE_FILE" ]; then
    echo ""
    echo -e "${GREEN}========================================${NC}"
    echo -e "${GREEN}    AppImage 打包成功！${NC}"
    echo -e "${GREEN}========================================${NC}"
    echo ""
    echo "文件位置:"
    ls -lh "$APPIMAGE_FILE"
    echo ""
    echo "使用方法:"
    echo "  chmod +x $APPIMAGE_FILE"
    echo "  ./$(basename "$APPIMAGE_FILE")"
    echo ""
    
    # 复制到 bundle 目录
    mkdir -p "$TARGET_DIR/bundle/appimage"
    cp "$APPIMAGE_FILE" "$TARGET_DIR/bundle/appimage/"
    echo "已复制到: $TARGET_DIR/bundle/appimage/"
else
    echo ""
    echo -e "${RED}✗ 未找到生成的 AppImage 文件${NC}"
    echo "请检查 $TARGET_DIR 目录"
    exit 1
fi
