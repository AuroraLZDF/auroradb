#!/usr/bin/env bash

# AuroraDB 一键发布脚本
#
# 依次完成: 计算新版本号 -> 同步版本号到各配置文件 -> 运行检查 -> 提交 -> 打 tag
#          -> 推送代码与 tag -> 构建安装包 -> 上传到 Gitee 发行版并更新 latest.json
#
# 用法:
#   ./release-all.sh                        # 自动 patch +1 (1.0.4 -> 1.0.5)
#   ./release-all.sh --version 1.1.0        # 指定版本号
#   ./release-all.sh --notes "修复若干问题"  # 自定义发行说明（默认自动收集 git log）
#   ./release-all.sh --bundles deb          # 只构建 deb (默认 deb,appimage)
#   ./release-all.sh --skip-checks          # 跳过 type-check 与单元测试
#   ./release-all.sh --dry-run              # 只打印计划, 不做任何改动
#   ./release-all.sh -y                     # 跳过交互确认
#
# 前置条件:
#   1. 工作区干净: 业务改动请先自行提交
#   2. GITEE_TOKEN 环境变量, 或 ~/.config/auroradb/gitee.token
#   3. 更新签名私钥 ~/.config/auroradb/updater.key
#
# 首次发布需在 Gitee 建好公开分发仓库 <owner>/auroradb-releases

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

log()  { echo -e "${BLUE}==>${NC} $*"; }
ok()   { echo -e "${GREEN}  ✓${NC} $*"; }
warn() { echo -e "${YELLOW}  !${NC} $*"; }
die()  { echo -e "${RED}错误: $*${NC}" >&2; exit 1; }

show_help() {
    sed -n '3,21p' "$0" | sed 's/^# \{0,1\}//'
}

VERSION=""
NOTES=""
BUNDLES="deb,appimage"
SKIP_CHECKS=0
DRY_RUN=0
ASSUME_YES=0

while [[ $# -gt 0 ]]; do
    case $1 in
        --version)     VERSION="${2:-}"; shift 2 ;;
        --notes)       NOTES="${2:-}";   shift 2 ;;
        --bundles)     BUNDLES="${2:-}"; shift 2 ;;
        --skip-checks) SKIP_CHECKS=1;    shift ;;
        --dry-run)     DRY_RUN=1;        shift ;;
        -y|--yes)      ASSUME_YES=1;     shift ;;
        -h|--help)     show_help; exit 0 ;;
        *)             die "未知参数: $1（--help 查看用法）" ;;
    esac
done

# ---------- 1. 环境预检 ----------
log "环境预检"

for cmd in git npm cargo curl python3; do
    command -v "$cmd" >/dev/null || die "未找到 $cmd"
done

git rev-parse --is-inside-work-tree >/dev/null 2>&1 || die "当前目录不是 git 仓库"

BRANCH="$(git branch --show-current)"
[[ -n "$BRANCH" ]] || die "当前处于 detached HEAD，请切回分支后再发布"

REMOTE_URL="$(git remote get-url origin 2>/dev/null)" || die "未配置 origin 远程仓库"

if [[ -n "$(git status --porcelain)" ]]; then
    echo -e "${RED}工作区不干净，无法发布：${NC}"
    git status --short
    die "请先提交或暂存上述改动"
fi
ok "工作区干净 ($BRANCH)"

# 版本号以 tauri.conf.json 为准
CURRENT="$(grep -m1 '"version"' src-tauri/tauri.conf.json | sed 's/.*"version": *"\([^"]*\)".*/\1/')"
[[ "$CURRENT" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "无法解析当前版本号: '$CURRENT'"

if [[ -z "$VERSION" ]]; then
    IFS=. read -r MAJOR MINOR PATCH <<< "$CURRENT"
    VERSION="$MAJOR.$MINOR.$((PATCH + 1))"
fi
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "版本号格式需为 X.Y.Z，当前为 '$VERSION'"

TAG="v$VERSION"

if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
    die "本地已存在 tag $TAG，请换一个版本号（--version）"
fi
if git ls-remote --exit-code --tags origin "refs/tags/$TAG" >/dev/null 2>&1; then
    die "远程已存在 tag $TAG，请换一个版本号（--version）"
fi

if [[ -z "${GITEE_TOKEN:-}" ]]; then
    TOKEN_FILE="$HOME/.config/auroradb/gitee.token"
    if [[ -f "$TOKEN_FILE" ]]; then
        export GITEE_TOKEN="$(tr -d '\r\n' < "$TOKEN_FILE")"
    fi
fi
[[ -n "${GITEE_TOKEN:-}" ]] || die "未设置 GITEE_TOKEN（或写入 ~/.config/auroradb/gitee.token）"

SIGN_KEY="$HOME/.config/auroradb/updater.key"
if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" && ! -f "$SIGN_KEY" ]]; then
    die "未找到更新签名私钥: $SIGN_KEY"
fi

# 加密私钥会让 Tauri CLI 交互式索要密码，必须提前提供，否则构建阶段会卡住
if [[ -f "$SIGN_KEY" && -z "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" ]] \
    && base64 -d "$SIGN_KEY" 2>/dev/null | grep -q "encrypted secret key"; then
    [[ -f "$HOME/.config/auroradb/updater.password" ]] \
        || die "签名私钥已加密但未提供密码: 请设置 TAURI_SIGNING_PRIVATE_KEY_PASSWORD 或写入 ~/.config/auroradb/updater.password"
fi

# 未显式传入 --notes 时，自动收集上一个版本 tag 以来的提交作为发行说明
if [[ -z "$NOTES" ]]; then
    LAST_TAG="v$CURRENT"
    CHANGELOG=""
    if git rev-parse -q --verify "refs/tags/$LAST_TAG" >/dev/null 2>&1; then
        CHANGELOG="$(git log --pretty=format:'- %s' "$LAST_TAG..HEAD" 2>/dev/null \
            | grep -vE '^\- (chore\(release\):|Merge)' || true)"
    fi
    if [[ -n "$CHANGELOG" ]]; then
        NOTES="AuroraDB $VERSION 版本发布

$CHANGELOG"
    else
        NOTES="AuroraDB $VERSION 版本发布"
    fi
fi

# ---------- 2. 确认发布计划 ----------
echo ""
echo "========================================"
echo "  发布计划"
echo "========================================"
echo "  仓库    : $REMOTE_URL"
echo "  分支    : $BRANCH"
echo "  版本    : $CURRENT -> $VERSION"
echo "  Tag     : $TAG"
echo "  安装包  : $BUNDLES"
echo "  说明    : $NOTES"
echo "  检查    : $([[ $SKIP_CHECKS -eq 1 ]] && echo '跳过' || echo 'type-check + 单测')"
echo "========================================"
echo ""

if [[ $DRY_RUN -eq 1 ]]; then
    log "--dry-run: 仅打印计划，未做任何改动"
    exit 0
fi

if [[ $ASSUME_YES -eq 0 ]]; then
    read -r -p "确认执行? [y/N] " REPLY
    [[ "$REPLY" =~ ^[Yy]$ ]] || { echo "已取消"; exit 1; }
fi

# ---------- 3. 同步版本号 ----------
log "同步版本号到 4 处配置"

python3 - "$VERSION" <<'PY'
import re
import sys

version = sys.argv[1]

def bump(path, pattern, flags=re.M):
    with open(path, encoding='utf-8') as f:
        text = f.read()
    new, count = re.subn(pattern, lambda m: m.group(1) + version + m.group(3), text, count=1, flags=flags)
    if count != 1:
        sys.exit(f"未能在 {path} 中定位版本号")
    with open(path, 'w', encoding='utf-8') as f:
        f.write(new)

bump('src-tauri/tauri.conf.json', r'("version"\s*:\s*")([^"]*)(")')
bump('src-ui/package.json', r'("version"\s*:\s*")([^"]*)(")')
bump('src-tauri/Cargo.toml', r'^(version\s*=\s*")([^"]*)(")')
bump('src-tauri/Cargo.lock', r'(name = "auroradb"\nversion = ")([^"]*)(")')
PY

for f in src-tauri/tauri.conf.json src-ui/package.json; do
    got="$(grep -m1 '"version"' "$f" | sed 's/.*"version": *"\([^"]*\)".*/\1/')"
    [[ "$got" == "$VERSION" ]] || die "$f 版本号同步失败（当前 $got）"
done
got="$(grep -m1 '^version' src-tauri/Cargo.toml | sed 's/.*"\([^"]*\)".*/\1/')"
[[ "$got" == "$VERSION" ]] || die "Cargo.toml 版本号同步失败（当前 $got）"
ok "版本号已更新为 $VERSION"

# ---------- 4. 检查 ----------
if [[ $SKIP_CHECKS -eq 0 ]]; then
    log "前端类型检查"
    ( cd src-ui && npm run type-check )
    log "前端单元测试"
    ( cd src-ui && npm test )
    ok "检查通过"
else
    warn "已跳过检查"
fi

# ---------- 5. 提交 + 打 tag ----------
log "提交版本号并打 tag"

git add src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock src-ui/package.json
if git diff --cached --quiet; then
    warn "版本号无变化，跳过提交"
else
    git commit -m "chore(release): $TAG"
    ok "已提交 chore(release): $TAG"
fi

git tag -a "$TAG" -m "AuroraDB $VERSION"
ok "已创建 tag $TAG"

# ---------- 6. 推送代码与 tag ----------
log "推送代码与 tag 到远程"
git push origin "$BRANCH"
git push origin "$TAG"
ok "已推送到 $REMOTE_URL ($BRANCH, $TAG)"

# ---------- 7. 构建安装包 ----------
log "构建安装包: $BUNDLES"
./build.sh --bundles "$BUNDLES"
ok "构建完成"

# ---------- 8. 上传发行版 ----------
log "上传发行版到 Gitee"
./release.sh --tag "$TAG" --notes "$NOTES"

# ---------- 9. 汇总 ----------
echo ""
echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}  $TAG 发布完成${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo "安装包目录: src-tauri/target/release/bundle"
echo ""
echo "如后续步骤失败（构建/上传），代码与 tag 已推送，可单独重试:"
echo "  ./build.sh --bundles $BUNDLES"
echo "  ./release.sh --tag $TAG --notes \"$NOTES\""
echo ""
