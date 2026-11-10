<template>
  <div v-if="open" class="history-mask" @click.self="emit('close')">
    <div class="history-dialog" role="dialog" aria-modal="true" :aria-label="t('history.title')">
      <div class="history-header">
        <span class="history-title">{{ t('history.title') }}</span>
        <button class="history-close" @click="emit('close')" aria-label="关闭">×</button>
      </div>

      <div class="history-filters">
        <input v-model="keyword" class="history-search" :placeholder="t('history.searchPlaceholder')" />
        <Select
          v-model="connectionFilter"
          class="history-select"
          size="sm"
          :options="connectionOptions"
          :placeholder="t('history.allConnections')"
        />
        <Select
          v-model="statusFilter"
          class="history-select"
          size="sm"
          :options="statusOptions"
          :placeholder="t('history.allStatus')"
        />
        <span class="history-count">{{ t('history.count', { count: filtered.length }) }}</span>
      </div>

      <div v-if="activeConnections.length === 0" class="history-hint">
        {{ t('history.offlineHint') }}
      </div>

      <div class="history-list">
        <div v-if="filtered.length === 0" class="history-empty">{{ t('history.empty') }}</div>
        <div v-for="entry in filtered" :key="entry.id" class="history-item">
          <div class="history-item-main">
            <div class="history-item-meta">
              <span class="history-status" :class="entry.success ? 'ok' : 'fail'">
                {{ entry.success ? t('history.success') : t('history.failed') }}
              </span>
              <span
                class="history-conn"
                :class="{ closed: !activeFor(entry) }"
                :title="connectionTitle(entry)"
              >
                {{ connectionLabel(entry) }}
              </span>
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
            <button
              class="history-action primary"
              :disabled="!rerunTarget(entry)"
              :title="rerunHint(entry)"
              @click="emit('rerun', entry)"
            >
              {{ t('history.rerun') }}
            </button>
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
import Select from '@/components/ui/Select.vue'
import { useQueryLibraryStore, type QueryHistoryEntry } from '@/stores/queryLibrary'
import { useSavedConnectionsStore } from '@/stores/savedConnections'
import { t } from '@/i18n'

const props = defineProps<{
  open: boolean
  /** 当前已打开的连接，用于判断某条历史能否直接重跑 */
  activeConnections: Array<{ id: string; name: string; host: string; port: number }>
}>()
const emit = defineEmits<{
  (e: 'close'): void
  (e: 'rerun', entry: QueryHistoryEntry): void
}>()

const queryLibrary = useQueryLibraryStore()
const savedConnections = useSavedConnectionsStore()

const keyword = ref('')
const connectionFilter = ref('')
const statusFilter = ref('')

const statusOptions = computed(() => [
  { label: t('history.allStatus'), value: '' },
  { label: t('history.successOnly'), value: 'success' },
  { label: t('history.failedOnly'), value: 'failed' },
])

/** 旧记录没有连接名，单独归成一个筛选分组 */
const LEGACY_GROUP = '__legacy__'

/**
 * 筛选项来自历史自身的连接名，而不是保存的连接表：
 * 历史里的 connectionId 是会话 id，和保存连接的 id 对不上，按 id 过滤永远是空列表。
 */
const connectionOptions = computed(() => {
  const names = new Set<string>()
  for (const item of queryLibrary.history) {
    if (item.connectionName) names.add(item.connectionName)
  }
  const options = [
    { label: t('history.allConnections'), value: '' },
    ...[...names]
      .sort((a, b) => a.localeCompare(b, 'zh-CN'))
      .map(name => ({ label: name, value: name })),
  ]
  if (queryLibrary.history.some(item => !item.connectionName)) {
    options.push({ label: t('history.legacyGroup'), value: LEGACY_GROUP })
  }
  return options
})

const filtered = computed(() => {
  const search = keyword.value.trim().toLowerCase()
  return queryLibrary.history.filter(entry => {
    if (connectionFilter.value === LEGACY_GROUP) {
      if (entry.connectionName) return false
    } else if (connectionFilter.value && entry.connectionName !== connectionFilter.value) {
      return false
    }
    if (statusFilter.value === 'success' && !entry.success) return false
    if (statusFilter.value === 'failed' && entry.success) return false
    if (search && !entry.sql.toLowerCase().includes(search)) return false
    return true
  })
})

function activeFor(entry: QueryHistoryEntry) {
  return props.activeConnections.find(conn => conn.id === entry.connectionId)
}

/** 这条历史对应的「保存的连接」：优先按 id，其次按名字 + 地址重新定位 */
function savedFor(entry: QueryHistoryEntry) {
  const list = savedConnections.savedConnections
  if (entry.connectionId) {
    const byId = list.find(conn => conn.id === entry.connectionId)
    if (byId) return byId
  }
  if (entry.connectionName) {
    return list.find(
      conn =>
        conn.name === entry.connectionName &&
        conn.host === entry.connectionHost &&
        conn.port === entry.connectionPort
    )
  }
  return undefined
}

function rerunTarget(entry: QueryHistoryEntry) {
  return activeFor(entry) ?? savedFor(entry)
}

function rerunHint(entry: QueryHistoryEntry): string {
  if (activeFor(entry)) return t('history.rerun')
  if (savedFor(entry)) return t('history.rerunReconnect')
  // 没有连接名说明是旧版本写入的记录，和「连接被删掉」不是一回事
  if (!entry.connectionName) return t('history.rerunLegacy')
  return t('history.rerunUnavailable')
}

/** 连接名：记录里的名字 → 当前活动连接 → 保存的连接；都拿不到就显示破折号，不要满屏「未知连接」 */
function connectionLabel(entry: QueryHistoryEntry): string {
  return entry.connectionName || activeFor(entry)?.name || savedFor(entry)?.name || '—'
}

/** 旧记录说不出是哪个连接，用 tooltip 解释清楚，避免误以为连接丢了 */
function connectionTitle(entry: QueryHistoryEntry): string | undefined {
  if (connectionLabel(entry) !== '—') return undefined
  return t('history.legacyNoConnection')
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
  background: var(--bg-overlay);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: var(--z-modal);
}

.history-dialog {
  position: relative;
  width: 820px;
  max-width: 92vw;
  max-height: 82vh;
  display: flex;
  flex-direction: column;
  background: var(--overlay);
  border: 1px solid var(--hairline-strong);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-xl);
}

.history-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: var(--toolbar-height);
  padding: 0 var(--space-3);
  border-bottom: 1px solid var(--hairline);
}

.history-title {
  font-weight: var(--font-semibold);
  font-size: var(--text-sm);
  color: var(--text-primary);
}

.history-close {
  background: none;
  border: none;
  width: var(--control-height-sm);
  height: var(--control-height-sm);
  border-radius: var(--radius-sm);
  color: var(--text-secondary);
  font-size: 16px;
  line-height: 1;
  cursor: pointer;
  transition: var(--transition-colors);
}

.history-close:hover {
  background: var(--hover-fill);
  color: var(--text-primary);
}

.history-filters {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--hairline);
}

.history-search {
  flex: 1;
  height: var(--control-height-sm);
  padding: 0 var(--space-2);
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  background: var(--raised);
  color: var(--text-primary);
  font-size: var(--text-xs);
  outline: none;
  transition: var(--transition-colors);
}

.history-search:focus {
  background: var(--canvas);
  border-color: var(--brand-500);
}

.history-select {
  /* CustomSelect 自身是 width:100%，这里要收回成内容宽度 */
  width: auto;
  flex: 0 0 auto;
  min-width: 150px;
}

.history-count {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  white-space: nowrap;
}

.history-hint {
  margin: 0 var(--space-3) var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-sm);
  background: var(--hover-fill);
  color: var(--text-secondary);
  font-size: var(--text-xs);
  line-height: 1.5;
}

.history-list {
  flex: 1;
  overflow: auto;
  padding: var(--space-1) var(--space-2);
}

.history-empty {
  padding: var(--space-6) 0;
  text-align: center;
  color: var(--text-tertiary);
  font-size: var(--text-sm);
}

.history-item {
  display: flex;
  gap: var(--space-3);
  padding: var(--space-2) var(--space-2);
  border-bottom: 1px solid var(--hairline);
  border-radius: var(--radius-sm);
}

.history-item:hover {
  background: var(--hover-fill);
}

.history-item-main {
  flex: 1;
  min-width: 0;
}

.history-item-meta {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: var(--space-2);
  font-size: var(--text-xs);
  color: var(--text-secondary);
}

.history-status {
  font-weight: var(--font-medium);
}

.history-status.ok {
  color: var(--success-500);
}

.history-status.fail {
  color: var(--error-500);
}

.history-conn.closed {
  color: var(--text-tertiary);
}

.history-conn {
  color: var(--accent);
}

.history-stat {
  font-variant-numeric: tabular-nums;
}

.history-sql {
  margin: 4px 0 0;
  font-family: var(--font-mono);
  font-size: var(--text-xs);
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 3.6em;
  overflow: hidden;
  color: var(--text-primary);
}

.history-params {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 4px;
}

.history-param {
  font-family: var(--font-mono);
  font-size: 10px;
  padding: 0 5px;
  border-radius: var(--radius-xs);
  background: var(--hover-fill);
  color: var(--text-secondary);
}

.history-error {
  margin-top: 4px;
  font-size: var(--text-xs);
  color: var(--error-500);
}

.history-item-actions {
  display: flex;
  flex-direction: column;
  gap: 6px;
  flex-shrink: 0;
}

.history-action {
  height: var(--control-height-sm);
  padding: 0 var(--space-2);
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--text-xs);
  cursor: pointer;
  transition: var(--transition-colors);
}

.history-action:hover {
  background: var(--hover-fill);
  color: var(--text-primary);
}

.history-action.primary {
  background: var(--brand-500);
  color: #fff;
}

.history-action.primary:hover:not(:disabled) {
  background: var(--brand-600);
  color: #fff;
}

.history-action:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.history-footer {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-top: 1px solid var(--hairline);
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
