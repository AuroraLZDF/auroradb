# 构建、发布与发版手册

面向维护者。普通安装请直接看 [README](../README.md) 的「下载安装」。

## 分发方式

安装包通过 GitHub Releases 分发，AppImage（full / lite）、deb、rpm、NSIS、macOS 的 dmg 与 `.app.tar.gz`、各自的 `.sig`、`SHA256SUMS` 与 `latest.json` 全部作为发行版附件上传，单附件上限 2GB。

应用内更新的固定入口是：

```
https://github.com/AuroraLZDF/auroradb/releases/latest/download/latest.json
```

该地址配置在 `src-tauri/tauri.conf.json` 的 `plugins.updater.endpoints`，每次发布自动更新内容，无需随版本改动。`latest.json` 按平台 key（`linux-x86_64`、`windows-x86_64`、`darwin-aarch64`）合并，各平台先后发布互不覆盖。

## 令牌配置

令牌在 GitHub「Settings → Developer settings → Personal access tokens」生成：

- 经典令牌：勾选 `repo`
- 细粒度令牌：Repository access 选中目标仓库，Permissions → `Contents: Read and write`

Linux 脚本只从环境变量读取 `GITHUB_TOKEN` 或 `GH_TOKEN`；Windows 脚本额外支持把令牌写入 `%USERPROFILE%\.config\auroradb\github.token`。令牌不要提交进仓库。

令牌有效不等于有写权限。`GET /repos/<owner>/<repo>` 返回 200 只说明令牌可读该仓库；带 `{}` 空 body 的 `POST /releases` 返回 422 是 GitHub 先校验请求体，**不能**据此判断有写权限。真正能暴露权限问题的是带 `tag_name` 的真实 POST，返回 403 `Resource not accessible by personal access token` 即令牌对该仓库只读。

## 自动发布（推荐）

打 tag 即发布，两级流水线在 `.github/workflows/release.yml`：

```bash
# 1. 改版本号（tools/release-tag.sh 会自动改全部位置并打 tag）
./tools/release-tag.sh 2.2.2            # 只升版本、提交、打 tag
git push origin main && git push origin v2.2.2
```

推 tag 后 CI 自动完成：

| Job | 内容 |
|---|---|
| `check` | 版本单一来源校验、tag 与 `tauri.conf.json` 版本比对、前端 type-check/lint/test |
| `build-linux` | deb + AppImage（重压签名）+ rpm，产物打成 tar |
| `build-windows` | NSIS 安装包 + `.sig` |
| `build-macos` | macOS arm64（未签名预览版）：`.app` + `.dmg` + `.app.tar.gz`/`.sig` |
| `publish-linux` | 调 `tools/publish/linux.sh --yes`，写 `linux-x86_64` 条目 |
| `publish-windows` | 调 `tools/publish/windows.ps1 -Yes`，合并 `windows-x86_64` 条目 |
| `publish-macos` | 调 `tools/publish/macos.sh --yes`，合并 `darwin-aarch64` 条目 |
| `verify` | 抓线上 `latest.json`，断言版本与 tag 一致且三个平台都在 |

关键约束：`publish-linux` 的 `needs` 同时包含三个 build job，`publish-windows` 依赖 `publish-linux`、`publish-macos` 依赖 `publish-windows`（清单串行合并，避免并发写 `latest.json` 互相覆盖）。**任一平台构建失败就整个不发布**——避免出现「清单只有一个平台、另一个平台用户更新不了」的半残发行版。

需要在仓库 Settings → Secrets and variables → Actions 里配置：

| Secret | 说明 |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | 更新签名私钥全文（`~/.config/auroradb/updater.key` 的内容） |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 私钥密码；私钥未加密时留空字符串 |

`GITHUB_TOKEN` 由 Actions 自动提供，workflow 已声明 `permissions: contents: write`。

只想验证流水线不发布时，用 `workflow_dispatch` 触发并勾选 `skip_publish`。

## 手工发布命令

在仓库根目录执行，先完成构建（见 [README](../README.md#构建与发布)）：

```bash
export GITHUB_TOKEN=<令牌>

./tools/publish/linux.sh                          # 发布 Linux 产物
./tools/publish/linux.sh --tag v2.1.0 --notes "…" # 指定 tag 与发行说明
./tools/publish/linux.sh --dry-run                # 只在本机校验并打印发布计划
./tools/publish/linux.sh --repo owner/repo        # 覆盖发布仓库（默认与 origin 同名）
./tools/publish/linux.sh --extra-artifact <文件>  # 追加附件，可重复（如 lite AppImage）
./tools/publish/linux.sh --yes                    # 跳过交互确认（CI/自动化用）
```

```bash
./tools/build/macos.sh                      # 仅 macOS 上可跑：aarch64 的 .app + .dmg
./tools/build/macos.sh --intel              # Intel 版（x86_64）
./tools/publish/macos.sh                    # 上传 dmg + .app.tar.gz，合并 darwin-aarch64
./tools/publish/macos.sh --tag v2.2.3 --dry-run
```

```powershell
$env:GITHUB_TOKEN = "<令牌>"

.\tools\publish\windows.ps1
.\tools\publish\windows.ps1 -Tag v2.1.0 -Notes "…"
.\tools\publish\windows.ps1 -Repo owner/repo -BundleDir <目录>
.\tools\publish\windows.ps1 -Yes                    # 跳过交互确认（CI/自动化用）
```

## macOS 分发（未签名预览版）

macOS 版目前只发布 **Apple Silicon（arm64）**、**未签名**：

- `bundle.macOS.signingIdentity = "-"`（ad-hoc 签名），保证 Apple Silicon 上能启动，但没有 Apple Developer ID，无法通过公证
- 用户首次打开会被 Gatekeeper 拦截，需右键 → 打开，或执行：

```bash
xattr -dr com.apple.quarantine /Applications/AuroraDB.app
```

- 应用内自动更新**不受影响**：更新包用 minisign 私钥签名（`.app.tar.gz.sig`），与 Apple 代码签名无关
- 后续要正式分发（无感安装、不被拦截）需要 Apple Developer Program（$99/年）+ 公证，CI 侧需要 `APPLE_CERTIFICATE` / `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID` 四个 Secret
- Intel（x86_64）版本可用 `./tools/build/macos.sh --intel` 构建，发布脚本会自动把 key 换成 `darwin-x86_64`

## 发布行为

- 上传前打印版本、标签、分支、提交、工作区状态与 tag 推送状态，需输入 `yes` 确认；`--yes` / `-Yes` 可跳过（CI 用）
- Linux 脚本强制调用 `tools/check-release.sh`，校验产物齐全、`.sig` 存在、体积不超限、更新源指向发布仓库，并生成 `SHA256SUMS`
- 任一附件上传失败立即中止，不写入 `latest.json`，避免用户更新到不存在的地址
- 同名附件先删除再上传，重复发布同一 tag 会覆盖旧产物
- 若线上已有更高版本的发行版，重新发布旧 tag 会被拒绝，防止 `/releases/latest` 分发的清单回退

## 发布前检查

1. 按 [发版冒烟清单](smoke-checklist.md) 在 Linux 与 Windows 上各跑一遍
2. `node tools/check-version.mjs` 确认所有版本号一致
3. `./tools/publish/linux.sh --dry-run` 预览上传集
4. 需要时用 `./tools/check-release.sh` 单独核对产物

## 手工发版

版本号以 `src-tauri/tauri.conf.json` 为准，需同步 `src-ui/package.json`、`src-ui/package-lock.json`、`src-tauri/Cargo.toml`、`crates/core/Cargo.toml`、`crates/pro/Cargo.toml` 与根目录 `Cargo.lock`：

```bash
node tools/check-version.mjs
git add -A && git commit -m "chore(release): v2.1.0"
git tag v2.1.0
git push origin main --tags
```

## 维护者工具

- `./tools/commit-brief.sh`：汇总分支状态、改动文件、diff 片段与最近提交风格，用于归纳提交信息
- `./tools/ship.sh -m "type(scope): 说明"`：校验版本号、拦截敏感与大文件，提交并推送私有仓后同步公开仓

## 常见报错

- **403 `Resource not accessible by personal access token`**：令牌对该仓库没有写权限，按上文补齐 `Contents: Read and write`
- **`curl: (56) OpenSSL SSL_read: ... unexpected eof`** 或 HTTP 000：本机到 `api.github.com` / `uploads.github.com` 不通，多为代理或 VPN 问题。用 `curl -sS -o /dev/null -w '%{http_code}\n' https://api.github.com/` 验证；返回 000 或 TLS 握手失败时切换节点再试，与令牌无关
- **`/releases/latest` 返回 404**：仓库还没有正式发布的发行版（草稿不计入），先完成一次发布
- **附件上传失败且提示体积超限**：单个附件不得超过 2GB，AppImage 可改用 lite 或分卷方案
