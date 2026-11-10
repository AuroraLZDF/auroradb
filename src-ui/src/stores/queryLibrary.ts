import { defineStore } from 'pinia'
import { ref, computed, watch } from 'vue'
import { t } from '@/i18n'

/** 查询历史条目 */
export interface QueryHistoryEntry {
  id: string
  sql: string
  /** 所属连接（会话 id，仅当次连接有效）；空串表示无法归属（旧版本历史迁移而来） */
  connectionId: string
  /** 连接显示名（会话 id 会随重连变化，名字才是给用户看的身份） */
  connectionName?: string
  /** 连接地址，与名字组合用于重新定位保存的连接 */
  connectionHost?: string
  connectionPort?: number
  database: string
  /** ISO 时间字符串，便于 localStorage 序列化 */
  executedAt: string
  durationMs?: number
  success: boolean
  error?: string
  /** 参数化查询的实参（`:name` → 用户输入），便于重跑时复用 */
  params?: Record<string, string>
  /** SELECT 返回行数 */
  rowsReturned?: number
  /** DML 影响行数 */
  rowsAffected?: number
}

/** 保存的查询 */
export interface SavedQuery {
  id: string
  name: string
  sql: string
  /** 文件夹名；空串表示未分类 */
  folder: string
  connectionId: string
  createdAt: string
  updatedAt: string
}

const HISTORY_STORAGE_KEY = 'auroradb.query-history.v2'
const SAVED_STORAGE_KEY = 'auroradb.saved-queries.v1'
/** 旧版本只存了全局 20 条历史，这里做一次性迁移 */
const LEGACY_HISTORY_KEY = 'sql-query-history'
/** 每个连接保留的历史条数 */
const HISTORY_LIMIT_PER_CONNECTION = 200

function newId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID()
  }
  return `${Date.now()}-${Math.random().toString(36).slice(2, 10)}`
}

function loadJson<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key)
    if (!raw) return fallback
    const parsed = JSON.parse(raw)
    return Array.isArray(parsed) ? (parsed as T) : fallback
  } catch {
    return fallback
  }
}

function persist(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value))
  } catch (e) {
    // 超出配额等写入失败不应影响查询本身
    console.error(`Failed to persist ${key}:`, e)
  }
}

export const useQueryLibraryStore = defineStore('queryLibrary', () => {
  const history = ref<QueryHistoryEntry[]>(loadJson<QueryHistoryEntry[]>(HISTORY_STORAGE_KEY, []))
  const savedQueries = ref<SavedQuery[]>(loadJson<SavedQuery[]>(SAVED_STORAGE_KEY, []))

  // 旧版本历史一次性迁移到新结构（归属连接未知，统一放到空 connectionId 下）
  if (history.value.length === 0) {
    const legacy = loadJson<{ sql: string; time: string }[]>(LEGACY_HISTORY_KEY, [])
    if (legacy.length > 0) {
      history.value = legacy
        .filter(item => typeof item?.sql === 'string' && item.sql.trim())
        .map(item => ({
          id: newId(),
          sql: item.sql,
          connectionId: '',
          database: '',
          executedAt: item.time ?? new Date().toISOString(),
          success: true
        }))
      persist(HISTORY_STORAGE_KEY, history.value)
      localStorage.removeItem(LEGACY_HISTORY_KEY)
    }
  }

  watch(history, value => persist(HISTORY_STORAGE_KEY, value), { deep: true })
  watch(savedQueries, value => persist(SAVED_STORAGE_KEY, value), { deep: true })

  /** 按连接取历史（新→旧）；同时包含旧版本迁移来的、无法归属连接的历史 */
  function historyFor(connectionId: string): QueryHistoryEntry[] {
    return history.value.filter(item => item.connectionId === connectionId || item.connectionId === '')
  }

  /** 记录一条历史：同一连接下相同 SQL 去重后置顶，并按连接裁剪条数 */
  function addHistory(entry: Omit<QueryHistoryEntry, 'id' | 'executedAt'> & { executedAt?: string }) {
    const sql = entry.sql.trim()
    if (!sql) return

    const record: QueryHistoryEntry = {
      ...entry,
      sql,
      id: newId(),
      executedAt: entry.executedAt ?? new Date().toISOString()
    }

    const next = [record, ...history.value.filter(
      item =>
        !(
          item.connectionId === record.connectionId &&
          item.sql === record.sql &&
          JSON.stringify(item.params ?? null) === JSON.stringify(record.params ?? null)
        )
    )]

    // 按连接裁剪：只保留该连接最近 N 条
    const keptPerConnection = new Map<string, number>()
    history.value = next.filter(item => {
      const count = keptPerConnection.get(item.connectionId) ?? 0
      if (item.connectionId === record.connectionId && count >= HISTORY_LIMIT_PER_CONNECTION) {
        return false
      }
      keptPerConnection.set(item.connectionId, count + 1)
      return true
    })
  }

  function removeHistory(id: string) {
    history.value = history.value.filter(item => item.id !== id)
  }

  /** 清空历史；传 connectionId 时只清该连接 */
  function clearHistory(connectionId?: string) {
    if (connectionId === undefined) {
      history.value = []
      return
    }
    history.value = history.value.filter(item => item.connectionId !== connectionId)
  }

  /** 保存查询；name 重复时追加序号 */
  function saveQuery(input: { name: string; sql: string; folder?: string; connectionId: string }): SavedQuery {
    const now = new Date().toISOString()
    const sql = input.sql.trim()
    const baseName = input.name.trim() || t('store.untitledQuery')
    const folder = (input.folder ?? '').trim()

    const sameFolder = savedQueries.value.filter(q => q.folder === folder)
    let name = baseName
    if (sameFolder.some(q => q.name === name)) {
      let index = 2
      while (sameFolder.some(q => q.name === `${baseName} (${index})`)) index++
      name = `${baseName} (${index})`
    }

    const record: SavedQuery = {
      id: newId(),
      name,
      sql,
      folder,
      connectionId: input.connectionId,
      createdAt: now,
      updatedAt: now
    }
    savedQueries.value = [record, ...savedQueries.value]
    return record
  }

  function updateSavedQuery(id: string, patch: Partial<Pick<SavedQuery, 'name' | 'sql' | 'folder'>>) {
    const index = savedQueries.value.findIndex(q => q.id === id)
    if (index === -1) return
    const current = savedQueries.value[index]
    if (!current) return
    savedQueries.value[index] = {
      ...current,
      ...patch,
      updatedAt: new Date().toISOString()
    }
  }

  function deleteSavedQuery(id: string) {
    savedQueries.value = savedQueries.value.filter(q => q.id !== id)
  }

  /** 已存在的文件夹（按名称排序，不含未分类） */
  const folders = computed(() => {
    const names = new Set<string>()
    for (const q of savedQueries.value) {
      if (q.folder) names.add(q.folder)
    }
    return [...names].sort((a, b) => a.localeCompare(b, 'zh-CN'))
  })

  return {
    history,
    savedQueries,
    folders,
    historyFor,
    addHistory,
    removeHistory,
    clearHistory,
    saveQuery,
    updateSavedQuery,
    deleteSavedQuery
  }
})
