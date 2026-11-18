<template>
  <div class="result-chart">
    <div class="chart-config">
      <div class="chart-types">
        <button
          v-for="ct in CHART_TYPES"
          :key="ct"
          :class="{ active: chartType === ct }"
          :title="t(`dataTable.chartType${ct}`)"
          :aria-label="t(`dataTable.chartType${ct}`)"
          @click="chartType = ct"
        >
          <component :is="CHART_TYPE_ICONS[ct]" :size="14" />
        </button>
      </div>
      <label class="chart-field">
        <span>{{ t('dataTable.chartDimension') }}</span>
        <Select v-model="dimensionCol" size="sm" :options="dimensionChoices" />
      </label>
      <label class="chart-field">
        <span>{{ t('dataTable.chartMeasure') }}</span>
        <Select
          multiple
          size="sm"
          :options="measureChoices"
          :multi-value="measureCols"
          :disabled="measureChoices.length === 0"
          @update:multi-value="measureCols = $event"
        />
      </label>
      <label class="chart-field">
        <span>{{ t('dataTable.chartGroup') }}</span>
        <Select v-model="groupCol" size="sm" :options="groupChoices" />
      </label>
      <label class="chart-field">
        <span>{{ t('dataTable.chartAggregation') }}</span>
        <Select v-model="aggFn" size="sm" :options="aggChoices" />
      </label>
      <span v-if="truncated" class="chart-truncated">{{ truncatedText }}</span>
      <button
        v-if="canPushdown"
        class="chart-pushdown"
        :title="t('dataTable.chartPushdownTitle')"
        @click="emit('pushdown', pushdownConfig)"
      >{{ t('dataTable.chartPushdown') }}</button>
    </div>
    <div v-if="measureChoices.length === 0" class="chart-empty">{{ t('dataTable.chartNoNumeric') }}</div>
    <div v-else ref="chartEl" class="chart-canvas"></div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import * as echarts from 'echarts/core'
import { BarChart, LineChart, PieChart, ScatterChart } from 'echarts/charts'
import { GridComponent, TooltipComponent, LegendComponent } from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'
import { BarChart3, LineChart as LineChartIcon, PieChart as PieChartIcon, ScatterChart as ScatterChartIcon } from 'lucide-vue-next'
import Select from '@/components/ui/Select.vue'
import { t } from '@/i18n'
import { useThemeStore } from '@/stores/theme'

echarts.use([BarChart, LineChart, PieChart, ScatterChart, GridComponent, TooltipComponent, LegendComponent, CanvasRenderer])

const props = defineProps<{
  columns: { name: string; data_type: string }[]
  rows: unknown[][]
}>()

const emit = defineEmits<{
  pushdown: [config: { dimension: string; measures: string[]; agg: string; group: string }]
}>()

type ChartType = 'bar' | 'line' | 'pie' | 'scatter'
type AggFn = 'raw' | 'sum' | 'avg' | 'count' | 'min' | 'max'

const CHART_TYPES: ChartType[] = ['bar', 'line', 'pie', 'scatter']
const CHART_TYPE_ICONS: Record<ChartType, unknown> = {
  bar: BarChart3,
  line: LineChartIcon,
  pie: PieChartIcon,
  scatter: ScatterChartIcon,
}
const AGG_FNS: AggFn[] = ['raw', 'sum', 'avg', 'count', 'min', 'max']

/** 图表只取前 N 行：结果集可能上万行，全部渲染会卡死图表 */
const ROW_LIMIT = 2000
/** 分组系列上限：分组值过多时图例爆炸，只取前 N 个 */
const MAX_SERIES = 20
/** 聚合后的类别（X 轴去重值）上限 */
const MAX_CATEGORIES = 200

const chartType = ref<ChartType>('bar')
const aggFn = ref<AggFn>('raw')
const chartEl = ref<HTMLElement | null>(null)
let chart: echarts.ECharts | null = null
let resizeObserver: ResizeObserver | null = null

const themeStore = useThemeStore()

// 数值列推断：优先按类型名，类型缺失时抽样看值
const NUMERIC_TYPE_RE = /int|decimal|numeric|float|double|real|number|money|serial/i
const numericCols = computed(() => {
  const sample = props.rows.slice(0, 50)
  return props.columns
    .map((col, idx) => {
      const byType = NUMERIC_TYPE_RE.test(col.data_type ?? '')
      const byValue = sample.every((row) => {
        const v = (row as unknown[])[idx]
        return v === null || v === undefined || v === '' || (typeof v === 'number' && Number.isFinite(v)) || (typeof v === 'string' && v.trim() !== '' && Number.isFinite(Number(v)))
      })
      return { name: col.name, idx, numeric: byType || (sample.length > 0 && byValue) }
    })
    .filter((c) => c.numeric)
})

const dimensionCol = ref('')
const measureCols = ref<string[]>([])
const groupCol = ref('')

const dimensionChoices = computed(() => props.columns.map((c) => ({ label: c.name, value: c.name })))
const measureChoices = computed(() => numericCols.value.map((c) => ({ label: c.name, value: c.name })))
const groupChoices = computed(() => [
  { label: t('dataTable.chartGroupNone'), value: '' },
  ...props.columns.filter((c) => c.name !== dimensionCol.value).map((c) => ({ label: c.name, value: c.name })),
])
const aggChoices = computed(() => AGG_FNS.map((fn) => ({ label: t(`dataTable.chartAgg${fn}`), value: fn })))

watch(() => props.columns, initDefaults, { immediate: true })

function initDefaults() {
  const nonNumeric = props.columns.find((c, idx) => !numericCols.value.some((n) => n.idx === idx))
  const defaultDim = (nonNumeric ?? props.columns[0])?.name ?? ''
  if (!props.columns.some((c) => c.name === dimensionCol.value)) dimensionCol.value = defaultDim
  if (!measureCols.value.some((name) => numericCols.value.some((c) => c.name === name))) {
    measureCols.value = numericCols.value.slice(0, 6).map((c) => c.name)
  }
}

/** 截断提示：原始模式看行数，聚合模式看去重后的类别数 */
const truncated = computed(() => aggFn.value === 'raw' ? props.rows.length > ROW_LIMIT : categoryCount() > MAX_CATEGORIES)
const truncatedText = computed(() =>
  aggFn.value === 'raw'
    ? t('dataTable.chartTruncated', { limit: ROW_LIMIT })
    : t('dataTable.chartCategoriesTruncated', { limit: MAX_CATEGORIES }),
)

/** 原始模式截断时，提示改用聚合下推数据库 */
const canPushdown = computed(() => aggFn.value === 'raw' && props.rows.length > ROW_LIMIT && measureCols.value.length > 0)
const pushdownConfig = computed(() => ({
  dimension: dimensionCol.value,
  measures: measureCols.value.slice(0, 1),
  agg: aggFn.value === 'raw' ? 'sum' : aggFn.value,
  group: groupCol.value,
}))

watch(() => [props.rows, props.columns], render, { deep: false })
watch([chartType, aggFn, dimensionCol, measureCols, groupCol], render)
watch(() => themeStore.theme, render)

function colIdx(name: string): number {
  return props.columns.findIndex((c) => c.name === name)
}

/** 去重后的维度类别数（聚合模式用），用 Map 保持首次出现顺序 */
function categoryCount(): number {
  const dimIdx = colIdx(dimensionCol.value)
  if (dimIdx < 0) return 0
  const seen = new Set<string>()
  for (const row of props.rows) {
    seen.add(rowKey((row as unknown[])[dimIdx]))
    if (seen.size > MAX_CATEGORIES) break
  }
  return seen.size
}

function rowKey(v: unknown): string {
  return v === null || v === undefined ? 'NULL' : String(v)
}

/** 单元格转数值：NULL/空串/非数值返回 null，由 ECharts 断点 */
function toNumber(v: unknown): number | null {
  if (v === null || v === undefined || v === '') return null
  const n = typeof v === 'number' ? v : Number(v)
  return Number.isFinite(n) ? n : null
}

/** 对一组数值执行聚合；count 计非空个数 */
function aggregate(values: (number | null)[], fn: AggFn): number | null {
  const vals = values.filter((v): v is number => v !== null)
  if (fn === 'count') return vals.length
  if (vals.length === 0) return null
  switch (fn) {
    case 'sum': return vals.reduce((a, b) => a + b, 0)
    case 'avg': return vals.reduce((a, b) => a + b, 0) / vals.length
    case 'min': return Math.min(...vals)
    case 'max': return Math.max(...vals)
    default: return null
  }
}

interface SeriesData {
  name: string
  type: ChartType
  data: (number | null)[]
}

/** 构建聚合后的类别与系列（共享给普通/分组/饼图）；饼图传 useGroup=false 取维度构成 */
function buildAggregated(useGroup: boolean): { categories: string[]; series: SeriesData[] } {
  const dimIdx = colIdx(dimensionCol.value)
  const groupIdx = useGroup && groupCol.value ? colIdx(groupCol.value) : -1
  const measureIdxs = measureCols.value.map(colIdx).filter((i) => i >= 0)
  const rows = props.rows

  // 类别 = 维度去重（保持出现顺序），超上限截断
  const categories: string[] = []
  const catIndex = new Map<string, number>()
  const groups: string[] = []
  const groupIndex = new Map<string, number>()
  for (const row of rows) {
    const cat = rowKey((row as unknown[])[dimIdx])
    if (!catIndex.has(cat)) {
      if (categories.length >= MAX_CATEGORIES) continue
      catIndex.set(cat, categories.length)
      categories.push(cat)
    }
    if (groupIdx >= 0) {
      const g = rowKey((row as unknown[])[groupIdx])
      if (!groupIndex.has(g) && groups.length < MAX_SERIES) {
        groupIndex.set(g, groups.length)
        groups.push(g)
      }
    }
  }

  // 桶：group(或 '-') × cat × measureIdx → 数值列表
  const buckets = new Map<string, (number | null)[][]>()
  for (const row of rows) {
    const cat = rowKey((row as unknown[])[dimIdx])
    const ci = catIndex.get(cat)
    if (ci === undefined) continue
    const g = groupIdx >= 0 ? rowKey((row as unknown[])[groupIdx]) : '-'
    const gi = groupIndex.get(g)
    if (gi === undefined && groupIdx >= 0) continue // 超出系列上限的分组直接丢弃
    for (let mi = 0; mi < measureIdxs.length; mi++) {
      const mIdx = measureIdxs[mi]
      if (mIdx === undefined) continue
      const key = `${g}|${mi}`
      let bucket = buckets.get(key)
      if (!bucket) { bucket = Array.from({ length: categories.length }, () => [] as (number | null)[]); buckets.set(key, bucket) }
      const slot = bucket[ci]
      if (slot) slot.push(toNumber((row as unknown[])[mIdx]))
    }
  }

  if (groupIdx >= 0) {
    // 分组模式：X = 维度，系列 = 分组值，度量取第一个
    const series = groups.map((g) => {
      const bucket = buckets.get(`${g}|0`)
      return {
        name: g,
        type: chartType.value,
        data: categories.map((_, ci) => {
          const slot = bucket?.[ci]
          return slot ? aggregate(slot, aggFn.value === 'raw' ? 'sum' : aggFn.value) : null
        }),
      }
    })
    return { categories, series }
  }

  const series = measureCols.value.map((name, mi) => {
    const bucket = buckets.get(`-|${mi}`)
    return {
      name,
      type: chartType.value,
      data: categories.map((_, ci) => {
        const slot = bucket?.[ci]
        return slot ? aggregate(slot, aggFn.value === 'raw' ? 'sum' : aggFn.value) : null
      }),
    }
  })
  return { categories, series }
}

/** 原始值模式：每行一个点，不做去重聚合 */
function buildRaw(): { categories: string[]; series: SeriesData[] } {
  const dimIdx = colIdx(dimensionCol.value)
  const rows = props.rows.slice(0, ROW_LIMIT)
  const categories = rows.map((row) => rowKey((row as unknown[])[dimIdx]))
  const series = measureCols.value
    .map(colIdx)
    .filter((i) => i >= 0)
    .map((idx) => {
      const col = props.columns[idx]
      return {
        name: col?.name ?? '',
        type: chartType.value,
        data: rows.map((row) => toNumber((row as unknown[])[idx])),
      }
    })
  return { categories, series }
}

function chartColors() {
  const styles = getComputedStyle(document.documentElement)
  const read = (name: string, fallback: string) => styles.getPropertyValue(name).trim() || fallback
  return {
    text: read('--text-secondary', '#5B5F66'),
    split: read('--hairline', '#E8EAEF'),
    surface: read('--raised', '#FFFFFF'),
  }
}

const PALETTE = ['#4C9AFF', '#34D399', '#F59E0B', '#F87171', '#A78BFA', '#22D3EE', '#F472B6', '#84CC16']

function buildOption(): echarts.EChartsCoreOption | null {
  const dimIdx = colIdx(dimensionCol.value)
  if (dimIdx < 0 || measureCols.value.length === 0) return null
  const { text, split, surface } = chartColors()
  const aggregated = aggFn.value !== 'raw'
  const { categories, series } = aggregated
    ? buildAggregated(chartType.value !== 'pie')
    : buildRaw()

  const axisStyles = {
    axisLine: { lineStyle: { color: split } },
    axisLabel: { color: text, overflow: 'truncate' as const, width: 90 },
    splitLine: { lineStyle: { color: split } },
  }
  const legendStyles = { textStyle: { color: text }, pageTextStyle: { color: text } }
  const showLegend = series.length > 1

  if (chartType.value === 'pie') {
    // 饼图：单系列，取维度类别构成（分组在饼图下忽略）
    const first = series[0]
    if (!first) return null
    return {
      color: PALETTE,
      tooltip: { trigger: 'item', backgroundColor: surface, textStyle: { color: text } },
      legend: { type: 'scroll', ...legendStyles },
      series: [{
        type: 'pie',
        radius: ['28%', '62%'],
        center: ['50%', '52%'],
        avoidLabelOverlap: true,
        label: { color: text },
        itemStyle: { borderColor: surface, borderWidth: 1 },
        data: categories.map((name, i) => ({ name, value: first.data[i] ?? 0 })),
      }],
    }
  }

  return {
    color: PALETTE,
    tooltip: { trigger: 'axis', backgroundColor: surface, textStyle: { color: text } },
    legend: { show: showLegend, ...legendStyles },
    grid: { left: 8, right: 16, top: showLegend ? 28 : 16, bottom: 8, containLabel: true },
    xAxis: { type: 'category', data: categories, ...axisStyles },
    yAxis: { type: 'value', ...axisStyles },
    series: series.map((s) => ({
      name: s.name,
      type: s.type,
      data: s.data,
      itemStyle: { borderRadius: chartType.value === 'bar' ? 2 : 0 },
      showSymbol: chartType.value === 'line' ? s.data.length <= 100 : undefined,
    })),
  }
}

function render() {
  if (!chartEl.value || measureChoices.value.length === 0) return
  if (!chart) chart = echarts.init(chartEl.value)
  const option = buildOption()
  if (option) chart.setOption(option, { notMerge: true })
}

onMounted(() => {
  render()
  resizeObserver = new ResizeObserver(() => chart?.resize())
  if (chartEl.value) resizeObserver.observe(chartEl.value)
})

onUnmounted(() => {
  resizeObserver?.disconnect()
  resizeObserver = null
  chart?.dispose()
  chart = null
})
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
