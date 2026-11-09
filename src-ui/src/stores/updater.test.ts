import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { setLocale } from '@/i18n'

// 更新源偶发 5xx 不该把英文报错抛给用户：这里验证重试与错误分类
const checkMock = vi.fn()
vi.mock('@tauri-apps/plugin-updater', () => ({
  check: (...args: unknown[]) => checkMock(...args),
}))
vi.mock('@tauri-apps/plugin-process', () => ({ relaunch: vi.fn() }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => 'auto') }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }))

const toastError = vi.fn()
vi.mock('@/composables/useToast', () => ({
  globalToast: {
    error: (...args: unknown[]) => toastError(...args),
    success: vi.fn(),
    info: vi.fn(),
    warning: vi.fn(),
    show: vi.fn(),
  },
  useToast: () => ({ setToastRef: vi.fn() }),
}))

const fakeUpdate = {
  version: '2.2.0',
  rawJson: {},
  close: vi.fn(async () => {}),
}

/** 取最后一次错误提示文本（toast 只调用一次，索引越界不会发生，这里统一收口断言） */
function lastToastMessage(): string {
  const call = toastError.mock.calls.at(-1)
  return String(call?.[0] ?? '')
}

/** 让内部的重试等待快进，避免用例真的等 2.4s */
async function settle<T>(promise: Promise<T>): Promise<T> {
  for (let i = 0; i < 6; i += 1) await vi.advanceTimersByTimeAsync(3000)
  return promise
}

describe('更新检查的重试与错误分类', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    setActivePinia(createPinia())
    checkMock.mockReset()
    toastError.mockReset()
    // node 环境没有 window；store 用 `'__TAURI_INTERNALS__' in window` 判断是否在 Tauri 里
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    // 断言的是中文文案：CI runner 的 navigator.language 是 en-US，这里显式固定语言
    setLocale('zh-CN')
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('首次失败后重试成功，不应报错', async () => {
    checkMock
      .mockRejectedValueOnce(new Error('Could not fetch a valid release JSON from the remote'))
      .mockResolvedValueOnce(fakeUpdate)
    const { useUpdaterStore } = await import('./updater')
    const store = useUpdaterStore()

    await expect(settle(store.checkForUpdate())).resolves.toBe('2.2.0')
    expect(checkMock).toHaveBeenCalledTimes(2)
    expect(toastError).not.toHaveBeenCalled()
  })

  it('三次都失败时给出带发行页链接的中文提示', async () => {
    checkMock.mockRejectedValue(new Error('Could not fetch a valid release JSON from the remote'))
    const { useUpdaterStore } = await import('./updater')
    const store = useUpdaterStore()

    await expect(settle(store.checkForUpdate())).resolves.toBeNull()
    expect(checkMock).toHaveBeenCalledTimes(3)
    const message = lastToastMessage()
    expect(message).toContain('无法连接更新服务器')
    expect(message).toContain('releases')
  })

  it('清单缺少当前平台条目时提示手动下载', async () => {
    checkMock.mockRejectedValue(
      new Error('the platform `windows-x86_64` was not found in the response `platforms` object'),
    )
    const { useUpdaterStore } = await import('./updater')
    const store = useUpdaterStore()

    await expect(settle(store.checkForUpdate())).resolves.toBeNull()
    const message = lastToastMessage()
    expect(message).toContain('当前平台还没有对应的更新包')
  })

  it('其它错误保留原始信息', async () => {
    checkMock.mockRejectedValue(new Error('signature verification failed'))
    const { useUpdaterStore } = await import('./updater')
    const store = useUpdaterStore()

    await expect(settle(store.checkForUpdate())).resolves.toBeNull()
    expect(lastToastMessage()).toContain('signature verification failed')
  })
})
