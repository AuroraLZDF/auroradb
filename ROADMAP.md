# AuroraDB 功能路线图

生成日期：2026-10-26
项目状态：v1.0.0 已发布，MySQL/PostgreSQL 支持、SSH 隧道、表/库设计器、结构对比、监控、审计、导入导出、离线激活体系已就绪。

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
| **AppImage 101MB 超限** | Gitee 单附件 100MB 上限，当前 AppImage 被跳过 → 自动更新链路断。解法：`appimagetool --comp zstd` 压到 100MB 内 | blocker |
| **Linux 应用内更新** | deb 不支持 tauri-plugin-updater，只有 AppImage 支持。更新 release.sh 改发 AppImage | blocker |
| **免费版 / Pro 边界细化** | 免费版：多连接 + 查询 + 导入导出 + 表设计器；Pro：结构同步 + 定时备份 + 审计 + SSH 多跳 + 跨库搜索 | 产品 |
| **试用期** | 14 天全功能；实现：激活码带 trial 标记，或首次启动开始倒计时 | 变现 |

## 实施顺序建议

```
blocker: AppImage 压缩 + updater 链路打通
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

## 变更日志

- 2026-10-26：初稿，P0-P2 分级 + 实施顺序
