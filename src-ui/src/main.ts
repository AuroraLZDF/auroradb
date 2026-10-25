import './assets/main.css'

import { createApp } from 'vue'
import { createPinia } from 'pinia'

import App from './App.vue'
import { useSavedConnectionsStore } from '@/stores/savedConnections'

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

// 初始化 savedConnections store，加载 localStorage 数据
const savedStore = useSavedConnectionsStore(pinia)
savedStore.loadFromStorage()

app.mount('#app')
