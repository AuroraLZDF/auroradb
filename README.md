# AuroraDB

[![License: AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![Latest release](https://img.shields.io/github/v/release/AuroraLZDF/auroradb)](https://github.com/AuroraLZDF/auroradb/releases/latest)
[![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20Windows-lightgrey.svg)](#下载安装)

面向 MySQL 与 PostgreSQL 的桌面数据库客户端，基于 Tauri 2 + Vue 3 + Rust 构建，提供连接管理、SQL 编辑、表设计、数据编辑与导入导出，并支持应用内自动更新。

全部功能永久免费，无激活码、无广告、无联网校验；源码以 [AGPL-3.0](LICENSE) 全量开源。

## 下载安装

安装包发布在[发行版页面](https://github.com/AuroraLZDF/auroradb/releases/latest)，当前版本 v2.1.0。

| 平台 | 文件名 | 安装方式 |
| --- | --- | --- |
| Debian / Ubuntu | `AuroraDB_<版本>_amd64.deb` | `sudo apt install ./AuroraDB_<版本>_amd64.deb` |
| 通用 Linux（full，推荐） | `AuroraDB_<版本>_amd64.AppImage` | `chmod +x` 后直接运行，自包含，支持应用内更新 |
| 通用 Linux（lite，体积小） | `AuroraDB_<版本>_amd64-lite.AppImage` | `chmod +x` 后直接运行，依赖系统 `libwebkit2gtk-4.1-0`，需手动更新 |
| Fedora / RHEL | `AuroraDB-<版本>-1.x86_64.rpm` | `sudo rpm -i AuroraDB-<版本>-1.x86_64.rpm` |
| Windows | `AuroraDB_<版本>_x64-setup.exe` | 双击安装（未签名，SmartScreen 会提示） |

每个发行版同时附带 `SHA256SUMS` 与各产物的 `.sig` 签名，下载后可核对完整性。两个平台分别发布，某平台的产物尚未构建时不会出现在最新发行版里。

> **Deepin / UOS 用户**：20.x 的系统源只有 `libwebkit2gtk-4.0`，deb 无法满足依赖；请下载 **full AppImage**，它自带 WebKitGTK 运行库，无需安装依赖。

## 功能特性

**连接与浏览**

- 保存多个连接，密码通过系统 keyring 加密存储
- 连接池与心跳检测，断线自动重连
- 对象树浏览数据库、表、视图、索引、外键

**SQL 与数据编辑**

- schema 感知的自动补全、SQL 格式化、多语句执行
- 结果集分页、虚拟滚动、查询取消
- 单元格编辑，变更暂存后批量提交
- 表设计器与数据库设计器，可视化查看和修改表结构

**导入导出**

- CSV / JSON / SQL / Excel 双向导入导出

**增强功能**

- SSH 隧道、结构对比、服务器监控、审计日志
- 执行计划可视化、ER 图、备份还原、跨库全局搜索

**体验**

- 明暗主题、中英文切换、应用内自动更新

## 快速开始

### 环境要求

- Node.js `^22.18.0` 或 `>=24.12.0`
- Rust `>= 1.77.2`
- Linux 额外依赖：`libwebkit2gtk-4.1-dev`、`libgtk-3-dev`、`libayatana-appindicator3-dev`、`librsvg2-dev`、`libssl-dev`、`build-essential`

### 启动开发模式

首次克隆后**必须先安装前端依赖**（否则会报 `vite: not found`）：

```bash
cd src-ui
npm ci        # 或 npm install；推荐 ci，严格按 package-lock.json 安装
```

之后两种方式任选其一：

```bash
# 方式 A：一键脚本（推荐，自动拉起前端 dev server + Tauri）
./start.sh

# 方式 B：分别启动（调试时方便看分离的日志）
cd src-ui && npm run dev &         # 前端 dev server
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

发版前请按 [发版冒烟清单](docs/smoke-checklist.md) 在 Linux 与 Windows 上各跑一遍。

## 构建与发布

Tauri 不支持交叉编译，Linux 包需在 Linux 构建，Windows 包需在 Windows 构建：

```bash
./tools/build/linux.sh --deb        # Linux：--appimage / --rpm / --all
./tools/build/linux.sh --appimage   # 重压缩并重新签名
```

```powershell
.\tools\build\windows.ps1           # Windows：NSIS 安装包
```

产物位于 `src-tauri/target/release/bundle/`。构建前需准备更新包签名私钥（不入库），默认读取 `~/.config/auroradb/updater.key`，也可用环境变量 `TAURI_SIGNING_PRIVATE_KEY` 提供；首次生成时在 `src-ui/` 下执行 `npm run tauri signer generate -- -w ~/.config/auroradb/updater.key`。`cargo tauri build` 下载 bundler 失败时，可用 `./tools/build/appimage-offline.sh` 复用已有产物。

产物发布到 GitHub Releases，应用内更新的固定入口是 `https://github.com/AuroraLZDF/auroradb/releases/latest/download/latest.json`，随每次发布更新。升版本并打 tag 用 `./tools/release-tag.sh 2.1.0`（同步 7 个版本文件、校验、提交并创建 `v2.1.0`，支持 `--dry-run` 预览与 `--push` 推送）。令牌配置、发布命令、发版检查与常见报错见 [构建、发布与发版手册](docs/RELEASING.md)。

## 项目结构

```
.
├── crates/
│   ├── core/               # 核心库 auroradb-core（连接、执行、导入导出等）
│   └── pro/                # 增强功能库 auroradb-pro（结构对比、监控、备份还原、ER 图等）
├── src-ui/                 # Vue 3 前端
│   ├── src/components/     # 业务组件与基础 UI 组件
│   ├── src/pro/            # 增强功能页面
│   ├── src/stores/         # Pinia 状态（连接、数据浏览、更新等）
│   ├── src/i18n/           # 轻量 i18n（中/英）
│   └── src/utils/          # SQL、表格变更与界面扩展注册表
├── src-tauri/              # Tauri 应用（命令注册、窗口与更新）
│   ├── src/commands/       # Tauri 命令层（pro.rs 为增强功能命令）
│   └── src/password_store.rs
├── assets/donate/          # 打赏收款码
├── tools/
│   ├── build/              # 构建实现（linux.sh / windows.ps1 / appimage-*.sh）
│   ├── publish/            # 发布实现（linux.sh / windows.ps1）
│   ├── check-version.mjs   # 版本号一致性校验
│   ├── check-release.sh    # 发布前产物检查
│   ├── merge-latest.py     # latest.json 合并
│   ├── release-tag.sh      # 升版本 + 校验 + 提交 + 打 tag
│   ├── commit-brief.sh     # 汇总改动供归纳提交信息（维护者工具）
│   └── ship.sh             # 提交 → 推送 origin（维护者工具）
├── docs/
│   ├── EULA.md             # 许可与免责声明（以根目录 LICENSE 为准）
│   ├── RELEASING.md        # 构建、发布与发版手册（维护者）
│   └── smoke-checklist.md  # 发版冒烟清单
├── LICENSE                 # AGPL-3.0
├── CLA.md                  # 贡献者许可协议
├── NOTICE                  # 第三方组件许可声明
└── start.sh                # 一键启动 Tauri 开发模式
```

## 常见问题

- **界面白屏 / 报 `Failed to create GBM buffer`**：程序默认关闭 WebKitGTK 的 DMA-BUF 渲染以兼容无 3D 加速的环境。若确认本机支持，可设置 `AURORADB_ENABLE_DMABUF=1` 恢复。
- **`Too many open files`**：提高文件描述符上限，例如 `ulimit -n 4096`。
- **Deepin / UOS 20.x 安装 deb 报缺少 `libwebkit2gtk-4.1-0`**：该系统源自带 4.0，ABI 不兼容且无法升级；请改用 full AppImage。
- **应用内更新无反应**：更新只对 AppImage（Linux）与 NSIS（Windows）生效，deb / rpm 需手动下载新版本；lite AppImage 不提供自动更新。

运行日志位于 `~/.local/share/mysql-client/logs/`（按天滚动，保留 7 天）。

## 已知限制

- 编辑器基于原生 `<textarea>`，没有语法高亮；提供 schema 感知自动补全与 SQL 格式化。
- 查询结果最多保留前 10,000 行（`MAX_RESULT_ROWS`），超出部分截断并提示；单次查询超时 300 秒，导出超时 1800 秒，单条 SQL 不超过 10MB；一次执行多条语句时只处理第一个结果集。
- Linux 应用内更新仅支持 AppImage；deb / rpm 需手动下载安装新版本。
- Windows 安装包暂未签名，SmartScreen 会提示；应用内更新只支持 NSIS，不支持 MSI。
- 暂不支持 macOS；数据源目前仅 MySQL 与 PostgreSQL（SQLite / Redis / SQL Server 在路线图中）。
- lite AppImage 依赖系统 `libwebkit2gtk-4.1-0`，且不提供应用内自动更新。

## 支持开发

如果 AuroraDB 对你有帮助，欢迎自愿打赏支持后续开发（金额随意，不影响任何功能）：

- **GitHub Sponsors**：<https://github.com/sponsors/auroraLZDF>（占位链接，待填写）
- **爱发电**：<https://afdian.com/a/auroradb>（占位链接，待填写）

| 微信支付 | 支付宝 |
| --- | --- |
| <img src="assets/donate/wechat.jpg" alt="微信收款码" width="220"> | <img src="assets/donate/alipay.jpg" alt="支付宝收款码" width="220"> |

收款码原图见 [assets/donate/](assets/donate/)。所有赞助用于域名、签名与开发投入；提交代码、反馈 Issue、给仓库点 Star 同样是支持。

## 许可与贡献

AuroraDB 以 [GNU AGPL-3.0](LICENSE) 开源（SPDX 标识 `AGPL-3.0-only`，仅适用第 3 版，不自动适用后续版本），`crates/core`、`crates/pro`、`src-tauri` 与 `src-ui` 适用同一许可证，各 `Cargo.toml` 与 `package.json` 的 `license` 字段与此一致；无单独的商业许可或付费版本。

- 分发修改后的版本时，需按 AGPL-3.0 提供完整的对应源码
- 通过网络向用户提供修改版本的服务时，同样需要提供源码（AGPL 第 13 条）
- 商业场景同样可直接使用，只需遵守 AGPL-3.0
- 提交贡献前请阅读 [CLA](CLA.md)，首次提交需在 Pull Request 中签署
- 第三方组件与许可证见 [NOTICE](NOTICE)，完整免责声明见 [docs/EULA.md](docs/EULA.md)
