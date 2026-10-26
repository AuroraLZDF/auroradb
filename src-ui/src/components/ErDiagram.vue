<template>
  <div class="er-diagram">
    <!-- 工具栏 -->
    <div class="er-toolbar">
      <button @click="loadData" :disabled="loading" class="btn-refresh">
        <RefreshCw :size="14" :class="{ spinning: loading }" />
        {{ t('common.refresh') }}
      </button>
      <div class="er-stats" v-if="data">
        <span>{{ t('erDiagram.tables', { count: data.tables.length }) }}</span>
        <span>{{ t('erDiagram.fks', { count: data.foreign_keys.length }) }}</span>
      </div>
      <div class="er-empty" v-if="!data && !loading">
        {{ t('erDiagram.empty') }}
      </div>
      <div class="er-driver" v-if="data">{{ data.driver.toUpperCase() }}</div>
      <button @click="resetView" class="btn-reset">
        <Maximize :size="14" />
      </button>
    </div>

    <!-- SVG 画布（支持缩放+拖拽） -->
    <div
      ref="canvasRef"
      class="er-canvas"
      @wheel.prevent="onWheel"
      @mousedown="onPanStart"
      @mousemove="onPanMove"
      @mouseup="onPanEnd"
      @mouseleave="onPanEnd"
    >
      <svg
        :style="{ transform: `translate(${tx}px, ${ty}px) scale(${scale})`, transformOrigin: '0 0' }"
        :viewBox="`0 0 ${svgW} ${svgH}`"
      >
        <!-- 外键连线（先画，在表框下） -->
        <g class="er-links">
          <path
            v-for="(edge, i) in edgePaths"
            :key="'edge-' + i"
            :d="edge.path"
            :class="{ active: edge.active }"
            @click.stop="highlightEdge(i)"
          />
        </g>

        <!-- 表框 -->
        <g
          v-for="node in positionedTables"
          :key="node.table.name"
          :transform="`translate(${node.x}, ${node.y})`"
          :class="{ highlighted: highlightedTable === node.table.name }"
          @click.stop="highlightTable(node.table.name)"
        >
          <rect
            :width="TABLE_W"
            :height="node.h"
            class="er-table-bg"
            rx="6"
            :class="{ highlighted: highlightedTable === node.table.name }"
          />
          <!-- 表头 -->
          <rect
            :width="TABLE_W"
            height="32"
            class="er-table-header"
            rx="6"
          />
          <text x="12" y="21" class="er-table-title">{{ node.table.name }}</text>
          <!-- 列 -->
          <g v-for="(col, ci) in node.table.columns" :key="col.name">
            <text
              x="12"
              :y="32 + (ci + 1) * ROW_H - 5"
              class="er-col"
              :class="{ pk: col.is_primary }"
            >
              <tspan v-if="col.is_primary" class="er-pk-mark">🔑 </tspan>
              {{ col.name }}
            </text>
            <text
              :x="TABLE_W - 12"
              :y="32 + (ci + 1) * ROW_H - 5"
              class="er-col-type"
            >{{ col.data_type }}</text>
          </g>
        </g>
      </svg>

      <!-- 加载/错误浮层 -->
      <div v-if="loading" class="er-overlay">
        <Loader2 :size="28" class="spinning" />
        <span>{{ t('erDiagram.loading') }}</span>
      </div>
      <div v-if="error" class="er-overlay error">
        <XCircle :size="20" />
        <span>{{ error }}</span>
      </div>
    </div>

    <!-- 缩放指示 -->
    <div class="er-zoom-indicator">{{ Math.round(scale * 100) }}%</div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { RefreshCw, Loader2, XCircle, Maximize } from 'lucide-vue-next'
import { invoke } from '@tauri-apps/api/core'
import { useConnectionStore } from '@/stores/connection'
import { useLicenseStore } from '@/stores/license'
import { t } from '@/i18n'

// ========== Props / Emits ==========
const props = defineProps<{
  /** 当前选中的数据库 */
  database: string
}>()

const emit = defineEmits<{
  (e: 'data-loaded', tables: number): void
}>()

// ========== 类型 ==========
interface ErColumn { name: string; data_type: string; is_primary: boolean }
interface ErTableNode { name: string; columns: ErColumn[] }
interface ErForeignKey { name: string; table: string; column: string; ref_table: string; ref_column: string }
interface ErDiagramData {
  driver: 'mysql' | 'postgres'
  database: string
  tables: ErTableNode[]
  foreign_keys: ErForeignKey[]
}

interface PositionedTable { table: ErTableNode; x: number; y: number; h: number }
interface EdgePath { path: string; active: boolean }

// ========== 常量 ==========
const TABLE_W = 220
const ROW_H = 20
const COL_GAP = 28
const LAYER_GAP = 80

// ========== 状态 ==========
const data = ref<ErDiagramData | null>(null)
const loading = ref(false)
const error = ref('')
const canvasRef = ref<HTMLDivElement>()

// 画布变换
const scale = ref(1)
const tx = ref(40)
const ty = ref(40)
let dragging = false
let lastX = 0
let lastY = 0

// 高亮
const highlightedTable = ref<string | null>(null)

// ========== 数据加载 ==========
async function loadData() {
  if (!props.database) {
    error.value = t('erDiagram.noDatabase')
    return
  }
  const connStore = useConnectionStore()
  if (!connStore.currentConnection?.id) {
    error.value = t('erDiagram.noConnection')
    return
  }

  loading.value = true
  error.value = ''
  try {
    const resp = await invoke<ErDiagramData>('get_er_diagram_data', {
      connectionId: connStore.currentConnection.id,
      database: props.database,
    })
    data.value = resp
    resetView()
    emit('data-loaded', resp.tables.length)
  } catch (e) {
    error.value = String(e)
    data.value = null
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  const license = useLicenseStore()
  if (!license.allows('er-diagram')) {
    license.openDialog()
    return
  }
  loadData()
})

// 数据库切换时自动刷新
watch(() => props.database, () => {
  if (props.database) loadData()
})

// ========== 布局算法 ==========
// 分层拓扑：无外键的表放 layer 0，被引用表在左、引用者在右
function computeLayers(keys: string[], fks: ErForeignKey[]): Map<string, number> {
  const layer = new Map<string, number>()
  // 入度（被引用次数）
  const indeg = new Map<string, number>()
  keys.forEach(k => indeg.set(k, 0))
  for (const fk of fks) {
    if (indeg.has(fk.ref_table)) indeg.set(fk.ref_table, (indeg.get(fk.ref_table) ?? 0) + 1)
  }
  // 初始：没有被任何表引用的表作为 layer 0（它们可能引用别人，但没人引用它们）
  // 反直觉的做法：被引用越多越靠左（是"源"），主动引用别人的放右边（从）
  // 所以 layer 决定于 fk.source_table 的 layer 必须 > fk.ref_table 的 layer
  // 初始 layer 0：没有外键（不引用别人）的表
  const has_fk = new Set(fks.map(f => f.table))
  const queue: string[] = keys.filter(k => !has_fk.has(k))
  for (const k of queue) layer.set(k, 0)
  // 迭代：如果表 A 引用表 B，A.layer = B.layer + 1
  let changed = true
  let iter = 0
  while (changed && iter < 100) {
    changed = false
    iter++
    for (const fk of fks) {
      const ref_l = layer.get(fk.ref_table)
      if (ref_l === undefined) continue
      const new_l = ref_l + 1
      const cur = layer.get(fk.table)
      if (cur === undefined || new_l > cur) {
        layer.set(fk.table, new_l)
        changed = true
      }
    }
  }
  // 剩余未分配的表（环形引用等）放 layer 0 兜底
  for (const k of keys) {
    if (!layer.has(k)) layer.set(k, 0)
  }
  return layer
}

const positionedTables = computed<PositionedTable[]>(() => {
  if (!data.value) return []
  const { tables, foreign_keys: fks } = data.value
  const keys = tables.map(t => t.name)
  const layerMap = computeLayers(keys, fks)

  // 按 layer 分组，同层按表名排序
  const layers: string[][] = []
  for (const [name, l] of layerMap) {
    while (layers.length <= l) layers.push([])
    layers[l]!.push(name)
  }
  layers.forEach(arr => arr.sort())

  const pos = new Map<string, { x: number; y: number; h: number }>()
  const layerHeights: number[] = layers.map(arr =>
    arr.reduce((acc, name) => {
      const t = tables.find(x => x.name === name)!
      return Math.max(acc, 32 + t.columns.length * ROW_H + 16)
    }, 0)
  )

  // 计算画布尺寸
  let totalH = 0
  for (let li = 0; li < layers.length; li++) {
    const rowH = layerHeights[li]!
    const n = layers[li]!.length
    totalH = Math.max(totalH, rowH * n + (n - 1) * LAYER_GAP)
  }

  // 居中计算
  for (let li = 0; li < layers.length; li++) {
    const rowH = layerHeights[li]!
    const n = layers[li]!.length
    const rowTotalH = n * rowH + (n - 1) * LAYER_GAP
    const startY = (totalH - rowTotalH) / 2
    for (let ti = 0; ti < n; ti++) {
      const name = layers[li]![ti]!
      const t = tables.find(x => x.name === name)!
      const h = 32 + t.columns.length * ROW_H + 16
      const x = li * (TABLE_W + COL_GAP)
      const y = startY + ti * (rowH + LAYER_GAP)
      pos.set(name, { x, y, h })
    }
  }

  return tables.map(t => {
    const p = pos.get(t.name)!
    return { table: t, x: p.x, y: p.y, h: p.h }
  })
})

const svgW = ref(800)
const svgH = ref(600)

// 画布尺寸独立 computed，避免 positionedTables 内的副作用违反 lint
const canvasSize = computed(() => {
  if (!data.value) return { w: 800, h: 600 }
  const { tables, foreign_keys: fks } = data.value
  const keys = tables.map(t => t.name)
  const layerMap = computeLayers(keys, fks)
  const layers: string[][] = []
  for (const [name, l] of layerMap) {
    while (layers.length <= l) layers.push([])
    layers[l]!.push(name)
  }
  const totalW = layers.length * (TABLE_W + COL_GAP)
  let totalH = 0
  for (let li = 0; li < layers.length; li++) {
    const rowH = 32 + tables.filter(t => layerMap.get(t.name) === li).reduce((acc, t) => Math.max(acc, t.columns.length * ROW_H + 16), 0)
    const n = layers[li]!.length
    totalH = Math.max(totalH, rowH * n + (n - 1) * LAYER_GAP)
  }
  return { w: Math.max(600, totalW + 80), h: Math.max(400, totalH + 80) }
})

// 画布尺寸更新通过 watch 而不是在 positionedTables computed 里做副作用
watch(canvasSize, size => {
  svgW.value = size.w
  svgH.value = size.h
}, { immediate: true })

// ========== 外键连线 ==========
function tableAnchor(tableName: string, side: 'left' | 'right', colName?: string): { x: number; y: number } {
  const node = positionedTables.value.find(n => n.table.name === tableName)
  if (!node) return { x: 0, y: 0 }
  const x = side === 'left' ? node.x : node.x + TABLE_W
  let y = node.y + 20 // 表头中心
  if (colName) {
    const ci = node.table.columns.findIndex(c => c.name === colName)
    if (ci >= 0) {
      y = node.y + 32 + (ci + 0.5) * ROW_H
    }
  }
  return { x, y }
}

const edgePaths = computed<EdgePath[]>(() => {
  if (!data.value) return []
  return data.value.foreign_keys.map(fk => {
    // 从源表右边缘 FK 列位置 → 目标表左边缘 PK 列位置
    const src = tableAnchor(fk.table, 'right', fk.column)
    const dst = tableAnchor(fk.ref_table, 'left', fk.ref_column)
    const midX = (src.x + dst.x) / 2
    // Z 形折线
    const path = `M ${src.x} ${src.y} C ${midX} ${src.y}, ${midX} ${dst.y}, ${dst.x} ${dst.y}`
    return { path, active: highlightedTable.value === fk.table || highlightedTable.value === fk.ref_table }
  })
})

function highlightTable(name: string) {
  highlightedTable.value = highlightedTable.value === name ? null : name
}

function highlightEdge(i: number) {
  void i
  // 预留：点击边可以高亮两端表，目前不需要额外动作
}

// ========== 画布交互 ==========
function onWheel(e: WheelEvent) {
  const delta = e.deltaY > 0 ? -0.1 : 0.1
  scale.value = Math.max(0.2, Math.min(3, scale.value + delta))
}

function onPanStart(e: MouseEvent) {
  dragging = true
  lastX = e.clientX
  lastY = e.clientY
  canvasRef.value?.setAttribute('data-panning', 'true')
}

function onPanMove(e: MouseEvent) {
  if (!dragging) return
  tx.value += e.clientX - lastX
  ty.value += e.clientY - lastY
  lastX = e.clientX
  lastY = e.clientY
}

function onPanEnd() {
  dragging = false
  canvasRef.value?.removeAttribute('data-panning')
}

function resetView() {
  scale.value = 1
  tx.value = 40
  ty.value = 40
}
</script>

<style scoped>
.er-diagram {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg-primary);
}

.er-toolbar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 14px;
  border-bottom: 1px solid var(--border-default);
  background: var(--surface-secondary);
}

.btn-refresh, .btn-reset {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 5px 10px;
  border: 1px solid var(--border-default);
  border-radius: 6px;
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: 12px;
  cursor: pointer;
  transition: border-color 150ms;
}
.btn-refresh:hover, .btn-reset:hover { border-color: var(--brand-primary); }
.btn-refresh:disabled { opacity: 0.5; cursor: not-allowed; }

.spinning { animation: spin 1s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }

.er-stats {
  display: flex;
  gap: 12px;
  color: var(--text-tertiary);
  font-size: 12px;
}

.er-empty {
  color: var(--text-tertiary);
  font-size: 12px;
}

.er-driver {
  margin-left: auto;
  font-size: 11px;
  color: var(--brand-primary);
  padding: 2px 8px;
  border-radius: 4px;
  background: var(--brand-dim);
}

.er-canvas {
  flex: 1;
  overflow: auto;
  position: relative;
  cursor: grab;
}
.er-canvas[data-panning="true"] { cursor: grabbing; }

svg {
  min-width: 100%;
  min-height: 100%;
}

/* 连线 */
.er-links path {
  fill: none;
  stroke: var(--border-hover);
  stroke-width: 1.5;
}
.er-links path.active {
  stroke: var(--brand-primary);
  stroke-width: 2;
}

/* 表框 */
.er-table-bg {
  fill: var(--surface-secondary);
  stroke: var(--border-default);
  stroke-width: 1;
  transition: stroke 150ms, fill 150ms;
}
.er-table-bg.highlighted {
  stroke: var(--brand-primary);
  fill: var(--brand-dim);
}

.er-table-header {
  fill: var(--surface-hover);
  rx: 6;
}

.er-table-title {
  fill: var(--text-primary);
  font-weight: 600;
  font-size: 12px;
}

.er-col {
  fill: var(--text-secondary);
  font-size: 11px;
}
.er-col.pk { fill: var(--brand-primary); font-weight: 600; }
.er-pk-mark { font-size: 10px; }

.er-col-type {
  fill: var(--text-tertiary);
  font-size: 10px;
  text-anchor: end;
}

/* 节点组 hover */
g:hover .er-table-bg {
  stroke: var(--brand-primary);
}

/* 浮层 */
.er-overlay {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  background: rgba(0,0,0,0.2);
  color: var(--text-tertiary);
  font-size: 13px;
  backdrop-filter: blur(2px);
}
.er-overlay.error { color: #dc2626; }

/* 缩放指示 */
.er-zoom-indicator {
  position: absolute;
  bottom: 12px;
  right: 12px;
  font-size: 11px;
  color: var(--text-tertiary);
  background: var(--surface-secondary);
  border: 1px solid var(--border-default);
  padding: 2px 8px;
  border-radius: 4px;
}
</style>
