<template>
  <div class="er-diagram">
    <!-- 工具栏 -->
    <div class="er-toolbar">
      <div class="er-undo-wrap">
        <button
          @click="undoLastLink"
          class="btn-icon"
          :class="{ flash: undoFlash }"
          :disabled="!canUndo"
          :title="t('erDiagram.undo')"
        >
          <Undo2 :size="14" />
        </button>
        <Transition name="er-undo-pop">
          <span v-if="undoFlash" class="er-undo-badge">{{ t('erDiagram.undone') }}</span>
        </Transition>
      </div>
      <div class="er-stats" v-if="data">
        <span>{{ t('erDiagram.tables', { count: data.tables.length }) }}</span>
        <span>{{ t('erDiagram.fks', { count: data.foreign_keys.length }) }}</span>
        <span v-if="manualLinks.length" class="er-manual" :title="t('erDiagram.manualHint')">
          {{ t('erDiagram.manualLinks', { count: manualLinks.length }) }}
        </span>
      </div>
      <div class="er-empty" v-if="!data && !loading">
        {{ t('erDiagram.empty') }}
      </div>
      <div class="er-driver" v-if="data">{{ data.driver.toUpperCase() }}</div>
      <button @click="fitToView" class="btn-icon" :title="t('erDiagram.fitView')" :disabled="!data">
        <Scan :size="14" />
      </button>
      <button @click="resetView" class="btn-icon" :title="t('erDiagram.resetView')">
        <RotateCcw :size="14" />
      </button>
      <div class="er-export" @click.stop>
        <button
          class="btn-icon"
          :title="t('erDiagram.exportImage')"
          :disabled="!data || exporting"
          @click="exportMenuOpen = !exportMenuOpen"
        >
          <Loader2 v-if="exporting" :size="14" class="spinning" />
          <Download v-else :size="14" />
        </button>
        <div v-if="exportMenuOpen" class="er-export-menu">
          <button @click="exportImage('png')">PNG</button>
          <button @click="exportImage('svg')">SVG</button>
        </div>
      </div>
    </div>

    <!-- 首次使用提示 -->
    <Transition name="er-tip">
      <div v-if="showTip" class="er-tip" role="note">
        <div class="er-tip-header">
          <Lightbulb :size="14" />
          <span>{{ t('erDiagram.tipTitle') }}</span>
          <button class="er-tip-close" :title="t('common.close')" @click="dismissTip">
            <X :size="12" />
          </button>
        </div>
        <ul class="er-tip-list">
          <li>{{ t('erDiagram.tipZoom') }}</li>
          <li>{{ t('erDiagram.tipDrag') }}</li>
          <li>{{ t('erDiagram.tipHighlight') }}</li>
          <li>{{ t('erDiagram.tipConnect') }}</li>
          <li>{{ t('erDiagram.tipRemove') }}</li>
          <li>{{ t('erDiagram.tipTools') }}</li>
        </ul>
        <button class="er-tip-ok" @click="dismissTip">{{ t('erDiagram.tipGotIt') }}</button>
      </div>
    </Transition>

    <!-- SVG 画布（支持缩放+拖拽） -->
    <div
      ref="canvasRef"
      class="er-canvas"
      :class="{ connecting: !!connectSource }"
      @wheel.prevent="onWheel"
      @mousedown="onPanStart"
      @mousemove="onPanMove"
      @mouseup="onPanEnd"
      @mouseleave="onPanEnd"
    >
      <svg
        ref="svgRef"
        :width="svgW"
        :height="svgH"
        :style="{ transform: `translate(${tx}px, ${ty}px) scale(${scale})`, transformOrigin: '0 0' }"
        :viewBox="`${CANVAS_ORIGIN} ${CANVAS_ORIGIN} ${svgW} ${svgH}`"
        preserveAspectRatio="xMinYMin meet"
      >
        <!-- 外键箭头 -->
        <defs>
          <marker
            id="er-fk-arrow"
            viewBox="0 0 10 10"
            refX="9"
            refY="5"
            markerWidth="10"
            markerHeight="10"
            markerUnits="userSpaceOnUse"
            orient="auto"
          >
            <path d="M 0 0 L 10 5 L 0 10 z" class="er-arrow-head" />
          </marker>
          <!-- 手动关联箭头（蓝色） -->
          <marker
            id="er-manual-arrow"
            viewBox="0 0 10 10"
            refX="9"
            refY="5"
            markerWidth="10"
            markerHeight="10"
            markerUnits="userSpaceOnUse"
            orient="auto"
          >
            <path d="M 0 0 L 10 5 L 0 10 z" class="er-arrow-head manual" />
          </marker>
        </defs>
        <!-- 外键连线（先画，在表框下） -->
        <g class="er-links">
          <template v-for="edge in edgePaths" :key="edge.key">
            <!-- 手动关联的加宽透明热区，方便点选细虚线 -->
            <path
              v-if="edge.manual"
              class="er-link-hit er-screen-only"
              :d="edge.path"
              @click.stop="onEdgeClick(edge)"
            >
              <title>{{ edge.title }}</title>
            </path>
            <path
              :d="edge.path"
              :class="{ active: edge.active, manual: edge.manual, selected: edge.selected }"
              :marker-end="edge.manual ? 'url(#er-manual-arrow)' : 'url(#er-fk-arrow)'"
              @click.stop="onEdgeClick(edge)"
            >
              <title>{{ edge.title }}</title>
            </path>
          </template>
        </g>

        <!-- 表框 -->
        <g
          v-for="node in positionedTables"
          :key="node.table.name"
          class="er-table-group"
          :transform="`translate(${node.x}, ${node.y})`"
          :class="{
            highlighted: highlightedTable === node.table.name,
            dragging: dragName === node.table.name,
          }"
          @click.stop="onTableClick(node.table.name)"
          @mousedown.stop="onTableMouseDown($event, node)"
          @mouseenter="hoveredTable = node.table.name"
          @mouseleave="hoveredTable = null"
        >
          <title>{{ tableTooltip(node.table) }}</title>
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
          <!-- 拖拽建关联时的行高亮 -->
          <rect
            v-if="highlightTop(node.table.name, 'source') !== undefined"
            x="0"
            :y="highlightTop(node.table.name, 'source') ?? 0"
            :width="TABLE_W"
            :height="ROW_H"
            class="er-row-highlight source"
          />
          <rect
            v-if="highlightTop(node.table.name, 'target') !== undefined"
            x="0"
            :y="highlightTop(node.table.name, 'target') ?? 0"
            :width="TABLE_W"
            :height="ROW_H"
            class="er-row-highlight target"
          />
          <text x="12" y="21" class="er-table-title">{{ node.table.name }}</text>
          <!-- 列 -->
          <g v-for="(col, ci) in node.table.columns" :key="col.name">
            <text
              x="12"
              :y="colTextY(node.table, ci)"
              class="er-col"
              :class="{ pk: col.is_primary }"
            >
              <tspan v-if="col.is_primary" class="er-pk-mark">🔑 </tspan>{{ col.name }}
            </text>
            <text
              :x="TABLE_W - 12"
              :y="colTextY(node.table, ci)"
              class="er-col-type"
            >
              <tspan v-if="col.is_unique" class="er-col-flag">U </tspan>
              <tspan v-if="col.nullable === false" class="er-col-flag">NN </tspan>
              {{ truncate(col.column_type || col.data_type, 24) }}
            </text>
            <text
              v-if="colHasExtra(col)"
              x="12"
              :y="colTextY(node.table, ci) + 13"
              class="er-col-extra"
            >
              <tspan v-if="col.comment">{{ truncate(col.comment, 22) }}</tspan>
              <tspan
                v-if="hasDefault(col)"
                :x="TABLE_W - 12"
                text-anchor="end"
                class="er-col-default"
              >= {{ truncate(String(col.default_value), 14) }}</tspan>
            </text>
            <!-- 手动建关联的拖拽起点：悬停表时显示，拖到另一张表的目标字段上即可 -->
            <circle
              class="er-connect-dot"
              :cx="TABLE_W - 7"
              :cy="colRowTop(node.table, ci) + ROW_H / 2"
              r="4"
              @mousedown.stop.prevent="onConnectStart($event, node, col)"
              @click.stop
            >
              <title>{{ t('erDiagram.connectFrom') }}</title>
            </circle>
          </g>
        </g>

        <!-- 拖拽中的预览线与删除按钮：仅屏幕显示，不参与导出 -->
        <g class="er-screen-only">
          <path v-if="connectPreview" class="er-link-preview" :d="connectPreview" />
          <g
            v-if="selectedEdge && selectedEdge.manual"
            class="er-edge-del"
            :transform="`translate(${selectedEdge.mid.x}, ${selectedEdge.mid.y})`"
            @mousedown.stop
            @click.stop="removeManualLink(selectedEdge.key)"
          >
            <title>{{ t('erDiagram.removeLink') }}</title>
            <circle r="8" />
            <path d="M -3.5 -3.5 L 3.5 3.5 M 3.5 -3.5 L -3.5 3.5" />
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
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue'
import {
  Loader2, XCircle, RotateCcw, Lightbulb, X, Scan, Download, Undo2,
} from 'lucide-vue-next'
import { invoke } from '@tauri-apps/api/core'
import { useConnectionStore } from '@/stores/connection'
import { useLicenseStore } from '@/stores/license'
import { globalToast } from '@/composables/useToast'
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
interface ErColumn {
  name: string
  data_type: string
  column_type?: string
  is_primary: boolean
  nullable?: boolean
  default_value?: string | null
  comment?: string
  is_unique?: boolean
}
interface ErTableNode { name: string; columns: ErColumn[] }
interface ErForeignKey { name: string; table: string; column: string; ref_table: string; ref_column: string }
interface ErDiagramData {
  driver: 'mysql' | 'postgres'
  database: string
  tables: ErTableNode[]
  foreign_keys: ErForeignKey[]
}

interface PositionedTable { table: ErTableNode; x: number; y: number; h: number }
interface Curve {
  x1: number
  y1: number
  c1x: number
  c1y: number
  c2x: number
  c2y: number
  x2: number
  y2: number
}
interface EdgePath {
  key: string
  path: string
  mid: { x: number; y: number }
  active: boolean
  selected: boolean
  manual: boolean
  title: string
  table: string
  refTable: string
}
/** 用户手动拖拽建立、保存在本地的字段关联 */
interface ManualLink { table: string; column: string; ref_table: string; ref_column: string }
interface ManualPosition { x: number; y: number }

// ========== 常量 ==========
const TABLE_W = 220
const ROW_H = 20
const EXTRA_H = 14   // 列下方的注释/默认值行高度
const COL_GAP = 28   // 列与列之间的水平间距
const ROW_GAP = 24   // 同一列内相邻表之间的垂直间距
const MAX_COL_HEIGHT = 900  // 自动布局时单列的最大高度，超出换列形成平铺网格
// 画布坐标系固定原点：允许表被拖到负坐标（向左/向上），同时保持视觉位置连续
const CANVAS_ORIGIN = -2000

// ========== 状态 ==========
const data = ref<ErDiagramData | null>(null)
const loading = ref(false)
const error = ref('')
const canvasRef = ref<HTMLDivElement>()
const svgRef = ref<SVGSVGElement>()

// 首次使用提示（关闭后记住，不再自动弹出）
const TIP_STORAGE_KEY = 'er-diagram-tip-seen'
const showTip = ref(false)

function dismissTip() {
  showTip.value = false
  try {
    localStorage.setItem(TIP_STORAGE_KEY, '1')
  } catch {
    // localStorage 不可用时仅本次关闭
  }
}

// 画布变换
const scale = ref(1)
const tx = ref(40)
const ty = ref(40)
let dragging = false
let lastX = 0
let lastY = 0

// 单表拖动：按连接 + 数据库记住手动位置
const manualPos = ref<Record<string, ManualPosition>>({})
const dragName = ref<string | null>(null)
let dragStartClientX = 0
let dragStartClientY = 0
let dragStartNodeX = 0
let dragStartNodeY = 0
let dragMoved = false
let lastDragEndAt = 0

// 导出图片
const exportMenuOpen = ref(false)
const exporting = ref(false)

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
    loadLayout(resp.tables.map(item => item.name))
    loadLinks()
    emit('data-loaded', resp.tables.length)
    await nextTick()
    fitToView()
  } catch (e) {
    error.value = String(e)
    data.value = null
  } finally {
    loading.value = false
  }
}

onMounted(async () => {
  const license = useLicenseStore()
  if (!license.allows('er-diagram')) {
    license.openDialog()
    return
  }
  loadData()
  try {
    showTip.value = localStorage.getItem(TIP_STORAGE_KEY) !== '1'
  } catch {
    showTip.value = true
  }
  window.addEventListener('keydown', onKeydown)
  window.addEventListener('click', closeExportMenu)
})

onUnmounted(() => {
  window.removeEventListener('keydown', onKeydown)
  window.removeEventListener('click', closeExportMenu)
  window.removeEventListener('mousemove', onTableDragMove)
  window.removeEventListener('mouseup', onTableDragEnd)
  window.removeEventListener('mousemove', onConnectMove)
  window.removeEventListener('mouseup', onConnectEnd)
  window.clearTimeout(undoFlashTimer)
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

const hasDefault = (col: ErColumn) => col.default_value !== null && col.default_value !== undefined
const colHasExtra = (col: ErColumn) => !!col.comment || hasDefault(col)
const colRowHeight = (col: ErColumn) => ROW_H + (colHasExtra(col) ? EXTRA_H : 0)
const colRowTop = (table: ErTableNode, index: number) =>
  32 + table.columns.slice(0, index).reduce((acc, col) => acc + colRowHeight(col), 0)
const colTextY = (table: ErTableNode, index: number) => colRowTop(table, index) + 14
const tableHeight = (table: ErTableNode) =>
  32 + table.columns.reduce((acc, col) => acc + colRowHeight(col), 0) + 16

function truncate(text: string, max: number) {
  return text.length > max ? `${text.slice(0, max - 1)}…` : text
}

function tableTooltip(table: ErTableNode) {
  const lines = [`${table.name}（${table.columns.length} 列）`]
  for (const col of table.columns) {
    const flags: string[] = []
    if (col.is_primary) flags.push('PK')
    if (col.is_unique) flags.push('UNIQUE')
    if (col.nullable === false) flags.push('NOT NULL')
    if (hasDefault(col)) flags.push(`DEFAULT ${col.default_value === '' ? "''" : col.default_value}`)
    const head = `${col.name} ${col.column_type || col.data_type}${flags.length ? ` · ${flags.join(' · ')}` : ''}`
    lines.push(col.comment ? `${head}\n  ${col.comment}` : head)
  }
  return lines.join('\n')
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

  const tableMap = new Map(tables.map(t => [t.name, t]))

  // 平铺布局：同层的表依次往下堆叠，单列超过 MAX_COL_HEIGHT 时换到下一列，
  // 避免没有外键的库把所有表挤成一条很长的竖列
  const columns: string[][] = []
  for (const names of layers) {
    let current: string[] = []
    let height = 0
    for (const name of names) {
      const h = tableHeight(tableMap.get(name)!)
      if (current.length > 0 && height + ROW_GAP + h > MAX_COL_HEIGHT) {
        columns.push(current)
        current = []
        height = 0
      }
      height += (current.length > 0 ? ROW_GAP : 0) + h
      current.push(name)
    }
    if (current.length > 0) columns.push(current)
  }

  // 每列从顶部开始排列，列与列横向平铺
  const pos = new Map<string, { x: number; y: number; h: number }>()
  columns.forEach((names, ci) => {
    const x = ci * (TABLE_W + COL_GAP)
    let y = 0
    for (const name of names) {
      const t = tableMap.get(name)!
      const h = tableHeight(t)
      pos.set(name, { x, y, h })
      y += h + ROW_GAP
    }
  })

  return tables.map(t => {
    const auto = pos.get(t.name)!
    const manual = manualPos.value[t.name]
    return { table: t, x: manual?.x ?? auto.x, y: manual?.y ?? auto.y, h: auto.h }
  })
})

const svgW = ref(800)
const svgH = ref(600)

// 画布尺寸直接由已定位的表节点推导，避免重复计算
const canvasSize = computed(() => {
  const nodes = positionedTables.value
  if (nodes.length === 0) return { w: 800, h: 600 }
  const maxX = Math.max(...nodes.map(n => n.x + TABLE_W))
  const maxY = Math.max(...nodes.map(n => n.y + n.h))
  return {
    w: Math.max(600, maxX - CANVAS_ORIGIN + 40),
    h: Math.max(400, maxY - CANVAS_ORIGIN + 40),
  }
})

// 画布尺寸更新通过 watch 而不是在 positionedTables computed 里做副作用
watch(canvasSize, size => {
  svgW.value = size.w
  svgH.value = size.h
}, { immediate: true })

// ========== 外键连线 ==========
// 当前鼠标悬停的表名（用于高亮它的所有外键箭头）
const hoveredTable = ref<string | null>(null)

// 锚点 Y：优先对齐到具体字段所在行，找不到字段时退化为表头中心
function columnAnchorY(node: PositionedTable, colName?: string): number {
  let y = node.y + 20 // 表头中心
  if (colName) {
    const ci = node.table.columns.findIndex(c => c.name === colName)
    if (ci >= 0) y = node.y + colRowTop(node.table, ci) + ROW_H / 2
  }
  return y
}

// 三次贝塞尔：从 (x1,y1) 平滑连到 (x2,y2)
function bezier(x1: number, y1: number, x2: number, y2: number): string {
  const midX = (x1 + x2) / 2
  return `M ${x1} ${y1} C ${midX} ${y1}, ${midX} ${y2}, ${x2} ${y2}`
}

function curveToPath(c: Curve): string {
  return `M ${c.x1} ${c.y1} C ${c.c1x} ${c.c1y}, ${c.c2x} ${c.c2y}, ${c.x2} ${c.y2}`
}

// 三次贝塞尔曲线在 t=0.5 处的点，用于放置删除按钮
function curveMidpoint(c: Curve) {
  return {
    x: (c.x1 + 3 * c.c1x + 3 * c.c2x + c.x2) / 8,
    y: (c.y1 + 3 * c.c1y + 3 * c.c2y + c.y2) / 8,
  }
}

// ========== 手动关联（用户拖拽建立，按连接 + 数据库保存在本地） ==========
const manualLinks = ref<ManualLink[]>([])
const selectedEdgeKey = ref<string | null>(null)

/** 手动关联的增删操作历史，用于撤销（Ctrl/Cmd+Z） */
interface LinkOp { type: 'add' | 'remove'; link: ManualLink }
const undoStack = ref<LinkOp[]>([])
const canUndo = computed(() => undoStack.value.length > 0)

// 撤销成功的视觉反馈：按钮高亮 + 图标回转 + 短暂弹出「已撤销」
const undoFlash = ref(false)
let undoFlashTimer: number | undefined

function flashUndo() {
  window.clearTimeout(undoFlashTimer)
  undoFlash.value = false
  // 下一帧再加类，保证连续撤销时动画能重新播放
  window.requestAnimationFrame(() => { undoFlash.value = true })
  undoFlashTimer = window.setTimeout(() => { undoFlash.value = false }, 1000)
}

function pushUndo(op: LinkOp) {
  undoStack.value = [...undoStack.value, op].slice(-50)
}

function linkKey(link: { table: string; column: string; ref_table: string; ref_column: string }) {
  return `${link.table}.${link.column}->${link.ref_table}.${link.ref_column}`
}

// 拖拽建关联时的起点/终点/光标位置（坐标与表位置同为画布坐标系）
const connectSource = ref<{ table: string; column: string } | null>(null)
const connectTarget = ref<{ table: string; column: string } | null>(null)
const connectCursor = ref<{ x: number; y: number } | null>(null)

const linkStorageKey = computed(() => {
  const connStore = useConnectionStore()
  return `er-diagram-links:v1:${connStore.currentConnection?.id ?? 'none'}:${props.database}`
})

function loadLinks() {
  manualLinks.value = []
  selectedEdgeKey.value = null
  undoStack.value = []
  const tables = data.value?.tables
  if (!tables) return
  try {
    const raw = localStorage.getItem(linkStorageKey.value)
    if (!raw) return
    const parsed = JSON.parse(raw) as ManualLink[]
    if (!Array.isArray(parsed)) return
    // 表结构变化后丢弃已不存在的字段关联
    const columnsOf = new Map(tables.map(t => [t.name, new Set(t.columns.map(c => c.name))]))
    manualLinks.value = parsed.filter(link =>
      !!link &&
      columnsOf.get(link.table)?.has(link.column) &&
      columnsOf.get(link.ref_table)?.has(link.ref_column),
    )
  } catch {
    manualLinks.value = []
  }
}

function saveLinks() {
  try {
    if (manualLinks.value.length > 0) {
      localStorage.setItem(linkStorageKey.value, JSON.stringify(manualLinks.value))
    } else {
      localStorage.removeItem(linkStorageKey.value)
    }
  } catch {
    // localStorage 不可用时仅本次生效
  }
}

function addManualLink(link: ManualLink) {
  const key = linkKey(link)
  const duplicated = manualLinks.value.some(item => linkKey(item) === key) ||
    (data.value?.foreign_keys ?? []).some(fk => linkKey(fk) === key)
  if (duplicated) {
    globalToast.info(t('erDiagram.linkExists'))
    return
  }
  manualLinks.value = [...manualLinks.value, link]
  pushUndo({ type: 'add', link })
  saveLinks()
  globalToast.success(t('erDiagram.linkAdded'))
}

function removeManualLink(key: string) {
  const removed = manualLinks.value.find(link => linkKey(link) === key)
  if (!removed) return
  manualLinks.value = manualLinks.value.filter(link => linkKey(link) !== key)
  pushUndo({ type: 'remove', link: removed })
  if (selectedEdgeKey.value === key) selectedEdgeKey.value = null
  saveLinks()
}

// 撤销上一步手动关联操作（新增 → 删除，删除 → 恢复）
function undoLastLink() {
  const op = undoStack.value[undoStack.value.length - 1]
  if (!op) return
  undoStack.value = undoStack.value.slice(0, -1)
  const key = linkKey(op.link)
  if (op.type === 'add') {
    manualLinks.value = manualLinks.value.filter(link => linkKey(link) !== key)
    if (selectedEdgeKey.value === key) selectedEdgeKey.value = null
  } else {
    // 字段已不存在（表结构变化）时不恢复，避免画出无效连线
    const tables = data.value?.tables ?? []
    const src = tables.find(t => t.name === op.link.table)
    const dst = tables.find(t => t.name === op.link.ref_table)
    const valid = !!src?.columns.some(c => c.name === op.link.column) &&
      !!dst?.columns.some(c => c.name === op.link.ref_column)
    if (!valid) {
      globalToast.info(t('erDiagram.undoInvalid'))
      return
    }
    manualLinks.value = [...manualLinks.value, op.link]
  }
  saveLinks()
  flashUndo()
  globalToast.success(t('erDiagram.undoDone'))
}

// ========== 拖拽建关联 ==========
function svgPoint(e: MouseEvent): { x: number; y: number } {
  const svg = svgRef.value
  if (!svg) return { x: 0, y: 0 }
  const rect = svg.getBoundingClientRect()
  const k = rect.width > 0 ? rect.width / svgW.value : 1
  return {
    x: (e.clientX - rect.left) / k + CANVAS_ORIGIN,
    y: (e.clientY - rect.top) / k + CANVAS_ORIGIN,
  }
}

// 光标落在哪张表的哪一行字段上
function findTargetAt(point: { x: number; y: number }): { table: string; column: string } | null {
  for (const node of positionedTables.value) {
    if (point.x < node.x || point.x > node.x + TABLE_W) continue
    if (point.y < node.y || point.y > node.y + node.h) continue
    const localY = point.y - node.y
    if (localY < 32) continue
    for (let i = 0; i < node.table.columns.length; i++) {
      const top = colRowTop(node.table, i)
      if (localY >= top && localY < top + colRowHeight(node.table.columns[i]!)) {
        const col = node.table.columns[i]!
        const source = connectSource.value
        if (source && source.table === node.table.name && source.column === col.name) return null
        return { table: node.table.name, column: col.name }
      }
    }
  }
  return null
}

function onConnectStart(e: MouseEvent, node: PositionedTable, col: ErColumn) {
  if (e.button !== 0) return
  connectSource.value = { table: node.table.name, column: col.name }
  connectTarget.value = null
  connectCursor.value = svgPoint(e)
  window.addEventListener('mousemove', onConnectMove)
  window.addEventListener('mouseup', onConnectEnd)
}

function onConnectMove(e: MouseEvent) {
  if (!connectSource.value) return
  connectCursor.value = svgPoint(e)
  connectTarget.value = findTargetAt(connectCursor.value)
}

function onConnectEnd() {
  window.removeEventListener('mousemove', onConnectMove)
  window.removeEventListener('mouseup', onConnectEnd)
  const source = connectSource.value
  const target = connectTarget.value
  connectSource.value = null
  connectTarget.value = null
  connectCursor.value = null
  if (!source || !target) return
  addManualLink({
    table: source.table,
    column: source.column,
    ref_table: target.table,
    ref_column: target.column,
  })
}

// 拖拽过程中的预览线：从起点圆点画到光标
const connectPreview = computed(() => {
  const source = connectSource.value
  const cursor = connectCursor.value
  if (!source || !cursor) return ''
  const node = positionedTables.value.find(n => n.table.name === source.table)
  if (!node) return ''
  const y1 = columnAnchorY(node, source.column)
  const x1 = cursor.x < node.x ? node.x : node.x + TABLE_W
  return bezier(x1, y1, cursor.x, cursor.y)
})

// 拖拽过程中高亮起点行与目标行
function highlightTop(tableName: string, kind: 'source' | 'target'): number | undefined {
  const point = kind === 'source' ? connectSource.value : connectTarget.value
  if (!point || point.table !== tableName) return undefined
  const table = data.value?.tables.find(t => t.name === tableName)
  if (!table) return undefined
  const ci = table.columns.findIndex(c => c.name === point.column)
  return ci >= 0 ? colRowTop(table, ci) : undefined
}

const selectedEdge = computed(() => edgePaths.value.find(e => e.selected) ?? null)

const edgePaths = computed<EdgePath[]>(() => {
  const nodes = new Map(positionedTables.value.map(n => [n.table.name, n]))
  const result: EdgePath[] = []

  const push = (link: ManualLink, manual: boolean) => {
    const srcNode = nodes.get(link.table)
    const dstNode = nodes.get(link.ref_table)
    // 引用到不在当前库/已被过滤的表时跳过，避免画出指向 (0,0) 的无效连线
    if (!srcNode || !dstNode) return

    const srcY = columnAnchorY(srcNode, link.column)
    const dstY = columnAnchorY(dstNode, link.ref_column)
    const srcRight = srcNode.x + TABLE_W
    const dstRight = dstNode.x + TABLE_W

    let curve: Curve
    if (dstNode.x >= srcRight) {
      // 目标在右侧：源表右边缘 → 目标表左边缘（箭头指向目标字段）
      const midX = (srcRight + dstNode.x) / 2
      curve = { x1: srcRight, y1: srcY, c1x: midX, c1y: srcY, c2x: midX, c2y: dstY, x2: dstNode.x, y2: dstY }
    } else if (dstRight <= srcNode.x) {
      // 目标在左侧：源表左边缘 → 目标表右边缘
      const midX = (srcNode.x + dstRight) / 2
      curve = { x1: srcNode.x, y1: srcY, c1x: midX, c1y: srcY, c2x: midX, c2y: dstY, x2: dstRight, y2: dstY }
    } else {
      // 同一列或水平重叠：从右边缘绕行，避免连线被表体遮挡
      const off = Math.max(srcRight, dstRight) + 48
      curve = { x1: srcRight, y1: srcY, c1x: off, c1y: srcY, c2x: off, c2y: dstY, x2: dstRight, y2: dstY }
    }

    const key = linkKey(link)
    result.push({
      key,
      path: curveToPath(curve),
      mid: curveMidpoint(curve),
      title: `${link.table}.${link.column} → ${link.ref_table}.${link.ref_column}${
        manual ? t('erDiagram.manualMark') : ''
      }`,
      table: link.table,
      refTable: link.ref_table,
      manual,
      selected: selectedEdgeKey.value === key,
      active:
        highlightedTable.value === link.table ||
        highlightedTable.value === link.ref_table ||
        hoveredTable.value === link.table ||
        hoveredTable.value === link.ref_table,
    })
  }

  for (const fk of data.value?.foreign_keys ?? []) push(fk, false)
  for (const link of manualLinks.value) push(link, true)
  return result
})

function highlightTable(name: string) {
  highlightedTable.value = highlightedTable.value === name ? null : name
}

function onEdgeClick(edge: EdgePath) {
  // 手动关联可点选，选中后按 Delete 或点线中间的 × 删除
  selectedEdgeKey.value = edge.manual
    ? (selectedEdgeKey.value === edge.key ? null : edge.key)
    : null
  highlightedTable.value = highlightedTable.value === edge.table ? null : edge.table
}

// ========== 手动布局持久化 ==========
const layoutStorageKey = computed(() => {
  const connStore = useConnectionStore()
  // v2：改为固定原点坐标 + 平铺自动布局，旧位置不再适用
  return `er-diagram-layout:v2:${connStore.currentConnection?.id ?? 'none'}:${props.database}`
})

function loadLayout(tableNames: string[]) {
  manualPos.value = {}
  try {
    const raw = localStorage.getItem(layoutStorageKey.value)
    if (!raw) return
    const parsed = JSON.parse(raw) as Record<string, ManualPosition>
    const valid = new Set(tableNames)
    const next: Record<string, ManualPosition> = {}
    for (const [name, pos] of Object.entries(parsed)) {
      if (valid.has(name) && typeof pos?.x === 'number' && typeof pos?.y === 'number') {
        next[name] = pos
      }
    }
    manualPos.value = next
  } catch {
    manualPos.value = {}
  }
}

function saveLayout() {
  try {
    if (Object.keys(manualPos.value).length > 0) {
      localStorage.setItem(layoutStorageKey.value, JSON.stringify(manualPos.value))
    } else {
      localStorage.removeItem(layoutStorageKey.value)
    }
  } catch {
    // localStorage 不可用时仅本次生效
  }
}

// ========== 单表拖动 ==========
function onTableMouseDown(e: MouseEvent, node: PositionedTable) {
  if (e.button !== 0) return
  dragName.value = node.table.name
  dragMoved = false
  dragStartClientX = e.clientX
  dragStartClientY = e.clientY
  dragStartNodeX = node.x
  dragStartNodeY = node.y
  window.addEventListener('mousemove', onTableDragMove)
  window.addEventListener('mouseup', onTableDragEnd)
  e.preventDefault()
}

function onTableDragMove(e: MouseEvent) {
  if (!dragName.value) return
  const dxClient = e.clientX - dragStartClientX
  const dyClient = e.clientY - dragStartClientY
  // 小于阈值视为点击，不触发拖动
  if (!dragMoved && Math.abs(dxClient) + Math.abs(dyClient) < 4) return
  dragMoved = true
  const nextX = Math.round(dragStartNodeX + dxClient / scale.value)
  const nextY = Math.round(dragStartNodeY + dyClient / scale.value)
  // 允许拖到负坐标（向左/向上），仅限制在画布可绘制范围内
  manualPos.value = {
    ...manualPos.value,
    [dragName.value]: {
      x: Math.max(CANVAS_ORIGIN + 20, nextX),
      y: Math.max(CANVAS_ORIGIN + 20, nextY),
    },
  }
}

function onTableDragEnd() {
  window.removeEventListener('mousemove', onTableDragMove)
  window.removeEventListener('mouseup', onTableDragEnd)
  if (dragName.value && dragMoved) {
    saveLayout()
    lastDragEndAt = Date.now()
  }
  dragName.value = null
}

function onTableClick(name: string) {
  // 拖动结束后紧接着的 click 不应切换高亮
  if (Date.now() - lastDragEndAt < 200) return
  highlightTable(name)
}

// ========== 适配窗口 ==========
const contentBounds = computed(() => {
  const nodes = positionedTables.value
  if (nodes.length === 0) return null
  const minX = Math.min(...nodes.map(n => n.x))
  const minY = Math.min(...nodes.map(n => n.y))
  const maxX = Math.max(...nodes.map(n => n.x + TABLE_W))
  const maxY = Math.max(...nodes.map(n => n.y + n.h))
  return {
    // 原始坐标（用于导出时的 viewBox 裁剪）
    rawX: minX,
    rawY: minY,
    // SVG 元素坐标（viewBox 起点固定为 CANVAS_ORIGIN）
    x: minX - CANVAS_ORIGIN,
    y: minY - CANVAS_ORIGIN,
    w: maxX - minX,
    h: maxY - minY,
  }
})

function fitToView() {
  const canvas = canvasRef.value
  const bounds = contentBounds.value
  if (!canvas || !bounds || bounds.w === 0 || bounds.h === 0) return
  const pad = 32
  const availW = Math.max(80, canvas.clientWidth - pad * 2)
  const availH = Math.max(80, canvas.clientHeight - pad * 2)
  const next = Math.min(1, availW / bounds.w, availH / bounds.h)
  scale.value = Math.max(0.2, Math.min(3, next))
  tx.value = pad + (availW - bounds.w * scale.value) / 2 - bounds.x * scale.value
  ty.value = pad + (availH - bounds.h * scale.value) / 2 - bounds.y * scale.value
  canvas.scrollTop = 0
  canvas.scrollLeft = 0
}

// ========== 导出图片 ==========
function closeExportMenu() {
  exportMenuOpen.value = false
}

function cssVar(name: string, fallback: string) {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim()
  return value || fallback
}

function buildExportSvg(): { svg: string; width: number; height: number } {
  const svg = svgRef.value
  if (!svg) throw new Error('画布尚未就绪')
  // 按内容裁剪，避免把固定原点带来的大片空白一起导出
  const bounds = contentBounds.value
  const pad = 20
  const width = Math.max(1, Math.ceil((bounds?.w ?? svgW.value) + pad * 2))
  const height = Math.max(1, Math.ceil((bounds?.h ?? svgH.value) + pad * 2))
  const vbX = bounds ? bounds.rawX - pad : CANVAS_ORIGIN
  const vbY = bounds ? bounds.rawY - pad : CANVAS_ORIGIN

  const clone = svg.cloneNode(true) as SVGSVGElement
  // 预览线、删除按钮、拖拽圆点等仅用于屏幕交互，不导出
  clone.querySelectorAll('.er-screen-only, .er-connect-dot, .er-row-highlight').forEach(el => el.remove())
  clone.removeAttribute('style')
  clone.setAttribute('xmlns', 'http://www.w3.org/2000/svg')
  clone.setAttribute('width', String(width))
  clone.setAttribute('height', String(height))
  clone.setAttribute('viewBox', `${vbX} ${vbY} ${width} ${height}`)

  const bg = cssVar('--bg-primary', '#ffffff')
  const tableBg = cssVar('--surface-subtle', '#f7f7f7')
  const border = cssVar('--border-default', '#d4d4d4')
  const headerBg = cssVar('--surface-hover', '#f0f0f0')
  const selectedBg = cssVar('--surface-selected', 'rgba(33,150,243,0.12)')
  const textPrimary = cssVar('--text-primary', '#1a1a1a')
  const textSecondary = cssVar('--text-secondary', '#444444')
  const textTertiary = cssVar('--text-tertiary', '#888888')
  const brand = cssVar('--brand-500', '#2196f3')
  const linkColor = cssVar('--border-strong', '#999999')
  const fontFamily = "-apple-system, BlinkMacSystemFont, 'Segoe UI', 'PingFang SC', 'Microsoft YaHei', sans-serif"

  // scoped 样式不会跟随克隆的 SVG，导出时内联一份等效样式
  const style = document.createElementNS('http://www.w3.org/2000/svg', 'style')
  style.textContent = `
    .er-table-bg { fill: ${tableBg}; stroke: ${border}; stroke-width: 1; }
    .er-table-bg.highlighted { fill: ${selectedBg}; stroke: ${brand}; }
    .er-table-header { fill: ${headerBg}; }
    .er-table-title { fill: ${textPrimary}; font-size: 12px; font-weight: 600; font-family: ${fontFamily}; }
    .er-col { fill: ${textSecondary}; font-size: 11px; font-family: ${fontFamily}; }
    .er-col.pk { fill: ${brand}; font-weight: 600; }
    .er-pk-mark { font-size: 10px; }
    .er-col-type { fill: ${textTertiary}; font-size: 10px; text-anchor: end; font-family: ${fontFamily}; }
    .er-col-flag { fill: ${brand}; font-size: 9px; }
    .er-col-extra { fill: ${textTertiary}; font-size: 9px; font-family: ${fontFamily}; }
    .er-col-default { fill: ${textTertiary}; font-size: 9px; }
    .er-links path { fill: none; stroke: ${linkColor}; stroke-width: 1.5; }
    .er-links path.active { stroke: ${brand}; stroke-width: 2; }
    .er-links path.manual { stroke: ${brand}; stroke-dasharray: 5 4; }
    .er-links path.manual.selected { stroke-width: 2.5; }
    .er-arrow-head { fill: ${linkColor}; }
    .er-arrow-head.manual { fill: ${brand}; }
  `
  clone.insertBefore(style, clone.firstChild)

  const background = document.createElementNS('http://www.w3.org/2000/svg', 'rect')
  background.setAttribute('x', String(vbX))
  background.setAttribute('y', String(vbY))
  background.setAttribute('width', String(width))
  background.setAttribute('height', String(height))
  background.setAttribute('fill', bg)
  clone.insertBefore(background, style.nextSibling)

  return { svg: new XMLSerializer().serializeToString(clone), width, height }
}

function base64FromBytes(bytes: Uint8Array) {
  let binary = ''
  const chunk = 0x8000
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk))
  }
  return btoa(binary)
}

async function renderPngBase64(svgString: string, width: number, height: number): Promise<string> {
  const blob = new Blob([svgString], { type: 'image/svg+xml;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  try {
    const img = new Image()
    await new Promise<void>((resolve, reject) => {
      img.onload = () => resolve()
      img.onerror = () => reject(new Error('SVG 渲染失败'))
      img.src = url
    })
    const ratio = 2
    const canvas = document.createElement('canvas')
    canvas.width = Math.ceil(width * ratio)
    canvas.height = Math.ceil(height * ratio)
    const ctx = canvas.getContext('2d')
    if (!ctx) throw new Error('无法创建画布')
    ctx.drawImage(img, 0, 0, canvas.width, canvas.height)
    const dataUrl = canvas.toDataURL('image/png')
    return dataUrl.slice(dataUrl.indexOf(',') + 1)
  } finally {
    URL.revokeObjectURL(url)
  }
}

async function exportImage(format: 'png' | 'svg') {
  exportMenuOpen.value = false
  if (!data.value || exporting.value) return
  exporting.value = true
  try {
    const built = buildExportSvg()
    const baseName = `${data.value.database || 'er'}-er-diagram`
    const base64 = format === 'svg'
      ? base64FromBytes(new TextEncoder().encode(built.svg))
      : await renderPngBase64(built.svg, built.width, built.height)
    const path = await invoke<string>('save_binary_file_to_dialog', {
      contentBase64: base64,
      defaultFilename: `${baseName}.${format}`,
      filterName: format === 'svg' ? 'SVG 图片' : 'PNG 图片',
      extensions: [format],
    })
    globalToast.success(t('erDiagram.exportSuccess', { path }))
  } catch (e) {
    const message = String(e)
    if (message.includes('取消')) return
    globalToast.error(t('erDiagram.exportFailed', { error: message }))
  } finally {
    exporting.value = false
  }
}

// ========== 画布交互 ==========
function onWheel(e: WheelEvent) {
  const delta = e.deltaY > 0 ? -0.1 : 0.1
  scale.value = Math.max(0.2, Math.min(3, scale.value + delta))
}

function onPanStart(e: MouseEvent) {
  // 点空白处取消手动关联的选中状态（点连线本身时不取消，交给 click 处理）
  const target = e.target as Element | null
  if (target?.tagName.toLowerCase() !== 'path') selectedEdgeKey.value = null
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
  // 清除手动拖动的表位置，恢复自动布局
  manualPos.value = {}
  saveLayout()
  scale.value = 1
  const bounds = contentBounds.value
  tx.value = bounds ? 40 - bounds.x : 40
  ty.value = bounds ? 40 - bounds.y : 40
}

// ========== 快捷键 ==========
function onKeydown(e: KeyboardEvent) {
  const target = e.target as HTMLElement | null
  const editing = !!target && (
    target.tagName === 'INPUT' ||
    target.tagName === 'TEXTAREA' ||
    target.isContentEditable
  )
  // 选中手动关联后按 Delete/Backspace 删除
  if (!editing && (e.key === 'Delete' || e.key === 'Backspace') && selectedEdgeKey.value) {
    e.preventDefault()
    removeManualLink(selectedEdgeKey.value)
    return
  }
  // Ctrl/Cmd+Z 撤销上一步手动关联操作
  if (!editing && (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z' && canUndo.value) {
    e.preventDefault()
    undoLastLink()
  }
}
</script>

<style scoped>
.er-diagram {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  position: relative;
  background: var(--bg-primary);
}

.er-toolbar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 14px;
  border-bottom: 1px solid var(--border-default);
  background: var(--surface-subtle);
}

.btn-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 5px;
  padding: 5px 8px;
  border: 1px solid var(--border-default);
  border-radius: 6px;
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: 12px;
  line-height: 1;
  white-space: nowrap;
  flex-shrink: 0;
  cursor: pointer;
  transition: border-color 150ms;
}
.btn-icon:hover { border-color: var(--brand-500); }
.btn-icon:disabled { opacity: 0.5; cursor: not-allowed; }

/* 撤销按钮 + 「已撤销」浮动提示 */
.er-undo-wrap {
  position: relative;
  flex-shrink: 0;
}
.er-undo-badge {
  position: absolute;
  top: calc(100% + 6px);
  left: 50%;
  transform: translateX(-50%);
  padding: 2px 8px;
  border-radius: 10px;
  background: var(--brand-500);
  color: #fff;
  font-size: 11px;
  line-height: 16px;
  white-space: nowrap;
  pointer-events: none;
  z-index: 8;
}
.er-undo-pop-enter-active, .er-undo-pop-leave-active {
  transition: opacity 200ms ease, transform 200ms ease;
}
.er-undo-pop-enter-from, .er-undo-pop-leave-to {
  opacity: 0;
  transform: translate(-50%, -4px);
}

/* 撤销成功时按钮高亮 + 图标回转一圈 */
.btn-icon.flash {
  border-color: var(--brand-500);
  color: var(--brand-500);
  animation: er-undo-ring 700ms ease;
}
/* 撤销后可能已无可撤销操作而进入禁用态，高亮反馈仍要完整可见 */
.btn-icon.flash:disabled { opacity: 1; }
.btn-icon.flash svg { animation: er-undo-spin 700ms ease; }
@keyframes er-undo-ring {
  0% { box-shadow: 0 0 0 0 var(--surface-selected); }
  50% { box-shadow: 0 0 0 5px var(--surface-selected); }
  100% { box-shadow: 0 0 0 0 var(--surface-selected); }
}
@keyframes er-undo-spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(-360deg); }
}

/* 导出图片下拉 */
.er-export {
  position: relative;
  flex-shrink: 0;
}
.er-export-menu {
  position: absolute;
  top: calc(100% + 4px);
  right: 0;
  z-index: 6;
  min-width: 88px;
  padding: 4px;
  display: flex;
  flex-direction: column;
  gap: 2px;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  background: var(--surface-default);
  box-shadow: var(--shadow-md);
}
.er-export-menu button {
  padding: 5px 10px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-primary);
  font-size: 12px;
  text-align: left;
  cursor: pointer;
}
.er-export-menu button:hover { background: var(--surface-hover); }

.spinning { animation: spin 1s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }

/* 首次使用提示 */
.er-tip {
  position: absolute;
  top: 56px;
  left: 16px;
  z-index: 5;
  width: 300px;
  max-width: calc(100% - 32px);
  padding: var(--space-3) var(--space-4);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  background: var(--surface-default);
  box-shadow: var(--shadow-md);
  color: var(--text-secondary);
  font-size: 12px;
  line-height: 1.6;
}

.er-tip-header {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: var(--space-2);
  color: var(--text-primary);
  font-weight: 600;
}

.er-tip-close {
  margin-left: auto;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 2px;
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  border-radius: var(--radius-sm);
}
.er-tip-close:hover { color: var(--text-primary); background: var(--surface-hover); }

.er-tip-list {
  margin: 0 0 var(--space-3);
  padding-left: 16px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.er-tip-ok {
  padding: 4px 12px;
  border: none;
  border-radius: var(--radius-md);
  background: var(--brand-500);
  color: #fff;
  font-size: 12px;
  cursor: pointer;
}
.er-tip-ok:hover { background: var(--brand-600); }

.er-tip-enter-active, .er-tip-leave-active {
  transition: opacity 150ms ease, transform 150ms ease;
}
.er-tip-enter-from, .er-tip-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}

.er-stats {
  display: flex;
  gap: 12px;
  color: var(--text-tertiary);
  font-size: 12px;
  white-space: nowrap;
  flex-shrink: 0;
}

.er-manual {
  color: var(--brand-500);
  cursor: help;
  flex-shrink: 0;
}

.er-empty {
  color: var(--text-tertiary);
  font-size: 12px;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.er-driver {
  margin-left: auto;
  font-size: 11px;
  color: var(--brand-500);
  padding: 2px 8px;
  border-radius: 4px;
  background: var(--surface-selected);
  flex-shrink: 0;
}

.er-canvas {
  flex: 1;
  /* SVG 有固有宽高，必须允许收缩，否则会把画布撑高导致自动适配算错可视高度 */
  min-height: 0;
  min-width: 0;
  /* 平移完全由拖拽控制；用 hidden 避免缩放后仍出现与实际内容不匹配的滚动条 */
  overflow: hidden;
  position: relative;
  cursor: grab;
}
.er-canvas[data-panning="true"] { cursor: grabbing; }

/* 固定为画布实际尺寸，避免 viewBox 被拉伸后表被放大 */
svg {
  display: block;
  flex: none;
}

/* 连线 */
.er-links path {
  fill: none;
  stroke: var(--border-strong);
  stroke-width: 1.5;
  pointer-events: stroke;
  cursor: pointer;
}
.er-links path.active {
  stroke: var(--brand-500);
  stroke-width: 2;
}

/* 手动关联的加宽透明热区：细虚线也能轻松点中，自身不显示线条 */
.er-links path.er-link-hit {
  stroke: transparent;
  stroke-width: 12;
  pointer-events: stroke;
  cursor: pointer;
}

/* 用户手动拖拽建立的关联用蓝色虚线区分 */
.er-links path.manual {
  stroke: var(--brand-500);
  stroke-dasharray: 5 4;
  opacity: 0.9;
}
.er-links path.manual.selected {
  stroke-width: 2.5;
  opacity: 1;
}

/* 外键连线终点箭头 */
.er-arrow-head {
  fill: var(--border-strong);
}
.er-arrow-head.manual {
  fill: var(--brand-500);
}

/* 表框 */
.er-table-bg {
  fill: var(--surface-subtle);
  stroke: var(--border-default);
  stroke-width: 1;
  transition: stroke 150ms, fill 150ms;
}
.er-table-bg.highlighted {
  stroke: var(--brand-500);
  fill: var(--surface-selected);
}

.er-table-header {
  fill: var(--surface-hover);
  rx: 6;
  cursor: move;
}

.er-table-title {
  fill: var(--text-primary);
  font-weight: 600;
  font-size: 12px;
  pointer-events: none;
}

.er-col {
  fill: var(--text-secondary);
  font-size: 11px;
}
.er-col.pk { fill: var(--brand-500); font-weight: 600; }
.er-pk-mark { font-size: 10px; }

.er-col-type {
  fill: var(--text-tertiary);
  font-size: 10px;
  text-anchor: end;
}
.er-col-flag { fill: var(--brand-500); font-size: 9px; }

/* 列下方的注释 / 默认值 */
.er-col-extra {
  fill: var(--text-tertiary);
  font-size: 9px;
}
.er-col-default { fill: var(--text-tertiary); font-size: 9px; }

/* 拖拽建关联：字段右侧的起点圆点，悬停表时才显示 */
.er-connect-dot {
  fill: var(--brand-500);
  opacity: 0;
  cursor: crosshair;
  transition: opacity 120ms;
}
.er-table-group:hover .er-connect-dot { opacity: 0.4; }
.er-connect-dot:hover { opacity: 1; }

/* 拖拽过程中的起点行 / 目标行高亮 */
.er-row-highlight {
  fill: var(--brand-500);
  opacity: 0.12;
  pointer-events: none;
}
.er-row-highlight.target { opacity: 0.2; }

/* 拖拽过程中的预览线 */
.er-link-preview {
  fill: none;
  stroke: var(--brand-500);
  stroke-width: 1.5;
  stroke-dasharray: 4 3;
  pointer-events: none;
}

/* 选中手动关联后的删除按钮 */
.er-edge-del { cursor: pointer; }
.er-edge-del circle {
  fill: var(--surface-default);
  stroke: var(--brand-500);
  stroke-width: 1;
}
.er-edge-del path {
  stroke: var(--brand-500);
  stroke-width: 1.5;
  stroke-linecap: round;
}
.er-edge-del:hover circle { fill: var(--brand-500); }
.er-edge-del:hover path { stroke: #fff; }

.er-canvas.connecting,
.er-canvas.connecting[data-panning="true"] { cursor: crosshair; }

/* 拖动中的表 */
g.dragging .er-table-bg {
  stroke: var(--brand-500);
  filter: drop-shadow(0 4px 8px rgba(0, 0, 0, 0.18));
}

/* 节点组 hover */
g:hover .er-table-bg {
  stroke: var(--brand-500);
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
  background: var(--surface-subtle);
  border: 1px solid var(--border-default);
  padding: 2px 8px;
  border-radius: 4px;
}
</style>
