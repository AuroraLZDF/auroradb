<template>
  <div class="lineage-dialog">
    <!-- 头部 -->
    <div class="lineage-header">
      <div class="header-title">
        <Workflow :size="18" class="title-icon" />
        <h2>{{ t('lineage.title') }}</h2>
        <span class="header-hint">{{ t('lineage.hint') }}</span>
      </div>
      <button class="icon-btn" :title="t('common.close')" @click="emit('close')" :aria-label="t('common.close')">
        <X :size="18" />
      </button>
    </div>

    <!-- 控制区 -->
    <div class="lineage-bar">
      <div class="field">
        <label class="field-label">{{ t('lineage.dialectLabel') }}</label>
        <Select v-model="dialect" :options="dialectOptions" />
      </div>
      <div class="field field-db">
        <label class="field-label">{{ t('lineage.defaultDbLabel') }}</label>
        <input
          v-model="defaultDatabase"
          class="text-input"
          :placeholder="t('lineage.defaultDbPlaceholder')"
          spellcheck="false"
        />
      </div>
      <Button variant="primary" :loading="analyzing" :disabled="!sqlText.trim()" @click="runAnalyze">
        <Play :size="14" /> {{ t('lineage.analyze') }}
      </Button>
    </div>

    <!-- SQL 输入 -->
    <textarea
      v-model="sqlText"
      class="sql-input"
      :placeholder="t('lineage.sqlPlaceholder')"
      spellcheck="false"
      rows="6"
    ></textarea>

    <div v-if="errorMessage" class="lineage-error">
      <AlertCircle :size="16" />
      <span>{{ errorMessage }}</span>
    </div>

    <!-- 空状态 -->
    <div v-if="!result && !errorMessage" class="lineage-empty">
      <Workflow :size="40" class="empty-icon" />
      <p>{{ t('lineage.emptyTitle') }}</p>
      <span>{{ t('lineage.emptyHint') }}</span>
    </div>

    <template v-if="result">
      <div v-if="result.unparsedCount > 0" class="lineage-warning">
        <AlertCircle :size="14" />
        <span>{{ t('lineage.parseFailed', { count: result.unparsedCount }) }}</span>
      </div>

      <div v-if="!graph.edges.length" class="lineage-empty">
        <Workflow :size="40" class="empty-icon" />
        <p>{{ t('lineage.noStatements') }}</p>
      </div>

      <!-- 血缘图 -->
      <div v-else class="lineage-graph-wrap">
        <div class="graph-legend">
          <span class="legend-item"><span class="dot dot-source"></span>{{ t('lineage.roleSource') }}</span>
          <span class="legend-item"><span class="dot dot-both"></span>{{ t('lineage.roleBoth') }}</span>
          <span class="legend-item"><span class="dot dot-target"></span>{{ t('lineage.roleTarget') }}</span>
        </div>
        <div class="graph-scroll">
          <svg :width="layout.width" :height="layout.height" :viewBox="`0 0 ${layout.width} ${layout.height}`" class="lineage-svg">
            <defs>
              <marker id="lineage-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">
                <path d="M0,0 L8,4 L0,8 Z" class="arrow-head" />
              </marker>
            </defs>
            <path
              v-for="edge in layout.edges"
              :key="edge.key"
              :d="edge.path"
              class="edge-line"
              marker-end="url(#lineage-arrow)"
            />
            <g v-for="node in layout.nodes" :key="node.id" class="node-group" :class="`node-${node.role}`">
              <rect
                :x="node.x"
                :y="node.y"
                :width="node.width"
                :height="node.height"
                rx="6"
                class="node-box"
              />
              <text :x="node.x + 10" :y="node.y + node.height / 2 + 1" dominant-baseline="middle" class="node-label">
                {{ node.label }}
              </text>
            </g>
            <g v-for="edge in layout.edges" :key="`${edge.key}-label`">
              <text :x="edge.labelX" :y="edge.labelY" dominant-baseline="middle" text-anchor="middle" class="edge-label">
                {{ edge.label }}
              </text>
            </g>
          </svg>
        </div>
      </div>

      <!-- 语句明细 -->
      <div v-if="writableStatements.length" class="statements">
        <h3 class="statements-title">{{ t('lineage.statementsTitle') }}</h3>
        <div v-for="statement in writableStatements" :key="statement.index" class="statement-row">
          <span class="op-badge">{{ opLabel(statement.operation) }}</span>
          <span class="statement-flow">
            <template v-if="statement.target">
              <code>{{ displayTable(statement.target) }}</code>
              <span class="flow-arrow">←</span>
              <template v-if="statement.sources.length">
                <code v-for="source in statement.sources" :key="`${source.database}.${source.table}`">
                  {{ displayTable(source) }}
                </code>
              </template>
              <span v-else class="no-source">{{ t('lineage.noSource') }}</span>
            </template>
          </span>
        </div>
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { AlertCircle, Play, Workflow, X } from 'lucide-vue-next'
import Select from '@/components/ui/Select.vue'
import Button from '@/components/ui/Button.vue'
import { invoke } from '@tauri-apps/api/core'
import { t } from '@/i18n'
import {
  buildLineageGraph,
  type LineageAnalysis,
  type LineageGraphEdge,
  type LineageGraphNode,
} from '@/utils/lineageGraph'

interface Props {
  initialDialect?: 'mysql' | 'postgres' | 'sqlite'
  initialDatabase?: string
}

const props = withDefaults(defineProps<Props>(), {
  initialDialect: undefined,
  initialDatabase: ''
})

const emit = defineEmits<{ close: [] }>()

// 命令方言参数：mysql / postgresql；SQLite 语法按 MySQL 方言解析
const dialect = ref(props.initialDialect === 'postgres' ? 'postgresql' : 'mysql')
const dialectOptions = [
  { label: t('lineage.dialectMysql'), value: 'mysql' },
  { label: t('lineage.dialectPostgresql'), value: 'postgresql' }
]
const defaultDatabase = ref(props.initialDatabase ?? '')
const sqlText = ref('')
const analyzing = ref(false)
const errorMessage = ref('')
const result = ref<LineageAnalysis | null>(null)

const graph = computed(() => buildLineageGraph(result.value?.statements ?? []))

const writableStatements = computed(() =>
  (result.value?.statements ?? []).filter((statement) => statement.target !== null)
)

function opLabel(operation: string): string {
  const label = t(`lineage.op.${operation}`)
  // 词表缺失时回退为原始操作名
  return label.startsWith('lineage.op.') ? operation : label
}

function displayTable(ref: { database: string | null; table: string }): string {
  return ref.database ? `${ref.database}.${ref.table}` : ref.table
}

async function runAnalyze() {
  analyzing.value = true
  errorMessage.value = ''
  try {
    result.value = await invoke<LineageAnalysis>('analyze_sql_lineage', {
      sql: sqlText.value,
      dialect: dialect.value,
      defaultDatabase: defaultDatabase.value.trim() || null
    })
  } catch (error) {
    result.value = null
    errorMessage.value = error instanceof Error ? error.message : String(error)
  } finally {
    analyzing.value = false
  }
}

// ---------- SVG 布局（确定性网格：源表列 / 读写列 / 目标表列） ----------

const NODE_WIDTH = 200
const NODE_HEIGHT = 36
const NODE_GAP = 14
const COLUMN_GAP = 170
const PADDING = 24

interface LaidOutNode extends LineageGraphNode {
  label: string
  x: number
  y: number
  width: number
  height: number
}

interface LaidOutEdge {
  key: string
  path: string
  label: string
  labelX: number
  labelY: number
}

const layout = computed(() => {
  const byRole: Record<LineageGraphNode['role'], LaidOutNode[]> = {
    source: [],
    both: [],
    target: []
  }
  const columnX: Record<LineageGraphNode['role'], number> = {
    source: PADDING,
    both: PADDING + NODE_WIDTH + COLUMN_GAP,
    target: PADDING + (NODE_WIDTH + COLUMN_GAP) * 2
  }

  for (const node of graph.value.nodes) {
    const column = byRole[node.role]
    const index = column.length
    byRole[node.role].push({
      ...node,
      label: node.id,
      x: columnX[node.role],
      y: PADDING + index * (NODE_HEIGHT + NODE_GAP),
      width: NODE_WIDTH,
      height: NODE_HEIGHT
    })
  }

  const positions = new Map<string, LaidOutNode>()
  for (const column of [byRole.source, byRole.both, byRole.target]) {
    for (const node of column) positions.set(node.id, node)
  }

  const maxRows = Math.max(byRole.source.length, byRole.both.length, byRole.target.length, 1)
  const height = PADDING * 2 + maxRows * NODE_HEIGHT + Math.max(0, maxRows - 1) * NODE_GAP
  const width = PADDING * 2 + (NODE_WIDTH + COLUMN_GAP) * 2 + NODE_WIDTH

  const edges: LaidOutEdge[] = graph.value.edges.map((edge: LineageGraphEdge) => {
    const from = positions.get(edge.from)
    const to = positions.get(edge.to)
    const key = `${edge.from}->${edge.to}`
    if (!from || !to) {
      return { key, path: '', label: edge.operations.join(' / '), labelX: 0, labelY: 0 }
    }
    const x1 = from.x + from.width
    const y1 = from.y + from.height / 2
    const x2 = to.x
    const y2 = to.y + to.height / 2
    const midX = (x1 + x2) / 2
    return {
      key,
      path: `M ${x1} ${y1} C ${midX} ${y1}, ${midX} ${y2}, ${x2} ${y2}`,
      label: edge.operations.join(' / '),
      labelX: midX,
      labelY: (y1 + y2) / 2 - 8
    }
  })

  const nodes = [...byRole.source, ...byRole.both, ...byRole.target]
  return { nodes, edges, width, height }
})
</script>

<style scoped>
.lineage-dialog {
  display: flex;
  flex-direction: column;
  gap: 12px;
  height: 100%;
  min-height: 0;
}

.lineage-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.header-title {
  display: flex;
  align-items: center;
  gap: 8px;
}

.header-title h2 {
  margin: 0;
  font-size: 16px;
}

.title-icon {
  color: var(--accent, #4f8ef7);
}

.header-hint {
  font-size: 12px;
  color: var(--text-secondary, #888);
}

.icon-btn {
  border: none;
  background: transparent;
  color: inherit;
  cursor: pointer;
  padding: 4px;
  border-radius: 4px;
  display: flex;
}

.lineage-bar {
  display: flex;
  align-items: flex-end;
  gap: 12px;
  flex-wrap: wrap;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 140px;
}

.field-db {
  flex: 0 1 280px;
}

.field-label {
  font-size: 12px;
  color: var(--text-secondary, #888);
}

.text-input {
  width: 100%;
  padding: 6px 10px;
  border: 1px solid var(--border-color, #444);
  border-radius: 6px;
  background: var(--bg-input, transparent);
  color: inherit;
  font-size: 13px;
}

.sql-input {
  width: 100%;
  min-height: 110px;
  padding: 10px 12px;
  border: 1px solid var(--border-color, #444);
  border-radius: 8px;
  background: var(--bg-input, transparent);
  color: inherit;
  font-family: var(--font-mono, monospace);
  font-size: 12.5px;
  line-height: 1.5;
  resize: vertical;
  box-sizing: border-box;
}

.lineage-error,
.lineage-warning {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  padding: 8px 12px;
  border-radius: 6px;
}

.lineage-error {
  color: var(--error-color, #e05656);
  background: rgba(224, 86, 86, 0.1);
}

.lineage-warning {
  color: var(--warning-color, #d9a13c);
  background: rgba(217, 161, 60, 0.1);
}

.lineage-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  padding: 36px 0;
  color: var(--text-secondary, #888);
}

.lineage-empty p {
  margin: 0;
  font-size: 14px;
}

.lineage-empty span {
  font-size: 12px;
}

.empty-icon {
  opacity: 0.5;
}

.lineage-graph-wrap {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
  flex: 1;
}

.graph-legend {
  display: flex;
  gap: 16px;
  font-size: 12px;
  color: var(--text-secondary, #888);
}

.legend-item {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  display: inline-block;
}

.dot-source {
  background: var(--accent-blue, #56a8f5);
}

.dot-both {
  background: var(--accent-purple, #a88bf0);
}

.dot-target {
  background: var(--accent-green, #5cc98c);
}

.graph-scroll {
  overflow: auto;
  flex: 1;
  min-height: 160px;
  border: 1px solid var(--border-color, #333);
  border-radius: 8px;
  background: var(--bg-graph, rgba(127, 127, 127, 0.04));
}

.lineage-svg {
  display: block;
}

.edge-line {
  fill: none;
  stroke: var(--border-color, #666);
  stroke-width: 1.6;
}

.arrow-head {
  fill: var(--border-color, #666);
}

.node-box {
  fill: var(--bg-node, rgba(127, 127, 127, 0.08));
  stroke: var(--border-color, #555);
}

.node-source .node-box {
  stroke: var(--accent-blue, #56a8f5);
}

.node-both .node-box {
  stroke: var(--accent-purple, #a88bf0);
}

.node-target .node-box {
  stroke: var(--accent-green, #5cc98c);
}

.node-label {
  fill: var(--text-primary, #ddd);
  font-size: 12.5px;
}

.edge-label {
  fill: var(--text-secondary, #999);
  font-size: 11px;
}

.statements {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 220px;
  overflow: auto;
}

.statements-title {
  margin: 0;
  font-size: 13px;
  color: var(--text-secondary, #888);
}

.statement-row {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 12.5px;
  flex-wrap: wrap;
}

.op-badge {
  padding: 1px 8px;
  border-radius: 10px;
  font-size: 11px;
  background: rgba(86, 168, 245, 0.15);
  color: var(--accent-blue, #56a8f5);
}

.statement-flow {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}

.statement-flow code {
  padding: 1px 6px;
  border-radius: 4px;
  background: rgba(127, 127, 127, 0.1);
  font-size: 12px;
}

.flow-arrow {
  color: var(--text-secondary, #888);
}

.no-source {
  color: var(--text-secondary, #888);
  font-size: 12px;
}
</style>
