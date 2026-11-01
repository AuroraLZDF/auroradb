import { quoteIdentifier } from './sql'
import type { DriverKind } from '@/stores/savedConnections'
import type { TableChange } from '@/stores/dataBrowser'

/** 生成 WHERE 子句与 SQL 字面量只需要列名、主键标记与列类型 */
interface ColumnMeta {
  name: string
  is_primary: boolean
  /** 列类型：二进制列的值是 Base64 文本，需还原成字节字面量 */
  data_type?: string
}

/** 单元格编辑：oldRowData 为修改前的整行，用于定位被修改的行 */
interface CellEdit {
  rowIndex: number
  colIndex: number
  oldRowData?: unknown[] | null
  newValue: unknown
}

interface SqlValueOptions {
  driver?: DriverKind
  data_type?: string
}

const BINARY_TYPE_KEYWORDS = ['binary', 'blob', 'bytea']

/** 是否为二进制列（后端把这些列编码成 Base64 字符串返回） */
export function isBinaryType(dataType?: string): boolean {
  if (!dataType) return false
  const lower = dataType.toLowerCase()
  return BINARY_TYPE_KEYWORDS.some(keyword => lower.includes(keyword))
}

function formatStringLiteral(value: string, driver: DriverKind): string {
  if (driver === 'postgres') {
    // PG 默认 standard_conforming_strings=on：反斜杠不是转义符，只需翻倍单引号
    // NUL 字符无法出现在 PG 文本中，写入前直接丢弃（不用正则，避免 lint 的控制字符告警）
    return `'${value.replaceAll('\u0000', '').replace(/'/g, "''")}'`
  }
  return `'${value.replace(/\\/g, '\\\\').replace(/'/g, "''")}'`
}

/** Base64 文本还原为字节字面量，避免把 Base64 文本本身写回二进制列 */
function formatBinaryLiteral(base64: string, driver: DriverKind): string {
  const escaped = base64.replace(/\\/g, '\\\\').replace(/'/g, "''")
  if (driver === 'postgres') {
    return `decode('${escaped}', 'base64')`
  }
  return `FROM_BASE64('${escaped}')`
}

/**
 * 将 JS 值转换为 SQL 字面量。
 * MySQL 默认开启反斜杠转义，因此字符串需同时转义 `\` 与 `'`；
 * PG 只翻倍单引号。二进制列的值是 Base64，需按方言还原为字节字面量。
 */
export function formatSqlValue(value: unknown, options: SqlValueOptions = {}): string {
  const driver = options.driver ?? 'mysql'
  if (value === null || value === undefined) return 'NULL'
  if (typeof value === 'string') {
    if (isBinaryType(options.data_type)) return formatBinaryLiteral(value, driver)
    return formatStringLiteral(value, driver)
  }
  if (typeof value === 'boolean') return value ? '1' : '0'
  return String(value)
}

function formatComparison(
  column: ColumnMeta,
  value: unknown,
  driver: DriverKind
): string {
  const quoted = quoteIdentifier(column.name, driver)
  if (value === null || value === undefined) return `${quoted} IS NULL`
  return `${quoted} = ${formatSqlValue(value, { driver, data_type: column.data_type })}`
}

/** 值判等：行数据可能是数字/布尔，输入框给的是字符串，按字符串归一化比较 */
export function isSameValue(a: unknown, b: unknown): boolean {
  if (a === b) return true
  if (a === null || a === undefined || b === null || b === undefined) return false
  return String(a) === String(b)
}

/**
 * 构建 WHERE 子句（用于 UPDATE 和 DELETE）。
 * 只用主键定位：无主键时返回空串，调用方据此禁止编辑。
 */
export function buildWhereClause(
  columns: ColumnMeta[],
  rowData: unknown[],
  driver: DriverKind = 'mysql'
): string {
  const primaryKeys = columns.filter(col => col.is_primary)
  if (primaryKeys.length === 0) return ''

  return primaryKeys
    .map(pk => {
      const colIndex = columns.findIndex(c => c.name === pk.name)
      const pkValue = colIndex >= 0 ? rowData[colIndex] : undefined
      return formatComparison(pk, pkValue, driver)
    })
    .join(' AND ')
}

/**
 * 汇总待提交的表数据变更，SQL 由前端生成、后端在同一事务中执行。
 * - 同一行的多个单元格编辑合并为一条 UPDATE
 * - 无主键的行无法定位，跳过其 UPDATE / DELETE
 */
export function collectTableChanges(params: {
  table: string
  columns: ColumnMeta[]
  edits: Iterable<CellEdit>
  inserts: unknown[][]
  deletes: unknown[][]
  driver?: DriverKind
}): TableChange[] {
  const { table, columns, edits, inserts, deletes } = params
  const driver = params.driver ?? 'mysql'
  const changes: TableChange[] = []

  const rowUpdates = new Map<number, { oldRow: unknown[]; values: Map<number, unknown> }>()
  for (const cell of edits) {
    if (!cell.oldRowData) continue
    let entry = rowUpdates.get(cell.rowIndex)
    if (!entry) {
      entry = { oldRow: cell.oldRowData, values: new Map() }
      rowUpdates.set(cell.rowIndex, entry)
    }
    entry.values.set(cell.colIndex, cell.newValue)
  }
  for (const entry of rowUpdates.values()) {
    const whereClause = buildWhereClause(columns, entry.oldRow, driver)
    if (!whereClause) continue
    const assignments = Array.from(entry.values.entries())
      .filter(([colIndex, value]) => {
        const column = columns[colIndex]
        if (!column) return false
        // 与原始行等值的改动不写入 SET，避免产生 `SET col = 原值` 的空更新
        return !isSameValue(entry.oldRow[colIndex], value)
      })
      .map(([colIndex, value]) => {
        const column = columns[colIndex]!
        const literal = formatSqlValue(value, { driver, data_type: column.data_type })
        return `${quoteIdentifier(column.name, driver)} = ${literal}`
      })
      .join(', ')
    if (!assignments) continue
    changes.push({
      kind: 'update',
      sql: `UPDATE ${quoteIdentifier(table, driver)} SET ${assignments} WHERE ${whereClause}`,
      table,
      where_clause: whereClause
    })
  }

  const columnList = columns.map(c => quoteIdentifier(c.name, driver)).join(', ')
  for (const row of inserts) {
    const values = row
      .map((value, index) =>
        formatSqlValue(value, { driver, data_type: columns[index]?.data_type })
      )
      .join(', ')
    changes.push({
      kind: 'insert',
      sql: `INSERT INTO ${quoteIdentifier(table, driver)} (${columnList}) VALUES (${values})`,
      table
    })
  }

  for (const rowData of deletes) {
    const whereClause = buildWhereClause(columns, rowData, driver)
    if (!whereClause) continue
    changes.push({
      kind: 'delete',
      sql: `DELETE FROM ${quoteIdentifier(table, driver)} WHERE ${whereClause}`,
      table,
      where_clause: whereClause
    })
  }

  return changes
}
