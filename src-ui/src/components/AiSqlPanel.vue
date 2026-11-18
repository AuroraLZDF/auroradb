<template>
  <div class="ai-sql-panel">
    <header class="ai-sql-head">
      <Sparkles :size="15" class="ai-sql-icon" />
      <h3>{{ t('aiSql.title') }}</h3>
      <button class="ai-sql-close" :title="t('common.close')" :aria-label="t('common.close')" @click="emit('close')">
        <X :size="15" />
      </button>
    </header>

    <textarea
      v-model="question"
      class="ai-sql-input"
      rows="3"
      :placeholder="t('aiSql.placeholder')"
      :disabled="generating"
      @keydown.enter.exact.prevent="generate"
    ></textarea>

    <div class="ai-sql-actions">
      <button class="ai-sql-btn primary" :disabled="generating || !question.trim()" @click="generate">
        <Loader2 v-if="generating" :size="14" class="spinning" />
        <Wand2 v-else :size="14" />
        {{ generating ? t('aiSql.generating') : t('aiSql.generate') }}
      </button>
      <span class="ai-sql-hint">{{ t('aiSql.schemaHint', { database }) }}</span>
    </div>

    <p v-if="errorMsg" class="ai-sql-error">{{ errorMsg }}</p>

    <template v-if="resultSql">
      <pre class="ai-sql-result">{{ resultSql }}</pre>
      <div class="ai-sql-actions">
        <button class="ai-sql-btn primary" @click="emit('insert', resultSql)">
          <ArrowDownToLine :size="14" />
          {{ t('aiSql.insert') }}
        </button>
        <button class="ai-sql-btn" @click="copySql">
          <Copy :size="14" />
          {{ t('common.copy') }}
        </button>
        <span class="ai-sql-hint">{{ t('aiSql.reviewHint') }}</span>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
/**
 * NL2SQL 面板（V3-2.2）：自然语言 → SQL。
 * 生成前拉取当前库 schema 快照注入精简上下文（限 100 表 × 40 列控制 token），
 * 结果先预览再插入编辑器，强制人工确认后才执行。
 */
import { ref } from 'vue'
import { ArrowDownToLine, Copy, Loader2, Sparkles, Wand2, X } from 'lucide-vue-next'
import { invoke } from '@tauri-apps/api/core'
import { globalToast } from '@/composables/useToast'
import { useDataBrowserStore, type SchemaSnapshot } from '@/stores/dataBrowser'
import { t } from '@/i18n'

const props = defineProps<{
  connectionId: string
  database: string
  driver: 'mysql' | 'postgres'
}>()

const emit = defineEmits<{ close: []; insert: [sql: string] }>()

const browserStore = useDataBrowserStore()
const question = ref('')
const generating = ref(false)
const resultSql = ref('')
const errorMsg = ref('')

const AI_MAX_TABLES = 100
const AI_MAX_COLUMNS = 40

const schemaContext = ref('')
let schemaDb = ''

/** 把 schema 快照压成紧凑文本上下文 */
function buildSchemaContext(snapshot: SchemaSnapshot): string {
  const lines: string[] = []
  const tables = snapshot.tables.slice(0, AI_MAX_TABLES)
  for (const table of tables) {
    const cols = table.columns.slice(0, AI_MAX_COLUMNS)
      .map((c) => `${c.name} ${c.data_type}`)
      .join(', ')
    let line = `- ${table.name}(${cols})`
    const fks = (table.foreign_keys ?? []).slice(0, 10)
    if (fks.length > 0) {
      line += `；外键: ${fks.map((fk) => `${fk.column}->${fk.ref_table}.${fk.ref_column}`).join(', ')}`
    }
    lines.push(line)
  }
  if (snapshot.tables.length > AI_MAX_TABLES) {
    lines.push(`（其余 ${snapshot.tables.length - AI_MAX_TABLES} 张表已省略）`)
  }
  return lines.join('\n')
}

async function ensureSchemaContext(): Promise<boolean> {
  if (schemaDb === props.database && schemaContext.value) return true
  try {
    const snapshot = await browserStore.getSchemaSnapshot(props.connectionId, props.database)
    schemaContext.value = buildSchemaContext(snapshot)
    schemaDb = props.database
    return true
  } catch (e) {
    errorMsg.value = String(e)
    return false
  }
}

function systemPrompt(): string {
  const dialect = props.driver === 'postgres' ? 'PostgreSQL' : 'MySQL'
  return [
    `你是资深的 ${dialect} 数据库专家。根据用户的自然语言需求，生成一条可以在以下数据库上执行的 ${dialect} SQL 查询。`,
    '要求：',
    '1. 只输出 SQL 语句本身，不要任何解释、不要 Markdown 代码块。',
    '2. 只生成只读的 SELECT 语句；用户明确要求写入时也不要生成 INSERT/UPDATE/DELETE。',
    '3. 只使用下方给出的表和列，不要臆造不存在的表、列或函数。',
    '4. 表名与列名使用反引号包裹（MySQL）或双引号包裹（PostgreSQL）。',
  ].join('\n')
}

function extractSql(raw: string): string {
  const fenced = raw.match(/```(?:sql)?\s*([\s\S]*?)```/i)
  return (fenced ? fenced[1]! : raw).trim()
}

async function generate() {
  const q = question.value.trim()
  if (!q || generating.value) return
  generating.value = true
  errorMsg.value = ''
  resultSql.value = ''
  try {
    const ok = await ensureSchemaContext()
    if (!ok) return
    const prompt = `数据库「${props.database}」的表结构如下：\n${schemaContext.value}\n\n用户需求：${q}`
    const raw = await invoke<string>('ai_chat', { prompt, system: systemPrompt() })
    const sql = extractSql(raw)
    if (!sql) {
      errorMsg.value = t('aiSql.emptyResult')
      return
    }
    resultSql.value = sql
  } catch (e) {
    errorMsg.value = String(e)
  } finally {
    generating.value = false
  }
}

async function copySql() {
  try {
    await navigator.clipboard.writeText(resultSql.value)
    globalToast.success(t('aiSql.copied'))
  } catch {
    globalToast.error(t('aiSql.copyFailed'))
  }
}
</script>

<style scoped>
.ai-sql-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.ai-sql-head {
  display: flex;
  align-items: center;
  gap: 6px;
}

.ai-sql-head h3 {
  margin: 0;
  flex: 1;
  font-size: var(--text-base);
  color: var(--text-primary);
}

.ai-sql-icon {
  color: var(--brand-600);
}

.ai-sql-close {
  display: flex;
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  padding: 4px;
  border-radius: var(--radius-sm);
}

.ai-sql-close:hover {
  color: var(--text-primary);
  background: var(--surface-hover);
}

.ai-sql-input {
  resize: vertical;
  min-height: 60px;
  padding: var(--space-2);
  border: 1px solid var(--hairline);
  border-radius: var(--radius-sm);
  background: var(--panel);
  color: var(--text-primary);
  font-size: var(--text-sm);
  font-family: inherit;
  line-height: 1.6;
}

.ai-sql-input:focus {
  outline: none;
  border-color: var(--brand-600);
}

.ai-sql-actions {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.ai-sql-btn {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 6px var(--space-3);
  border: 1px solid var(--hairline);
  border-radius: var(--radius-sm);
  background: var(--panel);
  color: var(--text-secondary);
  font-size: var(--text-sm);
  cursor: pointer;
}

.ai-sql-btn:hover:not(:disabled) {
  background: var(--surface-hover);
}

.ai-sql-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.ai-sql-btn.primary {
  color: var(--brand-600);
  border-color: var(--brand-600);
}

.ai-sql-hint {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.ai-sql-error {
  margin: 0;
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-sm);
  background: rgba(229, 72, 77, 0.1);
  color: var(--danger, #e5484d);
  font-size: var(--text-xs);
  line-height: 1.6;
  word-break: break-all;
}

.ai-sql-result {
  margin: 0;
  padding: var(--space-2) var(--space-3);
  max-height: 200px;
  overflow: auto;
  border: 1px solid var(--hairline);
  border-radius: var(--radius-sm);
  background: var(--panel);
  font-size: var(--text-sm);
  font-family: var(--font-mono, monospace);
  color: var(--text-primary);
  white-space: pre-wrap;
  word-break: break-all;
}

.spinning {
  animation: ai-sql-spin 1s linear infinite;
}

@keyframes ai-sql-spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}
</style>
