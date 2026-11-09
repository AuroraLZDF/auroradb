<template>
  <div class="import-dialog">
    <!-- 头部 -->
    <div class="dialog-header">
      <div class="header-title">
        <Upload :size="18" class="title-icon" />
        <h2>{{ t('app.importProgressTitle') }}</h2>
      </div>
      <button
        class="icon-btn"
        :title="t('common.close')"
        :disabled="!done"
        @click="emit('close')"
      >
        <X :size="18" />
      </button>
    </div>

    <div class="dialog-body">
      <div class="file-row">
        <span class="file-name" :title="fileName">{{ fileName }}</span>
        <span class="db-name">→ {{ database }}</span>
      </div>

      <!-- 字节进度 -->
      <div class="progress-track">
        <div
          :class="['progress-bar', { failed: done && !success }]"
          :style="{ width: `${percent}%` }"
        />
      </div>
      <div class="progress-meta">
        <span>{{ percent }}% · {{ formatBytes(bytes) }} / {{ formatBytes(total) }}</span>
        <span>{{ t('app.importStatementCount', { count: statementCount }) }}</span>
      </div>

      <!-- 逐条执行流水 -->
      <div ref="logRef" class="log-list">
        <div
          v-for="(line, index) in lines"
          :key="index"
          :class="['log-line', line.level]"
        >
          <span class="log-no">{{ line.no ?? '·' }}</span>
          <span class="log-text">{{ line.text }}</span>
          <span v-if="line.elapsedMs != null" class="log-time">{{ line.elapsedMs }}ms</span>
        </div>
        <div v-if="!lines.length" class="log-empty">
          {{ done ? t('app.importLogEmptyDone') : t('app.importLogEmpty') }}
        </div>
      </div>

      <!-- 当前状态 -->
      <div :class="['result', done ? (success ? 'ok' : 'fail') : 'running']">
        <Loader2 v-if="!done" :size="14" class="spin" />
        <CheckCircle2 v-else-if="success" :size="14" />
        <AlertCircle v-else :size="14" />
        <span v-if="!done">{{ t('app.importRunning') }}</span>
        <span v-else-if="success">{{ t('app.importSuccessSummary', { count: statementCount }) }}</span>
        <span v-else class="result-error">
          {{ error ? t('app.importFailedSummary', { error }) : t('app.importFailedNoError') }}
        </span>
      </div>
    </div>

    <div v-if="done" class="dialog-footer">
      <Button variant="primary" @click="emit('close')">{{ t('common.close') }}</Button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { t } from '@/i18n'
import Button from '@/components/ui/Button.vue'
import { AlertCircle, CheckCircle2, Loader2, Upload, X } from 'lucide-vue-next'

interface ImportLogLine {
  no?: number | null
  level: 'success' | 'error' | 'warn'
  text: string
  elapsedMs?: number | null
}

const props = defineProps<{
  fileName: string
  database: string
  bytes: number
  total: number
  lines: ImportLogLine[]
  done: boolean
  success: boolean
  error: string
}>()

const emit = defineEmits<{ close: [] }>()

const logRef = ref<HTMLElement | null>(null)

const percent = computed(() => {
  if (props.done && props.success) return 100
  if (props.total <= 0) return props.done ? 100 : 0
  return Math.min(99, Math.floor((props.bytes / props.total) * 100))
})

const statementCount = computed(
  () => props.lines.filter((line) => line.no != null && line.level === 'success').length
)

function formatBytes(value: number): string {
  if (value < 1024) return `${value} B`
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`
  return `${(value / 1024 / 1024).toFixed(1)} MB`
}

// 有新流水时自动滚到底部，始终能看到最新执行结果
watch(
  () => props.lines.length,
  async () => {
    await nextTick()
    const el = logRef.value
    if (el) el.scrollTop = el.scrollHeight
  }
)
</script>

<style scoped>
.import-dialog {
  display: flex;
  flex-direction: column;
  min-width: 0;
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

.icon-btn:hover:not(:disabled) {
  background: var(--surface-hover);
  color: var(--text-primary);
}

.icon-btn:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.dialog-body {
  padding: var(--space-5);
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.file-row {
  display: flex;
  align-items: baseline;
  gap: var(--space-2);
  font-size: var(--text-sm);
  min-width: 0;
}

.file-name {
  color: var(--text-primary);
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.db-name {
  color: var(--text-secondary);
  flex-shrink: 0;
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

.progress-bar.failed {
  background: var(--error-500);
}

.progress-meta {
  display: flex;
  justify-content: space-between;
  font-size: var(--text-xs);
  color: var(--text-secondary);
}

.log-list {
  height: 240px;
  overflow-y: auto;
  padding: var(--space-2);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  background: var(--surface-default);
  font-family: var(--font-mono, ui-monospace, SFMono-Regular, Menlo, monospace);
  font-size: var(--text-xs);
}

.log-line {
  display: flex;
  align-items: baseline;
  gap: var(--space-2);
  padding: 2px 0;
  color: var(--text-primary);
  line-height: 1.5;
}

.log-no {
  flex-shrink: 0;
  width: 3.5em;
  text-align: right;
  color: var(--text-secondary);
}

.log-text {
  flex: 1;
  min-width: 0;
  overflow-wrap: anywhere;
}

.log-time {
  flex-shrink: 0;
  color: var(--text-secondary);
}

.log-line.success .log-no {
  color: var(--success-500);
}

.log-line.error {
  color: var(--error-500);
}

.log-line.warn {
  color: var(--warning-500);
}

.log-empty {
  color: var(--text-secondary);
  padding: var(--space-2);
}

.result {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
  padding: var(--space-3);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
}

.result.running {
  background: var(--surface-hover);
  color: var(--text-secondary);
}

.result.ok {
  background: rgba(16, 185, 129, 0.12);
  border: 1px solid rgba(16, 185, 129, 0.35);
  color: var(--success-500);
}

.result.fail {
  background: rgba(239, 68, 68, 0.12);
  border: 1px solid rgba(239, 68, 68, 0.35);
  color: var(--error-500);
}

.result-error {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  padding: var(--space-3) var(--space-4);
  border-top: 1px solid var(--border-default);
}

.spin {
  animation: import-spin 1s linear infinite;
}

@keyframes import-spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
