<template>
  <div v-if="state.show" class="modal-overlay" @click.self="state.show = false">
    <div class="dialog import-dialog">
      <h3>{{ t('dataTable.importTitle') }}</h3>
      <div class="dialog-content">
        <div class="form-group">
          <label>{{ t('dataTable.format') }}</label>
          <Select
            :model-value="state.format"
            :options="importFormatOptions"
            @update:model-value="onFormatChange"
          />
        </div>
        <div class="form-group">
          <label>{{ t('dataTable.file') }}</label>
          <div class="file-select-area">
            <button @click="selectImportFile" :disabled="state.loading" class="btn-secondary">
              <Folder class="btn-icon" :size="16" /> {{ t('dataTable.selectFile') }}
            </button>
            <div v-if="state.filePath" class="file-info">
              <span class="file-name">{{ state.fileName }}</span>
              <span class="file-size">({{ formatFileSize(state.fileSize) }})</span>
            </div>
            <div v-else class="file-hint">
              {{ t('dataTable.fileHint') }}
            </div>
          </div>
        </div>
        <div class="form-group" v-if="state.data">
          <label>{{ t('dataTable.previewFirst500') }}</label>
          <div class="file-preview">
            <pre>{{ state.data.substring(0, 500) }}{{ state.data.length > 500 ? '...' : '' }}</pre>
          </div>
        </div>
      </div>
      <div class="dialog-actions">
        <button @click="state.show = false" class="btn-secondary">{{ t('common.cancel') }}</button>
        <button @click="executeImport" :disabled="state.loading || !state.data" class="btn-primary">
          {{ state.loading ? t('dataTable.importing') : t('common.import') }}
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { reactive } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { Folder } from 'lucide-vue-next'
import Select from '@/components/ui/Select.vue'
import { useDataBrowserStore } from '@/stores/dataBrowser'
import { t } from '@/i18n'

export type ImportFormat = 'csv' | 'sql'

const importFormatOptions: { label: string; value: ImportFormat }[] = [
  { label: 'CSV', value: 'csv' },
  { label: 'SQL', value: 'sql' },
]

const props = defineProps<{
  /** 目标表（数据库 + 表名）；为空时视为未打开表 */
  table: { connectionId: string; database: string; tableName: string } | null
}>()

const emit = defineEmits<{
  /** 导入成功后通知父组件刷新数据 */
  (e: 'imported', count: number): void
  /** 转发提示消息给父组件统一展示 */
  (e: 'toast', message: string, type?: 'success' | 'error'): void
}>()

const browserStore = useDataBrowserStore()

const state = reactive({
  show: false,
  format: 'csv' as ImportFormat,
  data: '',
  filePath: '',
  fileName: '',
  fileSize: 0,
  loading: false
})

function open() {
  state.show = true
  state.format = 'csv'
  state.data = ''
  state.filePath = ''
  state.fileName = ''
  state.fileSize = 0
  state.loading = false
}

function formatFileSize(bytes: number): string {
  if (bytes === 0) return '0 B'
  const k = 1024
  const sizes = ['B', 'KB', 'MB', 'GB']
  const i = Math.floor(Math.log(bytes) / Math.log(k))
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i]
}

// 选择导入文件
async function selectImportFile() {
  try {
    state.loading = true
    const result = await invoke<[string, string]>('read_file_from_dialog', {
      format: state.format
    })
    const [filePath, content] = result
    state.filePath = filePath
    state.fileName = filePath.split('/').pop() || filePath.split('\\').pop() || ''
    state.data = content
    // 估算文件大小（字符数 * 2 字节，近似 UTF-8）
    state.fileSize = new Blob([content]).size
  } catch (error: unknown) {
    const errorMessage = String(error)
    // 用户取消选择时不显示错误
    if (!errorMessage.includes('用户取消') && !errorMessage.includes('被取消')) {
      emit('toast', t('dataTable.selectFileFailed', { error: errorMessage }), 'error')
    }
  } finally {
    state.loading = false
  }
}

// 切换格式时清除已选择的文件
function onFormatChange(value: string) {
  state.format = value as ImportFormat
  state.data = ''
  state.filePath = ''
  state.fileName = ''
  state.fileSize = 0
}

async function executeImport() {
  if (!props.table) {
    emit('toast', t('dataTable.openTableFirst'), 'error')
    return
  }
  if (!state.data.trim()) {
    emit('toast', t('dataTable.importDataRequired'), 'error')
    return
  }

  state.loading = true
  try {
    const count = await browserStore.importData(
      props.table.connectionId,
      props.table.database,
      props.table.tableName,
      state.format,
      state.data
    )
    emit('toast', t('dataTable.importSuccess', { count }))
    state.show = false
    emit('imported', count)
  } catch (e: unknown) {
    console.error('Import failed:', e)
    emit('toast', t('dataTable.importFailed', { error: String(e) }), 'error')
  } finally {
    state.loading = false
  }
}

defineExpose({ open })
</script>

<style scoped>
.modal-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(var(--bg-primary-rgb, 30, 30, 30), 0.45);
  backdrop-filter: blur(2px);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
  animation: fade-in 0.3s ease-out;
}

@keyframes fade-in {
  from {
    opacity: 0;
    backdrop-filter: blur(0px);
  }
  to {
    opacity: 1;
    backdrop-filter: blur(1px);
  }
}

.dialog {
  background: var(--canvas);
  padding: 24px;
  border-radius: 8px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.2);
  min-width: 400px;
  max-width: 600px;
  width: 90vw;
}

.dialog h3 {
  margin: 0 0 20px 0;
  font-size: 16px;
  border-bottom: 1px solid var(--hairline);
  padding-bottom: 12px;
  color: var(--text-primary);
}

.dialog-content {
  margin-bottom: 20px;
}

.dialog-actions {
  display: flex;
  gap: 12px;
  justify-content: flex-end;
}

.btn-primary {
  background: var(--success-500);
  color: white;
  border: none;
  padding: 8px 16px;
  border-radius: 4px;
  cursor: pointer;
}

.btn-secondary {
  background: var(--panel);
  border: 1px solid var(--hairline);
  color: var(--text-primary);
  padding: 8px 16px;
  border-radius: 4px;
  cursor: pointer;
}

.btn-secondary:disabled,
.btn-primary:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.form-group {
  margin-bottom: 16px;
}

.form-group label {
  display: block;
  margin-bottom: 6px;
  font-size: 13px;
  color: var(--text-secondary);
}

/* 自定义下拉框：触发器与对话框内输入框保持相同的外观 */
.form-group :deep(.select-trigger) {
  width: 100%;
  height: 38px;
  min-height: 38px;
  padding: 0 12px;
  border-color: var(--hairline);
  border-radius: 4px;
  background: var(--canvas);
  color: var(--text-primary);
  font-size: 13px;
}

.import-dialog {
  max-height: 80vh;
  overflow-y: auto;
}

/* 文件选择区域 */
.file-select-area {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.file-select-area .btn-secondary {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 10px 16px;
}

.btn-icon {
  font-size: 16px;
}

.file-info {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  background: var(--panel);
  border-radius: 4px;
  border: 1px solid var(--hairline);
}

.file-name {
  font-weight: 500;
  color: var(--text-primary);
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.file-size {
  color: var(--text-secondary);
  font-size: 12px;
}

.file-hint {
  color: var(--text-secondary);
  font-size: 13px;
  padding: 8px 0;
}

.file-preview {
  background: var(--panel);
  border: 1px solid var(--hairline);
  border-radius: 4px;
  padding: 12px;
  max-height: 150px;
  overflow-y: auto;
}

.file-preview pre {
  margin: 0;
  font-family: 'JetBrains Mono', 'Fira Code', monospace;
  font-size: 12px;
  line-height: 1.4;
  color: var(--text-primary);
  white-space: pre-wrap;
  word-break: break-all;
}
</style>
