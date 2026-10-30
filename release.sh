#!/usr/bin/env bash

# 将构建产物上传到 Gitee 发行版（Releases）
#
# 用法:
#   GITEE_TOKEN=<令牌> ./release.sh
#   GITEE_TOKEN=<令牌> ./release.sh --tag v1.0.1 --notes "修复若干问题"
#   ./release.sh --repo owner/auroradb-releases   # 指定公开分发仓库
#   ./release.sh --platform windows               # 上传 Windows 产物（默认 linux）
#
# 默认发布到 <owner>/auroradb-releases（公开仓库），源码仓库保持私有。
# 令牌需在 Gitee「设置 → 私人令牌」生成，勾选 projects 权限。
# 令牌只从环境变量读取，不会写入仓库。

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

TAG=""
NOTES=""
RELEASE_REPO=""
PLATFORM="linux"
while [[ $# -gt 0 ]]; do
    case $1 in
        --tag) TAG="$2"; shift 2 ;;
        --notes) NOTES="$2"; shift 2 ;;
        --repo) RELEASE_REPO="$2"; shift 2 ;;
        --platform) PLATFORM="$2"; shift 2 ;;
        -h|--help)
            sed -n '3,13p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) echo -e "${RED}未知参数: $1${NC}"; exit 1 ;;
    esac
done

if [[ "$PLATFORM" != "linux" && "$PLATFORM" != "windows" ]]; then
    echo -e "${RED}错误: --platform 只支持 linux 或 windows，当前为 $PLATFORM${NC}"
    exit 1
fi

if [[ -z "${GITEE_TOKEN:-}" ]]; then
    echo -e "${RED}错误: 未设置 GITEE_TOKEN 环境变量${NC}"
    echo "  请先执行: export GITEE_TOKEN=<你的 Gitee 私人令牌>"
    exit 1
fi

command -v curl >/dev/null || { echo -e "${RED}错误: 未找到 curl${NC}"; exit 1; }
command -v python3 >/dev/null || { echo -e "${RED}错误: 未找到 python3${NC}"; exit 1; }

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

if [[ ! -d "$BUNDLE_DIR" ]]; then
    echo -e "${RED}错误: 未找到构建产物目录 $BUNDLE_DIR，请先执行 ./build.sh${NC}"
    exit 1
fi

echo -e "${YELLOW}发布仓库: $OWNER/$REPO  版本: $TAG${NC}"

# 上线前先自检：产物齐全、签名存在、不超限、更新地址一致，并生成 SHA256SUMS
bash tools/check-release.sh --platform "$PLATFORM" --repo "$OWNER/$REPO" --bundle-dir "$BUNDLE_DIR"

TMP="$(mktemp)"
LATEST_TMP="$(mktemp)"
README_TMP=""
trap 'rm -f "$TMP" "$LATEST_TMP" "$LATEST_RESP" "$README_TMP"' EXIT

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

# 2. 收集产物（安装包与更新签名文件）
FILES=()
while IFS= read -r f; do
    FILES+=("$f")
done < <(find "$BUNDLE_DIR" -maxdepth 2 -type f \
    \( -name '*.deb' -o -name '*.rpm' -o -name '*.AppImage' -o -name '*.exe' \
       -o -name '*.nsis.zip' -o -name '*.sig' -o -name 'latest.json' \
       -o -name 'SHA256SUMS' \) \
    ! -name '*.nsis.zip.sig' | sort)

if [[ ${#FILES[@]} -eq 0 ]]; then
    echo -e "${RED}错误: $BUNDLE_DIR 下没有可上传的产物${NC}"
    exit 1
fi

# 3. 逐个上传，超过 100MB 直接失败（否则 AppImage 会被静默漏发）
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

UPLOADED=0
for f in "${FILES[@]}"; do
    NAME_ONLY="$(basename "$f")"
    SIZE="$(stat -c %s "$f")"
    if (( SIZE > MAX_SIZE )); then
        echo -e "${RED}  ✗ $NAME_ONLY ($((SIZE / 1024 / 1024))MB) 超过 Gitee 单附件上限${NC}"
        echo "  Linux AppImage 需先重打包: ./repack-appimage.sh $f"
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
        echo "    https://gitee.com/$OWNER/$REPO/releases/download/$TAG/$NAME_ONLY"
        UPLOADED=$((UPLOADED + 1))
    else
        echo -e "${RED}  ✗ $NAME_ONLY 上传失败 (HTTP $HTTP_CODE)${NC}"
        cat "$TMP"
    fi
done

echo ""
if (( UPLOADED > 0 )); then
    echo -e "${GREEN}完成: $UPLOADED 个文件已发布到 $TAG${NC}"
else
    echo -e "${RED}没有任何文件上传成功${NC}"
    exit 1
fi

# 4. 生成/合并 latest.json 并写入分发仓库默认分支，作为 App 内自动更新的固定入口
#    Linux 与 Windows 分开发布，清单按平台 key 合并，互不覆盖
ARTIFACT=""
if [[ "$PLATFORM" == "windows" ]]; then
    PLATFORM_KEY="windows-x86_64"
    # 应用内更新只认 NSIS 更新包（*.nsis.zip），安装器 exe 只供手动下载
    while IFS= read -r f; do
        ARTIFACT="$f"
        break
    done < <(find "$BUNDLE_DIR/nsis" -maxdepth 1 -type f -name '*.nsis.zip' 2>/dev/null | sort)
    if [[ -z "$ARTIFACT" ]]; then
        echo -e "${RED}错误: 未找到 NSIS 更新包（*.nsis.zip），无法生成 latest.json${NC}"
        exit 1
    fi
else
    PLATFORM_KEY="linux-x86_64"
    while IFS= read -r f; do
        ARTIFACT="$f"
        break
    done < <(find "$BUNDLE_DIR/appimage" -maxdepth 1 -type f -name '*.AppImage' 2>/dev/null | sort)
    if [[ -z "$ARTIFACT" ]]; then
        echo -e "${RED}错误: 未找到 AppImage，无法生成 latest.json${NC}"
        exit 1
    fi
fi
if [[ ! -f "$ARTIFACT.sig" ]]; then
    echo -e "${RED}错误: 未找到 $(basename "$ARTIFACT").sig，请确认构建时已签名${NC}"
    exit 1
fi

ARTIFACT_URL="https://gitee.com/$OWNER/$REPO/releases/download/$TAG/$(basename "$ARTIFACT")"

# deb 安装的应用无法用 AppImage 原地升级，清单里额外给出 deb 包地址，
# 应用会把它下载到下载目录交给用户手动安装
DEB_URL=""
DEB_SIG=""
if [[ "$PLATFORM" == "linux" ]]; then
    DEB=""
    while IFS= read -r f; do
        DEB="$f"
        break
    done < <(find "$BUNDLE_DIR/deb" -maxdepth 1 -type f -name '*.deb' 2>/dev/null | sort)

    if [[ -n "$DEB" ]]; then
        DEB_URL="https://gitee.com/$OWNER/$REPO/releases/download/$TAG/$(basename "$DEB")"
        [[ -f "$DEB.sig" ]] && DEB_SIG="$DEB.sig"
    else
        echo -e "${YELLOW}  注意: 未找到 deb 安装包，deb 安装的用户将无法在应用内下载更新${NC}"
    fi
fi

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

MERGE_ARGS=(--existing "$LATEST_RESP" --output "$LATEST_TMP"
    --version "$VERSION" --notes "$NOTES" --tag "$TAG"
    --platform "$PLATFORM_KEY" --artifact-url "$ARTIFACT_URL"
    --signature-file "$ARTIFACT.sig")
if [[ -n "$DEB_URL" ]]; then
    MERGE_ARGS+=(--deb-url "$DEB_URL")
    [[ -n "$DEB_SIG" ]] && MERGE_ARGS+=(--deb-signature-file "$DEB_SIG")
fi
python3 tools/merge-latest.py "${MERGE_ARGS[@]}"

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
