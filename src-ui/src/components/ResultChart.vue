<template>
  <div class="result-chart">
    <div class="chart-config">
      <div class="chart-types">
        <button
          v-for="ct in CHART_TYPES"
          :key="ct"
          :class="{ active: config.type === ct }"
          :title="t(`dataTable.chartType${ct}`)"
          :aria-label="t(`dataTable.chartType${ct}`)"
          @click="config.type = ct"
        >
          <component :is="CHART_TYPE_ICONS[ct]" :size="14" />
        </button>
      </div>
      <label class="chart-field">
        <span>{{ t('dataTable.chartDimension') }}</span>
        <Select v-model="config.dimension" size="sm" :options="dimensionChoices" />
      </label>
      <label class="chart-field">
        <span>{{ t('dataTable.chartMeasure') }}</span>
        <Select
          multiple
          size="sm"
          :options="measureChoices"
          :multi-value="config.measures"
          :disabled="measureChoices.length === 0"
          @update:multi-value="config.measures = $event"
        />
      </label>
      <label class="chart-field">
        <span>{{ t('dataTable.chartGroup') }}</span>
        <Select v-model="config.group" size="sm" :options="groupChoices" />
      </label>
      <label class="chart-field">
        <span>{{ t('dataTable.chartAggregation') }}</span>
        <Select v-model="config.agg" size="sm" :options="aggChoices" />
      </label>
      <span v-if="truncated" class="chart-truncated">{{ truncatedText }}</span>
      <button
        v-if="canPushdown"
        class="chart-pushdown"
        :title="t('dataTable.chartPushdownTitle')"
        @click="emit('pushdown', pushdownConfig)"
      >{{ t('dataTable.chartPushdown') }}</button>
      <button
        v-if="canSave"
        class="chart-pushdown"
        :title="t('dataTable.chartSaveTitle')"
        @click="emit('save-dashboard', pushdownConfig)"
      >{{ t('dashboard.saveToDashboard') }}</button>
    </div>
    <div v-if="measureChoices.length === 0" class="chart-empty">{{ t('dataTable.chartNoNumeric') }}</div>
    <ChartRenderer v-else class="chart-canvas" :columns="columns" :rows="rows" :config="chartConfig" />
  </div>
</template>

<script setup lang="ts">
import { computed, reactive, watch } from 'vue'
import { BarChart3, LineChart as LineChartIcon, PieChart as PieChartIcon, ScatterChart as ScatterChartIcon } from 'lucide-vue-next'
import Select from '@/components/ui/Select.vue'
import ChartRenderer from '@/components/chart/ChartRenderer.vue'
import { t } from '@/i18n'
import {
  CHART_ROW_LIMIT,
  numericColumnIndices,
  type AggFn, type ChartConfig, type ChartType,
} from '@/utils/chart'

const props = defineProps<{
  columns: { name: string; data_type: string }[]
  rows: unknown[][]
}>()

const emit = defineEmits<{
  pushdown: [config: { dimension: string; measures: string[]; agg: string; group: string }]
  'save-dashboard': [config: { dimension: string; measures: string[]; agg: string; group: string }]
}>()

const CHART_TYPES: ChartType[] = ['bar', 'line', 'pie', 'scatter']
const CHART_TYPE_ICONS: Record<ChartType, unknown> = {
  bar: BarChart3,
  line: LineChartIcon,
  pie: PieChartIcon,
  scatter: ScatterChartIcon,
}
const AGG_FNS: AggFn[] = ['raw', 'sum', 'avg', 'count', 'min', 'max']

const config = reactive<ChartConfig>({ type: 'bar', dimension: '', measures: [], agg: 'raw', group: '' })

// 数值列索引用于默认选中度量；维度默认取第一个非数值列
const numericIdxs = computed(() => numericColumnIndices(props.columns, props.rows))

const dimensionChoices = computed(() => props.columns.map((c) => ({ label: c.name, value: c.name })))
const measureChoices = computed(() => numericIdxs.value.map((idx) => ({ label: props.columns[idx]?.name ?? '', value: props.columns[idx]?.name ?? '' })))
const groupChoices = computed(() => [
  { label: t('dataTable.chartGroupNone'), value: '' },
  ...props.columns.filter((c) => c.name !== config.dimension).map((c) => ({ label: c.name, value: c.name })),
])
const aggChoices = computed(() => AGG_FNS.map((fn) => ({ label: t(`dataTable.chartAgg${fn}`), value: fn })))

watch(() => props.columns, initDefaults, { immediate: true })

function initDefaults() {
  const numericNames = numericIdxs.value.map((idx) => props.columns[idx]?.name ?? '')
  const nonNumeric = props.columns.find((c) => !numericNames.includes(c.name))
  const defaultDim = (nonNumeric ?? props.columns[0])?.name ?? ''
  if (!props.columns.some((c) => c.name === config.dimension)) config.dimension = defaultDim
  if (!config.measures.some((name) => numericNames.includes(name))) {
    config.measures = numericNames.slice(0, 6)
  }
}

/** 截断提示：原始模式看行数，聚合模式看去重后的类别数 */
const truncated = computed(() => config.agg === 'raw' ? props.rows.length > CHART_ROW_LIMIT : distinctDimCount() > 200)
const truncatedText = computed(() =>
  config.agg === 'raw'
    ? t('dataTable.chartTruncated', { limit: CHART_ROW_LIMIT })
    : t('dataTable.chartCategoriesTruncated', { limit: 200 }),
)

/** 原始模式截断时：可下推数据库 / 存入仪表盘（卡片本身就是聚合查询） */
const canPushdown = computed(() => config.agg === 'raw' && props.rows.length > CHART_ROW_LIMIT && config.measures.length > 0)
const canSave = computed(() => props.rows.length > CHART_ROW_LIMIT && config.measures.length > 0)
const pushdownConfig = computed(() => ({
  dimension: config.dimension,
  measures: config.measures.slice(0, 1),
  agg: config.agg === 'raw' ? 'sum' : config.agg,
  group: config.group,
}))

const chartConfig = computed<ChartConfig>(() => ({ ...config }))

function distinctDimCount(): number {
  const dimIdx = props.columns.findIndex((c) => c.name === config.dimension)
  if (dimIdx < 0) return 0
  const seen = new Set<string>()
  for (const row of props.rows) {
    const v = (row as unknown[])[dimIdx]
    seen.add(v === null || v === undefined ? 'NULL' : String(v))
    if (seen.size > 200) break
  }
  return seen.size
}
</script>

<style scoped>
.result-chart {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
}

.chart-config {
  display: flex;
  align-items: center;
  gap: var(--space-4);
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--hairline);
  flex-wrap: wrap;
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

.chart-field {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-xs);
  color: var(--text-secondary);
}

.chart-field :deep(select) {
  min-width: 110px;
  max-width: 180px;
}

.chart-truncated {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.chart-pushdown {
  padding: 3px var(--space-2);
  border: 1px solid var(--hairline);
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--brand-600);
  font-size: var(--text-xs);
  cursor: pointer;
}

.chart-pushdown:hover {
  background: var(--surface-hover);
}

.chart-canvas {
  flex: 1;
  min-height: 0;
}

.chart-empty {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-tertiary);
  font-size: var(--text-sm);
}
</style>
