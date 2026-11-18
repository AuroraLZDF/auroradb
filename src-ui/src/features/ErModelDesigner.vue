<template>
  <div class="er-model-designer">
    <!-- 工具栏 -->
    <div class="erm-toolbar">
      <Select
        :model-value="modelStore.currentId"
        :options="modelOptions"
        size="sm"
        :placeholder="t('erModel.select')"
        @update:model-value="onSelectModel($event as string)"
      />
      <input
        v-if="modelStore.current"
        class="erm-name-input"
        :value="modelStore.current.name"
        :title="t('erModel.renamePrompt')"
        @change="modelStore.rename(modelStore.current!.id, ($event.target as HTMLInputElement).value)"
      />
      <button class="erm-btn" :title="t('erModel.newPrompt')" @click="createModel">
        <Plus :size="14" /> {{ t('erModel.new') }}
      </button>
      <button class="erm-btn danger" :disabled="!modelStore.current" @click="removeModel">
        <Trash2 :size="14" /> {{ removeModelArmed ? t('common.confirm') : t('erModel.deleteModel') }}
      </button>
      <span class="erm-divider"></span>
      <button class="erm-btn primary" :disabled="!modelStore.current" @click="addTable">
        <Plus :size="14" /> {{ t('erModel.addTable') }}
      </button>
      <span class="erm-divider"></span>
      <button class="erm-btn" :disabled="!modelStore.current" @click="importModel">
        <Upload :size="14" /> {{ t('erModel.import') }}
      </button>
      <button class="erm-btn" :disabled="!modelStore.current" @click="exportModel">
        <Download :size="14" /> {{ t('erModel.export') }}
      </button>
      <span class="erm-divider"></span>
      <button class="erm-btn primary" :disabled="!modelStore.current || modelStore.current.tables.length === 0" @click="openDdlDialog">
        <FileCode :size="14" /> {{ t('erModel.generateDdl') }}
      </button>
      <button class="erm-btn" :disabled="!modelStore.current || modelStore.current.tables.length === 0" @click="openSyncDialog">
        <GitCompare :size="14" /> {{ t('erModel.syncWithDb') }}
      </button>
      <span class="erm-spacer"></span>
      <div class="erm-stats" v-if="modelStore.current">
        <span>{{ t('erDiagram.tables', { count: modelStore.current.tables.length }) }}</span>
        <span>{{ t('erDiagram.fks', { count: modelStore.current.relations.length }) }}</span>
      </div>
      <button class="erm-btn" :disabled="!modelStore.current" :title="t('erDiagram.fitView')" @click="fitToView">
        <Scan :size="14" />
      </button>
    </div>

    <div class="erm-body">
      <!-- SVG 画布 -->
      <div
        ref="canvasRef"
        class="erm-canvas"
        :class="{ connecting: !!connectSource }"
        @wheel.prevent="onWheel"
        @mousedown="onPanStart"
        @mousemove="onPanMove"
        @mouseup="onPanEnd"
        @mouseleave="onPanEnd"
      >
        <div v-if="!modelStore.current" class="erm-empty">{{ t('erModel.noModel') }}</div>
        <div v-else-if="modelStore.current.tables.length === 0" class="erm-empty">
          {{ t('erModel.emptyTables') }}
        </div>
        <svg
          ref="svgRef"
          :width="svgW"
          :height="svgH"
          :style="{ transform: `translate(${tx}px, ${ty}px) scale(${scale})`, transformOrigin: '0 0' }"
          :viewBox="`${CANVAS_ORIGIN} ${CANVAS_ORIGIN} ${svgW} ${svgH}`"
          preserveAspectRatio="xMinYMin meet"
        >
          <defs>
            <marker
              id="erm-arrow"
              viewBox="0 0 10 10"
              refX="9"
              refY="5"
              markerWidth="10"
              markerHeight="10"
              markerUnits="userSpaceOnUse"
              orient="auto"
            >
              <path d="M 0 0 L 10 5 L 0 10 z" class="erm-arrow-head" />
            </marker>
          </defs>

          <!-- 关系连线 -->
          <g class="erm-links">
            <template v-for="edge in edgePaths" :key="edge.key">
              <path class="erm-link-hit" :d="edge.path" @click.stop="selectedRelationId = edge.key" />
              <path
                :d="edge.path"
                :class="{ selected: edge.selected }"
                marker-end="url(#erm-arrow)"
                @click.stop="selectedRelationId = edge.key"
              >
                <title>{{ edge.title }}</title>
              </path>
            </template>
          </g>

          <!-- 表框 -->
          <g
            v-for="node in tables"
            :key="node.id"
            class="erm-table-group"
            :transform="`translate(${node.x}, ${node.y})`"
            :class="{
              selected: selectedTableId === node.id,
              dragging: dragTableId === node.id,
            }"
            @click.stop="selectTable(node.id)"
            @mousedown.stop="onTableMouseDown($event, node)"
            @mouseenter="hoveredTableId = node.id"
            @mouseleave="hoveredTableId = null"
          >
            <rect :width="TABLE_W" :height="node.h" class="erm-table-bg" rx="6" />
            <rect :width="TABLE_W" height="32" class="erm-table-header" rx="6" />
            <!-- 拖拽建关系时的行高亮 -->
            <rect
              v-if="highlightTop(node.id, 'source') !== undefined"
              x="0" :y="highlightTop(node.id, 'source') ?? 0"
              :width="TABLE_W" :height="ROW_H"
              class="erm-row-highlight source"
            />
            <rect
              v-if="highlightTop(node.id, 'target') !== undefined"
              x="0" :y="highlightTop(node.id, 'target') ?? 0"
              :width="TABLE_W" :height="ROW_H"
              class="erm-row-highlight target"
            />
            <text x="12" y="21" class="erm-table-title">{{ node.name }}</text>
            <g v-for="(col, ci) in node.columns" :key="col.id">
              <text
                x="12"
                :y="colTextY(node, ci)"
                class="erm-col"
                :class="{ pk: col.pk }"
              >
                <tspan v-if="col.pk" class="erm-pk-mark">🔑 </tspan>{{ col.name }}
              </text>
              <text :x="TABLE_W - 12" :y="colTextY(node, ci)" class="erm-col-type">
                <tspan v-if="col.unique" class="erm-col-flag">U </tspan>
                <tspan v-if="!col.nullable" class="erm-col-flag">NN </tspan>
                {{ truncate(col.type, 24) }}
              </text>
              <text v-if="col.comment" x="12" :y="colTextY(node, ci) + 13" class="erm-col-extra">
                {{ truncate(col.comment, 34) }}
              </text>
              <!-- 拖拽建关系的起点：悬停表时显示，拖到另一张表的目标字段上建立关系 -->
              <circle
                class="erm-connect-dot"
                :cx="TABLE_W - 7"
                :cy="colRowTop(node, ci) + ROW_H / 2"
                r="4"
                @mousedown.stop.prevent="onConnectStart($event, node, col)"
                @click.stop
              >
                <title>{{ t('erModel.connectFrom') }}</title>
              </circle>
            </g>
          </g>

          <!-- 拖拽中的预览线与关系删除按钮：仅屏幕显示 -->
          <g>
            <path v-if="connectPreview" class="erm-link-preview" :d="connectPreview" />
            <g
              v-if="selectedRelation"
              :transform="`translate(${selectedRelation.mid.x}, ${selectedRelation.mid.y})`"
              class="erm-edge-del"
              @mousedown.stop
              @click.stop="removeRelation"
            >
              <title>{{ t('erModel.removeRelation') }}</title>
              <circle r="8" />
              <path d="M -3.5 -3.5 L 3.5 3.5 M 3.5 -3.5 L -3.5 3.5" />
            </g>
          </g>
        </svg>
      </div>

      <!-- 编辑面板：选中表后出现 -->
      <div v-if="selectedTable" class="erm-panel">
        <div class="erm-panel-head">
          <span>{{ t('erModel.editTable') }}</span>
          <button class="erm-panel-close" :title="t('common.close')" @click="selectedTableId = ''">
            <X :size="12" />
          </button>
        </div>
        <label class="erm-field">
          <span>{{ t('erModel.tableName') }}</span>
          <input
            class="erm-input"
            :value="selectedTable.name"
            @change="modelStore.renameTable(modelStore.current!.id, selectedTable!.id, ($event.target as HTMLInputElement).value)"
          />
        </label>
        <label class="erm-field">
          <span>{{ t('erModel.tableComment') }}</span>
          <input
            class="erm-input"
            :value="selectedTable.comment ?? ''"
            :placeholder="t('erModel.optional')"
            @change="modelStore.setTableComment(modelStore.current!.id, selectedTable!.id, ($event.target as HTMLInputElement).value)"
          />
        </label>

        <div class="erm-cols-head">
          <span>{{ t('erModel.columns', { count: selectedTable.columns.length }) }}</span>
          <button class="erm-btn" @click="addColumn(selectedTable)">
            <Plus :size="12" /> {{ t('erModel.addColumn') }}
          </button>
        </div>
        <div v-for="col in selectedTable.columns" :key="col.id" class="erm-col-row">
          <div class="erm-col-row-main">
            <input
              class="erm-input erm-col-name"
              :value="col.name"
              :placeholder="t('erModel.colName')"
              @change="modelStore.updateColumn(modelStore.current!.id, selectedTable!.id, col.id, { name: ($event.target as HTMLInputElement).value.trim() })"
            />
            <input
              class="erm-input erm-col-type"
              :value="col.type"
              :placeholder="'varchar(255)'"
              @change="modelStore.updateColumn(modelStore.current!.id, selectedTable!.id, col.id, { type: ($event.target as HTMLInputElement).value.trim() })"
            />
            <button class="erm-btn danger erm-col-del" :title="t('common.delete')" @click="removeColumn(col)">
              <Trash2 :size="12" />
            </button>
          </div>
          <div class="erm-col-row-flags">
            <label class="erm-flag" :title="t('erModel.flagPk')">
              <input type="checkbox" :checked="col.pk" @change="onTogglePk(col, ($event.target as HTMLInputElement).checked)" />
              PK
            </label>
            <label class="erm-flag" :title="t('erModel.flagNullable')">
              <input type="checkbox" :checked="!col.nullable" @change="modelStore.updateColumn(modelStore.current!.id, selectedTable!.id, col.id, { nullable: !($event.target as HTMLInputElement).checked })" />
              NOT NULL
            </label>
            <label class="erm-flag" :title="t('erModel.flagUnique')">
              <input type="checkbox" :checked="col.unique" @change="modelStore.updateColumn(modelStore.current!.id, selectedTable!.id, col.id, { unique: ($event.target as HTMLInputElement).checked })" />
              UNIQUE
            </label>
          </div>
          <input
            class="erm-input erm-col-tail"
            :value="col.default ?? ''"
            :placeholder="t('erModel.colDefault')"
            @change="modelStore.updateColumn(modelStore.current!.id, selectedTable!.id, col.id, { default: ($event.target as HTMLInputElement).value.trim() || undefined })"
          />
          <input
            class="erm-input erm-col-tail"
            :value="col.comment ?? ''"
            :placeholder="t('erModel.colComment')"
            @change="modelStore.updateColumn(modelStore.current!.id, selectedTable!.id, col.id, { comment: ($event.target as HTMLInputElement).value.trim() || undefined })"
          />
        </div>

        <button class="erm-btn danger erm-del-table" @click="removeTable">
          <Trash2 :size="14" /> {{ delTableArmed ? t('common.confirm') : t('erModel.deleteTable') }}
        </button>
      </div>
    </div>

    <!-- 缩放指示 -->
    <div class="erm-zoom">{{ Math.round(scale * 100) }}%</div>

    <!-- DDL 预览弹窗（V3-3.2） -->
    <div v-if="ddlOpen" class="erm-dialog-mask" @click.self="ddlOpen = false">
      <div class="erm-dialog" role="dialog" :aria-label="t('erModel.generateDdl')">
        <div class="erm-dialog-head">
          <span>{{ t('erModel.generateDdl') }}</span>
          <button class="erm-panel-close" :title="t('common.close')" @click="ddlOpen = false">
            <X :size="14" />
          </button>
        </div>
        <div class="erm-dialog-toolbar">
          <span class="erm-dialog-label">{{ t('erModel.ddlDialect') }}</span>
          <Select
            :model-value="ddlDialect"
            :options="ddlDialectOptions"
            size="sm"
            @update:model-value="ddlDialect = $event as ModelDialect"
          />
          <button class="erm-btn" @click="copyDdl">
            <Copy :size="13" /> {{ ddlCopied ? t('erModel.ddlCopied') : t('common.copy') }}
          </button>
        </div>
        <pre class="erm-dialog-code" data-testid="er-ddl-preview">{{ ddlText }}</pre>
      </div>
    </div>

    <!-- 比对同步弹窗（V3-3.3） -->
    <div v-if="syncOpen" class="erm-dialog-mask" @click.self="syncOpen = false">
      <div class="erm-dialog erm-dialog-wide" role="dialog" :aria-label="t('erModel.syncTitle')">
        <div class="erm-dialog-head">
          <span>{{ t('erModel.syncTitle') }}</span>
          <button class="erm-panel-close" :title="t('common.close')" @click="syncOpen = false">
            <X :size="14" />
          </button>
        </div>
        <div class="erm-dialog-toolbar">
          <span class="erm-dialog-label">{{ t('erModel.syncConnection') }}</span>
          <Select
            v-model="syncConnId"
            :options="syncConnOptions"
            size="sm"
            :placeholder="t('erModel.syncSelectConnection')"
          />
          <span class="erm-dialog-label">{{ t('erModel.syncDatabase') }}</span>
          <Select
            v-model="syncDb"
            :options="syncDbOptions"
            size="sm"
            :disabled="!syncConnId || syncDbLoading"
            :placeholder="syncDbLoading ? t('common.loading') : t('erModel.syncSelectDatabase')"
          />
          <button class="erm-btn primary" :disabled="!syncConnId || !syncDb || syncLoading" @click="runCompare">
            {{ syncLoading ? t('common.loading') : t('erModel.syncCompare') }}
          </button>
        </div>
        <div v-if="syncError" class="erm-sync-error">{{ syncError }}</div>
        <div v-if="syncPlan" class="erm-sync-body">
          <div class="erm-sync-options">
            <label class="erm-sync-opt">
              <input type="checkbox" v-model="syncDropTables" /> {{ t('erModel.syncDropTables') }}
            </label>
            <label class="erm-sync-opt">
              <input type="checkbox" v-model="syncDropCols" /> {{ t('erModel.syncDropColumns') }}
            </label>
          </div>
          <div v-if="syncPlan.items.length === 0" class="erm-sync-empty">
            <CheckCircle2 :size="16" /> {{ t('erModel.syncNoDiff') }}
          </div>
          <div v-else class="erm-sync-list">
            <label
              v-for="(item, idx) in syncPlan.items"
              :key="`${item.kind}-${item.tableName}-${item.columnName ?? ''}-${idx}`"
              class="erm-sync-item"
              :class="{ danger: item.kind === 'drop-table' || item.kind === 'drop-column' }"
            >
              <input type="checkbox" v-model="syncChecked[idx]" />
              <span>{{ syncItemLabel(item) }}</span>
            </label>
          </div>
          <div class="erm-sync-preview-head">{{ t('erModel.syncPreview') }}</div>
          <pre class="erm-dialog-code erm-sync-preview">{{ syncSqlPreview || t('erModel.syncPreviewEmpty') }}</pre>
          <div class="erm-dialog-toolbar erm-sync-actions">
            <button class="erm-btn primary" :disabled="!syncSelectedItems.length || syncExecuting" @click="executeSync">
              {{ syncExecuting ? t('erModel.syncExecuting') : t('erModel.syncExecute') }}
            </button>
            <span class="erm-dialog-label">
              {{ t('erModel.syncSelectedCount', { count: collectStatements(syncSelectedItems).length }) }}
            </span>
          </div>
        </div>
        <div v-else class="erm-sync-empty erm-sync-initial">{{ t('erModel.syncSelectHint') }}</div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue'
import { Plus, Trash2, X, Scan, Download, Upload, FileCode, Copy, GitCompare, CheckCircle2 } from 'lucide-vue-next'
import { invoke } from '@tauri-apps/api/core'
import Select from '@/components/ui/Select.vue'
import { useErModelStore, type ModelTable, type ModelColumn } from '@/stores/erModel'
import { useConnectionStore } from '@/stores/connection'
import { useDataBrowserStore } from '@/stores/dataBrowser'
import { confirmDanger } from '@/composables/useConfirm'
import { globalToast } from '@/composables/useToast'
import { t } from '@/i18n'
import { buildModelDdl, type ModelDialect } from '@/utils/erModelDdl'
import {
  buildModelSyncPlan,
  collectStatements,
  type DbSnapshot,
  type ModelSyncItem
} from '@/utils/erModelSync'

// ========== 常量 ==========
const TABLE_W = 220
const ROW_H = 20
const EXTRA_H = 14   // 注释行高度
const COL_GAP = 28   // 列与列横向间距
const ROW_GAP = 24   // 新表网格布局的行距
// 画布坐标系固定原点：允许表被拖到负坐标，同时保持视觉位置连续
const CANVAS_ORIGIN = -2000

// ========== 模型选择 ==========
const modelStore = useErModelStore()
const modelOptions = computed(() =>
  modelStore.models.map((m) => ({ value: m.id, label: m.name }))
)
const removeModelArmed = ref(false)
let removeModelTimer: number | undefined

function onSelectModel(id: string) {
  modelStore.setCurrent(id)
  resetEditorState()
}

function createModel() {
  const n = modelStore.models.length + 1
  modelStore.create(`${t('erModel.title')} ${n}`)
  resetEditorState()
}

async function removeModel() {
  const model = modelStore.current
  if (!model) return
  if (!removeModelArmed.value) {
    removeModelArmed.value = true
    window.clearTimeout(removeModelTimer)
    removeModelTimer = window.setTimeout(() => { removeModelArmed.value = false }, 3000)
    return
  }
  removeModelArmed.value = false
  window.clearTimeout(removeModelTimer)
  const ok = await confirmDanger({
    title: t('erModel.deleteModel'),
    message: t('erModel.deleteModelConfirm', { name: model.name }),
  })
  if (!ok) return
  modelStore.remove(model.id)
  resetEditorState()
  globalToast.success(t('erModel.modelDeleted'))
}

function resetEditorState() {
  selectedTableId.value = ''
  selectedRelationId.value = ''
  hoveredTableId.value = null
}

// ========== 画布变换 ==========
const canvasRef = ref<HTMLDivElement>()
const svgRef = ref<SVGSVGElement>()
const scale = ref(1)
const tx = ref(40)
const ty = ref(40)
let panning = false
let lastX = 0
let lastY = 0

function onWheel(e: WheelEvent) {
  const delta = e.deltaY > 0 ? -0.1 : 0.1
  scale.value = Math.max(0.2, Math.min(3, scale.value + delta))
}

function onPanStart(e: MouseEvent) {
  const target = e.target as Element | null
  // 点空白处取消选中（点表/连线本身时由各自的 click 处理）
  if (target?.tagName.toLowerCase() === 'svg' || target?.classList.contains('erm-canvas')) {
    selectedTableId.value = ''
    selectedRelationId.value = ''
  }
  panning = true
  lastX = e.clientX
  lastY = e.clientY
  canvasRef.value?.setAttribute('data-panning', 'true')
}

function onPanMove(e: MouseEvent) {
  if (!panning) return
  tx.value += e.clientX - lastX
  ty.value += e.clientY - lastY
  lastX = e.clientX
  lastY = e.clientY
}

function onPanEnd() {
  panning = false
  canvasRef.value?.removeAttribute('data-panning')
}

// ========== 表节点 ==========
interface PositionedTable {
  id: string
  name: string
  x: number
  y: number
  h: number
  columns: ModelColumn[]
}

const tables = computed<PositionedTable[]>(() =>
  (modelStore.current?.tables ?? []).map((tb) => ({
    id: tb.id,
    name: tb.name,
    x: tb.x,
    y: tb.y,
    h: tableHeight(tb),
    columns: tb.columns,
  }))
)

const svgW = ref(800)
const svgH = ref(600)

const canvasSize = computed(() => {
  const nodes = tables.value
  if (nodes.length === 0) return { w: 800, h: 600 }
  const maxX = Math.max(...nodes.map((n) => n.x + TABLE_W))
  const maxY = Math.max(...nodes.map((n) => n.y + n.h))
  return {
    w: Math.max(600, maxX - CANVAS_ORIGIN + 40),
    h: Math.max(400, maxY - CANVAS_ORIGIN + 40),
  }
})

watch(canvasSize, (size) => {
  svgW.value = size.w
  svgH.value = size.h
}, { immediate: true })

// 新表摆放：按已有表数量铺网格，避免全部叠在原点
function nextAutoPosition(): { x: number; y: number } {
  const count = modelStore.current?.tables.length ?? 0
  const perRow = 5
  return {
    x: (count % perRow) * (TABLE_W + COL_GAP),
    y: Math.floor(count / perRow) * (300 + ROW_GAP),
  }
}

function addTable() {
  const model = modelStore.current
  if (!model) return
  const pos = nextAutoPosition()
  modelStore.addTable(model.id, `table_${model.tables.length + 1}`, pos.x, pos.y)
}

// 拖拽画布上的表
const dragTableId = ref<string | null>(null)
let dragStartClientX = 0
let dragStartClientY = 0
let dragStartNodeX = 0
let dragStartNodeY = 0
let dragMoved = false
let lastDragEndAt = 0

function onTableMouseDown(e: MouseEvent, node: PositionedTable) {
  if (e.button !== 0) return
  dragTableId.value = node.id
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
  if (!dragTableId.value) return
  const dxClient = e.clientX - dragStartClientX
  const dyClient = e.clientY - dragStartClientY
  // 小于阈值视为点击，不触发拖动
  if (!dragMoved && Math.abs(dxClient) + Math.abs(dyClient) < 4) return
  dragMoved = true
  const nextX = Math.round(dragStartNodeX + dxClient / scale.value)
  const nextY = Math.round(dragStartNodeY + dyClient / scale.value)
  modelStore.moveTable(
    modelStore.current!.id,
    dragTableId.value,
    Math.max(CANVAS_ORIGIN + 20, nextX),
    Math.max(CANVAS_ORIGIN + 20, nextY),
  )
}

function onTableDragEnd() {
  window.removeEventListener('mousemove', onTableDragMove)
  window.removeEventListener('mouseup', onTableDragEnd)
  if (dragTableId.value && dragMoved) lastDragEndAt = Date.now()
  dragTableId.value = null
}

function selectTable(id: string) {
  // 拖动结束后紧接着的 click 不应切换选中
  if (Date.now() - lastDragEndAt < 200) return
  selectedTableId.value = id
}

// ========== 编辑面板 ==========
const selectedTableId = ref('')
const selectedTable = computed(() =>
  modelStore.current?.tables.find((tb) => tb.id === selectedTableId.value) ?? null
)
const delTableArmed = ref(false)
let delTableTimer: number | undefined

function removeColumn(col: ModelColumn) {
  const model = modelStore.current
  if (!model || !selectedTable.value) return
  const used = model.relations.some(
    (r) =>
      (r.fromTableId === selectedTable.value!.id && r.fromColumnId === col.id) ||
      (r.toTableId === selectedTable.value!.id && r.toColumnId === col.id)
  )
  if (used) {
    globalToast.warning(t('erModel.columnInRelation'))
    return
  }
  modelStore.removeColumn(model.id, selectedTable.value.id, col.id)
}

function addColumn(table: ModelTable) {
  const model = modelStore.current
  if (!model) return
  modelStore.addColumn(model.id, table.id, {
    name: `col_${table.columns.length + 1}`,
    type: 'varchar(255)',
    pk: false,
    nullable: true,
    unique: false,
  })
}

function onTogglePk(col: ModelColumn, checked: boolean) {
  const model = modelStore.current
  if (!model || !selectedTable.value) return
  modelStore.updateColumn(model.id, selectedTable.value.id, col.id, { pk: checked })
  // 设为主键时通常不应为空
  if (checked) modelStore.updateColumn(model.id, selectedTable.value.id, col.id, { nullable: false })
}

async function removeTable() {
  const model = modelStore.current
  const table = selectedTable.value
  if (!model || !table) return
  if (!delTableArmed.value) {
    delTableArmed.value = true
    window.clearTimeout(delTableTimer)
    delTableTimer = window.setTimeout(() => { delTableArmed.value = false }, 3000)
    return
  }
  delTableArmed.value = false
  window.clearTimeout(delTableTimer)
  const ok = await confirmDanger({
    title: t('erModel.deleteTable'),
    message: t('erModel.deleteTableConfirm', { name: table.name }),
  })
  if (!ok) return
  modelStore.removeTable(model.id, table.id)
  selectedTableId.value = ''
}

// ========== 拖拽建关系 ==========
const connectSource = ref<{ tableId: string; columnId: string } | null>(null)
const connectTarget = ref<{ tableId: string; columnId: string } | null>(null)
const connectCursor = ref<{ x: number; y: number } | null>(null)
const hoveredTableId = ref<string | null>(null)

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

// 光标落在哪张表的哪行字段上
function findTargetAt(point: { x: number; y: number }): { tableId: string; columnId: string } | null {
  for (const node of tables.value) {
    if (point.x < node.x || point.x > node.x + TABLE_W) continue
    if (point.y < node.y || point.y > node.y + node.h) continue
    const localY = point.y - node.y
    if (localY < 32) continue
    for (let i = 0; i < node.columns.length; i++) {
      const col = node.columns[i]!
      const top = colRowTop(node, i)
      if (localY >= top && localY < top + colRowHeight(col)) {
        const source = connectSource.value
        if (source && source.tableId === node.id && source.columnId === col.id) return null
        return { tableId: node.id, columnId: col.id }
      }
    }
  }
  return null
}

function onConnectStart(e: MouseEvent, node: PositionedTable, col: ModelColumn) {
  if (e.button !== 0) return
  connectSource.value = { tableId: node.id, columnId: col.id }
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
  const model = modelStore.current
  if (!model || !source || !target) return
  const rel = modelStore.addRelation(model.id, {
    fromTableId: source.tableId,
    fromColumnId: source.columnId,
    toTableId: target.tableId,
    toColumnId: target.columnId,
  })
  if (rel) {
    globalToast.success(t('erModel.relationAdded'))
  } else {
    globalToast.info(t('erModel.relationExists'))
  }
}

// 拖拽过程中的预览线：从起点画到光标
const connectPreview = computed(() => {
  const source = connectSource.value
  const cursor = connectCursor.value
  if (!source || !cursor) return ''
  const node = tables.value.find((n) => n.id === source.tableId)
  if (!node) return ''
  const col = node.columns.find((c) => c.id === source.columnId)
  if (!col) return ''
  const y1 = node.y + colRowTop(node, node.columns.indexOf(col)) + ROW_H / 2
  const x1 = cursor.x < node.x ? node.x : node.x + TABLE_W
  return bezier(x1, y1, cursor.x, cursor.y)
})

// 拖拽过程中高亮起点行与目标行
function highlightTop(tableId: string, kind: 'source' | 'target'): number | undefined {
  const point = kind === 'source' ? connectSource.value : connectTarget.value
  if (!point || point.tableId !== tableId) return undefined
  const node = tables.value.find((n) => n.id === tableId)
  if (!node) return undefined
  const ci = node.columns.findIndex((c) => c.id === point.columnId)
  return ci >= 0 ? colRowTop(node, ci) : undefined
}

// ========== 关系连线 ==========
interface EdgePath {
  key: string
  path: string
  mid: { x: number; y: number }
  selected: boolean
  title: string
}

const selectedRelationId = ref('')

const selectedRelation = computed(() =>
  edgePaths.value.find((e) => e.selected) ?? null
)

const edgePaths = computed<EdgePath[]>(() => {
  const model = modelStore.current
  if (!model) return []
  const nodes = new Map(tables.value.map((n) => [n.id, n]))
  const result: EdgePath[] = []
  for (const rel of model.relations) {
    const srcNode = nodes.get(rel.fromTableId)
    const dstNode = nodes.get(rel.toTableId)
    if (!srcNode || !dstNode) continue
    const srcColIdx = srcNode.columns.findIndex((c) => c.id === rel.fromColumnId)
    const dstColIdx = dstNode.columns.findIndex((c) => c.id === rel.toColumnId)
    const srcName = srcColIdx >= 0 ? srcNode.columns[srcColIdx]!.name : '?'
    const dstName = dstColIdx >= 0 ? dstNode.columns[dstColIdx]!.name : '?'
    const srcY = srcColIdx >= 0
      ? srcNode.y + colRowTop(srcNode, srcColIdx) + ROW_H / 2
      : srcNode.y + 20
    const dstY = dstColIdx >= 0
      ? dstNode.y + colRowTop(dstNode, dstColIdx) + ROW_H / 2
      : dstNode.y + 20
    const srcRight = srcNode.x + TABLE_W
    const dstRight = dstNode.x + TABLE_W

    let path: string
    let mid: { x: number; y: number }
    if (dstNode.x >= srcRight) {
      const midX = (srcRight + dstNode.x) / 2
      path = bezier(srcRight, srcY, dstNode.x, dstY)
      mid = { x: midX, y: (srcY + dstY) / 2 }
    } else if (dstRight <= srcNode.x) {
      const midX = (srcNode.x + dstRight) / 2
      path = bezier(srcNode.x, srcY, dstRight, dstY)
      mid = { x: midX, y: (srcY + dstY) / 2 }
    } else {
      // 同一列或水平重叠：从右边缘绕行
      const off = Math.max(srcRight, dstRight) + 48
      const d = `M ${srcRight} ${srcY} C ${off} ${srcY}, ${off} ${dstY}, ${dstRight} ${dstY}`
      path = d
      mid = { x: (srcRight + 3 * off + 3 * off + dstRight) / 8, y: (srcY + 3 * srcY + 3 * dstY + dstY) / 8 }
    }

    result.push({
      key: rel.id,
      path,
      mid,
      selected: selectedRelationId.value === rel.id,
      title: `${srcNode.name}.${srcName} → ${dstNode.name}.${dstName}`,
    })
  }
  return result
})

function removeRelation() {
  const model = modelStore.current
  if (!model || !selectedRelation.value) return
  modelStore.removeRelation(model.id, selectedRelation.value.key)
  selectedRelationId.value = ''
}

// ========== 导入 / 导出 ==========
// ========== DDL 预览（V3-3.2） ==========
const ddlOpen = ref(false)
const ddlDialect = ref<ModelDialect>('mysql')
const ddlCopied = ref(false)
let ddlCopiedTimer: number | undefined
const ddlDialectOptions = [
  { value: 'mysql' as ModelDialect, label: 'MySQL' },
  { value: 'postgres' as ModelDialect, label: 'PostgreSQL' },
]
const ddlText = computed(() => {
  const model = modelStore.current
  return model ? buildModelDdl(model, ddlDialect.value) : ''
})

function openDdlDialog() {
  ddlOpen.value = true
  ddlCopied.value = false
}

async function copyDdl() {
  try {
    await navigator.clipboard.writeText(ddlText.value)
    ddlCopied.value = true
    window.clearTimeout(ddlCopiedTimer)
    ddlCopiedTimer = window.setTimeout(() => { ddlCopied.value = false }, 2000)
  } catch {
    globalToast.error(t('erModel.ddlCopyFailed'))
  }
}

// ========== 比对同步（V3-3.3） ==========
const connStore = useConnectionStore()
const dataBrowserStore = useDataBrowserStore()

const syncOpen = ref(false)
const syncConnId = ref('')
const syncDb = ref('')
const syncDbLoading = ref(false)
const syncDbOptions = ref<{ label: string; value: string }[]>([])
const syncLoading = ref(false)
const syncError = ref('')
const syncSnapshot = ref<DbSnapshot | null>(null)
const syncDropTables = ref(false)
const syncDropCols = ref(false)
const syncChecked = ref<boolean[]>([])
const syncExecuting = ref(false)

const syncConnOptions = computed(() =>
  connStore.activeConnections.map((c) => ({ label: `${c.name} (${c.host}:${c.port})`, value: c.id }))
)

function openSyncDialog() {
  syncConnId.value = connStore.currentConnection?.id ?? ''
  syncDb.value = connStore.currentConnection?.database ?? ''
  syncSnapshot.value = null
  syncError.value = ''
  syncDropTables.value = false
  syncDropCols.value = false
  syncOpen.value = true
  void loadSyncDatabases()
}

async function loadSyncDatabases() {
  if (!syncConnId.value) {
    syncDbOptions.value = []
    syncDb.value = ''
    return
  }
  syncDbLoading.value = true
  try {
    const databases = await dataBrowserStore.listDatabases(syncConnId.value)
    syncDbOptions.value = databases.map((name) => ({ label: name, value: name }))
    if (!databases.includes(syncDb.value)) syncDb.value = databases[0] ?? ''
  } catch (e) {
    syncError.value = t('erModel.syncLoadDbsFailed', { error: String(e) })
  } finally {
    syncDbLoading.value = false
  }
}

// 切换连接：清空比对结果并重新加载库列表
watch(syncConnId, () => {
  syncSnapshot.value = null
  syncError.value = ''
  void loadSyncDatabases()
})

async function runCompare() {
  if (!modelStore.current || !syncConnId.value || !syncDb.value) return
  syncLoading.value = true
  syncError.value = ''
  try {
    // 复用反向 ER 的数据源：一次拿到表/列/外键快照
    syncSnapshot.value = await invoke<DbSnapshot>('get_er_diagram_data', {
      connectionId: syncConnId.value,
      database: syncDb.value,
    })
  } catch (e) {
    syncError.value = t('erModel.syncCompareFailed', { error: String(e) })
    syncSnapshot.value = null
  } finally {
    syncLoading.value = false
  }
}

/** 计划随模型 / 快照 / 删除选项即时重算；勾选状态按差异清单重置 */
const syncPlan = computed(() => {
  if (!modelStore.current || !syncSnapshot.value) return null
  return buildModelSyncPlan(modelStore.current, syncSnapshot.value, {
    dropMissingTables: syncDropTables.value,
    dropMissingColumns: syncDropCols.value,
  })
})

watch(() => syncPlan.value?.items.map((i) => `${i.kind}:${i.tableName}:${i.columnName ?? ''}`).join('|'), () => {
  const items = syncPlan.value?.items ?? []
  // DROP 类操作默认不勾选，其余差异默认勾选
  syncChecked.value = items.map((i) => i.kind !== 'drop-table' && i.kind !== 'drop-column')
})

const syncSelectedItems = computed(() =>
  (syncPlan.value?.items ?? []).filter((_, idx) => syncChecked.value[idx] === true)
)
const syncSqlPreview = computed(() => collectStatements(syncSelectedItems.value).join('\n\n'))

function syncItemLabel(item: ModelSyncItem): string {
  switch (item.kind) {
    case 'create-table': return t('erModel.syncKindCreateTable', { table: item.tableName })
    case 'drop-table': return t('erModel.syncKindDropTable', { table: item.tableName })
    case 'add-column': return t('erModel.syncKindAddColumn', { table: item.tableName, column: item.columnName ?? '' })
    case 'alter-column': return t('erModel.syncKindAlterColumn', { table: item.tableName, column: item.columnName ?? '' })
    case 'drop-column': return t('erModel.syncKindDropColumn', { table: item.tableName, column: item.columnName ?? '' })
    case 'add-fk': return t('erModel.syncKindAddFk', { table: item.tableName, column: item.columnName ?? '' })
  }
}

async function executeSync() {
  const statements = collectStatements(syncSelectedItems.value)
  if (!statements.length || !syncConnId.value || !syncDb.value) return
  const ok = await confirmDanger({
    title: t('erModel.syncExecute'),
    message: t('erModel.syncConfirm', { count: statements.length, database: syncDb.value }),
  })
  if (!ok) return
  syncExecuting.value = true
  syncError.value = ''
  try {
    // 复用 Schema Diff 的执行链路：逐条应用、遇错即停
    const res = await dataBrowserStore.applySchemaDiff(syncConnId.value, syncDb.value, statements)
    if (res.failed_at !== null || res.error) {
      syncError.value = t('erModel.syncPartial', {
        count: res.applied,
        failedAt: res.failed_at ?? 0,
        error: res.error ?? '',
      })
    } else {
      globalToast.success(t('erModel.syncApplied', { count: res.applied }))
      // 同步后重新比对，展示剩余差异
      void runCompare()
    }
  } catch (e) {
    syncError.value = t('erModel.syncCompareFailed', { error: String(e) })
  } finally {
    syncExecuting.value = false
  }
}

function base64FromBytes(bytes: Uint8Array) {
  let binary = ''
  const chunk = 0x8000
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk))
  }
  return btoa(binary)
}

async function exportModel() {
  const model = modelStore.current
  if (!model) return
  try {
    const json = modelStore.toFile(model.id)
    const base64 = base64FromBytes(new TextEncoder().encode(json))
    const path = await invoke<string>('save_binary_file_to_dialog', {
      contentBase64: base64,
      defaultFilename: `${model.name || 'model'}.json`,
      filterName: 'JSON 文件',
      extensions: ['json'],
    })
    globalToast.success(t('erModel.exportSuccess', { path }))
  } catch (e) {
    const message = String(e)
    if (message.includes('取消')) return
    globalToast.error(t('erModel.importExportFailed', { error: message }))
  }
}

async function importModel() {
  try {
    const [, content] = await invoke<[string, string]>('read_file_from_dialog', { format: 'json' })
    const error = modelStore.importFromJson(content, `${t('erModel.title')} ${modelStore.models.length + 1}`)
    if (error) {
      globalToast.error(error)
      return
    }
    resetEditorState()
    globalToast.success(t('erModel.importSuccess', { name: modelStore.current?.name ?? '' }))
    await nextTick()
    fitToView()
  } catch (e) {
    const message = String(e)
    if (message.includes('取消')) return
    globalToast.error(t('erModel.importExportFailed', { error: message }))
  }
}

// ========== 适配窗口 ==========
function fitToView() {
  const canvas = canvasRef.value
  const nodes = tables.value
  if (!canvas || nodes.length === 0) return
  const minX = Math.min(...nodes.map((n) => n.x))
  const minY = Math.min(...nodes.map((n) => n.y))
  const maxX = Math.max(...nodes.map((n) => n.x + TABLE_W))
  const maxY = Math.max(...nodes.map((n) => n.y + n.h))
  const w = Math.max(1, maxX - minX)
  const h = Math.max(1, maxY - minY)
  const pad = 32
  const availW = Math.max(80, canvas.clientWidth - pad * 2)
  const availH = Math.max(80, canvas.clientHeight - pad * 2)
  const next = Math.min(1, availW / w, availH / h)
  scale.value = Math.max(0.2, Math.min(3, next))
  tx.value = pad + (availW - w * scale.value) / 2 - (minX - CANVAS_ORIGIN) * scale.value
  ty.value = pad + (availH - h * scale.value) / 2 - (minY - CANVAS_ORIGIN) * scale.value
}

// ========== 快捷键 ==========
function onKeydown(e: KeyboardEvent) {
  const target = e.target as HTMLElement | null
  const editing = !!target && (
    target.tagName === 'INPUT' ||
    target.tagName === 'TEXTAREA' ||
    target.isContentEditable
  )
  if (!editing && (e.key === 'Delete' || e.key === 'Backspace') && selectedRelationId.value) {
    e.preventDefault()
    removeRelation()
  }
}

onMounted(() => {
  window.addEventListener('keydown', onKeydown)
})

onUnmounted(() => {
  window.removeEventListener('keydown', onKeydown)
  window.removeEventListener('mousemove', onTableDragMove)
  window.removeEventListener('mouseup', onTableDragEnd)
  window.removeEventListener('mousemove', onConnectMove)
  window.removeEventListener('mouseup', onConnectEnd)
  window.clearTimeout(removeModelTimer)
  window.clearTimeout(delTableTimer)
  window.clearTimeout(ddlCopiedTimer)
})

// ========== 绘制辅助 ==========
function bezier(x1: number, y1: number, x2: number, y2: number): string {
  const midX = (x1 + x2) / 2
  return `M ${x1} ${y1} C ${midX} ${y1}, ${midX} ${y2}, ${x2} ${y2}`
}

function truncate(text: string, max: number) {
  return text.length > max ? `${text.slice(0, max - 1)}…` : text
}

const colRowHeight = (col: ModelColumn) => ROW_H + (col.comment ? EXTRA_H : 0)
const colRowTop = (table: { columns: ModelColumn[] }, index: number) =>
  32 + table.columns.slice(0, index).reduce((acc, col) => acc + colRowHeight(col), 0)
const colTextY = (table: { columns: ModelColumn[] }, index: number) => colRowTop(table, index) + 14
const tableHeight = (table: ModelTable) =>
  32 + table.columns.reduce((acc, col) => acc + colRowHeight(col), 0) + 16
</script>

<style scoped>
.er-model-designer {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  position: relative;
  background: var(--canvas);
}

/* 工具栏 */
.erm-toolbar {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  height: var(--toolbar-height);
  padding: 0 var(--space-2);
  border-bottom: 1px solid var(--hairline);
  background: var(--canvas);
  flex-shrink: 0;
}

.erm-name-input {
  height: var(--control-height-sm);
  width: 160px;
  padding: 0 var(--space-2);
  border: 1px solid var(--hairline);
  border-radius: var(--radius-sm);
  background: var(--raised);
  color: var(--text-primary);
  font-size: var(--text-xs);
}
.erm-name-input:focus { outline: none; border-color: var(--brand-500); }

.erm-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 5px;
  height: var(--control-height-sm);
  padding: 0 var(--space-2);
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--text-xs);
  line-height: 1;
  white-space: nowrap;
  flex-shrink: 0;
  cursor: pointer;
  transition: var(--transition-colors);
}
.erm-btn:hover:not(:disabled) { background: var(--hover-fill); color: var(--text-primary); }
.erm-btn:disabled { opacity: 0.45; cursor: not-allowed; }
.erm-btn.primary { background: var(--brand-500); color: #fff; }
.erm-btn.primary:hover:not(:disabled) { background: var(--brand-600); color: #fff; }
.erm-btn.danger { color: var(--error-500); }
.erm-btn.danger:hover:not(:disabled) { background: var(--error-500); color: #fff; }

.erm-divider {
  width: 1px;
  height: 16px;
  background: var(--hairline);
  margin: 0 var(--space-1);
  flex-shrink: 0;
}

.erm-spacer { flex: 1; }

.erm-stats {
  display: flex;
  gap: var(--space-3);
  color: var(--text-tertiary);
  font-size: var(--text-xs);
  white-space: nowrap;
  flex-shrink: 0;
  margin-right: var(--space-2);
}

/* 主体：画布 + 编辑面板 */
.erm-body {
  flex: 1;
  display: flex;
  min-height: 0;
}

.erm-canvas {
  flex: 1;
  min-height: 0;
  min-width: 0;
  overflow: hidden;
  position: relative;
  cursor: grab;
}
.erm-canvas[data-panning="true"] { cursor: grabbing; }
.erm-canvas.connecting,
.erm-canvas.connecting[data-panning="true"] { cursor: crosshair; }

svg {
  display: block;
  flex: none;
}

.erm-empty {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-tertiary);
  font-size: var(--text-sm);
  pointer-events: none;
}

/* 连线 */
.erm-links path {
  fill: none;
  stroke: var(--hairline-strong);
  stroke-width: 1.5;
  pointer-events: stroke;
  cursor: pointer;
}
.erm-links path.selected {
  stroke: var(--brand-500);
  stroke-width: 2.5;
}
.erm-link-hit {
  stroke: transparent;
  stroke-width: 12;
}
.erm-arrow-head { fill: var(--hairline-strong); }

/* 表框 */
.erm-table-bg {
  fill: var(--raised);
  stroke: var(--hairline-strong);
  stroke-width: 1;
  transition: stroke 150ms, fill 150ms;
}
.erm-table-bg.selected,
g.selected .erm-table-bg {
  stroke: var(--brand-500);
  fill: var(--selected-fill);
}
.erm-table-header {
  fill: var(--hover-fill);
  rx: 6;
  cursor: move;
}
.erm-table-title {
  fill: var(--text-primary);
  font-weight: 600;
  font-size: 12px;
  pointer-events: none;
}
.erm-col {
  fill: var(--text-secondary);
  font-size: 11px;
}
.erm-col.pk { fill: var(--brand-500); font-weight: 600; }
.erm-pk-mark { font-size: 10px; }
.erm-col-type {
  fill: var(--text-tertiary);
  font-size: 10px;
  text-anchor: end;
}
.erm-col-flag { fill: var(--brand-500); font-size: 10px; }
.erm-col-extra { fill: var(--text-tertiary); font-size: 10px; }

g:hover .erm-table-bg { stroke: var(--brand-500); }
g.dragging .erm-table-bg {
  stroke: var(--brand-500);
  filter: drop-shadow(0 4px 8px rgba(0, 0, 0, 0.35));
}

/* 拖拽建关系的起点圆点 */
.erm-connect-dot {
  fill: var(--brand-500);
  opacity: 0;
  cursor: crosshair;
  transition: opacity 120ms;
}
.erm-table-group:hover .erm-connect-dot { opacity: 0.4; }
.erm-connect-dot:hover { opacity: 1; }

/* 行高亮 / 预览线 / 关系删除按钮 */
.erm-row-highlight {
  fill: var(--brand-500);
  opacity: 0.12;
  pointer-events: none;
}
.erm-row-highlight.target { opacity: 0.2; }
.erm-link-preview {
  fill: none;
  stroke: var(--brand-500);
  stroke-width: 1.5;
  stroke-dasharray: 4 3;
  pointer-events: none;
}
.erm-edge-del { cursor: pointer; }
.erm-edge-del circle {
  fill: var(--overlay);
  stroke: var(--brand-500);
  stroke-width: 1;
}
.erm-edge-del path {
  stroke: var(--brand-500);
  stroke-width: 1.5;
  stroke-linecap: round;
}
.erm-edge-del:hover circle { fill: var(--brand-500); }
.erm-edge-del:hover path { stroke: #fff; }

/* 缩放指示 */
.erm-zoom {
  position: absolute;
  bottom: 12px;
  right: 12px;
  font-size: 10px;
  color: var(--text-secondary);
  background: var(--raised);
  border: 1px solid var(--hairline);
  padding: 0 6px;
  line-height: 18px;
  border-radius: var(--radius-xs);
}

/* 编辑面板 */
.erm-panel {
  width: 280px;
  flex-shrink: 0;
  border-left: 1px solid var(--hairline);
  background: var(--raised);
  overflow-y: auto;
  padding: var(--space-3);
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.erm-panel-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  color: var(--text-primary);
  font-weight: 600;
  font-size: var(--text-sm);
}

.erm-panel-close {
  display: inline-flex;
  align-items: center;
  padding: 2px;
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  border-radius: var(--radius-sm);
}
.erm-panel-close:hover { color: var(--text-primary); background: var(--hover-fill); }

.erm-field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.erm-input {
  height: var(--control-height-sm);
  padding: 0 var(--space-2);
  border: 1px solid var(--hairline);
  border-radius: var(--radius-sm);
  background: var(--canvas);
  color: var(--text-primary);
  font-size: var(--text-xs);
  min-width: 0;
}
.erm-input:focus { outline: none; border-color: var(--brand-500); }

.erm-cols-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: var(--space-2);
  color: var(--text-primary);
  font-weight: 600;
  font-size: var(--text-xs);
}

.erm-col-row {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: var(--space-2);
  border: 1px solid var(--hairline);
  border-radius: var(--radius-md);
  background: var(--canvas);
}

.erm-col-row-main {
  display: flex;
  gap: 4px;
  align-items: center;
}
.erm-col-name { flex: 1; }
.erm-col-type { flex: 1.2; font-family: monospace; }
.erm-col-del { padding: 0 4px; }
.erm-col-tail { font-size: 11px; }

.erm-col-row-flags {
  display: flex;
  gap: var(--space-2);
}
.erm-flag {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  font-size: 10px;
  color: var(--text-secondary);
  cursor: pointer;
  user-select: none;
}
.erm-flag input { accent-color: var(--brand-500); }

.erm-del-table {
  margin-top: var(--space-3);
  border: 1px solid var(--hairline);
  justify-content: center;
}

/* DDL 预览弹窗 */
.erm-dialog-mask {
  position: absolute;
  inset: 0;
  z-index: 30;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--overlay);
}

.erm-dialog {
  display: flex;
  flex-direction: column;
  width: min(720px, calc(100% - 48px));
  max-height: min(560px, calc(100% - 48px));
  background: var(--raised);
  border: 1px solid var(--hairline);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xl);
  overflow: hidden;
}

.erm-dialog-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-3) var(--space-4);
  border-bottom: 1px solid var(--hairline);
  font-weight: 600;
  font-size: var(--text-sm);
  color: var(--text-primary);
}

.erm-dialog-toolbar {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-4);
  border-bottom: 1px solid var(--hairline);
}

.erm-dialog-label {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  white-space: nowrap;
}

.erm-dialog-code {
  flex: 1;
  min-height: 0;
  margin: 0;
  padding: var(--space-3) var(--space-4);
  overflow: auto;
  background: var(--canvas);
  color: var(--text-primary);
  font-family: var(--font-mono, monospace);
  font-size: var(--text-xs);
  line-height: 1.6;
  white-space: pre;
}

/* 比对同步弹窗（V3-3.3） */
.erm-dialog-wide {
  width: min(880px, calc(100% - 48px));
  max-height: min(700px, calc(100% - 48px));
}

.erm-sync-body {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
}

.erm-sync-options {
  display: flex;
  gap: var(--space-4);
  padding: var(--space-2) var(--space-4);
  border-bottom: 1px solid var(--hairline);
}

.erm-sync-opt {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: var(--text-xs);
  color: var(--text-secondary);
  cursor: pointer;
  user-select: none;
}

.erm-sync-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 200px;
  overflow: auto;
  padding: var(--space-2) var(--space-4);
  border-bottom: 1px solid var(--hairline);
}

.erm-sync-item {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: var(--text-xs);
  color: var(--text-primary);
  cursor: pointer;
  user-select: none;
}

.erm-sync-item.danger {
  color: var(--error-600, #dc2626);
}

.erm-sync-empty {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: var(--space-3) var(--space-4);
  font-size: var(--text-sm);
  color: var(--text-tertiary);
}

.erm-sync-initial {
  border-top: 1px solid var(--hairline);
}

.erm-sync-error {
  padding: var(--space-2) var(--space-4);
  font-size: var(--text-xs);
  color: var(--error-600, #dc2626);
}

.erm-sync-preview-head {
  padding: var(--space-2) var(--space-4) 0;
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.erm-sync-preview {
  min-height: 120px;
}

.erm-sync-actions {
  border-top: 1px solid var(--hairline);
}
</style>
