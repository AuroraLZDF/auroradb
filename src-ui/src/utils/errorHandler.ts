/**
 * 全局错误兜底的纯逻辑：把任意抛出物转成可展示文案，并对高频重复错误去重。
 *
 * 这里只放不依赖 Vue/Tauri 的纯函数，方便单测；真正的挂载在 `main.ts` 完成。
 * 约束：本文件不得抛错（调用方是错误处理器，再抛错会形成循环）。
 */

/** 展示用最大长度：toast 放不下整段堆栈 */
const DEFAULT_TOAST_MAX = 200

/**
 * 把任意抛出物转成一行文案。
 *
 * - `Error` 取 `message`（空则退回 `name`）
 * - 字符串原样返回
 * - 带 `message` 字段的对象（如 Tauri 的 IPC 错误）取 `message`
 * - 其余尝试 `JSON.stringify`，失败或结果为空再退回 `String()`
 */
export function formatError(error: unknown): string {
  try {
    if (error instanceof Error) {
      return error.message || error.name || 'Error'
    }
    if (typeof error === 'string') return error
    if (error === null || error === undefined) return ''

    if (typeof error === 'object') {
      const withMessage = error as { message?: unknown }
      if (typeof withMessage.message === 'string' && withMessage.message) {
        return withMessage.message
      }
      const json = JSON.stringify(error)
      if (json && json !== '{}' && json !== '[]') return json
    }

    return String(error)
  } catch {
    // 带 getter 抛错/循环引用的对象：退到最保守的描述
    try {
      return Object.prototype.toString.call(error)
    } catch {
      return 'Unknown error'
    }
  }
}

/**
 * 截断超长文案（按字符数，不破坏代理对）。
 * `max` 为总长度上限，超限时末尾以单个 `…` 收尾。
 */
export function truncateForToast(message: string, max = DEFAULT_TOAST_MAX): string {
  const text = message.trim()
  if (max <= 0) return ''
  if (text.length <= max) return text
  // 避免截在代理对中间，产生半个 emoji
  let end = max - 1
  const code = text.charCodeAt(end - 1)
  if (code >= 0xd800 && code <= 0xdbff) end -= 1
  return `${text.slice(0, Math.max(0, end))}…`
}

/**
 * 已知无害的错误：不弹 toast，只在控制台留痕。
 *
 * - `ResizeObserver loop ...`：虚拟滚动/侧栏拖拽改变尺寸时的浏览器警告
 * - `Script error.`：跨域脚本错误，浏览器不提供详情，无法定位也无从修复
 */
export function isIgnorableError(message: string): boolean {
  const text = message.trim()
  return text === 'Script error.' || text.startsWith('ResizeObserver loop')
}

export interface ErrorDeduperOptions {
  /** 同一文案的去重窗口（毫秒），默认 5 秒 */
  windowMs?: number
  /** 时间源，便于单测注入 */
  now?: () => number
  /** 最多记录多少条已上报错误，超出后清理过期项 */
  maxEntries?: number
}

export interface ErrorDeduper {
  /** 该文案在窗口期内是否尚未上报过（true = 应当上报） */
  shouldReport(message: string): boolean
  /** 清空记录（如用户手动重试后） */
  reset(): void
}

/**
 * 创建错误去重器。
 *
 * 用途：渲染循环或反复失败的请求会短时间内产生大量同文案错误，
 * 没有去重会把 toast 刷屏。窗口内相同文案只上报一次。
 */
export function createErrorDeduper(options: ErrorDeduperOptions = {}): ErrorDeduper {
  const windowMs = options.windowMs ?? 5000
  const now = options.now ?? (() => Date.now())
  const maxEntries = options.maxEntries ?? 50

  const lastSeen = new Map<string, number>()

  function shouldReport(message: string): boolean {
    // 空文案统一归为 unknown，避免空 key 互相压制
    const key = message.trim() || 'unknown'
    const timestamp = now()
    const previous = lastSeen.get(key)
    if (previous !== undefined && timestamp - previous < windowMs) return false

    // 控制内存：先删过期项，仍超限则整体清空（保留当前条即可）
    if (lastSeen.size >= maxEntries) {
      for (const [seenKey, seenAt] of lastSeen) {
        if (timestamp - seenAt >= windowMs) lastSeen.delete(seenKey)
      }
      if (lastSeen.size >= maxEntries) lastSeen.clear()
    }

    lastSeen.set(key, timestamp)
    return true
  }

  return {
    shouldReport,
    reset: () => lastSeen.clear(),
  }
}
