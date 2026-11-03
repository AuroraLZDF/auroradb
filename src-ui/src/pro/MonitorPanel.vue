<template>
  <div class="monitor-panel">
    <!-- 头部 -->
    <div class="panel-header">
      <div class="header-title">
        <Activity :size="18" class="title-icon" />
        <h2>{{ t('monitor.title') }}</h2>
        <span class="header-hint">{{ t('monitor.hint') }}</span>
      </div>
      <button class="icon-btn" :title="t('common.close')" @click="emit('close')">
        <X :size="18" />
      </button>
    </div>

    <!-- 标签页 -->
    <div class="tab-bar">
      <button
        class="tab-btn"
        :class="{ active: tab === 'process' }"
        @click="tab = 'process'"
      >
        <Activity :size="14" /> {{ t('monitor.tabProcess') }}
      </button>
      <button
        class="tab-btn"
        :class="{ active: tab === 'audit' }"
        @click="tab = 'audit'"
      >
        <ScrollText :size="14" /> {{ t('monitor.tabAudit') }}
      </button>
    </div>

    <!-- 进程 -->
    <div v-if="tab === 'process'" class="tab-body">
      <div class="toolbar">
        <div class="toolbar-field">
          <span class="field-label">{{ t('common.connection') }}</span>
          <Select
            v-model="processConnectionId"
            :options="connectionOptions"
            :placeholder="t('monitor.selectConnection')"
          />
        </div>
        <label class="auto-refresh">
          <input v-model="autoRefresh" type="checkbox" />
          <span>{{ t('monitor.autoRefresh') }}</span>
        </label>
        <Button size="sm" :icon="RefreshCw" :loading="processLoading" @click="refreshProcesses()">
          {{ t('common.refresh') }}
        </Button>
        <span v-if="metrics" class="version-hint">MySQL {{ metrics.version }}</span>
      </div>

      <div v-if="processError" class="alert error">
        <AlertCircle :size="16" />
        <span>{{ processError }}</span>
      </div>

      <div v-if="metrics" class="metrics-grid">
        <div class="metric-card">
          <span class="metric-label">{{ t('monitor.metricQps') }}</span>
          <span class="metric-value">{{ metrics.qps.toFixed(2) }}</span>
          <span class="metric-sub">{{ t('monitor.metricQpsSub') }}</span>
        </div>
        <div class="metric-card">
          <span class="metric-label">{{ t('monitor.metricConnections') }}</span>
          <span class="metric-value">
            {{ metrics.threads_connected }}<span class="metric-dim"> / {{ metrics.max_connections }}</span>
          </span>
          <span class="metric-sub">{{ t('monitor.metricConnectionsSub') }}</span>
        </div>
        <div class="metric-card">
          <span class="metric-label">{{ t('monitor.metricThreadsRunning') }}</span>
          <span class="metric-value">{{ metrics.threads_running }}</span>
          <span class="metric-sub">{{ t('monitor.metricThreadsRunningSub') }}</span>
        </div>
        <div class="metric-card">
          <span class="metric-label">{{ t('monitor.metricUptime') }}</span>
          <span class="metric-value">{{ formatUptime(metrics.uptime_seconds) }}</span>
          <span class="metric-sub">{{ t('monitor.metricUptimeSub') }}</span>
        </div>
        <div class="metric-card">
          <span class="metric-label">{{ t('monitor.metricSlowQueries') }}</span>
          <span class="metric-value">{{ metrics.slow_queries }}</span>
          <span class="metric-sub">
            {{ t('monitor.slowQuerySub', {
              state: metrics.slow_query_log ? t('monitor.slowLogOn') : t('monitor.slowLogOff'),
              seconds: metrics.long_query_time
            }) }}
          </span>
        </div>
        <div class="metric-card">
          <span class="metric-label">{{ t('monitor.metricBufferPool') }}</span>
          <span class="metric-value">
            {{ metrics.buffer_pool_hit_rate === null ? '—' : metrics.buffer_pool_hit_rate.toFixed(2) + '%' }}
          </span>
          <span class="metric-sub">InnoDB Buffer Pool</span>
        </div>
        <div class="metric-card">
          <span class="metric-label">{{ t('monitor.metricAbortedConnects') }}</span>
          <span class="metric-value">{{ metrics.aborted_connects }}</span>
          <span class="metric-sub">Aborted connects</span>
        </div>
        <div class="metric-card">
          <span class="metric-label">{{ t('monitor.metricReadWrite') }}</span>
          <span class="metric-value read-write">
            <span class="rw read">S {{ metrics.com_select }}</span>
            <span class="rw write">
              I {{ metrics.com_insert }} / U {{ metrics.com_update }} / D {{ metrics.com_delete }}
            </span>
          </span>
          <span class="metric-sub">COM_SELECT / INSERT / UPDATE / DELETE</span>
        </div>
      </div>

      <div class="table-wrap">
        <table class="data-table">
          <thead>
            <tr>
              <th class="col-id">{{ t('monitor.colId') }}</th>
              <th>{{ t('monitor.colUser') }}</th>
              <th>{{ t('monitor.colHost') }}</th>
              <th>{{ t('common.database') }}</th>
              <th>{{ t('monitor.colCommand') }}</th>
              <th class="col-num">{{ t('monitor.colTimeSeconds') }}</th>
              <th>{{ t('common.status') }}</th>
              <th>{{ t('monitor.colSql') }}</th>
              <th class="col-action">{{ t('common.actions') }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="process in processes" :key="process.id">
              <td class="col-id mono">{{ process.id }}</td>
              <td>{{ process.user }}</td>
              <td class="mono">{{ process.host }}</td>
              <td>{{ process.database ?? '—' }}</td>
              <td>
                <span class="command-chip" :class="{ sleeping: process.command === 'Sleep' }">
                  {{ process.command }}
                </span>
              </td>
              <td class="col-num">{{ process.time }}</td>
              <td class="state">{{ process.state ?? '—' }}</td>
              <td class="sql-cell mono" :title="process.info ?? ''">{{ process.info ?? '—' }}</td>
              <td class="col-action">
                <Button
                  size="xs"
                  variant="danger"
                  :loading="killingIds.has(process.id)"
                  @click="terminate(process)"
                >{{ t('monitor.actionKill') }}</Button>
              </td>
            </tr>
            <tr v-if="processes.length === 0">
              <td colspan="9" class="empty-cell">
                {{ processLoading ? t('common.loading') : t('monitor.noSessions') }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>

    <!-- 审计日志 -->
    <div v-else class="tab-body">
      <div class="toolbar">
        <div class="toolbar-field">
          <span class="field-label">{{ t('common.connection') }}</span>
          <Select
            v-model="auditConnectionId"
            :options="auditConnectionOptions"
            :placeholder="t('monitor.allConnections')"
          />
        </div>
        <div class="toolbar-field">
          <span class="field-label">{{ t('monitor.result') }}</span>
          <Select
            v-model="auditStatus"
            :options="statusOptions"
            :placeholder="t('common.all')"
          />
        </div>
        <div class="toolbar-field grow">
          <span class="field-label">{{ t('monitor.keyword') }}</span>
          <input
            v-model.trim="auditKeyword"
            class="text-input"
            type="text"
            :placeholder="t('monitor.keywordPlaceholder')"
            @keyup.enter="loadAuditEntries"
          />
        </div>
        <Button size="sm" :icon="Search" :loading="auditLoading" @click="loadAuditEntries">
          {{ t('monitor.query') }}
        </Button>
        <Button size="sm" :icon="Download" :disabled="auditEntries.length === 0" @click="exportAudit('csv')">
          CSV
        </Button>
        <Button size="sm" :icon="Download" :disabled="auditEntries.length === 0" @click="exportAudit('json')">
          JSON
        </Button>
        <Button
          size="sm"
          variant="danger"
          :icon="Trash2"
          :loading="clearingAudit"
          @click="clearAudit"
        >
          {{ t('common.clear') }}
        </Button>
      </div>

      <div v-if="auditError" class="alert error">
        <AlertCircle :size="16" />
        <span>{{ auditError }}</span>
      </div>
      <div v-if="auditMessage" class="alert success">
        <CheckCircle2 :size="16" />
        <span>{{ auditMessage }}</span>
      </div>

      <div class="table-wrap">
        <table class="data-table audit-table">
          <thead>
            <tr>
              <th class="col-time">{{ t('monitor.colTime') }}</th>
              <th>{{ t('common.connection') }}</th>
              <th>{{ t('common.database') }}</th>
              <th>{{ t('common.type') }}</th>
              <th>{{ t('monitor.colSql') }}</th>
              <th class="col-num">{{ t('monitor.colDuration') }}</th>
              <th class="col-num">{{ t('monitor.colRowsAffected') }}</th>
              <th class="col-result">{{ t('monitor.result') }}</th>
            </tr>
          </thead>
          <tbody>
            <template v-for="entry in auditEntries" :key="entry.id">
              <tr
                class="audit-row"
                :class="{ failed: !entry.success, expanded: expandedId === entry.id }"
                @click="expandedId = expandedId === entry.id ? '' : entry.id"
              >
                <td class="col-time mono">{{ formatTime(entry.timestamp) }}</td>
                <td :title="entry.host">{{ entry.connection_name || entry.connection_id }}</td>
                <td>{{ entry.database || '—' }}</td>
                <td>
                  <span class="kind-chip" :class="entry.kind">{{ kindLabel(entry.kind) }}</span>
                </td>
                <td class="sql-cell mono" :title="entry.sql">{{ entry.sql }}</td>
                <td class="col-num">{{ entry.duration_ms }} ms</td>
                <td class="col-num">{{ entry.rows_affected ?? entry.rows_returned }}</td>
                <td class="col-result">
                  <span v-if="entry.success" class="status-ok">{{ t('common.success') }}</span>
                  <span v-else class="status-failed">{{ t('common.failed') }}</span>
                </td>
              </tr>
              <tr v-if="expandedId === entry.id" class="audit-detail">
                <td colspan="8">
                  <pre class="sql-detail">{{ entry.sql }}</pre>
                  <div v-if="entry.error" class="detail-error">{{ t('monitor.detailError', { message: entry.error }) }}</div>
                </td>
              </tr>
            </template>
            <tr v-if="auditEntries.length === 0">
              <td colspan="8" class="empty-cell">
                {{ auditLoading ? t('common.loading') : t('monitor.auditEmpty') }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import {
  Activity,
  AlertCircle,
  CheckCircle2,
  Download,
  RefreshCw,
  ScrollText,
  Search,
  Trash2,
  X
} from 'lucide-vue-next'
import Select from '@/components/ui/Select.vue'
import Button from '@/components/ui/Button.vue'
import {
  useDataBrowserStore,
  type AuditEntry,
  type ProcessInfo,
  type ServerMetrics
} from '@/stores/dataBrowser'
import type { ConnectionStatus } from '@/stores/connection'
import { useLicenseStore } from '@/stores/license'
import { confirmDanger } from '@/composables/useConfirm'
import { t } from '@/i18n'

const props = withDefaults(defineProps<{
  connections: ConnectionStatus[]
  initialConnectionId?: string
}>(), {
  initialConnectionId: ''
})

const emit = defineEmits<{
  close: []
}>()

const dataBrowser = useDataBrowserStore()
const licenseStore = useLicenseStore()

const tab = ref<'process' | 'audit'>('process')
const connectionOptions = computed(() =>
  props.connections.map((connection) => ({
    label: `${connection.name} (${connection.host}:${connection.port})`,
    value: connection.id
  }))
)

// ---------- 进程 ----------
const processConnectionId = ref(props.initialConnectionId || props.connections[0]?.id || '')
const processes = ref<ProcessInfo[]>([])
const metrics = ref<ServerMetrics | null>(null)
const processLoading = ref(false)
const processError = ref('')
const autoRefresh = ref(true)
/** 正在执行 kill 的会话 id，避免同一行被重复点击 */
const killingIds = ref<Set<number>>(new Set())

let refreshTimer: ReturnType<typeof setInterval> | undefined
/** 请求序号：切换连接或发起新请求后，旧请求的结果必须丢弃 */
let processSeq = 0

async function refreshProcesses(silent = false) {
  const seq = ++processSeq
  if (!processConnectionId.value) {
    processes.value = []
    metrics.value = null
    processError.value = ''
    processLoading.value = false
    return
  }
  if (!silent) processLoading.value = true
  try {
    const [list, status] = await Promise.all([
      dataBrowser.getProcessList(processConnectionId.value),
      dataBrowser.getServerMetrics(processConnectionId.value)
    ])
    // 已有更新的请求（切换连接 / 手动刷新）时丢弃这次的旧结果
    if (seq !== processSeq) return
    processes.value = list
    metrics.value = status
    processError.value = ''
  } catch (err) {
    if (seq !== processSeq) return
    // 自动刷新失败也必须提示：否则界面会一直展示过期数据，用户无从察觉
    if (!silent) processes.value = []
    processError.value = t('monitor.loadFailed', { message: String(err) })
  } finally {
    if (seq === processSeq && !silent) processLoading.value = false
  }
}

async function terminate(process: ProcessInfo) {
  if (killingIds.value.has(process.id)) return
  const confirmed = await confirmDanger({
    message: t('monitor.confirmKill', { id: process.id, user: process.user, host: process.host }),
    confirmText: t('monitor.actionKill'),
  })
  if (!confirmed) return
  killingIds.value.add(process.id)
  try {
    await dataBrowser.killProcess(processConnectionId.value, process.id)
    await refreshProcesses(true)
  } catch (err) {
    processError.value = t('monitor.killFailed', { message: String(err) })
  } finally {
    killingIds.value.delete(process.id)
  }
}

function stopTimer() {
  if (refreshTimer) {
    clearInterval(refreshTimer)
    refreshTimer = undefined
  }
}

function startTimer() {
  stopTimer()
  if (tab.value !== 'process' || !autoRefresh.value || !processConnectionId.value) return
  refreshTimer = setInterval(() => {
    void refreshProcesses(true)
  }, 5000)
}

watch([tab, autoRefresh, processConnectionId], () => {
  startTimer()
})

watch(processConnectionId, (id) => {
  if (!id) {
    processes.value = []
    metrics.value = null
    return
  }
  void refreshProcesses()
}, { immediate: true })

onUnmounted(() => {
  stopTimer()
})

function formatUptime(seconds: number): string {
  const days = Math.floor(seconds / 86400)
  const hours = Math.floor((seconds % 86400) / 3600)
  const minutes = Math.floor((seconds % 3600) / 60)
  if (days > 0) return `${days}d ${hours}h`
  if (hours > 0) return `${hours}h ${minutes}m`
  return `${minutes}m`
}

// ---------- 审计 ----------
const auditEntries = ref<AuditEntry[]>([])
const auditLoading = ref(false)
const clearingAudit = ref(false)
const auditError = ref('')
const auditMessage = ref('')
const auditConnectionId = ref('')
const auditStatus = ref<'success' | 'failed' | ''>('')
const auditKeyword = ref('')
const expandedId = ref('')

const auditConnectionOptions = computed(() => [
  { label: t('monitor.allConnections'), value: '' },
  ...connectionOptions.value
])

const statusOptions = computed(() => [
  { label: t('common.all'), value: '' },
  { label: t('common.success'), value: 'success' },
  { label: t('common.failed'), value: 'failed' }
])

/** 审计查询序号：连续点击查询时只接受最后一次的结果 */
let auditSeq = 0

async function loadAuditEntries() {
  const seq = ++auditSeq
  auditLoading.value = true
  auditError.value = ''
  auditMessage.value = ''
  try {
    const entries = await dataBrowser.listAuditEntries({
      limit: 200,
      connectionId: auditConnectionId.value || undefined,
      success: auditStatus.value === '' ? undefined : auditStatus.value === 'success',
      keyword: auditKeyword.value || undefined
    })
    if (seq !== auditSeq) return
    auditEntries.value = entries
  } catch (err) {
    if (seq !== auditSeq) return
    auditEntries.value = []
    if (licenseStore.handleError(err)) {
      auditError.value = ''
      return
    }
    auditError.value = t('monitor.auditLoadFailed', { message: String(err) })
  } finally {
    if (seq === auditSeq) auditLoading.value = false
  }
}

async function clearAudit() {
  if (clearingAudit.value) return
  const confirmed = await confirmDanger({
    message: t('monitor.confirmClearAudit'),
    confirmText: t('common.clear'),
  })
  if (!confirmed) return
  clearingAudit.value = true
  try {
    const files = await dataBrowser.clearAuditLogs()
    auditEntries.value = []
    auditMessage.value = t('monitor.auditCleared', { count: files })
  } catch (err) {
    if (licenseStore.handleError(err)) return
    auditError.value = t('monitor.clearAuditFailed', { message: String(err) })
  } finally {
    clearingAudit.value = false
  }
}

async function exportAudit(format: 'csv' | 'json') {
  if (auditEntries.value.length === 0) return
  try {
    const content = format === 'csv' ? toCsv(auditEntries.value) : JSON.stringify(auditEntries.value, null, 2)
    const stamp = new Date().toISOString().slice(0, 10)
    await invoke('save_file_to_dialog', {
      content,
      defaultFilename: `audit-${stamp}.${format}`,
      format
    })
    auditMessage.value = t('monitor.auditExported', { count: auditEntries.value.length })
  } catch (err) {
    const text = String(err)
    if (!text.includes('用户取消')) {
      auditError.value = t('monitor.exportFailed', { message: text })
    }
  }
}

function toCsv(entries: AuditEntry[]): string {
  const header = [
    t('monitor.colTime'),
    t('common.connection'),
    t('monitor.csvHost'),
    t('common.database'),
    t('common.type'),
    t('monitor.colSql'),
    t('monitor.csvDuration'),
    t('monitor.colRowsAffected'),
    t('monitor.colRowsReturned'),
    t('monitor.result'),
    t('monitor.colError')
  ]
  const escape = (value: unknown) => `"${String(value ?? '').replace(/"/g, '""')}"`
  const lines = entries.map((entry) => [
    entry.timestamp,
    entry.connection_name || entry.connection_id,
    entry.host,
    entry.database,
    kindLabel(entry.kind),
    entry.sql,
    entry.duration_ms,
    entry.rows_affected ?? '',
    entry.rows_returned,
    entry.success ? t('common.success') : t('common.failed'),
    entry.error ?? ''
  ].map(escape).join(','))
  return [header.map(escape).join(','), ...lines].join('\n')
}

function kindLabel(kind: string): string {
  if (kind === 'table-edit') return t('monitor.kindTableEdit')
  if (kind === 'import') return t('monitor.kindImport')
  return t('monitor.kindQuery')
}

function formatTime(timestamp: string): string {
  const date = new Date(timestamp)
  if (Number.isNaN(date.getTime())) return timestamp
  const pad = (value: number) => String(value).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ` +
    `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
}

// 切到审计页时按需加载一次
watch(tab, (value) => {
  if (value === 'audit' && auditEntries.value.length === 0 && !auditLoading.value) {
    void loadAuditEntries()
  }
})

startTimer()
</script>

<style scoped>
.monitor-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  background: var(--surface-default);
  color: var(--text-primary);
}

/* 头部 */
.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  padding: var(--space-4) var(--space-5);
  border-bottom: 1px solid var(--border-subtle);
  flex-shrink: 0;
}

.header-title {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  min-width: 0;
}

.title-icon {
  color: var(--brand-500);
}

.header-title h2 {
  margin: 0;
  font-size: var(--text-lg);
  font-weight: var(--font-semibold);
  white-space: nowrap;
}

.header-hint {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.icon-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: none;
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  transition: var(--transition-all);
  flex-shrink: 0;
}

.icon-btn:hover {
  background: var(--surface-hover);
  color: var(--text-primary);
}

/* 标签页 */
.tab-bar {
  display: flex;
  gap: var(--space-1);
  padding: 0 var(--space-5);
  border-bottom: 1px solid var(--border-subtle);
  flex-shrink: 0;
}

.tab-btn {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-3) var(--space-3);
  border: none;
  border-bottom: 2px solid transparent;
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--text-sm);
  cursor: pointer;
  transition: var(--transition-colors);
}

.tab-btn:hover {
  color: var(--text-primary);
}

.tab-btn.active {
  color: var(--brand-500);
  border-bottom-color: var(--brand-500);
  font-weight: var(--font-medium);
}

.tab-body {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-height: 0;
  overflow: hidden;
}

/* 工具栏 */
.toolbar {
  display: flex;
  align-items: flex-end;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-5);
  border-bottom: 1px solid var(--border-subtle);
  flex-wrap: wrap;
  flex-shrink: 0;
}

.toolbar-field {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  min-width: 180px;
}

.toolbar-field.grow {
  flex: 1;
}

.field-label {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.text-input {
  height: 34px;
  padding: 0 var(--space-3);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  background: var(--surface-default);
  color: var(--text-primary);
  font-size: var(--text-sm);
  outline: none;
}

.text-input:focus {
  border-color: var(--brand-500);
}

.auto-refresh {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin-bottom: 8px;
  font-size: var(--text-sm);
  color: var(--text-secondary);
  cursor: pointer;
  white-space: nowrap;
}

.version-hint {
  margin-bottom: 8px;
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

/* 提示 */
.alert {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin: var(--space-3) var(--space-5) 0;
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  flex-shrink: 0;
}

.alert.error {
  border: 1px solid var(--error-500);
  background: rgba(239, 68, 68, 0.08);
  color: var(--error-500);
}

.alert.success {
  border: 1px solid var(--success-500);
  background: rgba(16, 185, 129, 0.08);
  color: var(--success-500);
}

/* 指标卡片 */
.metrics-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: var(--space-3);
  padding: var(--space-4) var(--space-5);
  flex-shrink: 0;
}

.metric-card {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: var(--space-3);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  background: var(--surface-subtle);
  min-width: 0;
}

.metric-label {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.metric-value {
  font-size: var(--text-xl);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.metric-dim {
  font-size: var(--text-sm);
  font-weight: var(--font-normal);
  color: var(--text-tertiary);
}

.metric-sub {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.read-write {
  display: flex;
  flex-direction: column;
  gap: 2px;
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
}

.rw.read {
  color: var(--success-500);
}

.rw.write {
  color: var(--warning-500);
}

/* 表格 */
.table-wrap {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 0 var(--space-5) var(--space-4);
}

.data-table {
  width: 100%;
  border-collapse: collapse;
  font-size: var(--text-sm);
}

.data-table th {
  position: sticky;
  top: 0;
  z-index: 1;
  text-align: left;
  padding: var(--space-2) var(--space-3);
  background: var(--surface-subtle);
  border-bottom: 1px solid var(--border-default);
  color: var(--text-secondary);
  font-weight: var(--font-medium);
  white-space: nowrap;
}

.data-table td {
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--border-subtle);
  vertical-align: top;
}

.data-table tbody tr:hover {
  background: var(--surface-hover);
}

.mono {
  font-family: var(--font-mono);
  font-size: var(--text-xs);
}

.col-id {
  width: 70px;
}

.col-num {
  width: 90px;
  text-align: right;
  white-space: nowrap;
}

.col-action {
  width: 72px;
  text-align: right;
}

.col-time {
  width: 150px;
  white-space: nowrap;
}

.col-result {
  width: 64px;
  text-align: center;
}

.command-chip {
  display: inline-block;
  padding: 1px 8px;
  border-radius: 999px;
  font-size: var(--text-xs);
  background: rgba(59, 130, 246, 0.12);
  color: var(--brand-500);
  white-space: nowrap;
}

.command-chip.sleeping {
  background: var(--surface-hover);
  color: var(--text-tertiary);
}

.state {
  max-width: 160px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sql-cell {
  max-width: 420px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-secondary);
}

.empty-cell {
  text-align: center;
  color: var(--text-tertiary);
  padding: var(--space-6) !important;
}

/* 审计行 */
.audit-row {
  cursor: pointer;
}

.audit-row.failed .sql-cell {
  color: var(--error-500);
}

.audit-row.expanded {
  background: var(--surface-active);
}

.kind-chip {
  display: inline-block;
  padding: 1px 8px;
  border-radius: 999px;
  font-size: var(--text-xs);
  background: var(--surface-hover);
  color: var(--text-secondary);
  white-space: nowrap;
}

.kind-chip.table-edit {
  background: rgba(245, 158, 11, 0.14);
  color: var(--warning-500);
}

.kind-chip.import {
  background: rgba(16, 185, 129, 0.12);
  color: var(--success-500);
}

.status-ok {
  color: var(--success-500);
  font-size: var(--text-xs);
}

.status-failed {
  color: var(--error-500);
  font-size: var(--text-xs);
}

.audit-detail td {
  background: var(--surface-subtle);
  padding: var(--space-3) var(--space-4);
}

.sql-detail {
  margin: 0;
  max-height: 220px;
  overflow: auto;
  font-family: var(--font-mono);
  font-size: var(--text-xs);
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--text-primary);
}

.detail-error {
  margin-top: var(--space-2);
  color: var(--error-500);
  font-size: var(--text-xs);
}

@media (max-width: 1023px) {
  .metrics-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>
