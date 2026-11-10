import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { nextTick } from 'vue'
import { useQueryLibraryStore } from './queryLibrary'

const HISTORY_KEY = 'auroradb.query-history.v2'
const LEGACY_HISTORY_KEY = 'sql-query-history'

type Store = ReturnType<typeof useQueryLibraryStore>

function addHistory(store: Store, sql: string, connectionId = 'conn-1') {
  store.addHistory({ sql, connectionId, database: 'db', success: true })
}

describe('queryLibrary 查询历史', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('同一连接下相同 SQL 去重后置顶，并保留最新执行时间', () => {
    const store = useQueryLibraryStore()
    addHistory(store, 'SELECT 1', 'conn-1')
    addHistory(store, 'SELECT 2', 'conn-1')
    addHistory(store, 'SELECT 1', 'conn-1')

    expect(store.history).toHaveLength(2)
    expect(store.history[0]?.sql).toBe('SELECT 1')
    expect(store.history[1]?.sql).toBe('SELECT 2')
  })

  it('不同连接下的相同 SQL 不算重复', () => {
    const store = useQueryLibraryStore()
    addHistory(store, 'SELECT 1', 'conn-1')
    addHistory(store, 'SELECT 1', 'conn-2')

    expect(store.history).toHaveLength(2)
  })

  it('空白 SQL 不入历史，入参 SQL 会被 trim', () => {
    const store = useQueryLibraryStore()
    addHistory(store, '   ')
    addHistory(store, '  SELECT 1  ')

    expect(store.history).toHaveLength(1)
    expect(store.history[0]?.sql).toBe('SELECT 1')
  })

  it('historyFor 同时包含该连接与无法归属连接的历史', () => {
    const store = useQueryLibraryStore()
    addHistory(store, 'SELECT 1', 'conn-1')
    addHistory(store, 'SELECT 2', 'conn-2')
    addHistory(store, 'SELECT 3', '')

    const sqls = store.historyFor('conn-1').map(item => item.sql)
    expect(sqls).toContain('SELECT 1')
    expect(sqls).toContain('SELECT 3')
    expect(sqls).not.toContain('SELECT 2')
  })

  it('clearHistory 传连接时只清该连接，不传则全清', () => {
    const store = useQueryLibraryStore()
    addHistory(store, 'SELECT 1', 'conn-1')
    addHistory(store, 'SELECT 2', 'conn-2')

    store.clearHistory('conn-1')
    expect(store.history.map(item => item.sql)).toEqual(['SELECT 2'])

    store.clearHistory()
    expect(store.history).toHaveLength(0)
  })

  it('removeHistory 按 id 删除单条', () => {
    const store = useQueryLibraryStore()
    addHistory(store, 'SELECT 1')
    const target = store.history[0]

    store.removeHistory(target?.id ?? '')

    expect(store.history).toHaveLength(0)
  })

  it('单连接最多保留 200 条，且不影响其它连接', () => {
    const store = useQueryLibraryStore()
    addHistory(store, 'SELECT other', 'conn-2')
    for (let i = 0; i < 205; i++) {
      addHistory(store, `SELECT ${i}`, 'conn-1')
    }

    const conn1 = store.history.filter(item => item.connectionId === 'conn-1')
    expect(conn1).toHaveLength(200)
    expect(conn1[0]?.sql).toBe('SELECT 204')
    expect(store.history.some(item => item.connectionId === 'conn-2')).toBe(true)
  })

  it('变更后写入 localStorage', async () => {
    const store = useQueryLibraryStore()
    addHistory(store, 'SELECT 1')

    await nextTick()

    const persisted = JSON.parse(localStorage.getItem(HISTORY_KEY) ?? '[]')
    expect(persisted).toHaveLength(1)
    expect(persisted[0].sql).toBe('SELECT 1')
  })

  it('旧版全局历史一次性迁移到当前连接未知的空 connectionId 下', () => {
    localStorage.setItem(
      LEGACY_HISTORY_KEY,
      JSON.stringify([{ sql: 'SELECT legacy', time: '2024-01-01T00:00:00.000Z' }]),
    )

    setActivePinia(createPinia())
    const store = useQueryLibraryStore()

    expect(store.history).toHaveLength(1)
    expect(store.history[0]?.sql).toBe('SELECT legacy')
    expect(store.history[0]?.connectionId).toBe('')
    expect(localStorage.getItem(LEGACY_HISTORY_KEY)).toBeNull()
    expect(localStorage.getItem(HISTORY_KEY)).not.toBeNull()
  })
})
