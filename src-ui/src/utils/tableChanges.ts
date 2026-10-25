import { quoteIdentifier } from './sql'
import type { TableChange } from '@/stores/dataBrowser'

/** 生成 WHERE 子句与 SQL 字面量只需要列名与主键标记 */
interface ColumnMeta {
  name: string
  is_primary: boolean
}

/** 单元格编辑：oldRowData 为修改前的整行，用于定位被修改的行 */
interface CellEdit {
  rowIndex: number
  colIndex: number
  oldRowData?: unknown[] | null
  newValue: unknown
}

/**
 * 将 JS 值转换为 SQL 字面量。
 * MySQL 默认开启反斜杠转义，因此字符串需同时转义 `\` 与 `'`。
 */
export function formatSqlValue(value: unknown): string {
  if (value === null || value === undefined) return 'NULL'
  if (typeof value === 'string') {
    return `'${value.replace(/\\/g, '\\\\').replace(/'/g, "''")}'`
  }
  if (typeof value === 'boolean') return value ? '1' : '0'
  return String(value)
}

function formatComparison(colName: string, value: unknown): string {
  const quoted = quoteIdentifier(colName)
  if (value === null || value === undefined) return `${quoted} IS NULL`
  return `${quoted} = ${formatSqlValue(value)}`
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
export function buildWhereClause(columns: ColumnMeta[], rowData: unknown[]): string {
  const primaryKeys = columns.filter(col => col.is_primary)
  if (primaryKeys.length === 0) return ''

  return primaryKeys
    .map(pk => {
      const colIndex = columns.findIndex(c => c.name === pk.name)
      const pkValue = colIndex >= 0 ? rowData[colIndex] : undefined
      return formatComparison(pk.name, pkValue)
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
}): TableChange[] {
  const { table, columns, edits, inserts, deletes } = params
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
    const whereClause = buildWhereClause(columns, entry.oldRow)
    if (!whereClause) continue
    const assignments = Array.from(entry.values.entries())
      .filter(([colIndex, value]) => {
        const column = columns[colIndex]
        if (!column) return false
        // 与原始行等值的改动不写入 SET，避免产生 `SET col = 原值` 的空更新
        return !isSameValue(entry.oldRow[colIndex], value)
      })
      .map(([colIndex, value]) => `${quoteIdentifier(columns[colIndex]!.name)} = ${formatSqlValue(value)}`)
      .join(', ')
    if (!assignments) continue
    changes.push({
      kind: 'update',
      sql: `UPDATE ${quoteIdentifier(table)} SET ${assignments} WHERE ${whereClause}`,
      table,
      where_clause: whereClause
    })
  }

  const columnList = columns.map(c => quoteIdentifier(c.name)).join(', ')
  for (const row of inserts) {
    const values = row.map(v => formatSqlValue(v)).join(', ')
    changes.push({
      kind: 'insert',
      sql: `INSERT INTO ${quoteIdentifier(table)} (${columnList}) VALUES (${values})`,
      table
    })
  }

  for (const rowData of deletes) {
    const whereClause = buildWhereClause(columns, rowData)
    if (!whereClause) continue
    changes.push({
      kind: 'delete',
      sql: `DELETE FROM ${quoteIdentifier(table)} WHERE ${whereClause}`,
      table,
      where_clause: whereClause
    })
  }

  return changes
}
