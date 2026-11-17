<template>
  <div v-if="state.show" class="modal-overlay" @click.self="state.show = false">
    <div class="dialog export-dialog">
      <h3>{{ t('dataTable.exportTitle') }}</h3>
      <div class="dialog-content">
        <div class="form-group">
          <label>{{ t('dataTable.format') }}</label>
          <Select
            :model-value="state.format"
            :options="exportFormatOptions"
            @update:model-value="onFormatChange"
          />
        </div>
      </div>
      <div class="dialog-actions">
        <button @click="state.show = false" class="btn-secondary">{{ t('common.cancel') }}</button>
        <button @click="executeExport" :disabled="state.loading" class="btn-primary">
          {{ state.loading ? t('dataTable.exporting') : t('common.export') }}
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { reactive } from 'vue'
import Select from '@/components/ui/Select.vue'
import { useDataBrowserStore } from '@/stores/dataBrowser'
import { t } from '@/i18n'

export type ExportFormat = 'csv' | 'xlsx' | 'sql' | 'json'

const exportFormatOptions: { label: string; value: ExportFormat }[] = [
  { label: 'CSV', value: 'csv' },
  { label: 'Excel (xlsx)', value: 'xlsx' },
  { label: 'SQL', value: 'sql' },
  { label: 'JSON', value: 'json' },
]

const props = defineProps<{
  /** 目标表（数据库 + 表名）；为空时视为未打开表 */
  table: { connectionId: string; database: string; tableName: string } | null
}>()

const emit = defineEmits<{
  /** 转发提示消息给父组件统一展示 */
  (e: 'toast', message: string, type?: 'success' | 'error'): void
}>()

const browserStore = useDataBrowserStore()

const state = reactive({
  show: false,
  format: 'csv' as ExportFormat,
  loading: false
})

function open() {
  state.show = true
  state.format = 'csv'
  state.loading = false
}

function onFormatChange(value: string) {
  state.format = value as ExportFormat
}

async function executeExport() {
  if (!props.table) {
    emit('toast', t('dataTable.openTableFirst'), 'error')
    return
  }

  state.loading = true
  const format = state.format
  try {
    // 两种格式都由后端直接写文件：Excel 用工作簿，其余逐行流式写出
    const filePath =
      format === 'xlsx'
        ? await browserStore.exportTableToExcel(
            props.table.connectionId,
            props.table.database,
            props.table.tableName,
            `${props.table.tableName}.xlsx`
          )
        : await browserStore.exportTableToFile(
            props.table.connectionId,
            props.table.database,
            props.table.tableName,
            format,
            `${props.table.tableName}.${format}`
          )
    emit('toast', t('dataTable.fileSaved', { path: filePath }))
    state.show = false
  } catch (e: unknown) {
    const errorMessage = String(e)
    // 用户取消保存时不提示错误
    if (!errorMessage.includes('用户取消')) {
      console.error('Export failed:', e)
      emit('toast', t('dataTable.exportFailed', { error: errorMessage }), 'error')
    }
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

.export-dialog {
  max-height: 80vh;
  overflow-y: auto;
}
</style>
