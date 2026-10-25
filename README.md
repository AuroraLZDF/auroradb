# AuroraDB

基于 Tauri 2 + Vue 3 + Rust 的桌面数据库客户端，支持 MySQL 与 PostgreSQL，当前打包目标为 Linux（deb / AppImage / rpm）。

## 功能

- **连接管理**：保存多个连接、密码通过系统 keyring 加密存储、连接池与心跳检测
- **SSH 隧道**：通过纯 Rust 实现的 SSH 隧道连接内网数据库
- **对象树浏览**：数据库、表、视图、索引、外键等对象的浏览与刷新
- **SQL 编辑器**：语法高亮、自动补全、多语句执行、结果集分页与取消查询
- **数据表格**：虚拟滚动、单元格编辑、变更暂存与批量提交
- **表设计器 / 数据库设计器**：可视化查看与修改表结构
- **结构对比**：两个数据库之间的表结构差异对比
- **服务器监控**：进程列表、连接数与运行指标
- **审计日志**：记录本机执行过的 SQL 操作
- **导入导出**：CSV / JSON / SQL / Excel
- **其他**：明暗主题、中英文切换、自动更新

## 技术栈

| 层 | 技术 |
| --- | --- |
| 桌面框架 | Tauri 2 |
| 前端 | Vue 3 + TypeScript + Vite + Pinia |
| 后端 | Rust（sqlx、tokio、sqlparser、russh） |
| 授权 | 离线 Ed25519 签名激活码（一码一机） |

## 目录结构

```
.
├── src-ui/                 # Vue 3 前端
│   ├── src/components/     # 业务组件与基础 UI 组件
│   ├── src/stores/         # Pinia 状态（连接、数据浏览、授权、更新等）
│   ├── src/i18n/           # 轻量 i18n（中/英）
│   └── src/utils/          # SQL 与表格变更工具函数
├── src-tauri/              # Rust 后端
│   ├── src/commands/       # Tauri 命令（连接、文件、审计、授权）
│   ├── src/core/           # 连接驱动、执行器、导出、结构对比、监控、授权校验
│   └── src/password_store.rs
├── tools/license-gen/      # 激活码签发工具（仅发行方使用）
├── build.sh                # Linux 安装包打包脚本
└── start.sh                # 启动 Tauri 开发模式（需前端 dev server 已运行）
```

## 开发

### 环境要求

- Node.js `^22.18.0` 或 `>=24.12.0`
- Rust `>= 1.77.2`
- Linux 额外依赖：`libwebkit2gtk-4.1-dev`、`libgtk-3-dev`、`libayatana-appindicator3-dev`、`librsvg2-dev`、`libssl-dev`、`build-essential`

### 启动开发模式

```bash
cd src-ui
npm install
npm run tauri:dev
```

### 常用检查

```bash
cd src-ui
npm run lint        # oxlint + eslint
npm run type-check  # vue-tsc
npm test            # vitest

cd ../src-tauri
cargo test --offline
```

## 打包

```bash
./build.sh --deb        # 只打 .deb（推荐）
./build.sh --appimage   # 只打 AppImage（需要联网下载工具）
./build.sh --rpm        # 只打 .rpm
./build.sh --all        # 全部格式
```

构建前需要更新包签名私钥（不入库），默认读取 `~/.config/auroradb/updater.key`，也可通过环境变量 `TAURI_SIGNING_PRIVATE_KEY` 提供。首次生成：

```bash
cd src-ui
npm run tauri signer generate -- -w ~/.config/auroradb/updater.key
```

产物位于 `src-tauri/target/release/bundle/`。

### 发布到 Gitee 发行版

源码仓库保持私有，安装包发布到独立的**公开**仓库 `auroradb-releases`（私有仓库的发行版附件需要登录鉴权，终端用户无法下载）。

```bash
export GITEE_TOKEN=<Gitee 私人令牌>   # 需勾选 projects 权限
./release.sh                          # 发布到 <owner>/auroradb-releases
./release.sh --tag v1.0.1 --notes "修复若干问题"
./release.sh --repo owner/other-repo   # 指定其他公开发布仓库
```

首次使用需先在 Gitee 创建公开仓库 `auroradb-releases` 并初始化 README，否则脚本会提示仓库不可访问。

脚本会校验发布仓库、读取其默认分支、创建发行版并上传安装包与 `.sig` 签名文件；单个超过 100MB 的产物会被跳过。令牌只从环境变量读取，不要写入仓库。

## 授权

应用采用**离线买断**模式：激活码由发行方离线签名，客户端本地验签，无需联网。

- 免费版可直接使用基础连接与查询功能
- Pro 版解锁：SSH 隧道、结构对比、服务器监控、审计日志、导入导出
- 激活码与机器码绑定（一码一机），机器码可在「激活」对话框中复制

签发工具位于 `tools/license-gen`，仅供发行方构建与使用：

```bash
cd tools/license-gen
cargo run -- keygen --out ~/.config/auroradb/license.key
cargo run -- sign --fingerprint XXXX-XXXX-XXXX-XXXX --key ~/.config/auroradb/license.key --days 0
```

## 日志

运行日志写入 `~/.local/share/mysql-client/logs/`（按天滚动，保留 7 天）。

## 常见问题

- **界面白屏 / 报 `Failed to create GBM buffer`**：程序默认关闭 WebKitGTK 的 DMA-BUF 渲染以兼容无 3D 加速的环境。若确认本机支持，可设置 `AURORADB_ENABLE_DMABUF=1` 恢复。
- **`Too many open files`**：提高文件描述符上限，例如 `ulimit -n 4096`。
