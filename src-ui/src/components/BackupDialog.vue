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

const activeTab = ref<'backup' | 'restore'>('backup')

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
</style>
