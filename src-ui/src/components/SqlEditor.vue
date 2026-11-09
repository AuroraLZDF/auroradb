<template>
  <div class="sql-editor" @click="closeResultCellMenu">
    <!-- 工具栏 -->
    <div class="toolbar">
      <div class="toolbar-left">
        <button v-if="!showCancel" @click="executeQuery" :disabled="loading || !sql.trim()" class="btn-execute">
          <Play :size="14" />
          {{ t('sqlEditor.execute') }}
        </button>
        <button v-else @click="cancelQuery" class="btn-cancel">
          <Square :size="14" />
          {{ t('common.cancel') }}
        </button>
        <button @click="explainSql" :disabled="explaining || !sql.trim()" class="btn-explain">
          <Cpu :size="14" />
          {{ t('sqlEditor.explain') }}
        </button>
        <button @click="handleFormatSql" :disabled="!sql.trim()" class="btn-format">
          <FileText :size="14" />
          {{ t('sqlEditor.format') }}
        </button>
        <button @click="clearEditor" :disabled="!sql.trim()" class="btn-clear">
          <Trash2 :size="14" />
          {{ t('common.clear') }}
        </button>
        <button
          @click="panelOpen = !panelOpen"
          class="btn-format"
          :class="{ 'is-active': panelOpen }"
        >
          <History :size="14" />
          {{ t('sqlEditor.queryLibrary') }}
        </button>
      </div>
      <div class="toolbar-right">
        <div class="database-select">
          <Select
            :model-value="selectedDatabase"
            :options="databaseOptions"
            :disabled="isPostgres"
            :placeholder="t('sqlEditor.selectDatabase')"
            @update:model-value="manualSelectedDatabase = $event"
          />
        </div>
      </div>
    </div>

    <!-- 编辑器区域 - 可调节高度 -->
    <div class="editor-section" :style="{ height: editorHeight + 'px' }">
      <div class="editor-container">
        <!-- CodeMirror 6 编辑器：语法高亮 / 补全 / 折叠 / 查找替换 / 多光标 -->
        <div ref="editorHostRef" class="cm-host"></div>
      </div>
      <!-- 拖拽分隔条 -->
      <div
        class="resize-handle"
        @mousedown="startResize"
        :title="t('sqlEditor.resizeHint')"
      ></div>
    </div>

    <!-- 多语句结果分栏：每条语句一个标签，点击切换查看各自结果 -->
    <div v-if="statementOutcomes.length > 1" class="statement-tabs">
      <button
        v-for="(outcome, index) in statementOutcomes"
        :key="index"
        :class="[
          'statement-tab',
          { active: index === activeStatementIndex, 'has-error': !!outcome.error || !!outcome.cancelled },
        ]"
        :title="outcome.sql"
        @click="selectStatement(index)"
      >
        <span class="statement-no">{{ t('sqlEditor.statementTab', { index: index + 1 }) }}</span>
        <span class="statement-sql">{{ truncateSql(outcome.sql.replace(/\s+/g, ' ').trim()) }}</span>
      </button>
    </div>

    <!-- 执行结果 -->
    <div v-if="error" class="result-panel error">
      <div class="result-header">
        <XCircle class="error-icon" :size="16" />
        <span>{{ t('sqlEditor.executeFailed') }}</span>
      </div>
      <div class="error-message">{{ error }}</div>
    </div>

    <div v-else-if="cancelled" class="result-panel cancelled">
      <div class="result-header">
        <Square class="cancelled-icon" :size="16" />
        <span>{{ t('sqlEditor.queryCancelled') }}</span>
      </div>
      <div class="no-results">{{ t('sqlEditor.queryCancelledDetail') }}</div>
    </div>

    <div v-else-if="loading" class="result-panel loading">
      <div class="result-header">
        <Loader2 class="loading-icon" :size="16" />
        <span>{{ t('sqlEditor.executing') }}</span>
      </div>
      <div class="no-results">{{ t('sqlEditor.executingDetail') }}</div>
    </div>

    <div v-else-if="explaining" class="result-panel loading">
      <div class="result-header">
        <Loader2 class="loading-icon" :size="16" />
        <span>{{ t('sqlEditor.explaining') }}</span>
      </div>
      <div class="no-results">{{ t('sqlEditor.explainingDetail') }}</div>
    </div>

    <div v-else-if="resultView === 'explain'" class="result-panel">
      <component :is="ProUi.ExplainTree" v-if="ProUi.ExplainTree" :plan="explainResult" />
    </div>

    <div v-else-if="result" class="result-panel">
      <div class="result-header">
        <CheckCircle class="success-icon" :size="16" />
        <span>{{ t('sqlEditor.executeSuccess') }}</span>
        <span class="execution-info">
          {{ result.rows_affected != null ? t('sqlEditor.rowsAffected', { count: result.rows_affected }) : t('sqlEditor.rowsReturned', { count: result.rows_returned }) }}
          · {{ t('sqlEditor.executionTime', { ms: result.execution_time_ms }) }}
        </span>
        <button @click="exportResults" :disabled="!result.rows || result.rows.length === 0" class="btn-export">
          <Download :size="14" />
          {{ t('common.export') }}
        </button>
        <div class="export-format-select">
          <Select
            :model-value="exportFormat"
            :options="exportFormatOptions"
            size="sm"
            @update:model-value="onExportFormatChange"
          />
        </div>
      </div>

      <!-- 结果集截断提示 -->
      <div v-if="result.warning" class="result-warning">
        {{ result.warning }}
      </div>

      <!-- 查询结果表格 -->
      <div
        v-if="result.rows && result.rows.length > 0"
        ref="resultContainer"
        class="result-table-container"
        @scroll="handleResultScroll"
      >
        <table class="result-table">
          <thead>
            <tr>
              <th 
                v-for="(col, index) in result.columns" 
                :key="col.name" 
                :title="col.data_type"
                :style="{ width: columnWidths[index] + 'px', minWidth: columnWidths[index] + 'px' }"
              >
                <div class="th-content">
                  <span class="col-name">{{ col.name }}</span>
                  <span class="data-type">{{ col.data_type }}</span>
                </div>
              </th>
            </tr>
          </thead>
          <tbody>
            <tr
              v-if="resultVirtualEnabled && virtualPaddingTop > 0"
              class="virtual-spacer"
              aria-hidden="true"
            >
              <td
                :colspan="result.columns.length"
                :style="{ height: virtualPaddingTop + 'px', padding: '0', border: 'none' }"
              ></td>
            </tr>
            <tr v-for="{ row, index } in visibleResultRows" :key="index">
              <td
                v-for="(cell, cellIndex) in row"
                :key="cellIndex"
                :class="{ 'is-null': cell === null }"
                :style="{ width: columnWidths[cellIndex] + 'px', minWidth: columnWidths[cellIndex] + 'px' }"
                :title="formatCell(cell)"
                @dblclick="openResultCellViewer(cell, cellIndex)"
                @contextmenu.prevent="showResultCellMenu($event, cell, cellIndex)"
              >
                <span class="cell-content">{{ formatCell(cell) }}</span>
              </td>
            </tr>
            <tr
              v-if="resultVirtualEnabled && virtualPaddingBottom > 0"
              class="virtual-spacer"
              aria-hidden="true"
            >
              <td
                :colspan="result.columns.length"
                :style="{ height: virtualPaddingBottom + 'px', padding: '0', border: 'none' }"
              ></td>
            </tr>
          </tbody>
        </table>
      </div>

      <div v-else-if="result.rows_affected != null" class="no-results">
        {{ t('sqlEditor.statementSuccess', { count: result.rows_affected }) }}
      </div>

      <div v-else class="no-results">
        {{ t('sqlEditor.querySuccessEmpty') }}
      </div>
    </div>

    <!-- 查询历史 / 已保存查询 -->
    <div v-if="panelOpen" class="library-panel">
      <div class="library-header">
        <div class="library-tabs">
          <button
            :class="['library-tab', { active: panelTab === 'history' }]"
            @click="panelTab = 'history'"
          >
            {{ t('sqlEditor.historyTab', { count: currentHistory.length }) }}
          </button>
          <button
            :class="['library-tab', { active: panelTab === 'saved' }]"
            @click="panelTab = 'saved'"
          >
            {{ t('sqlEditor.savedTab', { count: queryLibrary.savedQueries.length }) }}
          </button>
        </div>
        <input v-model="librarySearch" class="library-search" :placeholder="t('sqlEditor.searchLibrary')" />
        <button class="btn-clear-history" :title="t('common.close')" @click="panelOpen = false" :aria-label="t('common.close')">
          <X :size="14" />
        </button>
      </div>

      <div v-if="panelTab === 'history'" class="history-list">
        <div v-if="filteredHistory.length === 0" class="library-empty">
          {{ t('sqlEditor.noHistory') }}
        </div>
        <div
          v-for="item in filteredHistory"
          :key="item.id"
          class="history-item"
          :title="item.sql"
          @click="loadHistoryItem(item)"
        >
          <span class="history-sql">{{ truncateSql(item.sql) }}</span>
          <span v-if="!item.success" class="history-failed">{{ t('common.failed') }}</span>
          <span class="history-time">{{ formatTime(item.executedAt) }}</span>
          <button class="btn-item-delete" :title="t('common.delete')" @click.stop="queryLibrary.removeHistory(item.id)" :aria-label="t('common.delete')">
            ×
          </button>
        </div>
        <button
          v-if="currentHistory.length > 0"
          class="btn-clear-history block"
          @click="queryLibrary.clearHistory(effectiveConnectionId)"
        >
          {{ t('sqlEditor.clearConnectionHistory') }}
        </button>
      </div>

      <div v-else class="history-list">
        <div v-if="savedGroups.length === 0" class="library-empty">
          {{ t('sqlEditor.noSaved') }}
        </div>
        <template v-for="group in savedGroups" :key="group.folder">
          <div class="library-folder">{{ group.folder }}</div>
          <div
            v-for="item in group.items"
            :key="item.id"
            class="history-item"
            :title="item.sql"
            @click="loadSavedQuery(item)"
          >
            <span class="history-sql">{{ item.name }}</span>
            <span class="history-time">{{ formatTime(item.updatedAt) }}</span>
            <button class="btn-item-delete" :title="t('common.delete')" @click.stop="queryLibrary.deleteSavedQuery(item.id)" :aria-label="t('common.delete')">
              ×
            </button>
          </div>
        </template>
      </div>
    </div>

    <!-- 查询结果单元格右键菜单 -->
    <div
      v-if="resultCellMenu.show"
      class="context-menu"
      :style="{ top: resultCellMenu.y + 'px', left: resultCellMenu.x + 'px' }"
      @click.stop
    >
      <div class="menu-item" @click="copyResultCell">{{ t('common.copy') }}</div>
      <div class="menu-item" @click="viewResultCell">{{ t('dataTable.viewFullContent') }}</div>
    </div>

    <!-- 参数化查询：收集 `:name` 实参 -->
    <div v-if="paramDialog.show" class="param-mask" @click.self="cancelSqlParams">
      <div class="param-dialog">
        <div class="param-header">
          <span>{{ t('sqlEditor.paramsTitle') }}</span>
          <button class="param-close" @click="cancelSqlParams" aria-label="关闭">×</button>
        </div>
        <p class="param-hint">{{ t('sqlEditor.paramsHint') }}</p>
        <div class="param-list">
          <label v-for="name in paramDialog.names" :key="name" class="param-row">
            <span class="param-name">:{{ name }}</span>
            <input
              v-model="paramDialog.values[name]"
              :placeholder="t('sqlEditor.paramsPlaceholder')"
              @keydown.enter="confirmSqlParams"
            />
          </label>
        </div>
        <div class="param-actions">
          <button @click="cancelSqlParams">{{ t('common.cancel') }}</button>
          <button class="param-primary" @click="confirmSqlParams">{{ t('sqlEditor.paramsExecute') }}</button>
        </div>
      </div>
    </div>

    <!-- 单元格查看器（大文本 / JSON / BLOB） -->
    <CellViewer
      :open="cellViewer.show"
      :value="cellViewer.value"
      :column-name="cellViewer.columnName"
      :column-type="cellViewer.columnType"
      :driver="isPostgres ? 'postgres' : 'mysql'"
      @close="cellViewer.show = false"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, nextTick, watch } from 'vue'
import { useConnectionStore } from '@/stores/connection'
import { resolveTabConnectionId } from '@/utils/tabConnection'
import { useDataBrowserStore } from '@/stores/dataBrowser'
import { useEditorTabsStore } from '@/stores/editorTabs'
import { useQueryLibraryStore, type QueryHistoryEntry, type SavedQuery } from '@/stores/queryLibrary'
import { Play, FileText, Trash2, Download, XCircle, CheckCircle, Loader2, Square, History, X, Cpu } from 'lucide-vue-next'
import { t } from '@/i18n'
import { findDangerousStatements, formatSql, quoteIdentifier } from '@/utils/sql'
import { formatSqlValue } from '@/utils/tableChanges'
import { invoke } from '@tauri-apps/api/core'
import { useVirtualRows } from '@/composables/useVirtualRows'
import { globalToast } from '@/composables/useToast'
import { confirmDanger } from '@/composables/useConfirm'
import {
  useSqlCompletion,
  invalidateSchemaCache,
} from '@/composables/useSqlCompletion'
import { createSqlEditor, type SqlEditorApi } from '@/composables/useCodeMirror'
import type { ExplainPlan } from '@/utils/explain'
import { ProUi } from '@/utils/pro'
import CellViewer from '@/components/CellViewer.vue'
import Select from '@/components/ui/Select.vue'

const connStore = useConnectionStore()
const browserStore = useDataBrowserStore()
const editorTabs = useEditorTabsStore()
const queryLibrary = useQueryLibraryStore()
const completionEngine = useSqlCompletion()

// Props
const props = defineProps<{
  tabId?: string
  /** 本 Tab 绑定的连接 id；为空时回退全局当前连接 */
  connectionId?: string
}>()

// 确保 store 中存在这个 tab 的内容记录
if (props.tabId) editorTabs.ensure(props.tabId)

// CodeMirror 6 编辑器实例（按 tabId 作 key 挂载，切 Tab 时重建）
const editorHostRef = ref<HTMLElement>()
let editorApi: SqlEditorApi | null = null

// SQL 文本走 editorTabs store，组件只做读写代理
const sql = computed({
  get: () => props.tabId ? editorTabs.getSql(props.tabId) : '',
  set: (v: string) => {
    if (props.tabId) editorTabs.setSql(props.tabId, v)
  },
})

const loading = ref(false)
// 查询执行超过一定时间才显示「取消」，避免快速查询时按钮闪现
const showCancel = ref(false)
let cancelTimer: ReturnType<typeof setTimeout> | undefined
const error = ref('')
const cancelled = ref(false)
interface QueryResultPayload {
  columns: { name: string; data_type: string }[]
  rows: unknown[][]
  execution_time_ms: number
  rows_affected?: number
  rows_returned: number
  truncated?: boolean
  warning?: string
}
const result = ref<QueryResultPayload | null>(null)
// 多语句执行：每条语句一份结果，按语句分栏展示
interface StatementOutcome {
  sql: string
  result: QueryResultPayload | null
  error: string
  cancelled?: boolean
}
const statementOutcomes = ref<StatementOutcome[]>([])
const activeStatementIndex = ref(0)

// 切换到某条语句的结果分栏，并把该条结果同步到缓存（切换 Tab 后仍能还原）
function selectStatement(index: number) {
  const outcome = statementOutcomes.value[index]
  if (!outcome) return
  activeStatementIndex.value = index
  result.value = outcome.result
  error.value = outcome.error
  cancelled.value = !!outcome.cancelled
  if (props.tabId) {
    browserStore.setQueryResult(props.tabId, outcome.result)
    browserStore.setQueryError(props.tabId, outcome.error)
  }
}
const explainResult = ref<ExplainPlan | null>(null)
const explaining = ref(false)
// 结果视图切换：'result' | 'explain'，默认跟执行走
const resultView = ref<'result' | 'explain'>('result')
// 查询历史 / 已保存查询面板
const panelOpen = ref(false)
const panelTab = ref<'history' | 'saved'>('history')
const librarySearch = ref('')

// 导出格式
const exportFormat = ref<'csv' | 'json' | 'xlsx' | 'sql' | 'markdown'>('csv')

const exportFormatOptions = [
  { label: 'CSV', value: 'csv' },
  { label: 'JSON', value: 'json' },
  { label: 'Excel', value: 'xlsx' },
  { label: 'SQL INSERT', value: 'sql' },
  { label: 'Markdown', value: 'markdown' },
]

function onExportFormatChange(value: string) {
  exportFormat.value = value as typeof exportFormat.value
}

// 编辑器高度调节
const editorHeight = ref(200)  // 默认高度 200px
const minEditorHeight = 100    // 最小高度
const maxEditorHeight = 500    // 最大高度
let isResizing = false
let startY = 0
let startHeight = 0

// 开始拖拽调整高度
function startResize(e: MouseEvent) {
  isResizing = true
  startY = e.clientY
  startHeight = editorHeight.value

  // 添加全局事件监听
  document.addEventListener('mousemove', handleResize)
  document.addEventListener('mouseup', stopResize)
  document.body.style.cursor = 'row-resize'
  document.body.style.userSelect = 'none'
}

// 拖拽中
function handleResize(e: MouseEvent) {
  if (!isResizing) return
  const deltaY = e.clientY - startY
  const newHeight = Math.max(minEditorHeight, Math.min(maxEditorHeight, startHeight + deltaY))
  editorHeight.value = newHeight
}

// 停止拖拽
function stopResize() {
  isResizing = false
  document.removeEventListener('mousemove', handleResize)
  document.removeEventListener('mouseup', stopResize)
  document.body.style.cursor = ''
  document.body.style.userSelect = ''
}

// 当前连接的数据库列表
const databases = computed(() => browserStore.objectTree)

// 数据库下拉选项
const databaseOptions = computed(() =>
  databases.value.map(db => ({ label: db.name, value: db.name }))
)

// 本 Tab 实际使用的连接 id：Tab 绑定的连接优先（prop），未绑定时回退全局当前连接
const effectiveConnectionId = computed(() =>
  resolveTabConnectionId({ connectionId: props.connectionId }, connStore.currentConnection?.id)
)

// 连接对象：优先按 id 从活跃连接中查找，连接已断开时回退全局当前连接
const effectiveConnection = computed(
  () =>
    connStore.activeConnections.find(conn => conn.id === effectiveConnectionId.value) ??
    connStore.currentConnection,
)

// 当前连接是否为 PostgreSQL（PG 的「库」在连接时固定，下拉里这一层应为 schema）
const isPostgres = computed(() => effectiveConnection.value?.driver === 'postgres')

// 默认选中当前正在浏览的数据库
const selectedDatabase = computed(() => {
  // 如果有手动选择的值，优先使用
  if (manualSelectedDatabase.value) {
    return manualSelectedDatabase.value
  }
  // PostgreSQL 的库在连接时固定，不使用侧栏残留的库/模式名
  if (isPostgres.value) {
    return ''
  }
  // 否则使用当前浏览的数据库
  return browserStore.currentDatabase || ''
})

// 手动选择的数据库（用户通过下拉框选择）
const manualSelectedDatabase = ref('')

// 当前用于补全 / 执行的库名
const currentDatabaseName = computed(
  () => selectedDatabase.value || browserStore.currentDatabase || ''
)

/**
 * 解析本次执行要用的库名。
 * 优先当前浏览/手动选择的库，其次连接自身配置的默认库；
 * 都为空说明用户还没选库，此时不应擅自回落到系统库 mysql。
 */
function resolveDatabase(): string {
  if (isPostgres.value) return manualSelectedDatabase.value
  return (
    selectedDatabase.value ||
    browserStore.currentDatabase ||
    effectiveConnection.value?.database ||
    ''
  )
}

watch(
  [effectiveConnectionId, currentDatabaseName],
  ([connectionId, database]) => {
    // PostgreSQL 尚未提供 schema 快照，补全器只在 MySQL 下加载
    if (isPostgres.value) return
    if (connectionId && database) {
      completionEngine.loadSchema(connectionId, database)
    }
  },
  { immediate: true }
)

// 切换连接时清空手动选择，避免沿用上一条连接的库/schema 名
watch(effectiveConnectionId, () => {
  manualSelectedDatabase.value = ''
})

// 连接驱动变化时同步 SQL 方言（MySQL / PostgreSQL 语法高亮不同）
watch(isPostgres, value => {
  editorApi?.setDriver(value ? 'postgres' : 'mysql')
})

// 根据数据类型获取列宽
function getColumnWidthByType(dataType: string): number {
  if (!dataType) return 120
  const type = dataType.toLowerCase()
  // ID/主键列
  if (type.includes('int') || type.includes('bigint') || type.includes('serial')) {
    return 80
  }
  // 小整数
  if (type.includes('tinyint') || type.includes('smallint') || type.includes('mediumint')) {
    return 60
  }
  // 布尔值
  if (type.includes('bool')) {
    return 60
  }
  // 日期时间
  if (type.includes('datetime') || type.includes('timestamp')) {
    return 160
  }
  // 日期
  if (type.includes('date')) {
    return 100
  }
  // 时间
  if (type.includes('time')) {
    return 80
  }
  // 小数
  if (type.includes('decimal') || type.includes('numeric') || type.includes('float') || type.includes('double')) {
    return 100
  }
  // 枚举/集合
  if (type.includes('enum') || type.includes('set')) {
    return 120
  }
  // JSON
  if (type.includes('json')) {
    return 200
  }
  // 文本/大字段
  if (type.includes('text') || type.includes('blob')) {
    return 250
  }
  // 字符串
  if (type.includes('varchar') || type.includes('char')) {
    return 150
  }
  // 二进制
  if (type.includes('binary') || type.includes('varbinary')) {
    return 150
  }
  // 默认
  return 120
}

// 计算列宽
const columnWidths = computed(() => {
  if (!result.value?.columns) return []
  return result.value.columns.map(col => getColumnWidthByType(col.data_type))
})

// 大结果集虚拟滚动：只渲染可视区内的行，避免上万行一次性挂载
const resultContainer = ref<HTMLElement | null>(null)
const {
  enabled: resultVirtualEnabled,
  start: resultVirtualStart,
  end: resultVirtualEnd,
  paddingTop: virtualPaddingTop,
  paddingBottom: virtualPaddingBottom,
  onScroll: handleResultScroll,
} = useVirtualRows(resultContainer, () => result.value?.rows.length ?? 0, {
  rowHeight: 36,
  threshold: 100,
})

const visibleResultRows = computed(() => {
  const rows = result.value?.rows ?? []
  if (!resultVirtualEnabled.value) {
    return rows.map((row, index) => ({ row, index }))
  }
  return rows
    .slice(resultVirtualStart.value, resultVirtualEnd.value)
    .map((row, offset) => ({ row, index: resultVirtualStart.value + offset }))
})

// 更新执行状态：执行超过 300ms 才把「执行」切换为「取消」，避免快速查询时按钮闪现
function setLoading(value: boolean) {
  if (cancelTimer) {
    clearTimeout(cancelTimer)
    cancelTimer = undefined
  }
  loading.value = value
  if (value) {
    cancelTimer = setTimeout(() => {
      showCancel.value = loading.value
    }, 300)
  } else {
    showCancel.value = false
  }
}

// ---- 参数化查询（B4）：`:name` 占位符 ----
const paramDialog = ref<{
  show: boolean
  names: string[]
  values: Record<string, string>
}>({ show: false, names: [], values: {} })
let pendingParamResolve: ((values: Record<string, string> | null) => void) | null = null
/** 一键重跑时携带的历史参数，用于填充参数弹窗 */
const presetParams = ref<Record<string, string> | undefined>(undefined)

/** 跳过一段引号包裹的内容，返回结束位置（支持 SQL 的重复引号转义） */
function skipQuoted(text: string, start: number, quote: string): number {
  let i = start + 1
  while (i < text.length) {
    if (text[i] === quote) {
      if (text[i + 1] === quote) {
        i += 2
        continue
      }
      return i + 1
    }
    i++
  }
  return i
}

function skipLineComment(text: string, start: number): number {
  const end = text.indexOf('\n', start)
  return end === -1 ? text.length : end
}

function skipBlockComment(text: string, start: number): number {
  const end = text.indexOf('*/', start + 2)
  return end === -1 ? text.length : end + 2
}

/** 提取 SQL 中的命名参数，忽略字符串、注释与 `::` 类型转换 */
function extractSqlParams(text: string): string[] {
  const names: string[] = []
  let i = 0
  while (i < text.length) {
    const ch = text[i]!
    if (ch === "'" || ch === '"' || ch === '`') {
      i = skipQuoted(text, i, ch)
      continue
    }
    if ((ch === '-' && text[i + 1] === '-') || ch === '#') {
      i = skipLineComment(text, i)
      continue
    }
    if (ch === '/' && text[i + 1] === '*') {
      i = skipBlockComment(text, i)
      continue
    }
    if (ch === ':' && text[i + 1] !== ':') {
      const match = /^:([A-Za-z_][A-Za-z0-9_]*)/.exec(text.slice(i))
      if (match) {
        const name = match[1]!
        if (!names.includes(name)) names.push(name)
        i += match[0].length
        continue
      }
    }
    i++
  }
  return names
}

/** 把用户输入按「数字/布尔/NULL 原样、其它按字符串转义」转成 SQL 字面量 */
function sqlParamLiteral(raw: string, driver: 'mysql' | 'postgres'): string {
  const value = raw.trim()
  if (value === '') return "''"
  if (/^null$/i.test(value)) return 'NULL'
  if (/^true$/i.test(value)) return '1'
  if (/^false$/i.test(value)) return '0'
  if (/^-?\d+(\.\d+)?$/.test(value)) return value
  return formatSqlValue(value, { driver })
}

/** 仅替换字符串/注释之外的 `:name`，避免误改字面量内容 */
function substituteSqlParams(text: string, values: Record<string, string>): string {
  const driver = isPostgres.value ? 'postgres' : 'mysql'
  let out = ''
  let i = 0
  while (i < text.length) {
    const ch = text[i]!
    if (ch === "'" || ch === '"' || ch === '`') {
      const next = skipQuoted(text, i, ch)
      out += text.slice(i, next)
      i = next
      continue
    }
    if ((ch === '-' && text[i + 1] === '-') || ch === '#') {
      const next = skipLineComment(text, i)
      out += text.slice(i, next)
      i = next
      continue
    }
    if (ch === '/' && text[i + 1] === '*') {
      const next = skipBlockComment(text, i)
      out += text.slice(i, next)
      i = next
      continue
    }
    if (ch === ':' && text[i + 1] !== ':') {
      const match = /^:([A-Za-z_][A-Za-z0-9_]*)/.exec(text.slice(i))
      if (match && Object.prototype.hasOwnProperty.call(values, match[1]!)) {
        out += sqlParamLiteral(values[match[1]!]!, driver)
        i += match[0].length
        continue
      }
    }
    out += ch
    i++
  }
  return out
}

function requestSqlParams(
  names: string[],
  preset?: Record<string, string>
): Promise<Record<string, string> | null> {
  paramDialog.value = {
    show: true,
    names,
    values: Object.fromEntries(names.map(name => [name, preset?.[name] ?? '']))
  }
  return new Promise(resolve => {
    pendingParamResolve = resolve
  })
}

function confirmSqlParams() {
  const values = { ...paramDialog.value.values }
  paramDialog.value.show = false
  pendingParamResolve?.(values)
  pendingParamResolve = null
}

function cancelSqlParams() {
  paramDialog.value.show = false
  pendingParamResolve?.(null)
  pendingParamResolve = null
}

// 执行 SQL
async function executeQuery() {
  if (!sql.value.trim() || !effectiveConnectionId.value) return
  // 上一次查询还没结束时再次触发（F5 连按、按钮重复点击）会并发发出请求，
  // 后返回的旧结果可能覆盖新结果，这里直接忽略
  if (loading.value) return

  // MySQL 必须有库名才能执行（后端会校验），没有可用库时提示用户先选库，
  // 避免静默落到系统库 mysql 上
  if (!isPostgres.value && !resolveDatabase()) {
    error.value = t('sqlEditor.selectDatabaseFirst')
    result.value = null
    return
  }

  // 参数化查询：执行前先弹窗收集 `:name` 的实参，取消则不执行
  const templateSql = sql.value
  const paramNames = extractSqlParams(templateSql)
  let paramValues: Record<string, string> | null = null
  if (paramNames.length > 0) {
    paramValues = await requestSqlParams(paramNames, presetParams.value)
    presetParams.value = undefined
    if (!paramValues) return
  }
  const executedSql = paramValues ? substituteSqlParams(templateSql, paramValues) : templateSql

  // 安全模式下对高风险语句二次确认（无 WHERE 的 UPDATE/DELETE 由后端直接拦截）
  if (effectiveConnection.value?.safety_mode === 'safe') {
    const dangerous = findDangerousStatements(executedSql)
    if (
      dangerous.length > 0 &&
      !(await confirmDanger({
        message: t('sqlEditor.safetyConfirm', { statements: dangerous.join(', ') }),
        confirmText: t('common.confirm'),
      }))
    ) {
      return
    }
  }

  setLoading(true)
  error.value = ''
  cancelled.value = false
  result.value = null
  statementOutcomes.value = []
  activeStatementIndex.value = 0
  // 清掉上一次的错误行高亮
  editorApi?.highlightErrorLine(null)
  // 切回结果视图，避免上次 EXPLAIN 的执行计划残留在面板中
  resultView.value = 'result'

  // 先按语句切分再逐条执行：一次执行多条语句时后端只保留第一个结果集，
  // 拆开执行才能让每条语句都有自己的结果分栏。
  let statements: string[]
  try {
    statements = await invoke<string[]>('split_sql_statements', { sql: executedSql })
  } catch (e) {
    console.error('切分 SQL 语句失败，回退为整段执行:', e)
    statements = [executedSql]
  }
  if (statements.length === 0) statements = [executedSql]
  const sourceSql = executedSql
  const statementOffsets = computeStatementOffsets(sourceSql, statements)

  let databaseName = resolveDatabase()
  let schemaChanged = false
  let usedDatabase = ''

  try {
    const outcomes: StatementOutcome[] = []
    for (const statement of statements) {
      try {
        const response = await connStore.executeSql(
          effectiveConnectionId.value,
          databaseName,
          statement,
        ) as QueryResultPayload
        outcomes.push({ sql: statement, result: response, error: '' })

        if (isSchemaChangingStatement(statement)) schemaChanged = true
        // `USE db2` 生效：后续语句改到新库执行
        if (!isPostgres.value) {
          const used = leadingUseDatabase(statement)
          if (used) {
            databaseName = used
            usedDatabase = used
          }
        }
      } catch (e) {
        const message = String(e)
        const isCancelled = message.includes('用户取消')
        outcomes.push({
          sql: statement,
          result: null,
          error: isCancelled ? '' : message,
          cancelled: isCancelled,
        })
        // 遇错即停，避免后续语句在不完整的前提下继续执行
        break
      }
    }

    statementOutcomes.value = outcomes
    if (props.tabId) browserStore.setQueryStatements(props.tabId, outcomes)
    // 有失败/取消时定位到那条语句，保证错误不会被前面的成功结果挡住
    const failureIndex = outcomes.findIndex(outcome => outcome.error || outcome.cancelled)
    const failure = failureIndex >= 0 ? outcomes[failureIndex] : undefined
    selectStatement(failureIndex >= 0 ? failureIndex : 0)

    // 执行失败：把数据库报的错误行号换算成整段文本的行号，跳转并高亮
    if (failure?.error) {
      highlightStatementError(sourceSql, statementOffsets[failureIndex] ?? 0, failure.error)
    }

    const totalMs = outcomes.reduce((sum, outcome) => sum + (outcome.result?.execution_time_ms ?? 0), 0)
    const firstResult = outcomes.find(outcome => outcome.result)?.result
    recordHistory(!failure, failure?.error, totalMs, {
      sqlText: templateSql,
      params: paramValues ?? undefined,
      rowsReturned: firstResult?.rows_returned,
      rowsAffected: firstResult?.rows_affected,
    })

    if (props.tabId && outcomes.some(outcome => outcome.result)) {
      editorTabs.markExecuted(props.tabId)
    }

    // DDL 后表结构已变化，刷新补全快照
    if (schemaChanged) {
      invalidateSchemaCache()
      refreshSchema()
    }

    // `USE db2` 已生效：把编辑器与补全的当前库切到 db2
    if (usedDatabase) {
      manualSelectedDatabase.value = usedDatabase
      invalidateSchemaCache()
      refreshSchema()
    }
  } finally {
    setLoading(false)
  }
}

// 取消执行
async function cancelQuery() {
  if (!effectiveConnectionId.value) return
  try {
    const rolledBack = await connStore.cancelQuery(effectiveConnectionId.value)
    // 取消会丢弃被取消的事务连接（自动回滚），必须显式提示，避免用户以为事务还在
    if (rolledBack) {
      globalToast.warning(t('sqlEditor.transactionRolledBack'))
    }
  } catch (e) {
    console.error('取消查询失败:', e)
  }
}

// EXPLAIN 执行计划
async function explainSql() {
  if (!sql.value.trim() || !effectiveConnectionId.value) return
  explaining.value = true
  explainResult.value = null
  resultView.value = 'explain'

  try {
    const dbName = resolveDatabase()
    const response = await invoke<ExplainPlan>('explain_sql', {
      connectionId: effectiveConnectionId.value,
      database: dbName,
      sql: sql.value,
    })
    explainResult.value = response
  } catch (e) {
    error.value = String(e)
    resultView.value = 'result'
  } finally {
    explaining.value = false
  }
}

// 格式化 SQL：优先走后端 sqlformat（正确处理嵌套子查询/窗口函数），
// 非 Tauri 环境或失败时回退到前端正则版；结果写回 CodeMirror（可撤销）
async function handleFormatSql() {
  const source = sql.value
  let formatted: string
  try {
    formatted = await invoke<string>('format_sql', { sql: source })
  } catch (e) {
    console.warn('后端格式化失败，回退前端正则版:', e)
    formatted = formatSql(source)
  }
  if (formatted && formatted !== source) editorApi?.replaceDoc(formatted)
}

// 清空编辑器
function clearEditor() {
  editorApi?.replaceDoc('')
  error.value = ''
  cancelled.value = false
  result.value = null
  statementOutcomes.value = []
  activeStatementIndex.value = 0
  if (props.tabId) editorTabs.markClean(props.tabId)
  editorApi?.focus()
}

// 格式化单元格显示
function formatCell(cell: unknown): string {
  if (cell === null) return 'NULL'
  if (cell === undefined) return ''
  if (typeof cell === 'object') {
    try {
      return JSON.stringify(cell)
    } catch {
      return String(cell)
    }
  }
  return String(cell)
}

// ===== 查询结果单元格查看 / 右键菜单 =====
const cellViewer = ref({
  show: false,
  value: null as unknown,
  columnName: '',
  columnType: '',
})

const resultCellMenu = ref({
  show: false,
  x: 0,
  y: 0,
  cell: null as unknown,
  columnIndex: 0,
})

function closeResultCellMenu() {
  resultCellMenu.value.show = false
}

function showResultCellMenu(event: MouseEvent, cell: unknown, columnIndex: number) {
  resultCellMenu.value = { show: true, x: event.clientX, y: event.clientY, cell, columnIndex }
}

/** 双击结果单元格：在独立查看器中展示大文本 / JSON / BLOB */
function openResultCellViewer(cell: unknown, columnIndex: number) {
  if (cell === null || cell === undefined) return
  const column = result.value?.columns[columnIndex]
  cellViewer.value = {
    show: true,
    value: cell,
    columnName: column?.name ?? '',
    columnType: column?.data_type ?? '',
  }
  closeResultCellMenu()
}

function copyResultCell() {
  const cell = resultCellMenu.value.cell
  const text = cell === null || cell === undefined ? 'NULL' : formatCell(cell)
  navigator.clipboard.writeText(text)
  closeResultCellMenu()
  globalToast.success(t('dataTable.copiedToClipboard'))
}

function viewResultCell() {
  openResultCellViewer(resultCellMenu.value.cell, resultCellMenu.value.columnIndex)
}

// ===== 查询历史 / 已保存查询 =====
const currentHistory = computed(() => queryLibrary.historyFor(effectiveConnectionId.value))

const filteredHistory = computed(() => {
  const keyword = librarySearch.value.trim().toLowerCase()
  if (!keyword) return currentHistory.value
  return currentHistory.value.filter(item => item.sql.toLowerCase().includes(keyword))
})

const savedGroups = computed(() => {
  const keyword = librarySearch.value.trim().toLowerCase()
  const items = queryLibrary.savedQueries.filter(item =>
    !keyword ||
    item.name.toLowerCase().includes(keyword) ||
    item.sql.toLowerCase().includes(keyword)
  )

  const groups = new Map<string, SavedQuery[]>()
  const uncategorized = t('sqlEditor.uncategorized')
  for (const item of items) {
    const folder = item.folder || uncategorized
    const list = groups.get(folder) ?? []
    list.push(item)
    groups.set(folder, list)
  }

  return [...groups.entries()]
    .sort(([a], [b]) => {
      if (a === uncategorized) return 1
      if (b === uncategorized) return -1
      return a.localeCompare(b, 'zh-CN')
    })
    .map(([folder, list]) => ({ folder, items: list }))
})

function loadHistoryItem(item: QueryHistoryEntry) {
  editorApi?.replaceDoc(item.sql)
  nextTick(() => editorApi?.focus())
}

function loadSavedQuery(item: SavedQuery) {
  editorApi?.replaceDoc(item.sql)
  nextTick(() => editorApi?.focus())
}

/** 记录一次执行结果（成功/失败都留痕，便于回溯） */
function recordHistory(
  success: boolean,
  errorMessage?: string,
  durationMs?: number,
  extra: {
    sqlText?: string
    params?: Record<string, string>
    rowsReturned?: number
    rowsAffected?: number
  } = {}
) {
  const connectionId = effectiveConnectionId.value
  if (!connectionId) return
  queryLibrary.addHistory({
    sql: extra.sqlText ?? sql.value,
    connectionId,
    database: selectedDatabase.value,
    success,
    error: errorMessage,
    durationMs: success ? durationMs ?? result.value?.execution_time_ms : undefined,
    params: extra.params,
    rowsReturned: extra.rowsReturned ?? result.value?.rows_returned,
    rowsAffected: extra.rowsAffected ?? result.value?.rows_affected,
  })
}

/** 结构变更语句执行后需要让补全快照失效 */
function isSchemaChangingStatement(statement: string): boolean {
  return /^\s*(create|alter|drop|rename|truncate)\b/i.test(statement)
}

/**
 * 提取 SQL 文本开头 `USE <db>` 切到的库（与后端 leading_use_database 规则一致）。
 * 用于执行成功后把编辑器的当前库切到新库，避免后续语句仍按旧库补全/执行。
 */
function leadingUseDatabase(text: string): string {
  const withoutComments = text
    .replace(/\/\*[\s\S]*?\*\//g, ' ')
    .replace(/--[^\n]*/g, ' ')
    .replace(/#[^\n]*/g, ' ')
  let database = ''
  for (const raw of withoutComments.split(';')) {
    const statement = raw.trim()
    if (!statement) continue
    if (/^set\b/i.test(statement)) continue
    const match = statement.match(/^use\s+[`"]?([^`"\s;]+)[`"]?$/i)
    if (!match) break
    const name = match[1]
    if (!name) break
    database = name
  }
  return database
}

// ===== SQL 错误行定位 =====

/** 后端按语句切分保留原文，顺序 indexOf 定位每条语句在整段文本中的起始偏移 */
function computeStatementOffsets(source: string, statements: string[]): number[] {
  const offsets: number[] = []
  let cursor = 0
  for (const statement of statements) {
    const candidates = statement.trim() ? [statement, statement.trim()] : []
    let matched = -1
    let nextCursor = cursor
    for (const candidate of candidates) {
      const index = source.indexOf(candidate, cursor)
      if (index >= 0) {
        matched = index
        nextCursor = index + candidate.length
        break
      }
    }
    offsets.push(matched >= 0 ? matched : cursor)
    cursor = nextCursor
  }
  return offsets
}

/** 从错误消息解析相对当前语句的行号（PostgreSQL: LINE N: / MySQL: at line N） */
function parseErrorStatementLine(message: string): number | null {
  const pg = message.match(/\bLINE\s+(\d+)/i)
  if (pg) return Number(pg[1])
  const mysql = message.match(/\bat line\s+(\d+)/i)
  if (mysql) return Number(mysql[1])
  return null
}

function lineOfOffset(text: string, offset: number): number {
  let line = 1
  const end = Math.min(offset, text.length)
  for (let i = 0; i < end; i++) {
    if (text[i] === '\n') line++
  }
  return line
}

/** 把失败语句的错误行号换算成整段文本的行号，在编辑器中跳转并高亮 */
function highlightStatementError(source: string, statementOffset: number, message: string) {
  const relative = parseErrorStatementLine(message)
  if (relative === null) return
  const globalLine = lineOfOffset(source, statementOffset) + relative - 1
  editorApi?.highlightErrorLine(globalLine)
}

function refreshSchema() {
  const connectionId = effectiveConnectionId.value
  const database = currentDatabaseName.value
  // PostgreSQL 尚未提供 schema 快照
  if (!isPostgres.value && connectionId && database) {
    completionEngine.loadSchema(connectionId, database)
  }
}

// ===== CodeMirror 6 编辑器（补全复用 useSqlCompletion 引擎）=====
const databaseNames = computed(() => databases.value.map(db => db.name))

/** 引擎补全结果 → CM6 autocomplete source 的适配层 */
function completionAdapter(
  text: string,
  caret: number,
  explicit: boolean
): ReturnType<typeof completionEngine.complete> | null {
  const result = completionEngine.complete(
    text,
    caret,
    databaseNames.value,
    currentDatabaseName.value
  )
  if (result.items.length === 0) return null

  // 自动触发要求已输入至少两个字符或刚输入 `.`；手动触发（Ctrl+Space）不受限
  const charBefore = caret > 0 ? text.charAt(caret - 1) : ''
  if (!explicit && result.token.length < 2 && charBefore !== '.') return null

  return result
}

onMounted(() => {
  if (editorHostRef.value && !editorApi) {
    editorApi = createSqlEditor({
      parent: editorHostRef.value,
      initialValue: sql.value,
      driver: isPostgres.value ? 'postgres' : 'mysql',
      placeholder: t('sqlEditor.placeholder'),
      onChange: value => {
        sql.value = value
      },
      onExecute: () => executeQuery(),
      onFormat: () => handleFormatSql(),
      complete: completionAdapter,
    })
  }
  refreshSchema()

  // 从 store 恢复查询结果
  if (props.tabId) {
    const cachedResult = browserStore.getQueryResult(props.tabId)
    const cachedError = browserStore.getQueryError(props.tabId)
    const cachedStatements = browserStore.getQueryStatements(props.tabId)
    if (cachedStatements.length > 0) {
      statementOutcomes.value = cachedStatements
      // 缓存里只保留当前选中的那条结果，据此还原激活的语句标签
      const cachedIndex = cachedStatements.findIndex(outcome => outcome.result === cachedResult)
      activeStatementIndex.value = cachedIndex >= 0 ? cachedIndex : 0
    }
    if (cachedResult) {
      result.value = cachedResult
    }
    if (cachedError) {
      error.value = cachedError
    }
  }

  // 历史重跑：打开后带预设参数自动执行一次
  const pending = props.tabId ? editorTabs.consumePendingRun(props.tabId) : null
  if (pending) {
    presetParams.value = pending.params
    void nextTick(() => executeQuery())
  }
})

// 截断 SQL 显示
function truncateSql(querySql: string): string {
  const maxLength = 60
  if (querySql.length <= maxLength) return querySql
  return querySql.substring(0, maxLength) + '...'
}

// 格式化时间
function formatTime(time: string | Date): string {
  const date = new Date(time)
  const now = new Date()
  const diff = now.getTime() - date.getTime()

  if (diff < 60000) return t('sqlEditor.timeJustNow')
  if (diff < 3600000) return t('sqlEditor.minutesAgo', { count: Math.floor(diff / 60000) })
  if (diff < 86400000) return t('sqlEditor.hoursAgo', { count: Math.floor(diff / 3600000) })
  return date.toLocaleDateString()
}

// 导出结果
async function exportResults() {
  if (!result.value || !result.value.rows) return

  const timestamp = new Date().toISOString().slice(0, 19).replace(/:/g, '-')
  const rows = result.value.rows
  const columns = result.value.columns

  switch (exportFormat.value) {
    case 'csv':
      exportToCSV(rows, columns, timestamp)
      break
    case 'json':
      exportToJSON(rows, columns, timestamp)
      break
    case 'xlsx':
      await exportToExcelViaBackend(rows, columns, timestamp)
      break
    case 'sql':
      exportToSQL(rows, columns, timestamp)
      break
    case 'markdown':
      exportToMarkdown(rows, columns, timestamp)
      break
  }
}

// 导出为 CSV
function exportToCSV(
  rows: unknown[][],
  columns: { name: string; data_type: string }[],
  timestamp: string
) {
  const headers = columns.map(col => escapeCSVField(col.name)).join(',')
  const dataRows = rows.map(row =>
    row.map(cell => escapeCSVField(cell)).join(',')
  ).join('\n')

  const csv = headers + '\n' + dataRows
  downloadFile(csv, `query-result-${timestamp}.csv`, 'text/csv;charset=utf-8;')
}

// 转义 CSV 字段
function escapeCSVField(cell: unknown): string {
  if (cell === null) return 'NULL'
  if (cell === undefined) return ''
  const str = String(cell)
  // 如果包含逗号、换行或引号，需要用引号包裹并转义
  if (/[",\n\r]/.test(str)) {
    return `"${str.replace(/"/g, '""')}"`
  }
  return str
}

// 导出为 JSON
function exportToJSON(
  rows: unknown[][],
  columns: { name: string; data_type: string }[],
  timestamp: string
) {
  const data = rows.map(row => {
    const obj: Record<string, unknown> = {}
    columns.forEach((col, index) => {
      obj[col.name] = row[index]
    })
    return obj
  })

  const json = JSON.stringify(data, null, 2)
  downloadFile(json, `query-result-${timestamp}.json`, 'application/json;charset=utf-8;')
}

// 通过后端导出为真正的 Excel (XLSX)
async function exportToExcelViaBackend(
  rows: unknown[][],
  columns: { name: string; data_type: string }[],
  timestamp: string
) {
  try {
    // 转换数据为后端可接受的格式
    const data = rows.map(row => {
      const obj: Record<string, unknown> = {}
      columns.forEach((col, index) => {
        obj[col.name] = row[index]
      })
      return obj
    })

    await invoke('export_to_excel', {
      columns: columns.map(col => ({ name: col.name, data_type: col.data_type })),
      data,
      filename: `query-result-${timestamp}.xlsx`
    })
  } catch (e) {
    console.error('Failed to export to Excel:', e)
    // 降级到 CSV
    exportToCSV(rows, columns, timestamp)
  }
}

// 导出为 SQL INSERT 语句
function exportToSQL(
  rows: unknown[][],
  columns: { name: string; data_type: string }[],
  timestamp: string
) {
  const tableName = 'exported_data'
  // MySQL 与 PG 的标识符引用、字符串转义、二进制字面量都不同，统一交给 formatSqlValue
  const driver = effectiveConnection.value?.driver === 'postgres' ? 'postgres' : 'mysql'
  const columnNames = columns.map(col => quoteIdentifier(col.name, driver)).join(', ')

  const insertStatements = rows.map(row => {
    const values = row
      .map((cell, index) =>
        formatSqlValue(cell, { driver, data_type: columns[index]?.data_type })
      )
      .join(', ')

    return `INSERT INTO ${quoteIdentifier(tableName, driver)} (${columnNames}) VALUES (${values});`
  }).join('\n')

  downloadFile(insertStatements, `query-result-${timestamp}.sql`, 'text/plain;charset=utf-8;')
}

// 导出为 Markdown 表格
function exportToMarkdown(
  rows: unknown[][],
  columns: { name: string; data_type: string }[],
  timestamp: string
) {
  const escape = (cell: unknown): string => {
    if (cell === null) return 'NULL'
    // 竖线会破坏表格结构，换行压成空格
    return String(cell).replace(/\|/g, '\\|').replace(/\r?\n/g, ' ')
  }

  const header = `| ${columns.map(col => escape(col.name)).join(' | ')} |`
  const separator = `| ${columns.map(() => '---').join(' | ')} |`
  const body = rows.map(row => `| ${row.map(escape).join(' | ')} |`).join('\n')

  downloadFile(`${header}\n${separator}\n${body}\n`, `query-result-${timestamp}.md`, 'text/markdown;charset=utf-8;')
}

// 通用下载函数（使用 Tauri 文件对话框）
async function downloadFile(content: string, filename: string, mimeType: string) {
  try {
    const extension = filename.split('.').pop() || 'txt'
    await invoke('save_file_to_dialog', {
      content,
      defaultFilename: filename,
      format: extension
    })
  } catch (e) {
    console.error('Failed to save file:', e)
    // 降级到浏览器下载
    const blob = new Blob([content], { type: mimeType })
    const url = URL.createObjectURL(blob)
    const link = document.createElement('a')
    link.href = url
    link.download = filename
    link.style.display = 'none'
    document.body.appendChild(link)
    link.click()
    setTimeout(() => {
      document.body.removeChild(link)
      URL.revokeObjectURL(url)
    }, 100)
  }
}

onUnmounted(() => {
  if (cancelTimer) clearTimeout(cancelTimer)
  editorApi?.destroy()
  editorApi = null
  // 拖拽中卸载时清理全局监听与临时样式
  document.removeEventListener('mousemove', handleResize)
  document.removeEventListener('mouseup', stopResize)
  document.body.style.cursor = ''
  document.body.style.userSelect = ''
})
</script>

<style scoped>
.sql-editor {
  flex: 1;
  display: flex;
  flex-direction: column;
  background: var(--canvas);
  overflow: hidden;
}

/* 工具栏 */
.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  height: var(--toolbar-height);
  padding: 0 var(--space-2);
  background: var(--canvas);
  border-bottom: 1px solid var(--hairline);
  gap: var(--space-2);
  flex-shrink: 0;
}

.toolbar-left,
.toolbar-right {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  min-width: 0;
}

.btn-execute {
  height: var(--control-height-sm);
  padding: 0 var(--space-3);
  background: var(--brand-500);
  color: #fff;
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  cursor: pointer;
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  display: flex;
  align-items: center;
  gap: 5px;
  flex-shrink: 0;
  transition: var(--transition-colors);
}

.btn-execute:hover:not(:disabled) {
  background: var(--brand-600);
}

.btn-execute:disabled {
  background: var(--hover-fill);
  color: var(--text-tertiary);
  cursor: not-allowed;
}

.btn-cancel {
  height: var(--control-height-sm);
  padding: 0 var(--space-3);
  background: rgba(239, 68, 68, 0.14);
  color: var(--error-500);
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  cursor: pointer;
  font-size: var(--text-xs);
  display: flex;
  align-items: center;
  gap: 5px;
  flex-shrink: 0;
}

.btn-cancel:hover {
  background: rgba(239, 68, 68, 0.22);
}

.btn-explain,
.btn-format,
.btn-clear {
  height: var(--control-height-sm);
  padding: 0 var(--space-2);
  border: none;
  background: transparent;
  color: var(--text-secondary);
  border-radius: var(--radius-sm);
  cursor: pointer;
  font-size: var(--text-xs);
  display: flex;
  align-items: center;
  gap: 5px;
  flex-shrink: 0;
  transition: var(--transition-colors);
}

.btn-explain:hover:not(:disabled),
.btn-format:hover:not(:disabled),
.btn-clear:hover:not(:disabled) {
  background: var(--hover-fill);
  color: var(--text-primary);
}

.btn-explain:disabled,
.btn-format:disabled,
.btn-clear:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.btn-format.is-active {
  background: var(--selected-fill);
  color: var(--text-primary);
}

/* 数据库选择：外层控制宽度，触发器与工具栏按钮同高 */
.database-select {
  width: 160px;
  flex-shrink: 0;
}

.database-select :deep(.custom-select .select-trigger) {
  height: var(--control-height-sm);
  min-height: var(--control-height-sm);
  padding: 0 var(--space-2);
  border-color: transparent;
  border-radius: var(--radius-sm);
  background: var(--raised);
  color: var(--text-primary);
  font-size: var(--text-xs);
}

/* 导出格式选择框：紧凑尺寸，与结果区按钮同高 */
.export-format-select {
  width: 112px;
  flex-shrink: 0;
}

.export-format-select :deep(.custom-select .select-trigger) {
  height: 22px;
  border-color: transparent;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--text-xs);
}

/* 编辑器区域 - 可调节高度 */
.editor-section {
  display: flex;
  flex-direction: column;
  border-bottom: 1px solid var(--hairline);
  min-height: 100px;
  max-height: 500px;
}

.editor-container {
  flex: 1;
  min-height: 80px;
  position: relative;
  overflow: visible;
}

/* CodeMirror 6 宿主：填满编辑区，内部配色由 useCodeMirror 主题接管 */
.cm-host {
  height: 100%;
}

.editor-container:focus-within {
  background: var(--canvas);
}

.cm-host :deep(.cm-editor) {
  height: 100%;
}

.cm-host :deep(.cm-editor.cm-errorLine) {
  background: transparent;
}

/* 错误行高亮（A6）：执行失败的语句所在行 */
.cm-host :deep(.cm-line.cm-errorLine) {
  background: rgba(244, 67, 54, 0.14);
  box-shadow: inset 3px 0 0 var(--error-500);
}

/* 补全弹窗类型角标 */
.cm-host :deep(.cm-completionLabel) {
  font-family: 'Consolas', 'Monaco', 'Courier New', monospace;
}

/* 拖拽分隔条 */
.resize-handle {
  height: 4px;
  background: transparent;
  border-bottom: 1px solid var(--hairline);
  cursor: row-resize;
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background var(--duration-fast);
}

.resize-handle:hover {
  background: var(--brand-500);
}

/* 分隔条中间的小横线 */
.resize-handle::before {
  content: '';
  width: 24px;
  height: 2px;
  background: var(--hairline-strong);
  border-radius: 1px;
  transition: background var(--duration-fast);
}

.resize-handle:hover::before {
  background: #fff;
  opacity: 0.9;
}

/* 多语句结果分栏 */
.statement-tabs {
  display: flex;
  align-items: stretch;
  gap: 0;
  padding: 0 var(--space-2);
  height: var(--tabbar-height);
  overflow-x: auto;
  background: var(--panel);
  border-bottom: 1px solid var(--hairline);
  flex-shrink: 0;
}

.statement-tabs::-webkit-scrollbar {
  display: none;
}

.statement-tab {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 100%;
  padding: 0 var(--space-3);
  max-width: 240px;
  border: none;
  border-radius: 0;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  font-size: var(--text-xs);
  white-space: nowrap;
  box-shadow: inset -1px 0 0 var(--hairline);
  position: relative;
}

.statement-tab:hover {
  background: var(--hover-fill);
  color: var(--text-primary);
}

.statement-tab.active {
  background: var(--canvas);
  color: var(--text-primary);
  box-shadow: none;
}

.statement-tab.has-error {
  color: var(--error-500);
}

.statement-no {
  font-weight: 600;
}

.statement-sql {
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 180px;
}

/* 结果单元格右键菜单 */
.context-menu {
  position: fixed;
  background: var(--overlay);
  border: 1px solid var(--hairline-strong);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-lg);
  z-index: 1000;
  min-width: 120px;
  padding: var(--space-1);
}

.menu-item {
  height: var(--control-height-sm);
  display: flex;
  align-items: center;
  padding: 0 var(--space-2);
  border-radius: var(--radius-sm);
  cursor: pointer;
  font-size: var(--text-xs);
  color: var(--text-primary);
}

.menu-item:hover {
  background: var(--hover-fill);
}

/* 结果面板 */
.result-panel {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  min-height: 0;
  max-height: 100%;
}

.result-panel.error {
  background: rgba(239, 68, 68, 0.08);
}

.result-header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  height: var(--toolbar-height);
  padding: 0 var(--space-2);
  background: var(--canvas);
  border-bottom: 1px solid var(--hairline);
  flex-shrink: 0;
}

.success-icon {
  color: var(--success-500);
}

.error-icon {
  color: var(--error-500);
}

.loading-icon {
  color: var(--brand-500);
  animation: spin 1s linear infinite;
}

@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

.execution-info {
  color: var(--text-tertiary);
  font-size: var(--text-xs);
  margin-left: auto;
}

.btn-export {
  height: 22px;
  padding: 0 var(--space-2);
  background: transparent;
  border: none;
  color: var(--text-secondary);
  border-radius: var(--radius-sm);
  cursor: pointer;
  font-size: var(--text-xs);
}

.btn-export:hover:not(:disabled) {
  background: var(--hover-fill);
  color: var(--text-primary);
}

.error-message {
  padding: var(--space-3);
  color: var(--error-500);
  font-family: monospace;
  white-space: pre-wrap;
  overflow: auto;
}

/* 结果表格 */
.result-table-container {
  flex: 1;
  overflow: auto;
  overflow-x: auto;
  overflow-y: auto;
  max-height: 100%;
}

.result-table {
  width: max-content;
  min-width: 100%;
  border-collapse: separate;
  border-spacing: 0;
  table-layout: fixed;
  font-size: var(--text-sm);
}

.result-table th,
.result-table td {
  padding: 0 var(--space-2);
  text-align: left;
  border-bottom: 1px solid var(--hairline);
  border-right: none;
  font-size: var(--text-sm);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  vertical-align: middle;
}

.result-table td {
  height: var(--row-height);
  line-height: calc(var(--row-height) - 1px);
}

.result-table th {
  line-height: 1.3;
  padding-top: 3px;
  padding-bottom: 3px;
}

.result-table td {
  max-width: 0;
}

/* 虚拟滚动占位行：只用于撑起滚动高度 */
.result-table .virtual-spacer td {
  max-width: none;
  padding: 0;
  border: none;
}

.result-table th {
  background: var(--surface-default);
  font-weight: var(--font-medium);
  position: sticky;
  top: 0;
  z-index: 1;
}

.result-table .th-content {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.result-table .col-name {
  font-weight: 500;
}

.result-table .data-type {
  font-size: 10px;
  color: var(--text-tertiary);
  font-weight: normal;
}

.result-table tr:hover {
  background: var(--hover-fill);
}

.result-table td.is-null {
  color: var(--text-tertiary);
  font-style: italic;
}

.result-table .cell-content {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 100%;
}

.no-results {
  padding: var(--space-5);
  text-align: center;
  color: var(--text-tertiary);
  font-size: var(--text-sm);
}

.result-warning {
  padding: 10px 16px;
  background: var(--warning-50, rgba(245, 158, 11, 0.12));
  border-bottom: 1px solid var(--warning-500, #f59e0b);
  color: var(--warning-700, #b45309);
  font-size: 13px;
  flex-shrink: 0;
}

/* 查询历史 / 已保存查询面板 */
.library-panel {
  flex: 0 0 220px;
  display: flex;
  flex-direction: column;
  border-top: 1px solid var(--hairline);
  overflow: hidden;
}

.library-header {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  background: var(--panel);
  border-bottom: 1px solid var(--hairline);
  flex-shrink: 0;
}

.library-tabs {
  display: flex;
  gap: 4px;
}

.library-tab {
  background: none;
  border: none;
  padding: 5px 9px;
  border-radius: 4px;
  font-size: 12px;
  color: var(--text-secondary);
  cursor: pointer;
}

.library-tab:hover {
  background: var(--hover-fill);
}

.library-tab.active {
  background: var(--canvas);
  color: var(--text-primary);
  font-weight: 500;
  box-shadow: inset 0 0 0 1px var(--hairline);
}

.library-search {
  flex: 1;
  min-width: 0;
  padding: 5px 8px;
  border: 1px solid var(--hairline);
  border-radius: 4px;
  font-size: 12px;
  background: var(--canvas);
  color: var(--text-primary);
}

.library-search:focus {
  outline: none;
  border-color: var(--brand-500);
}

.library-empty {
  padding: 24px 16px;
  text-align: center;
  font-size: 12px;
  color: var(--text-secondary);
}

.library-folder {
  padding: 6px 16px;
  font-size: 11px;
  font-weight: 500;
  color: var(--text-secondary);
  background: var(--panel);
  border-bottom: 1px solid var(--hairline);
  position: sticky;
  top: 0;
}

.history-sql {
  flex: 1;
  font-family: monospace;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  margin-right: 12px;
}

.history-failed {
  font-size: 10px;
  color: var(--error-500);
  border: 1px solid currentColor;
  border-radius: 3px;
  padding: 0 4px;
  margin-right: 8px;
  flex-shrink: 0;
}

.btn-item-delete {
  background: none;
  border: none;
  color: var(--text-secondary);
  cursor: pointer;
  font-size: 14px;
  line-height: 1;
  padding: 0 2px;
  margin-left: 8px;
  flex-shrink: 0;
}

.btn-item-delete:hover {
  color: var(--error-500);
}

.btn-clear-history {
  background: none;
  border: none;
  color: var(--text-secondary);
  cursor: pointer;
  font-size: 12px;
  padding: 4px;
  display: flex;
  align-items: center;
}

.btn-clear-history:hover {
  color: var(--error-500);
}

.btn-clear-history.block {
  display: block;
  width: 100%;
  padding: 8px;
  border-top: 1px solid var(--hairline);
  font-size: 12px;
}

.btn-format.is-active {
  background: var(--hover-fill);
  border-color: var(--brand-500);
  color: var(--brand-500);
}

.history-list {
  flex: 1;
  overflow-y: auto;
}

.history-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 16px;
  cursor: pointer;
  border-bottom: 1px solid var(--hairline);
  font-size: 12px;
}

.history-item:hover {
  background: var(--hover-fill);
}

.history-sql {
  flex: 1;
  font-family: monospace;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  margin-right: 12px;
}

.history-time {
  color: var(--text-secondary);
  font-size: 11px;
}

/* 参数化查询弹窗 */
.param-mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.35);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 2000;
}

.param-dialog {
  width: 420px;
  max-width: 90vw;
  background: var(--panel);
  border: 1px solid var(--hairline-strong);
  border-radius: 8px;
  padding: 16px;
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.25);
}

.param-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-weight: 600;
  margin-bottom: 8px;
}

.param-close {
  background: none;
  border: none;
  color: var(--text-secondary);
  font-size: 18px;
  line-height: 1;
  cursor: pointer;
}

.param-hint {
  margin: 0 0 12px;
  font-size: 12px;
  color: var(--text-secondary);
}

.param-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  max-height: 45vh;
  overflow: auto;
}

.param-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.param-name {
  min-width: 110px;
  font-family: monospace;
  font-size: 12px;
  color: var(--brand-500);
}

.param-row input {
  flex: 1;
  padding: 6px 8px;
  border: 1px solid var(--hairline-strong);
  border-radius: 4px;
  background: var(--panel);
  color: var(--text-primary);
}

.param-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 16px;
}

.param-primary {
  background: var(--brand-500);
  color: #fff;
  border: none;
  border-radius: 4px;
  padding: 6px 14px;
  cursor: pointer;
}
</style>
