# AuroraDB

基于 Tauri 2 + Vue 3 + Rust 的桌面数据库客户端，支持 MySQL 与 PostgreSQL，打包目标为 Linux（deb / AppImage / rpm）与 Windows（NSIS）。

## 功能

- **连接管理**：保存多个连接、密码通过系统 keyring 加密存储、连接池与心跳检测
- **对象树浏览**：数据库、表、视图、索引、外键等对象的浏览与刷新
- **SQL 编辑器**：schema 感知自动补全、SQL 格式化、多语句执行、结果集分页与取消查询
- **数据表格**：虚拟滚动、单元格编辑、变更暂存与批量提交
- **表设计器 / 数据库设计器**：可视化查看与修改表结构
- **导入导出**：CSV / JSON / SQL / Excel
- **增强功能（全部免费）**：SSH 隧道、结构对比、服务器监控、审计日志、执行计划可视化、ER 图、备份还原、跨库全局搜索
- **其他**：明暗主题、中英文切换、自动更新

所有功能均永久免费开放：无功能限制、无广告、无需激活码、无需联网校验。源码以 [AGPL-3.0](LICENSE) 全量开源（含原增强功能模块），欢迎自建、二次开发与分发。

## 下载安装

安装包统一发布在公开仓库的[发行版页面](https://github.com/AuroraLZDF/auroradb/releases/latest)，源码同样开源（AGPL-3.0）。当前版本为 v2.0.2。

| 平台 | 文件名 | 安装方式 |
| --- | --- | --- |
| Debian / Ubuntu | `AuroraDB_<版本>_amd64.deb` | `sudo apt install ./AuroraDB_<版本>_amd64.deb` |
| 通用 Linux（full，推荐自动更新） | `AuroraDB_<版本>_amd64.AppImage` | `chmod +x` 后直接运行，自包含，支持应用内更新 |
| 通用 Linux（lite，体积小） | `AuroraDB_<版本>_amd64-lite.AppImage` | `chmod +x` 后直接运行，依赖系统 `libwebkit2gtk-4.1-0`，需手动更新 |
| Fedora / RHEL | `AuroraDB-<版本>-1.x86_64.rpm` | `sudo rpm -i AuroraDB-<版本>-1.x86_64.rpm` |
| Windows | `AuroraDB_<版本>_x64-setup.exe` | 双击安装（未签名，SmartScreen 会提示） |

每个发行版同时附带 `SHA256SUMS` 与各产物的 `.sig` 签名，下载后可核对完整性。两个平台分别发布，某平台的产物在构建机上还没发布时，对应的文件不会出现在最新发行版里。

应用内自动更新的固定入口是 `https://github.com/AuroraLZDF/auroradb/releases/latest/download/latest.json`，`latest.json` 随每次发布更新，无需随版本改动。

## 免费与开源

- **全部功能免费**：连接、查询、表编辑、导入导出，以及 SSH 隧道、结构对比、监控、审计、执行计划、ER 图、备份还原、跨库搜索，全部直接可用。
- **无功能限制、无广告、无账号**：不需要注册、登录或激活码，不联网校验；应用不含任何广告或使用限制。
- **源码全量开源**：包括原增强功能模块在内的完整源码以 [GNU AGPL-3.0](LICENSE) 发布，可自由使用、修改和分发（分发修改版需按 AGPL-3.0 提供对应源码）。
- **可持续方式**：项目依靠打赏与赞助维持开发，不销售授权、不做功能订阅。

## 支持开发（打赏）

如果 AuroraDB 对你有帮助，欢迎自愿打赏支持后续开发（金额随意，不影响任何功能）：

- **GitHub Sponsors**：<https://github.com/sponsors/auroraLZDF>（占位链接，待填写）
- **爱发电**：<https://afdian.com/a/auroradb>（占位链接，待填写）
- **微信 / 支付宝收款码**：将 `wechat.png`、`alipay.png` 两张图片放入本仓库 `assets/donate/` 目录后即可在此展示（当前尚未提供；目录说明见 [assets/donate/README.md](assets/donate/README.md)）。

打赏完全自愿，不提供付费特权，也不影响功能可用性；所有赞助用于服务器、签名与开发投入。代码贡献、Issue 反馈、给仓库点 Star 同样是支持。

## 技术栈

| 层 | 技术 |
| --- | --- |
| 桌面框架 | Tauri 2 |
| 前端 | Vue 3 + TypeScript + Vite + Pinia |
| 后端 | Rust（sqlx、tokio、sqlparser、russh） |
| 授权 | AGPL-3.0 全量开源（含增强功能） |

## 目录结构

```
.
├── crates/
│   ├── core/               # 核心库 auroradb-core（连接、执行、导入导出等）
│   └── pro/                # 增强功能库 auroradb-pro（结构对比、监控、备份还原、ER 图、全局搜索、执行计划、SSH 隧道）
├── src-ui/                 # Vue 3 前端
│   ├── src/components/     # 业务组件与基础 UI 组件
│   ├── src/pro/            # 增强功能页面（Schema 对比、监控、ER 图、全局搜索等）
│   ├── src/stores/         # Pinia 状态（连接、数据浏览、更新等）
│   ├── src/i18n/           # 轻量 i18n（中/英）
│   └── src/utils/          # SQL、表格变更与界面扩展注册表
├── src-tauri/              # Tauri 应用（命令注册、窗口与更新）
│   ├── src/commands/       # Tauri 命令层（pro.rs 为增强功能命令）
│   └── src/password_store.rs
├── assets/donate/          # 打赏收款码占位（wechat.png / alipay.png）
├── tools/
│   ├── build/              # 构建实现（linux.sh / windows.ps1 / appimage-*.sh）
│   ├── publish/            # 发布实现（linux.sh / windows.ps1）
│   ├── check-version.mjs   # 版本号一致性校验
│   ├── check-release.sh    # 发布前产物检查
│   ├── merge-latest.py     # latest.json 合并
│   ├── commit-brief.sh     # 汇总本次改动，供归纳提交信息（维护者工具）
│   ├── ship.sh             # 提交 → 推主仓 → 同步公开仓（维护者工具）
│   └── export-public.sh    # 从主仓导出公开仓（维护者工具）
├── docs/EULA.md            # 许可与免责声明（以根目录 LICENSE 为准）
├── LICENSE                 # AGPL-3.0
├── CLA.md                  # 贡献者许可协议
├── NOTICE                  # 第三方组件许可声明
└── start.sh                # 一键启动 Tauri 开发模式（首次运行会自动装前端依赖）
```

## 开发

### 环境要求

- Node.js `^22.18.0` 或 `>=24.12.0`
- Rust `>= 1.77.2`
- Linux 额外依赖：`libwebkit2gtk-4.1-dev`、`libgtk-3-dev`、`libayatana-appindicator3-dev`、`librsvg2-dev`、`libssl-dev`、`build-essential`

### 启动开发模式

首次克隆后，**必须先装前端依赖**（否则 `vite: not found`）：

```bash
cd src-ui
npm ci        # 或 npm install；推荐 ci，严格按 package-lock.json 安装
```

之后两种启动方式任选其一：

```bash
# 方式 A：一键脚本（推荐，自动拉起前端 dev server + Tauri）
./start.sh

# 方式 B：分别启动（调试时方便看分离的日志）
cd src-ui && npm run dev &        # 前端 dev server
cd ../src-tauri && cargo tauri dev # Tauri 壳
```

### 常用检查

```bash
cd src-ui
npm run lint        # oxlint + eslint
npm run type-check  # vue-tsc
npm test            # vitest

cd ../src-tauri
cargo test --offline
cargo check --offline
```

发版前按 [发版冒烟清单](docs/smoke-checklist.md) 在 Linux 与 Windows 上各跑一遍。

### 提交与推送

```bash
./tools/commit-brief.sh                 # 汇总改动、版本号与历史提交风格，用于归纳提交信息
./tools/ship.sh -m "fix(release): 修正 latest.json 上传顺序"
./tools/ship.sh -m "..." --dry-run      # 只打印计划，不改动仓库
./tools/ship.sh -m "..." --no-public    # 只提交并推送主仓
```

提交信息按 `type(scope): 说明` 写，说明为什么改而不是罗列文件。`ship.sh` 会先拦截疑似敏感文件与超过 5MB 的文件、跑版本号一致性校验，主仓推送成功后才同步公开仓，避免两仓版本错位。

## 打包

构建与发布脚本都放在 `tools/` 下，按平台区分：Linux 用 `.sh`，Windows 用 `.ps1`。

### Linux（在 Linux 机器上构建）

```bash
./tools/build/linux.sh --deb        # 只打 .deb（推荐）
./tools/build/linux.sh --appimage   # 只打 AppImage（自动重压缩并重新签名）
./tools/build/linux.sh --rpm        # 只打 .rpm
./tools/build/linux.sh --all        # 全部格式
```

### Windows（在 Windows 机器上构建）

```powershell
.\tools\build\windows.ps1                  # NSIS 安装包
.\tools\build\windows.ps1 -Bundles nsis    # 指定打包格式
```

Tauri 不支持交叉编译：Linux 包必须在 Linux 构建，Windows 包必须在 Windows 构建，脚本本身也只接受本平台执行。

AppImage 备用方案（`cargo tauri build` 下载 bundler 工具失败时）：

```bash
./tools/build/appimage-offline.sh          # 复用已有前端产物与 release 二进制
./tools/build/appimage-offline.sh --build  # 强制重新构建
```

### 签名密钥

构建前需要更新包签名私钥（不入库），默认读取 `~/.config/auroradb/updater.key`，也可通过环境变量 `TAURI_SIGNING_PRIVATE_KEY` 提供。首次生成：

```bash
cd src-ui
npm run tauri signer generate -- -w ~/.config/auroradb/updater.key
```

产物位于 `src-tauri/target/release/bundle/`。

### 发布到 GitHub 发行版

源码已开源（AGPL-3.0）。安装包发布到公开仓库 `AuroraLZDF/auroradb` 的发行版，方便终端用户直接下载：AppImage、deb、rpm、NSIS、`.sig`、`SHA256SUMS` 与 `latest.json` 全部作为发行版附件上传，GitHub 单附件上限 2GB。

```bash
export GITHUB_TOKEN=<GitHub 令牌>   # 经典令牌勾选 repo；细粒度令牌授予 Contents: Read and write

./tools/publish/linux.sh              # 发布 Linux 产物
./tools/publish/linux.sh --tag v1.0.1 --notes "修复若干问题"
./tools/publish/linux.sh --repo owner/other-repo  # 指定其他公开发布仓库
./tools/publish/linux.sh --dry-run    # 只在本机校验并预览发布计划
```

```powershell
$env:GITHUB_TOKEN = "<GitHub 令牌>"
.\tools\publish\windows.ps1
.\tools\publish\windows.ps1 -Tag v1.0.1 -Notes "修复若干问题"
```

两个平台的发布脚本都会显示当前分支、提交、工作区状态、tag 是否已创建并推送，需输入 `yes` 确认后才会上传；缺少 tag 或代码未推送时会给出对应命令提示，可先取消发布处理完再重跑。Linux 脚本在发布前强制调用 `tools/check-release.sh` 校验产物齐全、`.sig` 签名存在、单附件不超过 2GB、更新源指向发布仓库，并生成 `SHA256SUMS`；Windows 脚本不做产物预检，也不生成 `SHA256SUMS`，发布前需手工核对产物与签名。两个平台的 `latest.json` 条目按平台 key 合并，先后发布互不覆盖。令牌只从环境变量读取，不要写入仓库。

应用内自动更新的固定入口见 [下载安装](#下载安装) 一节。任一附件上传失败脚本都会立即中止，不会写入 `latest.json`，避免用户更新到不存在的地址。Tauri 只为 AppImage 生成更新签名，构建脚本会额外为 deb 补签 `.sig`。

### 手工发版步骤

版本号以 `src-tauri/tauri.conf.json` 为准，需同步 `src-ui/package.json`、`src-ui/package-lock.json`、`src-tauri/Cargo.toml`、`crates/core/Cargo.toml`、`crates/pro/Cargo.toml` 与根目录 `Cargo.lock`（公开仓没有 `crates/pro`，`check-version.mjs` 会自动跳过），然后：

```bash
node tools/check-version.mjs
git add -A && git commit -m "chore(release): v2.1.0"
git tag v2.1.0
git push origin main --tags
```

## 授权与许可

AuroraDB 以 [GNU AGPL-3.0](LICENSE) 开源，全部功能（含原增强功能模块）使用同一许可证，免费使用，无激活码、无订阅、无商业授权限制。

### 开源许可（AGPL-3.0）

`crates/core`、`crates/pro`、`src-tauri` 与 `src-ui` 均依据 GNU AGPL-3.0 授权。您可以自由使用、修改和分发，但：

- 分发修改后的版本时，需按 AGPL-3.0 提供完整的对应源码；
- 通过网络向用户提供修改版本的服务时，同样需要提供源码（AGPL 第 13 条）。

不再提供单独的「商业许可」或付费版本；商业场景同样可直接使用，只需遵守 AGPL-3.0。更完整的许可与免责声明见 [docs/EULA.md](docs/EULA.md)。

### 公开仓与贡献

- 源码已开源，公开仓包含全部功能源码与安装包；
- 提交贡献前请阅读 [CLA](CLA.md)，首次提交需在 Pull Request 中签署；
- 安装包中的第三方组件与许可证见 [NOTICE](NOTICE)。

## 已知限制

- 编辑器基于原生 `<textarea>`，没有语法高亮；提供 schema 感知自动补全与 SQL 格式化。
- 查询结果最多保留前 10,000 行（`MAX_RESULT_ROWS`），超出部分截断并提示；单次查询超时 300 秒，导出超时 1800 秒，单条 SQL 不超过 10MB；一次执行多条语句时只处理第一个结果集。
- Linux 应用内更新仅支持 AppImage；deb / rpm 需手动下载安装新版本。
- Windows 安装包暂未签名（内测分发），SmartScreen 会提示；应用内更新只支持 NSIS，不支持 MSI。
- 暂不支持 macOS；数据源目前仅 MySQL 与 PostgreSQL（SQLite / Redis / SQL Server 在路线图中）。
- AppImage 的 lite 版依赖系统 `libwebkit2gtk-4.1-0`，且不提供应用内自动更新；需要自动更新请使用 full 版。
- 全部功能免费、无激活与联网校验，因此不存在「离线授权/试用期/换机」等限制。

## 日志

运行日志写入 `~/.local/share/mysql-client/logs/`（按天滚动，保留 7 天）。

## 常见问题

- **界面白屏 / 报 `Failed to create GBM buffer`**：程序默认关闭 WebKitGTK 的 DMA-BUF 渲染以兼容无 3D 加速的环境。若确认本机支持，可设置 `AURORADB_ENABLE_DMABUF=1` 恢复。
- **`Too many open files`**：提高文件描述符上限，例如 `ulimit -n 4096`。
