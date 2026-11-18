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
        <Select v-model="measureCol" size="sm" :options="measureChoices" :disabled="measureChoices.length === 0" />
      </label>
      <span v-if="truncated" class="chart-truncated" :title="t('dataTable.chartTruncatedTitle', { limit: ROW_LIMIT })">
        {{ t('dataTable.chartTruncated', { limit: ROW_LIMIT }) }}
      </span>
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

type ChartType = 'bar' | 'line' | 'pie' | 'scatter'

const CHART_TYPES: ChartType[] = ['bar', 'line', 'pie', 'scatter']
const CHART_TYPE_ICONS: Record<ChartType, unknown> = {
  bar: BarChart3,
  line: LineChartIcon,
  pie: PieChartIcon,
  scatter: ScatterChartIcon,
}

/** 图表只取前 N 行：结果集可能上万行，全部渲染会卡死图表 */
const ROW_LIMIT = 2000

const chartType = ref<ChartType>('bar')
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
const measureCol = ref('')

// 维度默认取第一个非数值列；全数值时退化为第一列
const dimensionChoices = computed(() => props.columns.map((c) => ({ label: c.name, value: c.name })))
const measureChoices = computed(() => numericCols.value.map((c) => ({ label: c.name, value: c.name })))

watch(() => props.columns, initDefaults, { immediate: true })

function initDefaults() {
  const nonNumeric = props.columns.find((c, idx) => !numericCols.value.some((n) => n.idx === idx))
  const defaultDim = (nonNumeric ?? props.columns[0])?.name ?? ''
  if (!props.columns.some((c) => c.name === dimensionCol.value)) dimensionCol.value = defaultDim
  const defaultMeasure = numericCols.value[0]?.name ?? ''
  if (!numericCols.value.some((c) => c.name === measureCol.value)) measureCol.value = defaultMeasure
}

const truncated = computed(() => props.rows.length > ROW_LIMIT)

/** 数据变更时重新渲染；仅列变化时校正默认选中列 */
watch(() => [props.rows, props.columns], render, { deep: false })
watch([chartType, dimensionCol, measureCol], render)
watch(() => themeStore.theme, render)

function colIdx(name: string): number {
  return props.columns.findIndex((c) => c.name === name)
}

/** 单元格转数值：NULL/空串/非数值返回 null，由 ECharts 断点 */
function toNumber(v: unknown): number | null {
  if (v === null || v === undefined || v === '') return null
  const n = typeof v === 'number' ? v : Number(v)
  return Number.isFinite(n) ? n : null
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
  const measureIdx = colIdx(measureCol.value)
  if (dimIdx < 0 || measureIdx < 0) return null
  const { text, split, surface } = chartColors()

  const rows = props.rows.slice(0, ROW_LIMIT)
  const categories = rows.map((row) => {
    const v = (row as unknown[])[dimIdx]
    return v === null || v === undefined ? 'NULL' : String(v)
  })
  const values = rows.map((row) => toNumber((row as unknown[])[measureIdx]))
  const axisStyles = {
    axisLine: { lineStyle: { color: split } },
    axisLabel: { color: text, overflow: 'truncate' as const, width: 90 },
    splitLine: { lineStyle: { color: split } },
  }
  const legendStyles = { textStyle: { color: text }, pageTextStyle: { color: text } }

  if (chartType.value === 'pie') {
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
        data: categories.map((name, i) => ({ name, value: values[i] ?? 0 })),
      }],
    }
  }

  return {
    color: PALETTE,
    tooltip: { trigger: 'axis', backgroundColor: surface, textStyle: { color: text } },
    legend: { show: false, ...legendStyles },
    grid: { left: 8, right: 16, top: 16, bottom: 8, containLabel: true },
    xAxis: { type: 'category', data: categories, ...axisStyles },
    yAxis: { type: 'value', ...axisStyles },
    series: [{
      name: measureCol.value,
      type: chartType.value,
      data: values,
      itemStyle: { borderRadius: chartType.value === 'bar' ? 2 : 0 },
      showSymbol: chartType.value === 'line' ? rows.length <= 100 : undefined,
    }],
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
  min-width: 120px;
}

.chart-truncated {
  margin-left: auto;
  font-size: var(--text-xs);
  color: var(--text-tertiary);
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
