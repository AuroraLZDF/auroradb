#!/usr/bin/env bash

# 将 Linux 构建产物上传到 GitHub 发行版（Releases）
#
# 用法（在仓库根目录执行）:
#   GITHUB_TOKEN=<令牌> ./tools/publish/linux.sh
#   GITHUB_TOKEN=<令牌> ./tools/publish/linux.sh --tag v1.0.1 --notes "修复若干问题"
#   ./tools/publish/linux.sh --repo owner/repo      # 指定公开发布仓库
#   ./tools/publish/linux.sh --dry-run              # 只在本机校验并打印发布计划
#
# 发布目标默认为 origin 同名的 GitHub 公开仓库（如 AuroraLZDF/auroradb）。
# 安装包、deb、SHA256SUMS 与 latest.json 全部作为发行版附件上传，
# latest.json 的固定入口为 https://github.com/<owner>/<repo>/releases/latest/download/latest.json
#
# 选项:
#   --appimage <路径>        更新用 AppImage（默认 bundle/appimage 下第一个）
#   --extra-artifact <路径>  额外上传文件，可重复（如 lite AppImage 及其 .sig）
#
# 令牌需在 GitHub「Settings → Developer settings → Personal access tokens」生成，
# 经典令牌勾选 repo，或细粒度令牌授予目标仓库 Contents: Read and write。
# 令牌只从环境变量读取，不会写入仓库。
# Windows 产物由 tools/publish/windows.ps1 发布。

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
APPIMAGE=""
DRY_RUN=0
EXTRA_ARTIFACTS=()
while [[ $# -gt 0 ]]; do
    case $1 in
        --tag) TAG="$2"; shift 2 ;;
        --notes) NOTES="$2"; shift 2 ;;
        --repo) RELEASE_REPO="$2"; shift 2 ;;
        --appimage) APPIMAGE="$2"; shift 2 ;;
        --extra-artifact) EXTRA_ARTIFACTS+=("$2"); shift 2 ;;
        --dry-run) DRY_RUN=1; shift ;;
        -h|--help)
            sed -n '3,22p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) echo -e "${RED}未知参数: $1${NC}"; exit 1 ;;
    esac
done

TOKEN="${GITHUB_TOKEN:-${GH_TOKEN:-}}"
# dry-run 只做本地校验与计划输出，不需要令牌；正式发布会先拦下缺少令牌的情况
if (( DRY_RUN == 0 )) && [[ -z "$TOKEN" ]]; then
    echo -e "${RED}错误: 未设置 GITHUB_TOKEN 环境变量${NC}"
    echo "  请先执行: export GITHUB_TOKEN=<你的 GitHub 令牌>"
    exit 1
fi

command -v curl >/dev/null || { echo -e "${RED}错误: 未找到 curl${NC}"; exit 1; }
command -v python3 >/dev/null || { echo -e "${RED}错误: 未找到 python3${NC}"; exit 1; }
command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 \
    || { echo -e "${RED}错误: 未找到 sha256sum/shasum，无法计算安装包校验值${NC}"; exit 1; }

# 从 origin 解析 owner/repo，兼容 ssh 与 https 两种地址写法
REMOTE_URL="$(git remote get-url origin)"
PATH_PART="${REMOTE_URL##*[:/]}"
OWNER_PART="${REMOTE_URL%/*}"
OWNER="${OWNER_PART##*[:/]}"
REPO="${PATH_PART%.git}"
if [[ -z "$OWNER" || -z "$REPO" ]]; then
    echo -e "${RED}错误: 无法从远程地址解析仓库: $REMOTE_URL${NC}"
    exit 1
fi

# 分发目标：默认与 origin 同名的公开仓库，可用 --repo 覆盖
RELEASE_REPO="${RELEASE_REPO:-$OWNER/$REPO}"
if [[ "$RELEASE_REPO" != */* ]]; then
    echo -e "${RED}错误: --repo 需为 owner/repo 格式，当前为 $RELEASE_REPO${NC}"
    exit 1
fi
OWNER="${RELEASE_REPO%%/*}"
REPO="${RELEASE_REPO##*/}"

VERSION="$(grep -m1 '"version"' src-tauri/tauri.conf.json | sed 's/.*"version": *"\([^"]*\)".*/\1/')"
TAG="${TAG:-v$VERSION}"
RELEASE_NAME="AuroraDB $VERSION"
NOTES="${NOTES:-AuroraDB $VERSION 版本发布}"

API="https://api.github.com/repos/$OWNER/$REPO"
UPLOAD_API="https://uploads.github.com/repos/$OWNER/$REPO"
DOWNLOAD_PREFIX="https://github.com/$OWNER/$REPO/releases/download/"
BUNDLE_DIR="src-tauri/target/release/bundle"
MAX_SIZE=$((2 * 1024 * 1024 * 1024))

if [[ ! -d "$BUNDLE_DIR" ]]; then
    echo -e "${RED}错误: 未找到构建产物目录 $BUNDLE_DIR，请先执行 ./tools/build/linux.sh${NC}"
    exit 1
fi

TMP="$(mktemp)"
LATEST_TMP="$(mktemp)"
LATEST_EXISTING="$(mktemp)"
# trap 在 EXIT 时展开这些变量，set -u 下必须先初始化，否则早期失败会报 unbound
trap 'rm -f "$TMP" "$LATEST_TMP" "$LATEST_EXISTING"' EXIT

# 计算文件 SHA-256（小写十六进制）：写入 latest.json 供应用内下载的安装包校验完整性
sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

# 从 JSON 文件取字段：json_get 文件 点分路径（如 id、default_branch、platforms.linux-x86_64.url）
json_get() {
    python3 - "$1" "$2" <<'PY'
import json
import sys

with open(sys.argv[1], encoding='utf-8') as f:
    data = json.load(f)
for key in sys.argv[2].split('.'):
    if isinstance(data, dict):
        data = data.get(key)
    else:
        data = None
        break
if data is None:
    print('')
elif isinstance(data, (dict, list)):
    print(json.dumps(data, ensure_ascii=False))
else:
    print(data)
PY
}

# 调用 GitHub API：gh_api 方法 路径 [额外 curl 参数...]
# 响应写入 $TMP，HTTP 状态码通过全局变量 GH_CODE 返回
GH_CODE=""
gh_api() {
    local method="$1" url="$2"
    shift 2
    GH_CODE="$(curl -sS -X "$method" -o "$TMP" -w '%{http_code}' \
        -H "Authorization: Bearer $TOKEN" \
        -H "Accept: application/vnd.github+json" \
        -H "X-GitHub-Api-Version: 2022-11-28" \
        "$@" "$url")"
}

# ---- 解析更新包与上传集 ----
# 完整 AppImage（应用内更新包）：显式路径优先，否则取 bundle/appimage 下第一个
ARTIFACT=""
if [[ -n "$APPIMAGE" ]]; then
    ARTIFACT="$APPIMAGE"
else
    while IFS= read -r f; do
        ARTIFACT="$f"
        break
    done < <(find "$BUNDLE_DIR/appimage" -maxdepth 1 -type f -name '*.AppImage' 2>/dev/null | sort)
fi
if [[ -z "$ARTIFACT" ]]; then
    echo -e "${RED}错误: 未找到 AppImage 更新包，可用 --appimage 指定${NC}"
    exit 1
fi
if [[ ! -f "$ARTIFACT" ]]; then
    echo -e "${RED}错误: 更新包不存在: $ARTIFACT${NC}"
    exit 1
fi
if [[ ! -f "$ARTIFACT.sig" ]]; then
    echo -e "${RED}错误: 未找到 $(basename "$ARTIFACT").sig，请确认构建时已签名${NC}"
    exit 1
fi

ARTIFACT_URL="$DOWNLOAD_PREFIX$TAG/$(basename "$ARTIFACT")"
# 清单里带上校验值：用户手动下载 AppImage/deb 时也能核对完整性
ARTIFACT_SHA256="$(sha256_of "$ARTIFACT")"

# 显式模式：只要用了新参数，就按显式上传集发布，不再把 bundle 下所有产物都传上去
EXPLICIT_MODE=0
if [[ -n "$APPIMAGE" || ${#EXTRA_ARTIFACTS[@]} -gt 0 ]]; then
    EXPLICIT_MODE=1
fi

# deb 安装的应用无法用 AppImage 原地升级，清单里额外给出 deb 包地址，
# 应用会把它下载到下载目录交给用户手动安装
DEB_URL=""
DEB_SIG=""
DEB_SHA256=""
DEB=""
while IFS= read -r f; do
    DEB="$f"
    break
done < <(find "$BUNDLE_DIR/deb" -maxdepth 1 -type f -name '*.deb' 2>/dev/null | sort)

if [[ -n "$DEB" ]]; then
    DEB_URL="$DOWNLOAD_PREFIX$TAG/$(basename "$DEB")"
    DEB_SHA256="$(sha256_of "$DEB")"
    [[ -f "$DEB.sig" ]] && DEB_SIG="$DEB.sig"
else
    echo -e "${YELLOW}  注意: 未找到 deb 安装包，deb 安装的用户将无法在应用内下载更新${NC}"
fi

# 额外上传文件（lite AppImage 等）：必须存在；同名 .sig 一并上传
for extra in ${EXTRA_ARTIFACTS[@]+"${EXTRA_ARTIFACTS[@]}"}; do
    if [[ ! -f "$extra" ]]; then
        echo -e "${RED}错误: --extra-artifact 文件不存在: $extra${NC}"
        exit 1
    fi
done

UPLOAD_FILES=()
add_upload() {
    local f="$1" existing
    [[ -n "$f" && -f "$f" ]] || return 0
    for existing in ${UPLOAD_FILES[@]+"${UPLOAD_FILES[@]}"}; do
        [[ "$existing" == "$f" ]] && return 0
    done
    UPLOAD_FILES+=("$f")
}

add_upload "$ARTIFACT"
add_upload "$ARTIFACT.sig"
add_upload "$DEB"
add_upload "$DEB_SIG"
for extra in ${EXTRA_ARTIFACTS[@]+"${EXTRA_ARTIFACTS[@]}"}; do
    add_upload "$extra"
    add_upload "$extra.sig"
done

# 发布前人工确认：上传后发行版与 latest.json 立即对用户生效，先让发版人核对代码状态
echo ""
echo "========================================"
echo "  发布前确认"
echo "========================================"
echo "  版本: $VERSION"
echo "  标签: $TAG"
echo "  平台: linux"

BRANCH="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo 'unknown')"
echo "  分支: $BRANCH"
echo "  提交: $(git log -1 --oneline 2>/dev/null || echo 'unknown')"

if [[ -n "$(git status --porcelain 2>/dev/null)" ]]; then
    echo -e "${YELLOW}  ! 工作区有未提交的改动，确认这些改动不影响本次发版${NC}"
else
    echo -e "${GREEN}  ✓ 工作区干净${NC}"
fi

UPSTREAM="$(git rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>/dev/null || true)"
if [[ -n "$UPSTREAM" ]]; then
    AHEAD="$(git rev-list --count "$UPSTREAM..HEAD" 2>/dev/null || echo 0)"
    if (( AHEAD > 0 )); then
        echo -e "${YELLOW}  ! 有 $AHEAD 个提交未推送到 $UPSTREAM${NC}"
        echo "    推送: git push origin $BRANCH"
    else
        echo -e "${GREEN}  ✓ 代码已推送到 $UPSTREAM${NC}"
    fi
else
    echo -e "${YELLOW}  ! 当前分支未设置上游分支，无法判断是否已推送${NC}"
fi

if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null 2>&1; then
    TAG_COMMIT="$(git rev-list -n1 "$TAG")"
    HEAD_COMMIT="$(git rev-parse HEAD)"
    if [[ "$TAG_COMMIT" != "$HEAD_COMMIT" ]]; then
        echo -e "${YELLOW}  ! 本地 tag $TAG 指向 ${TAG_COMMIT:0:7}，不是当前 HEAD ${HEAD_COMMIT:0:7}${NC}"
    fi
    if (( DRY_RUN == 1 )); then
        echo -e "${YELLOW}  - dry-run 跳过远端 tag 检查（不联网）${NC}"
    elif git ls-remote --tags "https://github.com/$OWNER/$REPO.git" "refs/tags/$TAG" 2>/dev/null | grep -q "refs/tags/$TAG"; then
        echo -e "${GREEN}  ✓ tag $TAG 已推送到 GitHub${NC}"
    else
        echo -e "${YELLOW}  ! tag $TAG 尚未推送到 GitHub（发行版会自动基于当前分支创建 tag）${NC}"
        echo "    推送: git push https://github.com/$OWNER/$REPO.git $TAG"
    fi
else
    echo -e "${YELLOW}  ! 本地不存在 tag $TAG${NC}"
    echo "    创建并推送: git tag $TAG && git push origin $TAG"
fi

echo ""
if (( DRY_RUN == 1 )); then
    echo -e "${YELLOW}  dry-run: 跳过交互确认，也不会上传或联网${NC}"
else
    echo "  注意: 继续后将上传安装包并更新 latest.json，对终端用户立即可见。"
    read -r -p "  确认继续？输入 yes 继续，其他任意输入取消: " CONFIRM || CONFIRM=""
    if [[ "$CONFIRM" != "yes" ]]; then
        echo "已取消发布"
        exit 0
    fi
fi
echo ""

echo -e "${YELLOW}发布仓库: $OWNER/$REPO  版本: $TAG${NC}"

# 上线前先自检：产物齐全、签名存在、不超限、更新地址一致，并生成 SHA256SUMS
CHECK_ARGS=(--platform linux --repo "$OWNER/$REPO" --bundle-dir "$BUNDLE_DIR")
if (( EXPLICIT_MODE == 1 )); then
    CHECK_ARGS+=(--appimage "$ARTIFACT")
    for f in "${UPLOAD_FILES[@]}"; do
        CHECK_ARGS+=(--upload-file "$f")
    done
fi
bash tools/check-release.sh "${CHECK_ARGS[@]}"

# 上传清单：显式模式下只传 UPLOAD_FILES + SHA256SUMS；
# 默认模式下沿用原行为，把 bundle 下所有可发布产物都上传
FILES=()
if (( EXPLICIT_MODE == 1 )); then
    add_upload "$BUNDLE_DIR/SHA256SUMS"
    FILES=("${UPLOAD_FILES[@]}")
else
    while IFS= read -r f; do
        FILES+=("$f")
    done < <(find "$BUNDLE_DIR" -maxdepth 2 -type f \
        \( -name '*.deb' -o -name '*.rpm' -o -name '*.AppImage' \
           -o -name '*.sig' -o -name 'SHA256SUMS' \) | sort)
fi

if [[ ${#FILES[@]} -eq 0 ]]; then
    echo -e "${RED}错误: 没有可上传的产物${NC}"
    exit 1
fi

# 构造 latest.json 合并参数：Linux 与 Windows 分开发布，清单按平台 key 合并，互不覆盖
MERGE_ARGS=(--output "$LATEST_TMP"
    --version "$VERSION" --notes "$NOTES" --tag "$TAG"
    --platform "linux-x86_64" --artifact-url "$ARTIFACT_URL"
    --artifact-sha256 "$ARTIFACT_SHA256"
    --signature-file "$ARTIFACT.sig")
if [[ -n "$DEB_URL" ]]; then
    MERGE_ARGS+=(--deb-url "$DEB_URL" --deb-sha256 "$DEB_SHA256")
    [[ -n "$DEB_SIG" ]] && MERGE_ARGS+=(--deb-signature-file "$DEB_SIG")
fi

# dry-run：打印完整发布计划 + 本地生成 latest.json 预览，然后退出（不联网、不上传）
if (( DRY_RUN == 1 )); then
    echo ""
    echo -e "${YELLOW}== dry-run 发布计划（不联网、不上传、不写远端）==${NC}"
    echo "  更新包（latest.json 的 linux url）: $ARTIFACT_URL"
    echo "    本地文件: $ARTIFACT"
    echo "    SHA-256:  $ARTIFACT_SHA256"
    echo "    签名:     $ARTIFACT.sig"
    if [[ -n "$DEB" ]]; then
        echo "  deb: $DEB"
        echo "    url: $DEB_URL"
        echo "    SHA-256: $DEB_SHA256"
        [[ -n "$DEB_SIG" ]] && echo "    签名: $DEB_SIG"
    else
        echo "  deb: 无（deb 用户无法应用内下载更新）"
    fi
    for extra in ${EXTRA_ARTIFACTS[@]+"${EXTRA_ARTIFACTS[@]}"}; do
        echo "  额外上传: $extra"
        [[ -f "$extra.sig" ]] && echo "    签名: $extra.sig"
    done
    echo "  将上传到 GitHub 的附件:"
    for f in "${FILES[@]}"; do
        SIZE="$(stat -c %s "$f")"
        echo "    - $(basename "$f") ($((SIZE / 1024 / 1024))MB)"
    done
    echo "  SHA256SUMS: $BUNDLE_DIR/SHA256SUMS"
    echo ""

    # 本地试合并：用 null 作为线上旧清单，检查产物是否可生成
    echo 'null' > "$LATEST_EXISTING"
    python3 tools/merge-latest.py --existing "$LATEST_EXISTING" "${MERGE_ARGS[@]}"
    echo -e "${GREEN}  latest.json 合并预览:${NC}"
    sed 's/^/    /' "$LATEST_TMP"
    echo ""
    echo -e "${GREEN}dry-run 通过：以上命令在正式执行时才会访问网络${NC}"
    exit 0
fi

# 0. 校验发布仓库可访问：私有仓库的发行版附件对终端用户不可见，必须发布到公开仓库
gh_api GET "$API"
if [[ "$GH_CODE" != "200" ]]; then
    echo -e "${RED}错误: 无法访问发布仓库 $OWNER/$REPO (HTTP $GH_CODE)${NC}"
    echo "  GitHub 仓库需为公开仓库，且令牌需有 Contents: Read and write 权限"
    cat "$TMP"
    exit 1
fi
TARGET_BRANCH="$(json_get "$TMP" default_branch)"
TARGET_BRANCH="${TARGET_BRANCH:-main}"
# 仓库全名使用 GitHub 上的规范大小写，保证下载地址与更新源比对一致
FULL_NAME="$(json_get "$TMP" full_name)"
if [[ -n "$FULL_NAME" ]]; then
    OWNER="${FULL_NAME%%/*}"
    REPO="${FULL_NAME##*/}"
    DOWNLOAD_PREFIX="https://github.com/$OWNER/$REPO/releases/download/"
fi

# latest.json 的固定入口是 /releases/latest，若已存在更高版本的发行版，
# 重新发布旧 tag 会让所有客户端读到回退后的清单，这里直接中止
gh_api GET "$API/releases/latest"
if [[ "$GH_CODE" == "200" ]]; then
    LATEST_TAG_NAME="$(json_get "$TMP" tag_name)"
    if [[ -n "$LATEST_TAG_NAME" && "$LATEST_TAG_NAME" != "$TAG" ]] \
        && python3 - "$LATEST_TAG_NAME" "$TAG" <<'PY'
import sys


def parse(value):
    parts = value.strip().lstrip('v').split('.')
    return tuple(int(p) for p in parts) if len(parts) == 3 and all(p.isdigit() for p in parts) else None


latest, ours = parse(sys.argv[1]), parse(sys.argv[2])
sys.exit(0 if latest and ours and latest > ours else 1)
PY
    then
        echo -e "${RED}✗ 线上最新发行版是 $LATEST_TAG_NAME，高于本次 $TAG${NC}"
        echo "  latest.json 通过 /releases/latest 分发，发布旧版本会让客户端清单回退，已中止"
        exit 1
    fi
fi

# 1. 查找已有发行版（不存在返回 404），否则基于本次 tag 创建
gh_api GET "$API/releases/tags/$TAG"
RELEASE_ID=""
if [[ "$GH_CODE" == "200" ]]; then
    RELEASE_ID="$(json_get "$TMP" id)"
    echo -e "${GREEN}  发行版已存在 (id=$RELEASE_ID)，直接追加附件${NC}"
elif [[ "$GH_CODE" != "404" ]]; then
    echo -e "${RED}  ✗ 查询发行版失败 (HTTP $GH_CODE)${NC}"
    cat "$TMP"
    exit 1
fi

if [[ -z "$RELEASE_ID" ]]; then
    BODY_FILE="$(mktemp)"
    python3 - "$BODY_FILE" "$TAG" "$RELEASE_NAME" "$NOTES" "$TARGET_BRANCH" <<'PY'
import json
import sys

out, tag, name, notes, branch = sys.argv[1:6]
body = {
    'tag_name': tag,
    'name': name,
    'body': notes,
    'target_commitish': branch,
    'draft': False,
    'prerelease': False,
}
with open(out, 'w', encoding='utf-8') as f:
    json.dump(body, f, ensure_ascii=False)
PY
    gh_api POST "$API/releases" \
        -H "Content-Type: application/json" \
        --data-binary "@$BODY_FILE"
    rm -f "$BODY_FILE"
    if [[ "$GH_CODE" != "201" ]]; then
        echo -e "${RED}  ✗ 创建发行版失败 (HTTP $GH_CODE)${NC}"
        cat "$TMP"
        if [[ "$GH_CODE" == "404" || "$GH_CODE" == "403" ]]; then
            echo -e "${YELLOW}  请确认仓库 $OWNER/$REPO 公开可见，且令牌有 Contents: Read and write 权限${NC}"
        fi
        exit 1
    fi
    RELEASE_ID="$(json_get "$TMP" id)"
    echo -e "${GREEN}  已创建发行版 $RELEASE_NAME (id=$RELEASE_ID)${NC}"
fi

if [[ -z "$RELEASE_ID" ]]; then
    echo -e "${RED}  ✗ 无法解析发行版 id，接口原始响应:${NC}"
    cat "$TMP"
    echo ""
    exit 1
fi

# 2. 列出现有附件，同名附件先删除，重复发布时覆盖旧产物
declare -A EXISTING_ASSET=()
gh_api GET "$API/releases/$RELEASE_ID/assets?per_page=100"
if [[ "$GH_CODE" == "200" ]]; then
    while IFS=$'\t' read -r ASSET_ID ASSET_NAME; do
        [[ -n "$ASSET_ID" ]] && EXISTING_ASSET["$ASSET_NAME"]="$ASSET_ID"
    done < <(python3 - "$TMP" <<'PY'
import json
import sys

with open(sys.argv[1], encoding='utf-8') as f:
    for a in json.load(f) or []:
        print(f"{a.get('id', '')}\t{a.get('name', '')}")
PY
)
fi

# 先下载线上 latest.json：合并需要旧内容，且同名附件会在上传前被删除
LATEST_ASSET_ID="${EXISTING_ASSET[latest.json]:-}"
echo 'null' > "$LATEST_EXISTING"
if [[ -n "$LATEST_ASSET_ID" ]]; then
    if ! curl -sS -L -o "$LATEST_EXISTING" \
        -H "Authorization: Bearer $TOKEN" \
        -H "Accept: application/octet-stream" \
        "$API/releases/assets/$LATEST_ASSET_ID"; then
        echo -e "${RED}  ✗ 下载线上 latest.json 失败，中止发布（避免丢失其他平台的更新条目）${NC}"
        exit 1
    fi
    if ! python3 -c 'import json,sys; json.load(open(sys.argv[1]))' "$LATEST_EXISTING" 2>/dev/null; then
        echo -e "${RED}  ✗ 线上 latest.json 不是合法 JSON，中止发布${NC}"
        exit 1
    fi
    # 旧清单附件先删除，最后上传本次合并后的版本
    gh_api DELETE "$API/releases/assets/$LATEST_ASSET_ID"
    if [[ "$GH_CODE" != "204" ]]; then
        echo -e "${RED}  ✗ 删除旧 latest.json 附件失败 (HTTP $GH_CODE)${NC}"
        cat "$TMP"
        exit 1
    fi
fi

for f in "${FILES[@]}"; do
    NAME_ONLY="$(basename "$f")"
    SIZE="$(stat -c %s "$f")"
    if (( SIZE > MAX_SIZE )); then
        echo -e "${RED}  ✗ $NAME_ONLY ($((SIZE / 1024 / 1024))MB) 超过 GitHub 单附件 2GB 上限${NC}"
        exit 1
    fi

    if [[ -n "${EXISTING_ASSET[$NAME_ONLY]:-}" ]]; then
        gh_api DELETE "$API/releases/assets/${EXISTING_ASSET[$NAME_ONLY]}"
        if [[ "$GH_CODE" != "204" ]]; then
            echo -e "${RED}  ✗ 删除同名旧附件 $NAME_ONLY 失败 (HTTP $GH_CODE)${NC}"
            cat "$TMP"
            exit 1
        fi
        echo -e "${YELLOW}  - 已删除同名旧附件 $NAME_ONLY${NC}"
    fi

    ENCODED_NAME="$(python3 -c 'import sys,urllib.parse; print(urllib.parse.quote(sys.argv[1]))' "$NAME_ONLY")"
    GH_CODE="$(curl -sS --retry 3 --retry-delay 2 --retry-all-errors \
        --connect-timeout 30 --max-time 1800 \
        -X POST -o "$TMP" -w '%{http_code}' \
        -H "Authorization: Bearer $TOKEN" \
        -H "Accept: application/vnd.github+json" \
        -H "X-GitHub-Api-Version: 2022-11-28" \
        -H "Content-Type: application/octet-stream" \
        --data-binary "@$f" \
        "$UPLOAD_API/releases/$RELEASE_ID/assets?name=$ENCODED_NAME")"
    if [[ "$GH_CODE" == "201" ]]; then
        echo -e "${GREEN}  ✓ $NAME_ONLY ($((SIZE / 1024 / 1024))MB)${NC}"
        echo "    $DOWNLOAD_PREFIX$TAG/$NAME_ONLY"
    else
        # 任一附件失败都必须中止：继续写 latest.json 会让应用更新到不存在的地址
        echo -e "${RED}  ✗ $NAME_ONLY 上传失败 (HTTP $GH_CODE)${NC}"
        cat "$TMP"
        exit 1
    fi
done

# 上传成功不代表附件可用：写清单前核对远端附件列表，避免出现「有 sig 无安装包」
echo ""
echo -e "${YELLOW}[3/7] 核对远端附件...${NC}"
gh_api GET "$API/releases/$RELEASE_ID/assets?per_page=100"
if [[ "$GH_CODE" != "200" ]]; then
    echo -e "${RED}  ✗ 无法读取发行版附件列表 (HTTP $GH_CODE)${NC}"
    cat "$TMP"
    exit 1
fi
MISSING="$(python3 - "$TMP" "${FILES[@]}" <<'PY'
import json
import os
import sys

with open(sys.argv[1], encoding='utf-8') as f:
    have = {a.get('name') for a in json.load(f) or []}
missing = [os.path.basename(p) for p in sys.argv[2:] if os.path.basename(p) not in have]
print('\n'.join(missing))
PY
)"
if [[ -n "$MISSING" ]]; then
    echo -e "${RED}  ✗ 以下产物不在发行版附件中，已中止发布:${NC}"
    echo "$MISSING" | sed 's/^/    /'
    exit 1
fi
echo -e "${GREEN}  ✓ ${#FILES[@]} 个附件均已就绪${NC}"

# 4. 生成/合并 latest.json 并作为发行版附件上传，作为 App 内自动更新的固定入口
#    入口地址 https://github.com/<owner>/<repo>/releases/latest/download/latest.json
python3 tools/merge-latest.py --existing "$LATEST_EXISTING" "${MERGE_ARGS[@]}"

ENCODED_NAME="$(python3 -c 'import sys,urllib.parse; print(urllib.parse.quote(sys.argv[1]))' "latest.json")"
GH_CODE="$(curl -sS --retry 3 --retry-delay 2 --retry-all-errors \
    --connect-timeout 30 --max-time 300 \
    -X POST -o "$TMP" -w '%{http_code}' \
    -H "Authorization: Bearer $TOKEN" \
    -H "Accept: application/vnd.github+json" \
    -H "X-GitHub-Api-Version: 2022-11-28" \
    -H "Content-Type: application/octet-stream" \
    --data-binary "@$LATEST_TMP" \
    "$UPLOAD_API/releases/$RELEASE_ID/assets?name=$ENCODED_NAME")"

LATEST_URL="https://github.com/$OWNER/$REPO/releases/latest/download/latest.json"
if [[ "$GH_CODE" == "201" ]]; then
    echo -e "${GREEN}✓ 自动更新清单已上传: $LATEST_URL${NC}"
    CONFIGURED_ENDPOINT="$(python3 - <<'PY'
import json

with open('src-tauri/tauri.conf.json', encoding='utf-8') as f:
    data = json.load(f)
endpoints = ((data.get('plugins') or {}).get('updater') or {}).get('endpoints') or []
print(endpoints[0] if endpoints else '')
PY
)"
    if [[ "$CONFIGURED_ENDPOINT" != "$LATEST_URL" ]]; then
        echo -e "${YELLOW}  注意: tauri.conf.json 的更新地址为 $CONFIGURED_ENDPOINT，与本次发布地址不一致${NC}"
    fi
else
    echo -e "${RED}✗ 自动更新清单上传失败 (HTTP $GH_CODE)${NC}"
    cat "$TMP"
    exit 1
fi

echo ""
echo -e "${GREEN}发布完成: https://github.com/$OWNER/$REPO/releases/tag/$TAG${NC}"
