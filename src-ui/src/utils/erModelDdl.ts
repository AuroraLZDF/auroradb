import type { DriverKind } from './driver'
import { buildCreateTableSql, type DdlField, type DdlForeignKey, type DdlIndex, type TableDdlInput } from './ddl'
import { quoteIdentifier } from './sql'
import type { ErModel, ModelRelation, ModelTable } from '@/stores/erModel'

/**
 * ER 模型 → 正向 DDL（V3-3.2）。
 *
 * 把模型的表/字段/关系转换成 ddl.ts 的 TableDdlInput，复用表设计器同一条
 * CREATE TABLE 生成链路（MySQL 内联约束、PG 用 CONSTRAINT 形式 + COMMENT ON）。
 * 纯函数，方便单测覆盖；组件只负责方言选择与预览/复制。
 */

export type ModelDialect = 'mysql' | 'postgres'

/** 关系两端在模型里可能已被删除，生成前过滤掉悬空关系 */
function liveRelations(model: ErModel): ModelRelation[] {
  const columnsOf = (tableId: string) =>
    model.tables.find((t) => t.id === tableId)?.columns
  return model.relations.filter((r) => {
    const fromCols = columnsOf(r.fromTableId)
    const toCols = columnsOf(r.toTableId)
    return !!fromCols?.some((c) => c.id === r.fromColumnId) &&
      !!toCols?.some((c) => c.id === r.toColumnId)
  })
}

/** 单表 → 表设计器的 DDL 输入结构（方言差异由 buildCreateTableSql 按驱动处理） */
function tableToDdlInput(model: ErModel, table: ModelTable): TableDdlInput {
  const relations = liveRelations(model)

  const fields: DdlField[] = table.columns.map((col) => ({
    name: col.name,
    type: col.type,
    notNull: !col.nullable,
    defaultValue: col.default,
    comment: col.comment,
    isPrimary: col.pk,
    isUnique: false, // 唯一约束统一走索引，MySQL 侧才能生成 UNIQUE INDEX
    isAutoIncrement: false,
  }))

  // 非主键的唯一字段 → UNIQUE INDEX（两种方言通用）
  const indexes: DdlIndex[] = table.columns
    .filter((col) => col.unique && !col.pk)
    .map((col) => ({
      name: `${table.name}_${col.name}_uk`,
      columns: [col.name],
      type: 'BTREE',
      isUnique: true,
    }))

  // 指向本表的关系 → 外键；同一对表之间的多条关系用序号区分约束名
  const fkCounter = new Map<string, number>()
  const foreignKeys: DdlForeignKey[] = relations
    .filter((rel) => rel.fromTableId === table.id)
    .map((rel) => {
      const fromTable = model.tables.find((t) => t.id === rel.fromTableId)!
      const toTable = model.tables.find((t) => t.id === rel.toTableId)!
      const fromCol = fromTable.columns.find((c) => c.id === rel.fromColumnId)!
      const toCol = toTable.columns.find((c) => c.id === rel.toColumnId)!
      const base = `${fromTable.name}_${toTable.name}_fk`
      const seq = (fkCounter.get(base) ?? 0) + 1
      fkCounter.set(base, seq)
      return {
        name: seq === 1 ? base : `${base}_${seq}`,
        column: fromCol.name,
        refTable: toTable.name,
        refColumn: toCol.name,
        // 模型不表达级联策略，用两种方言语义一致的最保守值
        onDelete: 'NO ACTION',
        onUpdate: 'NO ACTION',
      }
    })

  return {
    tableName: table.name,
    fields,
    indexes,
    foreignKeys,
  }
}

/**
 * 生成建表语句顺序：被引用表在前（拓扑序），否则带外键的 CREATE 在执行时会失败。
 * 环形引用无法拓扑化，兜底按模型原顺序输出。
 */
export function orderedTables(model: ErModel): ModelTable[] {
  const relations = liveRelations(model)
  // 依赖边：from 表依赖 to 表（to 先建）
  const deps = new Map<string, Set<string>>() // tableId -> 依赖的 tableId 集合
  for (const table of model.tables) deps.set(table.id, new Set())
  for (const rel of relations) {
    if (rel.fromTableId !== rel.toTableId) {
      deps.get(rel.fromTableId)?.add(rel.toTableId)
    }
  }

  const ordered: ModelTable[] = []
  const done = new Set<string>()
  let remaining = [...model.tables]
  // Kahn 变体：每轮挑出依赖全部就绪的表；无进展（环）时按原序兜底
  while (remaining.length > 0) {
    const ready = remaining.filter((t) =>
      [...deps.get(t.id)!].every((dep) => done.has(dep))
    )
    if (ready.length === 0) {
      ordered.push(...remaining)
      break
    }
    ordered.push(...ready)
    ready.forEach((t) => done.add(t.id))
    remaining = remaining.filter((t) => !done.has(t.id))
  }
  return ordered
}

/** 模型 → 完整 DDL 文本（每张表一段 CREATE，多语句以空行分隔） */
export function buildModelDdl(model: ErModel, dialect: ModelDialect): string {
  const driver = dialect as DriverKind
  const statements: string[] = []

  for (const table of orderedTables(model)) {
    statements.push(buildCreateTableSql(tableToDdlInput(model, table), driver))
    // 表注释：MySQL 用 ALTER TABLE COMMENT，PG 用 COMMENT ON TABLE
    if (table.comment) {
      statements.push(dialect === 'postgres'
        ? `COMMENT ON TABLE ${quoteIdentifier(table.name, driver)} IS '${escapeSqlString(table.comment)}';`
        : `ALTER TABLE ${quoteIdentifier(table.name, driver)} COMMENT = '${escapeSqlString(table.comment)}';`)
    }
  }

  return statements.join('\n\n')
}

function escapeSqlString(value: string): string {
  return value.replace(/'/g, "''")
}
