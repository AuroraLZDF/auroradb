# AuroraDB

基于 Tauri 2 + Vue 3 + Rust 的桌面数据库客户端，支持 MySQL 与 PostgreSQL，打包目标为 Linux（deb / AppImage / rpm）与 Windows（NSIS）。

## 功能

- **连接管理**：保存多个连接、密码通过系统 keyring 加密存储、连接池与心跳检测
- **对象树浏览**：数据库、表、视图、索引、外键等对象的浏览与刷新
- **SQL 编辑器**：schema 感知自动补全、SQL 格式化、多语句执行、结果集分页与取消查询
- **数据表格**：虚拟滚动、单元格编辑、变更暂存与批量提交
- **表设计器 / 数据库设计器**：可视化查看与修改表结构
- **导入导出**：CSV / JSON / SQL / Excel
- **其他**：明暗主题、中英文切换、自动更新

专业版需激活码解锁，另含：SSH 隧道、结构对比、服务器监控、审计日志、执行计划可视化、ER 图、备份还原、跨库全局搜索。

## 下载安装

安装包统一发布在公开仓库的[发行版页面](https://github.com/AuroraLZDF/auroradb/releases/latest)（源码仓保持私有）。当前版本为 v2.0.1。

| 平台 | 文件名 | 安装方式 |
| --- | --- | --- |
| Debian / Ubuntu | `AuroraDB_<版本>_amd64.deb` | `sudo apt install ./AuroraDB_<版本>_amd64.deb` |
| 通用 Linux | `AuroraDB_<版本>_amd64.AppImage` | `chmod +x` 后直接运行，支持应用内更新 |
| Fedora / RHEL | `AuroraDB-<版本>-1.x86_64.rpm` | `sudo rpm -i AuroraDB-<版本>-1.x86_64.rpm` |
| Windows | `AuroraDB_<版本>_x64-setup.exe` | 双击安装（未签名，SmartScreen 会提示） |

每个发行版同时附带 `SHA256SUMS` 与各产物的 `.sig` 签名，下载后可核对完整性。两个平台分别发布，某平台的产物在构建机上还没发布时，对应的文件不会出现在最新发行版里。

应用内自动更新的固定入口是 `https://github.com/AuroraLZDF/auroradb/releases/latest/download/latest.json`，`latest.json` 随每次发布更新，无需随版本改动。

## 定价与购买

免费版可直接使用。专业版为**买断制**（离线激活、不联网校验），一次付费永久使用 v2.x；v3 大版本升级享 5 折优惠，不承诺终身免费升级。

| 版本 | 价格 | 适用 | 说明 |
| --- | --- | --- | --- |
| 个人版 | ¥199（早鸟 ¥149，前 100 单 / 30 天） | 1 台机器 | 全部专业版功能 |
| 团队版 | ¥499 / 3 台机器（早鸟 ¥399） | 3 台机器 | 3 个激活码（seat 1–3），共享同一授权编号 |
| 企业版 | ¥3,999 / 10 台机器 + 1 年维护（早鸟 ¥2,999） | 10 台机器 | 第 2 年起维护 ¥799/年 |
| 教育 / 开源 | ¥99 | 1 台机器 | 全部专业版功能 |
| 14 天试用 | 免费 | 1 台机器 | 应用内一键开启（无需申请、无需联网），全功能试用 14 天，每台机器限一次，到期自动回落免费版 |

**团队 / 企业购买流程**：客户在每台机器上复制机器码并一起提交（N 个机器码）→ 我们签发 N 个激活码（同一授权编号、seat 1…N）→ 每台机器输入对应的激活码。激活码与机器一一绑定，不能在多台机器上复用。

**换机**：12 个月内可免费重签 1 次，之后按 ¥29/次重签（需提供订单号与旧机器码）。

**退款 / 发票**：退款、发票与对公付款请联系【购买咨询：联系邮箱 / 微信（占位，待填）】。

## 技术栈

| 层 | 技术 |
| --- | --- |
| 桌面框架 | Tauri 2 |
| 前端 | Vue 3 + TypeScript + Vite + Pinia |
| 后端 | Rust（sqlx、tokio、sqlparser、russh） |
| 授权 | 开源核心 AGPL-3.0 + 专业版商业许可；离线 Ed25519 激活码（一码一机） |

## 目录结构

```
.
├── crates/
│   ├── core/               # 开源核心库 auroradb-core（连接、执行、导入导出、授权校验）
│   └── pro/                # 专业版库 auroradb-pro（私有，公开仓不含）
├── src-ui/                 # Vue 3 前端
│   ├── src/components/     # 免费版业务组件与基础 UI 组件
│   ├── src/pro/            # 专业版页面（私有，公开仓不含，缺失时自动退化为无入口）
│   ├── src/stores/         # Pinia 状态（连接、数据浏览、授权、更新等）
│   ├── src/i18n/           # 轻量 i18n（中/英）
│   └── src/utils/          # SQL、表格变更与专业版界面注册表
├── src-tauri/              # Tauri 应用（命令注册、窗口与更新）
│   ├── src/commands/       # Tauri 命令层（pro.rs 为专业版，公开仓以 pro_stub.rs 占位）
│   └── src/password_store.rs
├── tools/
│   ├── build/              # 构建实现（linux.sh / windows.ps1 / appimage-*.sh）
│   ├── publish/            # 发布实现（linux.sh / windows.ps1）
│   ├── check-version.mjs   # 版本号一致性校验
│   ├── check-release.sh    # 发布前产物检查
│   ├── merge-latest.py     # latest.json 合并
│   ├── commit-brief.sh     # 汇总本次改动，供归纳提交信息（私有，公开仓不含）
│   ├── ship.sh             # 提交 → 推私有仓 → 导出并推公开仓（私有，公开仓不含）
│   ├── export-public.sh    # 从私有主仓导出公开仓（私有，公开仓不含）
│   └── license-gen/        # 激活码签发工具（私有，公开仓不含）
├── docs/EULA.md            # 官方发行包与专业版的最终用户许可协议
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

# 校验免费版构建（不启用 pro feature，对应公开仓）
cargo check --offline --no-default-features
```

发版前按 [发版冒烟清单](docs/smoke-checklist.md) 在 Linux 与 Windows 上各跑一遍。

### 提交与推送

```bash
./tools/commit-brief.sh                 # 汇总改动、版本号与历史提交风格，用于归纳提交信息
./tools/ship.sh -m "fix(release): 修正 latest.json 上传顺序"
./tools/ship.sh -m "..." --dry-run      # 只打印计划，不改动仓库
./tools/ship.sh -m "..." --no-public    # 只提交并推送私有主仓
```

提交信息按 `type(scope): 说明` 写，说明为什么改而不是罗列文件。`ship.sh` 会先拦截疑似敏感文件与超过 5MB 的文件、跑版本号一致性校验，私有仓推送成功后才导出公开仓，避免两仓版本错位。

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

源码仓库保持私有，安装包发布到**公开**仓库 `AuroraLZDF/auroradb`（私有仓库的发行版附件需要登录鉴权，终端用户无法下载）。AppImage、deb、rpm、NSIS、`.sig`、`SHA256SUMS` 与 `latest.json` 全部作为发行版附件上传，GitHub 单附件上限 2GB。

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

AuroraDB 采用 open core 双许可：开源核心以 AGPL-3.0 发布，专业版功能以商业许可提供。

### 开源部分（AGPL-3.0）

`crates/core`、`src-tauri` 中的免费命令层与 `src-ui` 的免费界面依据 [GNU AGPL-3.0](LICENSE) 授权。您可以自由使用、修改和分发，但：

- 分发修改后的版本时，需按 AGPL-3.0 提供完整的对应源码；
- 通过网络向用户提供修改版本的服务时，同样需要提供源码（AGPL 第 13 条）。

### 专业版（商业许可）

免费版可直接使用连接管理与日常查询；专业版需激活码解锁，包含：

- SSH 隧道、结构对比、服务器监控、审计日志
- 执行计划可视化、ER 图、备份还原、跨库全局搜索

应用采用**离线买断**模式：激活码由发行方离线签名（Ed25519），客户端本地验签，无需联网。激活码与机器码绑定（一码一机），机器码可在「激活」对话框中复制。

专业版源码不在公开仓中，安装与使用受 [最终用户许可协议](docs/EULA.md) 约束。如需基于专业版功能二次开发或商业分发，请联系【联系邮箱】获取商业许可。

### 公开仓与贡献

- 公开仓由 `tools/export-public.sh` 从私有主仓导出，只包含上述开源部分；
- 公开仓保留专业版入口占位，点击时提示安装专业版，构建产物中不包含任何专业版代码；
- 提交贡献前请阅读 [CLA](CLA.md)，首次提交需在 Pull Request 中签署；
- 安装包中的第三方组件与许可证见 [NOTICE](NOTICE)。

激活码签发工具位于私有主仓的 `tools/license-gen`，仅供发行方构建与使用。

## 已知限制

- 编辑器基于原生 `<textarea>`，没有语法高亮；提供 schema 感知自动补全与 SQL 格式化。
- 查询结果最多保留前 10,000 行（`MAX_RESULT_ROWS`），超出部分截断并提示；单次查询超时 300 秒，导出超时 1800 秒，单条 SQL 不超过 10MB；一次执行多条语句时只处理第一个结果集。
- Linux 应用内更新仅支持 AppImage；deb / rpm 需手动下载安装新版本。
- Windows 安装包暂未签名（内测分发），SmartScreen 会提示；应用内更新只支持 NSIS，不支持 MSI。
- 离线激活无在线吊销；激活码有效期以本机时间为准，未做防回拨校验。
- 暂不支持 macOS；数据源目前仅 MySQL 与 PostgreSQL（SQLite / Redis / SQL Server 在路线图中）。
- 商业版定位为「买断 v2.x」：v3 大版本升级享 5 折，不承诺终身免费升级。
- 团队 / 企业授权按机器码一对一签发（seat），同一激活码不能在多台机器复用；换机需重签（12 个月内 1 次免费，之后 ¥29/次）。
- 14 天试用为应用内自助开启（`edition=trial`，与机器码绑定，带时间回拨检测），每台机器限一次，到期自动回落免费版；离线方案无法阻止删除全部本地试用记录后重开试用。
- AppImage 的 lite 版依赖系统 `libwebkit2gtk-4.1-0`，且不提供应用内自动更新；需要自动更新请使用 full 版。

## 日志

运行日志写入 `~/.local/share/mysql-client/logs/`（按天滚动，保留 7 天）。

## 常见问题

- **界面白屏 / 报 `Failed to create GBM buffer`**：程序默认关闭 WebKitGTK 的 DMA-BUF 渲染以兼容无 3D 加速的环境。若确认本机支持，可设置 `AURORADB_ENABLE_DMABUF=1` 恢复。
- **`Too many open files`**：提高文件描述符上限，例如 `ulimit -n 4096`。
