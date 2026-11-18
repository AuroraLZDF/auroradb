// 血缘图构建（V3-5.1）：把后端 analyze_sql_lineage 的语句级结果
// 去重合并成「节点 + 边」图结构，供 LineageDialog 渲染 SVG。
// 纯函数，不涉及组件与 invoke。

export interface LineageTableRef {
  database: string | null
  table: string
}

export interface LineageStatement {
  index: number
  operation: string
  target: LineageTableRef | null
  sources: LineageTableRef[]
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

export function buildLineageGraph(statements: LineageStatement[]): LineageGraph {
  const edges = new Map<string, LineageGraphEdge>()
  const nodeInfo = new Map<string, LineageTableRef>()
  const roles = new Map<string, Set<Role>>()

  const addRole = (ref: LineageTableRef, role: Role) => {
    const id = tableRefId(ref)
    if (!nodeInfo.has(id)) nodeInfo.set(id, ref)
    if (!roles.has(id)) roles.set(id, new Set())
    roles.get(id)!.add(role)
  }

  for (const statement of statements) {
    if (!statement.target) continue
    const targetId = tableRefId(statement.target)
    addRole(statement.target, 'target')
    for (const source of statement.sources) {
      const sourceId = tableRefId(source)
      // 同一表既是源又是目标（如 UPDATE 自身子查询）不画自环
      if (sourceId === targetId) continue
      addRole(source, 'source')
      const key = `${sourceId}->${targetId}`
      const edge = edges.get(key)
      if (edge) {
        if (!edge.operations.includes(statement.operation)) {
          edge.operations.push(statement.operation)
        }
        edge.statementIndexes.push(statement.index)
      } else {
        edges.set(key, {
          from: sourceId,
          to: targetId,
          operations: [statement.operation],
          statementIndexes: [statement.index],
        })
      }
    }
  }

  const nodes: LineageGraphNode[] = [...nodeInfo.entries()].map(([id, ref]) => {
    const roleSet = roles.get(id)!
    const role: LineageGraphNode['role'] =
      roleSet.has('source') && roleSet.has('target')
        ? 'both'
        : roleSet.has('source')
          ? 'source'
          : 'target'
    return { id, database: ref.database, table: ref.table, role }
  })

  // 布局顺序：源表 → 双重角色 → 目标表；同层按 id 字母序
  nodes.sort((a, b) => {
    const roleDiff = ROLE_ORDER[a.role] - ROLE_ORDER[b.role]
    return roleDiff !== 0 ? roleDiff : a.id.localeCompare(b.id)
  })

  return { nodes, edges: [...edges.values()] }
}
