// 血缘图构建（V3-5.1 表级 / V3-5.2 列级）：把后端 analyze_sql_lineage 的
// 语句级结果去重合并成「节点 + 边」图结构，供 LineageDialog 渲染 SVG。
// 纯函数，不涉及组件与 invoke。

export interface LineageTableRef {
  database: string | null
  table: string
}

export interface LineageColumnRef {
  table: LineageTableRef
  column: string
}

export interface ColumnLineage {
  target: LineageColumnRef
  sources: LineageColumnRef[]
}

export interface LineageStatement {
  index: number
  operation: string
  target: LineageTableRef | null
  sources: LineageTableRef[]
  /** 列级血缘（V3-5.2）；旧版后端无此字段 */
  columnLineage?: ColumnLineage[]
  warnings: string[]
}

export interface LineageAnalysis {
  statements: LineageStatement[]
  unparsedCount: number
}

export interface LineageGraphNode {
  id: string
  database: string | null
  table: string
  /** 列级节点携带列名；表级节点无 */
  column?: string
  role: 'source' | 'target' | 'both'
}

export interface LineageGraphEdge {
  from: string
  to: string
  operations: string[]
  statementIndexes: number[]
}

export interface LineageGraph {
  nodes: LineageGraphNode[]
  edges: LineageGraphEdge[]
}

type Role = 'source' | 'target'

const ROLE_ORDER: Record<LineageGraphNode['role'], number> = {
  source: 0,
  both: 1,
  target: 2,
}

/** 节点唯一标识：有库名用 `db.table`，否则裸表名 */
export function tableRefId(ref: LineageTableRef): string {
  return ref.database ? `${ref.database}.${ref.table}` : ref.table
}

/** 列节点唯一标识：`db.table.column`（无库名时 `table.column`） */
export function columnRefId(ref: LineageColumnRef): string {
  return `${tableRefId(ref.table)}.${ref.column}`
}

/** 图合并用的扁平依赖流 */
interface GraphFlow {
  from: LineageTableRef
  to: LineageTableRef
  fromColumn?: string
  toColumn?: string
  operation: string
  index: number
}

function buildGraphFromFlows(flows: GraphFlow[]): LineageGraph {
  const edges = new Map<string, LineageGraphEdge>()
  const nodeInfo = new Map<string, { ref: LineageTableRef; column?: string }>()
  const roles = new Map<string, Set<Role>>()

  const nodeId = (ref: LineageTableRef, column?: string) =>
    column !== undefined ? `${tableRefId(ref)}.${column}` : tableRefId(ref)

  const addRole = (id: string, ref: LineageTableRef, role: Role, column?: string) => {
    if (!nodeInfo.has(id)) nodeInfo.set(id, { ref, column })
    if (!roles.has(id)) roles.set(id, new Set())
    roles.get(id)!.add(role)
  }

  for (const flow of flows) {
    const toId = nodeId(flow.to, flow.toColumn)
    const fromId = nodeId(flow.from, flow.fromColumn)
    addRole(toId, flow.to, 'target', flow.toColumn)
    // 同一节点既是源又是目标（如 UPDATE 自身子查询）不画自环
    if (fromId === toId) continue
    addRole(fromId, flow.from, 'source', flow.fromColumn)
    const key = `${fromId}->${toId}`
    const edge = edges.get(key)
    if (edge) {
      if (!edge.operations.includes(flow.operation)) {
        edge.operations.push(flow.operation)
      }
      edge.statementIndexes.push(flow.index)
    } else {
      edges.set(key, {
        from: fromId,
        to: toId,
        operations: [flow.operation],
        statementIndexes: [flow.index],
      })
    }
  }

  const nodes: LineageGraphNode[] = [...nodeInfo.entries()].map(([id, info]) => {
    const roleSet = roles.get(id)!
    const role: LineageGraphNode['role'] =
      roleSet.has('source') && roleSet.has('target')
        ? 'both'
        : roleSet.has('source')
          ? 'source'
          : 'target'
    return {
      id,
      database: info.ref.database,
      table: info.ref.table,
      ...(info.column !== undefined ? { column: info.column } : {}),
      role,
    }
  })

  // 布局顺序：源 → 双重角色 → 目标；同层按 id 字母序
  nodes.sort((a, b) => {
    const roleDiff = ROLE_ORDER[a.role] - ROLE_ORDER[b.role]
    return roleDiff !== 0 ? roleDiff : a.id.localeCompare(b.id)
  })

  return { nodes, edges: [...edges.values()] }
}

export function buildLineageGraph(statements: LineageStatement[]): LineageGraph {
  const flows: GraphFlow[] = []
  for (const statement of statements) {
    if (!statement.target) continue
    for (const source of statement.sources) {
      flows.push({
        from: source,
        to: statement.target,
        operation: statement.operation,
        index: statement.index,
      })
    }
  }
  return buildGraphFromFlows(flows)
}

/** 列级血缘图：节点粒度为 `db.table.column` */
export function buildColumnLineageGraph(statements: LineageStatement[]): LineageGraph {
  const flows: GraphFlow[] = []
  for (const statement of statements) {
    for (const flow of statement.columnLineage ?? []) {
      for (const source of flow.sources) {
        flows.push({
          from: source.table,
          to: flow.target.table,
          fromColumn: source.column,
          toColumn: flow.target.column,
          operation: statement.operation,
          index: statement.index,
        })
      }
    }
  }
  return buildGraphFromFlows(flows)
}
