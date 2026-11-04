#!/usr/bin/env bash

# 一条命令完成提交与推送：暂存 → 校验 → 提交 → 推送 origin
#
# 用法:
#   ./tools/ship.sh -m "fix(release): 修正 latest.json 上传顺序"
#   ./tools/ship.sh -m "..." --dry-run        # 只打印计划，不改动仓库
#   ./tools/ship.sh -m "..." --no-push        # 只提交，不推送
#   ./tools/ship.sh -m "..." -y               # 跳过确认
#
# 提交信息格式 type(scope): 说明，type 取 feat/fix/docs/build/chore/test/refactor/perf/style/revert。
# 不确定写什么时先跑 ./tools/commit-brief.sh 取材料再归纳，不要用「更新代码」这类空话。
# 可重复 -m 追加正文段落。推送失败即中止。

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

fail() { echo -e "${RED}  ✗ $*${NC}"; exit 1; }
ok()   { echo -e "${GREEN}  ✓ $*${NC}"; }
warn() { echo -e "${YELLOW}  ! $*${NC}"; }

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$ROOT"

MSG_ARGS=()
DO_PUSH=1
DRY_RUN=0
ASSUME_YES=0
SKIP_CHECKS=0
SKIP_GUARDS=0

while [[ $# -gt 0 ]]; do
    case $1 in
        -m|--message)
            [[ $# -ge 2 ]] || fail "$1 需要一个提交信息"
            MSG_ARGS+=(-m "$2"); shift 2
            ;;
        --no-push) DO_PUSH=0; shift ;;
        --dry-run) DRY_RUN=1; shift ;;
        --skip-checks) SKIP_CHECKS=1; shift ;;
        --force) SKIP_GUARDS=1; shift ;;
        -y|--yes) ASSUME_YES=1; shift ;;
        -h|--help)
            sed -n '3,14p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) fail "未知参数: $1" ;;
    esac
done

(( ${#MSG_ARGS[@]} > 0 )) || fail "缺少提交信息，用法：./tools/ship.sh -m \"fix(release): 说明\"（先用 ./tools/commit-brief.sh 取材料）"

SUBJECT="${MSG_ARGS[1]}"
if [[ ! "$SUBJECT" =~ ^(feat|fix|docs|build|chore|test|refactor|perf|style|revert)(\([A-Za-z0-9._-]+\))?:\ .+ ]]; then
    fail "提交信息需为 type(scope): 说明 格式，当前为：$SUBJECT"
fi

git rev-parse --is-inside-work-tree >/dev/null 2>&1 || fail "当前目录不是 git 仓库: $ROOT"
BRANCH="$(git rev-parse --abbrev-ref HEAD)"
UPSTREAM="$(git rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>/dev/null || true)"

echo ""
echo -e "${CYAN}=== 提交计划 ===${NC}"
echo "  仓库: $ROOT（分支 $BRANCH${UPSTREAM:+ -> $UPSTREAM}）"
echo "  提交信息: $SUBJECT"
echo "  推送: $([[ $DO_PUSH -eq 1 ]] && echo '是' || echo '否')"
echo ""
./tools/commit-brief.sh --stat-only | sed 's/^/  /'

if (( DRY_RUN == 1 )); then
    echo ""
    echo -e "${YELLOW}  dry-run：以上为预览，未改动仓库${NC}"
    exit 0
fi

if [[ -z "$(git status --porcelain)" ]]; then
    echo ""
    echo -e "${GREEN}  ✓ 没有待提交的改动${NC}"
    exit 0
fi

if (( ASSUME_YES == 0 )) && [[ -t 0 ]]; then
    echo ""
    read -r -p "确认执行提交与推送？[y/N] " answer
    [[ "$answer" == "y" || "$answer" == "Y" ]] || { echo "已取消"; exit 0; }
fi

# 1. 暂存
git add -A
if git diff --cached --quiet; then
    echo -e "${GREEN}  ✓ 没有需要提交的改动${NC}"
    exit 0
fi

# 2. 敏感文件与超大文件守卫：避免把密钥、令牌或构建产物提交进历史
if (( SKIP_GUARDS == 0 )); then
    BLOCKED=()
    while IFS= read -r path; do
        [[ -n "$path" ]] || continue
        base="${path##*/}"
        case "$base" in
            .env|.env.*|*.pem|*.key|*.p12|*.pfx|id_rsa*|*credentials*|*token*) BLOCKED+=("$path") ;;
        esac
    done < <(git diff --cached --name-only --diff-filter=ACM)
    if (( ${#BLOCKED[@]} > 0 )); then
        fail "疑似敏感文件进入暂存区：${BLOCKED[*]}
确认无误后加 --force 重新执行，或先把它们加入 .gitignore 并 git restore --staged"
    fi

    TOO_BIG=()
    while IFS= read -r path; do
        [[ -f "$path" ]] || continue
        size="$(stat -c %s "$path" 2>/dev/null || echo 0)"
        (( size > 5 * 1024 * 1024 )) && TOO_BIG+=("$path ($((size / 1024 / 1024))MB)")
    done < <(git diff --cached --name-only --diff-filter=ACM)
    if (( ${#TOO_BIG[@]} > 0 )); then
        fail "暂存区有超过 5MB 的文件：${TOO_BIG[*]}
构建产物不应进入仓库；确认无误后加 --force 重新执行"
    fi
fi

# 3. 版本号一致性：配置漂移时先修再提交
if (( SKIP_CHECKS == 0 )) && command -v node >/dev/null 2>&1 && [[ -f tools/check-version.mjs ]]; then
    ( node tools/check-version.mjs >/dev/null ) || fail "版本号校验未通过，请先修版本再提交（跳过用 --skip-checks）"
    ok "版本号一致性校验通过"
fi

# 4. 提交
git commit "${MSG_ARGS[@]}"
SHA="$(git rev-parse --short HEAD)"
ok "已提交 $SHA"

# 5. 推送
if (( DO_PUSH == 1 )); then
    if [[ -n "$UPSTREAM" ]]; then
        git push || fail "推送失败（提交已完成，可手动 git push）"
    else
        git push -u origin "$BRANCH" || fail "推送失败（提交已完成，可手动 git push）"
    fi
    ok "已推送到 ${UPSTREAM:-origin/$BRANCH}"
fi

echo ""
echo -e "${GREEN}完成：$SHA${NC}"
