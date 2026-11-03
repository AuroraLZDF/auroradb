#!/usr/bin/env bash

# 从私有主仓导出发行开源公开仓（AGPL-3.0 部分）
#
# 用法:
#   ./tools/export-public.sh <目标目录>
#   ./tools/export-public.sh ../auroradb-public --force
#   ./tools/export-public.sh ../auroradb-public --dry-run
#   ./tools/export-public.sh ../auroradb-public --push gitee.com/owner/auroradb-public
#   ./tools/export-public.sh ../auroradb-public --push git@github.com:owner/auroradb-public.git
#   ./tools/export-public.sh ../auroradb-public --push https://gitee.com/owner/auroradb-public.git
#
# 导出内容：crates/core、免费的 src-tauri 命令层、免费前端与构建发布脚本
# 排除：crates/pro、src-ui/src/pro、src-tauri/src/commands/pro.rs、
#       tools/license-gen（激活码签发）以及全部构建产物
# 导出后自动移除专业版 workspace 成员与 Cargo feature，并重新生成 Cargo.lock。
# 加 --push 时会在导出后自动 init git、提交并推送到给定远端地址的 main 分支。

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

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
            sed -n '3,18p' "$0" | sed 's/^# \{0,1\}//'
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
if [[ -n "$PUSH_REMOTE" ]]; then
    command -v git >/dev/null || fail "未找到 git，无法 --push"
fi

# 目标目录不能与当前仓库互相包含，避免 --delete 误删源码
TARGET_PARENT="$(dirname "$TARGET")"
[[ -d "$TARGET_PARENT" ]] || fail "目标目录的上级目录不存在：$TARGET_PARENT"
TARGET="$(cd "$TARGET_PARENT" && pwd)/$(basename "$TARGET")"
[[ "$TARGET" != "/" && "$TARGET" != "$HOME" ]] || fail "拒绝导出到 $TARGET"
case "$TARGET/" in
    "$ROOT/"*) fail "目标目录不能位于当前仓库内：$TARGET" ;;
esac

if [[ -d "$TARGET" ]] && [[ -n "$(ls -A "$TARGET")" ]]; then
    [[ $FORCE -eq 1 ]] || fail "目标目录非空：$TARGET（确认覆盖请加 --force）"
fi

EXCLUDES=(
    --exclude=/.git/
    --exclude=/.trae/
    --exclude=/.tauri-tools/
    --exclude=/target/
    --exclude=/src-tauri/target/
    --exclude=/tools/license-gen/
    --exclude=/src-ui/node_modules/
    --exclude=/src-ui/dist/
    --exclude=/src-ui/playwright-report/
    --exclude=/src-ui/test-results/
    --exclude=/crates/pro/
    --exclude=/src-ui/src/pro/
    --exclude=/src-tauri/src/commands/pro.rs
    --exclude=/tools/export-public.sh
    --exclude=/docs/EULA.md          # 专业版最终用户协议，公开仓不含
    --exclude=/.github/workflows/    # CI 平台私有主仓用不到，公开仓独立维护
    --exclude=.DS_Store
    --exclude='*.log'
)

if [[ $DRY_RUN -eq 1 ]]; then
    echo "导出源: $ROOT"
    echo "导出到: $TARGET"
    echo "排除项:"
    printf '  %s\n' "${EXCLUDES[@]#--exclude=}"
    if [[ -n "$PUSH_REMOTE" ]]; then
        echo ""
        echo "将在导出后推送到: $PUSH_REMOTE"
        echo "目标分支: $PUSH_BRANCH"
    fi
    exit 0
fi

echo "导出到 $TARGET"
mkdir -p "$TARGET"
# 不加 --delete-excluded：目标目录若已 init 过 git，其 .git 会被保留
rsync -a --delete "${EXCLUDES[@]}" "$ROOT/" "$TARGET/"

# 目标目录中历史导出残留的专业版文件（被 exclude 后 rsync 不会删除）
rm -rf "$TARGET/crates/pro" \
       "$TARGET/src-ui/src/pro" \
       "$TARGET/src-tauri/src/commands/pro.rs" \
       "$TARGET/tools/license-gen"
rm -f "$TARGET/tools/export-public.sh" \
      "$TARGET/docs/EULA.md"
rm -rf "$TARGET/.github/workflows"

# 移除专业版依赖、feature 与 workspace 成员，失败即中止（结构变更时需同步本脚本）
python3 - "$TARGET" <<'PY'
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])


def edit(path, transform):
    file = root / path
    text = file.read_text(encoding="utf-8")
    updated = transform(text)
    if updated == text:
        raise SystemExit(f"导出失败：{path} 未匹配到预期内容，请同步更新 tools/export-public.sh")
    file.write_text(updated, encoding="utf-8")


def root_members(text):
    return re.sub(r'"crates/core",\s*"crates/pro",\s*', '"crates/core", ', text, count=1)


def tauri_manifest(text):
    # 1. 删掉 auroradb-pro 依赖行
    text = re.sub(r"^auroradb-pro = .*\r?\n", "", text, count=1, flags=re.M)
    # 2. 删掉"专业版：..."注释行
    text = re.sub(r"^# 专业版：.*\r?\n", "", text, count=1, flags=re.M)
    # 3. 保留 pro 作为**空 feature**，但不再绑定 dep:auroradb-pro
    #    —— 代码里的 #[cfg(feature = "pro")] 仍能匹配到合法 feature 名称，
    #       不会触发 unexpected-cfgs lint；启用后无任何依赖被拉入，编译器行为和免费版一致
    text = re.sub(r'^pro = \["dep:auroradb-pro"\]\r?\n', 'pro = []\n', text, count=1, flags=re.M)
    # 4. default 里去掉 pro（公开仓默认免费版）
    return text.replace('default = ["custom-protocol", "pro"]', 'default = ["custom-protocol"]')


edit("Cargo.toml", root_members)
edit("src-tauri/Cargo.toml", tauri_manifest)


# README 清理：把私有仓特有的内部信息和目录条目替换掉，
# 输出一个适合公开仓读者（开源贡献者和用户）的版本。
# 未匹配到某条规则时静默跳过（可能是私有 README 措辞改了，不致命）。
def clean_readme(text):
    import re as _re

    def _sub(pattern, repl, **kw):
        nonlocal text
        text = _re.sub(pattern, repl, text, **kw)

    def _replace_section(header, repl_body):
        """整块替换从 ## header 到下一个 ## 或文件结尾的内容。"""
        nonlocal text
        pat = r"(?ms)^## " + _re.escape(header) + r"\n.*?(?=^## \S|\Z)"
        text = _re.sub(pat, f"## {header}\n\n{repl_body}\n\n", text)

    # 1. 开头定位句 + 仓库身份声明
    _sub(
        r"基于 Tauri 2 \+ Vue 3 \+ Rust 的桌面数据库客户端，支持 MySQL 与 PostgreSQL，"
        r"打包目标为 Linux（deb / AppImage / rpm）与 Windows（NSIS）。",
        "基于 Tauri 2 + Vue 3 + Rust 的开源桌面数据库客户端，支持 MySQL 与 PostgreSQL。\n\n"
        "本仓库是 AuroraDB 的**开源核心**，以 [AGPL-3.0](LICENSE) 授权发布。"
        "完整功能版本与安装包见 [项目官网](https://gitee.com/auroraLZDF/auroradb-releases)。",
    )

    # 2. 删除功能列表后的专业版段落
    _sub(r"\n专业版需激活码解锁，另含：[^。]*。\n", "\n")

    # 3. 从技术栈表格删除"授权"行（内部实现细节）
    _sub(r"^\| 授权 \|[^\n]*\n", "", flags=_re.M)

    # 4. 整个"目录结构"章节——私有版包含 pro / license-gen / export-public.sh 等私有条目，
    #    清理后缩进会乱，直接整块替换成公开仓版本最可靠
    _replace_section(
        "目录结构",
        "```\n"
        ".\n"
        "├── crates/core/            # 核心库 auroradb-core（连接、执行、导入导出、授权校验）\n"
        "├── src-ui/                 # Vue 3 前端\n"
        "│   ├── src/components/     # 业务组件与基础 UI 组件\n"
        "│   ├── src/stores/         # Pinia 状态（连接、数据浏览、授权、更新等）\n"
        "│   ├── src/i18n/           # 轻量 i18n（中/英）\n"
        "│   └── src/utils/          # SQL、表格变更、执行计划类型与 Pro 界面占位注册表\n"
        "├── src-tauri/              # Tauri 应用（命令注册、窗口与更新）\n"
        "│   └── src/commands/       # Tauri 命令层\n"
        "├── tools/\n"
        "│   ├── build/              # 构建实现（linux.sh / windows.ps1 / appimage-*.sh）\n"
        "│   ├── publish/            # 发布实现（linux.sh / windows.ps1）\n"
        "│   ├── check-version.mjs   # 版本号一致性校验\n"
        "│   ├── check-release.sh    # 发布前产物检查\n"
        "│   └── merge-latest.py     # latest.json 合并\n"
        "├── docs/\n"
        "│   └── smoke-checklist.md  # 发版冒烟清单\n"
        "├── LICENSE                 # AGPL-3.0\n"
        "├── CLA.md                  # 贡献者许可协议\n"
        "├── NOTICE                  # 第三方组件许可声明\n"
        "└── start.sh                # 一键启动 Tauri 开发模式（首次运行需先装前端依赖）\n"
        "```\n",
    )

    # 5. 开发检查里的免费版构建注释与多余空行
    _sub(
        r"(\n)?# 校验免费版构建（不启用 pro feature，对应公开仓）\ncargo check --offline --no-default-features\n",
        "\n",
    )
    # 开发检查代码块结尾多出的空行
    _sub(r"^cargo test --offline\n\n```", "cargo test --offline\n```", flags=_re.M)

    # 6. 发布到 Gitee 章节开头的"源码仓库保持私有"
    _sub(r"源码仓库保持私有，安装包发布到独立的\*\*公开\*\*仓库[^。]*。",
         "安装包发布到独立渠道。")

    # 7. 手工发版步骤里的 crates/pro 与"自动跳过"说明（注意：带反引号！）
    _sub(
        r"需同步 `src-ui/package\.json`、`src-ui/package-lock\.json`、"
        r"`src-tauri/Cargo\.toml`、`crates/core/Cargo\.toml`、`crates/pro/Cargo\.toml`"
        r" 与根目录 `Cargo\.lock`（公开仓没有 `crates/pro`，`check-version\.mjs` 会自动跳过）",
        "需同步 `src-ui/package.json`、`src-ui/package-lock.json`、"
        "`src-tauri/Cargo.toml`、`crates/core/Cargo.toml` 与根目录 `Cargo.lock`",
    )

    # 8. 整个"授权与许可"章节
    _replace_section(
        "授权与许可",
        "本仓库全部代码依据 [GNU AGPL-3.0](LICENSE) 授权。您可以自由使用、修改和分发，"
        "但在分发修改后的版本或通过网络提供服务时，需按 AGPL 第 6/13 条提供完整对应源码。\n\n"
        "- 第三方组件及许可证见 [NOTICE](NOTICE)\n"
        "- 提交贡献前请阅读 [CLA.md](CLA.md)，首次提交需在 Pull Request 中签署\n"
        "- 完整功能版本包含额外的商业许可组件，安装包与商业许可请通过 [项目官网](https://gitee.com/auroraLZDF/auroradb-releases) 获取",
    )

    # 9. 删除结尾单独的激活码签发工具段落
    _sub(r"^激活码签发工具位于私有主仓的 [^\n]*\n?", "", flags=_re.M)

    # 10. 功能列表与下一章节之间可能有多余空行
    _sub(r"\n{3,}", "\n\n")

    return text


(root / "README.md").write_text(clean_readme((root / "README.md").read_text(encoding="utf-8")),
                                 encoding="utf-8")
PY
ok "已清理 README 中的私有发行细节"
ok "已移除专业版 workspace 成员与 Cargo feature"

# 重新解析依赖：移除 auroradb-pro 及其独占的传递依赖
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

# 导出结果自检
[[ ! -e "$TARGET/crates/pro" ]] || fail "导出目录仍包含 crates/pro"
[[ ! -e "$TARGET/src-ui/src/pro" ]] || fail "导出目录仍包含 src-ui/src/pro"
[[ ! -e "$TARGET/src-tauri/src/commands/pro.rs" ]] || fail "导出目录仍包含 commands/pro.rs"
grep -q 'auroradb-pro' "$TARGET/src-tauri/Cargo.toml" && fail "src-tauri/Cargo.toml 仍引用 auroradb-pro"
grep -q '"crates/pro"' "$TARGET/Cargo.toml" && fail "Cargo.toml 仍包含 crates/pro 成员"

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
