import { describe, expect, it } from 'vitest'
import {
  maxTabIndex,
  nextSqlTabId,
  nextTabCounter,
  resolveTabConnectionId,
} from './tabConnection'

describe('resolveTabConnectionId', () => {
  it('Tab 绑定了连接时优先使用 Tab 自己的连接', () => {
    expect(resolveTabConnectionId({ connectionId: 'conn-a' }, 'conn-b')).toBe('conn-a')
  })

  it('Tab 未绑定（空串 / 缺失）时回退全局当前连接', () => {
    expect(resolveTabConnectionId({ connectionId: '' }, 'conn-b')).toBe('conn-b')
    expect(resolveTabConnectionId({}, 'conn-b')).toBe('conn-b')
    expect(resolveTabConnectionId(null, 'conn-b')).toBe('conn-b')
    expect(resolveTabConnectionId(undefined, 'conn-b')).toBe('conn-b')
  })

  it('两者都为空时返回空串，表示没有可用连接', () => {
    expect(resolveTabConnectionId({ connectionId: '' }, '')).toBe('')
    expect(resolveTabConnectionId(null, null)).toBe('')
    expect(resolveTabConnectionId(undefined, undefined)).toBe('')
  })

  it('忽略首尾空白，只有空白的绑定视为未绑定', () => {
    expect(resolveTabConnectionId({ connectionId: '  conn-a  ' }, 'conn-b')).toBe('conn-a')
    expect(resolveTabConnectionId({ connectionId: '   ' }, 'conn-b')).toBe('conn-b')
  })
})

describe('maxTabIndex / nextTabCounter', () => {
  it('取同前缀 id 的最大数字后缀', () => {
    expect(maxTabIndex('sql-', ['sql-1', 'sql-7', 'sql-3'])).toBe(7)
    expect(maxTabIndex('sql-', [])).toBe(0)
  })

  it('忽略不同前缀与没有数字后缀的 id', () => {
    expect(maxTabIndex('sql-', ['console-9', 'sql-abc', 'objects', 'sql-4'])).toBe(4)
  })

  it('计数器推进到已用最大序号 +1', () => {
    expect(nextTabCounter('sql-', [])).toBe(1)
    expect(nextTabCounter('sql-', ['sql-1', 'sql-2'])).toBe(3)
    // 恢复草稿后序列号可能不连续，仍然按最大值推进
    expect(nextTabCounter('sql-', ['sql-2', 'sql-9'])).toBe(10)
  })
})

describe('nextSqlTabId', () => {
  it('空列表从 1 开始编号', () => {
    expect(nextSqlTabId('sql-', [])).toBe('sql-1')
  })

  it('在已用最大序号之后递增', () => {
    expect(nextSqlTabId('sql-', ['sql-1', 'sql-2'])).toBe('sql-3')
    expect(nextSqlTabId('sql-', ['sql-1', 'sql-5'])).toBe('sql-6')
  })

  it('生成的 id 一定不在现有 id 中（恢复草稿后也不撞号）', () => {
    const existing = ['objects', 'sql-1', 'sql-3', 'sql-4', 'console-2']
    for (let i = 0; i < 20; i++) {
      const id = nextSqlTabId('sql-', existing)
      expect(existing).not.toContain(id)
      existing.push(id)
    }
    expect(new Set(existing).size).toBe(existing.length)
  })

  it('不同前缀互不影响', () => {
    expect(nextSqlTabId('sql-', ['console-8', 'sql-2'])).toBe('sql-3')
  })
})
