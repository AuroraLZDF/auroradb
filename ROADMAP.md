# AuroraDB 功能路线图

生成日期：2026-10-26
项目状态：v1.0.8 已发布，MySQL/PostgreSQL 支持、SSH 隧道、表/库设计器、结构对比、监控、审计、导入导出、离线激活体系已就绪；v1.x 功能路线图已全部落地，v2.0 规划见文末。

---

## P0 · 基础体验补齐（2 周内，免费版也应享受）

| # | 功能 | 验收标准 | 技术要点 |
|---|------|----------|----------|
| P0-1 | **多查询标签页** | 可同时打开 N 个查询页，每个页签独立 SQL + 独立结果集；支持重命名、关闭、关闭其他、Ctrl+Tab 切换 | Pinia store 管理 `Tab[]`，每个 Tab 含 sql / results / active / dirty；SqlEditor.vue + DataTable.vue 由 store 驱动 |
| P0-2 | **SQL 格式化** | 快捷键 Shift+Alt+F 美化当前编辑器内容；支持 MySQL 与 PostgreSQL 方言 | 后端调 sqlparser / sql-formatter，纯文本处理；不解析 AST 也能先做基础缩进 |
| P0-3 | **查询草稿自动保存 + 崩溃恢复** | 每次编辑 2 秒 debounce 写入 IndexedDB；下次启动恢复上次全部 Tab 的 SQL | `localStorage` 或 IndexedDB（大 SQL 不卡）；版本号兼容升级 |
| P0-4 | **命令面板 + 快捷键体系** | Ctrl+K 呼出命令菜单，支持"执行查询 / 格式化 / 新建 Tab / 关闭 Tab / 切换连接 / 跳转到表"等；快捷键全量可配置 | 集中式 command registry，Editor/Footer 监听快捷键 |
| P0-5 | **大结果集体验** | 虚拟滚动已实现，补：复制选中、复制整列、列宽自动适应、冻结前 N 列 | DataTable.vue 改造，避免 DOM 爆炸 |

## P1 · Pro 专业能力（付费版核心卖点）

| # | 功能 | 验收标准 | 技术要点 |
|---|------|----------|----------|
| P1-1 | **EXPLAIN 可视化** | 选中 SQL 一键 Explain，树形计划 + 耗时/扫描行数高亮；MySQL 与 PostgreSQL 分别适配 | MySQL EXPLAIN JSON、PostgreSQL EXPLAIN (ANALYZE, FORMAT JSON)；前端组件渲染 |
| P1-2 | **ER 图自动生成** | 选中数据库一键生成关系图，表字段 + 外键连线 + 导出 PNG/SVG | 后端返回表列表与 FK 关系，前端用 dagre/elk 布局 |
| P1-3 | **备份 / 还原 / 定时任务** | 基于 mysqldump / pg_dump；定时策略（每天/每周/自定义 Cron）；备份历史与一键还原 | Rust 侧 spawn 子进程；定时用 chrono + tokio interval；Pro 门禁 |
| P1-4 | **跨库全局搜索** | 按表名 / 字段名 / 数据内容搜索，支持 LIKE 与正则；返回匹配表 + 行数 | 后端拼接 `information_schema` 查询 + `SELECT * FROM t WHERE col LIKE`；限制结果数 |
| P1-5 | **会话与锁管理** | 活动会话列表 + 慢查询 + KILL；MySQL `show processlist`、PostgreSQL `pg_stat_activity` | 后端 connection pool 复用或独立短连接；KILL 需鉴权 |
| P1-6 | **用户与权限管理** | 图形化创建用户 / GRANT / REVOKE，替代手写 SQL | 后端读 `mysql.user` / `pg_roles` + DDL 生成 |
| P1-7 | **存储过程 / 函数 / 触发器 / 事件编辑器** | 与表设计器并列的对象类型；支持 CREATE / ALTER / DROP | 后端返回 `information_schema.routines` 等；前端 Monaco 复用 |
| P1-8 | **数据同步（表→表 / 库→库）** | 在现有 SchemaDiff 基础上扩：结构同步 + 数据同步；支持 upsert、按主键匹配 | 后端 src-tauri/src/commands/sync.rs；Pro 门禁 |

## P2 · 扩大用户面

| # | 功能 | 说明 |
|---|------|------|
| P2-1 | **SQLite 驱动** | 接入成本最低，覆盖本地开发场景；用 rusqlite |
| P2-2 | **Redis 驱动** | 用 redis-rs；支持 key/string/hash/list/set/zset 可视化 |
| P2-3 | **SQL Server 驱动** | tiberius 或 ODBC；优先级较低 |
| P2-4 | **单元格查看器** | JSON 格式化、图片预览、大文本独立窗口；免费版即可 |
| P2-5 | **导出增强** | Excel (xlsx)、带格式 CSV、按查询结果批量导出 |

## 必须先解决的基础闭环

| 项 | 说明 | 优先级 |
|----|------|--------|
| **AppImage 101MB 超限** | ✅ 已解决：`repack-appimage.sh` 复用原 runtime，strip 符号后用 xz 重压包（101MB → 83MB），构建脚本自动调用并重新签名 | 已解决 |
| **Linux 应用内更新** | ✅ 已解决：updater endpoint 指向 `https://gitee.com/auroraLZDF/auroradb-releases/raw/main/latest.json`，`release.sh` 上传 AppImage 后自动生成并提交 latest.json；deb 不支持更新，仅 AppImage 生效 | 已解决 |
| **免费版 / Pro 边界细化** | 免费版：多连接 + 查询 + 导入导出 + 表设计器；Pro：结构同步 + 定时备份 + 审计 + SSH 多跳 + 跨库搜索 | 产品 |
| **试用期** | 14 天全功能；实现：激活码带 trial 标记，或首次启动开始倒计时 | 变现 |

## 实施顺序建议

```
blocker: AppImage 压缩 + updater 链路打通（已完成）
    ↓
P0-1 多标签页 → P0-3 草稿恢复 → P0-5 大结果集
    ↓
P0-2 格式化 → P0-4 命令面板
    ↓
P1-1 EXPLAIN 可视化 → P1-2 ER 图（演示用）
    ↓
P1-3 备份还原 → P1-8 数据同步（Pro 变现）
```

---

# v2.0 · 平台扩张（目标：Linux + Windows 双平台可售，macOS 延后到 v2.1）

> 定位：v1.x 已把功能面铺满（MySQL/PG、多 Tab、Explain、ER 图、Schema 同步、监控审计、备份还原、全局搜索、i18n、离线激活、自动更新）。
> v2.0 不再堆功能，只解决「装不上、不敢用、用不动」三类问题：先让 Windows 用户能装能用，再补大库与日常操作体验，最后把交付流程工程化。

## V2-0 前置决策结论（2026-10-30 已拍板）

| # | 决策项 | 结论 | 影响与遗留 |
|---|--------|------|------------|
| D1 | **Windows 代码签名** | **暂不签名，v2.0 按内测版分发** | 发布页与安装向导需注明「未签名内测版」、提供 SHA256 校验值、给出「更多信息 → 仍要运行」指引；企业终端受组策略拦截属已知限制。v2.1 再评估 Azure Trusted Signing（要求组织成立约 3 年）或 OV/EV 云端签名 |
| D2 | **macOS 支持** | **v2.0 不做，移入 v2.1+** | 无 Apple Developer 账号、无 macOS 构建机，即使出包也无法完成签名、公证与 Gatekeeper 验证，属于「发了也装不上」 |
| D3 | **构建环境** | **Windows + Linux 本地构建**（现有两台机器）；CI 仅保留现有 Linux 质量门禁 | Tauri 不能交叉编译，Windows 包必须在 Windows 机器产出。风险：发布依赖本地环境，必须把 Windows 构建步骤脚本化，并保证两台机器步骤一致 |
| D4 | **分发渠道** | **Gitee 主源 + GitHub Releases 镜像**（公开分发仓，源码仍留 Gitee 私有库） | 需新建 GitHub 公开分发仓与 token；`release.sh` 改为双通道上传；更新清单 endpoint 配置 Gitee 主 + GitHub 备；README 同时给出两个下载入口 |
| D5 | **更新清单结构** | **按平台 key 合并回写，保留上一版本** | v2.0 清单平台：`linux-x86_64` + `windows-x86_64`；`darwin-*` 待 v2.1 启用。合并逻辑见 V2-3.2 |

> 结论带来的范围变化：**v2.0 = Linux（存量）+ Windows（内测）+ 双通道发布 + 三项体验补齐**，工作量比原方案减少约 30%。

## V2-1 Windows 支持（P0）

| # | 任务 | 验收标准 | 技术要点 |
|---|------|----------|----------|
| V2-1.1 | 打包目标 | 产出 NSIS 安装包，干净 Win10/11 可安装、卸载、覆盖升级 | [tauri.conf.json](file:///home/code/sqlcustomer/mysql-client/src-tauri/tauri.conf.json#L50-L51) 按平台配置 `nsis`；**应用内更新只支持 NSIS，MSI 不支持**；校验 `icon.ico` 各尺寸 |
| V2-1.2 | 平台差异适配 | 日志、下载目录、临时凭据文件、外部工具调用均正常 | 临时凭据文件 Linux 用 0600，Windows 需改用受限 ACL 或改为 stdin 传参；`mysqldump`/`mysql` 缺失时给出 Windows 版安装指引（MySQL Shell 或官方客户端） |
| V2-1.3 | 后端平台分支 | 无 Linux 专属逻辑泄漏到 Windows | `WEBKIT_DISABLE_DMABUF_RENDERER`、AppImage 重打包、deb 手动升级分支全部 `#[cfg(target_os)]` 隔离 |
| V2-1.4 | 更新链路 | 老版本能收到并完成 NSIS 静默升级 | updater 清单增加 `windows-x86_64`；NSIS 参数与安装目录保持一致 |
| V2-1.5 | 冒烟 | 连接（含 SSH 隧道 + TLS）、查询、编辑保存、备份还原、导入导出全通过 | 在 Windows 机器做一次完整手工冒烟，结果记录在案 |
| V2-1.6 | 内测版分发 | 用户能正确安装并验证包完整性 | 安装包与发布页标注「未签名内测版」；发布 `SHA256SUMS`；README/发布说明给出 SmartScreen「更多信息 → 仍要运行」指引；补一份企业终端被拦截的说明 |

## V2-2 macOS 支持（v2.0 不做，移入 v2.1+）

已确认不满足开工条件：无 Apple Developer 账号（$99/年）、无 macOS 构建机，无法完成 Developer ID 签名、notarytool 公证与 Gatekeeper 实机验证。

保留以下结论供 v2.1 直接取用：

- 打包 `dmg` + `app`，优先 universal（aarch64 + x86_64），成本过高则分架构并在清单区分。
- 流程：Developer ID 签名 → notarytool 公证 → `stapler` 装订；签名身份与团队 ID 走 CI secret。
- 清单平台 key：`darwin-aarch64` / `darwin-x86_64`。
- keyring 在 macOS 走 Keychain，需验证首次授权弹窗；`mysqldump`/`mysql` 默认搜索 Homebrew 与官方安装路径。

**重启条件**：账号到位 + 有可用的 macOS 机器（或 CI macOS runner 预算）。

## V2-3 发布与更新链路双平台化（P0）

| # | 任务 | 验收标准 | 技术要点 |
|---|------|----------|----------|
| V2-3.1 | 构建与发布解耦 | Linux、Windows 各自构建，产物汇总到一处发布 | `build.sh`/`release.sh` 拆成 build（平台本地）与 publish（聚合上传）两步；Windows 构建步骤脚本化为 `build.ps1`，与 Linux 步骤逐项对齐 |
| V2-3.2 | 清单合并 | 发布 Windows 不影响 Linux 已有版本，反之亦然 | `latest.json` 按平台读旧值→合并→回写；保留上一版本信息用于回滚；v2.0 平台 key：`linux-x86_64`、`windows-x86_64`（[release.sh](file:///home/code/sqlcustomer/mysql-client/release.sh#L277-L296) 当前硬编码单平台） |
| V2-3.3 | 双通道上传 | Gitee 与 GitHub Releases 同版本可下载 | `release.sh` 增加 GitHub 通道（`gh release` 或 API + token）；两处文件名、校验值一致；任一通道失败不中断另一通道，但最终报错退出 |
| V2-3.4 | 更新源主备 | Gitee 不可达时能自动回退 GitHub | [tauri.conf.json](file:///home/code/sqlcustomer/mysql-client/src-tauri/tauri.conf.json#L54-L59) `endpoints` 配置 Gitee 主 + GitHub 备（Tauri 按顺序尝试） |
| V2-3.5 | 版本单一来源 | 三处版本号一致 | `tauri.conf.json` 为准，构建时校验 `Cargo.toml` / `package.json` 一致，不一致直接失败 |
| V2-3.6 | 发布前检查 | 防止漏发或发错 | 校验各平台 `.sig` 存在、单附件 < 100MB、更新地址与配置一致、`SHA256SUMS` 已生成 |

## V2-4 大库性能补齐（P1）

| # | 任务 | 验收标准 | 技术要点 |
|---|------|----------|----------|
| V2-4.1 | 服务端分批拉取 | 千万行表可连续滚动，不一次性载入 | 游标/keyset 分页替代 `MAX_RESULT_ROWS = 10_000` 硬截断（[executor.rs](file:///home/code/sqlcustomer/mysql-client/src-tauri/src/core/connection/executor.rs#L83)）；前端 `useVirtualRows` 触底预取 |
| V2-4.2 | 流式导出 | 500 万行导出内存平稳 | CSV/XLSX 边读边写，不再整表入内存；超限时明确提示而非静默截断 |
| V2-4.3 | 按需列加载 | 宽表（200+ 列）打开不卡 | 先取列元数据，勾选列后再查询；默认展示前 N 列 |
| V2-4.4 | BLOB 导入补全 | Base64/文件导入可还原二进制 | 补齐导入侧 Base64 反向解码（导出侧已支持） |

## V2-5 SQL 文件关联与多语句体验（P1）

| # | 任务 | 验收标准 | 技术要点 |
|---|------|----------|----------|
| V2-5.1 | `.sql` 文件关联 | Windows/Linux 双击 `.sql` 打开新查询页 | `bundle.fileAssociations`；启动参数解析 + 单实例锁把二次打开转发给已运行实例 |
| V2-5.2 | 拖拽打开 | 拖 `.sql` 到窗口即打开 | Tauri drag-drop 事件；与未保存内容守卫联动 |
| V2-5.3 | 多语句结果分栏 | 一次执行多条语句，结果按语句分栏 | 后端返回多结果集（当前只保留第一个，见 [executor.rs](file:///home/code/sqlcustomer/mysql-client/src-tauri/src/core/connection/executor.rs#L53)）；前端 tab store 扩展 |

## V2-6 单元格查看器（P1，免费版可用）

| # | 任务 | 验收标准 | 技术要点 |
|---|------|----------|----------|
| V2-6.1 | 大文本/JSON 查看器 | 双击单元格弹出，JSON 可格式化折叠 | 独立窗口组件；非法 JSON 回退原文；10MB 文本不卡顿 |
| V2-6.2 | BLOB 预览 | 图片 BLOB 直接预览，其它给十六进制 | 按列类型 + 文件头识别；大小超限提示；支持另存为 |
| V2-6.3 | 单元格操作 | 复制值 / 复制为 SQL 字面量 / 另存为文件 | 复用现有 `quoteIdentifier` 与转义工具 |

## V2-7 工程质量欠账（P1，贯穿全程）

| # | 任务 | 验收标准 | 技术要点 |
|---|------|----------|----------|
| V2-7.1 | 前端测试 | 核心链路有自动化用例 | Vitest 覆盖 store 与工具函数；Playwright E2E：连接→查询→编辑保存→导出 |
| V2-7.2 | CI 质量门禁 | PR 不绿不合 | `cargo test` + `clippy` + 前端 `typecheck` / `lint` / 单测 |
| V2-7.3 | 清理存量告警 | lint 归零 | 清掉遗留 eslint 错误；编译 warning 只减不增 |
| V2-7.4 | 平台兼容冒烟清单 | 每次发版可照着跑 | Linux/Windows 统一清单：安装、升级、连接、查询、备份、导入导出、激活（见 [smoke-checklist.md](docs/smoke-checklist.md)） |

## v2.0 范围外（明确不做，留给 v2.1+）

- 多数据源（SQLite / Redis / SQL Server）
- AI SQL 助手（NL2SQL、解释与优化建议）
- 企业协作（共享连接/查询库、数据脱敏、高危 SQL 审批）
- macOS 支持（见 V2-2，待账号与构建机到位）
- Windows 代码签名（见 D1，v2.0 以内测版分发）

理由：平台扩张 + 三项体验补齐已占满 v2.0 预算；这些能力均可独立成版本，不与平台改造争抢同一条发布链路。

## v2.0 实施顺序

```
V2-3.5 版本单一来源（先有地基，成本最低）
    ↓
V2-1.1 Windows NSIS 打包跑通（在 Windows 机器上）
    ↓
V2-1.2/1.3 平台差异适配 + 平台分支隔离
    ↓
V2-3.1/3.2/3.3/3.4 build 解耦 + 清单合并 + 双通道上传 + 更新源主备
    ↓
V2-1.4/1.5/1.6 更新链路 + Windows 冒烟 + 内测分发
    ↓
V2-6 单元格查看器 → V2-5 文件关联与多语句（体验）
    ↓
V2-4 大库性能（改动最深，放最后避免阻塞发版）
    ↓
V2-7 质量门禁随每批并行推进
```

## v2.0 验收总标准

1. 干净 Win10/11 安装未签名内测包，按发布说明可完成安装与首次启动，并走完一次自动更新。
2. Linux 与 Windows 同一 tag 的产物齐全，Gitee 与 GitHub 两个通道均可下载，`SHA256SUMS` 校验一致。
3. `latest.json` 同时包含 `linux-x86_64` 与 `windows-x86_64`，发布任一平台不影响另一平台。
4. 千万行表可连续滚动；500 万行导出内存不飙升；宽表打开无卡顿。
5. 双击 `.sql` 与拖拽均可打开；多语句执行结果分栏展示。
6. JSON/BLOB 单元格可独立查看，图片可预览。
7. 现有 Linux CI 质量门禁保持全绿；发版前检查脚本可一条命令跑完。

---

## 变更日志

- 2026-10-26：初稿，P0-P2 分级 + 实施顺序
- 2026-10-26：发布链路打通 — AppImage 重打包压到 83MB、updater endpoint 落地、release.sh 自动生成 latest.json
- 2026-10-30：新增 v2.0 规划 — 主题定为平台扩张（Windows/macOS/三平台发布链路），附带大库性能、SQL 文件关联、单元格查看器、工程质量欠账
- 2026-10-30：V2-0 决策完成 — Windows 暂不签名按内测分发、macOS 移入 v2.1、构建走 Windows+Linux 本地机器、分发 Gitee 主源 + GitHub 镜像；范围随之收缩，实施方案与验收标准同步修订
- 2026-10-30：V2-3.5 完成 — 新增 `tools/check-version.mjs`，以 `tauri.conf.json` 为基准校验 5 处版本号；接入 build.sh、release-all.sh 与 CI；修复 `package-lock.json` 停留在 1.0.4 的存量不一致
- 2026-10-30：V2-1.1 就绪待验证 — 新增 `build.ps1`（与 build.sh 六步对齐、共用同一把更新签名私钥）与 `tauri.windows.conf.json`（NSIS / currentUser / 中英双语），待 Windows 实机跑通
- 2026-10-30：V2-1.2/1.3 完成 — 打开下载目录改为按平台分发命令（xdg-open/open/explorer）；确认 WebKit DMA-BUF、AppImage 原地更新、deb 手动升级分支均已 `cfg` 隔离；Windows 临时凭据文件改由 `%LOCALAPPDATA%\Temp` 的用户级 ACL 保护并即时删除
- 2026-10-30：V2-3.2 完成 — `latest.json` 改为按平台 key 合并回写（新增 `tools/merge-latest.py`），只保留同 tag 条目、拒绝降级发布、保留 `previous` 供回滚；`release.sh` 新增 `--platform linux|windows`，Windows 走 `*.nsis.zip` 签名包
- 2026-10-30：V2-3.4 完成 — updater 配置 Gitee 主源 + GitHub 备源，Tauri 按顺序回退
- 2026-10-30：V2-3.6 完成 — 新增 `tools/check-release.sh`（更新包唯一且带签名、单附件 ≤100MB、更新源与发布仓库一致、生成 SHA256SUMS），`release.sh` 上传前强制调用并把校验文件一并发布
- 2026-10-30：V2-1.4 代码侧就绪 — 清单已写入 `windows-x86_64`，前端按 `auto` 走内置更新器（NSIS 静默升级），`updater:default` / `process:allow-restart` 权限已覆盖 Windows；待 Windows 实机验证升级
- 2026-10-30：V2-1.6 文档就绪 — 发布仓 README 增加「Windows 内测版（未签名）」章节：未签名说明、SmartScreen「更多信息 → 仍要运行」指引、企业终端白名单说明、`Get-FileHash` 校验 `SHA256SUMS`
- 2026-10-30：V2-3.3 待办 — GitHub 公开分发仓与 token 尚未就绪，`release.sh` 目前只上传 Gitee
- 2026-10-30：V2-7 完成（Playwright E2E 除外）— V2-7.1 新增 dataBrowser / editorTabs / queryLibrary 三个 store 单测与 Vitest 内存 localStorage setup，前端用例 89 条；V2-7.2 CI backend 增加 `cargo clippy -- -D warnings`；V2-7.3 修复 14 条 clippy 警告，本项目编译告警归零；V2-7.4 新增 `docs/smoke-checklist.md`，README 与路线图挂上入口。Playwright E2E 需要 Tauri 运行时与真实数据库，暂未落地
- 2026-10-30：V2-7.1 Playwright E2E 落地 — 新增 `playwright.config.ts` 与 `e2e/`（`support/tauri-mock.ts` 注入伪造 `__TAURI_INTERNALS__`、`support/app.ts` 封装页面操作、`connect-query-edit-export.spec.ts` 覆盖连接→查询→编辑保存→导出），无需 Tauri 运行时与真实数据库；`npm run test:e2e` 3 条用例通过，Vitest 用例收敛到 `src/**/*.test.ts`
