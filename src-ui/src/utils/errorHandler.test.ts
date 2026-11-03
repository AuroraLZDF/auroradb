import { describe, expect, it } from 'vitest'
import { createErrorDeduper, formatError, isIgnorableError, truncateForToast } from './errorHandler'

describe('formatError', () => {
  it('Error 取 message', () => {
    expect(formatError(new Error('boom'))).toBe('boom')
  })

  it('Error 的 message 为空时退回 name', () => {
    const error = new Error('')
    error.name = 'CustomError'
    expect(formatError(error)).toBe('CustomError')
  })

  it('字符串原样返回', () => {
    expect(formatError('用户取消')).toBe('用户取消')
  })

  it('null / undefined 返回空串', () => {
    expect(formatError(null)).toBe('')
    expect(formatError(undefined)).toBe('')
  })

  it('带 message 字段的对象取 message（如 Tauri IPC 错误）', () => {
    expect(formatError({ message: '数据库连接失败', code: 'ECONNREFUSED' })).toBe('数据库连接失败')
  })

  it('普通对象退回 JSON 文本', () => {
    expect(formatError({ code: 500 })).toBe('{"code":500}')
  })

  it('数字与布尔退回 String', () => {
    expect(formatError(42)).toBe('42')
    expect(formatError(false)).toBe('false')
  })

  it('循环引用对象不抛错', () => {
    const circular: Record<string, unknown> = {}
    circular.self = circular
    expect(() => formatError(circular)).not.toThrow()
  })

  it('空对象不返回无意义的 JSON', () => {
    expect(formatError({})).toBe('[object Object]')
  })
})

describe('truncateForToast', () => {
  it('不超限时原样返回（仅去首尾空白）', () => {
    expect(truncateForToast('  短消息  ')).toBe('短消息')
  })

  it('超限时截断并追加省略号，长度不超过 max', () => {
    const result = truncateForToast('a'.repeat(50), 10)
    expect(result).toBe('aaaaaaaaa…')
    expect(result.length).toBe(10)
  })

  it('max <= 0 返回空串', () => {
    expect(truncateForToast('abc', 0)).toBe('')
  })

  it('不会把代理对截成半个字符', () => {
    // '😀' 占两个 UTF-16 单元；max=2 时若直接 slice(0,1) 会留下半个代理对
    const result = truncateForToast('😀😀', 2)
    expect(result).toBe('…')
    expect(result.length).toBe(1)
  })
})

describe('isIgnorableError', () => {
  it('识别 ResizeObserver 循环警告', () => {
    expect(isIgnorableError('ResizeObserver loop completed with undelivered notifications.')).toBe(true)
    expect(isIgnorableError('ResizeObserver loop limit exceeded')).toBe(true)
  })

  it('识别跨域 Script error.', () => {
    expect(isIgnorableError('Script error.')).toBe(true)
  })

  it('真实业务错误不忽略', () => {
    expect(isIgnorableError('数据库连接失败')).toBe(false)
    expect(isIgnorableError('Script error. extra')).toBe(false)
  })
})

describe('createErrorDeduper', () => {
  it('窗口内相同文案只上报一次', () => {
    const deduper = createErrorDeduper({ windowMs: 1000, now: () => 0 })
    expect(deduper.shouldReport('boom')).toBe(true)
    expect(deduper.shouldReport('boom')).toBe(false)
  })

  it('超出窗口后允许再次上报', () => {
    let now = 0
    const deduper = createErrorDeduper({ windowMs: 1000, now: () => now })
    expect(deduper.shouldReport('boom')).toBe(true)
    now = 999
    expect(deduper.shouldReport('boom')).toBe(false)
    now = 1000
    expect(deduper.shouldReport('boom')).toBe(true)
  })

  it('不同文案互不影响', () => {
    const deduper = createErrorDeduper({ windowMs: 1000, now: () => 0 })
    expect(deduper.shouldReport('a')).toBe(true)
    expect(deduper.shouldReport('b')).toBe(true)
    expect(deduper.shouldReport('a')).toBe(false)
  })

  it('空文案与纯空白文案共用 unknown 键', () => {
    const deduper = createErrorDeduper({ windowMs: 1000, now: () => 0 })
    expect(deduper.shouldReport('')).toBe(true)
    expect(deduper.shouldReport('   ')).toBe(false)
  })

  it('reset 后可重新上报', () => {
    const deduper = createErrorDeduper({ windowMs: 1000, now: () => 0 })
    expect(deduper.shouldReport('boom')).toBe(true)
    deduper.reset()
    expect(deduper.shouldReport('boom')).toBe(true)
  })

  it('条目数超过上限时不会无限增长', () => {
    let now = 0
    const deduper = createErrorDeduper({ windowMs: 10, now: () => now, maxEntries: 2 })
    expect(deduper.shouldReport('a')).toBe(true)
    now = 20
    expect(deduper.shouldReport('b')).toBe(true)
    expect(deduper.shouldReport('c')).toBe(true)
    // 过期项已被清理，最早的 a 在窗口外可再次上报
    expect(deduper.shouldReport('a')).toBe(true)
  })
})
