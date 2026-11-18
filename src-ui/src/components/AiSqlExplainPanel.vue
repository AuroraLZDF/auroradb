<template>
  <div class="ai-explain-panel">
    <header class="ai-explain-head">
      <MessageSquareText :size="15" class="ai-explain-icon" />
      <h3>{{ t('aiExplain.title') }}</h3>
      <button class="ai-explain-close" :title="t('common.close')" :aria-label="t('common.close')" @click="emit('close')">
        <X :size="15" />
      </button>
    </header>

    <pre class="ai-explain-sql">{{ truncatedSql }}</pre>

    <div class="ai-explain-actions">
      <button class="ai-explain-btn primary" :disabled="generating || !sql.trim()" @click="run('explain')">
        <Loader2 v-if="generating && mode === 'explain'" :size="14" class="spinning" />
        <MessageSquareText v-else :size="14" />
        {{ t('aiExplain.explain') }}
      </button>
      <button class="ai-explain-btn primary" :disabled="generating || !sql.trim()" @click="run('optimize')">
        <Loader2 v-if="generating && mode === 'optimize'" :size="14" class="spinning" />
        <Gauge v-else :size="14" />
        {{ t('aiExplain.optimize') }}
      </button>
    </div>

    <p v-if="errorMsg" class="ai-explain-error">{{ errorMsg }}</p>

    <template v-if="result">
      <pre class="ai-explain-result">{{ result }}</pre>
      <div class="ai-explain-actions">
        <button class="ai-explain-btn" @click="copyResult">
          <Copy :size="14" />
          {{ t('common.copy') }}
        </button>
        <span class="ai-explain-hint">{{ t('aiExplain.hint') }}</span>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
/**
 * SQL 解释与优化（V3-2.3）：复用 ai_chat，对编辑器当前 SQL 提供
 * 「逐句解释」与「性能优化建议」两种模式，结果纯文本展示 + 复制。
 */
import { computed, ref } from 'vue'
import { Copy, Gauge, Loader2, MessageSquareText, X } from 'lucide-vue-next'
import { invoke } from '@tauri-apps/api/core'
import { globalToast } from '@/composables/useToast'
import { t } from '@/i18n'

const props = defineProps<{
  sql: string
  driver: 'mysql' | 'postgres'
}>()

const emit = defineEmits<{ close: [] }>()

type Mode = 'explain' | 'optimize'
const mode = ref<Mode>('explain')
const generating = ref(false)
const result = ref('')
const errorMsg = ref('')

const SQL_PREVIEW_LEN = 400

const truncatedSql = computed(() => {
  const s = props.sql.trim()
  return s.length > SQL_PREVIEW_LEN ? `${s.slice(0, SQL_PREVIEW_LEN)}\n…（${t('aiExplain.truncated', { total: s.length })}）` : s
})

function systemPrompt(m: Mode): string {
  const dialect = props.driver === 'postgres' ? 'PostgreSQL' : 'MySQL'
  if (m === 'explain') {
    return [
      `你是资深的 ${dialect} 数据库专家。请用简体中文逐步解释这条 SQL 的作用：`,
      '1. 按子句（SELECT/WHERE/JOIN/GROUP BY 等）逐段说明意图。',
      '2. 说明最终返回的结果集是什么。',
      '3. 只输出解释文本，不要生成新的 SQL。',
    ].join('\n')
  }
  return [
    `你是资深的 ${dialect} 性能优化专家。请分析这条 SQL 的潜在性能问题（如缺索引、函数包裹列导致索引失效、SELECT *、不必要的 JOIN/子查询、深分页等）：`,
    '1. 用简体中文输出简短的问题清单，每条一行。',
    '2. 如有更好的写法，在最后用 ```sql 代码围栏给出优化后的完整 SQL。',
    '3. 只基于这条 SQL 本身分析，不要臆造表结构之外的列。',
  ].join('\n')
}

async function run(m: Mode) {
  if (generating.value || !props.sql.trim()) return
  mode.value = m
  generating.value = true
  errorMsg.value = ''
  result.value = ''
  try {
    result.value = (await invoke<string>('ai_chat', {
      prompt: props.sql.trim(),
      system: systemPrompt(m),
    })) || t('aiExplain.emptyResult')
  } catch (e) {
    errorMsg.value = String(e)
  } finally {
    generating.value = false
  }
}

async function copyResult() {
  try {
    await navigator.clipboard.writeText(result.value)
    globalToast.success(t('aiSql.copied'))
  } catch {
    globalToast.error(t('aiSql.copyFailed'))
  }
}
</script>

<style scoped>
.ai-explain-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.ai-explain-head {
  display: flex;
  align-items: center;
  gap: 6px;
}

.ai-explain-head h3 {
  margin: 0;
  flex: 1;
  font-size: var(--text-base);
  color: var(--text-primary);
}

.ai-explain-icon {
  color: var(--brand-600);
}

.ai-explain-close {
  display: flex;
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  padding: 4px;
  border-radius: var(--radius-sm);
}

.ai-explain-close:hover {
  color: var(--text-primary);
  background: var(--surface-hover);
}

.ai-explain-sql {
  margin: 0;
  padding: var(--space-2) var(--space-3);
  max-height: 140px;
  overflow: auto;
  border: 1px solid var(--hairline);
  border-radius: var(--radius-sm);
  background: var(--panel);
  font-size: var(--text-xs);
  font-family: var(--font-mono, monospace);
  color: var(--text-secondary);
  white-space: pre-wrap;
  word-break: break-all;
}

.ai-explain-actions {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.ai-explain-btn {
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

.ai-explain-btn:hover:not(:disabled) {
  background: var(--surface-hover);
}

.ai-explain-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.ai-explain-btn.primary {
  color: var(--brand-600);
  border-color: var(--brand-600);
}

.ai-explain-hint {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.ai-explain-error {
  margin: 0;
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-sm);
  background: rgba(229, 72, 77, 0.1);
  color: var(--danger, #e5484d);
  font-size: var(--text-xs);
  line-height: 1.6;
  word-break: break-all;
}

.ai-explain-result {
  margin: 0;
  padding: var(--space-2) var(--space-3);
  max-height: 320px;
  overflow: auto;
  border: 1px solid var(--hairline);
  border-radius: var(--radius-sm);
  background: var(--panel);
  font-size: var(--text-sm);
  font-family: inherit;
  color: var(--text-primary);
  white-space: pre-wrap;
  word-break: break-word;
  line-height: 1.7;
}

.spinning {
  animation: ai-explain-spin 1s linear infinite;
}

@keyframes ai-explain-spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}
</style>
