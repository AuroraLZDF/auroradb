import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useDataBrowserStore } from './dataBrowser'

const sampleResult = {
  columns: [{ name: 'id', data_type: 'int' }],
  rows: [[1]],
  execution_time_ms: 3,
  rows_returned: 1,
}

describe('dataBrowser 查询结果缓存', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('未写入时返回默认值，避免调用方判空', () => {
    const store = useDataBrowserStore()
    expect(store.getQueryResult('tab-a')).toBeNull()
    expect(store.getQueryError('tab-a')).toBe('')
    expect(store.getQueryStatements('tab-a')).toEqual([])
    expect(store.getDatabaseTables('db')).toEqual([])
  })

  it('结果按 tabId 隔离，互不覆盖', () => {
    const store = useDataBrowserStore()
    const other = { ...sampleResult, rows: [[2]], rows_returned: 1 }

    store.setQueryResult('tab-a', sampleResult)
    store.setQueryResult('tab-b', other)

    expect(store.getQueryResult('tab-a')).toEqual(sampleResult)
    expect(store.getQueryResult('tab-b')).toEqual(other)
  })

  it('错误信息按 tabId 缓存', () => {
    const store = useDataBrowserStore()
    store.setQueryError('tab-a', 'Table not found')
    expect(store.getQueryError('tab-a')).toBe('Table not found')
    expect(store.getQueryError('tab-b')).toBe('')
  })

  it('多语句结果保留每条语句的 SQL、结果与失败原因', () => {
    const store = useDataBrowserStore()
    const statements = [
      { sql: 'SELECT 1', result: sampleResult, error: '' },
      { sql: 'SELECT * FROM missing', result: null, error: 'Table not found' },
      { sql: 'SELECT 2', result: null, error: '', cancelled: true },
    ]

    store.setQueryStatements('tab-a', statements)

    const cached = store.getQueryStatements('tab-a')
    expect(cached).toHaveLength(3)
    expect(cached[0]?.sql).toBe('SELECT 1')
    expect(cached[1]?.error).toBe('Table not found')
    expect(cached[2]?.cancelled).toBe(true)
  })

  it('清理只影响目标 tab，单条结果/错误/多语句一并移除', () => {
    const store = useDataBrowserStore()
    store.setQueryResult('tab-a', sampleResult)
    store.setQueryError('tab-a', 'boom')
    store.setQueryStatements('tab-a', [{ sql: 'SELECT 1', result: sampleResult, error: '' }])
    store.setQueryResult('tab-b', sampleResult)

    store.clearQueryResult('tab-a')

    expect(store.getQueryResult('tab-a')).toBeNull()
    expect(store.getQueryError('tab-a')).toBe('')
    expect(store.getQueryStatements('tab-a')).toEqual([])
    expect(store.getQueryResult('tab-b')).toEqual(sampleResult)
  })
})

describe('dataBrowser 分页状态', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('切换每页条数后回到第一页', () => {
    const store = useDataBrowserStore()
    store.setPage(5)
    store.setPageSize(200)

    expect(store.pageSize).toBe(200)
    expect(store.page).toBe(1)
  })
})
