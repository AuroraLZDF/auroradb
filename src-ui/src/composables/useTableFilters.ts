import { ref, computed, type Ref } from 'vue'
import type { DataFilterCondition } from '@/stores/dataBrowser'
import { quoteIdentifier } from '@/utils/sql'
import { confirmAction } from '@/composables/useConfirm'
import type { DriverKind } from '@/utils/driver'
import { t } from '@/i18n'

export type FilterOperator = 'Eq' | 'Ne' | 'Gt' | 'Gte' | 'Lt' | 'Lte' | 'Like' | 'IsNull' | 'IsNotNull'

export interface FilterRow {
  id: number
  column: string
  operator: FilterOperator
  value: string
}

export interface SavedFilter {
  name: string
  logic: 'AND' | 'OR'
  rows: { column: string; operator: FilterOperator; value: string }[]
}

/** 列元数据最小契约（data_type 用于数值列的值类型推断） */
export interface FilterColumnMeta {
  name: string
  data_type: string
}

interface UseTableFiltersOptions {
  /** 当前表 tab（取 connectionId/database/tableName 作为已存过滤器的持久化 key） */
  currentTab: Ref<{ connectionId: string; database: string; tableName: string } | null>
  /** 当前表全部列（已按需加载完成） */
  allColumnsForTab: Ref<FilterColumnMeta[]>
  /** 当前驱动类型，决定 WHERE 预览的标识符引用风格 */
  driverKind: Ref<DriverKind>
  /** 翻页/加载依赖 */
  setPage: (page: number) => void
  loadData: () => Promise<void>
  hasUnsavedChanges: () => boolean
  showToast: (message: string, type?: 'success' | 'error') => void
}

/**
 * 数据浏览的过滤栏逻辑：多条件构建器 + WHERE 预览 + 已存过滤器（localStorage 持久化）。
 * 与 DataTable.vue 解耦，只通过 options 依赖外部状态。
 */
export function useTableFilters(options: UseTableFiltersOptions) {
  const showFilterPanel = ref(false)
  const filterRows = ref<FilterRow[]>([])
  const filterLogic = ref<'AND' | 'OR'>('AND')
  /** 已应用到请求的筛选（面板里可以继续编辑而不立即生效） */
  const activeFilters = ref<FilterRow[]>([])
  const activeFilterLogic = ref<'AND' | 'OR'>('AND')
  const savedFilters = ref<SavedFilter[]>([])
  const selectedSavedFilter = ref('')
  const newFilterName = ref('')
  let filterRowSeq = 0

  const filterOperatorOptions: { label: string; value: FilterOperator }[] = [
    { label: '=', value: 'Eq' },
    { label: '!=', value: 'Ne' },
    { label: '>', value: 'Gt' },
    { label: '>=', value: 'Gte' },
    { label: '<', value: 'Lt' },
    { label: '<=', value: 'Lte' },
    { label: 'LIKE（包含）', value: 'Like' },
    { label: 'IS NULL', value: 'IsNull' },
    { label: 'IS NOT NULL', value: 'IsNotNull' },
  ]

  const filterColumnOptions = computed(() => options.allColumnsForTab.value.map(col => col.name))

  /** 列选择器选项（CustomSelect 需要 { label, value } 结构，但支持纯字符串，这里保持字符串便于 placeholder） */
  const filterColumnChoices = computed(() => filterColumnOptions.value)

  /** 已保存筛选的选项 */
  const savedFilterChoices = computed(() =>
    savedFilters.value.map(item => ({ label: item.name, value: item.name })),
  )

  const savedFilterKey = computed(() =>
    options.currentTab.value
      ? `auroradb:savedFilters:${options.currentTab.value.connectionId}:${options.currentTab.value.database}:${options.currentTab.value.tableName}`
      : ''
  )

  /** 条件是否有效：IS NULL 类不需要值，其它必须填值 */
  function isUsableFilter(row: FilterRow): boolean {
    if (!row.column) return false
    if (row.operator === 'IsNull' || row.operator === 'IsNotNull') return true
    return row.value !== ''
  }

  function isNullOperator(operator: FilterOperator): boolean {
    return operator === 'IsNull' || operator === 'IsNotNull'
  }

  /** 数值列把输入转成数字，避免 PG 文本与整型比较报错 */
  function coerceFilterValue(row: FilterRow): unknown {
    if (isNullOperator(row.operator)) return null
    const type = options.allColumnsForTab.value.find(col => col.name === row.column)?.data_type?.toLowerCase() ?? ''
    const numeric = /int|decimal|numeric|float|double|real|year|bit/.test(type)
    if (numeric && row.value.trim() !== '' && !Number.isNaN(Number(row.value))) {
      return Number(row.value)
    }
    return row.value
  }

  function toFilterConditions(rows: FilterRow[]): DataFilterCondition[] {
    return rows.filter(isUsableFilter).map(row => ({
      column: row.column,
      operator: row.operator,
      value: coerceFilterValue(row),
    }))
  }

  function addFilterRow(column = '') {
    showFilterPanel.value = true
    filterRows.value = [
      ...filterRows.value,
      {
        id: ++filterRowSeq,
        column: column || filterColumnOptions.value[0] || '',
        operator: 'Eq',
        value: '',
      },
    ]
  }

  function removeFilterRow(id: number) {
    filterRows.value = filterRows.value.filter(row => row.id !== id)
  }

  /** 生成的 WHERE 预览（可直接复制到 SQL 编辑器） */
  const filterWherePreview = computed(() => {
    const rows = filterRows.value.filter(isUsableFilter)
    if (rows.length === 0) return ''
    const quote = (name: string) => quoteIdentifier(name, options.driverKind.value)
    const literal = (row: FilterRow): string => {
      const coerced = coerceFilterValue(row)
      if (coerced === null) return ''
      if (typeof coerced === 'number') return String(coerced)
      return `'${String(coerced).replace(/'/g, "''")}'`
    }
    const parts = rows.map(row => {
      const column = quote(row.column)
      switch (row.operator) {
        case 'Eq': return `${column} = ${literal(row)}`
        case 'Ne': return `${column} != ${literal(row)}`
        case 'Gt': return `${column} > ${literal(row)}`
        case 'Gte': return `${column} >= ${literal(row)}`
        case 'Lt': return `${column} < ${literal(row)}`
        case 'Lte': return `${column} <= ${literal(row)}`
        // LIKE 是「包含」语义：不为空且没写通配符时自动补 `%`，与后端 like_pattern 保持一致
        case 'Like': {
          const raw = String(coerceFilterValue(row) ?? '')
          const pattern = raw.includes('%') || raw.includes('_') ? raw : `%${raw}%`
          return `${column} LIKE '${pattern.replace(/'/g, "''")}'`
        }
        case 'IsNull': return `${column} IS NULL`
        case 'IsNotNull': return `${column} IS NOT NULL`
      }
    })
    return `WHERE ${parts.join(` ${filterLogic.value} `)}`
  })

  async function copyWherePreview() {
    try {
      await navigator.clipboard.writeText(filterWherePreview.value)
      options.showToast(t('dataTable.filterCopied'))
    } catch {
      options.showToast(t('dataTable.filterCopied'), 'error')
    }
  }

  /** 应用筛选：会丢弃未保存的修改，因此先确认 */
  async function applyFilters() {
    if (options.hasUnsavedChanges()) {
      const confirmed = await confirmAction({
        message: t('dataTable.filterDiscardConfirm'),
        confirmText: t('common.confirm'),
      })
      if (!confirmed) return
    }
    activeFilters.value = filterRows.value.filter(isUsableFilter).map(row => ({ ...row }))
    activeFilterLogic.value = filterLogic.value
    options.setPage(1)
    await options.loadData()
  }

  async function clearFilters() {
    filterRows.value = []
    activeFilters.value = []
    activeFilterLogic.value = 'AND'
    filterLogic.value = 'AND'
    selectedSavedFilter.value = ''
    options.setPage(1)
    await options.loadData()
  }

  function loadSavedFilters() {
    if (!savedFilterKey.value) {
      savedFilters.value = []
      return
    }
    try {
      const raw = localStorage.getItem(savedFilterKey.value)
      savedFilters.value = raw ? (JSON.parse(raw) as SavedFilter[]) : []
    } catch {
      savedFilters.value = []
    }
  }

  function persistSavedFilters() {
    if (!savedFilterKey.value) return
    localStorage.setItem(savedFilterKey.value, JSON.stringify(savedFilters.value))
  }

  function saveCurrentFilter() {
    const name = newFilterName.value.trim()
    if (!name) return
    const rows = filterRows.value.filter(isUsableFilter).map(row => ({
      column: row.column,
      operator: row.operator,
      value: row.value,
    }))
    const next: SavedFilter = { name, logic: filterLogic.value, rows }
    const index = savedFilters.value.findIndex(item => item.name === name)
    if (index >= 0) savedFilters.value[index] = next
    else savedFilters.value.push(next)
    persistSavedFilters()
    selectedSavedFilter.value = name
    newFilterName.value = ''
  }

  async function applySavedFilter(name: string) {
    const saved = savedFilters.value.find(item => item.name === name)
    if (!saved) return
    filterLogic.value = saved.logic
    filterRows.value = saved.rows.map(row => ({ ...row, id: ++filterRowSeq }))
    await applyFilters()
  }

  function deleteSavedFilter() {
    const name = selectedSavedFilter.value
    if (!name) return
    savedFilters.value = savedFilters.value.filter(item => item.name !== name)
    persistSavedFilters()
    selectedSavedFilter.value = ''
  }

  /** 切换表时复位全部过滤状态（对应 DataTable 的 switchTab 逻辑） */
  function resetFilters() {
    filterRows.value = []
    activeFilters.value = []
    filterLogic.value = 'AND'
    activeFilterLogic.value = 'AND'
    selectedSavedFilter.value = ''
    newFilterName.value = ''
    loadSavedFilters()
  }

  return {
    showFilterPanel,
    filterRows,
    filterLogic,
    activeFilters,
    activeFilterLogic,
    selectedSavedFilter,
    newFilterName,
    filterOperatorOptions,
    filterColumnOptions,
    filterColumnChoices,
    savedFilterChoices,
    savedFilterKey,
    isUsableFilter,
    isNullOperator,
    coerceFilterValue,
    toFilterConditions,
    addFilterRow,
    removeFilterRow,
    filterWherePreview,
    copyWherePreview,
    applyFilters,
    clearFilters,
    loadSavedFilters,
    persistSavedFilters,
    saveCurrentFilter,
    applySavedFilter,
    deleteSavedFilter,
    resetFilters,
  }
}
