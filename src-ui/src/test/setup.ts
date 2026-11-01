import { beforeEach } from 'vitest'

/**
 * node 环境没有 Web Storage API，而 store 的草稿/历史持久化依赖 localStorage。
 * 这里注入一个内存实现，行为与浏览器一致（值统一按字符串存取）。
 */
class MemoryStorage implements Storage {
  private data = new Map<string, string>()

  get length(): number {
    return this.data.size
  }

  clear(): void {
    this.data.clear()
  }

  getItem(key: string): string | null {
    return this.data.get(key) ?? null
  }

  key(index: number): string | null {
    return [...this.data.keys()][index] ?? null
  }

  removeItem(key: string): void {
    this.data.delete(key)
  }

  setItem(key: string, value: string): void {
    this.data.set(key, String(value))
  }
}

if (typeof globalThis.localStorage === 'undefined') {
  Object.defineProperty(globalThis, 'localStorage', {
    value: new MemoryStorage(),
    configurable: true,
  })
}

// 每个用例从干净存储开始，避免上个用例的草稿/历史串味
beforeEach(() => {
  localStorage.clear()
})
