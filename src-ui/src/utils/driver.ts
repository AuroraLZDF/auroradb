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
  /** Pro 工具支持情况（P1 仅 ER 图支持 PG） */
  proTools: {
    schemaDiff: boolean
    monitor: boolean
    backup: boolean
    globalSearch: boolean
    erDiagram: boolean
  }
}

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
      // 监控 / 备份 / 全局搜索目前仅实现 MySQL；PG 支持在 P2
      monitor: true,
      backup: true,
      globalSearch: true,
      erDiagram: true,
    },
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
      schemaDiff: false,
      monitor: false,
      backup: false,
      globalSearch: false,
      erDiagram: true,
    },
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
