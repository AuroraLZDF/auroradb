#!/usr/bin/env bash

# 将 macOS 构建产物上传到 GitHub 发行版（Releases），并合并 darwin-aarch64 清单条目
#
# 用法（在仓库根目录执行）:
#   GITHUB_TOKEN=<令牌> ./tools/publish/macos.sh
#   GITHUB_TOKEN=<令牌> ./tools/publish/macos.sh --tag v2.2.3 --notes "修复若干问题"
#   ./tools/publish/macos.sh --dry-run          # 只在本机校验并打印发布计划
#   ./tools/publish/macos.sh --yes              # 跳过交互确认（CI/自动化用）
#
# 选项:
#   --dmg <路径>        安装包（默认取 target/aarch64-apple-darwin/release/bundle/dmg/*.dmg）
#   --app-tar <路径>    应用内更新包（默认取 .../bundle/macos/*.app.tar.gz）
#   --target <triple>   构建目标架构，默认 aarch64-apple-darwin
#
# 说明:
# - latest.json 里 macOS 的 key 是 darwin-aarch64（Apple Silicon）；更新包地址指向
#   AuroraDB.app.tar.gz，签名取同名 .sig（Tauri updater 用它验签，与 Apple 签名无关）。
# - 本次分发为未签名预览版：首次打开需绕过 Gatekeeper（见 docs/RELEASING.md）。

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

TAG=""
NOTES=""
RELEASE_REPO=""
DMG=""
APP_TAR=""
TARGET="aarch64-apple-darwin"
DRY_RUN=0
ASSUME_YES=0

while [[ $# -gt 0 ]]; do
    case $1 in
        --tag) TAG="$2"; shift 2 ;;
        --notes) NOTES="$2"; shift 2 ;;
        --repo) RELEASE_REPO="$2"; shift 2 ;;
        --dmg) DMG="$2"; shift 2 ;;
        --app-tar) APP_TAR="$2"; shift 2 ;;
        --target) TARGET="$2"; shift 2 ;;
        --dry-run) DRY_RUN=1; shift ;;
        -y|--yes) ASSUME_YES=1; shift ;;
        -h|--help) sed -n '3,20p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo -e "${RED}未知参数: $1${NC}"; exit 1 ;;
    esac
done

TOKEN="${GITHUB_TOKEN:-${GH_TOKEN:-}}"
if (( DRY_RUN == 0 )) && [[ -z "$TOKEN" ]]; then
    echo -e "${RED}错误: 未设置 GITHUB_TOKEN 环境变量${NC}"
    exit 1
fi

command -v curl >/dev/null || { echo -e "${RED}错误: 未找到 curl${NC}"; exit 1; }
command -v python3 >/dev/null || { echo -e "${RED}错误: 未找到 python3${NC}"; exit 1; }

REMOTE_URL="$(git remote get-url origin)"
PATH_PART="${REMOTE_URL##*[:/]}"
OWNER_PART="${REMOTE_URL%/*}"
OWNER="${OWNER_PART##*[:/]}"
REPO="${PATH_PART%.git}"
RELEASE_REPO="${RELEASE_REPO:-$OWNER/$REPO}"
OWNER="${RELEASE_REPO%%/*}"
REPO="${RELEASE_REPO##*/}"

VERSION="$(grep -m1 '"version"' src-tauri/tauri.conf.json | sed 's/.*"version": *"\([^"]*\)".*/\1/')"
TAG="${TAG:-v$VERSION}"
RELEASE_NAME="AuroraDB $VERSION"
NOTES="${NOTES:-AuroraDB $VERSION 版本发布}"

# 平台 key 由架构决定：darwin-aarch64 / darwin-x86_64
case "$TARGET" in
    aarch64-apple-darwin) PLATFORM="darwin-aarch64" ;;
    x86_64-apple-darwin) PLATFORM="darwin-x86_64" ;;
    *) echo -e "${RED}错误: 不支持的 target: $TARGET${NC}"; exit 1 ;;
esac

BUNDLE_DIR="src-tauri/target/$TARGET/release/bundle"
API="https://api.github.com/repos/$OWNER/$REPO"
UPLOAD_API="https://uploads.github.com/repos/$OWNER/$REPO"
DOWNLOAD_PREFIX="https://github.com/$OWNER/$REPO/releases/download/"
MAX_SIZE=$((2 * 1024 * 1024 * 1024))

[[ -n "$DMG" ]] || DMG="$(find "$BUNDLE_DIR/dmg" -maxdepth 1 -name '*.dmg' 2>/dev/null | sort | head -1)"
[[ -n "$APP_TAR" ]] || APP_TAR="$(find "$BUNDLE_DIR/macos" -maxdepth 1 -name '*.app.tar.gz' 2>/dev/null | sort | head -1)"

if [[ -z "$DMG" || ! -f "$DMG" ]]; then
    echo -e "${RED}错误: 未找到 dmg，请先执行 ./tools/build/macos.sh${NC}"
    exit 1
fi
if [[ -z "$APP_TAR" || ! -f "$APP_TAR" ]]; then
    echo -e "${RED}错误: 未找到 .app.tar.gz（应用内更新包），请用 --bundles app,dmg 构建${NC}"
    exit 1
fi
if [[ ! -f "$APP_TAR.sig" ]]; then
    echo -e "${RED}错误: 未找到 $(basename "$APP_TAR").sig，构建时未签名则无法应用内更新${NC}"
    exit 1
fi

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

APP_TAR_URL="$DOWNLOAD_PREFIX$TAG/$(basename "$APP_TAR")"
APP_TAR_SHA256="$(sha256_of "$APP_TAR")"
DMG_SIZE="$(wc -c < "$DMG")"
if (( DMG_SIZE > MAX_SIZE )); then
    echo -e "${RED}错误: dmg 超过 GitHub 单文件 2GB 上限${NC}"
    exit 1
fi

TMP="$(mktemp)"
LATEST_EXISTING="$(mktemp)"
LATEST_NEW="$(mktemp)"
trap 'rm -f "$TMP" "$LATEST_EXISTING" "$LATEST_NEW"' EXIT

GH_CODE=""
gh_api() {
    local method="$1" url="$2"
    shift 2
    GH_CODE="$(curl -sS -X "$method" -o "$TMP" -w '%{http_code}' \
        -H "Authorization: Bearer $TOKEN" \
        -H "Accept: application/vnd.github+json" \
        -H "X-GitHub-Api-Version: 2022-11-28" \
        "$@" "$url" 2>/dev/null)" || {
        echo -e "${RED}  ✗ 无法连接 GitHub（$method $url）${NC}"
        exit 1
    }
}

json_get() {
    python3 - "$1" "$2" <<'PY'
import json, sys
with open(sys.argv[1], encoding='utf-8') as f:
    data = json.load(f)
for key in sys.argv[2].split('.'):
    data = data.get(key) if isinstance(data, dict) else None
print('' if data is None else data)
PY
}

MERGE_ARGS=(--output "$LATEST_NEW"
    --version "$VERSION" --notes "$NOTES" --tag "$TAG"
    --platform "$PLATFORM" --artifact-url "$APP_TAR_URL"
    --artifact-sha256 "$APP_TAR_SHA256"
    --signature-file "$APP_TAR.sig")

# 本平台签名清单：只覆盖本次 tag，避免旧条目残留
SUMS_FILE="$(mktemp)"
{
    echo "$(sha256_of "$DMG")  $(basename "$DMG")"
    echo "$(sha256_of "$APP_TAR")  $(basename "$APP_TAR")"
    if [[ -f "$APP_TAR.sig" ]]; then
        echo "$(sha256_of "$APP_TAR.sig")  $(basename "$APP_TAR").sig"
    fi
} > "$SUMS_FILE"

echo ""
echo "========================================"
echo "  发布前确认"
echo "========================================"
echo "  版本: $VERSION"
echo "  标签: $TAG"
echo "  平台: macOS ($PLATFORM)"
echo "  dmg:  $(basename "$DMG") ($((DMG_SIZE / 1024 / 1024))MB)"
echo "  更新包: $(basename "$APP_TAR")"

if (( DRY_RUN == 1 )); then
    echo -e "${YELLOW}  dry-run: 跳过交互确认，也不会上传或联网${NC}"
elif (( ASSUME_YES == 1 )); then
    echo -e "${YELLOW}  --yes: 跳过交互确认（CI 用）${NC}"
else
    read -r -p "  确认继续？输入 yes 继续，其他任意输入取消: " CONFIRM || CONFIRM=""
    [[ "$CONFIRM" == "yes" ]] || { echo "已取消发布"; exit 0; }
fi

if (( DRY_RUN == 1 )); then
    echo 'null' > "$LATEST_EXISTING"
    python3 tools/merge-latest.py --existing "$LATEST_EXISTING" "${MERGE_ARGS[@]}"
    echo -e "${GREEN}  latest.json 合并预览:${NC}"
    sed 's/^/    /' "$LATEST_NEW"
    echo ""
    echo -e "${GREEN}dry-run 通过${NC}"
    rm -f "$SUMS_FILE"
    exit 0
fi

# 1. 读取现有发行版（没有就基于 tag 创建）
gh_api GET "$API/releases/tags/$TAG"
RELEASE_ID=""
if [[ "$GH_CODE" == "200" ]]; then
    RELEASE_ID="$(json_get "$TMP" id)"
    echo -e "${GREEN}  发行版已存在 (id=$RELEASE_ID)，追加附件${NC}"
elif [[ "$GH_CODE" != "404" ]]; then
    echo -e "${RED}  ✗ 查询发行版失败 (HTTP $GH_CODE)${NC}"; cat "$TMP"; exit 1
fi

if [[ -z "$RELEASE_ID" ]]; then
    BODY="$(mktemp)"
    python3 - "$BODY" "$TAG" "$RELEASE_NAME" "$NOTES" <<'PY'
import json, sys
out, tag, name, notes = sys.argv[1:5]
json.dump({'tag_name': tag, 'name': name, 'body': notes, 'draft': False, 'prerelease': False},
          open(out, 'w', encoding='utf-8'), ensure_ascii=False)
PY
    gh_api POST "$API/releases" -H 'Content-Type: application/json' --data-binary "@$BODY"
    rm -f "$BODY"
    [[ "$GH_CODE" == "201" ]] || { echo -e "${RED}  ✗ 创建发行版失败 (HTTP $GH_CODE)${NC}"; cat "$TMP"; exit 1; }
    RELEASE_ID="$(json_get "$TMP" id)"
    echo -e "${GREEN}  已创建发行版 (id=$RELEASE_ID)${NC}"
fi

# 2. 删除同名旧附件后上传（重跑发布时避免 422 重名冲突）
for f in "$DMG" "$APP_TAR" "$APP_TAR.sig" "$SUMS_FILE"; do
    NAME="$(basename "$f")"
    [[ "$NAME" == "$(basename "$SUMS_FILE")" ]] && NAME="SHA256SUMS-macos"
    gh_api GET "$API/releases/$RELEASE_ID/assets?per_page=100"
    ASSET_ID="$(python3 - "$TMP" "$NAME" <<'PY'
import json, sys
data = json.load(open(sys.argv[1], encoding='utf-8'))
name = sys.argv[2]
for a in (data if isinstance(data, list) else []):
    if a.get('name') == name:
        print(a['id']); break
PY
)"
    if [[ -n "$ASSET_ID" ]]; then
        gh_api DELETE "$API/releases/assets/$ASSET_ID"
        echo -e "${YELLOW}  已删除同名旧附件 $NAME${NC}"
    fi
    UPLOAD_NAME="$NAME"
    gh_api POST "$UPLOAD_API/releases/$RELEASE_ID/assets?name=$UPLOAD_NAME" \
        -H 'Content-Type: application/octet-stream' --data-binary "@$f"
    if [[ "$GH_CODE" != "201" ]]; then
        echo -e "${RED}  ✗ 上传 $NAME 失败 (HTTP $GH_CODE)${NC}"; cat "$TMP"; exit 1
    fi
    echo -e "${GREEN}  ✓ 已上传 $NAME${NC}"
done
rm -f "$SUMS_FILE"

# 3. 合并 latest.json：先取线上清单，只替换本平台条目
gh_api GET "$API/releases/latest"
if [[ "$GH_CODE" == "200" ]]; then
    ASSET_ID="$(python3 - "$TMP" <<'PY'
import json, sys
data = json.load(open(sys.argv[1], encoding='utf-8'))
for a in data.get('assets') or []:
    if a.get('name') == 'latest.json':
        print(a['id']); break
PY
)"
    if [[ -n "$ASSET_ID" ]]; then
        curl -sSL -H "Authorization: Bearer $TOKEN" -o "$LATEST_EXISTING" \
            -H 'Accept: application/octet-stream' "$API/releases/assets/$ASSET_ID" || true
    fi
fi
python3 tools/merge-latest.py --existing "$LATEST_EXISTING" "${MERGE_ARGS[@]}"

gh_api GET "$API/releases/$RELEASE_ID/assets?per_page=100"
ASSET_ID="$(python3 - "$TMP" <<'PY'
import json, sys
data = json.load(open(sys.argv[1], encoding='utf-8'))
for a in (data if isinstance(data, list) else []):
    if a.get('name') == 'latest.json':
        print(a['id']); break
PY
)"
if [[ -n "$ASSET_ID" ]]; then
    gh_api DELETE "$API/releases/assets/$ASSET_ID"
fi
gh_api POST "$UPLOAD_API/releases/$RELEASE_ID/assets?name=latest.json" \
    -H 'Content-Type: application/octet-stream' --data-binary "@$LATEST_NEW"
[[ "$GH_CODE" == "201" ]] || { echo -e "${RED}  ✗ 上传 latest.json 失败 (HTTP $GH_CODE)${NC}"; cat "$TMP"; exit 1; }

echo ""
echo -e "${GREEN}✓ macOS 发布完成: $TAG ($PLATFORM)${NC}"
echo "  更新包: $APP_TAR_URL"
echo "  清单:   https://github.com/$OWNER/$REPO/releases/latest/download/latest.json"
