<template>
  <div v-if="open" class="history-mask" @click.self="emit('close')">
    <div class="history-dialog">
      <div class="history-header">
        <span class="history-title">{{ t('history.title') }}</span>
        <button class="history-close" @click="emit('close')">×</button>
      </div>

      <div class="history-filters">
        <input v-model="keyword" class="history-search" :placeholder="t('history.searchPlaceholder')" />
        <select v-model="connectionFilter" class="history-select">
          <option value="">{{ t('history.allConnections') }}</option>
          <option v-for="conn in connectionOptions" :key="conn.id" :value="conn.id">
            {{ conn.name }}
          </option>
        </select>
        <select v-model="statusFilter" class="history-select">
          <option value="">{{ t('history.allStatus') }}</option>
          <option value="success">{{ t('history.successOnly') }}</option>
          <option value="failed">{{ t('history.failedOnly') }}</option>
        </select>
        <span class="history-count">{{ t('history.count', { count: filtered.length }) }}</span>
      </div>

      <div class="history-list">
        <div v-if="filtered.length === 0" class="history-empty">{{ t('history.empty') }}</div>
        <div v-for="entry in filtered" :key="entry.id" class="history-item">
          <div class="history-item-main">
            <div class="history-item-meta">
              <span class="history-status" :class="entry.success ? 'ok' : 'fail'">
                {{ entry.success ? t('history.success') : t('history.failed') }}
              </span>
              <span class="history-conn">{{ connectionName(entry.connectionId) }}</span>
              <span v-if="entry.database" class="history-db">{{ entry.database }}</span>
              <span class="history-time">{{ formatTime(entry.executedAt) }}</span>
              <span v-if="entry.durationMs != null" class="history-stat">
                {{ entry.durationMs }} ms
              </span>
              <span v-if="entry.rowsReturned != null" class="history-stat">
                {{ t('history.rows', { count: entry.rowsReturned }) }}
              </span>
              <span v-else-if="entry.rowsAffected != null" class="history-stat">
                {{ t('history.affected', { count: entry.rowsAffected }) }}
              </span>
            </div>
            <pre class="history-sql" :title="entry.sql">{{ entry.sql }}</pre>
            <div v-if="entry.params && Object.keys(entry.params).length > 0" class="history-params">
              <span v-for="(value, key) in entry.params" :key="key" class="history-param">
                :{{ key }} = {{ value }}
              </span>
            </div>
            <div v-if="entry.error" class="history-error">{{ entry.error }}</div>
          </div>
          <div class="history-item-actions">
            <button class="history-action" @click="copySql(entry.sql)">{{ t('common.copy') }}</button>
            <button class="history-action primary" @click="emit('rerun', entry)">{{ t('history.rerun') }}</button>
          </div>
        </div>
      </div>

      <div class="history-footer">
        <button class="history-action" :disabled="queryLibrary.history.length === 0" @click="clearAll">
          {{ t('history.clearAll') }}
        </button>
      </div>
      <div v-if="notice" class="history-notice">{{ notice }}</div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { useQueryLibraryStore, type QueryHistoryEntry } from '@/stores/queryLibrary'
import { useSavedConnectionsStore } from '@/stores/savedConnections'
import { t } from '@/i18n'

defineProps<{ open: boolean }>()
const emit = defineEmits<{
  (e: 'close'): void
  (e: 'rerun', entry: QueryHistoryEntry): void
}>()

const queryLibrary = useQueryLibraryStore()
const savedConnections = useSavedConnectionsStore()

const keyword = ref('')
const connectionFilter = ref('')
const statusFilter = ref('')

const connectionOptions = computed(() => {
  const ids = new Set(queryLibrary.history.map(item => item.connectionId))
  return savedConnections.savedConnections
    .filter(conn => ids.has(conn.id))
    .map(conn => ({ id: conn.id, name: conn.name }))
})

const filtered = computed(() => {
  const search = keyword.value.trim().toLowerCase()
  return queryLibrary.history.filter(entry => {
    if (connectionFilter.value && entry.connectionId !== connectionFilter.value) return false
    if (statusFilter.value === 'success' && !entry.success) return false
    if (statusFilter.value === 'failed' && entry.success) return false
    if (search && !entry.sql.toLowerCase().includes(search)) return false
    return true
  })
})

function connectionName(connectionId: string): string {
  return (
    savedConnections.savedConnections.find(conn => conn.id === connectionId)?.name ?? connectionId
  )
}

function formatTime(value: string): string {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return date.toLocaleString()
}

async function copySql(sql: string) {
  try {
    await navigator.clipboard.writeText(sql)
    showNotice(t('history.copied'))
  } catch {
    showNotice(t('history.copyFailed'))
  }
}

function clearAll() {
  queryLibrary.clearHistory()
  showNotice(t('history.cleared'))
}

const notice = ref('')
let noticeTimer: ReturnType<typeof setTimeout> | null = null
function showNotice(message: string) {
  notice.value = message
  if (noticeTimer) clearTimeout(noticeTimer)
  noticeTimer = setTimeout(() => {
    notice.value = ''
  }, 2000)
}
</script>

<style scoped>
.history-mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.35);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 2000;
}

.history-dialog {
  position: relative;
  width: 820px;
  max-width: 92vw;
  max-height: 82vh;
  display: flex;
  flex-direction: column;
  background: var(--surface-default, var(--color-background));
  border: 1px solid var(--border-default, var(--color-border));
  border-radius: 8px;
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.25);
}

.history-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 16px;
  border-bottom: 1px solid var(--border-default, var(--color-border));
}

.history-title {
  font-weight: 600;
  font-size: 14px;
}

.history-close {
  background: none;
  border: none;
  color: var(--text-secondary, var(--color-text-secondary));
  font-size: 18px;
  line-height: 1;
  cursor: pointer;
}

.history-filters {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 16px;
  border-bottom: 1px solid var(--border-default, var(--color-border));
}

.history-search {
  flex: 1;
  padding: 6px 8px;
  border: 1px solid var(--border-default, var(--color-border));
  border-radius: 4px;
  background: var(--bg-secondary, var(--color-background-soft));
  color: var(--text-primary, var(--color-text));
}

.history-select {
  padding: 6px 8px;
  border: 1px solid var(--border-default, var(--color-border));
  border-radius: 4px;
  background: var(--bg-secondary, var(--color-background-soft));
  color: var(--text-primary, var(--color-text));
}

.history-count {
  font-size: 12px;
  color: var(--text-secondary, var(--color-text-secondary));
  white-space: nowrap;
}

.history-list {
  flex: 1;
  overflow: auto;
  padding: 8px 16px;
}

.history-empty {
  padding: 40px 0;
  text-align: center;
  color: var(--text-secondary, var(--color-text-secondary));
  font-size: 13px;
}

.history-item {
  display: flex;
  gap: 12px;
  padding: 10px 0;
  border-bottom: 1px solid var(--border-default, var(--color-border));
}

.history-item-main {
  flex: 1;
  min-width: 0;
}

.history-item-meta {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
  font-size: 12px;
  color: var(--text-secondary, var(--color-text-secondary));
}

.history-status {
  font-weight: 600;
}

.history-status.ok {
  color: var(--color-success, #2e7d32);
}

.history-status.fail {
  color: var(--color-danger, #c62828);
}

.history-conn {
  color: var(--color-primary);
}

.history-stat {
  font-variant-numeric: tabular-nums;
}

.history-sql {
  margin: 6px 0 0;
  font-family: monospace;
  font-size: 12px;
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 3.6em;
  overflow: hidden;
  color: var(--text-primary, var(--color-text));
}

.history-params {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 4px;
}

.history-param {
  font-family: monospace;
  font-size: 11px;
  padding: 1px 6px;
  border-radius: 3px;
  background: var(--surface-hover, rgba(0, 0, 0, 0.06));
  color: var(--text-secondary, var(--color-text-secondary));
}

.history-error {
  margin-top: 4px;
  font-size: 12px;
  color: var(--color-danger, #c62828);
}

.history-item-actions {
  display: flex;
  flex-direction: column;
  gap: 6px;
  flex-shrink: 0;
}

.history-action {
  padding: 4px 10px;
  border: 1px solid var(--border-default, var(--color-border));
  border-radius: 4px;
  background: transparent;
  color: var(--text-primary, var(--color-text));
  font-size: 12px;
  cursor: pointer;
}

.history-action.primary {
  background: var(--color-primary);
  border-color: var(--color-primary);
  color: #fff;
}

.history-action:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.history-footer {
  display: flex;
  justify-content: flex-end;
  padding: 10px 16px;
  border-top: 1px solid var(--border-default, var(--color-border));
}

.history-notice {
  position: absolute;
  left: 50%;
  bottom: 64px;
  transform: translateX(-50%);
  padding: 6px 14px;
  border-radius: 4px;
  background: rgba(0, 0, 0, 0.75);
  color: #fff;
  font-size: 12px;
}
</style>
