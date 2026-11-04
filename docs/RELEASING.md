# 构建、发布与发版手册

面向维护者。普通安装请直接看 [README](../README.md) 的「下载安装」。

## 分发方式

安装包通过 GitHub Releases 分发，AppImage（full / lite）、deb、rpm、NSIS、各自的 `.sig`、`SHA256SUMS` 与 `latest.json` 全部作为发行版附件上传，单附件上限 2GB。

应用内更新的固定入口是：

```
https://github.com/AuroraLZDF/auroradb/releases/latest/download/latest.json
```

该地址配置在 `src-tauri/tauri.conf.json` 的 `plugins.updater.endpoints`，每次发布自动更新内容，无需随版本改动。`latest.json` 按平台 key（`linux-x86_64`、`windows-x86_64`）合并，两个平台先后发布互不覆盖。

## 令牌配置

令牌在 GitHub「Settings → Developer settings → Personal access tokens」生成：

- 经典令牌：勾选 `repo`
- 细粒度令牌：Repository access 选中目标仓库，Permissions → `Contents: Read and write`

Linux 脚本只从环境变量读取 `GITHUB_TOKEN` 或 `GH_TOKEN`；Windows 脚本额外支持把令牌写入 `%USERPROFILE%\.config\auroradb\github.token`。令牌不要提交进仓库。

令牌有效不等于有写权限。`GET /repos/<owner>/<repo>` 返回 200 只说明令牌可读该仓库；带 `{}` 空 body 的 `POST /releases` 返回 422 是 GitHub 先校验请求体，**不能**据此判断有写权限。真正能暴露权限问题的是带 `tag_name` 的真实 POST，返回 403 `Resource not accessible by personal access token` 即令牌对该仓库只读。

## 发布命令

在仓库根目录执行，先完成构建（见 [README](../README.md#构建与发布)）：

```bash
export GITHUB_TOKEN=<令牌>

./tools/publish/linux.sh                          # 发布 Linux 产物
./tools/publish/linux.sh --tag v2.1.0 --notes "…" # 指定 tag 与发行说明
./tools/publish/linux.sh --dry-run                # 只在本机校验并打印发布计划
./tools/publish/linux.sh --repo owner/repo        # 覆盖发布仓库（默认与 origin 同名）
./tools/publish/linux.sh --extra-artifact <文件>  # 追加附件，可重复（如 lite AppImage）
```

```powershell
$env:GITHUB_TOKEN = "<令牌>"

.\tools\publish\windows.ps1
.\tools\publish\windows.ps1 -Tag v2.1.0 -Notes "…"
.\tools\publish\windows.ps1 -Repo owner/repo -BundleDir <目录>
```

## 发布行为

- 上传前打印版本、标签、分支、提交、工作区状态与 tag 推送状态，需输入 `yes` 确认
- Linux 脚本强制调用 `tools/check-release.sh`，校验产物齐全、`.sig` 存在、体积不超限、更新源指向发布仓库，并生成 `SHA256SUMS`
- 任一附件上传失败立即中止，不写入 `latest.json`，避免用户更新到不存在的地址
- 同名附件先删除再上传，重复发布同一 tag 会覆盖旧产物
- 若线上已有更高版本的发行版，重新发布旧 tag 会被拒绝，防止 `/releases/latest` 分发的清单回退

## 发布前检查

1. 按 [发版冒烟清单](smoke-checklist.md) 在 Linux 与 Windows 上各跑一遍
2. `node tools/check-version.mjs` 确认所有版本号一致
3. `./tools/publish/linux.sh --dry-run` 预览上传集
4. 需要时用 `./tools/check-release.sh` 单独核对产物

## 升版本与打 tag

用脚本一次完成版本同步、校验、提交与打 tag：

```bash
./tools/release-tag.sh 2.1.0             # 升到 2.1.0，提交并打 tag v2.1.0
./tools/release-tag.sh v2.1.0            # 版本号可带 v 前缀
./tools/release-tag.sh 2.1.0 --dry-run   # 只预览改动，不落盘
./tools/release-tag.sh 2.1.0 --push      # 完成后推送到 origin
```

脚本同步 `src-tauri/tauri.conf.json`、`src-ui/package.json`、`src-ui/package-lock.json`、`src-tauri/Cargo.toml`、`crates/core/Cargo.toml`、`crates/pro/Cargo.toml`、`Cargo.lock` 与 README 的版本展示，随后跑 `node tools/check-version.mjs`。要求工作区干净、新版本高于当前版本、同名 tag 不存在；并校验两个锁定文件只改了本仓库的版本行，避免依赖版本被误改。

手工等价操作（版本号以 `src-tauri/tauri.conf.json` 为准）：

```bash
node tools/check-version.mjs
git add -A && git commit -m "chore(release): v2.1.0"
git tag -a v2.1.0 -m "AuroraDB 2.1.0"
git push origin main --tags
```

手工改版本号时注意：`src-ui/package-lock.json` 里依赖自身也有 `version` 字段，只能改文件头部的根字段与 `packages[""]`；`Cargo.lock` 只改 `auroradb`、`auroradb-core`、`auroradb-pro` 三处。

## 维护者工具

- `./tools/commit-brief.sh`：汇总分支状态、改动文件、diff 片段与最近提交风格，用于归纳提交信息
- `./tools/ship.sh -m "type(scope): 说明"`：校验版本号、拦截敏感与大文件，提交并推送私有仓后同步公开仓

## 常见报错

- **403 `Resource not accessible by personal access token`**：令牌对该仓库没有写权限，按上文补齐 `Contents: Read and write`
- **`curl: (56) OpenSSL SSL_read: ... unexpected eof`** 或 HTTP 000：本机到 `api.github.com` / `uploads.github.com` 不通，多为代理或 VPN 问题。用 `curl -sS -o /dev/null -w '%{http_code}\n' https://api.github.com/` 验证；返回 000 或 TLS 握手失败时切换节点再试，与令牌无关
- **`/releases/latest` 返回 404**：仓库还没有正式发布的发行版（草稿不计入），先完成一次发布
- **附件上传失败且提示体积超限**：单个附件不得超过 2GB，AppImage 可改用 lite 或分卷方案
