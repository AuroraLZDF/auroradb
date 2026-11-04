#!/usr/bin/env bash

# 一键升版本 + 提交 + 打 tag，供发版使用。
#
# 用法（在仓库任意目录执行均可）:
#   ./tools/release-tag.sh 2.1.0             # 升到 2.1.0，提交并打 tag v2.1.0
#   ./tools/release-tag.sh v2.1.0            # 版本号可带 v 前缀
#   ./tools/release-tag.sh 2.1.0 --dry-run   # 只展示将产生的改动，不提交
#   ./tools/release-tag.sh 2.1.0 --push      # 提交打 tag 后推送到 origin
#
# 同步范围: src-tauri/tauri.conf.json、src-ui/package.json、src-ui/package-lock.json、
# src-tauri/Cargo.toml、crates/core/Cargo.toml、crates/pro/Cargo.toml、Cargo.lock，
# 以及 README 里的「当前版本」展示；随后执行 node tools/check-version.mjs 校验。
# 前置条件: 工作区干净、新版本高于当前版本、同名 tag 不存在。

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

die() { echo -e "${RED}错误: $1${NC}" >&2; exit 1; }
info() { echo -e "  $1"; }
ok() { echo -e "${GREEN}  ✓ $1${NC}"; }
warn() { echo -e "${YELLOW}  ! $1${NC}"; }

usage() {
    sed -n '3,15p' "$0" | sed 's/^# \{0,1\}//'
}

NEW=""
DRY_RUN=0
PUSH=0
while [[ $# -gt 0 ]]; do
    case $1 in
        --dry-run) DRY_RUN=1; shift ;;
        --push) PUSH=1; shift ;;
        -h|--help) usage; exit 0 ;;
        -*) die "未知参数: $1" ;;
        *)
            [[ -z "$NEW" ]] || die "只能指定一个版本号"
            NEW="$1"; shift ;;
    esac
done

[[ -n "$NEW" ]] || { usage; die "缺少版本号"; }
NEW="${NEW#v}"
[[ "$NEW" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "版本号需为 X.Y.Z 形式，当前: $NEW"
TAG="v$NEW"

VERSION_FILES=(
    src-tauri/tauri.conf.json
    src-ui/package.json
    src-ui/package-lock.json
    src-tauri/Cargo.toml
    crates/core/Cargo.toml
    crates/pro/Cargo.toml
    Cargo.lock
)
for f in "${VERSION_FILES[@]}"; do
    [[ -f "$f" ]] || die "缺少版本文件: $f"
done

# dry-run 会临时改写文件展示 diff，退出时无条件还原（前置条件已保证工作区干净）
restore_if_dry_run() {
    if (( DRY_RUN )); then
        git checkout -- "${VERSION_FILES[@]}" README.md 2>/dev/null || true
    fi
}
trap restore_if_dry_run EXIT

# ---- 前置检查 ----
[[ -z "$(git status --porcelain)" ]] \
    || die "工作区有未提交改动，请先提交或 stash 后再打 tag"
if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
    die "标签 $TAG 已存在，如需重建请先删除: git tag -d $TAG"
fi
if (( PUSH )) && git ls-remote --tags origin "refs/tags/$TAG" 2>/dev/null | grep -q .; then
    die "远程已存在标签 $TAG，无法重复推送"
fi

OLD="$(grep -m1 '"version"' src-tauri/tauri.conf.json | sed 's/.*"version": *"\([^"]*\)".*/\1/')"
[[ -n "$OLD" ]] || die "无法从 src-tauri/tauri.conf.json 读取当前版本"
[[ "$NEW" != "$OLD" ]] || die "版本号已经是 $OLD，无需升级"
if [[ "$(printf '%s\n%s\n' "$OLD" "$NEW" | sort -V | tail -n1)" != "$NEW" ]]; then
    die "新版本 $NEW 必须高于当前版本 $OLD"
fi

BRANCH="$(git rev-parse --abbrev-ref HEAD)"
if [[ "$BRANCH" != "main" && "$BRANCH" != "master" ]]; then
    warn "当前分支是 $BRANCH，不是 main/master"
fi

echo ""
info "版本: $OLD -> $NEW"
info "标签: $TAG"
info "分支: $BRANCH"
echo ""

# ---- 改写版本号 ----
# 转义点号，避免 2.0.2 误匹配 2x0x2
OLD_RE="${OLD//./\\.}"

sed -i "s/\"version\": \"$OLD_RE\"/\"version\": \"$NEW\"/" src-tauri/tauri.conf.json src-ui/package.json
sed -i "s/^version = \"$OLD_RE\"$/version = \"$NEW\"/" src-tauri/Cargo.toml crates/core/Cargo.toml crates/pro/Cargo.toml

# package-lock.json 里依赖自身也有 "version" 字段，只改文件头部的前两处（根字段与 packages[""]）
replace_first_n() {
    local file="$1" count="$2" tmp
    tmp="$(mktemp)"
    awk -v n="$count" -v old="$OLD" -v new="$NEW" '
        {
            pat = "\"version\": \"" old "\""
            if (done < n && index($0, pat) > 0) {
                pos = index($0, pat)
                $0 = substr($0, 1, pos - 1) "\"version\": \"" new "\"" substr($0, pos + length(pat))
                done++
            }
            print
        }
    ' "$file" > "$tmp"
    cat "$tmp" > "$file"
    rm -f "$tmp"
}
replace_first_n src-ui/package-lock.json 2

# Cargo.lock 同理，只改本仓库的 3 个包
sed -i "/^name = \"auroradb/,+1 s/^version = \"$OLD_RE\"/version = \"$NEW\"/" Cargo.lock

if grep -qF "当前版本 v$OLD" README.md; then
    sed -i "s/当前版本 v$OLD_RE/当前版本 v$NEW/" README.md
else
    warn "README 未找到「当前版本 v$OLD」，请手工确认版本展示"
fi

# ---- 防误伤：锁定文件的改动行数必须完全符合预期 ----
changed_lines() { git diff -U0 -- "$1" | grep -cE '^[-+][^-+]' || true; }
LOCK_CHANGED="$(changed_lines src-ui/package-lock.json)"
[[ "$LOCK_CHANGED" -eq 4 ]] \
    || die "src-ui/package-lock.json 改动了 $LOCK_CHANGED 行（预期 4），可能有依赖版本被误改，已中止"
CARGO_LOCK_CHANGED="$(changed_lines Cargo.lock)"
[[ "$CARGO_LOCK_CHANGED" -eq 6 ]] \
    || die "Cargo.lock 改动了 $CARGO_LOCK_CHANGED 行（预期 6），可能有依赖版本被误改，已中止"

# ---- 校验 ----
node tools/check-version.mjs || die "版本号校验失败"

if (( DRY_RUN )); then
    echo ""
    info "将提交以下改动（dry-run，不提交）:"
    git diff --stat
    echo ""
    warn "dry-run 结束，文件已还原"
    exit 0
fi

# ---- 提交并打 tag ----
git add "${VERSION_FILES[@]}" README.md
git commit -m "chore(release): $TAG"
git tag -a "$TAG" -m "AuroraDB $NEW"

ok "已提交并创建标签 $TAG"
info "提交: $(git log -1 --oneline)"

if (( PUSH )); then
    git push origin HEAD
    git push origin "$TAG"
    ok "已推送到 origin"
else
    echo ""
    info "尚未推送，继续执行:"
    info "  git push origin $BRANCH"
    info "  git push origin $TAG"
    info "发布安装包: ./tools/publish/linux.sh --tag $TAG"
fi
