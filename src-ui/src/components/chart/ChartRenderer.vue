<template>
  <div ref="chartEl" class="chart-renderer"></div>
</template>

<script setup lang="ts">
/**
 * 纯图表渲染层（V3-1.3）：输入列/行/字段映射，输出 ECharts 画布。
 * ResultChart（结果集图表）与 DashboardView（仪表盘卡片）共用。
 */
import { onMounted, onUnmounted, ref, watch } from 'vue'
import * as echarts from 'echarts/core'
import { BarChart, LineChart, PieChart, ScatterChart } from 'echarts/charts'
import { GridComponent, TooltipComponent, LegendComponent } from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'
import { useThemeStore } from '@/stores/theme'
import {
  CHART_MAX_CATEGORIES, CHART_MAX_SERIES, CHART_PALETTE, CHART_ROW_LIMIT,
  aggregate, chartThemeColors, rowKey, toNumber,
  type ChartConfig,
} from '@/utils/chart'

echarts.use([BarChart, LineChart, PieChart, ScatterChart, GridComponent, TooltipComponent, LegendComponent, CanvasRenderer])

const props = defineProps<{
  columns: { name: string; data_type?: string }[]
  rows: unknown[][]
  config: ChartConfig
}>()

const chartEl = ref<HTMLElement | null>(null)
let chart: echarts.ECharts | null = null
let resizeObserver: ResizeObserver | null = null

const themeStore = useThemeStore()

function colIdx(name: string): number {
  return props.columns.findIndex((c) => c.name === name)
}

interface SeriesData {
  name: string
  data: (number | null)[]
}

/** 原始值模式：每行一个点，不做去重聚合 */
function buildRaw(): { categories: string[]; series: SeriesData[] } {
  const dimIdx = colIdx(props.config.dimension)
  const rows = props.rows.slice(0, CHART_ROW_LIMIT)
  const categories = rows.map((row) => rowKey((row as unknown[])[dimIdx]))
  const series = props.config.measures
    .map(colIdx)
    .filter((i) => i >= 0)
    .map((idx) => ({
      name: props.columns[idx]?.name ?? '',
      data: rows.map((row) => toNumber((row as unknown[])[idx])),
    }))
  return { categories, series }
}

/** 构建聚合后的类别与系列；饼图传 useGroup=false 取维度构成 */
function buildAggregated(useGroup: boolean): { categories: string[]; series: SeriesData[] } {
  const { dimension, measures, agg, group } = props.config
  const dimIdx = colIdx(dimension)
  const groupIdx = useGroup && group ? colIdx(group) : -1
  const measureIdxs = measures.map(colIdx).filter((i) => i >= 0)
  const aggFn = agg === 'raw' ? 'sum' : agg
  const rows = props.rows

  // 类别 = 维度去重（保持出现顺序），超上限截断
  const categories: string[] = []
  const catIndex = new Map<string, number>()
  const groups: string[] = []
  const groupIndex = new Map<string, number>()
  for (const row of rows) {
    const cat = rowKey((row as unknown[])[dimIdx])
    if (!catIndex.has(cat)) {
      if (categories.length >= CHART_MAX_CATEGORIES) continue
      catIndex.set(cat, categories.length)
      categories.push(cat)
    }
    if (groupIdx >= 0) {
      const g = rowKey((row as unknown[])[groupIdx])
      if (!groupIndex.has(g) && groups.length < CHART_MAX_SERIES) {
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
    if (groupIdx >= 0 && !groupIndex.has(g)) continue // 超出系列上限的分组直接丢弃
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

  const seriesOf = (bucket: (number | null)[][] | undefined, name: string): SeriesData => ({
    name,
    data: categories.map((_, ci) => {
      const slot = bucket?.[ci]
      return slot ? aggregate(slot, aggFn) : null
    }),
  })

  if (groupIdx >= 0) {
    // 分组模式：X = 维度，系列 = 分组值，度量取第一个
    return { categories, series: groups.map((g) => seriesOf(buckets.get(`${g}|0`), g)) }
  }
  return { categories, series: measures.map((name, mi) => seriesOf(buckets.get(`-|${mi}`), name)) }
}

function buildOption(): echarts.EChartsCoreOption | null {
  const cfg = props.config
  const dimIdx = colIdx(cfg.dimension)
  if (dimIdx < 0 || cfg.measures.length === 0) return null
  const { text, split, surface } = chartThemeColors()
  const { categories, series } = cfg.agg === 'raw' ? buildRaw() : buildAggregated(cfg.type !== 'pie')

  const axisStyles = {
    axisLine: { lineStyle: { color: split } },
    axisLabel: { color: text, overflow: 'truncate' as const, width: 90 },
    splitLine: { lineStyle: { color: split } },
  }
  const legendStyles = { textStyle: { color: text }, pageTextStyle: { color: text } }
  const showLegend = series.length > 1

  if (cfg.type === 'pie') {
    const first = series[0]
    if (!first) return null
    return {
      color: CHART_PALETTE,
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
    color: CHART_PALETTE,
    tooltip: { trigger: 'axis', backgroundColor: surface, textStyle: { color: text } },
    legend: { show: showLegend, ...legendStyles },
    grid: { left: 8, right: 16, top: showLegend ? 28 : 16, bottom: 8, containLabel: true },
    xAxis: { type: 'category', data: categories, ...axisStyles },
    yAxis: { type: 'value', ...axisStyles },
    series: series.map((s) => ({
      name: s.name,
      type: cfg.type,
      data: s.data,
      itemStyle: { borderRadius: cfg.type === 'bar' ? 2 : 0 },
      showSymbol: cfg.type === 'line' ? s.data.length <= 100 : undefined,
    })),
  }
}

function render() {
  if (!chartEl.value) return
  if (!chart) chart = echarts.init(chartEl.value)
  const option = buildOption()
  if (option) chart.setOption(option, { notMerge: true })
}

watch(() => [props.rows, props.config], render, { deep: true })
watch(() => themeStore.theme, render)

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
.chart-renderer {
  width: 100%;
  height: 100%;
  min-height: 0;
}
</style>
