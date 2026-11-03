#!/usr/bin/env bash

# 汇总本次改动，供人工或模型归纳提交信息（只读，不做任何提交动作）
#
# 用法:
#   ./tools/commit-brief.sh               # 工作区 + 暂存区全部改动
#   ./tools/commit-brief.sh --staged      # 只看已暂存的改动
#   ./tools/commit-brief.sh --stat-only   # 只列文件与增删行数，不展开改动内容
#   ./tools/commit-brief.sh --context 80  # 每个文件最多展示 80 行改动（默认 40）
#
# 输出：分支与上游同步状态、版本号与最新 tag、按目录分组的变更文件、
#       逐个文件的改动片段（跳过 lock 文件与构建产物）、最近提交的风格样例。
# 结尾给出下一步命令；真正的提交与推送由 tools/ship.sh 执行。

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

fail() { echo -e "${RED}  ✗ $*${NC}"; exit 1; }

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$ROOT"

SCOPE="all"
STAT_ONLY=0
CONTEXT=40

while [[ $# -gt 0 ]]; do
    case $1 in
        --staged) SCOPE="staged"; shift ;;
        --stat-only) STAT_ONLY=1; shift ;;
        --context) CONTEXT="${2:-}"; shift 2 ;;
        -h|--help)
            sed -n '3,14p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) fail "未知参数: $1" ;;
    esac
done

[[ "$CONTEXT" =~ ^[0-9]+$ ]] || fail "--context 需要非负整数，当前为 $CONTEXT"
command -v git >/dev/null || fail "未找到 git"
git rev-parse --is-inside-work-tree >/dev/null 2>&1 || fail "当前目录不是 git 仓库: $ROOT"

if [[ "$SCOPE" == "staged" ]]; then
    DIFF_REF=("--cached")
    SCOPE_LABEL="暂存区"
else
    if git rev-parse -q --verify HEAD >/dev/null 2>&1; then
        DIFF_REF=("HEAD")
    else
        DIFF_REF=("--cached")
    fi
    SCOPE_LABEL="工作区 + 暂存区"
fi

BRANCH="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo 'unknown')"
UPSTREAM="$(git rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>/dev/null || true)"
VERSION="$(grep -m1 '"version"' src-tauri/tauri.conf.json 2>/dev/null \
    | sed 's/.*"version": *"\([^"]*\)".*/\1/' || true)"
TAG="$(git describe --tags --abbrev=0 2>/dev/null || true)"

echo ""
echo -e "${CYAN}=== 仓库概况 ===${NC}"
echo "  路径: $ROOT"
echo "  范围: $SCOPE_LABEL"
if [[ -n "$UPSTREAM" ]]; then
    read -r BEHIND AHEAD < <(git rev-list --left-right --count "$UPSTREAM...HEAD" 2>/dev/null || echo "0 0")
    echo "  分支: $BRANCH -> $UPSTREAM（领先 $AHEAD / 落后 $BEHIND）"
else
    echo "  分支: $BRANCH（未设置上游分支）"
fi
echo "  版本: ${VERSION:-未知}（最新 tag: ${TAG:-无}）"

# 变更文件：状态 + 增删行数
declare -A STATUS_OF=()
declare -A NUMSTAT=()
declare -a ORDER=()

add_entry() {
    local path="$1" status="$2"
    if [[ -z "${STATUS_OF[$path]+x}" ]]; then
        ORDER+=("$path")
    fi
    STATUS_OF["$path"]="$status"
}

while IFS=$'\t' read -r st p1 p2; do
    [[ -n "${st:-}" ]] || continue
    path="${p2:-$p1}"
    add_entry "$path" "${st:0:1}"
done < <(git diff --name-status "${DIFF_REF[@]}")

if [[ "$SCOPE" == "all" ]]; then
    while IFS= read -r path; do
        [[ -n "$path" ]] || continue
        add_entry "$path" "?"
    done < <(git ls-files --others --exclude-standard)
fi

while IFS=$'\t' read -r add del path; do
    [[ -n "${path:-}" ]] || continue
    NUMSTAT["$path"]="$add/$del"
done < <(git diff --numstat "${DIFF_REF[@]}" 2>/dev/null | awk -F'\t' 'NF>=3 {print $1"\t"$2"\t"$NF}')

if (( ${#ORDER[@]} == 0 )); then
    echo ""
    echo -e "${GREEN}  ✓ ${SCOPE_LABEL}干净，没有可提交的改动${NC}"
    exit 0
fi

ADDED=0
REMOVED=0
for path in "${ORDER[@]}"; do
    stat="${NUMSTAT[$path]:-}"
    if [[ "$stat" =~ ^([0-9]+)/([0-9]+)$ ]]; then
        ADDED=$(( ADDED + ${BASH_REMATCH[1]} ))
        REMOVED=$(( REMOVED + ${BASH_REMATCH[2]} ))
    fi
done
echo "  变更: ${#ORDER[@]} 个文件（+$ADDED / -$REMOVED）"

echo ""
echo -e "${CYAN}=== 变更文件（按目录）===${NC}"
declare -A DIRS=()
for path in "${ORDER[@]}"; do
    dir="$(dirname "$path")"
    [[ "$dir" == "." ]] && dir="(仓库根)"
    DIRS["$dir"]+="$path"$'\n'
done
while IFS= read -r dir; do
    [[ -n "$dir" ]] || continue
    echo "  $dir"
    while IFS= read -r path; do
        [[ -n "$path" ]] || continue
        st="${STATUS_OF[$path]}"
        case "$st" in
            A) label="新增" ;;
            M) label="修改" ;;
            D) label="删除" ;;
            R) label="重命名" ;;
            ?) label="未跟踪" ;;
            *) label="$st" ;;
        esac
        stat="${NUMSTAT[$path]:-}"
        if [[ -n "$stat" ]]; then
            printf '    %-6s %-44s +%s -%s\n' "$label" "$path" "${stat%%/*}" "${stat##*/}"
        else
            printf '    %-6s %s\n' "$label" "$path"
        fi
    done < <(printf '%s' "${DIRS[$dir]}")
done < <(printf '%s\n' "${!DIRS[@]}" | sort)

should_skip() {
    case "$1" in
        package-lock.json|*/package-lock.json) return 0 ;;
        Cargo.lock|*/Cargo.lock|*.lock) return 0 ;;
        *.min.js|*.min.css|*.map) return 0 ;;
        *.png|*.jpg|*.jpeg|*.gif|*.ico|*.webp|*.pdf) return 0 ;;
        *.woff|*.woff2|*.ttf|*.eot) return 0 ;;
    esac
    return 1
}

if (( STAT_ONLY == 0 )); then
    echo ""
    echo -e "${CYAN}=== 改动内容（每个文件最多 $CONTEXT 行）===${NC}"
    SKIPPED=()
    TRUNCATED=0
    for path in "${ORDER[@]}"; do
        if should_skip "$path"; then
            SKIPPED+=("$path")
            continue
        fi
        st="${STATUS_OF[$path]}"
        if [[ "$st" == "?" ]]; then
            echo ""
            echo -e "${YELLOW}+++ $path（未跟踪，仅列前 $CONTEXT 行）${NC}"
            head -n "$CONTEXT" -- "$path" 2>/dev/null || true
        elif [[ "$st" == "D" ]]; then
            echo ""
            echo -e "${YELLOW}--- $path（已删除）${NC}"
        else
            echo ""
            echo -e "${YELLOW}--- $path${NC}"
            LINES="$(git diff "${DIFF_REF[@]}" -- "$path" | wc -l)"
            git diff "${DIFF_REF[@]}" -- "$path" | head -n "$CONTEXT" || true
            if (( LINES > CONTEXT )); then
                TRUNCATED=$(( TRUNCATED + 1 ))
                echo "    …（共 $LINES 行，已截断）"
            fi
        fi
    done
    if (( ${#SKIPPED[@]} > 0 )); then
        echo ""
        echo "  已跳过（lock 文件 / 图片 / 字体 / 压缩产物）：${SKIPPED[*]}"
    fi
    if (( TRUNCATED > 0 )); then
        echo "  提示：$TRUNCATED 个文件的改动被截断，需要完整内容请加 --context 或直接 git diff"
    fi
fi

echo ""
echo -e "${CYAN}=== 最近提交风格 ===${NC}"
git log --oneline -15 2>/dev/null | sed 's/^/  /' || true

if (( STAT_ONLY == 0 )); then
    echo ""
    echo -e "${CYAN}=== 下一步 ===${NC}"
    echo "  1. 按 type(scope): 说明 归纳一句话（说清为什么改，不罗列文件）"
    echo "  2. 执行提交与推送："
    echo "     ./tools/ship.sh -m \"fix(release): 修正 latest.json 上传顺序\""
    echo ""
fi
