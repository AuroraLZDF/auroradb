import { defineStore } from 'pinia'
import { ref, computed } from 'vue'

export interface EditorTabState {
  sql: string
  dirty: boolean
  lastExecutedAt?: number
}

/**
 * SQL 编辑器 tab 内容存储
 *
 * MainContent.vue 负责 tab 元数据（id、title、激活态），本 store 负责内容：
 * SQL 文本、是否有未保存修改、上次执行时间。按 tabId 作 key。
 *
 * 这样拆分是为了后续草稿恢复（P0-3）可以独立持久化内容而不影响 UI 结构。
 */
export const useEditorTabsStore = defineStore('editorTabs', () => {
  const tabs = ref<Record<string, EditorTabState>>({})

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
    delete tabs.value[tabId]
  }

  function reset() {
    tabs.value = {}
  }

  function dirtyCount() {
    return Object.values(tabs.value).filter(t => t.dirty).length
  }

  return {
    tabs,
    ensure,
    setSql,
    getSql,
    markClean,
    markExecuted,
    remove,
    reset,
    dirtyCount: computed(dirtyCount),
  }
})
