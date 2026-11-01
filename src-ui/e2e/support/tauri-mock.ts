import type { Page } from '@playwright/test'

/**
 * E2E 用的 Tauri IPC 假实现。
 *
 * 真实应用里所有数据都来自 Tauri 后端（`invoke`），浏览器里没有 Rust 侧实现。
 * 这里在页面加载前注入一个最小可用的 `window.__TAURI_INTERNALS__`，
 * 让前端代码无需任何改动就能跑在 Vite dev server 上：
 * - 后端命令按「内存表 + fixture 列定义」返回固定数据；
 * - `apply_table_changes` 会把 UPDATE 语句回写到内存行，保证保存后重新加载能看到新值；
 * - 所有调用都记录在 `window.__e2e.calls`，供用例断言「哪个命令被调用了」。
 */

export interface MockColumn {
  name: string
  data_type: string
  is_primary?: boolean
  is_auto_increment?: boolean
  is_nullable?: boolean
}

export interface MockFixture {
  connectionName: string
  host: string
  port: number
  username: string
  password: string
  database: string
  table: string
  columns: MockColumn[]
  rows: unknown[][]
}

export interface InvokeCall {
  cmd: string
  args: Record<string, unknown>
}

/** 默认数据集：一个 `shop.users` 表，两行数据，`id` 为主键（可编辑的前置条件） */
export const FIXTURE: MockFixture = {
  connectionName: 'E2E 本地库',
  host: '127.0.0.1',
  port: 3306,
  username: 'root',
  password: 'secret',
  database: 'shop',
  table: 'users',
  columns: [
    { name: 'id', data_type: 'int', is_primary: true, is_auto_increment: true },
    { name: 'name', data_type: 'varchar' },
    { name: 'email', data_type: 'varchar' },
  ],
  rows: [
    [1, 'Alice', 'alice@example.com'],
    [2, 'Bob', 'bob@example.com'],
  ],
}

/** 在每个页面加载前注入 Tauri mock，必须在 `page.goto` 之前调用 */
export async function installTauriMock(page: Page, fixture: MockFixture = FIXTURE): Promise<void> {
  await page.addInitScript((data: MockFixture) => {
    interface MockState {
      calls: InvokeCall[]
      rows: unknown[][]
      connectionId: string
      database: string
      connected: boolean
    }

    type Args = Record<string, unknown>

    const state: MockState = {
      calls: [],
      rows: data.rows.map((row) => row.slice()),
      connectionId: '',
      database: data.database,
      connected: false,
    }

    const columns = data.columns.map((column) => ({
      name: column.name,
      data_type: column.data_type,
      is_nullable: column.is_nullable !== false,
      is_primary: column.is_primary === true,
      is_auto_increment: column.is_auto_increment === true,
    }))

    function connectionStatus() {
      return {
        id: state.connectionId || 'conn_e2e',
        name: data.connectionName,
        driver: 'mysql',
        database: state.database,
        host: data.host,
        port: data.port,
        username: data.username,
        state: state.connected ? 'connected' : 'disconnected',
        message: state.connected ? '连接成功' : '未连接',
        connected_at: state.connected ? new Date().toISOString() : undefined,
        stability_score: 100,
        health_status: 'good',
        // 非只读模式，且驱动不是 postgres，表格才允许编辑
        safety_mode: 'off',
      }
    }

    // ---- 极简 SQL 解析：够用即可，只处理前端生成的 UPDATE ... SET ... WHERE ... ----

    function stripIdentifier(raw: string): string {
      return raw.replace(/^`|`$/g, '')
    }

    function columnIndex(name: string): number {
      return columns.findIndex((column) => column.name === name)
    }

    function cellText(value: unknown): string {
      return value === null || value === undefined ? 'NULL' : String(value)
    }

    function unescapeSqlString(literal: string): string {
      return literal.slice(1, -1).replaceAll("''", "'").replaceAll('\\\\', '\\')
    }

    /** 从 `start` 位置读一个 SQL 字面量，返回值与下一个扫描位置 */
    function readLiteral(text: string, start: number): { value: unknown; next: number } {
      let index = start
      while (index < text.length && /\s/.test(text[index] ?? '')) index += 1
      const rest = text.slice(index)
      const quoted = /^'(?:[^'\\]|\\.|'')*'/.exec(rest)
      if (quoted) {
        return { value: unescapeSqlString(quoted[0]), next: index + quoted[0].length }
      }
      const bare = /^(NULL|-?\d+(?:\.\d+)?)/i.exec(rest)
      if (bare) {
        return {
          value: /^NULL$/i.test(bare[0]) ? null : Number(bare[0]),
          next: index + bare[0].length,
        }
      }
      return { value: rest.trim(), next: text.length }
    }

    /** 按分隔符切分，但跳过引号内的内容（值里可能包含逗号或 AND） */
    function splitTopLevel(text: string, separator: RegExp): string[] {
      const parts: string[] = []
      let current = ''
      let index = 0
      while (index < text.length) {
        const char = text[index]
        if (char === "'") {
          const literal = /^'(?:[^'\\]|\\.|'')*'/.exec(text.slice(index))
          if (literal) {
            current += literal[0]
            index += literal[0].length
            continue
          }
        }
        const match = separator.exec(text.slice(index))
        if (match && match.index === 0) {
          parts.push(current)
          current = ''
          index += match[0].length
          continue
        }
        current += char
        index += 1
      }
      parts.push(current)
      return parts.map((part) => part.trim()).filter((part) => part.length > 0)
    }

    function parseAssignments(text: string): Array<{ name: string; value: unknown }> {
      return splitTopLevel(text, /^,\s*/).flatMap((segment) => {
        const match = /^(`[^`]+`|[\w$]+)\s*=\s*/.exec(segment)
        if (!match) return []
        return [{ name: stripIdentifier(match[1]), value: readLiteral(segment, match[0].length).value }]
      })
    }

    function parseConditions(text: string): Array<{ name: string; value: unknown; isNull: boolean }> {
      return splitTopLevel(text, /^\s+AND\s+/i).flatMap((segment) => {
        const nullMatch = /^(`[^`]+`|[\w$]+)\s+IS\s+(NOT\s+)?NULL$/i.exec(segment)
        if (nullMatch) {
          return [{ name: stripIdentifier(nullMatch[1]), value: null, isNull: !nullMatch[2] }]
        }
        const match = /^(`[^`]+`|[\w$]+)\s*=\s*/.exec(segment)
        if (!match) return []
        return [
          {
            name: stripIdentifier(match[1]),
            value: readLiteral(segment, match[0].length).value,
            isNull: false,
          },
        ]
      })
    }

    /** 把 UPDATE 语句应用到内存行，模拟后端写库 */
    function applyUpdate(sql: string): void {
      const match = /^\s*UPDATE\s+(?:`[^`]+`|[\w$]+)\s+SET\s+([\s\S]+?)\s+WHERE\s+([\s\S]+?)\s*;?\s*$/i.exec(sql)
      if (!match) return
      const assignments = parseAssignments(match[1] ?? '')
      const conditions = parseConditions(match[2] ?? '')
      if (assignments.length === 0) return

      for (const row of state.rows) {
        const matched = conditions.every((condition) => {
          const index = columnIndex(condition.name)
          if (index < 0) return false
          if (condition.isNull) return row[index] === null || row[index] === undefined
          return cellText(row[index]) === cellText(condition.value)
        })
        if (!matched) continue
        for (const assignment of assignments) {
          const index = columnIndex(assignment.name)
          if (index >= 0) row[index] = assignment.value
        }
      }
    }

    // ---- 命令路由 ----

    function databaseTree() {
      return [{ id: `db_${data.database}`, name: state.database, object_type: 'Database' }]
    }

    function tableTree() {
      return [{ id: `tbl_${data.table}`, name: data.table, object_type: 'Table' }]
    }

    function queryTableData(args: Args) {
      const request = (args.request ?? {}) as {
        page?: number
        page_size?: number
        search?: string
      }
      const page = request.page ?? 1
      const pageSize = request.page_size ?? 100
      const keyword = (request.search ?? '').trim().toLowerCase()
      const rows = keyword
        ? state.rows.filter((row) => row.some((cell) => cellText(cell).toLowerCase().includes(keyword)))
        : state.rows
      const start = (page - 1) * pageSize
      return {
        columns,
        rows: rows.slice(start, start + pageSize),
        total_count: rows.length,
        page,
        page_size: pageSize,
        execution_time_ms: 1,
      }
    }

    function tableStructure() {
      return {
        table_name: data.table,
        engine: 'InnoDB',
        charset: 'utf8mb4',
        collation: 'utf8mb4_general_ci',
        row_count: state.rows.length,
        data_size: 16384,
        index_size: 0,
        create_time: '2026-01-01 00:00:00',
        update_time: '2026-01-01 00:00:00',
        create_statement: `CREATE TABLE \`${data.table}\` (id INT PRIMARY KEY)`,
        columns: columns.map((column) => ({
          name: column.name,
          data_type: column.data_type,
          is_nullable: column.is_nullable,
          is_primary: column.is_primary,
          is_auto_increment: column.is_auto_increment,
          default_value: null,
          comment: '',
        })),
        indexes: [
          {
            name: 'PRIMARY',
            index_type: 'BTREE',
            columns: ['id'],
            is_unique: true,
          },
        ],
        foreign_keys: [],
      }
    }

    function databaseInfo() {
      return {
        database: state.database,
        charset: 'utf8mb4',
        collation: 'utf8mb4_general_ci',
        table_count: 1,
        data_size: 16384,
        index_size: 0,
        total_size: 16384,
        create_time: '2026-01-01 00:00:00',
      }
    }

    function executeSql(args: Args) {
      const sql = String(args.sql ?? '')
      // 只读语句返回整表结果集，其余语句按「影响 1 行」处理
      if (/^\s*(select|show|desc|describe|explain)\b/i.test(sql)) {
        return {
          columns,
          rows: state.rows.map((row) => row.slice()),
          execution_time_ms: 1,
          rows_returned: state.rows.length,
        }
      }
      return {
        columns: [],
        rows: [],
        execution_time_ms: 1,
        rows_affected: 1,
        rows_returned: 0,
      }
    }

    function applyTableChanges(args: Args) {
      const changes = (args.changes ?? []) as Array<{ kind?: string; sql?: string }>
      for (const change of changes) {
        if (change.kind === 'update' && change.sql) applyUpdate(change.sql)
      }
      return { applied: changes.length, unchanged: 0 }
    }

    function handleCommand(cmd: string, args: Args): unknown {
      switch (cmd) {
        case 'get_license_status':
          return { activated: false, edition: 'free', fingerprint: 'e2e', features: [] }
        case 'connect': {
          const config = (args.config ?? {}) as { id?: string; database?: string }
          state.connectionId = config.id ?? 'conn_e2e'
          state.database = config.database || data.database
          state.connected = true
          return connectionStatus()
        }
        case 'disconnect':
          state.connected = false
          return null
        case 'get_connection_status':
          return connectionStatus()
        case 'get_object_tree':
          return args.database ? tableTree() : databaseTree()
        case 'query_table_data':
          return queryTableData(args)
        case 'get_table_structure':
          return tableStructure()
        case 'get_database_info':
          return databaseInfo()
        case 'get_schema_snapshot':
          return {
            database: state.database,
            tables: [
              {
                name: data.table,
                object_type: 'Table',
                columns: columns.map((column) => ({ name: column.name, data_type: column.data_type })),
              },
            ],
          }
        case 'split_sql_statements':
          return [args.sql]
        case 'execute_sql':
          return executeSql(args)
        case 'apply_table_changes':
          return applyTableChanges(args)
        case 'export_table_to_file':
        case 'export_table_to_excel':
          return `/tmp/auroradb-e2e/${String(args.filename ?? 'export.csv')}`
        case 'import_data':
          return 0
        case 'save_connection_password':
        case 'get_connection_password':
        case 'find_connection_password':
        case 'delete_connection_password':
        case 'begin_transaction':
        case 'commit_transaction':
        case 'rollback_transaction':
        case 'plugin:event|unlisten':
          return null
        case 'cancel_query':
          return false
        case 'take_pending_sql_files':
          return []
        case 'plugin:event|listen':
          return 1
        default:
          // 未覆盖的命令返回 null：前端大多会 await 后忽略结果
          return null
      }
    }

    // ---- 注入最小可用的 Tauri 运行时 ----

    const callbacks = new Map<number, unknown>()
    let nextCallbackId = 1

    const tauriWindow = window as unknown as {
      __TAURI_INTERNALS__: Record<string, unknown>
      __e2e: MockState
    }

    tauriWindow.__e2e = state
    tauriWindow.__TAURI_INTERNALS__ = {
      invoke: (cmd: string, args?: Args) => {
        state.calls.push({ cmd, args: args ?? {} })
        return Promise.resolve(handleCommand(cmd, args ?? {}))
      },
      transformCallback: (callback?: unknown) => {
        const id = nextCallbackId
        nextCallbackId += 1
        callbacks.set(id, callback)
        return id
      },
      unregisterCallback: (id: number) => {
        callbacks.delete(id)
      },
      convertFileSrc: (path: string) => path,
      metadata: {
        currentWindow: { label: 'main' },
        currentWebview: { label: 'main', windowLabel: 'main' },
      },
    }
  }, fixture)
}

/** 读取页面内已记录的 Tauri 命令调用（按调用顺序） */
export async function getTauriCalls(page: Page): Promise<InvokeCall[]> {
  return page.evaluate(() => {
    const state = (window as unknown as { __e2e?: { calls: InvokeCall[] } }).__e2e
    return state ? JSON.parse(JSON.stringify(state.calls)) : []
  })
}
