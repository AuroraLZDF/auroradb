<template>
  <div class="backup-dialog">
    <!-- 头部 -->
    <div class="dialog-header">
      <div class="header-title">
        <Database :size="18" class="title-icon" />
        <h2>{{ t('backup.title') }}</h2>
      </div>
      <button class="icon-btn" :title="t('common.close')" @click="emit('close')" :aria-label="t('common.close')">
        <X :size="18" />
      </button>
    </div>

    <!-- Tab 切换 -->
    <div class="dialog-tabs">
      <button
        :class="['tab-btn', { active: activeTab === 'backup' }]"
        @click="activeTab = 'backup'"
      >
        <Download :size="14" />
        {{ t('backup.tabBackup') }}
      </button>
      <button
        :class="['tab-btn', { active: activeTab === 'restore' }]"
        @click="activeTab = 'restore'"
      >
        <Upload :size="14" />
        {{ t('backup.tabRestore') }}
      </button>
    </div>

    <div class="dialog-body">
      <!-- Backup tab -->
      <div v-if="activeTab === 'backup'" class="tab-panel">
        <div class="form-row">
          <label>{{ t('backup.selectConnection') }}</label>
          <Select
            v-model="backupConnId"
            :options="connectionOptions"
            :placeholder="t('backup.selectConnection')"
          />
        </div>
        <div class="form-row">
          <label>{{ t('backup.selectDatabase') }}</label>
          <Select
            v-model="backupDatabase"
            :options="backupDbOptions"
            :disabled="!backupConnId || backupDbLoading"
            :placeholder="backupDbLoading ? t('common.loading') : t('backup.selectDatabase')"
            searchable
          />
        </div>
        <Button
          variant="primary"
          :loading="backuping"
          :disabled="!canBackup"
          @click="runBackup"
        >
          <Download :size="14" />
          {{ backuping ? t('backup.backuping') : t('backup.startBackup') }}
        </Button>
        <p v-if="backupHint" class="scope-hint">{{ backupHint }}</p>
        <p v-if="backupError" class="error-msg">
          <AlertCircle :size="14" /> {{ backupError }}
        </p>
      </div>

      <!-- Restore tab -->
      <div v-if="activeTab === 'restore'" class="tab-panel">
        <div class="form-row">
          <label>{{ t('backup.selectConnection') }}</label>
          <Select
            v-model="restoreConnId"
            :options="connectionOptions"
            :placeholder="t('backup.selectConnection')"
          />
        </div>
        <div class="form-row">
          <label>{{ t('backup.selectDatabase') }}</label>
          <Select
            v-model="restoreDatabase"
            :options="restoreDbOptions"
            :disabled="!restoreConnId || restoreDbLoading"
            :placeholder="restoreDbLoading ? t('common.loading') : t('backup.selectDatabase')"
            searchable
          />
        </div>
        <div class="form-row">
          <label>{{ t('backup.chooseFile') }}</label>
          <div class="file-picker">
            <input
              v-if="restoreFilePath"
              type="text"
              :value="restoreFilePath"
              readonly
              class="text-input file-input"
            />
            <Button variant="secondary" :disabled="!restoreConnId" @click="pickFile">
              <FolderOpen :size="14" />
              {{ t('backup.chooseFile') }}
            </Button>
          </div>
        </div>
        <div class="restore-hint">
          <AlertTriangle :size="14" />
          {{ restoreHintText }}
        </div>
        <Button
          variant="danger"
          :loading="restoring"
          :disabled="!canRestore"
          @click="runRestore"
        >
          <Upload :size="14" />
          {{ restoring ? t('backup.restoring') : t('backup.startRestore') }}
        </Button>

        <!-- 还原进度：大文件执行期间给出实时反馈，避免用户误以为卡死 -->
        <div v-if="restoring || restoreProgress.total > 0" class="restore-progress">
          <div class="progress-track">
            <div class="progress-bar" :style="{ width: `${restorePercent}%` }" />
          </div>
          <div class="progress-meta">
            <span>
              {{ restorePercent }}% · {{ formatBytes(restoreProgress.bytes) }} /
              {{ formatBytes(restoreProgress.total) }}
            </span>
            <span>{{ t('backup.progressStatements', { count: restoreStatementCount }) }}</span>
          </div>
          <div v-if="restoreTail.length" class="progress-log">
            <div v-for="(line, index) in restoreTail" :key="index" :class="['log-line', line.level]">
              <span class="log-text">{{ line.text }}</span>
            </div>
          </div>
        </div>

        <p v-if="restoreError" class="error-msg">
          <AlertCircle :size="14" /> {{ restoreError }}
        </p>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { t } from '@/i18n'
import { useToast } from '@/composables/useToast'
import { confirmDanger } from '@/composables/useConfirm'
import { useConnectionStore } from '@/stores/connection'
import { capabilitiesOf } from '@/utils/driver'
import Select from '@/components/ui/Select.vue'
import Button from '@/components/ui/Button.vue'
import {
  Database,
  X,
  Download,
  Upload,
  AlertCircle,
  AlertTriangle,
  FolderOpen,
} from 'lucide-vue-next'

const emit = defineEmits<{
  close: []
  /** 还原成功：通知父组件刷新该连接下数据库的表列表 */
  restored: [connectionId: string, database: string]
}>()

const { success: showToast } = useToast()

interface ConnectionOption { value: string; label: string }

/** 可用活跃连接（MySQL 走 mysqldump，PostgreSQL 走 pg_dump/psql；两者都支持） */
const connectionOptions = computed<ConnectionOption[]>(() => {
  // 从全局 store 读活跃连接
  return activeConnections.value.map(c => ({
    value: c.id,
    label: `${c.name} (${c.host}:${c.port})`
  }))
})

const connStore = useConnectionStore()
const activeConnections = computed(() => connStore.activeConnections)

/** 当前选中连接的驱动能力：备份工具/还原提示按驱动区分（mysqldump vs pg_dump/psql） */
function capsForConnection(connectionId: string) {
  const connection = activeConnections.value.find(c => c.id === connectionId)
  return capabilitiesOf(connection?.driver)
}
const backupCaps = computed(() => capsForConnection(backupConnId.value))
const restoreCaps = computed(() => capsForConnection(restoreConnId.value))

/** 备份提示：PG 使用 pg_dump 导出纯 SQL；MySQL 走 mysqldump */
const backupHint = computed(() => {
  if (!backupConnId.value) return ''
  return backupCaps.value.backupDialect === 'postgres'
    ? t('backup.pgBackupHint')
    : t('backup.mysqlBackupHint')
})

/** 还原风险提示：PG 使用 psql 逐条执行，失败时会停在出错语句（可能部分生效） */
const restoreHintText = computed(() =>
  restoreCaps.value.backupDialect === 'postgres'
    ? t('backup.pgRestoreHint')
    : t('backup.restoreHint')
)

// ---------- Backup ----------
const backupConnId = ref('')
const backupDatabase = ref('')
const backuping = ref(false)
const backupError = ref('')
const backupDbLoading = ref(false)
const backupDbOptions = computed(() => dbOptionsFor(backupConnId.value))

// ---------- Restore ----------
const restoreConnId = ref('')
const restoreDatabase = ref('')
const restoreFilePath = ref('')
const restoring = ref(false)
const restoreError = ref('')
const restoreDbLoading = ref(false)
const restoreDbOptions = computed(() => dbOptionsFor(restoreConnId.value))

/** 还原进度事件（由后端 restore-progress 推送） */
interface RestoreLogLine {
  no?: number | null
  level: 'success' | 'error' | 'warn'
  text: string
  elapsedMs?: number | null
}

const restoreProgress = ref({
  bytes: 0,
  total: 0,
  lines: [] as RestoreLogLine[],
  done: false,
  success: false,
})

const restorePercent = computed(() => {
  const { bytes, total, done, success } = restoreProgress.value
  if (done && success) return 100
  if (total <= 0) return done ? 100 : 0
  return Math.min(99, Math.floor((bytes / total) * 100))
})

const restoreStatementCount = computed(
  () => restoreProgress.value.lines.filter(line => line.no != null && line.level === 'success').length
)

/** 只展示最近几条流水，让用户感知到「正在执行」而不挤占对话框 */
const restoreTail = computed(() => restoreProgress.value.lines.slice(-6))

function formatBytes(value: number): string {
  if (value < 1024) return `${value} B`
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`
  return `${(value / 1024 / 1024).toFixed(1)} MB`
}

const activeTab = ref<'backup' | 'restore'>('backup')

onMounted(() => {
  // 自动选中首个可用连接
  const first = connectionOptions.value[0]
  if (first) {
    backupConnId.value = first.value
    restoreConnId.value = first.value
  }
})

let restoreUnlisten: UnlistenFn | undefined

onMounted(async () => {
  restoreUnlisten = await listen<{
    bytes: number
    total: number
    lines: RestoreLogLine[]
    done: boolean
    success: boolean
  }>('restore-progress', (event) => {
    // 只处理本次还原期间的事件，避免上一次残留进度串到新任务
    if (!restoring.value) return
    const payload = event.payload
    const progress = restoreProgress.value
    progress.bytes = payload.bytes
    progress.total = payload.total
    if (payload.lines.length) {
      progress.lines.push(...payload.lines)
      // 超长脚本的流水只保留尾部，防止内存无限增长
      if (progress.lines.length > 500) {
        progress.lines.splice(0, progress.lines.length - 500)
      }
    }
    progress.done = payload.done
    progress.success = payload.success
  })
})

onUnmounted(() => {
  restoreUnlisten?.()
})

const canBackup = computed(() => !!backupConnId.value && !!backupDatabase.value && !backuping.value)
const canRestore = computed(() => !!restoreConnId.value && !!restoreDatabase.value && !!restoreFilePath.value && !restoring.value)

async function runBackup() {
  backupError.value = ''
  if (!backupConnId.value) {
    backupError.value = t('backup.noConnection')
    return
  }
  if (!canBackup.value) {
    backupError.value = t('backup.noDatabase')
    return
  }
  backuping.value = true
  try {
    // 默认文件名：库名_时间戳.sql，路径由用户在系统保存对话框中选择
    const stamp = new Date().toISOString().replace(/[-:T]/g, '').slice(0, 14)
    const defaultFilename = `${backupDatabase.value}_${stamp}.sql`
    const path = await invoke<string>('backup_database', {
      connectionId: backupConnId.value,
      database: backupDatabase.value,
      defaultFilename,
    })
    showToast(t('backup.success', { path }))
  } catch (e) {
    const msg = String(e)
    // 用户在保存对话框中取消，不当作错误提示
    if (msg.includes('取消')) return
    backupError.value = t('backup.error', { error: msg })
  } finally {
    backuping.value = false
  }
}

async function pickFile() {
  try {
    // 只取路径，文件内容由后端流式送入 mysql，不受 10MB 限制
    restoreFilePath.value = await invoke<string>('pick_file_path', { format: 'sql' })
    restoreError.value = ''
  } catch (e) {
    const msg = String(e)
    // 用户主动取消不算错误；其余失败必须提示，否则用户会误以为文件选择正常
    if (msg.includes('取消')) return
    restoreError.value = t('backup.error', { error: msg })
  }
}

async function runRestore() {
  restoreError.value = ''
  // 先做参数校验，再弹不可撤销的确认框，避免用户确认后才发现没选库
  if (!restoreConnId.value) {
    restoreError.value = t('backup.noConnection')
    return
  }
  if (!canRestore.value) {
    restoreError.value = restoreDatabase.value ? t('backup.noFile') : t('backup.noDatabase')
    return
  }

  const confirmed = await confirmDanger({
    message: restoreHintText.value,
    confirmText: t('backup.startRestore'),
    requireAcknowledge: true,
  })
  if (!confirmed) return

  restoring.value = true
  restoreProgress.value = { bytes: 0, total: 0, lines: [], done: false, success: false }
  try {
    await invoke('restore_database', {
      connectionId: restoreConnId.value,
      database: restoreDatabase.value,
      dumpPath: restoreFilePath.value,
    })
    showToast(t('backup.restoreSuccess'))
    emit('restored', restoreConnId.value, restoreDatabase.value)
    emit('close')
  } catch (e) {
    restoreError.value = t('backup.error', { error: String(e) })
  } finally {
    restoring.value = false
  }
}

// ---------- 工具 ----------

interface DbObject {
  name: string
  object_type: string
  children?: DbObject[]
}

/** 动态加载某连接的数据库列表 */
function dbOptionsFor(connId: string): { value: string; label: string }[] {
  // 简单实现：目前直接从 store 的已加载连接信息里取数据库名
  // 更完整的做法是 invoke get_object_tree 并筛选 ObjectType.Database
  // 这里为避免重复调用，先做一个同步版本；连接已展开时数据库已经存在 connectionDatabases
  return databasesByConn(connId)
    .map(db => ({ value: db, label: db }))
}

const connectionDatabases = ref<Record<string, string[]>>({})

// 切换连接后旧库名可能不属于新连接，必须清空已选库，否则会备份/还原到错误的库
watch(backupConnId, () => {
  backupDatabase.value = ''
  void refreshDbList(backupConnId.value, 'backup')
})

watch(restoreConnId, () => {
  restoreDatabase.value = ''
  void refreshDbList(restoreConnId.value, 'restore')
})

async function refreshDbList(connId: string, kind: 'backup' | 'restore') {
  const loading = kind === 'backup' ? backupDbLoading : restoreDbLoading
  if (!connId) {
    loading.value = false
    return
  }
  loading.value = true
  try {
    const objects = await invoke<DbObject[]>('get_object_tree', { connectionId: connId })
    // 递归收集所有 Database 对象的 name
    const names: string[] = []
    const walk = (list: DbObject[]) => {
      for (const obj of list) {
        if (obj.object_type === 'Database') names.push(obj.name)
        if (obj.children) walk(obj.children)
      }
    }
    walk(objects)
    connectionDatabases.value[connId] = names.sort()
  } catch (e) {
    // 加载失败不能静默：空下拉框会让用户误以为该连接下没有数据库
    connectionDatabases.value[connId] = []
    const msg = t('backup.error', { error: String(e) })
    if (kind === 'backup') backupError.value = msg
    else restoreError.value = msg
  } finally {
    loading.value = false
  }
}

function databasesByConn(connId: string): string[] {
  return connectionDatabases.value[connId] ?? []
}
</script>

<style scoped>
.backup-dialog {
  display: flex;
  flex-direction: column;
}

.dialog-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: var(--space-4);
  border-bottom: 1px solid var(--border-default);
}

.header-title {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.title-icon {
  color: var(--brand-500);
}

.header-title h2 {
  margin: 0;
  font-size: var(--text-lg);
}

.icon-btn {
  background: none;
  border: none;
  padding: var(--space-1);
  cursor: pointer;
  color: var(--text-secondary);
  border-radius: var(--radius-md);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  transition: var(--transition-colors);
}

.icon-btn:hover {
  background: var(--surface-hover);
  color: var(--text-primary);
}

.dialog-tabs {
  display: flex;
  gap: var(--space-1);
  padding: var(--space-2) var(--space-4);
  border-bottom: 1px solid var(--border-default);
}

.tab-btn {
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
  padding: var(--space-2) var(--space-3);
  border: 1px solid transparent;
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--text-sm);
  cursor: pointer;
  transition: var(--transition-colors);
}

.tab-btn:hover {
  background: var(--surface-hover);
  color: var(--text-primary);
}

.tab-btn.active {
  background: var(--brand-500);
  color: white;
}

.dialog-body {
  padding: var(--space-5);
}

.tab-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.form-row {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.form-row label {
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

.text-input {
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  background: var(--surface-default);
  color: var(--text-primary);
  font-size: var(--text-sm);
  outline: none;
  transition: var(--transition-colors);
}

.text-input:focus {
  border-color: var(--brand-500);
}

.file-picker {
  display: flex;
  gap: var(--space-2);
  align-items: center;
}

.file-input {
  flex: 1;
}

.restore-hint {
  display: flex;
  gap: var(--space-2);
  padding: var(--space-3);
  background: rgba(245, 158, 11, 0.14);
  border: 1px solid rgba(245, 158, 11, 0.35);
  border-radius: var(--radius-md);
  color: var(--warning-500);
  font-size: var(--text-sm);
  align-items: flex-start;
}

.error-msg {
  display: flex;
  gap: var(--space-2);
  padding: var(--space-3);
  background: rgba(239, 68, 68, 0.12);
  border: 1px solid rgba(239, 68, 68, 0.35);
  border-radius: var(--radius-md);
  color: var(--error-500);
  font-size: var(--text-sm);
  margin: 0;
  align-items: center;
}

.scope-hint {
  margin: 0;
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  line-height: 1.5;
}

/* 还原进度 */
.restore-progress {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.progress-track {
  height: 6px;
  border-radius: var(--radius-full, 999px);
  background: var(--surface-hover);
  overflow: hidden;
}

.progress-bar {
  height: 100%;
  border-radius: var(--radius-full, 999px);
  background: var(--brand-500);
  transition: width 150ms linear;
}

.progress-meta {
  display: flex;
  justify-content: space-between;
  font-size: var(--text-xs);
  color: var(--text-secondary);
}

.progress-log {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: var(--space-2);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  background: var(--surface-default);
  font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, monospace);
  font-size: var(--text-xs);
}

.log-line {
  display: flex;
  gap: var(--space-2);
  color: var(--text-primary);
  line-height: 1.5;
}

.log-line.error {
  color: var(--error-500);
}

.log-line.warn {
  color: var(--warning-500);
}

.log-text {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
