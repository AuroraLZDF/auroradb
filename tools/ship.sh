#!/usr/bin/env bash

# 一条命令完成提交与推送：暂存 → 校验 → 提交 → 推私有仓 → 导出并推公开仓
#
# 用法:
#   ./tools/ship.sh -m "fix(release): 修正 latest.json 上传顺序"
#   ./tools/ship.sh -m "..." --dry-run        # 只打印计划，不改动仓库
#   ./tools/ship.sh -m "..." --no-public      # 只处理私有主仓
#   ./tools/ship.sh -m "..." --no-push        # 只提交，不推送
#   ./tools/ship.sh -m "..." --public-dir ../auroradb-public
#   ./tools/ship.sh -m "..." --public-message "chore: 同步发布链路迁移"
#   ./tools/ship.sh -m "..." -y               # 跳过确认
#
# 提交信息格式 type(scope): 说明，type 取 feat/fix/docs/build/chore/test/refactor/perf/style/revert。
# 不确定写什么时先跑 ./tools/commit-brief.sh 取材料再归纳，不要用「更新代码」这类空话。
# 可重复 -m 追加正文段落。公开仓消息默认与私有仓一致，可用 --public-message 覆盖。
# 推送失败即中止，不会继续导出公开仓，避免两仓版本错位。

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
PUBLIC_MSG=""
PUBLIC_DIR="$ROOT/../auroradb-public"
DO_PUBLIC=1
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
        --public-message)
            [[ $# -ge 2 ]] || fail "--public-message 需要一个提交信息"
            PUBLIC_MSG="$2"; shift 2
            ;;
        --public-dir)
            [[ $# -ge 2 ]] || fail "--public-dir 需要一个路径"
            PUBLIC_DIR="$2"; shift 2
            ;;
        --no-public) DO_PUBLIC=0; shift ;;
        --no-push) DO_PUSH=0; shift ;;
        --dry-run) DRY_RUN=1; shift ;;
        --skip-checks) SKIP_CHECKS=1; shift ;;
        --force) SKIP_GUARDS=1; shift ;;
        -y|--yes) ASSUME_YES=1; shift ;;
        -h|--help)
            sed -n '3,19p' "$0" | sed 's/^# \{0,1\}//'
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

PUBLIC_DIR="$(realpath -m -- "$PUBLIC_DIR")"
if (( DO_PUBLIC == 1 )); then
    [[ -d "$PUBLIC_DIR" ]] || fail "公开仓目录不存在: $PUBLIC_DIR（可用 --public-dir 指定，或 --no-public 跳过）"
    git -C "$PUBLIC_DIR" rev-parse --is-inside-work-tree >/dev/null 2>&1 \
        || fail "公开仓目录不是 git 仓库: $PUBLIC_DIR"
    for owned in README.md ROADMAP.md .github/workflows/ci.yml; do
        [[ -e "$PUBLIC_DIR/$owned" ]] || fail "公开仓缺少自有文件 $owned，导出会失败: $PUBLIC_DIR"
    done
fi

echo ""
echo -e "${CYAN}=== 提交计划 ===${NC}"
echo "  私有仓: $ROOT（分支 $BRANCH${UPSTREAM:+ -> $UPSTREAM}）"
echo "  提交信息: $SUBJECT"
if (( DO_PUBLIC == 1 )); then
    echo "  公开仓: $PUBLIC_DIR（导出后提交 ${PUBLIC_MSG:-$SUBJECT}）"
else
    echo "  公开仓: 跳过"
fi
echo "  推送: $([[ $DO_PUSH -eq 1 ]] && echo '是' || echo '否')"
echo ""
./tools/commit-brief.sh --stat-only | sed 's/^/  /'

if (( DRY_RUN == 1 )); then
    echo ""
    echo -e "${YELLOW}  dry-run：以上为预览，未改动任何仓库${NC}"
    exit 0
fi

if [[ -z "$(git status --porcelain)" ]]; then
    echo ""
    echo -e "${GREEN}  ✓ 私有仓没有待提交的改动${NC}"
    warn "公开仓如有未同步内容，请单独执行 ./tools/export-public.sh \"$PUBLIC_DIR\" --force"
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
PRIVATE_SHA="$(git rev-parse --short HEAD)"
ok "私有仓已提交 $PRIVATE_SHA"

# 5. 推送私有仓
if (( DO_PUSH == 1 )); then
    if [[ -n "$UPSTREAM" ]]; then
        git push || fail "私有仓推送失败，公开仓未同步"
    else
        git push -u origin "$BRANCH" || fail "私有仓推送失败，公开仓未同步"
    fi
    ok "私有仓已推送到 ${UPSTREAM:-origin/$BRANCH}"
fi

# 6. 导出并推送公开仓：放在私有仓推送成功之后，避免版本错位
if (( DO_PUBLIC == 0 )); then
    echo ""
    ok "完成（已跳过公开仓）"
    exit 0
fi

echo ""
./tools/export-public.sh "$PUBLIC_DIR" --force \
    || fail "导出公开仓失败（私有仓已提交$([[ $DO_PUSH -eq 1 ]] && echo '并推送')）"

if [[ -z "$(git -C "$PUBLIC_DIR" status --porcelain)" ]]; then
    echo ""
    ok "公开仓无差异，无需提交"
    exit 0
fi

PUBLIC_SUBJECT="${PUBLIC_MSG:-$SUBJECT}"
git -C "$PUBLIC_DIR" add -A
git -C "$PUBLIC_DIR" commit -m "$PUBLIC_SUBJECT"
PUBLIC_SHA="$(git -C "$PUBLIC_DIR" rev-parse --short HEAD)"
ok "公开仓已提交 $PUBLIC_SHA"

if (( DO_PUSH == 1 )); then
    PUBLIC_BRANCH="$(git -C "$PUBLIC_DIR" rev-parse --abbrev-ref HEAD)"
    PUBLIC_UPSTREAM="$(git -C "$PUBLIC_DIR" rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>/dev/null || true)"
    if [[ -n "$PUBLIC_UPSTREAM" ]]; then
        git -C "$PUBLIC_DIR" push || fail "公开仓推送失败（两仓提交已完成，可进入 $PUBLIC_DIR 手动 git push）"
    else
        git -C "$PUBLIC_DIR" push -u origin "$PUBLIC_BRANCH" \
            || fail "公开仓推送失败（两仓提交已完成，可进入 $PUBLIC_DIR 手动 git push）"
    fi
    ok "公开仓已推送到 ${PUBLIC_UPSTREAM:-origin/$PUBLIC_BRANCH}"
fi

echo ""
echo -e "${GREEN}完成：私有仓 $PRIVATE_SHA / 公开仓 $PUBLIC_SHA${NC}"
