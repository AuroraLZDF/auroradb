/**
 * 图表共享类型与推断/聚合工具（V3-1.1~V3-1.3）。
 * ResultChart（结果集图表）与 ChartRenderer（渲染层）共用。
 */

export type ChartType = 'bar' | 'line' | 'pie' | 'scatter'
export type AggFn = 'raw' | 'sum' | 'avg' | 'count' | 'min' | 'max'

/** 一张图表的字段映射配置 */
export interface ChartConfig {
  type: ChartType
  dimension: string
  measures: string[]
  agg: AggFn
  /** 分组列，空串表示不分组 */
  group: string
}

/** 图表只取前 N 行原始值：结果集可能上万行，全部渲染会卡死图表 */
export const CHART_ROW_LIMIT = 2000
/** 分组系列上限：分组值过多时图例爆炸，只取前 N 个 */
export const CHART_MAX_SERIES = 20
/** 聚合后的类别（X 轴去重值）上限 */
export const CHART_MAX_CATEGORIES = 200

export const CHART_PALETTE = ['#1B6AF0', '#34D399', '#F59E0B', '#F87171', '#A78BFA', '#22D3EE', '#F472B6', '#84CC16']

const NUMERIC_TYPE_RE = /int|decimal|numeric|float|double|real|number|money|serial/i

/** 数值列推断：优先按类型名，类型缺失时抽样看值 */
export function isNumericColumn(col: { name: string; data_type?: string }, sampleRows: unknown[][], colIdx: number): boolean {
  if (NUMERIC_TYPE_RE.test(col.data_type ?? '')) return true
  if (sampleRows.length === 0) return false
  return sampleRows.every((row) => {
    const v = (row as unknown[])[colIdx]
    return v === null || v === undefined || v === '' || (typeof v === 'number' && Number.isFinite(v)) || (typeof v === 'string' && v.trim() !== '' && Number.isFinite(Number(v)))
  })
}

/** 找出结果集中的数值列索引 */
export function numericColumnIndices(columns: { name: string; data_type?: string }[], rows: unknown[][]): number[] {
  const sample = rows.slice(0, 50)
  return columns.map((col, idx) => ({ col, idx })).filter(({ col, idx }) => isNumericColumn(col, sample, idx)).map(({ idx }) => idx)
}

/** 单元格转数值：NULL/空串/非数值返回 null，由 ECharts 断点 */
export function toNumber(v: unknown): number | null {
  if (v === null || v === undefined || v === '') return null
  const n = typeof v === 'number' ? v : Number(v)
  return Number.isFinite(n) ? n : null
}

/** 维度取值转类别名，NULL/undefined 统一显示为 NULL */
export function rowKey(v: unknown): string {
  return v === null || v === undefined ? 'NULL' : String(v)
}

/** 对一组数值执行聚合；count 计非空个数 */
export function aggregate(values: (number | null)[], fn: AggFn): number | null {
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

/** 图表主题色：从 CSS 变量读取，跟随主题切换 */
export function chartThemeColors() {
  const styles = getComputedStyle(document.documentElement)
  const read = (name: string, fallback: string) => styles.getPropertyValue(name).trim() || fallback
  return {
    text: read('--text-secondary', '#5B5F66'),
    split: read('--hairline', '#E8EAEF'),
    surface: read('--raised', '#FFFFFF'),
  }
}
