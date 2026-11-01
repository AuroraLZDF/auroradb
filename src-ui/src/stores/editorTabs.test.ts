import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { nextTick } from 'vue'
import { useEditorTabsStore } from './editorTabs'

const DRAFTS_KEY = 'auroradb.editor.drafts'

describe('editorTabs 内容状态', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    // 自动保存是 2 秒 debounce，用假定时器避免用例结束后仍有真实定时器写盘
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('ensure 只初始化一次，不覆盖已有内容', () => {
    const store = useEditorTabsStore()
    expect(store.ensure('tab-1')).toEqual({ sql: '', dirty: false })

    store.setSql('tab-1', 'SELECT 1')
    expect(store.ensure('tab-1').sql).toBe('SELECT 1')
  })

  it('setSql 标记为已修改，markClean 复位', () => {
    const store = useEditorTabsStore()
    store.setSql('tab-1', 'SELECT 1')

    expect(store.getSql('tab-1')).toBe('SELECT 1')
    expect(store.tabs['tab-1']?.dirty).toBe(true)

    store.markClean('tab-1')
    expect(store.tabs['tab-1']?.dirty).toBe(false)
  })

  it('markExecuted 清除修改标记并记录执行时间', () => {
    const store = useEditorTabsStore()
    store.setSql('tab-1', 'SELECT 1')
    store.markExecuted('tab-1')

    expect(store.tabs['tab-1']?.dirty).toBe(false)
    expect(typeof store.tabs['tab-1']?.lastExecutedAt).toBe('number')
  })

  it('未创建的 tab 不会因 getSql / markClean 被创建', () => {
    const store = useEditorTabsStore()
    store.markClean('missing')
    store.markExecuted('missing')

    expect(store.getSql('missing')).toBe('')
    expect(store.tabs['missing']).toBeUndefined()
  })

  it('dirtyCount 统计未保存修改，remove / reset 后同步减少', () => {
    const store = useEditorTabsStore()
    store.setSql('tab-1', 'SELECT 1')
    store.setSql('tab-2', 'SELECT 2')
    store.ensure('tab-3')

    expect(store.dirtyCount).toBe(2)

    store.markClean('tab-1')
    expect(store.dirtyCount).toBe(1)

    store.remove('tab-2')
    expect(store.dirtyCount).toBe(0)

    store.reset()
    expect(store.tabs).toEqual({})
  })

  it('restoredTabIds 过滤空内容并按最后执行时间倒序', () => {
    const store = useEditorTabsStore()
    store.setSql('tab-a', 'SELECT 1')
    store.setSql('tab-b', 'SELECT 2')
    store.setSql('tab-c', '   \n  ')

    store.markExecuted('tab-a')
    vi.advanceTimersByTime(1000)
    store.markExecuted('tab-b')

    // tab-c 只有空白内容，不应在启动时被恢复
    expect(store.restoredTabIds()).toEqual(['tab-b', 'tab-a'])
  })

  it('未标记执行时间的 tab 排在已执行的之后', () => {
    const store = useEditorTabsStore()
    store.setSql('draft', 'SELECT 1')
    store.setSql('executed', 'SELECT 2')
    store.markExecuted('executed')

    expect(store.restoredTabIds()).toEqual(['executed', 'draft'])
  })

  it('修改后 2 秒自动写入 localStorage', async () => {
    const store = useEditorTabsStore()
    store.setSql('tab-1', 'SELECT 1')

    // watch 回调在微任务中调度定时器
    await nextTick()
    expect(localStorage.getItem(DRAFTS_KEY)).toBeNull()

    vi.advanceTimersByTime(2000)

    const payload = JSON.parse(localStorage.getItem(DRAFTS_KEY) ?? '{}')
    expect(payload.version).toBe(1)
    expect(payload.tabs['tab-1'].sql).toBe('SELECT 1')
  })
})

describe('editorTabs 草稿恢复', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('saveNow 与 restore 往返保留 SQL，恢复后不算未保存', () => {
    const store = useEditorTabsStore()
    store.setSql('tab-1', 'SELECT * FROM users')
    store.setSql('tab-2', 'SELECT 2')
    store.markExecuted('tab-2')
    store.saveNow()

    setActivePinia(createPinia())
    const restored = useEditorTabsStore()
    restored.restore()

    expect(restored.getSql('tab-1')).toBe('SELECT * FROM users')
    expect(restored.getSql('tab-2')).toBe('SELECT 2')
    expect(restored.dirtyCount).toBe(0)
  })

  it('版本不符时丢弃整份草稿', () => {
    localStorage.setItem(
      DRAFTS_KEY,
      JSON.stringify({ version: 99, tabs: { 'tab-1': { sql: 'SELECT 1', dirty: true } } }),
    )

    const store = useEditorTabsStore()
    store.restore()

    expect(store.tabs).toEqual({})
  })

  it('跳过空白 SQL 且最多恢复 30 个 tab', () => {
    const tabs: Record<string, { sql: string; dirty: boolean }> = {
      blank: { sql: '  ', dirty: true },
    }
    for (let i = 0; i < 35; i++) {
      tabs[`tab-${i}`] = { sql: `SELECT ${i}`, dirty: true }
    }
    localStorage.setItem(DRAFTS_KEY, JSON.stringify({ version: 1, tabs, restoredAt: 0 }))

    const store = useEditorTabsStore()
    store.restore()

    expect(store.tabs['blank']).toBeUndefined()
    expect(Object.keys(store.tabs)).toHaveLength(30)
    expect(store.getSql('tab-0')).toBe('SELECT 0')
    expect(store.getSql('tab-29')).toBe('SELECT 29')
    expect(store.getSql('tab-30')).toBe('')
  })

  it('草稿内容损坏时静默忽略，不影响启动', () => {
    localStorage.setItem(DRAFTS_KEY, '{not json')

    const store = useEditorTabsStore()
    expect(() => store.restore()).not.toThrow()
    expect(store.tabs).toEqual({})
  })

  it('clearDrafts 只删除草稿数据', () => {
    const store = useEditorTabsStore()
    store.setSql('tab-1', 'SELECT 1')
    store.saveNow()

    store.clearDrafts()

    expect(localStorage.getItem(DRAFTS_KEY)).toBeNull()
  })
})
