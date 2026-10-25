#!/usr/bin/env bash

# 将构建产物上传到 Gitee 发行版（Releases）
#
# 用法:
#   GITEE_TOKEN=<令牌> ./release.sh
#   GITEE_TOKEN=<令牌> ./release.sh --tag v1.0.1 --notes "修复若干问题"
#   ./release.sh --repo owner/auroradb-releases   # 指定公开分发仓库
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
while [[ $# -gt 0 ]]; do
    case $1 in
        --tag) TAG="$2"; shift 2 ;;
        --notes) NOTES="$2"; shift 2 ;;
        --repo) RELEASE_REPO="$2"; shift 2 ;;
        -h|--help)
            sed -n '3,12p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) echo -e "${RED}未知参数: $1${NC}"; exit 1 ;;
    esac
done

if [[ -z "${GITEE_TOKEN:-}" ]]; then
    echo -e "${RED}错误: 未设置 GITEE_TOKEN 环境变量${NC}"
    echo "  请先执行: export GITEE_TOKEN=<你的 Gitee 私人令牌>"
    exit 1
fi

command -v curl >/dev/null || { echo -e "${RED}错误: 未找到 curl${NC}"; exit 1; }

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

TMP="$(mktemp)"
trap 'rm -f "$TMP"' EXIT

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
    \( -name '*.deb' -o -name '*.rpm' -o -name '*.AppImage' -o -name '*.sig' -o -name 'latest.json' \) | sort)

if [[ ${#FILES[@]} -eq 0 ]]; then
    echo -e "${RED}错误: $BUNDLE_DIR 下没有可上传的产物${NC}"
    exit 1
fi

# 3. 逐个上传，超过 100MB 的跳过
UPLOADED=0
for f in "${FILES[@]}"; do
    NAME_ONLY="$(basename "$f")"
    SIZE="$(stat -c %s "$f")"
    if (( SIZE > MAX_SIZE )); then
        echo -e "${YELLOW}  跳过 $NAME_ONLY ($((SIZE / 1024 / 1024))MB，超过 Gitee 单附件上限)${NC}"
        continue
    fi

    HTTP_CODE="$(curl -sS -X POST -o "$TMP" -w '%{http_code}' \
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
