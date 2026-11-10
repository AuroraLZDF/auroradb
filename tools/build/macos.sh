#!/bin/bash

# AuroraDB MySQL Client macOS 打包脚本
#
# 默认产出 Apple Silicon（aarch64-apple-darwin）的 .app 与 .dmg：
#   - src-tauri/target/aarch64-apple-darwin/release/bundle/macos/AuroraDB.app
#   - src-tauri/target/aarch64-apple-darwin/release/bundle/macos/AuroraDB.app.tar.gz(.sig)  # 应用内更新包
#   - src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/AuroraDB_<ver>_aarch64.dmg
#
# 用法:
#   ./tools/build/macos.sh                 # arm64（默认）
#   ./tools/build/macos.sh --intel         # x86_64（需在 Intel runner 或交叉工具链下）
#   ./tools/build/macos.sh --bundles app   # 只打 .app（调试用，省去 dmg 生成时间）
#
# 说明:
# - 未签名分发：tauri.conf.json 里 bundle.macOS.signingIdentity = "-"（ad-hoc 签名），
#   保证 Apple Silicon 上能运行；正式分发需要 Apple Developer ID + 公证（见 docs/RELEASING.md）。
# - 更新包签名与 Apple 签名无关，仍由 TAURI_SIGNING_PRIVATE_KEY 产生 .sig。

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

TARGET="aarch64-apple-darwin"
BUNDLES="app,dmg"

while [[ $# -gt 0 ]]; do
    case $1 in
        --intel) TARGET="x86_64-apple-darwin"; shift ;;
        --bundles) BUNDLES="$2"; shift 2 ;;
        -h|--help) sed -n '3,20p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo -e "${RED}未知参数: $1${NC}"; exit 1 ;;
    esac
done

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo -e "${RED}错误: macOS 安装包只能在 macOS 上构建（GitHub Actions 的 macos-14 runner 即可）${NC}"
    exit 1
fi

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}    AuroraDB macOS 打包${NC}"
echo -e "${BLUE}========================================${NC}"
echo "  目标架构: $TARGET"
echo "  打包类型: $BUNDLES"
echo ""

VERSION="$(grep -m1 '"version"' src-tauri/tauri.conf.json | sed 's/.*"version": *"\([^"]*\)".*/\1/')"
echo -e "${GREEN}[1/4]${NC} 版本: $VERSION"

echo -e "${GREEN}[2/4]${NC} 检查打包环境..."
command -v cargo >/dev/null || { echo -e "${RED}  ✗ 未找到 cargo${NC}"; exit 1; }
command -v node >/dev/null || { echo -e "${RED}  ✗ 未找到 node${NC}"; exit 1; }
if ! rustup target list --installed 2>/dev/null | grep -q "^$TARGET$"; then
    echo -e "${YELLOW}  安装 Rust 目标 $TARGET${NC}"
    rustup target add "$TARGET"
fi
if [[ ! -d node_modules && ! -d src-ui/node_modules ]]; then
    echo -e "${YELLOW}  前端依赖缺失，执行 npm ci${NC}"
    (cd src-ui && npm ci)
fi
echo -e "${GREEN}  ✓ 环境就绪${NC}"

# 更新包签名：Tauri CLI 只读取这些环境变量（内容，不是路径）
DEFAULT_SIGN_KEY="$HOME/.config/auroradb/updater.key"
DEFAULT_SIGN_PASS="$HOME/.config/auroradb/updater.password"
if [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
    if [ -f "$DEFAULT_SIGN_KEY" ]; then
        export TAURI_SIGNING_PRIVATE_KEY="$(cat "$DEFAULT_SIGN_KEY")"
        echo -e "${GREEN}  ✓ 已读取更新签名私钥${NC}"
    else
        echo -e "${YELLOW}  提示: 未设置 TAURI_SIGNING_PRIVATE_KEY，本次构建不会生成更新签名${NC}"
    fi
fi
if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ] && [ -z "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" ] && [ -f "$DEFAULT_SIGN_PASS" ]; then
    export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(tr -d '\r\n' < "$DEFAULT_SIGN_PASS")"
fi

echo -e "${GREEN}[3/4]${NC} 前端构建 + Tauri 打包（首次编译较慢）..."
BUILD_LOG="$(mktemp -t auroradb-macos-build.XXXXXX.log)"
if ! (cd src-ui && npm run build) 2>&1 | tee "$BUILD_LOG"; then
    echo -e "${RED}  ✗ 前端构建失败（日志: $BUILD_LOG）${NC}"
    tail -20 "$BUILD_LOG"
    exit 1
fi
if ! cargo tauri build --target "$TARGET" --bundles "$BUNDLES" 2>&1 | tee -a "$BUILD_LOG"; then
    echo -e "${RED}  ✗ 打包失败（日志: $BUILD_LOG）${NC}"
    if grep -q "Wrong password for that key" "$BUILD_LOG"; then
        echo -e "${YELLOW}  原因: 更新签名密码与私钥不匹配${NC}"
    else
        tail -20 "$BUILD_LOG"
    fi
    exit 1
fi
rm -f "$BUILD_LOG"

BUNDLE_DIR="src-tauri/target/$TARGET/release/bundle"
DMG="$(find "$BUNDLE_DIR/dmg" -maxdepth 1 -name '*.dmg' 2>/dev/null | head -1)"
APP_TAR="$(find "$BUNDLE_DIR/macos" -maxdepth 1 -name '*.app.tar.gz' 2>/dev/null | head -1)"

echo -e "${GREEN}[4/4]${NC} 产物:"
if [[ -n "$APP_TAR" ]]; then
    echo "  更新包: $APP_TAR ($(du -h "$APP_TAR" | cut -f1))"
    [[ -f "$APP_TAR.sig" ]] && echo "  签名:   $APP_TAR.sig" || echo -e "${YELLOW}  ⚠ 缺少 .sig，应用内更新不可用${NC}"
else
    echo -e "${YELLOW}  ⚠ 未生成 .app.tar.gz（--bundles 需包含 app）${NC}"
fi
if [[ -n "$DMG" ]]; then
    echo "  安装包: $DMG ($(du -h "$DMG" | cut -f1))"
else
    echo -e "${YELLOW}  ⚠ 未生成 dmg（--bundles 需包含 dmg）${NC}"
fi

echo ""
echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}    打包完成${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo -e "${YELLOW}未签名分发提示: 首次打开会被 Gatekeeper 拦截，用户需右键→打开，"
echo -e "或执行 xattr -dr com.apple.quarantine /Applications/AuroraDB.app${NC}"
