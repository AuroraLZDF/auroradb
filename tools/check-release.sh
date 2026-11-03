#!/usr/bin/env bash

# 发布前检查：在上传之前发现漏发、发错或超限
#
# 用法:
#   ./tools/check-release.sh                     # 检查 Linux 产物（默认）
#   ./tools/check-release.sh --platform windows  # 检查 Windows 产物
#   ./tools/check-release.sh --repo owner/repo
#   ./tools/check-release.sh --bundle-dir <目录> # 指定产物目录（默认 src-tauri/target/release/bundle）
#
# 上传集可用 --upload-file 显式指定（可重复），更新包可用 --appimage 指定
# （允许位于 bundle 目录之外）。不传这两个参数时行为与旧版一致（更新包与
# 安装包都视为上传集）。上传文件必须 <=2GB（GitHub 单附件上限）；更新包
# 不限制大小，但必须存在、带 .sig、能算出 SHA-256。
#
# 检查项:
#   1. 平台必需产物齐全，更新包带有 .sig 签名
#   2. 上传附件不超过 GitHub 的 2GB 单附件上限
#   3. tauri.conf.json 的更新源地址指向本次发布仓库
#   4. 生成 SHA256SUMS（随安装包一起上传，供用户核对完整性）

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PLATFORM="linux"
RELEASE_REPO=""
BUNDLE_DIR="src-tauri/target/release/bundle"
MAX_SIZE=$((2 * 1024 * 1024 * 1024))
APPIMAGE=""
UPLOAD_FILES=()

while [[ $# -gt 0 ]]; do
    case $1 in
        --platform) PLATFORM="${2:-}"; shift 2 ;;
        --repo) RELEASE_REPO="${2:-}"; shift 2 ;;
        --bundle-dir) BUNDLE_DIR="${2:-}"; shift 2 ;;
        --appimage) APPIMAGE="${2:-}"; shift 2 ;;
        --upload-file) UPLOAD_FILES+=("${2:-}"); shift 2 ;;
        -h|--help)
            sed -n '3,20p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) echo -e "${RED}未知参数: $1${NC}"; exit 1 ;;
    esac
done

fail() { echo -e "${RED}  ✗ $*${NC}"; exit 1; }
ok()   { echo -e "${GREEN}  ✓ $*${NC}"; }
warn() { echo -e "${YELLOW}  ! $*${NC}"; }

[[ "$PLATFORM" == "linux" || "$PLATFORM" == "windows" ]] \
    || fail "--platform 只支持 linux 或 windows，当前为 $PLATFORM"
command -v python3 >/dev/null || fail "未找到 python3"
[[ -d "$BUNDLE_DIR" ]] || fail "未找到产物目录 $BUNDLE_DIR，请先构建"

# 发布仓库：默认与 origin 同名（如 AuroraLZDF/auroradb），与发布脚本保持一致
if [[ -z "$RELEASE_REPO" ]]; then
    REMOTE_URL="$(git remote get-url origin 2>/dev/null || true)"
    [[ -n "$REMOTE_URL" ]] || fail "未配置 origin 远程仓库，请用 --repo 指定发布仓库"
    REMOTE_URL="${REMOTE_URL%.git}"
    REPO_NAME="${REMOTE_URL##*/}"
    OWNER_PART="${REMOTE_URL%/*}"
    OWNER_NAME="${OWNER_PART##*[:/]}"
    [[ -n "$OWNER_NAME" && -n "$REPO_NAME" ]] || fail "无法从 origin 解析仓库地址: $REMOTE_URL"
    RELEASE_REPO="$OWNER_NAME/$REPO_NAME"
fi
[[ "$RELEASE_REPO" == */* ]] || fail "--repo 需为 owner/repo 格式，当前为 $RELEASE_REPO"

echo "发布前检查: $PLATFORM -> $RELEASE_REPO"

# 1. 平台必需产物：更新包（应用内升级用）与安装包（手动安装用）
shopt -s nullglob
EXPLICIT_UPDATER=0
if [[ "$PLATFORM" == "windows" ]]; then
    PLATFORM_KEY="windows-x86_64"
    # Tauri 2 的 NSIS 更新包就是 *-setup.exe 本身（tools/publish/windows.ps1 也按此口径上传与签名）
    UPDATER=( "$BUNDLE_DIR"/nsis/*-setup.exe )
    INSTALLERS=( "${UPDATER[@]}" )
else
    PLATFORM_KEY="linux-x86_64"
    if [[ -n "$APPIMAGE" ]]; then
        # 完整 AppImage 可以不在 bundle 目录里（例如从其它构建目录取），且不参与大小限制
        [[ -f "$APPIMAGE" ]] || fail "指定的更新包不存在: $APPIMAGE"
        UPDATER=( "$APPIMAGE" )
        EXPLICIT_UPDATER=1
    else
        UPDATER=( "$BUNDLE_DIR"/appimage/*.AppImage )
    fi
    INSTALLERS=( "$BUNDLE_DIR"/deb/*.deb )
fi
shopt -u nullglob

# 未显式指定更新包时沿用旧口径：bundle 里必须恰好有一个 AppImage/setup.exe
if (( EXPLICIT_UPDATER == 0 )); then
    (( ${#UPDATER[@]} == 1 )) || fail "更新包数量应为 1，实际 ${#UPDATER[@]} 个（请清理旧产物后重新构建）"
fi
[[ -f "${UPDATER[0]}.sig" ]] || fail "缺少 ${UPDATER[0]##*/}.sig，应用内更新会校验失败"
ok "更新包 ${UPDATER[0]##*/} 及签名就绪"

if [[ "$PLATFORM" == "windows" ]]; then
    (( ${#INSTALLERS[@]} >= 1 )) || fail "未找到 NSIS 安装包（*.exe）"
    ok "安装包 ${INSTALLERS[0]##*/} 就绪"
elif (( ${#INSTALLERS[@]} == 0 )); then
    warn "未找到 deb 安装包，deb 用户将无法在应用内下载更新"
elif [[ ! -f "${INSTALLERS[0]}.sig" ]]; then
    warn "缺少 ${INSTALLERS[0]##*/}.sig，清单中不会包含 deb 校验值"
fi

# 上传集：显式给了 --upload-file 就用它（更新包不在其中时不参与大小限制/SHA256SUMS）
if (( ${#UPLOAD_FILES[@]} > 0 )); then
    UPLOAD=( "${UPLOAD_FILES[@]}" )
    for f in "${UPLOAD[@]}"; do
        [[ -f "$f" ]] || fail "指定的上传文件不存在: $f"
    done
else
    UPLOAD=( "${UPDATER[@]}" ${INSTALLERS[@]+"${INSTALLERS[@]}"} )
fi

# 2. 上传附件大小：GitHub 单附件上限 2GB，超限会被拒收
for f in "${UPLOAD[@]}"; do
    SIZE="$(stat -c %s "$f")"
    (( SIZE <= MAX_SIZE )) || fail "$(basename "$f") ($((SIZE / 1024 / 1024))MB) 超过 GitHub 2GB 上限"
done
ok "上传附件大小均在 2GB 以内"

# 更新包不进上传集时（latest.json 直接指向它的 URL），单独确认存在、签名与校验值
if ! printf '%s\n' "${UPLOAD[@]}" | grep -qxF "${UPDATER[0]}"; then
    UPDATER_SHA256="$(python3 - "$UPDATER" <<'PY'
import hashlib
import sys

digest = hashlib.sha256()
with open(sys.argv[1], 'rb') as f:
    for chunk in iter(lambda: f.read(1 << 20), b''):
        digest.update(chunk)
print(digest.hexdigest())
PY
)"
    ok "更新包 ${UPDATER[0]##*/} 不在上传集，SHA-256: $UPDATER_SHA256"
fi

# 3. 更新源与发布仓库一致性：地址写错时应用永远收不到更新
ENDPOINT="$(python3 - <<'PY'
import json

with open('src-tauri/tauri.conf.json', encoding='utf-8') as f:
    data = json.load(f)
endpoints = ((data.get('plugins') or {}).get('updater') or {}).get('endpoints') or []
print(endpoints[0] if endpoints else '')
PY
)"
[[ -n "$ENDPOINT" ]] || fail "tauri.conf.json 未配置 plugins.updater.endpoints"
[[ "$ENDPOINT" == */latest.json ]] || fail "更新源应以 latest.json 结尾: $ENDPOINT"
# GitHub 的 owner/repo 大小写不敏感，统一转小写比对（origin 是 auroraLZDF，规范名是 AuroraLZDF）
[[ "${ENDPOINT,,}" == *"/${RELEASE_REPO,,}/"* ]] || fail "更新源 $ENDPOINT 与发布仓库 $RELEASE_REPO 不一致"
ok "更新源指向 $RELEASE_REPO"

# 4. SHA256SUMS：签名只覆盖更新包，安装包完整性需靠校验值（只列真正上传的文件）
python3 - "$BUNDLE_DIR/SHA256SUMS" "${UPLOAD[@]}" <<'PY'
import hashlib
import os
import sys

out, files = sys.argv[1], sys.argv[2:]
lines = []
# 同一个文件可能重复出现在参数里（如 Windows 的 setup.exe 身兼更新包与安装包），去重
for path in sorted(set(files)):
    digest = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(1 << 20), b''):
            digest.update(chunk)
    lines.append(f"{digest.hexdigest()}  {os.path.basename(path)}")

with open(out, 'w', encoding='utf-8') as f:
    f.write("\n".join(lines) + "\n")
print("\n".join(lines))
PY
ok "SHA256SUMS 已生成: $BUNDLE_DIR/SHA256SUMS"

echo -e "${GREEN}发布前检查通过（清单平台 key: $PLATFORM_KEY）${NC}"
