import { describe, expect, it } from 'vitest'

import {
  buildColumnLineageGraph,
  buildLineageGraph,
  columnRefId,
  tableRefId,
  type ColumnLineage,
  type LineageColumnRef,
  type LineageStatement,
} from './lineageGraph'

function stmt(
  index: number,
  operation: string,
  target: string | null,
  sources: string[],
): LineageStatement {
  const toRef = (id: string) => {
    const [database, table] = id.split('.')
    return database && table
      ? { database, table }
      : { database: null, table: id }
  }
  return {
    index,
    operation,
    target: target ? toRef(target) : null,
    sources: sources.map(toRef),
    warnings: [],
  }
}

describe('tableRefId', () => {
  it('joins database and table', () => {
    expect(tableRefId({ database: 'mydb', table: 'users' })).toBe('mydb.users')
  })

  it('falls back to bare table name', () => {
    expect(tableRefId({ database: null, table: 'users' })).toBe('users')
  })
})

describe('buildLineageGraph', () => {
  it('builds edges from target statements', () => {
    const graph = buildLineageGraph([stmt(0, 'INSERT', 'mydb.logs', ['mydb.events'])])
    expect(graph.edges).toHaveLength(1)
    expect(graph.edges[0]!).toMatchObject({
      from: 'mydb.events',
      to: 'mydb.logs',
      operations: ['INSERT'],
      statementIndexes: [0],
    })
    expect(graph.nodes.map((n) => n.id)).toEqual(['mydb.events', 'mydb.logs'])
    expect(graph.nodes[0]!.role).toBe('source')
    expect(graph.nodes[1]!.role).toBe('target')
  })

  it('skips statements without target', () => {
    const graph = buildLineageGraph([stmt(0, 'SELECT', null, ['mydb.users'])])
    expect(graph.nodes).toHaveLength(0)
    expect(graph.edges).toHaveLength(0)
  })

  it('merges duplicate edges and collects operations', () => {
    const graph = buildLineageGraph([
      stmt(0, 'INSERT', 'mydb.t', ['mydb.a']),
      stmt(1, 'UPDATE', 'mydb.t', ['mydb.a']),
      stmt(2, 'INSERT', 'mydb.t', ['mydb.a']),
    ])
    expect(graph.edges).toHaveLength(1)
    expect(graph.edges[0]!.operations).toEqual(['INSERT', 'UPDATE'])
    expect(graph.edges[0]!.statementIndexes).toEqual([0, 1, 2])
  })

  it('marks nodes referenced both as source and target as both', () => {
    const graph = buildLineageGraph([
      stmt(0, 'UPDATE', 'mydb.users', ['mydb.orders']),
      stmt(1, 'INSERT', 'mydb.archive', ['mydb.users']),
    ])
    const users = graph.nodes.find((n) => n.id === 'mydb.users')
    expect(users?.role).toBe('both')
  })

  it('does not create self-loop edges', () => {
    const graph = buildLineageGraph([stmt(0, 'UPDATE', 'mydb.users', ['mydb.users'])])
    expect(graph.edges).toHaveLength(0)
    // 目标仍然成节点（自引用不产生连线，按目标角色展示）
    expect(graph.nodes.map((n) => n.id)).toEqual(['mydb.users'])
    expect(graph.nodes[0]!.role).toBe('target')
  })

  it('keeps cross-database same-table edges distinct', () => {
    const graph = buildLineageGraph([
      stmt(0, 'UPDATE', 'db2.users', ['db1.users']),
    ])
    expect(graph.edges).toHaveLength(1)
    expect(graph.edges[0]!.from).toBe('db1.users')
    expect(graph.edges[0]!.to).toBe('db2.users')
  })

  it('sorts nodes by role then id', () => {
    const graph = buildLineageGraph([
      stmt(0, 'INSERT', 'mydb.z_target', ['mydb.b_src', 'mydb.a_src']),
      stmt(1, 'UPDATE', 'mydb.z_target', ['mydb.z_target']),
    ])
    expect(graph.nodes.map((n) => `${n.role}:${n.id}`)).toEqual([
      'source:mydb.a_src',
      'source:mydb.b_src',
      'target:mydb.z_target',
    ])
  })
})

describe('columnRefId', () => {
  it('joins database, table and column', () => {
    const ref: LineageColumnRef = {
      table: { database: 'mydb', table: 'users' },
      column: 'name',
    }
    expect(columnRefId(ref)).toBe('mydb.users.name')
  })

  it('falls back to bare table name', () => {
    const ref: LineageColumnRef = {
      table: { database: null, table: 'users' },
      column: 'name',
    }
    expect(columnRefId(ref)).toBe('users.name')
  })
})

describe('buildColumnLineageGraph', () => {
  function col(
    tableId: string,
    column: string,
  ): LineageColumnRef {
    const [database, table] = tableId.split('.')
    return database && table
      ? { table: { database, table }, column }
      : { table: { database: null, table: tableId }, column }
  }

  function columnStmt(
    index: number,
    operation: string,
    flows: ColumnLineage[],
  ): LineageStatement {
    return { ...stmt(index, operation, null, []), columnLineage: flows }
  }

  it('builds edges from column lineage flows', () => {
    const graph = buildColumnLineageGraph([
      columnStmt(0, 'INSERT', [
        {
          target: col('mydb.logs', 'a'),
          sources: [col('mydb.events', 'x')],
        },
        {
          target: col('mydb.logs', 'b'),
          sources: [col('mydb.events', 'y')],
        },
      ]),
    ])
    expect(graph.edges).toHaveLength(2)
    expect(graph.edges[0]).toMatchObject({
      from: 'mydb.events.x',
      to: 'mydb.logs.a',
      operations: ['INSERT'],
      statementIndexes: [0],
    })
    expect(graph.nodes.map((n) => n.id).sort()).toEqual([
      'mydb.events.x',
      'mydb.events.y',
      'mydb.logs.a',
      'mydb.logs.b',
    ])
    const target = graph.nodes.find((n) => n.id === 'mydb.logs.a')
    expect(target?.role).toBe('target')
    expect(target?.column).toBe('a')
    expect(target?.table).toBe('logs')
  })

  it('merges duplicate column edges across statements', () => {
    const flow: ColumnLineage = {
      target: col('mydb.t', 'a'),
      sources: [col('mydb.s', 'x')],
    }
    const graph = buildColumnLineageGraph([
      columnStmt(0, 'INSERT', [flow]),
      columnStmt(1, 'UPDATE', [flow]),
    ])
    expect(graph.edges).toHaveLength(1)
    expect(graph.edges[0]!.operations).toEqual(['INSERT', 'UPDATE'])
    expect(graph.edges[0]!.statementIndexes).toEqual([0, 1])
  })

  it('marks columns read and written as both', () => {
    const graph = buildColumnLineageGraph([
      columnStmt(0, 'UPDATE', [
        {
          target: col('mydb.users', 'display_name'),
          sources: [col('mydb.users', 'name')],
        },
      ]),
      columnStmt(1, 'UPDATE', [
        {
          target: col('mydb.users', 'name'),
          sources: [col('mydb.users', 'first_name')],
        },
      ]),
    ])
    // name 既被读（display_name 的来源）也被写（first_name 的去向）
    const name = graph.nodes.find((n) => n.id === 'mydb.users.name')
    expect(name?.role).toBe('both')
  })

  it('ignores statements without column lineage', () => {
    const graph = buildColumnLineageGraph([
      stmt(0, 'INSERT', 'mydb.t', ['mydb.s']),
      { ...stmt(1, 'UPDATE', 'mydb.t', ['mydb.s']), columnLineage: [] },
    ])
    expect(graph.nodes).toHaveLength(0)
    expect(graph.edges).toHaveLength(0)
  })

  it('keeps same-table cross-column edges distinct', () => {
    const graph = buildColumnLineageGraph([
      columnStmt(0, 'UPDATE', [
        {
          target: col('mydb.users', 'a'),
          sources: [col('mydb.users', 'a')],
        },
        {
          target: col('mydb.users', 'b'),
          sources: [col('mydb.users', 'a')],
        },
      ]),
    ])
    // 自引用（a <- a）不成边；a <- b 正常成边
    expect(graph.edges).toHaveLength(1)
    expect(graph.edges[0]!.from).toBe('mydb.users.a')
    expect(graph.edges[0]!.to).toBe('mydb.users.b')
    expect(graph.nodes[0]!.role).toBe('both')
  })
})
