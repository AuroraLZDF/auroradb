#!/usr/bin/env bash

# 从私有主仓导出发行开源公开仓（AGPL-3.0 全量源码）
#
# 用法:
#   ./tools/export-public.sh <目标目录>
#   ./tools/export-public.sh ../auroradb-public --force
#   ./tools/export-public.sh ../auroradb-public --dry-run
#   ./tools/export-public.sh ../auroradb-public --push gitee.com/owner/auroradb-public
#   ./tools/export-public.sh ../auroradb-public --push git@github.com:owner/auroradb-public.git
#   ./tools/export-public.sh ../auroradb-public --push https://gitee.com/owner/auroradb-public.git
#
# 导出内容：完整源码（crates/core、crates/pro、src-tauri、src-ui 含 pro 组件）与构建发布脚本。
# 排除：tools/ship.sh 与 tools/commit-brief.sh（私有提交工作流）、docs/EULA.md、
#       node_modules、target、dist 等全部构建产物。
# 保留目标目录自有内容：README.md、ROADMAP.md 与 .github/（公开仓独立维护，
#       不会被私有仓版本覆盖；目标目录必须先准备好这三项）。
# 导出后做公开内容检查：README/ROADMAP/docs 不得出现内部词，Markdown 相对链接
#       指向的文件必须存在，且必须保留公开仓自己的 .github/workflows/ci.yml；任一失败即中止。
# 导出后重新生成 Cargo.lock，并校验 pro 源码存在与版本号一致性。
# 加 --push 时会在导出后自动 init git、提交并推送到给定远端地址的 main 分支。

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"

fail() { echo -e "${RED}  ✗ $*${NC}"; exit 1; }
ok()   { echo -e "${GREEN}  ✓ $*${NC}"; }
warn() { echo -e "${YELLOW}  ! $*${NC}"; }

TARGET=""
FORCE=0
DRY_RUN=0
PUSH_REMOTE=""
PUSH_BRANCH="main"

while [[ $# -gt 0 ]]; do
    case $1 in
        --force) FORCE=1; shift ;;
        --dry-run) DRY_RUN=1; shift ;;
        --push)
            [[ $# -ge 2 ]] || fail "--push 需要一个远端地址（git@...、https://... 或 gitee.com/owner/repo）"
            PUSH_REMOTE="$2"; shift 2
            ;;
        --push-branch)
            [[ $# -ge 2 ]] || fail "--push-branch 需要分支名"
            PUSH_BRANCH="$2"; shift 2
            ;;
        -h|--help)
            sed -n '3,21p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        -*) fail "未知参数: $1" ;;
        *)
            [[ -z "$TARGET" ]] || fail "只能指定一个目标目录"
            TARGET="$1"
            shift
            ;;
    esac
done

[[ -n "$TARGET" ]] || fail "请指定目标目录，例如 ./tools/export-public.sh ../auroradb-public"
command -v rsync >/dev/null || fail "未找到 rsync"
command -v python3 >/dev/null || fail "未找到 python3"
command -v realpath >/dev/null || fail "未找到 realpath"
if [[ -n "$PUSH_REMOTE" ]]; then
    command -v git >/dev/null || fail "未找到 git，无法 --push"
fi

# 目标目录不能与当前仓库互相包含，避免 --delete 误删源码。
# 先规范化为绝对路径（realpath -m 允许目标尚不存在），再做四类守卫：
#   1. 拒绝 / 与 $HOME；2. 拒绝目标 == 仓库根；
#   3. 拒绝目标是仓库根的祖先（rsync --delete 会清空其下所有内容）；
#   4. 拒绝目标位于仓库根内部。
TARGET="$(realpath -m -- "$TARGET")" \
    || fail "无法解析目标目录路径：$TARGET"
ROOT="$(realpath -m -- "$ROOT")"
[[ "$TARGET" != "/" && "$TARGET" != "$HOME" ]] || fail "拒绝导出到 $TARGET"
[[ "$TARGET" != "$ROOT" ]] || fail "目标目录不能是当前仓库本身：$TARGET"
case "$ROOT/" in
    "$TARGET/"*) fail "目标目录不能是当前仓库的上级目录（--delete 会误删源码）：$TARGET" ;;
esac
case "$TARGET/" in
    "$ROOT/"*) fail "目标目录不能位于当前仓库内：$TARGET" ;;
esac

if [[ -d "$TARGET" ]] && [[ -n "$(ls -A "$TARGET")" ]]; then
    [[ $FORCE -eq 1 ]] || fail "目标目录非空：$TARGET（确认覆盖请加 --force）"
fi

# 公开仓自有文件：导出时从 rsync 中排除、原地保留，绝不用私有仓版本覆盖。
# 这些文件（尤其 .github/workflows/ci.yml）与私有仓内容不同，首次导出前必须先准备好。
PUBLIC_OWNED=(
    README.md
    ROADMAP.md
    .github/workflows/ci.yml
)
for owned in "${PUBLIC_OWNED[@]}"; do
    [[ -e "$TARGET/$owned" ]] || fail "目标目录缺少公开仓自有文件：$owned
公开仓的 README.md / ROADMAP.md / .github/workflows/ci.yml 独立维护，导出时不会被覆盖。
首次导出请先克隆或复制现有公开仓到 $TARGET，再重新执行本脚本。"
done

EXCLUDES=(
    --exclude=/.git/                 # 保留目标目录已有的 git 历史/远端
    --exclude=/.trae/
    --exclude=/.tauri-tools/
    --exclude=/target/
    --exclude=/src-tauri/target/
    --exclude=/src-ui/node_modules/
    --exclude=/src-ui/dist/
    --exclude=/src-ui/playwright-report/
    --exclude=/src-ui/test-results/
    --exclude=/tools/export-public.sh
    --exclude=/tools/ship.sh           # 提交/双仓发布工作流，仅私有仓使用
    --exclude=/tools/commit-brief.sh   # 提交信息采集器，仅私有仓使用
    --exclude=/docs/EULA.md          # 专业版最终用户协议，公开仓不含
    --exclude=/.github/              # 公开仓 CI 独立维护，与私有仓不同，整体不覆盖
    --exclude=/README.md             # 公开仓 README 独立维护，不复制私有版本
    --exclude=/ROADMAP.md            # 公开仓 ROADMAP 独立维护，不复制私有版本
    --exclude=.DS_Store
    --exclude='*.log'
    --exclude=__pycache__/          # python 脚本运行产物
    --exclude='*.pyc'
)

if [[ $DRY_RUN -eq 1 ]]; then
    echo "导出源: $ROOT"
    echo "导出到: $TARGET"
    echo "排除项:"
    printf '  %s\n' "${EXCLUDES[@]#--exclude=}"
    echo ""
    echo "保留目标目录自有（不复制、不删除）："
    printf '  %s\n' "${PUBLIC_OWNED[@]}"
    echo ""
    echo "导出后将执行：README/ROADMAP/docs 内部词与相对链接检查；"
    echo "                校验 $TARGET/.github/workflows/ci.yml 仍存在。"
    if [[ -n "$PUSH_REMOTE" ]]; then
        echo ""
        echo "将在导出后推送到: $PUSH_REMOTE"
        echo "目标分支: $PUSH_BRANCH"
    fi
    exit 0
fi

echo "导出到 $TARGET"
mkdir -p "$TARGET"
# 不加 --delete-excluded：被 exclude 的路径（目标 .git、公开仓自有 README/ROADMAP/.github）
# 既不参与复制，也不会被 --delete 删除。
rsync -a --delete "${EXCLUDES[@]}" "$ROOT/" "$TARGET/"

# 目标目录中历史导出残留的私有文件（被 exclude 后 rsync 不会删除）
rm -f "$TARGET/tools/export-public.sh" \
      "$TARGET/tools/ship.sh" \
      "$TARGET/tools/commit-brief.sh" \
      "$TARGET/docs/EULA.md"

# 公开内容检查：内部词 + Markdown 相对链接。
# 目标目录的 README/ROADMAP/docs 由公开仓独立维护，必须保证不含内部信息；
# 任一检查失败都中止导出（此时尚未 push，可修复后重跑）。
python3 - "$TARGET" <<'PY'
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
INTERNAL_WORDS = ["私有", "主仓", "export-public", "Gitee 私有"]

files = [root / "README.md", root / "ROADMAP.md"]
files += sorted((root / "docs").glob("*.md"))

failures = []
for file in files:
    if not file.is_file():
        failures.append(f"缺少公开文档：{file.relative_to(root)}")
        continue
    text = file.read_text(encoding="utf-8")
    for word in INTERNAL_WORDS:
        if word in text:
            failures.append(f"{file.relative_to(root)} 含内部词「{word}」")
    for match in re.finditer(r"\[[^\]]*\]\(([^)]+)\)", text):
        link = match.group(1).strip().split(" ", 1)[0]
        if link.startswith("<") and link.endswith(">"):
            link = link[1:-1]
        # 跳过 URL、锚点、mailto 等非仓库内链接
        if re.match(r"^[a-z][a-z0-9+.-]*:", link, re.I) or link.startswith("#"):
            continue
        path = link.split("#", 1)[0]
        if not path:
            continue
        target = (file.parent / path).resolve()
        if not target.exists():
            failures.append(f"{file.relative_to(root)} 相对链接不存在：{link}")

if failures:
    print("\n".join(f"  ✗ {item}" for item in failures), file=sys.stderr)
    sys.exit(1)
PY
ok "公开文档检查通过（无内部词、相对链接均存在）"

[[ -f "$TARGET/.github/workflows/ci.yml" ]] \
    || fail "导出后缺少公开仓 CI：$TARGET/.github/workflows/ci.yml（该文件不应被导出覆盖，请检查）"
ok "已保留公开仓 CI（.github/workflows/ci.yml）"

# 重新生成 Cargo.lock：公开仓与私有仓 Cargo.toml 完全一致，只重新解析锁文件
rm -f "$TARGET/Cargo.lock"
if command -v cargo >/dev/null 2>&1; then
    if ( cd "$TARGET" && cargo generate-lockfile --offline >/dev/null 2>&1 ); then
        ok "已重新生成 Cargo.lock"
    elif ( cd "$TARGET" && cargo generate-lockfile >/dev/null 2>&1 ); then
        ok "已重新生成 Cargo.lock（联网解析）"
    else
        warn "Cargo.lock 生成失败，请在导出目录执行 cargo generate-lockfile 后提交"
    fi
else
    warn "未找到 cargo，导出目录缺少 Cargo.lock，请在构建机上执行 cargo generate-lockfile"
fi

# 导出结果自检：完整源码必须齐全（公开仓与私有仓源码一致，不再剔除 Pro）
[[ -e "$TARGET/crates/pro" ]] \
    || fail "导出目录缺少 crates/pro（完整源码导出失败）"
[[ -e "$TARGET/src-ui/src/pro" ]] \
    || fail "导出目录缺少 src-ui/src/pro（完整源码导出失败）"
[[ -f "$TARGET/src-tauri/src/commands/pro.rs" ]] \
    || fail "导出目录缺少 src-tauri/src/commands/pro.rs（完整源码导出失败）"
grep -q 'auroradb-pro' "$TARGET/src-tauri/Cargo.toml" \
    || fail "src-tauri/Cargo.toml 缺少 auroradb-pro 依赖（应保留完整源码结构）"
grep -q '"crates/pro"' "$TARGET/Cargo.toml" \
    || fail "Cargo.toml 缺少 crates/pro 成员（应保留完整 workspace）"
ok "完整源码校验通过（crates/pro、src-ui/src/pro、commands/pro.rs 均在）"

if command -v node >/dev/null 2>&1 && [[ -f "$TARGET/Cargo.lock" ]]; then
    ( cd "$TARGET" && node tools/check-version.mjs >/dev/null ) \
        && ok "版本号一致性校验通过" \
        || warn "版本号校验未通过，请在导出目录检查 Cargo.lock"
fi

echo ""
ok "导出完成：$TARGET"

if [[ -z "$PUSH_REMOTE" ]]; then
    echo "后续步骤："
    echo "  cd $TARGET"
    echo "  git init -b main && git add . && git commit -m 'chore: initial public export'"
    echo "  git remote add origin <公开仓地址> && git push -u origin main"
    exit 0
fi

# === --push 流程 ===

# 从主仓读取版本号用于 commit message（无 node 时跳过版本号前缀）
SOURCE_VERSION=""
if command -v node >/dev/null 2>&1; then
    SOURCE_VERSION="$(node -e "console.log(require('$ROOT/src-tauri/tauri.conf.json').version)")"
fi

# 标准化远端地址：允许 gitee.com/owner/repo 简写
REMOTE="$PUSH_REMOTE"
case "$REMOTE" in
    *://*|git@*) : ;;  # 已是完整 URL
    gitee.com/*|github.com/*|gitlab.com/*) REMOTE="https://$REMOTE.git" ;;
    *) fail "无法识别的远端地址格式：$PUSH_REMOTE（需 git@...、https://... 或 gitee.com/owner/repo）" ;;
esac

# 交互式确认
echo ""
echo "即将推送："
echo "  仓库：$REMOTE"
echo "  分支：$PUSH_BRANCH"
echo "  目录：$TARGET"
echo "  来源：私有主仓${SOURCE_VERSION:+ v$SOURCE_VERSION}"
read -r -p "确认执行？[y/N] " CONFIRM
[[ "$CONFIRM" =~ ^[Yy]$ ]] || { echo "已取消"; exit 0; }

# 目标目录准备
cd "$TARGET"
if [[ ! -d .git ]]; then
    git init -b "$PUSH_BRANCH" >/dev/null
    ok "已初始化 git 仓库（分支 $PUSH_BRANCH）"
else
    warn "目标目录已有 .git，保留现有历史"
    CURRENT_BRANCH="$(git branch --show-current || true)"
    if [[ "$CURRENT_BRANCH" != "$PUSH_BRANCH" ]]; then
        warn "当前分支为 $CURRENT_BRANCH，非 $PUSH_BRANCH；将在 $PUSH_BRANCH 上执行"
    fi
fi

# 设置 origin（已存在则更新）
if git remote get-url origin >/dev/null 2>&1; then
    git remote set-url origin "$REMOTE"
    ok "已更新 origin -> $REMOTE"
else
    git remote add origin "$REMOTE"
    ok "已添加 origin -> $REMOTE"
fi

# 远端可达性快速探测（不校验权限，只测连通）
if git ls-remote --exit-code origin HEAD >/dev/null 2>&1; then
    ok "远端仓库可达"
else
    RC=$?
    if [[ $RC -eq 2 ]]; then
        ok "远端仓库可达（空仓库，未创建 HEAD）"
    else
        warn "远端不可达或无读取权限，push 时会再做一次鉴权；若使用 SSH 请确认 ssh-agent 已加载对应密钥"
    fi
fi

# 确保工作区干净（排除 .gitignore 中的文件）
if [[ -n "$(git status --porcelain)" ]]; then
    git add -A
else
    ok "工作区无未提交变更，跳过 add"
fi

COMMIT_MSG="chore(export): initial public export${SOURCE_VERSION:+ from v$SOURCE_VERSION}"
if git diff --cached --quiet; then
    warn "无待提交变更，跳过 commit"
else
    git commit -q -m "$COMMIT_MSG"
    ok "已提交：$COMMIT_MSG"
fi

# 执行 push
echo ""
echo "推送中…"
PUSH_OUTPUT="$(git push -u origin "$PUSH_BRANCH" 2>&1)" || {
    PUSH_RC=$?
    echo "$PUSH_OUTPUT"
    echo ""
    case "$PUSH_OUTPUT" in
        *"ERROR: Repository not found"*|*"repository not found"*|*"No such repository"*)
            fail "远端仓库不存在或你无权限访问。请确认仓库地址拼写并已在平台创建该公开仓库。"
            ;;
        *"Permission denied (publickey)"*)
            fail "SSH 公钥认证失败。请先在本机执行 ssh-add 加载对应密钥，或改用 HTTPS + token 地址。脚本不会自动重试同一操作。"
            ;;
        *"remote contains work"*|*"rejected"*|*"non-fast-forward"*)
            fail "远端已有提交，push 被拒。请先在导出目录执行 git fetch origin，或加 --force 前确认这是你想要的行为。"
            ;;
        *"could not read Username"*|*"Authentication failed"*|*"FATAL: no authentication"*)
            fail "认证失败。HTTPS 地址需配合 token；SSH 需在 ssh-agent 中加载密钥。"
            ;;
        *"pre-push"*|*"hook"*|*"no-verify"*|*"denied"*|*"protected branch"*)
            fail "pre-push hook 拒绝或分支受保护。请在正常终端环境中重试，或明确授权后用 git push --no-verify 作为一次性逃生通道。脚本不会自动跳过 hook。"
            ;;
        *)
            fail "push 失败（退出码 $PUSH_RC）。请根据上一行日志处理后重跑脚本。"
            ;;
    esac
}

echo "$PUSH_OUTPUT" | tail -3
echo ""
ok "推送完成：$REMOTE $PUSH_BRANCH"
echo "导出目录仍保留在 $TARGET，可随时再次 --force 导出 + --push 更新"
