#!/usr/bin/env bash

# AuroraDB 离线备用方案：不依赖 Tauri bundler，直接用 linuxdeploy 打 AppImage
#
# 适用场景:
#   ./tools/build/linux.sh --appimage 因网络/linuxdeploy 下载失败而无法完成时使用。
#   首次运行会把所需工具下载到仓库下的 .tauri-tools/，之后可离线重复使用。
#
# 用法:
#   ./tools/build/appimage-offline.sh          # 复用已有 release 二进制；缺失时才构建
#   ./tools/build/appimage-offline.sh --build  # 强制重新构建前端与 Rust
#
# 产物: src-tauri/target/release/bundle/appimage/*.AppImage（已重压缩并签名）

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OFFLINE_DIR="$ROOT/.tauri-tools"
TARGET_DIR="$ROOT/src-tauri/target/release"
APPDIR="$TARGET_DIR/AppDir"
FORCE_BUILD=0

for arg in "$@"; do
    case "$arg" in
        --build) FORCE_BUILD=1 ;;
        -h|--help) sed -n '3,14p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo -e "${RED}未知参数: $arg${NC}"; exit 1 ;;
    esac
done

echo "========================================"
echo "  AuroraDB 离线打包 AppImage"
echo "========================================"
echo ""

# ---------- 1. 准备 linuxdeploy 工具 ----------
download() {
    local url="$1" out="$2"
    if command -v wget &>/dev/null; then
        wget --timeout=120 -O "$out" "$url"
    elif command -v curl &>/dev/null; then
        curl --max-time 120 -L -o "$out" "$url"
    else
        echo -e "${RED}错误: 未找到 wget 或 curl${NC}" >&2
        return 1
    fi
}

# 必需工具；下载失败直接终止，避免后面拿半个文件去打包
mkdir -p "$OFFLINE_DIR"
declare -A TOOLS=(
    ["linuxdeploy-x86_64.AppImage"]="https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage"
    ["linuxdeploy-plugin-appimage-x86_64.AppImage"]="https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous/linuxdeploy-plugin-appimage-x86_64.AppImage"
    ["AppRun-x86_64"]="https://github.com/tauri-apps/binary-releases/releases/download/apprun-old/AppRun-x86_64"
)
for name in "${!TOOLS[@]}"; do
    if [[ ! -f "$OFFLINE_DIR/$name" ]]; then
        echo -e "${YELLOW}下载 $name ...${NC}"
        download "${TOOLS[$name]}" "$OFFLINE_DIR/$name" || {
            rm -f "$OFFLINE_DIR/$name"
            echo -e "${RED}✗ $name 下载失败${NC}"
            exit 1
        }
    fi
    chmod +x "$OFFLINE_DIR/$name"
done
echo -e "${GREEN}✓ 离线工具就绪（$OFFLINE_DIR）${NC}"
echo ""

# ---------- 2. 准备前端资源与 Rust 二进制 ----------
if (( FORCE_BUILD )) || [[ ! -d "$ROOT/src-ui/dist" ]]; then
    echo -e "${YELLOW}[1/3] 构建前端...${NC}"
    cd "$ROOT/src-ui"
    [[ -d node_modules ]] || npm install
    npm run build
else
    echo -e "${GREEN}[1/3] 复用已有前端产物 src-ui/dist${NC}"
fi

if (( FORCE_BUILD )) || [[ ! -x "$TARGET_DIR/auroradb" ]]; then
    echo -e "${YELLOW}[2/3] 构建 Rust 可执行文件...${NC}"
    cd "$ROOT/src-tauri"
    cargo build --release
else
    echo -e "${GREEN}[2/3] 复用已有二进制 $TARGET_DIR/auroradb${NC}"
fi
cd "$ROOT"

# ---------- 3. 组装 AppDir 并用 linuxdeploy 打包 ----------
echo -e "${YELLOW}[3/3] 组装 AppDir 并打包...${NC}"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" \
         "$APPDIR/usr/share/applications" \
         "$APPDIR/usr/share/auroradb" \
         "$APPDIR/usr/share/icons/hicolor/256x256/apps"

cp "$TARGET_DIR/auroradb" "$APPDIR/usr/bin/"
cp -r "$ROOT/src-ui/dist/." "$APPDIR/usr/share/auroradb/"

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
ln -sf "usr/share/applications/auroradb.desktop" "$APPDIR/auroradb.desktop"

if [[ -f "$ROOT/src-tauri/icons/256x256.png" ]]; then
    cp "$ROOT/src-tauri/icons/256x256.png" "$APPDIR/usr/share/icons/hicolor/256x256/apps/auroradb.png"
else
    echo -e "${YELLOW}  警告: 未找到 src-tauri/icons/256x256.png${NC}"
    touch "$APPDIR/usr/share/icons/hicolor/256x256/apps/auroradb.png"
fi
ln -sf "usr/share/icons/hicolor/256x256/apps/auroradb.png" "$APPDIR/auroradb.png"

cp "$OFFLINE_DIR/AppRun-x86_64" "$APPDIR/AppRun"
chmod +x "$APPDIR/AppRun"

# linuxdeploy 通过 PATH 查找插件，这里把工具目录加进去
export PATH="$OFFLINE_DIR:$PATH"
ln -sf "$OFFLINE_DIR/linuxdeploy-plugin-appimage-x86_64.AppImage" \
       "$OFFLINE_DIR/linuxdeploy-plugin-appimage"

cd "$ROOT"
"$OFFLINE_DIR/linuxdeploy-x86_64.AppImage" --appdir "$APPDIR" --output appimage

# linuxdeploy 在仓库根目录生成 AppImage
shopt -s nullglob
GENERATED=(./*.AppImage)
shopt -u nullglob
if (( ${#GENERATED[@]} == 0 )); then
    echo -e "${RED}✗ 未找到生成的 AppImage${NC}"
    exit 1
fi

BUNDLE_DIR="$TARGET_DIR/bundle/appimage"
mkdir -p "$BUNDLE_DIR"
mv "${GENERATED[0]}" "$BUNDLE_DIR/"

# linuxdeploy 默认压缩率偏大，重压缩并重新签名
BUNDLED="$BUNDLE_DIR/$(basename "${GENERATED[0]}")"
"$ROOT/tools/build/appimage-repack.sh" "$BUNDLED"

echo ""
echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}    AppImage 打包成功！${NC}"
echo -e "${GREEN}========================================${NC}"
ls -lh "$BUNDLED"
echo ""
echo "发布: ./tools/publish/linux.sh"
