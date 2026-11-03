import './assets/main.css'

import { createApp } from 'vue'
import { createPinia } from 'pinia'

import App from './App.vue'
import { useSavedConnectionsStore } from '@/stores/savedConnections'
import { globalToast } from '@/composables/useToast'
import { t } from '@/i18n'
import { createErrorDeduper, formatError, isIgnorableError, truncateForToast } from '@/utils/errorHandler'

// Tauri 2 始终注入 __TAURI_INTERNALS__（与是否开启 withGlobalTauri 无关）
const isTauri = '__TAURI_INTERNALS__' in window

if (!isTauri) {
  console.warn('Not running in Tauri environment. Some features may not work.')
}

// 只禁用 WebView 默认右键菜单（重新加载、检查元素）
// 应用自定义的右键菜单（数据库、表格等）仍可正常使用
if (isTauri && !import.meta.env.DEV) {
  document.addEventListener('contextmenu', (e) => {
    // 如果目标元素在自定义右键菜单内，则不阻止
    const target = e.target as HTMLElement
    if (target.closest('.context-menu')) {
      return
    }
    // 如果点击的是树形节点、表数据行等应用元素，也不阻止
    // 因为应用会通过 @contextmenu.prevent 自行处理
    if (target.closest('.database-header') ||
        target.closest('.object-item') ||
        target.closest('.tree-node') ||
        target.closest('.connection-item') ||
        target.closest('.data-table') ||
        target.closest('.cell-text')) {
      return
    }
    // 其他情况（空白区域）阻止默认右键菜单
    e.preventDefault()
  })
}

const app = createApp(App)

const pinia = createPinia()
app.use(pinia)

// ===== 全局错误兜底 =====
// 同一错误 5 秒内只提示一次，避免渲染循环/重复失败把 toast 刷屏
const shouldReportError = createErrorDeduper({ windowMs: 5000 })

/**
 * 统一错误出口：记录 + 提示。自身绝不抛错——否则会从错误处理器里再抛一次，
 * 形成「错误 → 处理器 → 错误」的循环。
 */
function reportError(error: unknown, source: string) {
  try {
    const message = formatError(error)
    console.error(`[${source}]`, error)
    // 已知无害的错误（ResizeObserver 警告等）只留日志
    if (isIgnorableError(message)) return
    if (!shouldReportError.shouldReport(message)) return
    // Toast 组件尚未挂载时 globalToast 内部会静默忽略，不会因此再报错
    globalToast.error(t('app.unexpectedError', { message: truncateForToast(message) }))
  } catch {
    // 兜底自保：错误处理链路自身失败时不再向外扩散
  }
}

// Vue 组件内的渲染/生命周期/watch 异常
app.config.errorHandler = (error, _instance, info) => {
  reportError(error, `vue:${info}`)
}

// 未被 Vue 捕获的同步异常（如原生事件回调里 throw）
window.addEventListener('error', (event) => {
  reportError(event.error ?? event.message, 'window')
})

// 未处理的 Promise reject（Tauri invoke 漏 catch 时会走到这里）
window.addEventListener('unhandledrejection', (event) => {
  reportError(event.reason, 'promise')
})

// 初始化 savedConnections store，加载 localStorage 数据
const savedStore = useSavedConnectionsStore(pinia)
savedStore.loadFromStorage()

app.mount('#app')
