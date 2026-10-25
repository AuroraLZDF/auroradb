import { ref } from 'vue'
import { useDataBrowserStore, type SchemaSnapshot, type SchemaTableInfo } from '@/stores/dataBrowser'
import { quoteIdentifier } from '@/utils/sql'

/** 补全项类型，用于排序与图标区分 */
export type CompletionKind = 'column' | 'table' | 'view' | 'database' | 'keyword'

export interface CompletionItem {
  /** 实际插入编辑器的文本（必要时带反引号） */
  label: string
  /** 原始名称（用于前缀匹配与展示） */
  name: string
  kind: CompletionKind
  /** 附加说明，如数据类型 */
  detail?: string
}

interface CompletionContext {
  /** 当前连接可见的数据库 */
  databases: string[]
  /** 当前选中的数据库 */
  currentDatabase: string
  /** 当前数据库的表/视图以及列 */
  tables: SchemaTableInfo[]
}

interface CompletionResult {
  items: CompletionItem[]
  /** 替换区间（只替换光标前的标识符片段，保留 db.table. 限定符） */
  replaceStart: number
  replaceEnd: number
  /** 触发补全的标识符片段 */
  token: string
  /** 限定符，如 `db.table`，未限定为空串 */
  qualifier: string
}

/** 补全候选上限，避免大表/大库时弹窗过长 */
const MAX_ITEMS = 50

const SQL_KEYWORDS = [
  'SELECT', 'FROM', 'WHERE', 'INSERT', 'INTO', 'VALUES', 'UPDATE', 'SET', 'DELETE',
  'CREATE', 'ALTER', 'DROP', 'TRUNCATE', 'TABLE', 'VIEW', 'INDEX', 'DATABASE', 'SCHEMA',
  'JOIN', 'INNER', 'LEFT', 'RIGHT', 'FULL', 'OUTER', 'CROSS', 'ON', 'USING',
  'GROUP', 'BY', 'ORDER', 'HAVING', 'LIMIT', 'OFFSET', 'UNION', 'ALL', 'DISTINCT',
  'AS', 'AND', 'OR', 'NOT', 'NULL', 'IS', 'IN', 'EXISTS', 'BETWEEN', 'LIKE',
  'CASE', 'WHEN', 'THEN', 'ELSE', 'END', 'ASC', 'DESC', 'WITH', 'RECURSIVE',
  'COUNT', 'SUM', 'AVG', 'MIN', 'MAX', 'IF', 'IFNULL', 'COALESCE', 'CAST',
  'BEGIN', 'COMMIT', 'ROLLBACK', 'START', 'TRANSACTION', 'USE', 'SHOW',
  'DESCRIBE', 'EXPLAIN', 'PRIMARY', 'KEY', 'FOREIGN', 'REFERENCES', 'UNIQUE',
  'DEFAULT', 'AUTO_INCREMENT', 'ENGINE', 'CHARSET', 'COLLATE', 'REPLACE', 'DUPLICATE'
]

/** SQL 关键字中被当作别名捕获时需忽略的词 */
const ALIAS_STOP_WORDS = new Set([
  ...SQL_KEYWORDS,
  'WHERE', 'GROUP', 'ORDER', 'LIMIT', 'HAVING', 'UNION', 'ON', 'USING', 'SET',
  'LEFT', 'RIGHT', 'INNER', 'OUTER', 'CROSS', 'JOIN', 'STRAIGHT_JOIN'
])

const IDENTIFIER_CHAR = /[A-Za-z0-9_$.`]/
const PLAIN_IDENTIFIER = /^[A-Za-z_$][A-Za-z0-9_$]*$/
const KIND_ORDER: Record<CompletionKind, number> = {
  column: 0,
  table: 1,
  view: 2,
  database: 3,
  keyword: 4
}

function formatIdentifier(name: string): string {
  return PLAIN_IDENTIFIER.test(name) ? name : quoteIdentifier(name)
}

/** 光标前最近一个标识符链（如 `db.tbl.co`），并解析出限定符与待替换片段 */
function splitIdentifierChain(before: string) {
  let start = before.length
  while (start > 0 && IDENTIFIER_CHAR.test(before.charAt(start - 1))) start--
  const raw = before.slice(start)
  const segments = raw.split('.')
  const prefixRaw = segments.pop() ?? ''
  const qualifiers = segments.map(s => s.replace(/`/g, '')).filter(Boolean)

  return {
    prefix: prefixRaw.replace(/`/g, ''),
    prefixRaw,
    qualifiers
  }
}

/** 从 `FROM/JOIN xxx [AS] alias` 中解析别名，支持 `alias.` 补全列 */
function collectAliases(before: string): Map<string, string> {
  const aliases = new Map<string, string>()
  const re = /\b(?:from|join)\s+`?([A-Za-z0-9_$]+)`?(?:\s+(?:as\s+)?`?([A-Za-z0-9_$]+)`?)?/gi

  for (const match of before.matchAll(re)) {
    const table = match[1]
    const alias = match[2]
    if (!table) continue
    if (alias && !ALIAS_STOP_WORDS.has(alias.toUpperCase())) {
      aliases.set(alias.toLowerCase(), table)
    }
  }

  return aliases
}

function findTable(tables: SchemaTableInfo[], name: string): SchemaTableInfo | undefined {
  const lower = name.toLowerCase()
  return tables.find(t => t.name.toLowerCase() === lower)
}

function columnItems(tables: SchemaTableInfo[], tableName: string): CompletionItem[] {
  const table = findTable(tables, tableName)
  if (!table) return []
  return table.columns.map(col => ({
    label: formatIdentifier(col.name),
    name: col.name,
    kind: 'column' as const,
    detail: col.data_type
  }))
}

function tableItems(tables: SchemaTableInfo[]): CompletionItem[] {
  return tables.map(table => ({
    label: formatIdentifier(table.name),
    name: table.name,
    kind: table.object_type === 'View' ? ('view' as const) : ('table' as const)
  }))
}

function keywordItems(): CompletionItem[] {
  return SQL_KEYWORDS.map(word => ({ label: word, name: word, kind: 'keyword' as const }))
}

/**
 * 纯函数：计算光标处的补全候选。
 *
 * 支持：
 * - `表名.` / `别名.` → 该表列名
 * - `库名.` → 该库表名（仅当前库有快照，其它库返回空）
 * - `库名.表名.` → 列名
 * - 无限定符 → 关键字 + 表/视图 + 库名
 */
function getSqlCompletions(
  sql: string,
  caret: number,
  ctx: CompletionContext
): CompletionResult {
  const before = sql.slice(0, caret)
  const { prefix, prefixRaw, qualifiers } = splitIdentifierChain(before)

  let candidates: CompletionItem[]
  if (qualifiers.length >= 2) {
    const db = qualifiers[qualifiers.length - 2] ?? ''
    const table = qualifiers[qualifiers.length - 1] ?? ''
    const sameDatabase = db.toLowerCase() === ctx.currentDatabase.toLowerCase()
    candidates = sameDatabase ? columnItems(ctx.tables, table) : []
  } else if (qualifiers.length === 1) {
    const qualifier = qualifiers[0] ?? ''
    const tableName = collectAliases(before).get(qualifier.toLowerCase()) ?? qualifier

    const isTable = findTable(ctx.tables, tableName) !== undefined
    if (isTable) {
      candidates = columnItems(ctx.tables, tableName)
    } else {
      // 限定符是库名：只有当前库有快照，其它库无从得知表名
      const isCurrentDatabase = qualifier.toLowerCase() === ctx.currentDatabase.toLowerCase()
      candidates = isCurrentDatabase ? tableItems(ctx.tables) : []
    }
  } else {
    candidates = [
      ...tableItems(ctx.tables),
      ...ctx.databases.map(db => ({
        label: formatIdentifier(db),
        name: db,
        kind: 'database' as const
      })),
      ...keywordItems()
    ]
  }

  const lowerPrefix = prefix.toLowerCase()
  const startsWith = candidates.filter(item => item.name.toLowerCase().startsWith(lowerPrefix))
  const matched = startsWith.length > 0
    ? startsWith
    : candidates.filter(item => item.name.toLowerCase().includes(lowerPrefix))

  // 去重（同一名称可能同时是库名与表名）+ 排序：完全匹配 > 类型优先级 > 名称长度
  const seen = new Set<string>()
  const items = matched
    .filter(item => {
      const key = `${item.kind}:${item.label}`
      if (seen.has(key)) return false
      seen.add(key)
      return true
    })
    .sort((a, b) => {
      const aExact = a.name.toLowerCase() === lowerPrefix ? 0 : 1
      const bExact = b.name.toLowerCase() === lowerPrefix ? 0 : 1
      if (aExact !== bExact) return aExact - bExact
      if (KIND_ORDER[a.kind] !== KIND_ORDER[b.kind]) return KIND_ORDER[a.kind] - KIND_ORDER[b.kind]
      if (a.name.length !== b.name.length) return a.name.length - b.name.length
      return a.name.localeCompare(b.name)
    })
    .slice(0, MAX_ITEMS)

  return {
    items,
    replaceStart: caret - prefixRaw.length,
    replaceEnd: caret,
    token: prefix,
    qualifier: qualifiers.join('.')
  }
}

// schema 快照按「连接 + 库」缓存，SqlEditor 与 Console 共享
const snapshotCache = new Map<string, SchemaSnapshot>()
const pendingRequests = new Map<string, Promise<SchemaSnapshot>>()

/** 清空快照缓存：执行 DDL 后调用，避免补全一直用旧结构 */
export function invalidateSchemaCache() {
  snapshotCache.clear()
}

export function useSqlCompletion() {
  const browserStore = useDataBrowserStore()
  const snapshot = ref<SchemaSnapshot | null>(null)
  const loading = ref(false)

  async function loadSchema(connectionId: string, database: string) {
    if (!connectionId || !database) {
      snapshot.value = null
      return
    }

    const key = `${connectionId}::${database}`
    const cached = snapshotCache.get(key)
    if (cached) {
      snapshot.value = cached
      return
    }

    loading.value = true
    try {
      let request = pendingRequests.get(key)
      if (!request) {
        request = browserStore.getSchemaSnapshot(connectionId, database)
        pendingRequests.set(key, request)
      }
      const data = await request
      snapshotCache.set(key, data)
      snapshot.value = data
    } catch (e) {
      // 补全失败不影响正常编辑，仅降级为关键字/表名补全
      console.warn('Failed to load schema snapshot:', e)
      snapshot.value = null
    } finally {
      pendingRequests.delete(key)
      loading.value = false
    }
  }

  function complete(
    sql: string,
    caret: number,
    databases: string[],
    currentDatabase: string
  ): CompletionResult {
    const tables = snapshot.value && snapshot.value.database === currentDatabase
      ? snapshot.value.tables
      : []
    return getSqlCompletions(sql, caret, { databases, currentDatabase, tables })
  }

  return {
    snapshot,
    loading,
    loadSchema,
    complete
  }
}
