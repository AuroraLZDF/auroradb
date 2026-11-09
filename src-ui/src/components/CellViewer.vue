<template>
  <div v-if="open" class="cell-viewer-overlay" @click.self="close">
    <div class="cell-viewer" role="dialog" aria-modal="true" @click.stop>
      <header class="viewer-header">
        <div class="viewer-title">
          <span class="viewer-column" :title="columnName">{{ columnName || t('cellViewer.title') }}</span>
          <span v-if="columnType" class="viewer-type">{{ columnType }}</span>
          <span v-if="sizeLabel" class="viewer-size">{{ sizeLabel }}</span>
        </div>
        <button class="viewer-close" :title="t('common.close')" @click="close" :aria-label="t('common.close')">
          <X :size="16" />
        </button>
      </header>

      <div class="viewer-toolbar">
        <div v-if="json.isJson" class="viewer-tabs">
          <button :class="{ active: mode === 'text' }" @click="mode = 'text'">
            {{ t('cellViewer.viewText') }}
          </button>
          <button :class="{ active: mode === 'tree' }" @click="mode = 'tree'">
            {{ t('cellViewer.viewTree') }}
          </button>
          <button v-if="mode === 'text'" @click="showRaw = !showRaw">
            {{ showRaw ? t('cellViewer.formatted') : t('cellViewer.raw') }}
          </button>
        </div>
        <div v-else class="viewer-tabs-placeholder"></div>
        <div class="viewer-actions">
          <button :disabled="isNull" @click="copyValue">
            <Copy :size="14" />
            {{ t('cellViewer.copyValue') }}
          </button>
          <button :disabled="isNull" @click="copySqlLiteral">
            {{ t('cellViewer.copySql') }}
          </button>
          <button :disabled="isNull || saving" @click="saveAs">
            <Download :size="14" />
            {{ saving ? t('cellViewer.saving') : t('cellViewer.saveAs') }}
          </button>
        </div>
      </div>

      <div class="viewer-body">
        <div v-if="isNull" class="viewer-null">{{ t('cellViewer.nullValue') }}</div>

        <template v-else-if="isBinary">
          <div v-if="decodeFailed" class="viewer-notice">{{ t('cellViewer.decodeFailed') }}</div>
          <template v-else>
            <img
              v-if="imageDataUrl"
              :src="imageDataUrl"
              class="viewer-image"
              :alt="t('cellViewer.imagePreview')"
            />
            <div v-else class="viewer-notice">
              {{ imageTooLarge
                ? t('cellViewer.imageTooLarge', { limit: formatByteSize(IMAGE_PREVIEW_MAX_BYTES) })
                : t('cellViewer.nonImageHint') }}
            </div>
            <div class="viewer-section-title">{{ t('cellViewer.hexView') }}</div>
            <pre class="viewer-pre viewer-hex">{{ hexText }}</pre>
            <div v-if="hexTruncated" class="viewer-notice">
              {{ t('cellViewer.hexTruncated', {
                shown: formatByteSize(Math.min(HEX_PREVIEW_BYTES, byteSize)),
                total: formatByteSize(byteSize)
              }) }}
            </div>
          </template>
        </template>

        <template v-else>
          <div v-if="jsonTooLarge" class="viewer-notice">
            {{ t('cellViewer.jsonTooLarge', { limit: formatByteSize(JSON_PARSE_MAX_CHARS) }) }}
          </div>
          <div v-else-if="jsonFallback" class="viewer-notice">{{ t('cellViewer.jsonFallback') }}</div>
          <JsonTree v-if="json.isJson && mode === 'tree'" :value="json.value" />
          <pre v-else class="viewer-pre">{{ rendered.text }}</pre>
          <div v-if="rendered.truncated" class="viewer-notice">
            {{ t('cellViewer.textTruncated', {
              chars: TEXT_RENDER_MAX_CHARS,
              total: textValue.length
            }) }}
          </div>
        </template>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { Copy, Download, X } from '@/components/icons'
import JsonTree from '@/components/JsonTree.vue'
import { t } from '@/i18n'
import { globalToast } from '@/composables/useToast'
import type { DriverKind } from '@/stores/savedConnections'
import { formatSqlValue, isBinaryType } from '@/utils/tableChanges'
import {
  HEX_PREVIEW_BYTES,
  IMAGE_PREVIEW_MAX_BYTES,
  JSON_PARSE_MAX_CHARS,
  TEXT_RENDER_MAX_CHARS,
  base64ToBytes,
  defaultFileName,
  formatByteSize,
  formatJsonText,
  sniffImageType,
  toHexDump,
  truncateForRender,
} from '@/utils/cellViewer'

/**
 * 单元格查看器：大文本 / JSON / BLOB 的独立查看弹窗。
 *
 * - JSON 可格式化、切树形折叠；非法 JSON 自动回退原文
 * - BLOB 按文件头识别图片并预览，其它给十六进制视图
 * - 支持复制值、复制为 SQL 字面量、另存为文件
 */
const props = withDefaults(
  defineProps<{
    open: boolean
    value: unknown
    columnName?: string
    columnType?: string
    driver?: DriverKind
  }>(),
  { columnName: '', columnType: '', driver: 'mysql' }
)

const emit = defineEmits<{ (e: 'close'): void }>()

const saving = ref(false)
const mode = ref<'text' | 'tree'>('text')
const showRaw = ref(false)

const isNull = computed(() => props.value === null || props.value === undefined)
const isBinary = computed(() => isBinaryType(props.columnType))
const textValue = computed(() => (isNull.value ? '' : String(props.value)))
const normalizedBase64 = computed(() => textValue.value.replace(/\s+/g, ''))

const bytes = computed<Uint8Array | null>(() =>
  isBinary.value ? base64ToBytes(textValue.value) : null
)
const decodeFailed = computed(() => isBinary.value && bytes.value === null)
const image = computed(() => {
  const data = bytes.value
  return data ? sniffImageType(data) : null
})
const imageTooLarge = computed(() => (bytes.value?.length ?? 0) > IMAGE_PREVIEW_MAX_BYTES)
const imageDataUrl = computed(() => {
  if (!image.value || imageTooLarge.value) return ''
  return `data:${image.value.mime};base64,${normalizedBase64.value}`
})
const hexText = computed(() => (bytes.value ? toHexDump(bytes.value) : ''))
const hexTruncated = computed(() => (bytes.value?.length ?? 0) > HEX_PREVIEW_BYTES)

const byteSize = computed(() => {
  if (isNull.value) return 0
  if (isBinary.value) return bytes.value?.length ?? textValue.value.length
  return new TextEncoder().encode(textValue.value).length
})
const sizeLabel = computed(() =>
  isNull.value ? '' : t('cellViewer.size', { size: formatByteSize(byteSize.value) })
)

const json = computed(() =>
  isBinary.value ? { isJson: false, text: '' } : formatJsonText(textValue.value)
)
const looksLikeJson = computed(() => {
  const trimmed = textValue.value.trim()
  return trimmed.startsWith('{') || trimmed.startsWith('[')
})
const jsonTooLarge = computed(
  () => !isBinary.value && looksLikeJson.value && textValue.value.length > JSON_PARSE_MAX_CHARS
)
const jsonFallback = computed(
  () => !isBinary.value && looksLikeJson.value && !jsonTooLarge.value && !json.value.isJson
)

const rendered = computed(() =>
  truncateForRender(json.value.isJson && !showRaw.value ? json.value.text : textValue.value)
)

/** 按当前展示模式取要复制的文本（复制值始终是原始值） */
const valueForCopy = computed(() => (isNull.value ? 'NULL' : textValue.value))

function close() {
  emit('close')
}

async function writeClipboard(text: string) {
  try {
    await navigator.clipboard.writeText(text)
    return true
  } catch (error) {
    globalToast.error(t('cellViewer.copyFailed', { error: String(error) }))
    return false
  }
}

async function copyValue() {
  if (await writeClipboard(valueForCopy.value)) {
    globalToast.success(t('dataTable.copiedToClipboard'))
  }
}

async function copySqlLiteral() {
  const literal = formatSqlValue(props.value, {
    driver: props.driver,
    data_type: props.columnType,
  })
  if (await writeClipboard(literal)) {
    globalToast.success(t('cellViewer.copySqlDone'))
  }
}

async function saveAs() {
  if (isNull.value || saving.value) return
  saving.value = true
  try {
    let path: string
    if (isBinary.value) {
      const extension = image.value?.extension ?? 'bin'
      path = await invoke<string>('save_binary_file_to_dialog', {
        contentBase64: normalizedBase64.value,
        defaultFilename: defaultFileName(props.columnName, extension),
        filterName: image.value ? t('cellViewer.imageFile') : t('cellViewer.binaryFile'),
        extensions: [extension],
      })
    } else {
      const format = json.value.isJson ? 'json' : 'txt'
      path = await invoke<string>('save_file_to_dialog', {
        content: textValue.value,
        defaultFilename: defaultFileName(props.columnName, format),
        format,
      })
    }
    globalToast.success(t('cellViewer.saved', { path }))
  } catch (error) {
    const message = String(error)
    if (message.includes('取消')) return
    globalToast.error(t('cellViewer.saveFailed', { error: message }))
  } finally {
    saving.value = false
  }
}

function onKeyDown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    event.stopPropagation()
    close()
  }
}

watch(
  () => props.open,
  open => {
    if (open) {
      mode.value = 'text'
      showRaw.value = false
      document.addEventListener('keydown', onKeyDown)
    } else {
      document.removeEventListener('keydown', onKeyDown)
    }
  }
)

onUnmounted(() => {
  document.removeEventListener('keydown', onKeyDown)
})
</script>

<style scoped>
.cell-viewer-overlay {
  position: fixed;
  inset: 0;
  z-index: 2000;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-overlay);
}

.cell-viewer {
  display: flex;
  flex-direction: column;
  width: min(900px, 90vw);
  height: min(720px, 85vh);
  background: var(--bg-elevated);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xl);
  overflow: hidden;
}

.viewer-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
  padding: var(--space-3) var(--space-4);
  border-bottom: 1px solid var(--border-subtle);
}

.viewer-title {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-width: 0;
}

.viewer-column {
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.viewer-type {
  padding: 1px var(--space-2);
  border-radius: var(--radius-sm);
  background: var(--surface-subtle);
  color: var(--text-secondary);
  font-size: var(--text-xs);
  font-family: var(--font-mono);
}

.viewer-size {
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.viewer-close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
}

.viewer-close:hover {
  background: var(--surface-hover);
  color: var(--text-primary);
}

.viewer-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-4);
  border-bottom: 1px solid var(--border-subtle);
  background: var(--bg-secondary);
}

.viewer-tabs,
.viewer-actions {
  display: flex;
  align-items: center;
  gap: var(--space-1);
}

.viewer-tabs button,
.viewer-actions button {
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
  padding: var(--space-1) var(--space-2);
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--text-sm);
  cursor: pointer;
}

.viewer-tabs button:hover,
.viewer-actions button:hover:not(:disabled) {
  background: var(--surface-hover);
  color: var(--text-primary);
}

.viewer-tabs button.active {
  border-color: var(--brand-500);
  color: var(--brand-600);
  background: var(--surface-active);
}

.viewer-actions button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.viewer-body {
  flex: 1;
  min-height: 0;
  padding: var(--space-3) var(--space-4);
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.viewer-null {
  color: var(--text-tertiary);
  font-style: italic;
}

.viewer-notice {
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-sm);
  background: var(--surface-subtle);
  color: var(--text-secondary);
  font-size: var(--text-xs);
}

.viewer-section-title {
  color: var(--text-tertiary);
  font-size: var(--text-xs);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.viewer-pre {
  margin: 0;
  padding: var(--space-3);
  border-radius: var(--radius-md);
  background: var(--bg-secondary);
  color: var(--text-primary);
  font-family: var(--font-mono);
  font-size: var(--text-base);
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-all;
}

.viewer-hex {
  white-space: pre;
  overflow-x: auto;
  font-size: var(--text-sm);
}

.viewer-image {
  max-width: 100%;
  max-height: 60vh;
  object-fit: contain;
  border-radius: var(--radius-md);
  background: var(--bg-secondary);
  align-self: flex-start;
}
</style>
