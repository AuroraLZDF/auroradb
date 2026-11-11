import type { DriverKind } from '@/stores/savedConnections'
/** 执行计划节点，与后端 `explain_sql` 返回结构一致 */
export interface ExplainNode {
  operation: string
  table?: string
  index?: string
  rows?: number
  cost?: number
  filtered?: number
  extra: string[]
  children: ExplainNode[]
}

export interface ExplainPlan {
  driver: DriverKind
  sql: string
  nodes: ExplainNode[]
  warnings: string[]
  raw_json: string
  execution_time_ms: number
}
