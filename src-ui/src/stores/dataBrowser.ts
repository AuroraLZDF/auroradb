import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { quoteIdentifier } from '@/utils/sql'
import { invoke } from '@tauri-apps/api/core'

/** 统一走模块化 invoke；非 Tauri 环境（如浏览器预览）返回明确错误 */
function tauriInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!('__TAURI_INTERNALS__' in window)) {
    return Promise.reject(new Error('Not running in Tauri environment'))
  }
  return invoke<T>(cmd, args)
}

export interface DatabaseObject {
  id: string
  name: string
  object_type: 'Connection' | 'Database' | 'Table' | 'View' | 'Procedure' | 'Function'
  schema?: string
  children?: DatabaseObject[]
}

interface ColumnDefinition {
  name: string
  data_type: string
  is_nullable: boolean
  is_primary: boolean
  is_auto_increment: boolean
}

interface DataQueryResponse {
  columns: ColumnDefinition[]
  rows: unknown[][]
  total_count: number
  page: number
  page_size: number
  execution_time_ms: number
}

/** schema 快照：供 SQL 编辑器补全使用的表/列信息 */
interface SchemaColumnInfo {
  name: string
  data_type: string
}

export interface SchemaTableInfo {
  name: string
  object_type: 'Table' | 'View'
  columns: SchemaColumnInfo[]
}

export interface SchemaSnapshot {
  database: string
  tables: SchemaTableInfo[]
}

/** Schema 对比：差异类型。added=仅源库存在，removed=仅目标库存在，changed=两边不一致 */
export type DiffChange = 'added' | 'removed' | 'changed'

export interface SchemaColumnDiff {
  name: string
  change: DiffChange
  source: string | null
  target: string | null
  details: string[]
}

export interface SchemaIndexDiff {
  name: string
  change: DiffChange
  source: string | null
  target: string | null
}

interface TableColumnInfo {
  name: string
  data_type: string
  length?: number
  decimals?: number
  is_nullable: boolean
  default_value?: string
  is_auto_increment: boolean
  is_primary: boolean
  unsigned: boolean
  on_update_current_timestamp: boolean
  comment?: string
}

interface TableIndexInfo {
  name: string
  type: string
  columns: string[]
  is_unique: boolean
}

interface TableForeignKeyInfo {
  name: string
  column: string
  ref_table: string
  ref_column: string
  on_delete: string
  on_update: string
}

/** 与后端 TableStructure 对应，createStatement 为 serde 重命名后的字段 */
export interface TableStructure {
  columns: TableColumnInfo[]
  indexes: TableIndexInfo[]
  foreign_keys: TableForeignKeyInfo[]
  createStatement: string
  engine?: string
  charset?: string
  collation?: string
  row_count?: number
  data_size?: number
  index_size?: number
  create_time?: string
  update_time?: string
}

export interface DatabaseInfo {
  name: string
  charset?: string
  collation?: string
  table_count: number
  data_size?: number
  index_size?: number
  total_size?: number
  create_time?: string
}

interface SchemaTableDiff {
  name: string
  change: DiffChange
  columns: SchemaColumnDiff[]
  indexes: SchemaIndexDiff[]
  statements: string[]
}

interface SchemaDiffSummary {
  added_tables: number
  removed_tables: number
  changed_tables: number
  added_columns: number
  removed_columns: number
  changed_columns: number
  added_indexes: number
  removed_indexes: number
  changed_indexes: number
}

/** Schema 对比结果：source 为基准，target 为待同步 */
export interface SchemaDiff {
  source_database: string
  target_database: string
  tables: SchemaTableDiff[]
  statements: string[]
  summary: SchemaDiffSummary
}

/** 单条表数据变更：SQL 由前端生成，后端在单事务中执行并校验影响行数 */
export interface TableChange {
  kind: 'update' | 'insert' | 'delete'
  sql: string
  table: string
  where_clause?: string
}

interface ApplyChangesResult {
  applied: number
  unchanged: number
}

/** SHOW FULL PROCESSLIST 中的一行 */
export interface ProcessInfo {
  id: number
  user: string
  host: string
  database: string | null
  command: string
  time: number
  state: string | null
  info: string | null
}

/** 实例级状态指标（来自 SHOW GLOBAL STATUS / VARIABLES） */
export interface ServerMetrics {
  version: string
  uptime_seconds: number
  qps: number
  threads_connected: number
  threads_running: number
  max_connections: number
  slow_queries: number
  slow_query_log: boolean
  long_query_time: number
  aborted_connects: number
  buffer_pool_hit_rate: number | null
  bytes_received: number
  bytes_sent: number
  com_select: number
  com_insert: number
  com_update: number
  com_delete: number
}

/** 一条审计记录：谁在何时对哪个连接执行了什么 SQL */
export interface AuditEntry {
  id: string
  timestamp: string
  connection_id: string
  connection_name: string
  host: string
  database: string
  kind: 'query' | 'table-edit' | 'import' | string
  sql: string
  success: boolean
  error?: string
  duration_ms: number
  rows_affected: number | null
  rows_returned: number
}

export const useDataBrowserStore = defineStore('dataBrowser', () => {
  const objects = ref<DatabaseObject[]>([])
  const currentDatabase = ref<string>('')
  const currentTable = ref<string>('')
  const tableData = ref<DataQueryResponse | null>(null)
  const loading = ref(false)
  const page = ref(1)
  const pageSize = ref(100)

  // 数据库列表（根级对象）
  const objectTree = computed(() => objects.value)

  // 每个数据库的表列表
  const databaseTables = ref<Record<string, DatabaseObject[]>>({})

  // Query Tab 的查询结果缓存
  const queryResults = ref<Record<string, {
    columns: { name: string; data_type: string }[]
    rows: unknown[][]
    execution_time_ms: number
    rows_affected?: number
    rows_returned: number
    truncated?: boolean
    warning?: string
  } | null>>({})

  // Query Tab 的错误缓存
  const queryErrors = ref<Record<string, string>>({})

  // 获取特定数据库的表
  function getDatabaseTables(dbName: string): DatabaseObject[] {
    return databaseTables.value[dbName] || []
  }

  // 保存 Query Tab 的查询结果
  function setQueryResult(tabId: string, data: typeof queryResults.value[string]) {
    queryResults.value[tabId] = data
  }

  // 获取 Query Tab 的查询结果
  function getQueryResult(tabId: string) {
    return queryResults.value[tabId] || null
  }

  // 保存 Query Tab 的错误
  function setQueryError(tabId: string, error: string) {
    queryErrors.value[tabId] = error
  }

  // 获取 Query Tab 的错误
  function getQueryError(tabId: string) {
    return queryErrors.value[tabId] || ''
  }

  // 清除 Query Tab 的结果
  function clearQueryResult(tabId: string) {
    delete queryResults.value[tabId]
    delete queryErrors.value[tabId]
  }

  async function loadObjectTree(connectionId: string, database?: string): Promise<DatabaseObject[]> {
    loading.value = true
    try {
      const result = await tauriInvoke<DatabaseObject[]>('get_object_tree', {
        connectionId,
        database,
      })
      if (!database) {
        // 加载数据库列表
        objects.value = result
      } else {
        // 加载特定数据库的表，保存到 databaseTables
        databaseTables.value[database] = result
      }
      return result
    } catch (e) {
      console.error('Failed to load object tree:', e)
      throw e
    } finally {
      loading.value = false
    }
  }

  async function loadTableData(
    connectionId: string,
    database: string,
    table: string,
    options: {
      page?: number
      pageSize?: number
      sortColumn?: string
      sortDirection?: 'asc' | 'desc'
    } = {}
  ) {
    loading.value = true
    currentDatabase.value = database
    currentTable.value = table

    try {
      // 转换排序方向为首字母大写，以匹配后端枚举
      const sortDirection = options.sortDirection
        ? options.sortDirection.charAt(0).toUpperCase() + options.sortDirection.slice(1)
        : undefined

      const result = await tauriInvoke<DataQueryResponse>('query_table_data', {
        request: {
          connection_id: connectionId,
          database,
          table,
          page: options.page || page.value,
          page_size: options.pageSize || pageSize.value,
          sort_column: options.sortColumn,
          sort_direction: sortDirection,
        },
      })
      tableData.value = result
      return result
    } catch (e) {
      console.error('Failed to load table data:', e)
      throw e
    } finally {
      loading.value = false
    }
  }

  function setPage(p: number) {
    page.value = p
  }

  function setPageSize(size: number) {
    pageSize.value = size
    page.value = 1
  }

  // 事务管理
  async function beginTransaction(connectionId: string): Promise<void> {
    await tauriInvoke<void>('begin_transaction', { connectionId })
  }

  async function commitTransaction(connectionId: string): Promise<void> {
    await tauriInvoke<void>('commit_transaction', { connectionId })
  }

  async function rollbackTransaction(connectionId: string): Promise<void> {
    await tauriInvoke<void>('rollback_transaction', { connectionId })
  }

  // 导入导出
  async function exportTableData(
    connectionId: string,
    database: string,
    table: string,
    format: 'csv' | 'sql' | 'json'
  ): Promise<string> {
    return await tauriInvoke<string>('export_table_data', {
      connectionId,
      database,
      table,
      format,
    })
  }

  // Excel 导出在后端直接读取整表并写工作簿，避免大表数据经前端往返
  async function exportTableToExcel(
    connectionId: string,
    database: string,
    table: string,
    filename: string
  ): Promise<string> {
    return await tauriInvoke<string>('export_table_to_excel', {
      connectionId,
      database,
      table,
      filename,
    })
  }

  async function importData(
    connectionId: string,
    database: string,
    table: string,
    format: 'csv' | 'sql',
    data: string
  ): Promise<number> {
    return await tauriInvoke<number>('import_data', {
      connectionId,
      database,
      table,
      format,
      data,
    })
  }

  // 在单事务中批量应用表数据变更（任一条失败则整体回滚）
  async function applyTableChanges(
    connectionId: string,
    database: string,
    changes: TableChange[]
  ): Promise<ApplyChangesResult> {
    return await tauriInvoke<ApplyChangesResult>('apply_table_changes', {
      connectionId,
      database,
      changes,
    })
  }

  // 执行 SQL 语句
  async function executeSql(
    connectionId: string,
    database: string,
    sql: string
  ): Promise<{ rows_affected: number }> {
    return await tauriInvoke<{ rows_affected: number }>('execute_sql', {
      connectionId,
      database,
      sql,
    })
  }

  // 删除表
  async function dropTable(
    connectionId: string,
    database: string,
    table: string
  ): Promise<void> {
    await executeSql(connectionId, database, `DROP TABLE IF EXISTS ${quoteIdentifier(table)}`)
  }

  // 清空表数据 (DELETE - 不清除ID自增值)
  async function clearTable(
    connectionId: string,
    database: string,
    table: string
  ): Promise<void> {
    await executeSql(connectionId, database, `DELETE FROM ${quoteIdentifier(table)}`)
  }

  // 清空表数据 (TRUNCATE - 清除ID自增值)
  async function truncateTable(
    connectionId: string,
    database: string,
    table: string
  ): Promise<void> {
    await executeSql(connectionId, database, `TRUNCATE TABLE ${quoteIdentifier(table)}`)
  }

  // 复制表结构和数据
  async function copyTable(
    connectionId: string,
    database: string,
    sourceTable: string,
    newTable: string,
    copyData: boolean = true
  ): Promise<void> {
    // 先复制表结构
    await executeSql(connectionId, database, `CREATE TABLE ${quoteIdentifier(newTable)} LIKE ${quoteIdentifier(sourceTable)}`)
    // 如果需要，复制数据
    if (copyData) {
      await executeSql(connectionId, database, `INSERT INTO ${quoteIdentifier(newTable)} SELECT * FROM ${quoteIdentifier(sourceTable)}`)
    }
  }

  // 获取表结构
  async function getTableStructure(
    connectionId: string,
    database: string,
    table: string
  ): Promise<TableStructure> {
    return await tauriInvoke('get_table_structure', {
      connectionId,
      database,
      table
    })
  }

  /** 获取数据库 schema 快照（表/视图 + 列），供 SQL 编辑器补全使用 */
  async function getSchemaSnapshot(
    connectionId: string,
    database: string
  ): Promise<SchemaSnapshot> {
    return await tauriInvoke('get_schema_snapshot', {
      connectionId,
      database
    })
  }

  /** 读取指定连接的库名列表（不修改当前连接的对象树） */
  async function listDatabases(connectionId: string): Promise<string[]> {
    const result = await tauriInvoke<DatabaseObject[]>('get_object_tree', {
      connectionId
    })
    return result.map((item) => item.name)
  }

  /** 对比两个库的表结构，source 为基准、target 为待同步 */
  async function compareSchemas(
    sourceConnectionId: string,
    sourceDatabase: string,
    targetConnectionId: string,
    targetDatabase: string
  ): Promise<SchemaDiff> {
    return await tauriInvoke('compare_schemas', {
      sourceConnectionId,
      sourceDatabase,
      targetConnectionId,
      targetDatabase
    })
  }

  /** 读取实例当前会话列表（SHOW FULL PROCESSLIST） */
  async function getProcessList(connectionId: string): Promise<ProcessInfo[]> {
    return await tauriInvoke<ProcessInfo[]>('get_process_list', { connectionId })
  }

  /** 终止指定会话；processId 由后端按 u64 校验 */
  async function killProcess(connectionId: string, processId: number): Promise<void> {
    await tauriInvoke<void>('kill_process', { connectionId, processId })
  }

  /** 读取实例级状态指标 */
  async function getServerMetrics(connectionId: string): Promise<ServerMetrics> {
    return await tauriInvoke<ServerMetrics>('get_server_metrics', { connectionId })
  }

  /** 读取本地审计日志，按时间倒序 */
  async function listAuditEntries(options: {
    limit?: number
    connectionId?: string
    success?: boolean
    keyword?: string
  } = {}): Promise<AuditEntry[]> {
    return await tauriInvoke<AuditEntry[]>('list_audit_entries', {
      limit: options.limit,
      connectionId: options.connectionId,
      success: options.success,
      keyword: options.keyword
    })
  }

  /** 清空本地审计日志，返回删除的文件数 */
  async function clearAuditLogs(): Promise<number> {
    return await tauriInvoke<number>('clear_audit_logs')
  }

  // 导出数据库SQL
  async function exportDatabaseSql(
    connectionId: string,
    database: string
  ): Promise<string> {
    return await tauriInvoke<string>('export_database_sql', {
      connectionId,
      database
    })
  }

  // 导入数据库SQL
  async function importDatabaseSql(
    connectionId: string,
    database: string,
    sql: string
  ): Promise<void> {
    await tauriInvoke('import_database_sql', {
      connectionId,
      database,
      sql
    })
  }

  return {
    objects,
    objectTree,
    databaseTables,
    getDatabaseTables,
    currentDatabase,
    currentTable,
    tableData,
    loading,
    page,
    pageSize,
    queryResults,
    queryErrors,
    setQueryResult,
    getQueryResult,
    setQueryError,
    getQueryError,
    clearQueryResult,
    loadObjectTree,
    loadTableData,
    setPage,
    setPageSize,
    beginTransaction,
    commitTransaction,
    rollbackTransaction,
    exportTableData,
    exportTableToExcel,
    importData,
    getSchemaSnapshot,
    listDatabases,
    compareSchemas,
    applyTableChanges,
    executeSql,
    dropTable,
    clearTable,
    truncateTable,
    copyTable,
    getTableStructure,
    getProcessList,
    killProcess,
    getServerMetrics,
    listAuditEntries,
    clearAuditLogs,
    exportDatabaseSql,
    importDatabaseSql,
  }
})
