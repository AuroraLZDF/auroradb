<template>
  <div class="explain-tree">
    <div v-if="!plan" class="explain-empty">
      {{ t('sqlEditor.noExplainResult') }}
    </div>
    <div v-else class="explain-result">
      <div class="explain-header">
        <span class="explain-driver">{{ plan.driver.toUpperCase() }}</span>
        <span class="explain-time">{{ t('sqlEditor.explainTime', { ms: plan.execution_time_ms }) }}</span>
        <button class="btn-toggle-raw" @click="showRaw = !showRaw">
          {{ showRaw ? t('sqlEditor.hideRaw') : t('sqlEditor.showRaw') }}
        </button>
      </div>

      <!-- 树形视图 -->
      <div v-if="!showRaw" class="explain-nodes">
        <ExplainNodeRow
          v-for="(n, i) in plan.nodes"
          :key="'root-' + i"
          :node="n"
          :depth="0"
        />
      </div>

      <!-- 原始 JSON -->
      <pre v-else class="explain-raw">{{ plan.raw_json }}</pre>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { t } from '@/i18n'
import type { ExplainPlan } from '@/utils/explain'
import ExplainNodeRow from './ExplainNodeRow.vue'

defineProps<{
  plan: ExplainPlan | null
}>()

const showRaw = ref(false)
</script>

<style scoped>
.explain-tree {
  padding: 12px;
  font-size: 13px;
}

.explain-empty {
  text-align: center;
  color: var(--text-tertiary);
  padding: 20px;
}

.explain-header {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 12px;
  background: var(--surface-secondary);
  border: 1px solid var(--border-default);
  border-radius: 8px;
  margin-bottom: 10px;
}

.explain-driver {
  font-weight: 600;
  color: var(--brand-500);
  font-size: 12px;
  padding: 2px 8px;
  border-radius: 4px;
  background: var(--selected-fill);
}

.explain-time {
  color: var(--text-secondary);
  font-size: 12px;
}

.btn-toggle-raw {
  margin-left: auto;
  font-size: 12px;
  color: var(--text-tertiary);
  background: transparent;
  border: 1px solid var(--border-default);
  border-radius: 4px;
  padding: 3px 10px;
  cursor: pointer;
}

.btn-toggle-raw:hover {
  color: var(--text-primary);
  border-color: var(--hairline-strong);
}

.explain-nodes {
  background: var(--bg-primary);
  border: 1px solid var(--border-default);
  border-radius: 8px;
  overflow: hidden;
}

.explain-raw {
  background: var(--bg-secondary);
  border: 1px solid var(--border-default);
  border-radius: 8px;
  padding: 12px;
  font-size: 12px;
  font-family: monospace;
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 60vh;
  overflow: auto;
  color: var(--text-secondary);
}
</style>
