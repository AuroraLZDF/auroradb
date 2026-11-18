# AuroraDB 功能路线图

生成日期：2026-10-26
项目状态：v1.0.8 已发布，MySQL/PostgreSQL 支持、SSH 隧道、表/库设计器、结构对比、监控、审计、导入导出等能力已就绪；全部功能永久免费，源码以 AGPL-3.0 全量开源；v1.x 功能路线图已全部落地，v2.0 / v3.0 规划见文末。

---

## P0 · 基础体验补齐（2 周内）

| # | 功能 | 验收标准 | 技术要点 |
|---|------|----------|----------|
| P0-1 | **多查询标签页** | 可同时打开 N 个查询页，每个页签独立 SQL + 独立结果集；支持重命名、关闭、关闭其他、Ctrl+Tab 切换 | Pinia store 管理 `Tab[]`，每个 Tab 含 sql / results / active / dirty；SqlEditor.vue + DataTable.vue 由 store 驱动 |
| P0-2 | **SQL 格式化** | 快捷键 Shift+Alt+F 美化当前编辑器内容；支持 MySQL 与 PostgreSQL 方言 | 后端调 sqlparser / sql-formatter，纯文本处理；不解析 AST 也能先做基础缩进 |
| P0-3 | **查询草稿自动保存 + 崩溃恢复** | 每次编辑 2 秒 debounce 写入 IndexedDB；下次启动恢复上次全部 Tab 的 SQL | `localStorage` 或 IndexedDB（大 SQL 不卡）；版本号兼容升级 |
| P0-4 | **命令面板 + 快捷键体系** | Ctrl+K 呼出命令菜单，支持"执行查询 / 格式化 / 新建 Tab / 关闭 Tab / 切换连接 / 跳转到表"等；快捷键全量可配置 | 集中式 command registry，Editor/Footer 监听快捷键 |
| P0-5 | **大结果集体验** | 虚拟滚动已实现，补：复制选中、复制整列、列宽自动适应、冻结前 N 列 | DataTable.vue 改造，避免 DOM 爆炸 |

## P1 · 增强能力

| # | 功能 | 验收标准 | 技术要点 |
|---|------|----------|----------|
| P1-1 | **EXPLAIN 可视化** | 选中 SQL 一键 Explain，树形计划 + 耗时/扫描行数高亮；MySQL 与 PostgreSQL 分别适配 | MySQL EXPLAIN JSON、PostgreSQL EXPLAIN (ANALYZE, FORMAT JSON)；前端组件渲染 |
| P1-2 | **ER 图自动生成** | 选中数据库一键生成关系图，表字段 + 外键连线 + 导出 PNG/SVG | 后端返回表列表与 FK 关系，前端用 dagre/elk 布局 |
| P1-3 | **备份 / 还原 / 定时任务** | 基于 mysqldump / pg_dump；定时策略（每天/每周/自定义 Cron）；备份历史与一键还原 | Rust 侧 spawn 子进程；定时用 chrono + tokio interval；无需授权门禁，全部功能直接可用 |
| P1-4 | **跨库全局搜索** | 按表名 / 字段名 / 数据内容搜索，支持 LIKE 与正则；返回匹配表 + 行数 | 后端拼接 `information_schema` 查询 + `SELECT * FROM t WHERE col LIKE`；限制结果数 |
| P1-5 | **会话与锁管理** | 活动会话列表 + 慢查询 + KILL；MySQL `show processlist`、PostgreSQL `pg_stat_activity` | 后端 connection pool 复用或独立短连接；KILL 需鉴权 |
| P1-6 | **用户与权限管理** | 图形化创建用户 / GRANT / REVOKE，替代手写 SQL | 后端读 `mysql.user` / `pg_roles` + DDL 生成 |
| P1-7 | **存储过程 / 函数 / 触发器 / 事件编辑器** | 与表设计器并列的对象类型；支持 CREATE / ALTER / DROP | 后端返回 `information_schema.routines` 等；前端 Monaco 复用 |
| P1-8 | **数据同步（表→表 / 库→库）** | 在现有 SchemaDiff 基础上扩：结构同步 + 数据同步；支持 upsert、按主键匹配 | 后端 src-tauri/src/commands/sync.rs；无需授权门禁，全部功能直接可用 |

## P2 · 扩大用户面

| # | 功能 | 说明 |
|---|------|------|
| P2-1 | **SQLite 驱动** | 接入成本最低，覆盖本地开发场景；用 rusqlite |
| P2-2 | **Redis 驱动** | 用 redis-rs；支持 key/string/hash/list/set/zset 可视化 |
| P2-3 | **SQL Server 驱动** | tiberius 或 ODBC；优先级较低 |
| P2-4 | **单元格查看器** | JSON 格式化、图片预览、大文本独立窗口 |
| P2-5 | **导出增强** | Excel (xlsx)、带格式 CSV、按查询结果批量导出 |

## 必须先解决的基础闭环

| 项 | 说明 | 优先级 |
|----|------|--------|
| **AppImage 体积超限** | ✅ 已解决：`tools/build/appimage-repack.sh` 复用原 runtime，strip 符号后用 zstd 重压包；full 实测 103,709,176 B（约 98.9 MiB），另提供 `--lite` 变体（约 30.2 MiB，依赖系统 webkit2gtk-4.1），构建脚本自动调用并重新签名 | 已解决 |
| **Linux 应用内更新** | ✅ 已解决：updater endpoint 指向 `https://github.com/AuroraLZDF/auroradb/releases/latest/download/latest.json`，`tools/publish/linux.sh` 上传 AppImage 后自动合并并上传 latest.json；deb 不支持原地更新，仅 AppImage 生效 | 已解决 |
| **打赏与社区** | README 与首页提供 GitHub Sponsors、爱发电、微信/支付宝收款码入口；打赏自愿，不设付费档位、不影响功能 | 社区 |

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
P1-3 备份还原 → P1-8 数据同步
```

---

# v2.0 · 平台扩张（目标：Linux + Windows 双平台可用，macOS 延后到 v2.1）

> 定位：v1.x 已把功能面铺满（MySQL/PG、多 Tab、Explain、ER 图、Schema 同步、监控审计、备份还原、全局搜索、i18n、自动更新）。
> v2.0 不再堆功能，只解决「装不上、不敢用、用不动」三类问题：先让 Windows 用户能装能用，再补大库与日常操作体验，最后把交付流程工程化。

## V2-0 前置决策结论（2026-10-30 已拍板）

| # | 决策项 | 结论 | 影响与遗留 |
|---|--------|------|------------|
| D1 | **Windows 代码签名** | **暂不签名，v2.0 按内测版分发** | 发布页与安装向导需注明「未签名内测版」、提供 SHA256 校验值、给出「更多信息 → 仍要运行」指引；企业终端受组策略拦截属已知限制。v2.1 再评估 Azure Trusted Signing（要求组织成立约 3 年）或 OV/EV 云端签名 |
| D2 | **macOS 支持** | **v2.0 不做，移入 v2.1+** | 无 Apple Developer 账号、无 macOS 构建机，即使出包也无法完成签名、公证与 Gatekeeper 验证，属于「发了也装不上」 |
| D3 | **构建环境** | **Windows + Linux 本地构建**（现有两台机器）；CI 仅保留现有 Linux 质量门禁 | Tauri 不能交叉编译，Windows 包必须在 Windows 机器产出。风险：发布依赖本地环境，必须把 Windows 构建步骤脚本化，并保证两台机器步骤一致 |
| D4 | **分发渠道** | **统一 GitHub Releases**（单仓维护：安装包发布到本仓库 `AuroraLZDF/auroradb` Releases，源码同仓以 AGPL-3.0 开源） | 原方案为「Gitee 主源 + GitHub 镜像」，因 Gitee 发行版附件总量仅 1GB 无法承载持续发版，2026-11-03 改为只发 GitHub；`tools/publish/` 下两个脚本统一走 GitHub API；更新清单 endpoint 单条指向 GitHub；老版本（≤v2.0.0）需手动下载一次新版本 |
| D5 | **更新清单结构** | **按平台 key 合并回写，保留上一版本** | v2.0 清单平台：`linux-x86_64` + `windows-x86_64`；`darwin-*` 待 v2.1 启用。合并逻辑见 V2-3.2 |

> 结论带来的范围变化：**v2.0 = Linux（存量）+ Windows（内测）+ GitHub 统一发布 + 三项体验补齐**，工作量比原方案减少约 30%。

## V2-1 Windows 支持（P0）

| # | 任务 | 验收标准 | 技术要点 |
|---|------|----------|----------|
| V2-1.1 | 打包目标 | 产出 NSIS 安装包，干净 Win10/11 可安装、卸载、覆盖升级 | [tauri.conf.json](src-tauri/tauri.conf.json#L50-L51) 按平台配置 `nsis`；**应用内更新只支持 NSIS，MSI 不支持**；校验 `icon.ico` 各尺寸 |
| V2-1.2 | 平台差异适配 | 日志、下载目录、临时凭据文件、外部工具调用均正常 | 临时凭据文件 Linux 用 0600，Windows 需改用受限 ACL 或改为 stdin 传参；`mysqldump`/`mysql` 缺失时给出 Windows 版安装指引（MySQL Shell 或官方客户端） |
| V2-1.3 | 后端平台分支 | 无 Linux 专属逻辑泄漏到 Windows | `WEBKIT_DISABLE_DMABUF_RENDERER`、AppImage 重打包、deb 手动升级分支全部 `#[cfg(target_os)]` 隔离 |
| V2-1.4 | 更新链路 | 老版本能收到并完成 NSIS 静默升级 | updater 清单增加 `windows-x86_64`；NSIS 参数与安装目录保持一致 |
| V2-1.5 | 冒烟 | 连接（含 SSH 隧道 + TLS）、查询、编辑保存、备份还原、导入导出全通过 | 在 Windows 机器做一次完整手工冒烟，结果记录在案 |
| V2-1.6 | 内测版分发 | 用户能正确安装并验证包完整性 | 安装包与发布页标注「未签名内测版」；发布 `SHA256SUMS`；README/发布说明给出 SmartScreen「更多信息 → 仍要运行」指引；补一份企业终端被拦截的说明 |

## V2-2 macOS 支持（v2.2.3 阶段 1 已落地：arm64 · 未签名预览版）

构建机不再是阻塞项：GitHub Actions 提供 `macos-14`（Apple Silicon）runner，公开仓库免费。

已完成（阶段 1）：

- ✅ 图标补齐 `icon.icns`（`cargo tauri icon` 生成，源图建议 ≥1024×1024）
- ✅ `bundle.macOS = { minimumSystemVersion: "10.15", signingIdentity: "-" }`（ad-hoc 签名，Apple Silicon 可启动）
- ✅ `tools/build/macos.sh`（默认 arm64，`--intel` 切 x86_64，产出 `.app` + `.dmg` + `.app.tar.gz`/`.sig`）
- ✅ `tools/publish/macos.sh`（上传 dmg + 更新包，合并 `darwin-aarch64` 条目）
- ✅ `release.yml` 新增 `build-macos` / `publish-macos`，`verify` 断言三平台齐全
- ✅ `merge-latest.py` 的缺失平台告警覆盖 `darwin-aarch64`

限制与后续（阶段 2，待 Apple Developer 账号）：

- 未签名 → 用户首次打开需右键 → 打开（README 有说明）
- 无公证 → 无法无感安装
- 届时补齐：Developer ID 签名 → notarytool 公证 → `stapler` 装订；CI secret `APPLE_CERTIFICATE` / `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID`
- 如后续需要 Intel 版：`./tools/build/macos.sh --intel` + 发布脚本自动用 `darwin-x86_64` key
- 遗留验证：Keychain 首次授权弹窗、`mysqldump`/`mysql` 在 Homebrew 路径下的查找行为

## V2-3 发布与更新链路双平台化（P0）

| # | 任务 | 验收标准 | 技术要点 |
|---|------|----------|----------|
| V2-3.1 | 构建与发布解耦 | Linux、Windows 各自构建，产物汇总到一处发布 | 构建与发布分别收敛到 `tools/build/`、`tools/publish/`，Linux 用 `.sh`、Windows 用 `.ps1`，入口不再做跨平台分发 |
| V2-3.2 | 清单合并 | 发布 Windows 不影响 Linux 已有版本，反之亦然 | `latest.json` 按平台读旧值→合并→回写；保留上一版本信息用于回滚；v2.0 平台 key：`linux-x86_64`、`windows-x86_64`（[linux.sh](tools/publish/linux.sh) 只写 Linux 条目，Windows 条目由 `windows.ps1` 写入） |
| V2-3.3 | 统一 GitHub 发布 | 同一 tag 的 Linux / Windows 产物均发布到本仓库 `AuroraLZDF/auroradb` Releases | `tools/publish/` 下两个脚本走 GitHub API：建发行版、删同名旧附件、`uploads.github.com` 上传（单附件 ≤2GB）；任一附件失败立即中止，不写 latest.json |
| V2-3.4 | 更新源单一入口 | 客户端从 GitHub 读取清单 | [tauri.conf.json](src-tauri/tauri.conf.json) `endpoints` 只保留 `https://github.com/AuroraLZDF/auroradb/releases/latest/download/latest.json` |
| V2-3.5 | 版本单一来源 | 三处版本号一致 | `tauri.conf.json` 为准，构建时校验 `Cargo.toml` / `package.json` 一致，不一致直接失败 |
| V2-3.6 | 发布前检查 | 防止漏发或发错 | 校验各平台 `.sig` 存在、单附件 ≤2GB、更新地址与配置一致、`SHA256SUMS` 已生成 |
| V2-3.7 ✅ | tag 触发自动发布 | `git push --tags` 后无需人工介入，两个平台产物与合并后的清单自动到位 | [.github/workflows/release.yml](.github/workflows/release.yml)：`check`（版本/tag 比对/签名私钥/前端）→ `build-linux` + `build-windows` → `publish-linux` → `publish-windows` → `verify`（断言清单含两个平台）。`publish-*` 同时依赖两个 build，任一平台构建失败即整体不发布 |
| V2-3.8 ✅ | e2e 进入 CI 门禁 | 每次 push/PR 跑 Playwright 用例 | [.github/workflows/ci.yml](.github/workflows/ci.yml) 新增 `e2e` job（`npx playwright install --with-deps chromium` + `npm run test:e2e`），失败上传 report |

## V2-4 大库性能补齐（P1）

| # | 任务 | 验收标准 | 技术要点 |
|---|------|----------|----------|
| V2-4.1 | 服务端分批拉取 | 千万行表可连续滚动，不一次性载入 | 游标/keyset 分页替代 `MAX_RESULT_ROWS = 10_000` 硬截断（[executor.rs](crates/core/src/connection/executor.rs#L81)）；前端 `useVirtualRows` 触底预取 |
| V2-4.2 | 流式导出 | 500 万行导出内存平稳 | CSV/XLSX 边读边写，不再整表入内存；超限时明确提示而非静默截断 |
| V2-4.3 | 按需列加载 | 宽表（200+ 列）打开不卡 | 先取列元数据，勾选列后再查询；默认展示前 N 列 |
| V2-4.4 | BLOB 导入补全 | Base64/文件导入可还原二进制 | 补齐导入侧 Base64 反向解码（导出侧已支持） |

## V2-5 SQL 文件关联与多语句体验（P1）

| # | 任务 | 验收标准 | 技术要点 |
|---|------|----------|----------|
| V2-5.1 | `.sql` 文件关联 | Windows/Linux 双击 `.sql` 打开新查询页 | `bundle.fileAssociations`；启动参数解析 + 单实例锁把二次打开转发给已运行实例 |
| V2-5.2 | 拖拽打开 | 拖 `.sql` 到窗口即打开 | Tauri drag-drop 事件；与未保存内容守卫联动 |
| V2-5.3 | 多语句结果分栏 | 一次执行多条语句，结果按语句分栏 | 后端返回多结果集（当前只保留第一个，见 [executor.rs](crates/core/src/connection/executor.rs#L51)）；前端 tab store 扩展 |

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
| V2-7.4 | 平台兼容冒烟清单 | 每次发版可照着跑 | Linux/Windows 统一清单：安装、升级、连接、查询、备份、导入导出（见 [smoke-checklist.md](docs/smoke-checklist.md)） |

## v2.0 范围外（明确不做，留给 v2.1+）

- 多数据源（SQLite / Redis / SQL Server）
- AI SQL 助手（NL2SQL、解释与优化建议）
- 企业协作（共享连接/查询库、数据脱敏、高危 SQL 审批）
- macOS 公证件（见 V2-2 阶段 2，待 Apple Developer 账号）
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
V2-3.1/3.2/3.3/3.4 build 解耦 + 清单合并 + 统一 GitHub 发布 + 更新源单一
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
2. Linux 与 Windows 同一 tag 的产物齐全，在 GitHub 发行版可下载，`SHA256SUMS` 校验一致。
3. `latest.json` 同时包含 `linux-x86_64` 与 `windows-x86_64`，发布任一平台不影响另一平台。
4. 千万行表可连续滚动；500 万行导出内存不飙升；宽表打开无卡顿。
5. 双击 `.sql` 与拖拽均可打开；多语句执行结果分栏展示。
6. JSON/BLOB 单元格可独立查看，图片可预览。
7. 现有 Linux CI 质量门禁保持全绿；发版前检查脚本可一条命令跑完。

---

# v3.0 · 数据可视化 BI 与智能辅助（目标：从「管数据」延伸到「用数据」）

> 定位：v2.x 已完成平台扩张（Linux / Windows / macOS 三平台 + 发布链路自动化）。v3.0 主题为数据可视化 BI（主线），同步落地 AI SQL 助手与 ER 建模正向化。全部功能永久免费，维持 AGPL-3.0 全量开源。

## V3-0 前置决策结论（2026-11-18 已拍板）

| # | 决策项 | 结论 | 影响与遗留 |
|---|--------|------|------------|
| D1 | **版本主线** | **数据可视化 BI，一步做到仪表盘** | 结果集图表化 → 图表构建器 → 仪表盘 + 定时刷新，完整轻量 BI 闭环 |
| D2 | **AI SQL 助手接入** | **BYOK 用户自带 Key，OpenAI 兼容端点** | 无服务端成本，延续隐私/离线定位；不做内置付费服务，Key 不出本机 |
| D3 | **ER 图/建模** | **补正向能力：模型设计器 → DDL 生成 → Schema Diff 比对同步** | 反向 ER 已有，正向补齐形成建模闭环 |
| D4 | **免费/Pro 归属** | **全部免费** | 2026-11-04 起已全量开源，无 Pro 边界问题 |
| D5 | **图表库选型** | **ECharts 按需引入** | 图表类型全、生态好；AppImage 体积敏感，必须 tree-shaking（`echarts/core` 只注册所用图表） |
| D6 | **后续排期原则（2026-11-18 补充拍板）** | **个人 SQL 工作台日常功能优先** | v3.0 四条线（BI / AI / ER / 血缘）均已收尾；评审结论：BI 仪表盘、ER 建模、血缘分析属低频治理类工具，与个人日常使用场景不匹配。下一阶段优先编辑器/查询体验本身（查询历史管理、结果集导出、连接管理增强等日常功能）；治理类功能及 follow-up（如视图列展开）冻结排期，已完成功能保留不砍 |

## V3-1 数据可视化 BI（主线）

| # | 任务 | 验收标准 | 技术要点 |
|---|------|----------|----------|
| V3-1.1 | **结果集图表化** | 查询结果一键切换 表格/图表 视图；柱状/折线/饼图/散点开箱即用；自动识别维度列/度量列 | 挂在 [DataTable.vue](src-ui/src/components/DataTable.vue) 现有结果集上，入口成本最低；列类型推断（文本→维度、数值→度量），识别不准时允许手动调整 |
| V3-1.2 | **图表构建器** | 字段映射（X/Y/分组/聚合）可编辑；聚合支持 SUM/AVG/COUNT/MIN/MAX；图表类型随意切换 | 复用 V3-1.1 渲染层；结果集已在前端，默认前端聚合；超限结果集提示改为聚合查询下推数据库 |
| V3-1.3 | **仪表盘** | 多图表自由布局成 Dashboard；JSON 持久化，重启可恢复；支持打开/重命名/删除 | 布局 JSON 存数据目录（复用备份历史的持久化模式）；网格布局 + 拖拽调整 |
| V3-1.4 | **定时刷新与参数化** | 仪表盘图表支持手动/固定间隔刷新；`:param` 参数化查询 + 交互式筛选 | 定时复用 P1-3 定时备份的 chrono + tokio interval 方案；参数绑定由后端拼接防注入 |
| V3-1.5 | **卡片拖拽布局** | 卡片可拖拽换位、拖角调整跨列/跨行（1~4 列 × 1~3 行），布局随定义持久化 | 原生 HTML5 DnD 换位 + Pointer 事件手柄缩放，CSS Grid `grid-auto-flow: dense` 排布，无第三方库 |

## V3-2 AI SQL 助手（BYOK）

| # | 任务 | 验收标准 | 技术要点 |
|---|------|----------|----------|
| V3-2.1 | **BYOK 设置页** | 可配置 OpenAI 兼容端点 + API Key + 模型名，带连接测试 | Key 存储沿用现有凭据加密方案；请求直连用户配置的端点，不经过任何中间服务 |
| V3-2.2 | **NL2SQL** | 自然语言生成 SQL，自动注入当前连接的库表结构上下文；可预览、一键插入编辑器 | information_schema → 精简 schema prompt（表/列/注释/外键），控制 token 用量 |
| V3-2.3 | **SQL 解释与优化建议** | 选中 SQL 一键解释、给出优化建议；与现有 EXPLAIN 可视化入口打通 | 流式输出；解释结果支持以注释形式插回编辑器 |

## V3-3 ER 图 / 数据建模深化

| # | 任务 | 验收标准 | 技术要点 |
|---|------|----------|----------|
| V3-3.1 ✅ | **模型设计器** | 可视化编辑表/字段/关系，保存为模型文件（JSON），不连库也能建模 | 新增 erModel store（localStorage 持久化）与 ErModelDesigner 视图：SVG 画布上添加/删除/重命名表、编辑字段（PK/NOT NULL/UNIQUE/默认值/注释）、拖拽建立关系与拖拽定位；模型文件 JSON 导入导出走系统对话框；工具菜单新增入口，无连接也可用 |
| V3-3.2 ✅ | **正向 DDL 生成** | 模型一键生成 CREATE TABLE DDL，MySQL / PostgreSQL 方言可选 | 新增 `utils/erModelDdl.ts`：模型表/字段/关系转换成 `ddl.ts` 的 `TableDdlInput`，复用表设计器同一条方言生成链路（MySQL 内联约束 + ALTER COMMENT，PG CONSTRAINT + COMMENT ON）；建表语句按外键依赖拓扑排序（环引用兜底原序），悬空关系自动过滤；设计器工具栏「生成 DDL」弹窗预览 + 方言切换 + 一键复制 |
| V3-3.3 ✅ | **模型 ↔ 库比对同步** | 模型与实际库结构 Diff，差异一键同步 | 新增 `utils/erModelSync.ts`：目标库快照取自 `get_er_diagram_data`（与反向 ER 同一数据源），前端 diff 出新表（复用 V3-3.2 拓扑序建表链路）/列增改删（复用表设计器 ALTER 生成链路，current=模型值 original=库值）/缺失外键补建，库多出的表列可选 DROP（默认不勾选）；执行复用 `apply_schema_diff` 逐条应用、遇错即停；设计器工具栏「与库比对」弹窗：选连接/库 → 差异清单勾选 → SQL 预览 → 二次确认执行后自动重新比对；单测新增 15 条 |

## V3-4 前置清理与体积回归

| # | 任务 | 验收标准 | 技术要点 |
|---|------|----------|----------|
| V3-4.1 | **pro 目录收敛** | `src-ui/src/pro/` 目录命名清理（2026-11-04 起全部功能免费开源，pro 命名已无意义） | ErDiagram.vue 等迁往常规目录；同步检查公开仓库导出脚本与文档中的 pro 引用 |
| V3-4.2 | **包体积回归** | 引入 ECharts 后 AppImage full 体积无明显回退 | `echarts/core` 按需注册组件与图表类型；对照 v2.x 压缩链路（[appimage-repack.sh](tools/build/appimage-repack.sh)）复测 |

## V3-5 数据血缘分析

| # | 任务 | 验收标准 | 技术要点 |
|---|------|----------|----------|
| V3-5.1 ✅ | **表级血缘** | 解析 SQL 推导「源表 → 目标表」依赖并可视化；工具菜单入口，不依赖连接离线可用 | 新增 `crates/core/src/sql/lineage.rs`：sqlparser AST + Visitor 收集关系位置，覆盖 INSERT...SELECT / REPLACE / UPDATE（JOIN / FROM / 子查询）/ DELETE（多表 + USING）/ CTAS / LIKE / CLONE / CREATE VIEW / WITH CTE 前缀 DML（CTE 别名不计入源表）；未限定表名补默认库前缀、跨库引用保留库名；单条解析失败计数跳过不中断。前端 `utils/lineageGraph.ts` 边去重合并 + `LineageDialog.vue` SVG 血缘图（源表左 / 目标表右 / 读写居中，连线标注操作类型）+ 语句明细；方言 MySQL / PG 可选 |
| V3-5.2 ✅ | **列级血缘** | 解析 SELECT 列表达式，推导字段 → 字段依赖并叠加到血缘图 | `lineage.rs` 扩展 `ColumnLineage`（target ← sources）：INSERT/REPLACE 列清单与 SELECT 投影按位置对齐、无列清单时投影名/别名作目标列名、UPDATE 赋值右侧表达式列引用（别名映射 + 无限定列单表归属、`SET a = a` 自赋值跳过）、CTAS / CREATE VIEW 投影名对齐；`SELECT *`、UNION 主体、CTE 中间层列引用不解析（fail-open）。前端 `buildColumnLineageGraph` 列节点图 + LineageDialog 表级/列级视图切换，语句明细叠加列级映射；视图列展开留待后续 |

## v3.0 实施顺序

```
V3-4.1 pro 目录收敛（地基，先做避免路径返工）
    ↓
V3-1.1 结果集图表化 → V3-1.2 图表构建器 → V3-1.3 仪表盘 → V3-1.4 定时刷新与参数
    ↓
V3-2 AI SQL 助手（与 BI 并行可开发，互不阻塞）
    ↓
V3-3 ER 建模正向化（依赖 Schema Diff 已有能力）
    ↓
V3-4.2 包体积回归（随每批验证，不留到最后）
    ↓
V3-5.1 表级血缘（纯文本解析，不依赖连接）
```

## v3.0 验收总标准

1. 查询结果一键切换图表，柱状/折线/饼图/散点开箱即用，维度/度量可手动调整。
2. 图表构建器完成字段映射与聚合；仪表盘多图布局可保存恢复、可定时刷新、支持参数筛选。
3. BYOK 配置完成后 NL2SQL / 解释 / 优化三类能力可用，API Key 与查询上下文不出本机。
4. 模型设计器可正向生成 DDL，并通过 Schema Diff 同步到实际库。
5. AppImage 体积对照 v2.x 基线无明显回退；三平台构建与发布链路不受影响。
6. 粘贴 SQL 即可得到表级血缘图，解析失败的语句不阻塞整批分析。

---

## 变更日志

- 2026-10-26：初稿，P0-P2 分级 + 实施顺序
- 2026-10-26：发布链路打通 — AppImage 重打包压到约 98.9 MiB、updater endpoint 落地、release.sh 自动生成 latest.json
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
- 2026-11-02：构建/发布脚本收敛 — 入口只剩 `build.sh`（按平台分发，`--windows` 转 PowerShell）与 `release.sh`（`--platform linux|windows`）；平台实现下沉 `tools/build/`（`linux.sh`、`windows.ps1`、`appimage-repack.sh`、`appimage-offline.sh`）与 `tools/publish/windows.ps1`；`build-appimage-offline.sh` 内联吸收 `download-offline.sh`（已删除），Windows 构建不再内置发布步骤；删除只发 Linux 的 `release-all.sh`，版本号同步改为手工流程并写入 README
- 2026-11-03：分发渠道改为只发 GitHub — Gitee 发行版附件总量仅 1GB，无法承载持续发版；`tools/publish/linux.sh`、`windows.ps1` 重写为 GitHub API 版（建发行版、覆盖同名附件、latest.json 作为发行版附件上传），删除双通道补丁 `github-full.sh`；updater endpoint 收敛为单条 GitHub 地址；`check-release.sh` 附件上限改为 2GB。v2.0.1 起见新版本走 GitHub 自动更新，≤v2.0.0 老客户端需手动下载一次
- 2026-11-04：取消买断 / 试用 / 激活等商业授权 — 全部功能永久免费开放，源码（含原增强功能模块）以 AGPL-3.0 全量开源；收益模式改为打赏与赞助，相关文档与 README 同步更新
- 2026-11-18：新增 v3.0 规划 — 主题定为数据可视化 BI（结果集图表化 → 图表构建器 → 仪表盘 + 定时刷新），同步确定 AI SQL 助手（BYOK，OpenAI 兼容端点）与 ER 建模正向化（模型设计器 → DDL → Schema Diff 同步）；全部功能免费，图表库选型 ECharts 按需引入
- 2026-11-18：V3-0 BI 线四批完成（V3-4.1 收敛 features 目录、V3-1.1 结果集图表化、V3-1.2 图表构建器、V3-1.3 仪表盘、V3-1.4 定时刷新与 :param 参数化）；V3-4.2 包体积回归复测通过 — ECharts 经 utils/echarts 单一动态入口拆为懒加载 chunk，主包 1623KB → 1055KB（gzip 532 → 336KB），echarts chunk 576KB（gzip 199KB）仅在首次渲染图表时加载，前端总体积 +8KB
- 2026-11-18：V3-1.5 卡片拖拽布局完成 — BI 主线收尾；卡片支持拖拽换位与拖角调整跨列跨行（1~4×1~3），布局随卡片定义持久化，4 列网格自适应降级到 2 列/1 列
- 2026-11-18：V3-2.1 BYOK 设置页完成 — 新增 Rust 端 ai.rs（base_url/model 存 app 数据目录 ai.json，API Key 存系统密钥环与连接密码同方案，`ai_test_connection` 直连端点拉取模型列表）；设置菜单新增「AI 助手」入口与配置对话框
- 2026-11-18：V3-2.2 NL2SQL 完成 — Rust 端新增 `ai_chat`（OpenAI 兼容 /chat/completions，非流式）；SQL 编辑器新增「AI 生成 SQL」入口，生成前注入当前库 schema 快照精简上下文（限 100 表 × 40 列控制 token），系统提示词限定只读 SELECT 且只用给出的表列；结果先预览（自动剥代码围栏）再插入编辑器，强制人工确认
- 2026-11-18：V3-2.3 SQL 解释与优化完成 — 复用 `ai_chat`，SQL 编辑器新增「AI 解释/优化」入口：逐句解释 SQL 意图、性能问题清单 + 优化写法建议（提示词按方言组装）；超长 SQL 预览截断，结果可复制；V3-2 AI 助手线收尾
- 2026-11-18：V3-3.1 模型设计器完成 — 新增 `stores/erModel.ts`（多模型 CRUD，模型 JSON 持久化 localStorage）与 `features/ErModelDesigner.vue`（SVG 画布：拖拽定位、拖拽连线建关系、右侧面板编辑表/字段）；模型文件（.json）导入导出走系统对话框（`read_file_from_dialog` / `save_binary_file_to_dialog`）；工具菜单新增「模型设计器」，不依赖连接离线可用
- 2026-11-18：V3-3.2 正向 DDL 生成完成 — 新增 `utils/erModelDdl.ts`（复用 `ddl.ts` 方言链路生成 CREATE TABLE：PK/NOT NULL/UNIQUE/默认值/注释/外键，拓扑排序建表顺序，环引用兜底）；设计器工具栏「生成 DDL」弹窗：MySQL/PG 方言切换、预览、一键复制；单测新增 10 条（含拓扑序、约束命名去重、悬空关系过滤）
- 2026-11-18：V3-3.3 模型↔库比对同步完成，V3-3 ER 建模线收尾 — 新增 `utils/erModelSync.ts`（库快照取自 `get_er_diagram_data`，前端 diff：新表拓扑序 CREATE、列差异按方言 ALTER（复用表设计器生成链路）、缺失外键补建、多余表列可选 DROP）；设计器工具栏「与库比对」弹窗：连接/库选择 → 差异勾选（DROP 默认不勾）→ SQL 预览 → 二次确认 → `apply_schema_diff` 逐条执行遇错即停，成功后自动重新比对；单测新增 15 条
- 2026-11-18：V3-5.1 表级血缘完成 — 新增 v3.0「V3-5 数据血缘分析」小节；`crates/core/src/sql/lineage.rs` 基于 sqlparser AST + Visitor 提取表级血缘（INSERT...SELECT / REPLACE / UPDATE / DELETE / CTAS / LIKE / CLONE / CREATE VIEW / WITH 前缀 DML，CTE 别名不入源表，解析失败逐条计数跳过，22 条单测）；Tauri 命令 `analyze_sql_lineage`（MySQL / PG 方言）；前端 `utils/lineageGraph.ts`（9 条单测）与 `LineageDialog.vue` SVG 血缘图 + 语句明细；工具菜单「血缘分析」离线可用；前端单测 171 → 180 条
- 2026-11-18：V3-5.2 列级血缘完成，V3-5 数据血缘线收尾 — `lineage.rs` 新增 `ColumnLineage`（目标列 ← 源列列表）：INSERT/REPLACE 列清单与 SELECT 投影按位置对齐、无列清单时投影名/别名作目标列名、UPDATE 赋值右侧表达式列引用（别名映射、无限定列仅单表归属、`SET a = a` 自赋值跳过）、CTAS / CREATE VIEW 投影名对齐；子查询内列引用、`SELECT *`、UNION 主体、CTE 中间层不解析（fail-open），Rust 单测 22 → 45 条；前端 `buildColumnLineageGraph`（单测 9 → 16 条）、LineageDialog 表级/列级视图切换 + 语句明细列级映射；命令与调用方签名不变；前端单测 180 → 187 条
- 2026-11-18：v3.0 复盘后补充决策 D6「后续排期原则」— 评审认为 BI 仪表盘 / ER 建模 / 血缘分析属低频治理类功能，与个人日常使用场景不匹配；下一阶段优先编辑器 / 查询体验日常功能，治理类功能与 follow-up（视图列展开等）冻结排期，已完成功能保留不砍
