#!/bin/bash

# AuroraDB 离线打包工具下载脚本
# 用于下载 Tauri 打包 AppImage 所需的所有离线工具

set -e

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo "========================================"
echo "  AuroraDB 离线打包工具下载脚本"
echo "========================================"
echo ""

# 创建离线工具目录
OFFLINE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/.tauri-tools"
mkdir -p "$OFFLINE_DIR"
cd "$OFFLINE_DIR"

echo "工具将下载到: $OFFLINE_DIR"
echo ""

# 工具版本
LINUXDEPLOY_VERSION="continuous"
APPIMAGE_TOOL_VERSION="13"

echo "开始下载工具..."
echo ""

# 1. linuxdeploy (x86_64)
echo -e "${YELLOW}[1/4] 下载 linuxdeploy...${NC}"
LINUXDEPLOY_URL="https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage"
LINUXDEPLOY_FILE="linuxdeploy-x86_64.AppImage"

if [ -f "$LINUXDEPLOY_FILE" ]; then
    echo -e "${GREEN}  ✓ linuxdeploy 已存在${NC}"
else
    echo "  下载: $LINUXDEPLOY_URL"
    if command -v wget &> /dev/null; then
        wget --timeout=120 -O "$LINUXDEPLOY_FILE" "$LINUXDEPLOY_URL" || echo -e "${RED}  ✗ 下载失败${NC}"
    elif command -v curl &> /dev/null; then
        curl --max-time 120 -L -o "$LINUXDEPLOY_FILE" "$LINUXDEPLOY_URL" || echo -e "${RED}  ✗ 下载失败${NC}"
    else
        echo -e "${RED}  ✗ 未找到 wget 或 curl${NC}"
    fi
    
    if [ -f "$LINUXDEPLOY_FILE" ]; then
        chmod +x "$LINUXDEPLOY_FILE"
        echo -e "${GREEN}  ✓ linuxdeploy 下载完成${NC}"
    fi
fi

# 2. linuxdeploy-plugin-appimage
echo -e "${YELLOW}[2/4] 下载 linuxdeploy-plugin-appimage...${NC}"
PLUGIN_URL="https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/releases/download/continuous/linuxdeploy-plugin-appimage-x86_64.AppImage"
PLUGIN_FILE="linuxdeploy-plugin-appimage-x86_64.AppImage"

if [ -f "$PLUGIN_FILE" ]; then
    echo -e "${GREEN}  ✓ linuxdeploy-plugin-appimage 已存在${NC}"
else
    echo "  下载: $PLUGIN_URL"
    if command -v wget &> /dev/null; then
        wget --timeout=120 -O "$PLUGIN_FILE" "$PLUGIN_URL" || echo -e "${RED}  ✗ 下载失败${NC}"
    elif command -v curl &> /dev/null; then
        curl --max-time 120 -L -o "$PLUGIN_FILE" "$PLUGIN_URL" || echo -e "${RED}  ✗ 下载失败${NC}"
    fi
    
    if [ -f "$PLUGIN_FILE" ]; then
        chmod +x "$PLUGIN_FILE"
        echo -e "${GREEN}  ✓ linuxdeploy-plugin-appimage 下载完成${NC}"
    fi
fi

# 3. linuxdeploy-plugin-gtk (可选，如果你的应用用了 GTK)
echo -e "${YELLOW}[3/4] 下载 linuxdeploy-plugin-gtk (可选)...${NC}"
GTK_PLUGIN_URL="https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh"
GTK_PLUGIN_FILE="linuxdeploy-plugin-gtk.sh"

if [ -f "$GTK_PLUGIN_FILE" ]; then
    echo -e "${GREEN}  ✓ linuxdeploy-plugin-gtk 已存在${NC}"
else
    echo "  下载: $GTK_PLUGIN_URL"
    if command -v wget &> /dev/null; then
        wget --timeout=120 -O "$GTK_PLUGIN_FILE" "$GTK_PLUGIN_URL" 2>/dev/null || echo -e "${BLUE}  ℹ 跳过 (非必须)${NC}"
    elif command -v curl &> /dev/null; then
        curl --max-time 120 -L -o "$GTK_PLUGIN_FILE" "$GTK_PLUGIN_URL" 2>/dev/null || echo -e "${BLUE}  ℹ 跳过 (非必须)${NC}"
    fi
    
    if [ -f "$GTK_PLUGIN_FILE" ]; then
        chmod +x "$GTK_PLUGIN_FILE"
        echo -e "${GREEN}  ✓ linuxdeploy-plugin-gtk 下载完成${NC}"
    fi
fi

# 4. AppRun (Tauri 专用)
echo -e "${YELLOW}[4/4] 下载 AppRun...${NC}"
APPRUN_URL="https://github.com/tauri-apps/binary-releases/releases/download/apprun-old/AppRun-x86_64"
APPRUN_FILE="AppRun-x86_64"

if [ -f "$APPRUN_FILE" ]; then
    echo -e "${GREEN}  ✓ AppRun 已存在${NC}"
else
    echo "  下载: $APPRUN_URL"
    if command -v wget &> /dev/null; then
        wget --timeout=120 -O "$APPRUN_FILE" "$APPRUN_URL" || echo -e "${RED}  ✗ 下载失败${NC}"
    elif command -v curl &> /dev/null; then
        curl --max-time 120 -L -o "$APPRUN_FILE" "$APPRUN_URL" || echo -e "${RED}  ✗ 下载失败${NC}"
    fi
    
    if [ -f "$APPRUN_FILE" ]; then
        chmod +x "$APPRUN_FILE"
        echo -e "${GREEN}  ✓ AppRun 下载完成${NC}"
    fi
fi

echo ""
echo "========================================"
echo "下载完成！"
echo "========================================"
echo ""
echo "工具清单:"
echo ""
ls -lh "$OFFLINE_DIR" | grep -v "^d" | tail -n +2 | awk '{printf "  %-40s %s\n", $9, $5}'
echo ""
echo "========================================"
echo "下一步操作:"
echo "========================================"
echo ""
echo "方法1: 直接打包 (使用离线工具)"
echo "  ./build-offline.sh --appimage"
echo ""
echo "方法2: 复制工具到缓存目录"
echo "  cp -r .tauri-tools/* ~/.cache/tauri-tools/"
echo "  ./build.sh --appimage"
echo ""
echo "方法3: 手动复制到目标机器"
echo "  将整个 .tauri-tools 目录复制到目标机器的相同位置"
echo "  然后运行 build.sh"
echo ""
