<template>
  <div v-if="open" class="gen-mask" @click.self="emit('close')">
    <div class="gen-dialog">
      <div class="gen-header">
        <span class="gen-title">{{ t('dataGenerator.title', { table }) }}</span>
        <button class="gen-close" @click="emit('close')">×</button>
      </div>

      <div class="gen-body">
        <div class="gen-config">
          <label class="gen-field">
            <span>{{ t('dataGenerator.rowCount') }}</span>
            <input v-model.number="rowCount" type="number" min="1" max="1000" />
          </label>
          <label class="gen-field">
            <span>{{ t('dataGenerator.tableName') }}</span>
            <input v-model="tableNameInput" />
          </label>
        </div>

        <div v-if="loading" class="gen-loading">{{ t('common.loading') }}</div>
        <div v-else-if="loadError" class="gen-error">{{ loadError }}</div>
        <template v-else>
          <div class="gen-columns">
            <div class="gen-columns-header">
              <span>{{ t('dataGenerator.column') }}</span>
              <span>{{ t('dataGenerator.type') }}</span>
              <span>{{ t('dataGenerator.pattern') }}</span>
            </div>
            <div v-for="col in columns" :key="col.name" class="gen-column-row">
              <label class="gen-col-name">
                <input v-model="disabledColumns[col.name]" type="checkbox" />
                <span :class="{ muted: disabledColumns[col.name] }">{{ col.name }}</span>
                <span v-if="col.is_auto_increment" class="gen-tag">{{ t('dataGenerator.autoInc') }}</span>
              </label>
              <span class="gen-col-type">{{ col.data_type }}</span>
              <input
                v-model="patterns[col.name]"
                class="gen-col-pattern"
                :placeholder="t('dataGenerator.patternPlaceholder')"
                :disabled="disabledColumns[col.name]"
              />
            </div>
          </div>

          <div class="gen-preview-header">
            <span>{{ t('dataGenerator.preview') }}</span>
            <button class="gen-link" @click="regenerate">{{ t('dataGenerator.regenerate') }}</button>
          </div>
          <pre class="gen-preview">{{ previewSql }}</pre>
        </template>
      </div>

      <div class="gen-footer">
        <button class="gen-btn" @click="copySql">{{ t('common.copy') }}</button>
        <button class="gen-btn" @click="emit('close')">{{ t('common.cancel') }}</button>
        <button class="gen-btn primary" :disabled="loading || !!loadError || !previewSql" @click="openInEditor">
          {{ t('dataGenerator.openInEditor') }}
        </button>
      </div>
      <div v-if="notice" class="gen-notice">{{ notice }}</div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { t } from '@/i18n'
import { quoteIdentifier } from '@/utils/sql'

interface ColumnInfo {
  name: string
  data_type: string
  length?: number | null
  decimals?: number | null
  is_nullable: boolean
  is_auto_increment: boolean
  is_primary: boolean
}

interface TableStructure {
  columns: ColumnInfo[]
}

const props = defineProps<{
  open: boolean
  connectionId: string
  database: string
  table: string
  driver: 'mysql' | 'postgres'
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'open-sql', sql: string, title: string): void
}>()

const loading = ref(false)
const loadError = ref('')
const columns = ref<ColumnInfo[]>([])
const rowCount = ref(10)
const tableNameInput = ref('')
const disabledColumns = ref<Record<string, boolean>>({})
const patterns = ref<Record<string, string>>({})
const previewSql = ref('')
const notice = ref('')
let noticeTimer: ReturnType<typeof setTimeout> | null = null

watch(
  () => props.open,
  open => {
    if (open) {
      tableNameInput.value = props.table
      void loadStructure()
    }
  },
  { immediate: true },
)

watch([rowCount, tableNameInput, disabledColumns, patterns], regenerate, { deep: true })

async function loadStructure() {
  loading.value = true
  loadError.value = ''
  columns.value = []
  try {
    const structure = await invoke<TableStructure>('get_table_structure', {
      connectionId: props.connectionId,
      database: props.database,
      table: props.table,
    })
    columns.value = structure.columns
    disabledColumns.value = Object.fromEntries(
      structure.columns.map(col => [col.name, col.is_auto_increment]),
    )
    patterns.value = Object.fromEntries(structure.columns.map(col => [col.name, '']))
    regenerate()
  } catch (e) {
    loadError.value = String(e)
  } finally {
    loading.value = false
  }
}

function randInt(min: number, max: number): number {
  return Math.floor(Math.random() * (max - min + 1)) + min
}

function randomChar(chars: string): string {
  return chars[randInt(0, chars.length - 1)]!
}

function escapeToken(esc: string | undefined): string {
  switch (esc) {
    case 'd':
      return String(randInt(0, 9))
    case 'w':
      return randomChar('abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_')
    case 's':
      return ' '
    default:
      return esc ?? ''
  }
}

function randomFromClass(cls: string): string {
  const negated = cls.startsWith('^')
  const body = negated ? cls.slice(1) : cls
  // 展开 a-z / 0-9 之类的范围
  const chars: string[] = []
  for (let i = 0; i < body.length; i++) {
    const ch = body[i]!
    const next = body[i + 1]
    if (next === '-' && body[i + 2]) {
      const start = ch.charCodeAt(0)
      const end = body[i + 2]!.charCodeAt(0)
      for (let code = start; code <= end; code++) chars.push(String.fromCharCode(code))
      i += 2
    } else {
      chars.push(ch)
    }
  }
  if (negated) {
    const allowed = 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789'
    const filtered = allowed.split('').filter(c => !chars.includes(c))
    return filtered.length > 0 ? randomChar(filtered.join('')) : ''
  }
  return chars.length > 0 ? randomChar(chars.join('')) : ''
}

/** 极简正则 → 随机字符串，支持 \\d \\w、[a-z]、{n} / {n,m} 与字面量 */
function generateFromPattern(pattern: string): string | null {
  try {
    new RegExp(pattern)
  } catch {
    return null
  }
  let out = ''
  let i = 0
  while (i < pattern.length) {
    const ch = pattern[i]!
    if (ch === '\\') {
      out += escapeToken(pattern[i + 1])
      i += 2
    } else if (ch === '[') {
      const end = pattern.indexOf(']', i + 1)
      if (end === -1) {
        out += ch
        i++
      } else {
        out += randomFromClass(pattern.slice(i + 1, end))
        i = end + 1
      }
    } else if (ch === '{') {
      const end = pattern.indexOf('}', i)
      if (end === -1) {
        out += ch
        i++
      } else {
        const [minRaw, maxRaw] = pattern.slice(i + 1, end).split(',')
        const min = Number(minRaw) || 1
        const max = maxRaw ? Number(maxRaw) || min : min
        const count = randInt(min, max)
        out += out.slice(-1).repeat(Math.max(0, count - 1))
        i = end + 1
      }
    } else if (ch === '(' || ch === ')' || ch === '?' || ch === '*' || ch === '+') {
      i++
    } else if (ch === '.') {
      out += randomChar('abcdefghijklmnopqrstuvwxyz')
      i++
    } else {
      out += ch
      i++
    }
  }
  return out
}

function isNumericType(type: string): boolean {
  return /int|decimal|numeric|float|double|real|serial|money/i.test(type)
}

function isDateType(type: string): boolean {
  return /date|time/i.test(type)
}

function randomDate(): string {
  const date = new Date(Date.now() - randInt(0, 30) * 86400000)
  return date.toISOString().slice(0, 10)
}

function randomDateTime(): string {
  const date = new Date(Date.now() - randInt(0, 30) * 86400000 - randInt(0, 86399) * 1000)
  return date.toISOString().slice(0, 19).replace('T', ' ')
}

function columnValue(col: ColumnInfo, row: number): string {
  const pattern = (patterns.value[col.name] ?? '').trim()
  if (pattern) {
    const generated = generateFromPattern(pattern)
    if (generated !== null) {
      return isNumericType(col.data_type) ? generated || '0' : literal(generated)
    }
  }
  const type = col.data_type.toLowerCase()
  if (isNumericType(type)) {
    if (/decimal|numeric|float|double|real|money/.test(type)) {
      return (Math.random() * 1000).toFixed(2)
    }
    return String(randInt(1, 100000) + row)
  }
  if (/bool|bit/.test(type)) return Math.random() > 0.5 ? '1' : '0'
  if (/year/.test(type)) return String(randInt(1990, 2030))
  if (type.includes('timestamp') || type.includes('datetime')) return literal(randomDateTime())
  if (type.includes('date')) return literal(randomDate())
  if (type.includes('time')) return literal(`${String(randInt(0, 23)).padStart(2, '0')}:${String(randInt(0, 59)).padStart(2, '0')}:00`)
  if (/json/.test(type)) return literal(JSON.stringify({ id: row, name: `item-${row}` }))
  if (/blob|binary|bytea/.test(type)) return `X'${randInt(0, 255).toString(16).padStart(2, '0')}'`
  return literal(`测试数据_${props.table}_${row}`)
}

function literal(value: string): string {
  return `'${value.replace(/'/g, "''")}'`
}

function regenerate() {
  if (columns.value.length === 0) {
    previewSql.value = ''
    return
  }
  const driver = props.driver
  const names = columns.value
    .filter(col => !disabledColumns.value[col.name])
    .map(col => col.name)
  if (names.length === 0) {
    previewSql.value = ''
    return
  }
  const table = tableNameInput.value.trim() || props.table
  const qualified = driver === 'postgres'
    ? table.split('.').map(part => quoteIdentifier(part, driver)).join('.')
    : `${quoteIdentifier(props.database, driver)}.${quoteIdentifier(table, driver)}`
  const quotedColumns = names.map(name => quoteIdentifier(name, driver)).join(', ')
  const rows: string[] = []
  const total = Math.max(1, Math.min(1000, Number(rowCount.value) || 1))
  for (let row = 1; row <= total; row++) {
    const values = columns.value
      .filter(col => !disabledColumns.value[col.name])
      .map(col => columnValue(col, row))
    rows.push(`  (${values.join(', ')})`)
  }
  previewSql.value = `INSERT INTO ${qualified} (${quotedColumns})\nVALUES\n${rows.join(',\n')};`
}

function copySql() {
  navigator.clipboard
    .writeText(previewSql.value)
    .then(() => showNotice(t('dataGenerator.copied')))
    .catch(() => showNotice(t('dataGenerator.copyFailed')))
}

function openInEditor() {
  emit('open-sql', previewSql.value, t('dataGenerator.title', { table: props.table }))
  emit('close')
}

function showNotice(message: string) {
  notice.value = message
  if (noticeTimer) clearTimeout(noticeTimer)
  noticeTimer = setTimeout(() => {
    notice.value = ''
  }, 2000)
}
</script>

<style scoped>
/* 数据生成器：与命令面板/历史弹窗同一套对话框语言（overlay 底 + hairline 边 + 32px 头部） */
.gen-mask {
  position: fixed;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg-overlay);
  backdrop-filter: blur(2px);
  z-index: var(--z-modal);
}

.gen-dialog {
  position: relative;
  display: flex;
  flex-direction: column;
  width: 760px;
  max-width: 94vw;
  max-height: 86vh;
  background: var(--overlay);
  border: 1px solid var(--hairline-strong);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xl);
  overflow: hidden;
}

.gen-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  height: var(--toolbar-height);
  padding: 0 var(--space-3);
  border-bottom: 1px solid var(--hairline);
  flex-shrink: 0;
}

.gen-title {
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.gen-close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: var(--control-height-sm);
  height: var(--control-height-sm);
  padding: 0;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-tertiary);
  font-size: 16px;
  line-height: 1;
  cursor: pointer;
  transition: var(--transition-colors);
}

.gen-close:hover {
  background: var(--hover-fill);
  color: var(--text-primary);
}

.gen-body {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: var(--space-3) var(--space-4);
}

.gen-config {
  display: flex;
  align-items: center;
  gap: var(--space-4);
  margin-bottom: var(--space-3);
}

.gen-field {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-xs);
  color: var(--text-secondary);
}

.gen-field input {
  width: 140px;
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

.gen-field input:focus {
  background: var(--canvas);
  border-color: var(--brand-500);
}

.gen-loading,
.gen-empty {
  padding: var(--space-5) 0;
  text-align: center;
  font-size: var(--text-sm);
  color: var(--text-tertiary);
}

.gen-error {
  padding: var(--space-2) 0;
  font-size: var(--text-xs);
  color: var(--error-500);
}

/* 列配置：只留横向 hairline，不再用圆角卡片包裹 */
.gen-columns {
  border: 1px solid var(--hairline);
  border-radius: var(--radius-sm);
  overflow: hidden;
}

.gen-columns-header,
.gen-column-row {
  display: grid;
  grid-template-columns: 1.2fr 0.8fr 1.4fr;
  align-items: center;
  gap: var(--space-3);
  min-height: var(--tree-row-height);
  padding: 2px var(--space-3);
  font-size: var(--text-xs);
}

.gen-columns-header {
  background: var(--raised);
  color: var(--text-tertiary);
  font-weight: var(--font-medium);
}

.gen-column-row + .gen-column-row {
  border-top: 1px solid var(--hairline);
}

.gen-column-row:hover {
  background: var(--hover-fill);
}

.gen-col-name {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  min-width: 0;
}

.gen-col-name span.muted {
  text-decoration: line-through;
  opacity: 0.55;
}

.gen-tag {
  padding: 0 4px;
  border-radius: var(--radius-xs);
  background: var(--hover-fill);
  color: var(--text-secondary);
  font-size: 10px;
  line-height: 15px;
}

.gen-col-type {
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-tertiary);
}

.gen-col-pattern {
  width: 100%;
  height: 22px;
  padding: 0 var(--space-2);
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  background: var(--canvas);
  color: var(--text-primary);
  font-family: var(--font-mono);
  font-size: 11px;
  outline: none;
  transition: var(--transition-colors);
}

.gen-col-pattern:focus {
  border-color: var(--brand-500);
}

.gen-preview-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin: var(--space-3) 0 var(--space-1);
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.gen-link {
  padding: 0;
  border: none;
  background: transparent;
  color: var(--brand-500);
  font-size: var(--text-xs);
  cursor: pointer;
}

.gen-link:hover {
  color: var(--brand-400);
}

.gen-preview {
  margin: 0;
  padding: var(--space-2) var(--space-3);
  max-height: 220px;
  overflow: auto;
  border: 1px solid var(--hairline);
  border-radius: var(--radius-sm);
  background: var(--canvas);
  color: var(--text-primary);
  font-family: var(--font-mono);
  font-size: 11px;
  line-height: 1.6;
  white-space: pre;
}

.gen-footer {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: var(--space-2);
  height: 44px;
  padding: 0 var(--space-3);
  border-top: 1px solid var(--hairline);
  flex-shrink: 0;
}

.gen-btn {
  height: var(--control-height-sm);
  padding: 0 var(--space-3);
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--text-xs);
  cursor: pointer;
  transition: var(--transition-colors);
}

.gen-btn:hover:not(:disabled) {
  background: var(--hover-fill);
  color: var(--text-primary);
}

.gen-btn.primary {
  background: var(--brand-500);
  color: #fff;
}

.gen-btn.primary:hover:not(:disabled) {
  background: var(--brand-600);
  color: #fff;
}

.gen-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.gen-notice {
  position: absolute;
  left: 50%;
  bottom: 64px;
  transform: translateX(-50%);
  padding: var(--space-1) var(--space-3);
  border-radius: var(--radius-sm);
  background: var(--overlay);
  border: 1px solid var(--hairline-strong);
  box-shadow: var(--shadow-lg);
  color: var(--text-primary);
  font-size: var(--text-xs);
}
</style>
