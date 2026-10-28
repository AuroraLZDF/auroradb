#!/bin/bash

# AuroraDB MySQL Client 打包脚本
# 用于构建 Linux 应用安装包

set -e  # 遇到错误立即退出

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# 帮助信息
show_help() {
    echo "AuroraDB MySQL Client 打包脚本"
    echo ""
    echo "用法: ./build.sh [选项]"
    echo ""
    echo "选项:"
    echo "  --deb         只打包 .deb 格式 (Debian/Ubuntu)"
    echo "  --appimage    只打包 .AppImage 格式 (需要下载工具)"
    echo "  --rpm         只打包 .rpm 格式 (Fedora/CentOS)"
    echo "  --bundles X   自定义格式，逗号分隔，如 deb,appimage"
    echo "  --all         打包所有格式 (deb,appimage,rpm)"
    echo "  --help        显示帮助信息"
    echo ""
    echo "注意: AppImage 打包需要从 GitHub 下载工具，"
    echo "      如果网络不好建议只打包 --deb"
}

# 解析参数
BUNDLE_TYPE="deb"  # 默认只打包 deb

while [[ $# -gt 0 ]]; do
    case $1 in
        --deb)
            BUNDLE_TYPE="deb"
            shift
            ;;
        --appimage)
            BUNDLE_TYPE="appimage"
            shift
            ;;
        --rpm)
            BUNDLE_TYPE="rpm"
            shift
            ;;
        --bundles)
            if [[ $# -lt 2 || -z "$2" ]]; then
                echo "错误: --bundles 需要参数，如 --bundles deb,appimage"
                exit 1
            fi
            BUNDLE_TYPE="$2"
            shift 2
            ;;
        --all)
            BUNDLE_TYPE="deb,appimage,rpm"
            shift
            ;;
        --help|-h)
            show_help
            exit 0
            ;;
        *)
            echo "未知选项: $1"
            show_help
            exit 1
            ;;
    esac
done

echo "========================================"
echo "  AuroraDB MySQL Client 打包脚本"
echo "========================================"
echo ""

# 获取脚本所在目录
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# 检查前端依赖
echo -e "${YELLOW}[1/5] 检查前端依赖...${NC}"
cd src-ui
if [ ! -d "node_modules" ]; then
    echo "  安装前端依赖..."
    npm install
fi

# 构建前端
echo -e "${YELLOW}[2/5] 构建前端...${NC}"
npm run build
echo -e "${GREEN}  ✓ 前端构建完成${NC}"

# 检查 Rust 环境
cd "$SCRIPT_DIR"
echo -e "${YELLOW}[3/5] 检查 Rust 环境...${NC}"
if ! command -v rustc &> /dev/null; then
    echo -e "${RED}错误: 未安装 Rust${NC}"
    echo "请运行以下命令安装 Rust:"
    echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

if ! command -v cargo &> /dev/null; then
    echo -e "${RED}错误: 未找到 Cargo${NC}"
    exit 1
fi

echo -e "${GREEN}  ✓ Rust 环境正常${NC}"

# 安装 Tauri CLI（如果没有安装）
echo -e "${YELLOW}[4/5] 检查 Tauri CLI...${NC}"
if ! cargo tauri --version &> /dev/null 2>&1; then
    echo "  安装 Tauri CLI..."
    cargo install tauri-cli --locked
fi
echo -e "${GREEN}  ✓ Tauri CLI 就绪${NC}"

# 预下载 AppImage 工具（如果需要）
if [[ "$BUNDLE_TYPE" == *"appimage"* ]]; then
    echo -e "${YELLOW}[4.5/5] 准备 AppImage 工具...${NC}"
    
    # 创建工具目录
    TOOLS_DIR="$HOME/.cache/tauri-tools"
    mkdir -p "$TOOLS_DIR"
    
    # 检查并下载 AppRun
    APPRUN_URL="https://github.com/tauri-apps/binary-releases/releases/download/apprun-old/AppRun-x86_64"
    APPRUN_FILE="$TOOLS_DIR/AppRun-x86_64"
    
    if [ ! -f "$APPRUN_FILE" ]; then
        echo "  下载 AppRun..."
        if command -v wget &> /dev/null; then
            wget --timeout=60 --tries=3 -O "$APPRUN_FILE" "$APPRUN_URL" || {
                echo -e "${RED}  ✗ 下载 AppRun 失败${NC}"
                echo "  建议只打包 deb: ./build.sh --deb"
                exit 1
            }
        elif command -v curl &> /dev/null; then
            curl --max-time 60 --retry 3 -L -o "$APPRUN_FILE" "$APPRUN_URL" || {
                echo -e "${RED}  ✗ 下载 AppRun 失败${NC}"
                echo "  建议只打包 deb: ./build.sh --deb"
                exit 1
            }
        else
            echo -e "${RED}  ✗ 未找到 wget 或 curl${NC}"
            exit 1
        fi
        chmod +x "$APPRUN_FILE"
        echo -e "${GREEN}  ✓ AppRun 下载完成${NC}"
    else
        echo -e "${GREEN}  ✓ AppRun 已缓存${NC}"
    fi
    
    # 设置环境变量让 Tauri 使用本地工具
    export TAURI_APP_IMAGE_APPRUN_PATH="$APPRUN_FILE"
fi

# 构建应用
echo -e "${YELLOW}[5/5] 构建 Linux 安装包...${NC}"
echo "  打包格式: $BUNDLE_TYPE"
cd src-tauri

# 更新包签名：Tauri CLI 只读取 TAURI_SIGNING_PRIVATE_KEY（内容），不读 _PATH
# 未显式提供时回退到本地默认路径（私钥不入库）
if [ -z "$TAURI_SIGNING_PRIVATE_KEY" ]; then
    DEFAULT_SIGN_KEY="$HOME/.config/auroradb/updater.key"
    if [ -f "$DEFAULT_SIGN_KEY" ]; then
        export TAURI_SIGNING_PRIVATE_KEY="$(cat "$DEFAULT_SIGN_KEY")"
        echo "  更新签名私钥: $DEFAULT_SIGN_KEY"
    else
        echo -e "${RED}错误: 未找到更新签名私钥${NC}"
        echo "  请先生成: cd src-ui && npm run tauri signer generate -- -w ~/.config/auroradb/updater.key"
        echo "  或设置 TAURI_SIGNING_PRIVATE_KEY（私钥文件内容）"
        exit 1
    fi
fi

# 私钥若带密码，Tauri CLI 会交互式索要，无法在一键脚本中使用
# 支持从本地文件读取密码（不入库）：~/.config/auroradb/updater.password
if [ -z "$TAURI_SIGNING_PRIVATE_KEY_PASSWORD" ]; then
    DEFAULT_SIGN_PASS="$HOME/.config/auroradb/updater.password"
    if [ -f "$DEFAULT_SIGN_PASS" ]; then
        export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(tr -d '\r\n' < "$DEFAULT_SIGN_PASS")"
        echo "  更新签名密码: $DEFAULT_SIGN_PASS"
    else
        echo -e "${YELLOW}  提示: 私钥已加密，构建时会要求输入密码${NC}"
        echo "  可写入 ~/.config/auroradb/updater.password 或设置 TAURI_SIGNING_PRIVATE_KEY_PASSWORD 免交互"
    fi
fi

# 使用 timeout 防止卡死
if command -v timeout &> /dev/null; then
    timeout 600 cargo tauri build --bundles "$BUNDLE_TYPE" || {
        echo -e "${RED}  ✗ 打包超时或失败${NC}"
        echo ""
        echo "如果卡在网络下载，建议:"
        echo "  1. 只打包 deb:  ./build.sh --deb"
        echo "  2. 配置代理后重试"
        exit 1
    }
else
    cargo tauri build --bundles "$BUNDLE_TYPE"
fi

# AppImage 默认压缩率下超过 100MB，会被 Gitee 发行版拒收；重打包后再签名
if [[ "$BUNDLE_TYPE" == *"appimage"* ]]; then
    echo -e "${YELLOW}[5.5/5] 重新压缩 AppImage...${NC}"
    shopt -s nullglob
    IMAGES=(target/release/bundle/appimage/*.AppImage)
    shopt -u nullglob
    if (( ${#IMAGES[@]} == 0 )); then
        echo -e "${RED}  ✗ 未生成 AppImage${NC}"
        exit 1
    fi
    for img in "${IMAGES[@]}"; do
        "$SCRIPT_DIR/repack-appimage.sh" "$img"
    done
fi

echo ""
echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}    打包成功！${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""

# 显示生成的文件
BUNDLE_DIR="target/release/bundle"
echo "安装包位置:"
echo ""

if [ -d "$BUNDLE_DIR/deb" ]; then
    echo -e "${GREEN}[Debian/Ubuntu 安装包]${NC}"
    ls -lh "$BUNDLE_DIR/deb"/*.deb 2>/dev/null | awk '{print "  " $9 " (" $5 ")"}' || echo "  未生成"
    echo ""
fi

if [ -d "$BUNDLE_DIR/appimage" ]; then
    echo -e "${GREEN}[AppImage (免安装)]${NC}"
    ls -lh "$BUNDLE_DIR/appimage"/*.AppImage 2>/dev/null | awk '{print "  " $9 " (" $5 ")"}' || echo "  未生成"
    echo ""
fi

if [ -d "$BUNDLE_DIR/rpm" ]; then
    echo -e "${GREEN}[RPM 安装包]${NC}"
    ls -lh "$BUNDLE_DIR/rpm"/*.rpm 2>/dev/null | awk '{print "  " $9 " (" $5 ")"}' || echo "  未生成"
    echo ""
fi

# 输出使用说明
echo "========================================"
echo "使用说明:"
echo "========================================"
echo ""

if [ -d "$BUNDLE_DIR/appimage" ] && [ -n "$(ls -A "$BUNDLE_DIR/appimage"/*.AppImage 2>/dev/null)" ]; then
    echo "1. AppImage (推荐):"
    echo "   cd $BUNDLE_DIR/appimage"
    echo "   chmod +x *.AppImage"
    echo "   ./mysql-client_*.AppImage"
    echo ""
fi

if [ -d "$BUNDLE_DIR/deb" ] && [ -n "$(ls -A "$BUNDLE_DIR/deb"/*.deb 2>/dev/null)" ]; then
    echo "2. Debian/Ubuntu 安装:"
    echo "   cd $BUNDLE_DIR/deb"
    echo "   sudo dpkg -i mysql-client_*.deb"
    echo "   sudo apt-get install -f  # 如有依赖问题"
    echo ""
    echo "3. 卸载应用:"
    echo "   sudo apt remove mysql-client"
    echo ""
fi

echo "重新打包:"
echo "  ./build.sh --deb        # 只打包 deb"
echo "  ./build.sh --appimage   # 只打包 AppImage"
echo "  ./build.sh --all        # 打包所有格式"
echo ""
