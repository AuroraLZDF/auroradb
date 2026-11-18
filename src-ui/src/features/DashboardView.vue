<template>
  <div class="dashboard-view">
    <div class="dashboard-toolbar">
      <Select
        :model-value="dashStore.currentId"
        :options="dashboardOptions"
        size="sm"
        @update:model-value="dashStore.setCurrent($event)"
      />
      <input
        v-if="dashStore.current"
        class="dash-name-input"
        :value="dashStore.current.name"
        :title="t('dashboard.renamePrompt')"
        @change="dashStore.rename(dashStore.current!.id, ($event.target as HTMLInputElement).value)"
      />
      <button class="dash-btn" :title="t('dashboard.newNamePrompt')" @click="createDashboard">
        <Plus :size="14" /> {{ t('dashboard.new') }}
      </button>
      <button class="dash-btn danger" :disabled="!dashStore.current" @click="confirmRemove">
        <Trash2 :size="14" /> {{ removeArmed ? t('common.confirm') : t('dashboard.delete') }}
      </button>
      <span class="dash-spacer"></span>
      <button class="dash-btn" :disabled="!dashStore.current" @click="refreshAll">
        <RefreshCw :size="14" /> {{ t('dashboard.refreshAll') }}
      </button>
      <button class="dash-btn primary" :disabled="!dashStore.current" @click="openCardEditor">
        <Plus :size="14" /> {{ t('dashboard.addCard') }}
      </button>
    </div>

    <div v-if="!dashStore.current" class="dashboard-empty">{{ t('dashboard.select') }}</div>
    <div v-else-if="dashStore.current.cards.length === 0" class="dashboard-empty">{{ t('dashboard.empty') }}</div>
    <div v-else class="dashboard-grid">
      <div v-for="card in dashStore.current.cards" :key="card.id" class="dash-card">
        <div class="dash-card-head">
          <span class="dash-card-title" :title="card.title">{{ card.title }}</span>
          <span v-if="errorOf(card.id)" class="dash-card-error" :title="errorOf(card.id)">{{ t('dashboard.cardNoData') }}</span>
          <button class="dash-card-btn" :title="t('dashboard.refreshAll')" @click="runCard(card)">
            <RefreshCw :size="13" :class="{ spinning: isLoading(card.id) }" />
          </button>
          <button class="dash-card-btn" :title="t('dashboard.delete')" @click="removeCard(card.id)">
            <X :size="13" />
          </button>
        </div>
        <div class="dash-card-body">
          <div v-if="isLoading(card.id)" class="dash-card-empty"><RefreshCw :size="16" class="spinning" /></div>
          <div v-else-if="errorOf(card.id)" class="dash-card-empty" :title="errorOf(card.id)">{{ errorOf(card.id) }}</div>
          <div v-else-if="!hasData(card.id)" class="dash-card-empty">{{ t('dashboard.cardNoData') }}</div>
          <ChartRenderer v-else :columns="dataOf(card.id).columns" :rows="dataOf(card.id).rows" :config="card.config" />
        </div>
      </div>
    </div>

    <!-- 添加卡片：SQL + 运行预览 + 字段映射 -->
    <div v-if="editorOpen" class="card-editor-mask" @click.self="editorOpen = false">
      <div class="card-editor">
        <div class="card-editor-row">
          <input v-model="editorTitle" class="card-editor-title" :placeholder="t('dashboard.cardTitle')" />
        </div>
        <textarea
          v-model="editorSql"
          class="card-editor-sql"
          spellcheck="false"
          :placeholder="t('dashboard.cardSqlHint')"
        ></textarea>
        <div class="card-editor-row chart-config">
          <div class="chart-types">
            <button
              v-for="ct in CHART_TYPES"
              :key="ct"
              :class="{ active: editorConfig.type === ct }"
              :title="t(`dataTable.chartType${ct}`)"
              @click="editorConfig.type = ct"
            >
              <component :is="CHART_TYPE_ICONS[ct]" :size="14" />
            </button>
          </div>
          <label class="chart-field">
            <span>{{ t('dataTable.chartDimension') }}</span>
            <Select v-model="editorConfig.dimension" size="sm" :options="editorColumnOptions" />
          </label>
          <label class="chart-field">
            <span>{{ t('dataTable.chartMeasure') }}</span>
            <Select
              multiple size="sm"
              :options="editorColumnOptions"
              :multi-value="editorConfig.measures"
              @update:multi-value="editorConfig.measures = $event"
            />
          </label>
          <label class="chart-field">
            <span>{{ t('dataTable.chartAggregation') }}</span>
            <Select v-model="editorConfig.agg" size="sm" :options="aggOptions" />
          </label>
          <button class="dash-btn" :disabled="!editorSql.trim()" @click="runPreview">
            <Play :size="13" /> {{ t('dashboard.preview') }}
          </button>
        </div>
        <div class="card-editor-preview">
          <div v-if="editorError" class="dash-card-empty">{{ editorError }}</div>
          <div v-else-if="!previewData" class="dash-card-empty">{{ t('dashboard.preview') }}</div>
          <ChartRenderer v-else :columns="previewData.columns" :rows="previewData.rows" :config="editorConfig" />
        </div>
        <div class="card-editor-foot">
          <button class="dash-btn" @click="editorOpen = false">{{ t('common.cancel') }}</button>
          <button
            class="dash-btn primary"
            :disabled="!editorSql.trim() || !editorConfig.dimension || editorConfig.measures.length === 0 || !previewData"
            @click="saveCard"
          >{{ t('dashboard.saveCard') }}</button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
/**
 * 仪表盘视图（V3-1.3）：卡片网格 + 聚合查询取数。
 * 卡片定义（SQL + 字段映射）存 localStorage，数据每次打开/刷新时执行 SQL 获取。
 */
import { computed, onMounted, reactive, ref } from 'vue'
import { Play, Plus, RefreshCw, Trash2, X } from 'lucide-vue-next'
import { BarChart3, LineChart as LineChartIcon, PieChart as PieChartIcon, ScatterChart as ScatterChartIcon } from 'lucide-vue-next'
import Select from '@/components/ui/Select.vue'
import ChartRenderer from '@/components/chart/ChartRenderer.vue'
import { globalToast } from '@/composables/useToast'
import { t } from '@/i18n'
import { useConnectionStore } from '@/stores/connection'
import { useDashboardStore, type DashboardCard } from '@/stores/dashboard'
import { numericColumnIndices, type AggFn, type ChartConfig, type ChartType } from '@/utils/chart'

interface ResultPayload {
  columns: { name: string; data_type: string }[]
  rows: unknown[][]
}

const props = defineProps<{
  /** 当前选中的库：新建仪表盘时绑定 */
  selectedDatabase: string
}>()

const connStore = useConnectionStore()
const dashStore = useDashboardStore()

const CHART_TYPES: ChartType[] = ['bar', 'line', 'pie', 'scatter']
const CHART_TYPE_ICONS: Record<ChartType, unknown> = {
  bar: BarChart3,
  line: LineChartIcon,
  pie: PieChartIcon,
  scatter: ScatterChartIcon,
}

const dashboardOptions = computed(() => dashStore.dashboards.map((d) => ({ label: d.name, value: d.id })))
const aggOptions = computed(() => (['raw', 'sum', 'avg', 'count', 'min', 'max'] as AggFn[]).map((fn) => ({ label: t(`dataTable.chartAgg${fn}`), value: fn })))

// 卡片运行状态：key 为 card id；用普通对象保证模板响应式与类型友好
const cardData = reactive<Record<string, ResultPayload>>({})
const cardError = reactive<Record<string, string>>({})
const cardLoading = reactive<Record<string, boolean>>({})

function hasData(id: string): boolean {
  return cardData[id] !== undefined
}

function dataOf(id: string): ResultPayload {
  return cardData[id] as ResultPayload
}

function errorOf(id: string): string {
  return cardError[id] ?? ''
}

function isLoading(id: string): boolean {
  return cardLoading[id] === true
}

/** 卡片 SQL 执行：仪表盘绑定连接 + 库 */
async function runCard(card: DashboardCard) {
  const dash = dashStore.current
  if (!dash) return
  const conn = connStore.activeConnections.find((c) => c.id === dash.connectionId)
  if (!conn) {
    cardError[card.id] = t('dashboard.needConnection')
    delete cardData[card.id]
    return
  }
  cardLoading[card.id] = true
  try {
    const result = await connStore.executeSql(dash.connectionId, dash.database, card.sql) as ResultPayload
    cardData[card.id] = { columns: result.columns, rows: result.rows }
    delete cardError[card.id]
  } catch (e) {
    delete cardData[card.id]
    cardError[card.id] = t('dashboard.cardError', { error: String(e) })
  } finally {
    cardLoading[card.id] = false
  }
}

async function runAllCards() {
  for (const card of dashStore.current?.cards ?? []) void runCard(card)
}

function refreshAll() {
  void runAllCards()
}

onMounted(() => {
  // 恢复最近使用的仪表盘；无则用当前连接新建
  if (!dashStore.current && dashStore.recent) dashStore.setCurrent(dashStore.recent.id)
  void runAllCards()
})

function createDashboard() {
  if (!connStore.currentConnection) {
    globalToast.warning(t('dashboard.needConnection'))
    return
  }
  if (!props.selectedDatabase) {
    globalToast.warning(t('dashboard.selectDbFirst'))
    return
  }
  const n = dashStore.dashboards.length + 1
  const dash = dashStore.create(`${t('dashboard.title')} ${n}`, connStore.currentConnection.id, props.selectedDatabase)
  globalToast.success(t('dashboard.newDashboardCreated', { name: dash.name }))
}

const removeArmed = ref(false)
let removeTimer: ReturnType<typeof setTimeout> | null = null

/** 删除需二次确认：第一次点击武装，3 秒内再点才删 */
function confirmRemove() {
  const dash = dashStore.current
  if (!dash) return
  if (!removeArmed.value) {
    removeArmed.value = true
    removeTimer = setTimeout(() => { removeArmed.value = false }, 3000)
    return
  }
  if (removeTimer) clearTimeout(removeTimer)
  removeArmed.value = false
  const name = dash.name
  dashStore.remove(dash.id)
  globalToast.success(t('dashboard.deleted'))
  void name
}

function removeCard(cardId: string) {
  if (dashStore.current) {
    dashStore.removeCard(dashStore.current.id, cardId)
    delete cardData[cardId]
    delete cardError[cardId]
    delete cardLoading[cardId]
  }
}

// ---- 添加卡片 ----
const editorOpen = ref(false)
const editorTitle = ref('')
const editorSql = ref('')
const editorError = ref('')
const previewData = ref<ResultPayload | null>(null)
const editorConfig = reactive<ChartConfig>({ type: 'bar', dimension: '', measures: [], agg: 'raw', group: '' })

const editorColumnOptions = computed(() => (previewData.value?.columns ?? []).map((c) => ({ label: c.name, value: c.name })))

function openCardEditor() {
  editorTitle.value = ''
  editorSql.value = ''
  editorError.value = ''
  previewData.value = null
  Object.assign(editorConfig, { type: 'bar', dimension: '', measures: [], agg: 'raw', group: '' })
  editorOpen.value = true
}

async function runPreview() {
  const dash = dashStore.current
  if (!dash || !editorSql.value.trim()) return
  const conn = connStore.activeConnections.find((c) => c.id === dash.connectionId)
  if (!conn) {
    editorError.value = t('dashboard.needConnection')
    previewData.value = null
    return
  }
  editorError.value = ''
  try {
    const result = await connStore.executeSql(dash.connectionId, dash.database, editorSql.value) as ResultPayload
    previewData.value = { columns: result.columns, rows: result.rows }
    // 自动推断默认映射：维度取第一个非数值列，度量取数值列
    const numericNames = numericColumnIndices(result.columns, result.rows).map((idx) => result.columns[idx]?.name ?? '')
    const nonNumeric = result.columns.find((c) => !numericNames.includes(c.name))
    editorConfig.dimension = nonNumeric?.name ?? result.columns[0]?.name ?? ''
    editorConfig.measures = numericNames.slice(0, 6)
  } catch (e) {
    previewData.value = null
    editorError.value = t('dashboard.cardError', { error: String(e) })
  }
}

function saveCard() {
  const dash = dashStore.current
  if (!dash || !previewData.value) return
  dashStore.addCard(dash.id, {
    title: editorTitle.value.trim() || editorConfig.dimension,
    sql: editorSql.value.trim(),
    config: { ...editorConfig },
  })
  editorOpen.value = false
  globalToast.success(t('dashboard.saved'))
  void runAllCards()
}

defineExpose({ runAllCards })
</script>

<style scoped>
.dashboard-view {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
}

.dashboard-toolbar {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--hairline);
}

.dash-name-input {
  width: 160px;
  padding: 4px var(--space-2);
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-primary);
  font-size: var(--text-sm);
}

.dash-name-input:hover,
.dash-name-input:focus {
  border-color: var(--hairline);
  background: var(--surface-subtle);
  outline: none;
}

.dash-spacer {
  flex: 1;
}

.dash-btn {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px var(--space-2);
  border: 1px solid var(--hairline);
  border-radius: var(--radius-sm);
  background: var(--panel);
  color: var(--text-secondary);
  font-size: var(--text-xs);
  cursor: pointer;
}

.dash-btn:hover:not(:disabled) {
  background: var(--surface-hover);
}

.dash-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.dash-btn.primary {
  color: var(--brand-600);
  border-color: var(--brand-600);
}

.dash-btn.danger {
  color: var(--danger, #e5484d);
}

.dashboard-empty {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-tertiary);
  font-size: var(--text-sm);
}

.dashboard-grid {
  flex: 1;
  min-height: 0;
  overflow: auto;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(360px, 1fr));
  gap: var(--space-3);
  padding: var(--space-3);
  align-content: start;
}

.dash-card {
  display: flex;
  flex-direction: column;
  height: 280px;
  border: 1px solid var(--hairline);
  border-radius: var(--radius-md);
  background: var(--raised);
  overflow: hidden;
}

.dash-card-head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--hairline);
}

.dash-card-title {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--text-sm);
  color: var(--text-primary);
}

.dash-card-error {
  font-size: var(--text-xs);
  color: var(--danger, #e5484d);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.dash-card-btn {
  display: flex;
  align-items: center;
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  padding: 2px;
  border-radius: var(--radius-sm);
}

.dash-card-btn:hover {
  color: var(--text-primary);
  background: var(--surface-hover);
}

.dash-card-body {
  flex: 1;
  min-height: 0;
}

.dash-card-empty {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-tertiary);
  font-size: var(--text-xs);
  padding: var(--space-2);
  overflow: hidden;
}

.spinning {
  animation: dash-spin 1s linear infinite;
}

@keyframes dash-spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

/* ---- 卡片编辑器 ---- */
.card-editor-mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.4);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
}

.card-editor {
  width: 720px;
  max-width: calc(100vw - 48px);
  height: 560px;
  max-height: calc(100vh - 48px);
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-3);
  background: var(--raised);
  border: 1px solid var(--hairline);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-lg, 0 8px 32px rgba(0, 0, 0, 0.25));
}

.card-editor-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.chart-field {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-xs);
  color: var(--text-secondary);
}

.chart-field :deep(select) {
  min-width: 100px;
  max-width: 160px;
}

.chart-types {
  display: flex;
  gap: 2px;
  padding: 2px;
  border: 1px solid var(--hairline);
  border-radius: var(--radius-md);
}

.chart-types button {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 22px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
}

.chart-types button.active {
  background: var(--surface-active);
  color: var(--brand-600);
}

.card-editor-title {
  flex: 1;
  padding: 6px var(--space-2);
  border: 1px solid var(--hairline);
  border-radius: var(--radius-sm);
  background: var(--panel);
  color: var(--text-primary);
  font-size: var(--text-sm);
}

.card-editor-sql {
  flex: 1;
  min-height: 90px;
  padding: var(--space-2);
  border: 1px solid var(--hairline);
  border-radius: var(--radius-sm);
  background: var(--panel);
  color: var(--text-primary);
  font-family: var(--font-mono, monospace);
  font-size: var(--text-xs);
  resize: vertical;
}

.card-editor-preview {
  flex: 1;
  min-height: 0;
  border: 1px solid var(--hairline);
  border-radius: var(--radius-sm);
  overflow: hidden;
}

.card-editor-foot {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
}
</style>
