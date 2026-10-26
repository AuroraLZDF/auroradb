<template>
  <div class="explain-node" :style="{ paddingLeft: depth * 20 + 'px' }">
    <div class="node-row" :class="{ highlight: isHighCost }">
      <span class="node-operation">{{ node.operation }}</span>
      <span v-if="node.table" class="node-table">
        <Folder :size="12" /> {{ node.table }}
      </span>
      <span v-if="node.index" class="node-index">{{ node.index }}</span>
      <span v-if="node.rows !== undefined" class="node-rows">
        <span class="node-label">rows</span> {{ formatNum(node.rows) }}
      </span>
      <span v-if="node.cost !== undefined" class="node-cost" :class="{ high: isHighCost }">
        <span class="node-label">cost</span> {{ formatCost(node.cost) }}
      </span>
      <span v-if="node.filtered !== undefined" class="node-filtered">
        filtered {{ node.filtered.toFixed(0) }}%
      </span>
    </div>
    <div v-if="node.extra.length > 0" class="node-extra">
      <span v-for="(e, i) in node.extra" :key="i" class="extra-chip">{{ e }}</span>
    </div>
    <div v-if="node.children && node.children.length > 0" class="node-children">
      <ExplainNodeRow
        v-for="(c, i) in node.children"
        :key="i"
        :node="c"
        :depth="depth + 1"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { Folder } from 'lucide-vue-next'
import type { ExplainNode } from './ExplainTree.vue'

defineProps<{
  node: ExplainNode
  depth: number
}>()

function formatNum(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + 'M'
  if (n >= 1_000) return (n / 1_000).toFixed(1) + 'K'
  return String(n)
}

function formatCost(c: number): string {
  return c >= 1000 ? (c / 1000).toFixed(1) + 'K' : c.toFixed(2)
}

// cost / rows 粗略的"高成本"判定：父节点的 rows * cost 越大越红
const isHighCost = false  // 简化：暂不做跨节点相对成本对比
</script>

<style scoped>
.explain-node {
  border-bottom: 1px solid var(--border-default);
}

.explain-node:last-child {
  border-bottom: none;
}

.node-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 12px;
  flex-wrap: wrap;
  transition: background 150ms;
}

.node-row:hover {
  background: var(--surface-hover);
}

.node-row.highlight {
  background: rgba(234, 88, 12, 0.08);
}

.node-operation {
  font-weight: 600;
  color: var(--text-primary);
  font-size: 13px;
}

.node-table {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  color: var(--text-secondary);
  font-size: 12px;
}

.node-index {
  color: var(--brand-primary);
  font-size: 12px;
  padding: 1px 6px;
  border-radius: 10px;
  background: var(--brand-dim);
}

.node-rows, .node-cost, .node-filtered {
  color: var(--text-tertiary);
  font-size: 11px;
}

.node-label {
  color: var(--text-tertiary);
  margin-right: 2px;
}

.node-cost.high {
  color: #ea580c;
  font-weight: 600;
}

.node-extra {
  display: flex;
  gap: 6px;
  padding: 0 12px 8px;
  flex-wrap: wrap;
}

.extra-chip {
  font-size: 11px;
  padding: 1px 8px;
  border-radius: 10px;
  background: var(--surface-hover);
  color: var(--text-secondary);
}

.node-children {
  /* 递归嵌套自然通过 paddingLeft 缩进 */
}
</style>
