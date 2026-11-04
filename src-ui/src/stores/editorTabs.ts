import { defineStore } from 'pinia'
import { ref, computed, watch } from 'vue'

export interface EditorTabState {
  sql: string
  dirty: boolean
  lastExecutedAt?: number
}

const DRAFTS_KEY = 'auroradb.editor.drafts'
const DRAFTS_VERSION = 1
const MAX_DRAFTS = 30
const DEBOUNCE_MS = 2000

interface DraftPayload {
  version: number
  tabs: Record<string, EditorTabState>
  restoredAt: number
}

/**
 * SQL 编辑器 tab 内容存储 + 草稿持久化
 *
 * MainContent.vue 负责 tab 元数据（id、title、激活态），本 store 负责内容：
 * SQL 文本、是否有未保存修改、上次执行时间。按 tabId 作 key。
 *
 * 变更会以 2 秒 debounce 写入 localStorage（草稿自动保存）；启动时调用
 * restore() 可恢复上次关闭时仍存在的 tab 内容，MainContent.vue 据此重建
 * tab 栏。
 */
export const useEditorTabsStore = defineStore('editorTabs', () => {
  const tabs = ref<Record<string, EditorTabState>>({})
  let saveTimer: ReturnType<typeof setTimeout> | null = null
  /** 待自动执行的 tab（重跑历史等），只存在内存中，避免重启后误执行 */
  const pendingRuns = new Map<string, { params?: Record<string, string> }>()

  function ensure(tabId: string) {
    if (!tabs.value[tabId]) {
      tabs.value[tabId] = { sql: '', dirty: false }
    }
    return tabs.value[tabId]
  }

  function setSql(tabId: string, sql: string) {
    const state = ensure(tabId)
    state.sql = sql
    state.dirty = true
  }

  function getSql(tabId: string): string {
    return tabs.value[tabId]?.sql ?? ''
  }

  function markClean(tabId: string) {
    const state = tabs.value[tabId]
    if (state) state.dirty = false
  }

  function markExecuted(tabId: string) {
    const state = tabs.value[tabId]
    if (state) {
      state.dirty = false
      state.lastExecutedAt = Date.now()
    }
  }

  function remove(tabId: string) {
    pendingRuns.delete(tabId)
    delete tabs.value[tabId]
  }

  /** 安排某个 tab 打开后自动执行一次（可带预设参数，用于一键重跑） */
  function scheduleRun(tabId: string, params?: Record<string, string>) {
    pendingRuns.set(tabId, { params })
  }

  /** 取出并清除待执行标记 */
  function consumePendingRun(tabId: string): { params?: Record<string, string> } | null {
    const pending = pendingRuns.get(tabId)
    if (!pending) return null
    pendingRuns.delete(tabId)
    return pending
  }

  function reset() {
    tabs.value = {}
  }

  function dirtyCount() {
    return Object.values(tabs.value).filter(t => t.dirty).length
  }

  /** 所有有内容的 tabId，按最后活动时间倒序 */
  function restoredTabIds(): string[] {
    return Object.entries(tabs.value)
      .filter(([, v]) => v.sql.trim().length > 0)
      .sort((a, b) => (b[1].lastExecutedAt ?? 0) - (a[1].lastExecutedAt ?? 0))
      .map(([id]) => id)
  }

  /** 立即持久化，debounce 到期会自动调 */
  function saveNow() {
    try {
      const payload: DraftPayload = {
        version: DRAFTS_VERSION,
        tabs: tabs.value,
        restoredAt: Date.now(),
      }
      localStorage.setItem(DRAFTS_KEY, JSON.stringify(payload))
    } catch {
      // localStorage 可能在某些系统/沙箱中被禁用，静默降级
    }
  }

  /** 启动时恢复上次的所有草稿 */
  function restore() {
    try {
      const raw = localStorage.getItem(DRAFTS_KEY)
      if (!raw) return
      const payload = JSON.parse(raw) as DraftPayload
      if (payload.version !== DRAFTS_VERSION) return
      const valid: Record<string, EditorTabState> = {}
      let count = 0
      for (const [id, state] of Object.entries(payload.tabs)) {
        if (!state || typeof state.sql !== 'string' || !state.sql.trim()) continue
        if (count >= MAX_DRAFTS) break
        valid[id] = { sql: state.sql, dirty: false, lastExecutedAt: state.lastExecutedAt }
        count++
      }
      tabs.value = valid
    } catch {
      // 损坏则丢弃
    }
  }

  /** 关闭应用前清掉 localStorage（仅在用户显式关闭主窗口时调） */
  function clearDrafts() {
    try { localStorage.removeItem(DRAFTS_KEY) } catch { /* noop */ }
  }

  // ====== 自动持久化 ======
  watch(
    tabs,
    () => {
      if (saveTimer) clearTimeout(saveTimer)
      saveTimer = setTimeout(saveNow, DEBOUNCE_MS)
    },
    { deep: true }
  )

  return {
    tabs,
    ensure,
    setSql,
    getSql,
    markClean,
    markExecuted,
    scheduleRun,
    consumePendingRun,
    remove,
    reset,
    restore,
    saveNow,
    clearDrafts,
    restoredTabIds,
    dirtyCount: computed(dirtyCount),
  }
})
