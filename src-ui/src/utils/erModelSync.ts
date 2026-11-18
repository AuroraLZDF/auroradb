import type { DriverKind } from './driver'
import {
  buildAlterTableStatements,
  buildCreateTableStatements,
  buildFieldDefinition,
  normalizeColumnType,
  type DdlField
} from './ddl'
import { quoteIdentifier } from './sql'
import { orderedTables, tableToDdlInput } from './erModelDdl'
import type { ErModel, ModelTable } from '@/stores/erModel'

/**
 * 模型 ↔ 库比对同步（V3-3.3）。
 *
 * 目标库快照来自 get_er_diagram_data（反向 ER 同一数据源，含表/列/外键），
 * 与模型结构在前端做 diff，差异语句复用表设计器的 DDL 生成链路：
 * - 新表 → buildCreateTableStatements（与 V3-3.2 正向 DDL 同一条链路）
 * - 列差异 → buildAlterTableStatements（current=模型值，original=库值，按方言生成 ALTER）
 * - 缺失外键 → ALTER TABLE ADD CONSTRAINT
 * - 库多出的表/列 → 可选 DROP（默认不勾选，由调用方控制）
 *
 * 纯函数，方便单测；执行走 apply_schema_diff（逐条应用、遇错即停）。
 */

/** get_er_diagram_data 快照中的列（与 ErDiagram.vue 的 ErColumn 对应） */
export interface DbColumnSnapshot {
  name: string
  data_type: string
  column_type?: string
  is_primary: boolean
  nullable?: boolean
  default_value?: string | null
  comment?: string
  is_unique?: boolean
}

/** get_er_diagram_data 快照中的外键 */
export interface DbForeignKeySnapshot {
  name: string
  table: string
  column: string
  ref_table: string
  ref_column: string
}

export interface DbTableSnapshot {
  name: string
  columns: DbColumnSnapshot[]
}

/** 目标库结构快照（get_er_diagram_data 的返回子集） */
export interface DbSnapshot {
  driver: DriverKind
  database: string
  tables: DbTableSnapshot[]
  foreign_keys: DbForeignKeySnapshot[]
}

export interface ModelSyncOptions {
  /** 库中存在但模型没有的表：生成 DROP TABLE（默认不删） */
  dropMissingTables?: boolean
  /** 两边都有但模型没有的列：生成 DROP COLUMN（默认不删） */
  dropMissingColumns?: boolean
}

export type ModelSyncItemKind =
  | 'create-table'
  | 'drop-table'
  | 'add-column'
  | 'alter-column'
  | 'drop-column'
  | 'add-fk'

export interface ModelSyncItem {
  kind: ModelSyncItemKind
  tableName: string
  columnName?: string
  statements: string[]
}

export interface ModelSyncPlan {
  items: ModelSyncItem[]
}

/** 类型表达式归一：小写 + 去空白，用于比较（decimal(10, 2) vs decimal(10,2)） */
function normalizeTypeExpression(type: string): string {
  return (type ?? '').replace(/\s+/g, '').toLowerCase()
}

/** 库侧类型表达式：PG 用 normalizeColumnType 还原 format_type 输出，MySQL 直接用完整类型 */
function dbTypeExpression(col: DbColumnSnapshot, driver: DriverKind): string {
  const raw = (col.column_type ?? col.data_type ?? '').trim()
  if (raw === '') return ''
  const normalized = normalizeColumnType(raw, driver)
  let expr = normalized.type
  if (normalized.length !== undefined) {
    expr += `(${normalized.length}${normalized.decimals !== undefined ? ',' + normalized.decimals : ''})`
  }
  return expr
}

/** 默认值归一比较：空串/null/undefined 视为相同，其余 trim 后精确比较 */
function sameDefault(a: string | null | undefined, b: string | null | undefined): boolean {
  return (a ?? '').trim() === (b ?? '').trim()
}

/** 列在类型/非空/默认值/注释上是否有差异（主键/唯一不参与比对，避免破坏性主键重建） */
function columnDiffers(
  col: { nullable: boolean; default?: string; comment?: string },
  dbCol: DbColumnSnapshot,
  modelTypeExpr: string,
  dbTypeExpr: string
): string[] {
  const diffs: string[] = []
  if (normalizeTypeExpression(modelTypeExpr) !== normalizeTypeExpression(dbTypeExpr)) {
    diffs.push(`${modelTypeExpr} → ${dbTypeExpr || '?'}`)
  }
  const dbNullable = dbCol.nullable ?? true
  if (col.nullable !== dbNullable) {
    diffs.push(col.nullable ? 'NULL' : 'NOT NULL')
  }
  if (!sameDefault(col.default, dbCol.default_value)) {
    diffs.push(`DEFAULT ${col.default ?? 'NULL'}`)
  }
  if ((col.comment ?? '').trim() !== (dbCol.comment ?? '').trim()) {
    diffs.push('COMMENT')
  }
  return diffs
}

/** 模型列 → DdlField：type 带完整精度，方言差异由生成器处理 */
function modelColumnToField(col: {
  name: string
  type: string
  nullable: boolean
  default?: string
  comment?: string
}): DdlField {
  return {
    name: col.name,
    type: col.type,
    notNull: !col.nullable,
    defaultValue: col.default,
    comment: col.comment,
    isPrimary: false,
    isAutoIncrement: false,
  }
}

/** 建表语句（含索引与表注释，语句不带分号，交给 apply_schema_diff 逐条执行） */
function createTableStatements(model: ErModel, table: ModelTable, driver: DriverKind): string[] {
  const statements = buildCreateTableStatements(tableToDdlInput(model, table), driver)
  if (table.comment) {
    statements.push(driver === 'postgres'
      ? `COMMENT ON TABLE ${quoteIdentifier(table.name, driver)} IS '${table.comment.replace(/'/g, "''")}'`
      : `ALTER TABLE ${quoteIdentifier(table.name, driver)} COMMENT = '${table.comment.replace(/'/g, "''")}'`)
  }
  return statements
}

/**
 * 计算模型与目标库的差异同步计划。
 * 语句顺序：新表（拓扑序）→ 已有表的列差异 → 缺失外键 → 可选的 DROP。
 */
export function buildModelSyncPlan(
  model: ErModel,
  snapshot: DbSnapshot,
  options: ModelSyncOptions = {}
): ModelSyncPlan {
  const driver = snapshot.driver
  const items: ModelSyncItem[] = []
  const dbTables = new Map(snapshot.tables.map((t) => [t.name, t]))
  const dbFkSet = new Set(
    snapshot.foreign_keys.map((fk) =>
      [fk.table, fk.column, fk.ref_table, fk.ref_column].map((v) => v.toLowerCase()).join('\u0000')
    )
  )

  // ===== 新表（拓扑序，保证被引用表先建）=====
  const missingTables = model.tables.filter((t) => !dbTables.has(t.name))
  if (missingTables.length > 0) {
    const subModel: ErModel = { ...model, tables: missingTables }
    for (const table of orderedTables(subModel)) {
      items.push({
        kind: 'create-table',
        tableName: table.name,
        statements: createTableStatements(subModel, table, driver),
      })
    }
  }

  // ===== 两边都有表的列差异 =====
  const fkCounter = new Map<string, number>()
  const plannedFkNames = new Set(snapshot.foreign_keys.map((fk) => fk.name))
  for (const table of model.tables) {
    const dbTable = dbTables.get(table.name)
    if (!dbTable) continue
    const dbCols = new Map(dbTable.columns.map((c) => [c.name.toLowerCase(), c]))

    for (const col of table.columns) {
      const dbCol = dbCols.get(col.name.toLowerCase())
      if (!dbCol) {
        const field = modelColumnToField(col)
        const statements = [
          `ALTER TABLE ${quoteIdentifier(table.name, driver)} ADD COLUMN ${buildFieldDefinition(field, driver, false)}`,
        ]
        if (driver === 'postgres' && col.comment) {
          statements.push(
            `COMMENT ON COLUMN ${quoteIdentifier(table.name, driver)}.${quoteIdentifier(col.name, driver)} IS '${col.comment.replace(/'/g, "''")}'`
          )
        }
        items.push({ kind: 'add-column', tableName: table.name, columnName: col.name, statements })
        continue
      }
      const modelType = col.type
      const dbType = dbTypeExpression(dbCol, driver)
      const diffs = columnDiffers(col, dbCol, modelType, dbType)
      if (diffs.length === 0) continue
      // current=模型值，original=库值：只有有差异的维度会生成语句
      const field: DdlField = {
        ...modelColumnToField(col),
        isModified: true,
        originalName: dbCol.name,
        originalTypeSignature: dbType,
        originalNotNull: !(dbCol.nullable ?? true),
        originalDefaultValue: dbCol.default_value ?? '',
        originalComment: dbCol.comment ?? '',
      }
      const statements = buildAlterTableStatements(
        { tableName: table.name, fields: [field], indexes: [], foreignKeys: [] },
        driver
      )
      items.push({ kind: 'alter-column', tableName: table.name, columnName: col.name, statements })
    }

    // 模型有、库没有的列 → 可选 DROP
    if (options.dropMissingColumns) {
      const modelColNames = new Set(table.columns.map((c) => c.name.toLowerCase()))
      for (const dbCol of dbTable.columns) {
        if (!modelColNames.has(dbCol.name.toLowerCase())) {
          items.push({
            kind: 'drop-column',
            tableName: table.name,
            columnName: dbCol.name,
            statements: [
              `ALTER TABLE ${quoteIdentifier(table.name, driver)} DROP COLUMN ${quoteIdentifier(dbCol.name, driver)}`,
            ],
          })
        }
      }
    }
  }

  // ===== 缺失外键：模型关系在库中不存在（两端表都在库里）=====
  for (const rel of model.relations) {
    const fromTable = model.tables.find((t) => t.id === rel.fromTableId)
    const toTable = model.tables.find((t) => t.id === rel.toTableId)
    const fromCol = fromTable?.columns.find((c) => c.id === rel.fromColumnId)
    const toCol = toTable?.columns.find((c) => c.id === rel.toColumnId)
    if (!fromTable || !toTable || !fromCol || !toCol) continue
    // 任一端表是本次要新建的 → 外键已随 CREATE TABLE 内联，跳过
    if (missingTables.some((t) => t.id === rel.fromTableId || t.id === rel.toTableId)) continue
    if (!dbTables.has(fromTable.name) || !dbTables.has(toTable.name)) continue
    const sig = [fromTable.name, fromCol.name, toTable.name, toCol.name]
      .map((v) => v.toLowerCase()).join('\u0000')
    if (dbFkSet.has(sig)) continue

    // 约束名与 V3-3.2 正向 DDL 同规则，并避开库里已有的同名约束
    const base = `${fromTable.name}_${toTable.name}_fk`
    let seq = fkCounter.get(base) ?? 0
    let name = ''
    do {
      seq += 1
      name = seq === 1 ? base : `${base}_${seq}`
    } while (plannedFkNames.has(name))
    fkCounter.set(base, seq)
    plannedFkNames.add(name)

    items.push({
      kind: 'add-fk',
      tableName: fromTable.name,
      columnName: fromCol.name,
      statements: [
        `ALTER TABLE ${quoteIdentifier(fromTable.name, driver)} ADD CONSTRAINT ${quoteIdentifier(name, driver)} ` +
          `FOREIGN KEY (${quoteIdentifier(fromCol.name, driver)}) REFERENCES ${quoteIdentifier(toTable.name, driver)}(${quoteIdentifier(toCol.name, driver)}) ` +
          `ON DELETE NO ACTION ON UPDATE NO ACTION`,
      ],
    })
  }

  // ===== 库多出的表 → 可选 DROP（放最后，避免影响前面的语句）=====
  if (options.dropMissingTables) {
    const modelTableNames = new Set(model.tables.map((t) => t.name.toLowerCase()))
    for (const dbTable of snapshot.tables) {
      if (!modelTableNames.has(dbTable.name.toLowerCase())) {
        items.push({
          kind: 'drop-table',
          tableName: dbTable.name,
          statements: [`DROP TABLE IF EXISTS ${quoteIdentifier(dbTable.name, driver)}`],
        })
      }
    }
  }

  return { items }
}

/** 汇总选中项的语句列表（按计划顺序、去重相邻空语句） */
export function collectStatements(items: ModelSyncItem[]): string[] {
  return items.flatMap((item) => item.statements).filter((s) => s.trim() !== '')
}
