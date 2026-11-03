#!/usr/bin/env bash

# 将 Linux 构建产物上传到 Gitee 发行版（Releases）
#
# 用法（在仓库根目录执行）:
#   GITEE_TOKEN=<令牌> ./tools/publish/linux.sh
#   GITEE_TOKEN=<令牌> ./tools/publish/linux.sh --tag v1.0.1 --notes "修复若干问题"
#   ./tools/publish/linux.sh --repo owner/auroradb-releases   # 指定公开分发仓库
#   ./tools/publish/linux.sh --dry-run                        # 只在本机校验并打印发布计划
#
# 双层分发（Gitee 发 deb+lite 手动下载包，GitHub 发 full 更新包）推荐顺序:
#   1) ./tools/publish/github-full.sh --tag v$VERSION --appimage <完整 AppImage>
#      → 先拿到 https://github.com/<repo>/releases/download/v$VERSION/<文件>
#   2) GITEE_TOKEN=... ./tools/publish/linux.sh --appimage-url <上一步 URL> \
#          --extra-artifact <lite AppImage> [--extra-artifact <.lite.sig>]
#   原因：Linux 客户端自动更新只认 AppImage；full 用户若更新成 lite，目标机缺少系统
#   webkit 会直接启动失败，所以 latest.json 的 linux url 必须指向 GitHub 上的 full 资产。
#   Gitee 1GB 附件配额只放 deb 与 lite 手动下载包；full AppImage 的 .sig 与 SHA-256
#   仍在本地计算并写入 latest.json（--appimage-url 允许非 Gitee 域名）。
#
# 选项:
#   --appimage <路径>        更新用完整 AppImage（默认 bundle/appimage 下第一个）
#   --appimage-url <https>   latest.json 中 linux url（允许非 Gitee 域名）
#   --extra-artifact <路径>  额外上传文件，可重复（如 lite AppImage 及其 .sig）
#
# 默认发布到 <owner>/auroradb-releases（公开仓库），源码仓库保持私有。
# 令牌需在 Gitee「设置 → 私人令牌」生成，勾选 projects 权限。
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
APPIMAGE_URL=""
DRY_RUN=0
EXTRA_ARTIFACTS=()
while [[ $# -gt 0 ]]; do
    case $1 in
        --tag) TAG="$2"; shift 2 ;;
        --notes) NOTES="$2"; shift 2 ;;
        --repo) RELEASE_REPO="$2"; shift 2 ;;
        --appimage) APPIMAGE="$2"; shift 2 ;;
        --appimage-url) APPIMAGE_URL="$2"; shift 2 ;;
        --extra-artifact) EXTRA_ARTIFACTS+=("$2"); shift 2 ;;
        --dry-run) DRY_RUN=1; shift ;;
        -h|--help)
            sed -n '3,29p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) echo -e "${RED}未知参数: $1${NC}"; exit 1 ;;
    esac
done

# dry-run 只做本地校验与计划输出，不需要令牌；正式发布会先拦下缺少令牌的情况
if (( DRY_RUN == 0 )) && [[ -z "${GITEE_TOKEN:-}" ]]; then
    echo -e "${RED}错误: 未设置 GITEE_TOKEN 环境变量${NC}"
    echo "  请先执行: export GITEE_TOKEN=<你的 Gitee 私人令牌>"
    exit 1
fi
if [[ -n "$APPIMAGE_URL" && "$APPIMAGE_URL" != https://* ]]; then
    echo -e "${RED}错误: --appimage-url 必须是 https:// 地址，当前为 $APPIMAGE_URL${NC}"
    exit 1
fi

command -v curl >/dev/null || { echo -e "${RED}错误: 未找到 curl${NC}"; exit 1; }
command -v python3 >/dev/null || { echo -e "${RED}错误: 未找到 python3${NC}"; exit 1; }
command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1 \
    || { echo -e "${RED}错误: 未找到 sha256sum/shasum，无法计算安装包校验值${NC}"; exit 1; }

# 从 origin 解析 owner/repo，兼容 SSH 与 HTTPS 两种地址
REMOTE_URL="$(git remote get-url origin)"
PATH_PART="${REMOTE_URL#*gitee.com[:/]}"
PATH_PART="${PATH_PART%.git}"
OWNER="${PATH_PART%%/*}"
REPO="${PATH_PART##*/}"
if [[ -z "$OWNER" || -z "$REPO" ]]; then
    echo -e "${RED}错误: 无法从远程地址解析仓库: $REMOTE_URL${NC}"
    exit 1
fi

# 分发目标：默认同账号下的公开仓库，源码仓库保持私有
RELEASE_REPO="${RELEASE_REPO:-$OWNER/auroradb-releases}"
if [[ "$RELEASE_REPO" != */* ]]; then
    echo -e "${RED}错误: --repo 需为 owner/repo 格式，当前为 $RELEASE_REPO${NC}"
    exit 1
fi
OWNER="${RELEASE_REPO%%/*}"
REPO="${RELEASE_REPO##*/}"

VERSION="$(grep -m1 '"version"' src-tauri/tauri.conf.json | sed 's/.*"version": *"\([^"]*\)".*/\1/')"
TAG="${TAG:-v$VERSION}"
RELEASE_NAME="AuroraDB $VERSION"
# Gitee 要求发行版描述非空
NOTES="${NOTES:-AuroraDB $VERSION 版本发布}"

API="https://gitee.com/api/v5/repos/$OWNER/$REPO"
BUNDLE_DIR="src-tauri/target/release/bundle"
MAX_SIZE=$((100 * 1024 * 1024))
GITEE_DOWNLOAD_PREFIX="https://gitee.com/$OWNER/$REPO/releases/download/"

if [[ ! -d "$BUNDLE_DIR" ]]; then
    echo -e "${RED}错误: 未找到构建产物目录 $BUNDLE_DIR，请先执行 ./tools/build/linux.sh${NC}"
    exit 1
fi

TMP="$(mktemp)"
LATEST_TMP="$(mktemp)"
README_TMP=""
# trap 在 EXIT 时展开这些变量，set -u 下必须先初始化，否则早期失败会报 unbound
LATEST_RESP=""
trap 'rm -f "$TMP" "$LATEST_TMP" "$LATEST_RESP" "$README_TMP"' EXIT

# 计算文件 SHA-256（小写十六进制）：写入 latest.json 供应用内下载的安装包校验完整性
sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

json_id() {
    # 忽略 JSON 中的换行与空格，取第一个顶层 "id"
    awk 'BEGIN { RS = "\0" } {
        gsub(/[[:space:]]+/, "")
        if (match($0, /"id":[0-9]+/)) {
            print substr($0, RSTART + 5, RLENGTH - 5)
            exit
        }
    }' "$1"
}

json_field() {
    # 取第一个顶层字符串字段值，如 json_field default_branch file
    awk -v key="$1" '
        BEGIN { RS = "\0" } {
            gsub(/[[:space:]]+/, "")
            pat = "\"" key "\":\"[^\"]*\""
            if (match($0, pat)) {
                v = substr($0, RSTART, RLENGTH)
                sub("^\"" key "\":\"", "", v)
                sub("\"$", "", v)
                print v
                exit
            }
        }' "$2"
}

# ---- 解析更新包与上传集（双通道发布的核心：full 可能不上传 Gitee）----
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

ARTIFACT_URL="$APPIMAGE_URL"
if [[ -z "$ARTIFACT_URL" ]]; then
    ARTIFACT_URL="$GITEE_DOWNLOAD_PREFIX$TAG/$(basename "$ARTIFACT")"
fi
# 更新包地址不在当前 Gitee 仓库时，不上传 full（省 1GB 配额），但签名与校验值照常读取
FULL_ON_GITEE=0
if [[ "$ARTIFACT_URL" == "$GITEE_DOWNLOAD_PREFIX"* ]]; then
    FULL_ON_GITEE=1
else
    echo -e "${YELLOW}  注意: 更新包地址不是当前 Gitee 仓库（$ARTIFACT_URL），full AppImage 不上传 Gitee${NC}"
    echo "        签名与 SHA-256 仍会写入 latest.json，确保地址与本地 .sig 对应同一文件"
fi
# 清单里带上校验值：用户手动下载 AppImage/deb 时也能核对完整性
ARTIFACT_SHA256="$(sha256_of "$ARTIFACT")"

# 显式模式：只要用了新参数，就按显式上传集发布，不再把 bundle 下所有产物都传上去
EXPLICIT_MODE=0
if [[ -n "$APPIMAGE" || -n "$APPIMAGE_URL" || ${#EXTRA_ARTIFACTS[@]} -gt 0 ]]; then
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
    DEB_URL="$GITEE_DOWNLOAD_PREFIX$TAG/$(basename "$DEB")"
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

if (( FULL_ON_GITEE == 1 )); then
    add_upload "$ARTIFACT"
    add_upload "$ARTIFACT.sig"
fi
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
    elif git ls-remote --tags origin "refs/tags/$TAG" 2>/dev/null | grep -q "refs/tags/$TAG"; then
        echo -e "${GREEN}  ✓ tag $TAG 已推送到远程${NC}"
    else
        echo -e "${YELLOW}  ! 本地 tag $TAG 尚未推送到远程${NC}"
        echo "    推送: git push origin $TAG"
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

# 上传清单：显式模式下只传 UPLOAD_FILES + SHA256SUMS（full 不在 Gitee 时自然不在其中）；
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
           -o -name '*.sig' -o -name 'latest.json' -o -name 'SHA256SUMS' \) | sort)
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
    if (( FULL_ON_GITEE == 1 )); then
        echo "    Gitee 上传: 是（地址指向当前发布仓库）"
    else
        echo "    Gitee 上传: 否（非当前 Gitee 仓库地址，省 1GB 配额，仅写入 latest.json）"
    fi
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
    echo "  将上传到 Gitee 的附件:"
    for f in "${FILES[@]}"; do
        SIZE="$(stat -c %s "$f")"
        echo "    - $(basename "$f") ($((SIZE / 1024 / 1024))MB)"
    done
    echo "  SHA256SUMS: $BUNDLE_DIR/SHA256SUMS"
    echo ""

    # 本地试合并：用 null 作为线上旧清单，检查产物是否可生成
    LATEST_RESP="$(mktemp)"
    echo 'null' > "$LATEST_RESP"
    python3 tools/merge-latest.py --existing "$LATEST_RESP" "${MERGE_ARGS[@]}"
    echo -e "${GREEN}  latest.json 合并预览:${NC}"
    sed 's/^/    /' "$LATEST_TMP"
    echo ""
    echo -e "${GREEN}dry-run 通过：以上命令在正式执行时才会访问网络${NC}"
    exit 0
fi

# 0. 校验发布仓库可访问：私有仓库的发行版附件对终端用户不可见，必须发布到公开仓库
HTTP_CODE="$(curl -sS -o "$TMP" -w '%{http_code}' "$API?access_token=$GITEE_TOKEN")"
if [[ "$HTTP_CODE" != "200" ]]; then
    echo -e "${RED}错误: 无法访问发布仓库 $OWNER/$REPO (HTTP $HTTP_CODE)${NC}"
    echo "  请先在 Gitee 创建公开仓库 $OWNER/$REPO，并初始化 README，确保令牌有 projects 权限"
    exit 1
fi
TARGET_BRANCH="$(json_field default_branch "$TMP")"
TARGET_BRANCH="${TARGET_BRANCH:-main}"

# 1. 查找已有发行版，不存在则创建
HTTP_CODE="$(curl -sS -o "$TMP" -w '%{http_code}' \
    "$API/releases/tags/$TAG?access_token=$GITEE_TOKEN")"

# Gitee 在发行版不存在时返回 200 + body null，需同时判断 id 是否解析成功
RELEASE_ID=""
if [[ "$HTTP_CODE" == "200" ]]; then
    RELEASE_ID="$(json_id "$TMP")"
fi

if [[ -n "$RELEASE_ID" ]]; then
    echo -e "${GREEN}  发行版已存在 (id=$RELEASE_ID)，直接追加附件${NC}"
else
    HTTP_CODE="$(curl -sS -X POST -o "$TMP" -w '%{http_code}' \
        "$API/releases?access_token=$GITEE_TOKEN" \
        --data-urlencode "tag_name=$TAG" \
        --data-urlencode "name=$RELEASE_NAME" \
        --data-urlencode "body=$NOTES" \
        --data-urlencode "target_commitish=$TARGET_BRANCH")"
    if [[ "$HTTP_CODE" != "201" ]]; then
        echo -e "${RED}  ✗ 创建发行版失败 (HTTP $HTTP_CODE)${NC}"
        cat "$TMP"
        if [[ "$HTTP_CODE" == "404" ]]; then
            echo -e "${YELLOW}  请先在 Gitee 创建公开仓库 $OWNER/$REPO，并确保令牌对其有 projects 权限${NC}"
        fi
        exit 1
    fi
    RELEASE_ID="$(json_id "$TMP")"
    echo -e "${GREEN}  已创建发行版 $RELEASE_NAME (id=$RELEASE_ID)${NC}"
fi

if [[ -z "$RELEASE_ID" ]]; then
    echo -e "${RED}  ✗ 无法解析发行版 id，接口原始响应:${NC}"
    cat "$TMP"
    echo ""
    exit 1
fi

# 2. 逐个上传，超过 100MB 直接失败（否则 AppImage 会被静默漏发）
#    同名附件先删除，重复发布时覆盖旧产物而不是报错
declare -A EXISTING_ATTACH=()
HTTP_CODE="$(curl -sS -o "$TMP" -w '%{http_code}' \
    "$API/releases/$RELEASE_ID/attach_files?access_token=$GITEE_TOKEN")"
if [[ "$HTTP_CODE" == "200" ]]; then
    while IFS=$'\t' read -r ATTACH_ID ATTACH_NAME; do
        [[ -n "$ATTACH_ID" ]] && EXISTING_ATTACH["$ATTACH_NAME"]="$ATTACH_ID"
    done < <(python3 - "$TMP" <<'PY'
import json
import sys

with open(sys.argv[1], encoding='utf-8') as f:
    for a in json.load(f) or []:
        print(f"{a.get('id', '')}\t{a.get('name', '')}")
PY
)
fi

for f in "${FILES[@]}"; do
    NAME_ONLY="$(basename "$f")"
    SIZE="$(stat -c %s "$f")"
    if (( SIZE > MAX_SIZE )); then
        echo -e "${RED}  ✗ $NAME_ONLY ($((SIZE / 1024 / 1024))MB) 超过 Gitee 单附件上限${NC}"
        echo "  Linux AppImage 需先重打包: ./tools/build/appimage-repack.sh $f"
        exit 1
    fi

    if [[ -n "${EXISTING_ATTACH[$NAME_ONLY]:-}" ]]; then
        HTTP_CODE="$(curl -sS -X DELETE -o "$TMP" -w '%{http_code}' \
            "$API/releases/$RELEASE_ID/attach_files/${EXISTING_ATTACH[$NAME_ONLY]}?access_token=$GITEE_TOKEN")"
        if [[ "$HTTP_CODE" != "204" && "$HTTP_CODE" != "200" ]]; then
            echo -e "${RED}  ✗ 删除同名旧附件 $NAME_ONLY 失败 (HTTP $HTTP_CODE)${NC}"
            cat "$TMP"
            exit 1
        fi
        echo -e "${YELLOW}  - 已删除同名旧附件 $NAME_ONLY${NC}"
    fi

    HTTP_CODE="$(curl -sS --retry 3 --retry-delay 2 --retry-all-errors \
        --connect-timeout 30 --max-time 600 \
        -X POST -o "$TMP" -w '%{http_code}' \
        "$API/releases/$RELEASE_ID/attach_files?access_token=$GITEE_TOKEN" \
        -F "file=@$f")"
    if [[ "$HTTP_CODE" == "201" ]]; then
        echo -e "${GREEN}  ✓ $NAME_ONLY ($((SIZE / 1024 / 1024))MB)${NC}"
        echo "    $GITEE_DOWNLOAD_PREFIX$TAG/$NAME_ONLY"
    else
        # 任一附件失败都必须中止：继续写 latest.json 会让应用更新到不存在的地址
        echo -e "${RED}  ✗ $NAME_ONLY 上传失败 (HTTP $HTTP_CODE)${NC}"
        cat "$TMP"
        if grep -q '配额' "$TMP"; then
            echo -e "${YELLOW}  Gitee 仓库附件总配额（1GB）已用尽：请删除旧发行版的安装包附件后重试${NC}"
        fi
        exit 1
    fi
done

# 上传成功不代表附件可用：写清单前核对远端附件列表，避免出现「有 sig 无安装包」
echo ""
echo -e "${YELLOW}[3/7] 核对远端附件...${NC}"
HTTP_CODE="$(curl -sS -o "$TMP" -w '%{http_code}' \
    "$API/releases/$RELEASE_ID/attach_files?access_token=$GITEE_TOKEN")"
if [[ "$HTTP_CODE" != "200" ]]; then
    echo -e "${RED}  ✗ 无法读取发行版附件列表 (HTTP $HTTP_CODE)${NC}"
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

# 4. 生成/合并 latest.json 并写入分发仓库默认分支，作为 App 内自动更新的固定入口
#    full AppImage 在 GitHub 上（--appimage-url），这里只写 URL/signature/sha256，不重复上传
# 先读线上清单：合并需要旧内容，PUT 也需要 sha
LATEST_RESP="$(mktemp)"
HTTP_CODE="$(curl -sS -o "$LATEST_RESP" -w '%{http_code}' \
    "$API/contents/latest.json?access_token=$GITEE_TOKEN&ref=$TARGET_BRANCH")"
EXISTING_SHA=""
if [[ "$HTTP_CODE" == "200" ]]; then
    EXISTING_SHA="$(json_field sha "$LATEST_RESP")"
else
    echo 'null' > "$LATEST_RESP"
fi

python3 tools/merge-latest.py --existing "$LATEST_RESP" "${MERGE_ARGS[@]}"

B64_CONTENT="$(base64 -w0 "$LATEST_TMP")"
if [[ -n "$EXISTING_SHA" ]]; then
    METHOD="PUT"
    SHA_ARGS=(--data-urlencode "sha=$EXISTING_SHA")
else
    METHOD="POST"
    SHA_ARGS=()
fi

HTTP_CODE="$(curl -sS -X "$METHOD" -o "$TMP" -w '%{http_code}' \
    "$API/contents/latest.json" \
    --data-urlencode "access_token=$GITEE_TOKEN" \
    --data-urlencode "content=$B64_CONTENT" \
    --data-urlencode "message=chore: 更新自动更新清单 $VERSION" \
    --data-urlencode "branch=$TARGET_BRANCH" \
    ${SHA_ARGS[@]+"${SHA_ARGS[@]}"})"

LATEST_URL="https://gitee.com/$OWNER/$REPO/raw/$TARGET_BRANCH/latest.json"
if [[ "$HTTP_CODE" == "200" || "$HTTP_CODE" == "201" ]]; then
    echo -e "${GREEN}✓ 自动更新清单已更新: $LATEST_URL${NC}"
    CONFIGURED_ENDPOINT="$(grep -o 'https://[^"]*latest\.json' src-tauri/tauri.conf.json | head -n 1)"
    if [[ -n "$CONFIGURED_ENDPOINT" && "$CONFIGURED_ENDPOINT" != "$LATEST_URL" ]]; then
        echo -e "${YELLOW}  注意: tauri.conf.json 的更新地址为 $CONFIGURED_ENDPOINT，与本次发布地址不一致${NC}"
    fi
else
    echo -e "${RED}✗ 自动更新清单更新失败 (HTTP $HTTP_CODE)${NC}"
    cat "$TMP"
    exit 1
fi

# 7. 同步 README 中的版本号：徽章、「最新版本」、安装包名与下载直链
#    README 由终端用户直接打开，版本号写死在其中，发版后必须一起更新。
HTTP_CODE="$(curl -sS -o "$TMP" -w '%{http_code}' \
    "$API/contents/README.md?access_token=$GITEE_TOKEN&ref=$TARGET_BRANCH")"

if [[ "$HTTP_CODE" != "200" ]]; then
    echo -e "${YELLOW}  注意: 未读取到发布仓库的 README.md (HTTP $HTTP_CODE)，跳过版本号同步${NC}"
else
    README_SHA="$(json_field sha "$TMP")"
    README_TMP="$(mktemp)"
    CHANGED="$(python3 - "$TMP" "$README_TMP" "$VERSION" "$TAG" <<'PY'
import base64
import json
import re
import sys

api_json, out, version, tag = sys.argv[1:5]
text = base64.b64decode(json.load(open(api_json, encoding='utf-8'))['content']).decode('utf-8')

new = text
# shields.io 版本徽章
new = re.sub(r'version-[0-9][^-]*?-blue', f'version-{version}-blue', new)
# 「最新版本：**v1.0.7**」
new = re.sub(r'(最新版本：\*\*)v[^*]+(\*\*)', rf'\g<1>{tag}\g<2>', new)
# 安装包文件名 AuroraDB_1.0.7_amd64.*
new = re.sub(r'AuroraDB_[0-9][^_]*_amd64', f'AuroraDB_{version}_amd64', new)
# 下载直链 .../releases/download/v1.0.7/...
new = re.sub(r'releases/download/v[^/]+/', f'releases/download/{tag}/', new)

open(out, 'w', encoding='utf-8').write(new)
print('changed' if new != text else 'same')
PY
)"

    if [[ "$CHANGED" != "changed" ]]; then
        echo -e "${GREEN}✓ README 版本号已是最新 ($TAG)${NC}"
    else
        B64_CONTENT="$(base64 -w0 "$README_TMP")"
        HTTP_CODE="$(curl -sS -X PUT -o "$TMP" -w '%{http_code}' \
            "$API/contents/README.md" \
            --data-urlencode "access_token=$GITEE_TOKEN" \
            --data-urlencode "content=$B64_CONTENT" \
            --data-urlencode "message=chore: 更新 README 版本号 $TAG" \
            --data-urlencode "sha=$README_SHA" \
            --data-urlencode "branch=$TARGET_BRANCH")"

        if [[ "$HTTP_CODE" == "200" || "$HTTP_CODE" == "201" ]]; then
            echo -e "${GREEN}✓ README 版本号已更新为 $TAG${NC}"
            # 本地副本同步，避免下次手工编辑时又用旧版本号
            LOCAL_README="../docs/auroradb-releases-README.md"
            if [[ -f "$LOCAL_README" ]]; then
                cp "$README_TMP" "$LOCAL_README"
                echo -e "  本地副本已同步: $LOCAL_README"
            fi
        else
            echo -e "${YELLOW}  注意: README 版本号更新失败 (HTTP $HTTP_CODE)，可手工修改${NC}"
            cat "$TMP"
        fi
    fi
fi
