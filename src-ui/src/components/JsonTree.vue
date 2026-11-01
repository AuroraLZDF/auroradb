<template>
  <div class="json-tree-node">
    <template v-if="isContainer">
      <div class="json-tree-line json-tree-branch" @click="expanded = !expanded">
        <span class="json-tree-toggle">{{ expanded ? '▾' : '▸' }}</span>
        <span v-if="label !== undefined" class="json-tree-key">{{ label }}:</span>
        <span class="json-tree-summary">{{ summary }}</span>
      </div>
      <div v-if="expanded" class="json-tree-children">
        <JsonTree
          v-for="child in visibleChildren"
          :key="child.key"
          :value="child.value"
          :label="child.label"
          :depth="depth + 1"
        />
        <div v-if="hiddenCount > 0" class="json-tree-more">
          {{ t('cellViewer.treeMore', { count: hiddenCount }) }}
        </div>
      </div>
    </template>
    <div v-else class="json-tree-line">
      <span class="json-tree-toggle json-tree-toggle-empty"></span>
      <span v-if="label !== undefined" class="json-tree-key">{{ label }}:</span>
      <span class="json-tree-value" :class="valueClass">{{ display }}</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { t } from '@/i18n'

/**
 * JSON 树节点：对象/数组可折叠，标量直接展示。
 * 单层子节点数量超过 MAX_CHILDREN 时只渲染前 MAX_CHILDREN 项，避免超大 JSON 卡顿。
 */
const MAX_CHILDREN = 200
const DEFAULT_EXPAND_DEPTH = 2

const props = withDefaults(
  defineProps<{
    value: unknown
    label?: string
    depth?: number
  }>(),
  { depth: 0 }
)

const expanded = ref(props.depth < DEFAULT_EXPAND_DEPTH)

const isContainer = computed(
  () => Array.isArray(props.value) || (typeof props.value === 'object' && props.value !== null)
)

const entries = computed<{ key: string; label: string; value: unknown }[]>(() => {
  if (Array.isArray(props.value)) {
    return props.value.map((item, index) => ({
      key: String(index),
      label: String(index),
      value: item,
    }))
  }
  if (typeof props.value === 'object' && props.value !== null) {
    return Object.entries(props.value as Record<string, unknown>).map(([key, item]) => ({
      key,
      label: key,
      value: item,
    }))
  }
  return []
})

const visibleChildren = computed(() => entries.value.slice(0, MAX_CHILDREN))
const hiddenCount = computed(() => Math.max(0, entries.value.length - MAX_CHILDREN))

const summary = computed(() => {
  const count = entries.value.length
  if (Array.isArray(props.value)) return `[${t('cellViewer.treeItems', { count })}]`
  return `{${t('cellViewer.treeItems', { count })}}`
})

const display = computed(() => {
  if (props.value === null) return 'null'
  if (typeof props.value === 'string') return JSON.stringify(props.value)
  return String(props.value)
})

const valueClass = computed(() => {
  if (props.value === null) return 'json-null'
  if (typeof props.value === 'string') return 'json-string'
  if (typeof props.value === 'number') return 'json-number'
  if (typeof props.value === 'boolean') return 'json-boolean'
  return ''
})
</script>

<style scoped>
.json-tree-node {
  font-family: var(--font-mono);
  font-size: var(--text-base);
  line-height: 1.6;
}

.json-tree-line {
  display: flex;
  gap: var(--space-1);
  white-space: pre-wrap;
  word-break: break-all;
}

.json-tree-branch {
  cursor: pointer;
  user-select: none;
}

.json-tree-branch:hover {
  background: var(--surface-hover);
}

.json-tree-toggle {
  width: 12px;
  flex: none;
  color: var(--text-tertiary);
}

.json-tree-toggle-empty {
  display: inline-block;
}

.json-tree-children {
  padding-left: var(--space-4);
  border-left: 1px dashed var(--border-subtle);
  margin-left: 5px;
}

.json-tree-key {
  color: var(--brand-600);
}

.json-tree-summary {
  color: var(--text-tertiary);
}

.json-string {
  color: var(--success-600);
}

.json-number {
  color: var(--brand-600);
}

.json-boolean {
  color: var(--warning-600);
}

.json-null {
  color: var(--text-tertiary);
}

.json-tree-more {
  color: var(--text-tertiary);
  font-style: italic;
  padding-left: 12px;
}
</style>
