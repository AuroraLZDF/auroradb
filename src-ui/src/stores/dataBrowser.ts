import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { quoteIdentifier } from '@/utils/sql'
import { useConnectionStore } from './connection'
import type { DriverKind } from './savedConnections'
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
  /** 自增值（仅 MySQL 表；无自增列时为 null） */
  auto_increment?: number | null
  /** 最后修改时间（仅 MySQL 表；空表或从未修改时为 null） */
  updated_at?: string | null
  /** 表占用空间字节数（数据 + 索引，仅 MySQL 表） */
  size_bytes?: number | null
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
  /** 下一次游标翻页的主键值；为 null/undefined 表示不支持游标翻页 */
  next_cursor?: unknown
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

/** Schema 差异脚本执行结果：逐条应用，遇错即停；failed_at 为失败语句序号（从 1 开始） */
export interface ApplySchemaDiffResult {
  applied: number
  failed_at: number | null
  error: string | null
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
  /** 表数据加载中（DataTable 工具栏据此切换「取消查询」按钮） */
  const loading = ref(false)
  /** 大表触底续页中：与整页加载区分，避免全屏遮罩闪烁 */
  const loadingMore = ref(false)
  /** 对象树/表列表加载中，与表数据加载严格区分，避免侧栏刷新误触发表格工具栏状态 */
  const objectTreeLoading = ref(false)
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

  // Query Tab 的多语句结果缓存：切换 Tab 后仍能还原按语句分栏的结果
  const queryStatements = ref<Record<string, {
    sql: string
    result: typeof queryResults.value[string]
    error: string
    cancelled?: boolean
  }[]>>({})

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

  // 保存 Query Tab 的多语句结果
  function setQueryStatements(tabId: string, data: typeof queryStatements.value[string]) {
    queryStatements.value[tabId] = data
  }

  // 获取 Query Tab 的多语句结果
  function getQueryStatements(tabId: string) {
    return queryStatements.value[tabId] || []
  }

  // 清除 Query Tab 的结果
  function clearQueryResult(tabId: string) {
    delete queryResults.value[tabId]
    delete queryErrors.value[tabId]
    delete queryStatements.value[tabId]
  }

  async function loadObjectTree(connectionId: string, database?: string): Promise<DatabaseObject[]> {
    objectTreeLoading.value = true
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
      objectTreeLoading.value = false
    }
  }

  /**
   * 表数据请求序号：快速切换表/翻页时，先发出的慢请求可能后返回，
   * 用序号丢弃过期响应，避免旧表数据覆盖当前表。
   */
  let tableLoadSeq = 0

  async function loadTableData(
    connectionId: string,
    database: string,
    table: string,
    options: {
      page?: number
      pageSize?: number
      sortColumn?: string
      sortDirection?: 'asc' | 'desc'
      /** 关键字：后端在所有可比较列上做 LIKE '%keyword%' 匹配 */
      search?: string
      /** 只查询这些列（宽表按需加载）；为空则由后端 SELECT * */
      columns?: string[]
    } = {}
  ) {
    const seq = ++tableLoadSeq
    loading.value = true
    // 整表重新加载会作废进行中的续页，顺带复位续页状态，避免「加载中」卡死
    loadingMore.value = false
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
          columns: options.columns && options.columns.length > 0 ? options.columns : undefined,
          page: options.page || page.value,
          page_size: options.pageSize || pageSize.value,
          sort_column: options.sortColumn,
          sort_direction: sortDirection,
          search: options.search || undefined,
        },
      })
      // 已有更新的请求发出，本次响应作废
      if (seq !== tableLoadSeq) return null
      tableData.value = result
      return result
    } catch (e) {
      // 过期请求的失败同样不打扰用户，交给最新请求的结果决定界面
      if (seq !== tableLoadSeq) return null
      console.error('Failed to load table data:', e)
      throw e
    } finally {
      if (seq === tableLoadSeq) loading.value = false
    }
  }

  /**
   * 游标续页：把下一批行追加到当前结果末尾（大表触底加载）。
   *
   * 与 `loadTableData` 共用请求序号：期间若发生刷新/排序/搜索，本次结果作废。
   * 续页不重复统计总数，`total_count` 沿用首页的值。
   */
  async function loadMoreTableData(
    connectionId: string,
    database: string,
    table: string,
    options: {
      pageSize?: number
      sortColumn?: string
      sortDirection?: 'asc' | 'desc'
      search?: string
      columns?: string[]
    } = {}
  ) {
    const current = tableData.value
    if (!current || current.next_cursor == null) return null

    const seq = ++tableLoadSeq
    loadingMore.value = true
    try {
      const sortDirection = options.sortDirection
        ? options.sortDirection.charAt(0).toUpperCase() + options.sortDirection.slice(1)
        : undefined

      const result = await tauriInvoke<DataQueryResponse>('query_table_data', {
        request: {
          connection_id: connectionId,
          database,
          table,
          columns: options.columns && options.columns.length > 0 ? options.columns : undefined,
          page: 1,
          page_size: options.pageSize || pageSize.value,
          sort_column: options.sortColumn,
          sort_direction: sortDirection,
          search: options.search || undefined,
          after: current.next_cursor,
        },
      })
      if (seq !== tableLoadSeq) return null
      tableData.value = {
        ...result,
        rows: [...current.rows, ...result.rows],
        total_count: current.total_count || result.total_count,
        execution_time_ms: current.execution_time_ms + result.execution_time_ms,
      }
      return tableData.value
    } catch (e) {
      if (seq !== tableLoadSeq) return null
      console.error('Failed to load more table data:', e)
      throw e
    } finally {
      if (seq === tableLoadSeq) loadingMore.value = false
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
  // 导出由后端逐行写盘，前端只负责给出默认文件名并接收保存路径
  async function exportTableToFile(
    connectionId: string,
    database: string,
    table: string,
    format: 'csv' | 'sql' | 'json',
    filename: string
  ): Promise<string> {
    return await tauriInvoke<string>('export_table_to_file', {
      connectionId,
      database,
      table,
      format,
      filename,
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

  /** 按连接 ID 取驱动：标识符引用与转义规则随方言变化，缺省按 MySQL 处理 */
  function driverOf(connectionId: string): DriverKind {
    const connections = useConnectionStore()
    return connections.activeConnections.find(c => c.id === connectionId)?.driver ?? 'mysql'
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

  // 删除表（支持一次删除多张：拼成一条 DROP TABLE 语句）
  async function dropTables(
    connectionId: string,
    database: string,
    tables: string[]
  ): Promise<void> {
    if (tables.length === 0) return
    const driver = driverOf(connectionId)
    const list = tables.map(table => quoteIdentifier(table, driver)).join(', ')
    await executeSql(connectionId, database, `DROP TABLE IF EXISTS ${list}`)
  }

  // 清空表数据 (DELETE - 不清除ID自增值)
  async function clearTable(
    connectionId: string,
    database: string,
    table: string
  ): Promise<void> {
    const driver = driverOf(connectionId)
    await executeSql(connectionId, database, `DELETE FROM ${quoteIdentifier(table, driver)}`)
  }

  // 清空表数据 (TRUNCATE - 清除ID自增值)
  async function truncateTable(
    connectionId: string,
    database: string,
    table: string
  ): Promise<void> {
    const driver = driverOf(connectionId)
    await executeSql(connectionId, database, `TRUNCATE TABLE ${quoteIdentifier(table, driver)}`)
  }

  // 复制表结构和数据
  async function copyTable(
    connectionId: string,
    database: string,
    sourceTable: string,
    newTable: string,
    copyData: boolean = true
  ): Promise<void> {
    const driver = driverOf(connectionId)
    // 先复制表结构
    await executeSql(connectionId, database, `CREATE TABLE ${quoteIdentifier(newTable, driver)} LIKE ${quoteIdentifier(sourceTable, driver)}`)
    // 如果需要，复制数据
    if (copyData) {
      await executeSql(connectionId, database, `INSERT INTO ${quoteIdentifier(newTable, driver)} SELECT * FROM ${quoteIdentifier(sourceTable, driver)}`)
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

  /** 将 Schema 差异迁移脚本逐条应用到目标库，遇错即停 */
  async function applySchemaDiff(
    connectionId: string,
    database: string,
    statements: string[]
  ): Promise<ApplySchemaDiffResult> {
    return await tauriInvoke<ApplySchemaDiffResult>('apply_schema_diff', {
      connectionId,
      database,
      statements
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

  // 从文件导入数据库 SQL（后端流式执行，无大小限制）
  // taskId 用于把后端推送的 import-progress 事件对回本次任务
  async function importSqlFile(
    connectionId: string,
    database: string,
    filePath: string,
    taskId: string
  ): Promise<void> {
    await tauriInvoke('import_sql_file', {
      connectionId,
      database,
      filePath,
      taskId
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
    loadingMore,
    objectTreeLoading,
    page,
    pageSize,
    queryResults,
    queryErrors,
    queryStatements,
    setQueryResult,
    getQueryResult,
    setQueryError,
    getQueryError,
    setQueryStatements,
    getQueryStatements,
    clearQueryResult,
    loadObjectTree,
    loadTableData,
    loadMoreTableData,
    setPage,
    setPageSize,
    beginTransaction,
    commitTransaction,
    rollbackTransaction,
    exportTableToFile,
    exportTableToExcel,
    importData,
    getSchemaSnapshot,
    listDatabases,
    compareSchemas,
    applySchemaDiff,
    applyTableChanges,
    executeSql,
    dropTables,
    clearTable,
    truncateTable,
    copyTable,
    getTableStructure,
    getProcessList,
    killProcess,
    getServerMetrics,
    listAuditEntries,
    clearAuditLogs,
    importSqlFile,
  }
})
