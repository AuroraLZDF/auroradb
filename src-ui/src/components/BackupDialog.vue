<template>
  <div class="backup-dialog">
    <!-- 头部 -->
    <div class="dialog-header">
      <div class="header-title">
        <Database :size="18" class="title-icon" />
        <h2>{{ t('backup.title') }}</h2>
      </div>
      <button class="icon-btn" :title="t('common.close')" @click="emit('close')">
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
      <button
        :class="['tab-btn', { active: activeTab === 'history' }]"
        @click="switchToHistory"
      >
        <History :size="14" />
        {{ t('backup.tabHistory') }}
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
        <div class="form-row">
          <label>{{ t('backup.labelOptional') }}</label>
          <input
            v-model="backupLabel"
            type="text"
            class="text-input"
            maxlength="64"
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
          {{ t('backup.restoreHint') }}
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
        <p v-if="restoreError" class="error-msg">
          <AlertCircle :size="14" /> {{ restoreError }}
        </p>
      </div>

      <!-- History tab -->
      <div v-if="activeTab === 'history'" class="tab-panel">
        <div v-if="historyLoading" class="empty-state">
          <RefreshCw :size="24" class="spinning" />
          {{ t('common.loading') }}
        </div>
        <div v-else-if="history.length === 0" class="empty-state">
          <History :size="40" class="empty-icon" />
          {{ t('backup.historyEmpty') }}
        </div>
        <table v-else class="history-table">
          <thead>
            <tr>
              <th>{{ t('backup.createdAt') }}</th>
              <th>{{ t('backup.driver') }}</th>
              <th>{{ t('backup.fileSize') }}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="rec in history" :key="rec.id">
              <td>
                <div>{{ rec.created_at.replace('T', ' ').slice(0, 19) }}</div>
                <div v-if="rec.label" class="record-label">{{ rec.label }}</div>
              </td>
              <td>{{ rec.driver }}</td>
              <td>{{ formatSize(rec.file_size) }}</td>
              <td class="action-cell">
                <button class="icon-btn danger" @click="confirmDelete(rec.id)">
                  <Trash2 :size="14" />
                </button>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { t } from '@/i18n'
import { useToast } from '@/composables/useToast'
import Select from '@/components/ui/Select.vue'
import Button from '@/components/ui/Button.vue'
import {
  Database,
  X,
  Download,
  Upload,
  History,
  AlertCircle,
  AlertTriangle,
  FolderOpen,
  Trash2,
  RefreshCw,
} from 'lucide-vue-next'

const emit = defineEmits<{
  close: []
}>()

const { success: showToast, error: showError } = useToast()

interface BackupRecord {
  id: string
  connection_id: string
  database: string
  driver: string
  file_path: string
  file_size: number
  created_at: string
  label?: string
}

interface ConnectionOption { value: string; label: string }

/** 可用活跃连接（驱动 MySQL 的才显示，PostgreSQL 备份暂未实现） */
const connectionOptions = computed<ConnectionOption[]>(() => {
  // 从全局 store 读活跃连接：通过 App.vue 传入或直接 invoke 拿？
  // 实际上更简单的做法：BackupDialog 接受 connections props，这里先从全局 store 读取
  return activeConnections.value
    .filter(c => (c.driver ?? 'mysql') === 'mysql')
    .map(c => ({ value: c.id, label: `${c.name} (${c.host}:${c.port})` }))
})

// 需要 import connection store
import { useConnectionStore } from '@/stores/connection'
const connStore = useConnectionStore()
const activeConnections = computed(() => connStore.activeConnections)

// ---------- Backup ----------
const backupConnId = ref('')
const backupDatabase = ref('')
const backupLabel = ref('')
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

// ---------- History ----------
const activeTab = ref<'backup' | 'restore' | 'history'>('backup')
const history = ref<BackupRecord[]>([])
const historyLoading = ref(false)

function switchToHistory() {
  activeTab.value = 'history'
  loadHistory()
}

async function loadHistory() {
  if (!backupConnId.value || !backupDatabase.value) {
    history.value = []
    return
  }
  historyLoading.value = true
  try {
    history.value = await invoke<BackupRecord[]>('list_backup_history', {
      connectionId: backupConnId.value,
      database: backupDatabase.value,
    })
  } catch (e) {
    showError(t('backup.error', { error: String(e) }))
  } finally {
    historyLoading.value = false
  }
}

// 当用户切换连接/数据库时，若在 history tab 自动刷新
watch([backupConnId, backupDatabase], () => {
  if (activeTab.value === 'history') loadHistory()
})

onMounted(() => {
  // 自动选中首个可用连接
  const first = connectionOptions.value[0]
  if (first) {
    backupConnId.value = first.value
    restoreConnId.value = first.value
  }
})

const canBackup = computed(() => !!backupConnId.value && !!backupDatabase.value && !backuping.value)
const canRestore = computed(() => !!restoreConnId.value && !!restoreDatabase.value && !!restoreFilePath.value && !restoring.value)

async function runBackup() {
  backupError.value = ''
  if (!canBackup.value) {
    backupError.value = t('backup.noDatabase')
    return
  }
  backuping.value = true
  try {
    const record = await invoke<BackupRecord>('backup_database', {
      connectionId: backupConnId.value,
      database: backupDatabase.value,
      label: backupLabel.value || undefined,
    })
    showToast(t('backup.success', { path: record.file_path }))
    // 切到 history 并刷新
    if (activeTab.value === 'backup') activeTab.value = 'history'
    loadHistory()
    // 还原标签输入
    backupLabel.value = ''
  } catch (e) {
    const msg = String(e)
    backupError.value = t('backup.error', { error: msg })
  } finally {
    backuping.value = false
  }
}

async function pickFile() {
  try {
    // 只取路径，文件内容由后端流式送入 mysql，不受 10MB 限制
    restoreFilePath.value = await invoke<string>('pick_file_path', { format: 'sql' })
  } catch {
    // 用户取消或出错时保持原状
  }
}

async function runRestore() {
  restoreError.value = ''
  if (!confirm(t('backup.restoreHint'))) return
  if (!canRestore.value) {
    restoreError.value = t('backup.noDatabase')
    return
  }
  restoring.value = true
  try {
    await invoke('restore_database', {
      connectionId: restoreConnId.value,
      database: restoreDatabase.value,
      dumpPath: restoreFilePath.value,
    })
    showToast(t('backup.restoreSuccess'))
    emit('close')
  } catch (e) {
    restoreError.value = t('backup.error', { error: String(e) })
  } finally {
    restoring.value = false
  }
}

async function confirmDelete(id: string) {
  if (!confirm(t('backup.deleteConfirm'))) return
  try {
    await invoke('delete_backup_record', { id })
    showToast(t('common.deleted'))
    loadHistory()
  } catch (e) {
    showError(t('backup.error', { error: String(e) }))
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

watch([() => backupConnId.value, () => restoreConnId.value], () => {
  void refreshDbList(backupConnId.value)
  void refreshDbList(restoreConnId.value)
})

async function refreshDbList(connId: string) {
  if (!connId) return
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
  } catch {
    connectionDatabases.value[connId] = []
  }
}

function databasesByConn(connId: string): string[] {
  return connectionDatabases.value[connId] ?? []
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 ** 3) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / 1024 ** 3).toFixed(2)} GB`
}
</script>

<style scoped>
.backup-dialog {
  display: flex;
  flex-direction: column;
  height: 100%;
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

.icon-btn.danger:hover {
  color: var(--color-danger, #ef4444);
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
  flex: 1;
  overflow: auto;
  padding: var(--space-4);
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
  background: var(--color-warning-bg, #fef3c7);
  border: 1px solid var(--color-warning-border, #fcd34d);
  border-radius: var(--radius-md);
  color: var(--color-warning-text, #92400e);
  font-size: var(--text-sm);
  align-items: flex-start;
}

.error-msg {
  display: flex;
  gap: var(--space-2);
  padding: var(--space-3);
  background: var(--color-danger-bg, #fee2e2);
  border: 1px solid var(--color-danger-border, #fca5a5);
  border-radius: var(--radius-md);
  color: var(--color-danger-text, #991b1b);
  font-size: var(--text-sm);
  margin: 0;
  align-items: center;
}

.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-3);
  padding: var(--space-8);
  color: var(--text-tertiary);
}

.empty-icon {
  color: var(--text-tertiary);
}

.spinning {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

.history-table {
  width: 100%;
  border-collapse: collapse;
}

.history-table th,
.history-table td {
  text-align: left;
  padding: var(--space-3);
  border-bottom: 1px solid var(--border-default);
  font-size: var(--text-sm);
}

.history-table th {
  color: var(--text-secondary);
  font-weight: var(--font-medium);
}

.history-table tbody tr:hover {
  background: var(--surface-hover);
}

.record-label {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.action-cell {
  text-align: right;
}
</style>
