#!/usr/bin/env bash

# 将完整 AppImage（应用内自动更新包）及签名上传到 GitHub Release
#
# 发布顺序（双层分发）:
#   1) 先执行本脚本，把 full AppImage + .sig 传到 GitHub
#   2) 再执行 tools/publish/linux.sh --appimage-url <这里打印的 URL> \
#          --extra-artifact <lite AppImage>，由它更新 Gitee 的 latest.json
#   原因：Linux 客户端自动更新只认 AppImage。full 是自包含包（自带系统 webkit），
#   lite 依赖目标机已有 webkit；若让 full 用户更新成 lite，可能因缺少系统库而启动失败，
#   所以 latest.json 的 linux url 必须指向 GitHub 上的 full 资产。Gitee 1GB 配额只放
#   deb 与 lite 手动下载包。
#
# 用法（在仓库根目录执行）:
#   GH_TOKEN=<令牌> ./tools/publish/github-full.sh --tag v2.0.1
#   ./tools/publish/github-full.sh --tag v2.0.1 --appimage <完整 AppImage> --dry-run
#
# 选项:
#   --tag <vX.Y.Z>       发行版标签（默认取 src-tauri/tauri.conf.json 的版本）
#   --repo <owner/repo>  GitHub 仓库，默认 AuroraLZDF/auroradb
#   --appimage <路径>    完整 AppImage，默认 bundle/appimage 下第一个
#   --notes <说明>       新建发行版时的说明
#   --dry-run            只打印计划与命令，不联网、不上传
#
# 凭据: GH_TOKEN 或 GITHUB_TOKEN 环境变量；或已登录的 gh CLI。
# 令牌只从环境变量读取，不会写入仓库。

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

TAG=""
REPO="AuroraLZDF/auroradb"
APPIMAGE=""
NOTES=""
DRY_RUN=0
while [[ $# -gt 0 ]]; do
    case $1 in
        --tag) TAG="$2"; shift 2 ;;
        --repo) REPO="$2"; shift 2 ;;
        --appimage) APPIMAGE="$2"; shift 2 ;;
        --notes) NOTES="$2"; shift 2 ;;
        --dry-run) DRY_RUN=1; shift ;;
        -h|--help)
            sed -n '3,26p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) echo -e "${RED}未知参数: $1${NC}"; exit 1 ;;
    esac
done

[[ "$REPO" == */* ]] || { echo -e "${RED}错误: --repo 需为 owner/repo 格式，当前为 $REPO${NC}"; exit 1; }

VERSION="$(grep -m1 '"version"' src-tauri/tauri.conf.json 2>/dev/null \
    | sed 's/.*"version": *"\([^"]*\)".*/\1/' || true)"
VERSION="${VERSION:-unknown}"
TAG="${TAG:-v$VERSION}"
NOTES="${NOTES:-AuroraDB $VERSION 版本发布}"
BUNDLE_DIR="src-tauri/target/release/bundle"

# 完整 AppImage：显式路径优先，否则取 bundle/appimage 下第一个
if [[ -z "$APPIMAGE" ]]; then
    while IFS= read -r f; do
        APPIMAGE="$f"
        break
    done < <(find "$BUNDLE_DIR/appimage" -maxdepth 2 -type f -name '*.AppImage' 2>/dev/null | sort)
fi
if [[ -z "$APPIMAGE" ]]; then
    echo -e "${RED}错误: 未找到完整 AppImage，可用 --appimage 指定${NC}"
    exit 1
fi
if [[ ! -f "$APPIMAGE" ]]; then
    echo -e "${RED}错误: 更新包不存在: $APPIMAGE${NC}"
    exit 1
fi
if [[ ! -f "$APPIMAGE.sig" ]]; then
    echo -e "${RED}错误: 未找到 $(basename "$APPIMAGE").sig，请确认构建时已签名${NC}"
    exit 1
fi

ASSET_NAME="$(basename "$APPIMAGE")"
ASSET_URL="https://github.com/$REPO/releases/download/$TAG/$ASSET_NAME"
SIG_URL="https://github.com/$REPO/releases/download/$TAG/$ASSET_NAME.sig"

TOKEN="${GH_TOKEN:-${GITHUB_TOKEN:-}}"
HAS_GH=0
command -v gh >/dev/null 2>&1 && HAS_GH=1
GH_LOGGED_IN=0
if (( HAS_GH == 1 )) && gh auth status >/dev/null 2>&1; then
    GH_LOGGED_IN=1
fi
if [[ -z "$TOKEN" ]] && (( GH_LOGGED_IN == 0 )); then
    if (( DRY_RUN == 1 )); then
        echo -e "${YELLOW}  ! dry-run: 未检测到 GH_TOKEN/GITHUB_TOKEN，也未登录 gh；正式上传会因缺少凭据失败${NC}"
    else
        echo -e "${RED}错误: 未找到 GitHub 凭据${NC}"
        echo "  请设置环境变量: export GH_TOKEN=<GitHub 令牌>"
        echo "  或先登录 gh CLI: gh auth login"
        exit 1
    fi
fi

echo "发布完整 AppImage 到 GitHub: $REPO  标签: $TAG"
echo "  文件: $APPIMAGE ($(( $(stat -c %s "$APPIMAGE") / 1024 / 1024 ))MB)"
echo "  签名: $APPIMAGE.sig"
echo ""

if (( DRY_RUN == 1 )); then
    echo -e "${YELLOW}== dry-run：以下为正式执行时将运行的命令（不联网、不上传）==${NC}"
    if (( HAS_GH == 1 )) && { [[ -n "$TOKEN" ]] || (( GH_LOGGED_IN == 1 )); }; then
        echo "  gh release view $TAG --repo $REPO"
        echo "  gh release create $TAG --repo $REPO --title 'AuroraDB $VERSION' --notes '$NOTES'   # 发行版不存在时"
        echo "  gh release upload $TAG \"$APPIMAGE\" \"$APPIMAGE.sig\" --repo $REPO --clobber"
    else
        if [[ -n "$TOKEN" ]]; then
            echo "  （未安装 gh，使用 curl + GH_TOKEN 上传）:"
        else
            echo "  （curl 回退分支）需要 GH_TOKEN/GITHUB_TOKEN，例如:"
        fi
        echo "  curl -sS -X POST https://api.github.com/repos/$REPO/releases \\"
        echo "       -H 'Authorization: Bearer \$GH_TOKEN' -H 'Accept: application/vnd.github+json' \\"
        echo "       -d '{\"tag_name\":\"$TAG\",\"name\":\"AuroraDB $VERSION\",\"body\":\"$NOTES\"}'"
        echo "  curl -sS -X POST \"https://uploads.github.com/repos/$REPO/releases/<id>/assets?name=$ASSET_NAME\" \\"
        echo "       -H 'Authorization: Bearer \$GH_TOKEN' -H 'Content-Type: application/octet-stream' \\"
        echo "       --data-binary @\"$APPIMAGE\""
    fi
    echo ""
    echo -e "${GREEN}上传完成后 latest.json 的 linux url 应为:${NC}"
    echo "  $ASSET_URL"
    echo ""
    echo "下一步（Gitee 只发 deb + lite，latest.json 指向上面的 full）:"
    echo "  GITEE_TOKEN=<令牌> ./tools/publish/linux.sh --tag $TAG \\"
    echo "      --appimage \"$APPIMAGE\" --appimage-url \"$ASSET_URL\" \\"
    echo "      --extra-artifact <lite AppImage> [--extra-artifact <lite AppImage.sig>]"
    exit 0
fi

# 计算仓库 GitHub 上的版本号（用于发行版标题），已由 VERSION 提供
RELEASE_NAME="AuroraDB $VERSION"

# 上传：优先用 gh CLI（自动处理已存在资产/发行版），否则用 curl + 令牌
if (( HAS_GH == 1 )) && { [[ -n "$TOKEN" ]] || (( GH_LOGGED_IN == 1 )); }; then
    if gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1; then
        echo -e "${GREEN}  发行版 $TAG 已存在，追加/覆盖资产${NC}"
    else
        echo "  创建发行版 $TAG"
        gh release create "$TAG" --repo "$REPO" \
            --title "$RELEASE_NAME" --notes "$NOTES" >/dev/null
    fi
    gh release upload "$TAG" "$APPIMAGE" "$APPIMAGE.sig" --repo "$REPO" --clobber
else
    # curl 回退分支：需要 python3 解析 JSON
    command -v python3 >/dev/null || { echo -e "${RED}错误: 未找到 python3${NC}"; exit 1; }
    AUTH=(-H "Authorization: Bearer $TOKEN" -H "Accept: application/vnd.github+json" -H "X-GitHub-Api-Version: 2022-11-28")

    RELEASE_JSON="$(mktemp)"
    trap 'rm -f "$RELEASE_JSON"' EXIT
    HTTP_CODE="$(curl -sS -o "$RELEASE_JSON" -w '%{http_code}' \
        "${AUTH[@]}" "https://api.github.com/repos/$REPO/releases/tags/$TAG")"
    if [[ "$HTTP_CODE" == "404" ]]; then
        echo "  创建发行版 $TAG"
        HTTP_CODE="$(curl -sS -X POST -o "$RELEASE_JSON" -w '%{http_code}' \
            "${AUTH[@]}" "https://api.github.com/repos/$REPO/releases" \
            -d "{\"tag_name\":\"$TAG\",\"name\":\"$RELEASE_NAME\",\"body\":\"$NOTES\"}")"
        if [[ "$HTTP_CODE" != "201" ]]; then
            echo -e "${RED}  ✗ 创建 GitHub 发行版失败 (HTTP $HTTP_CODE)${NC}"
            cat "$RELEASE_JSON"
            exit 1
        fi
    elif [[ "$HTTP_CODE" != "200" ]]; then
        echo -e "${RED}  ✗ 查询 GitHub 发行版失败 (HTTP $HTTP_CODE)${NC}"
        cat "$RELEASE_JSON"
        exit 1
    fi

    RELEASE_ID="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("id",""))' "$RELEASE_JSON")"
    UPLOAD_URL="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("upload_url",""))' "$RELEASE_JSON")"
    UPLOAD_URL="${UPLOAD_URL%%\{*}"
    if [[ -z "$RELEASE_ID" || -z "$UPLOAD_URL" ]]; then
        echo -e "${RED}  ✗ 无法解析发行版 id/upload_url${NC}"
        cat "$RELEASE_JSON"
        exit 1
    fi

    upload_asset() {
        local file="$1" name
        name="$(basename "$file")"
        local code
        code="$(curl -sS --retry 3 --retry-delay 2 --connect-timeout 30 --max-time 900 \
            -X POST -o /dev/null -w '%{http_code}' \
            "${AUTH[@]}" -H "Content-Type: application/octet-stream" \
            "$UPLOAD_URL?name=$name" --data-binary @"$file")"
        if [[ "$code" == "422" ]]; then
            # 资产已存在：先删同名资产再重传（等价 gh --clobber）
            echo -e "${YELLOW}  资产 $name 已存在，删除后重传${NC}"
            local ASSET_ID
            ASSET_ID="$(curl -sS "${AUTH[@]}" "https://api.github.com/repos/$REPO/releases/$RELEASE_ID/assets" \
                | python3 -c 'import json,sys; print(next((a["id"] for a in json.load(sys.stdin) if a.get("name")==sys.argv[1]), ""))' "$name")"
            [[ -n "$ASSET_ID" ]] && curl -sS -X DELETE "${AUTH[@]}" \
                "https://api.github.com/repos/$REPO/releases/assets/$ASSET_ID" -o /dev/null
            code="$(curl -sS --retry 3 --retry-delay 2 --connect-timeout 30 --max-time 900 \
                -X POST -o /dev/null -w '%{http_code}' \
                "${AUTH[@]}" -H "Content-Type: application/octet-stream" \
                "$UPLOAD_URL?name=$name" --data-binary @"$file")"
        fi
        if [[ "$code" != "201" && "$code" != "200" ]]; then
            echo -e "${RED}  ✗ 上传 $name 失败 (HTTP $code)${NC}"
            exit 1
        fi
        echo -e "${GREEN}  ✓ 已上传 $name${NC}"
    }

    upload_asset "$APPIMAGE"
    upload_asset "$APPIMAGE.sig"
fi

echo ""
echo -e "${GREEN}✓ 完整 AppImage 已发布到 GitHub${NC}"
echo "  $ASSET_URL"
echo "  $SIG_URL"
echo ""
echo "下一步（Gitee 只发 deb + lite，latest.json 指向上面的 full）:"
echo "  GITEE_TOKEN=<令牌> ./tools/publish/linux.sh --tag $TAG \\"
echo "      --appimage \"$APPIMAGE\" --appimage-url \"$ASSET_URL\" \\"
echo "      --extra-artifact <lite AppImage> [--extra-artifact <lite AppImage.sig>]"
