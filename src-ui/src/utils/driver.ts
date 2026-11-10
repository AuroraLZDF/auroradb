import type { DriverKind } from '@/stores/savedConnections'

export type { DriverKind } from '@/stores/savedConnections'

/**
 * 驱动能力表：所有「按驱动禁用/降级」的判断集中在这里，组件里不要再散落
 * `driver === 'postgres'` 之类的硬编码。
 *
 * 后续新增驱动（例如 sqlite）时，需要在这里补一条能力配置；
 * 因为类型是 Record<DriverKind, DriverCapabilities>，漏配会直接编译失败。
 */
export interface DriverCapabilities {
  /** 驱动展示名（只读展示，不走 i18n） */
  label: string
  /** 控制台提示符前缀，例如 `mysql [db]>` / `postgres [db]>` */
  prompt: string
  /** 标识符引用符：MySQL 反引号 / PostgreSQL 双引号 */
  identifierQuote: '`' | '"'
  /** 默认端口 */
  defaultPort: number
  /** 是否支持在前端编辑并保存表数据（驱动写路径未实现时置 false 即可禁用编辑） */
  editableTableData: boolean
  /** 日期/时间列是否支持 MySQL 风格的零值默认值（0000-00-00） */
  zeroDateDefaults: boolean
  /** 是否原生支持 SHOW CREATE TABLE；否则控制台回退到 get_table_structure */
  nativeShowCreateTable: boolean
  /** 是否原生支持 USE db 语句；否则切换会话（PG 的 database 参数由后端映射为 SET search_path） */
  nativeUseStatement: boolean
  /** 信息面板 DDL 是否交给 utils/sql.ts 的 formatSql 统一格式化 */
  formatDdl: boolean
  /** 执行 SQL 前是否必须选择数据库（MySQL 必须；PG 可走默认 search_path） */
  requiresDatabase: boolean
  /** 表名限定方式：database=库名.表名（MySQL），schema=schema.表名（PG） */
  tableQualification: 'database' | 'schema'
  /** Pro 工具支持情况（P2 起 PG 全部支持） */
  proTools: {
    schemaDiff: boolean
    monitor: boolean
    backup: boolean
    globalSearch: boolean
    erDiagram: boolean
  }
  /** 监控指标语义：MySQL 的 InnoDB/COM_* 与 PG 的共享缓冲/行数统计展示名称不同 */
  monitorDialect: 'mysql' | 'postgres'
  /** 备份/还原使用的客户端工具方言：mysqldump / pg_dump + psql */
  backupDialect: 'mysql' | 'postgres'
  /** 设计器 DDL 方言：表设计器/库设计器生成语句时使用 */
  ddlDialect: 'mysql' | 'postgres'
  /** 一次 execute_sql 调用能否携带多条语句（MySQL 文本协议可以；PG 扩展协议只能单条） */
  multiStatementDdl: boolean
  /** 是否支持列位置 FIRST / AFTER（PG 不支持重排已有列） */
  supportsColumnPosition: boolean
  /** 列注释是否内联在列定义中（MySQL）；否则用 COMMENT ON */
  inlineColumnComments: boolean
  /** 设计器字段类型下拉选项 */
  dataTypes: readonly string[]
  /** 设计器索引类型下拉选项 */
  indexTypes: readonly string[]
  /** 是否支持 UNSIGNED 修饰（MySQL 整数类型） */
  supportsUnsigned: boolean
  /** 是否支持 ON UPDATE CURRENT_TIMESTAMP */
  supportsOnUpdateTimestamp: boolean
  /** 是否支持虚拟列（MySQL） */
  supportsVirtualColumns: boolean
  /** 自增列是否必须依赖某个索引/主键（MySQL 的 AUTO_INCREMENT 要求；PG 的 IDENTITY 不要求） */
  autoIncrementRequiresPrimaryKey: boolean
  /** 库设计器语义：database=数据库（MySQL）/ schema=模式（PG） */
  databaseDesignerMode: 'database' | 'schema'
  /** 库设计器是否展示 charset / collation（MySQL 专有概念） */
  supportsDatabaseCharset: boolean
  /** 全局搜索是否支持「全部数据库」范围（PG 连接固定搜索当前数据库） */
  supportsAllDatabaseSearch: boolean
}

/** 设计器字段类型下拉（顺序即展示顺序；MySQL 保持历史顺序） */
export const MYSQL_DATA_TYPES: readonly string[] = [
  'bigint', 'binary', 'bit', 'blob', 'boolean', 'char', 'date', 'datetime',
  'decimal', 'double', 'enum', 'float', 'int', 'json', 'longblob', 'longtext',
  'mediumblob', 'mediumint', 'mediumtext', 'set', 'smallint', 'text',
  'time', 'timestamp', 'tinyblob', 'tinyint', 'tinytext', 'varbinary',
  'varchar', 'year'
]

/** PostgreSQL 类型集（不含 MySQL 专有语义） */
export const POSTGRES_DATA_TYPES: readonly string[] = [
  'smallint', 'integer', 'bigint', 'smallserial', 'serial', 'bigserial',
  'numeric', 'real', 'double precision', 'boolean', 'char', 'varchar',
  'text', 'date', 'time', 'timestamp', 'timestamptz', 'interval',
  'uuid', 'json', 'jsonb', 'bytea'
]

export const MYSQL_INDEX_TYPES: readonly string[] = ['BTREE', 'HASH', 'FULLTEXT']
export const POSTGRES_INDEX_TYPES: readonly string[] = ['btree', 'hash', 'gin', 'gist', 'brin']

export const DRIVER_CAPABILITIES: Record<DriverKind, DriverCapabilities> = {
  mysql: {
    label: 'MySQL',
    prompt: 'mysql',
    identifierQuote: '`',
    defaultPort: 3306,
    editableTableData: true,
    zeroDateDefaults: true,
    nativeShowCreateTable: true,
    nativeUseStatement: true,
    formatDdl: false,
    requiresDatabase: true,
    tableQualification: 'database',
    proTools: {
      schemaDiff: true,
      monitor: true,
      backup: true,
      globalSearch: true,
      erDiagram: true,
    },
    monitorDialect: 'mysql',
    backupDialect: 'mysql',
    ddlDialect: 'mysql',
    multiStatementDdl: true,
    supportsColumnPosition: true,
    inlineColumnComments: true,
    dataTypes: MYSQL_DATA_TYPES,
    indexTypes: MYSQL_INDEX_TYPES,
    supportsUnsigned: true,
    supportsOnUpdateTimestamp: true,
    supportsVirtualColumns: true,
    autoIncrementRequiresPrimaryKey: true,
    databaseDesignerMode: 'database',
    supportsDatabaseCharset: true,
    supportsAllDatabaseSearch: true,
  },
  postgres: {
    label: 'PostgreSQL',
    prompt: 'postgres',
    identifierQuote: '"',
    defaultPort: 5432,
    // 后端 P1 已打通 PG 表数据写路径
    editableTableData: true,
    // PG 没有 MySQL 的零值日期（0000-00-00 非法）
    zeroDateDefaults: false,
    nativeShowCreateTable: false,
    nativeUseStatement: false,
    formatDdl: true,
    requiresDatabase: false,
    tableQualification: 'schema',
    proTools: {
      schemaDiff: true,
      monitor: true,
      backup: true,
      globalSearch: true,
      erDiagram: true,
    },
    monitorDialect: 'postgres',
    backupDialect: 'postgres',
    ddlDialect: 'postgres',
    // PG 的 execute 走 sqlx 扩展协议，一次只能一条语句，设计器保存时需前端拆分
    multiStatementDdl: false,
    supportsColumnPosition: false,
    inlineColumnComments: false,
    dataTypes: POSTGRES_DATA_TYPES,
    indexTypes: POSTGRES_INDEX_TYPES,
    supportsUnsigned: false,
    supportsOnUpdateTimestamp: false,
    supportsVirtualColumns: false,
    autoIncrementRequiresPrimaryKey: false,
    databaseDesignerMode: 'schema',
    supportsDatabaseCharset: false,
    supportsAllDatabaseSearch: false,
  },
}

/** 缺省驱动统一回落到 MySQL，避免旧数据没有 driver 字段时行为变化 */
export function normalizeDriver(driver: DriverKind | null | undefined): DriverKind {
  return driver ?? 'mysql'
}

/** 读取驱动能力；未知/缺省驱动按 MySQL 处理 */
export function capabilitiesOf(driver: DriverKind | null | undefined): DriverCapabilities {
  return DRIVER_CAPABILITIES[normalizeDriver(driver)]
}
