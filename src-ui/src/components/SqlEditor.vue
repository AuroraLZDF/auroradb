<template>
  <div class="sql-editor">
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
        <button @click="explainSql" :disabled="explaining || !sql.trim() || !licenseStore.allows('query-plan')" class="btn-explain">
          <Cpu :size="14" />
          {{ t('sqlEditor.explain') }}
        </button>
        <button @click="formatSql" :disabled="!sql.trim()" class="btn-format">
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
            :placeholder="t('sqlEditor.selectDatabase')"
            @update:model-value="manualSelectedDatabase = $event"
          />
        </div>
      </div>
    </div>

    <!-- 编辑器区域 - 可调节高度 -->
    <div class="editor-section" :style="{ height: editorHeight + 'px' }">
      <div class="editor-container">
        <textarea
          ref="editorRef"
          v-model="sql"
          class="sql-textarea"
          :placeholder="t('sqlEditor.placeholder')"
          spellcheck="false"
          @keydown="handleKeydown"
          @input="handleEditorInput"
          @click="closeCompletion"
          @blur="closeCompletion"
        ></textarea>
        <!-- 补全弹窗 -->
        <div
          v-if="completion.open && completion.items.length > 0"
          class="completion-popup"
          :style="{ top: completion.top + 'px', left: completion.left + 'px' }"
        >
          <div
            v-for="(item, index) in completion.items"
            :key="item.kind + ':' + item.label"
            :class="['completion-item', { active: index === completion.index }]"
            @mousedown.prevent="acceptCompletion(index)"
            @mouseenter="completion.index = index"
          >
            <span class="completion-label">{{ item.label }}</span>
            <span class="completion-kind">{{ kindLabel(item.kind) }}</span>
            <span v-if="item.detail" class="completion-detail">{{ item.detail }}</span>
          </div>
        </div>
      </div>
      <!-- 拖拽分隔条 -->
      <div
        class="resize-handle"
        @mousedown="startResize"
        :title="t('sqlEditor.resizeHint')"
      ></div>
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
      <ExplainTree :plan="explainResult" />
    </div>

    <div v-else-if="result" class="result-panel">
      <div class="result-header">
        <CheckCircle class="success-icon" :size="16" />
        <span>{{ t('sqlEditor.executeSuccess') }}</span>
        <span class="execution-info">
          {{ result.rows_affected !== undefined ? t('sqlEditor.rowsAffected', { count: result.rows_affected }) : t('sqlEditor.rowsReturned', { count: result.rows_returned }) }}
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

      <div v-else-if="result.rows_affected !== undefined" class="no-results">
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
        <button class="btn-clear-history" :title="t('common.close')" @click="panelOpen = false">
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
          <button class="btn-item-delete" :title="t('common.delete')" @click.stop="queryLibrary.removeHistory(item.id)">
            ×
          </button>
        </div>
        <button
          v-if="currentHistory.length > 0"
          class="btn-clear-history block"
          @click="queryLibrary.clearHistory(currentConnectionId)"
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
            <button class="btn-item-delete" :title="t('common.delete')" @click.stop="queryLibrary.deleteSavedQuery(item.id)">
              ×
            </button>
          </div>
        </template>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, nextTick, reactive, watch } from 'vue'
import { useConnectionStore } from '@/stores/connection'
import { useDataBrowserStore } from '@/stores/dataBrowser'
import { useEditorTabsStore } from '@/stores/editorTabs'
import { useLicenseStore } from '@/stores/license'
import { useQueryLibraryStore, type QueryHistoryEntry, type SavedQuery } from '@/stores/queryLibrary'
import { Play, FileText, Trash2, Download, XCircle, CheckCircle, Loader2, Square, History, X, Cpu } from 'lucide-vue-next'
import { t } from '@/i18n'
import { findDangerousStatements, quoteIdentifier } from '@/utils/sql'
import { invoke } from '@tauri-apps/api/core'
import { useVirtualRows } from '@/composables/useVirtualRows'
import {
  useSqlCompletion,
  invalidateSchemaCache,
  type CompletionItem,
  type CompletionKind,
} from '@/composables/useSqlCompletion'
import ExplainTree from './ExplainTree.vue'
import type { ExplainPlan } from './ExplainTree.vue'
import Select from '@/components/ui/Select.vue'

const connStore = useConnectionStore()
const browserStore = useDataBrowserStore()
const editorTabs = useEditorTabsStore()
const queryLibrary = useQueryLibraryStore()
const licenseStore = useLicenseStore()
const completionEngine = useSqlCompletion()

// Props
const props = defineProps<{
  tabId?: string
}>()

// 确保 store 中存在这个 tab 的内容记录
if (props.tabId) editorTabs.ensure(props.tabId)

const editorRef = ref<HTMLTextAreaElement>()

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
const result = ref<{
  columns: { name: string; data_type: string }[]
  rows: unknown[][]
  execution_time_ms: number
  rows_affected?: number
  rows_returned: number
  truncated?: boolean
  warning?: string
} | null>(null)
const explainResult = ref<ExplainPlan | null>(null)
const explaining = ref(false)
// 结果视图切换：'result' | 'explain'，默认跟执行走
const resultView = ref<'result' | 'explain'>('result')
// 查询历史 / 已保存查询面板
const panelOpen = ref(false)
const panelTab = ref<'history' | 'saved'>('history')
const librarySearch = ref('')

// 导出格式
const exportFormat = ref<'csv' | 'json' | 'xlsx' | 'sql'>('csv')

const exportFormatOptions = [
  { label: 'CSV', value: 'csv' },
  { label: 'JSON', value: 'json' },
  { label: 'Excel', value: 'xlsx' },
  { label: 'SQL INSERT', value: 'sql' },
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

// 获取当前连接的连接ID
const currentConnectionId = computed(() => connStore.currentConnection?.id || '')

// 默认选中当前正在浏览的数据库
const selectedDatabase = computed(() => {
  // 如果有手动选择的值，优先使用
  if (manualSelectedDatabase.value) {
    return manualSelectedDatabase.value
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

watch(
  [currentConnectionId, currentDatabaseName],
  ([connectionId, database]) => {
    if (connectionId && database) {
      completionEngine.loadSchema(connectionId, database)
    }
  },
  { immediate: true }
)

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

// 执行 SQL
async function executeQuery() {
  if (!sql.value.trim() || !currentConnectionId.value) return

  // 安全模式下对高风险语句二次确认（无 WHERE 的 UPDATE/DELETE 由后端直接拦截）
  if (connStore.currentConnection?.safety_mode === 'safe') {
    const dangerous = findDangerousStatements(sql.value)
    if (dangerous.length > 0 && !confirm(t('sqlEditor.safetyConfirm', { statements: dangerous.join(', ') }))) {
      return
    }
  }

  setLoading(true)
  error.value = ''
  cancelled.value = false
  result.value = null
  // 切回结果视图，避免上次 EXPLAIN 的执行计划残留在面板中
  resultView.value = 'result'

  try {
    // PG 没有「默认 mysql 库」的概念，空串表示沿用服务端默认 search_path
    const fallbackDb = connStore.currentConnection?.driver === 'postgres' ? '' : 'mysql'
    const dbName = selectedDatabase.value || browserStore.currentDatabase || fallbackDb
    const response = await connStore.executeSql(currentConnectionId.value, dbName, sql.value) as {
      columns: { name: string; data_type: string }[]
      rows: unknown[][]
      execution_time_ms: number
      rows_affected?: number
      rows_returned: number
      truncated?: boolean
      warning?: string
    }
    result.value = response

    // 保存到 store 缓存
    if (props.tabId && result.value) {
      browserStore.setQueryResult(props.tabId, result.value)
      browserStore.setQueryError(props.tabId, '')
      editorTabs.markExecuted(props.tabId)
    }

    // 添加到历史
    recordHistory(true)

    // DDL 后表结构已变化，刷新补全快照
    if (isSchemaChangingStatement(sql.value)) {
      invalidateSchemaCache()
      refreshSchema()
    }
  } catch (e) {
    const message = String(e)
    if (message.includes('用户取消')) {
      cancelled.value = true
      error.value = ''
      if (props.tabId) {
        browserStore.setQueryError(props.tabId, '')
        browserStore.setQueryResult(props.tabId, null)
      }
    } else {
      error.value = message
      recordHistory(false, message)
      // 保存错误到 store 缓存
      if (props.tabId) {
        browserStore.setQueryError(props.tabId, error.value)
        browserStore.setQueryResult(props.tabId, null)
      }
    }
  } finally {
    setLoading(false)
  }
}

// 取消执行
async function cancelQuery() {
  if (!currentConnectionId.value) return
  try {
    await connStore.cancelQuery(currentConnectionId.value)
  } catch (e) {
    console.error('取消查询失败:', e)
  }
}

// EXPLAIN 执行计划（Pro 功能）
async function explainSql() {
  if (!sql.value.trim() || !currentConnectionId.value) return
  if (!licenseStore.allows('query-plan')) {
    licenseStore.openDialog()
    return
  }
  explaining.value = true
  explainResult.value = null
  resultView.value = 'explain'

  try {
    const fallbackDb = connStore.currentConnection?.driver === 'postgres' ? '' : 'mysql'
    const dbName = selectedDatabase.value || browserStore.currentDatabase || fallbackDb
    const response = await invoke<ExplainPlan>('explain_sql', {
      connectionId: currentConnectionId.value,
      database: dbName,
      sql: sql.value,
    })
    explainResult.value = response
  } catch (e) {
    const message = String(e)
    if (message.includes('LICENSE_REQUIRED')) {
      licenseStore.openDialog()
    } else {
      error.value = message
      resultView.value = 'result'
    }
  } finally {
    explaining.value = false
  }
}

// 格式化 SQL
function formatSql() {
  // 简单的格式化：大写关键字，换行
  const keywords = ['SELECT', 'FROM', 'WHERE', 'JOIN', 'LEFT', 'RIGHT', 'INNER', 'OUTER',
    'ON', 'AND', 'OR', 'NOT', 'GROUP BY', 'ORDER BY', 'HAVING', 'LIMIT',
    'INSERT INTO', 'VALUES', 'UPDATE', 'SET', 'DELETE FROM', 'CREATE TABLE',
    'ALTER TABLE', 'DROP TABLE', 'TRUNCATE TABLE']

  let formatted = sql.value

  // 大写关键字
  keywords.forEach(keyword => {
    const regex = new RegExp(`\\b${keyword}\\b`, 'gi')
    formatted = formatted.replace(regex, keyword)
  })

  // 在关键字前换行
  formatted = formatted
    .replace(/\s*,\s*/g, ', ')
    .replace(/\s+/g, ' ')

  sql.value = formatted
}

// 清空编辑器
function clearEditor() {
  sql.value = ''
  error.value = ''
  result.value = null
  if (props.tabId) editorTabs.markClean(props.tabId)
  editorRef.value?.focus()
}

// 处理键盘事件
function handleKeydown(e: KeyboardEvent) {
  // 补全弹窗打开时接管方向键 / 确认 / 关闭
  if (completion.open) {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      completion.index = (completion.index + 1) % completion.items.length
      return
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault()
      completion.index = (completion.index - 1 + completion.items.length) % completion.items.length
      return
    }
    if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault()
      acceptCompletion(completion.index)
      return
    }
    if (e.key === 'Escape') {
      e.preventDefault()
      closeCompletion()
      return
    }
  }

  // Ctrl+Space 手动触发补全
  if (e.ctrlKey && e.code === 'Space') {
    e.preventDefault()
    openCompletion(true)
    return
  }

  // F5 执行
  if (e.key === 'F5') {
    e.preventDefault()
    executeQuery()
    return
  }

  // Shift+Alt+F 格式化
  if (e.shiftKey && e.altKey && (e.key === 'f' || e.key === 'F')) {
    e.preventDefault()
    formatSql()
    return
  }

  // Tab 插入缩进
  if (e.key === 'Tab') {
    e.preventDefault()
    const start = editorRef.value?.selectionStart
    const end = editorRef.value?.selectionEnd
    if (start !== undefined && end !== undefined) {
      sql.value = sql.value.substring(0, start) + '  ' + sql.value.substring(end)
      editorRef.value?.setSelectionRange(start + 2, start + 2)
    }
  }
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

// ===== 查询历史 / 已保存查询 =====
const currentHistory = computed(() => queryLibrary.historyFor(currentConnectionId.value))

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
  sql.value = item.sql
  closeCompletion()
  nextTick(() => editorRef.value?.focus())
}

function loadSavedQuery(item: SavedQuery) {
  sql.value = item.sql
  closeCompletion()
  nextTick(() => editorRef.value?.focus())
}

/** 记录一次执行结果（成功/失败都留痕，便于回溯） */
function recordHistory(success: boolean, errorMessage?: string) {
  const connectionId = currentConnectionId.value
  if (!connectionId) return
  queryLibrary.addHistory({
    sql: sql.value,
    connectionId,
    database: selectedDatabase.value,
    success,
    error: errorMessage,
    durationMs: success ? result.value?.execution_time_ms : undefined,
  })
}

/** 结构变更语句执行后需要让补全快照失效 */
function isSchemaChangingStatement(statement: string): boolean {
  return /^\s*(create|alter|drop|rename|truncate)\b/i.test(statement)
}

function refreshSchema() {
  const connectionId = currentConnectionId.value
  const database = currentDatabaseName.value
  if (connectionId && database) {
    completionEngine.loadSchema(connectionId, database)
  }
}

// ===== Schema 感知补全 =====
const databaseNames = computed(() => databases.value.map(db => db.name))

const completion = reactive({
  open: false,
  items: [] as CompletionItem[],
  index: 0,
  top: 0,
  left: 0,
  replaceStart: 0,
  replaceEnd: 0,
})

const kindLabels = computed<Record<CompletionKind, string>>(() => ({
  column: t('sqlEditor.kindColumn'),
  table: t('sqlEditor.kindTable'),
  view: t('sqlEditor.kindView'),
  database: t('sqlEditor.kindDatabase'),
  keyword: t('sqlEditor.kindKeyword'),
}))

function kindLabel(kind: CompletionKind): string {
  return kindLabels.value[kind]
}

// textarea 没有原生 caret 坐标 API，用等宽字体度量估算弹窗位置
const COMPLETION_WIDTH = 280
const COMPLETION_MAX_HEIGHT = 220
const EDITOR_PADDING = 16
let charWidth = 8.4
let lineHeight = 21

function measureEditorMetrics() {
  const el = editorRef.value
  if (!el) return
  const style = getComputedStyle(el)
  const computedLineHeight = parseFloat(style.lineHeight)
  if (!Number.isNaN(computedLineHeight) && computedLineHeight > 0) {
    lineHeight = computedLineHeight
  }
  const context = document.createElement('canvas').getContext('2d')
  if (context) {
    context.font = `${style.fontSize} ${style.fontFamily}`
    const measured = context.measureText('M').width
    if (measured > 0) charWidth = measured
  }
}

function positionCompletion(caret: number) {
  const el = editorRef.value
  if (!el) return

  const before = sql.value.slice(0, caret)
  const lines = before.split('\n')
  const line = lines.length - 1
  const column = lines[line]?.length ?? 0

  const left = EDITOR_PADDING + column * charWidth - el.scrollLeft
  const maxLeft = Math.max(0, el.clientWidth - COMPLETION_WIDTH - 8)
  const top = EDITOR_PADDING + (line + 1) * lineHeight - el.scrollTop
  const maxTop = Math.max(0, el.clientHeight - COMPLETION_MAX_HEIGHT - 8)

  completion.left = Math.max(0, Math.min(left, maxLeft))
  completion.top = Math.max(0, Math.min(top, maxTop))
}

function closeCompletion() {
  completion.open = false
  completion.items = []
}

function openCompletion(manual = false) {
  const el = editorRef.value
  if (!el) return
  if (el.selectionStart !== el.selectionEnd) return

  const caret = el.selectionStart ?? 0
  const value = el.value
  const result = completionEngine.complete(
    value,
    caret,
    databaseNames.value,
    currentDatabaseName.value
  )

  if (result.items.length === 0) {
    closeCompletion()
    return
  }

  // 自动触发要求已输入至少两个字符或刚输入 `.`；手动触发（Ctrl+Space）不受限
  const charBefore = caret > 0 ? value.charAt(caret - 1) : ''
  if (!manual && result.token.length < 2 && charBefore !== '.') {
    closeCompletion()
    return
  }

  completion.items = result.items
  completion.index = 0
  completion.replaceStart = result.replaceStart
  completion.replaceEnd = result.replaceEnd
  completion.open = true
  positionCompletion(caret)
}

function handleEditorInput(event: Event) {
  const target = event.target as HTMLTextAreaElement
  const caret = target.selectionStart ?? 0
  const lastChar = target.value[caret - 1] ?? ''

  if (/[A-Za-z0-9_$.]/.test(lastChar)) {
    nextTick(() => openCompletion(false))
  } else {
    closeCompletion()
  }
}

function acceptCompletion(index: number) {
  const item = completion.items[index]
  if (!item) return

  const el = editorRef.value
  const start = completion.replaceStart
  const end = completion.replaceEnd
  sql.value = sql.value.slice(0, start) + item.label + sql.value.slice(end)
  closeCompletion()

  nextTick(() => {
    const caret = start + item.label.length
    el?.focus()
    el?.setSelectionRange(caret, caret)
  })
}

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
  const columnNames = columns.map(col => quoteIdentifier(col.name)).join(', ')

  const insertStatements = rows.map(row => {
    const values = row.map((cell, index) => {
      if (cell === null) return 'NULL'
      const col = columns[index]
      if (!col) return 'NULL'
      const dataType = col.data_type.toLowerCase()
      // 数字类型不加引号
      if (dataType.includes('int') || dataType.includes('float') || dataType.includes('double') || dataType.includes('decimal') || dataType.includes('bool')) {
        return String(cell)
      }
      // 字符串类型加引号并转义
      return `'${String(cell).replace(/\\/g, '\\\\').replace(/'/g, "\\'")}'`
    }).join(', ')

    return `INSERT INTO ${quoteIdentifier(tableName)} (${columnNames}) VALUES (${values});`
  }).join('\n')

  downloadFile(insertStatements, `query-result-${timestamp}.sql`, 'text/plain;charset=utf-8;')
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

// 加载缓存的查询结果
onMounted(() => {
  measureEditorMetrics()
  refreshSchema()

  // 从 store 恢复查询结果
  if (props.tabId) {
    const cachedResult = browserStore.getQueryResult(props.tabId)
    const cachedError = browserStore.getQueryError(props.tabId)
    if (cachedResult) {
      result.value = cachedResult
    }
    if (cachedError) {
      error.value = cachedError
    }
  }
})

onUnmounted(() => {
  if (cancelTimer) clearTimeout(cancelTimer)
})
</script>

<style scoped>
.sql-editor {
  flex: 1;
  display: flex;
  flex-direction: column;
  background: var(--color-background);
  overflow: hidden;
}

/* 工具栏 */
.toolbar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px 16px;
  background: var(--color-background-soft);
  border-bottom: 1px solid var(--color-border);
  gap: 12px;
}

.toolbar-left {
  display: flex;
  gap: 8px;
}

.toolbar-right {
  display: flex;
  gap: 8px;
}

.btn-execute {
  background: var(--color-success);
  color: white;
  border: none;
  padding: 8px 16px;
  border-radius: 4px;
  cursor: pointer;
  font-size: 13px;
  display: flex;
  align-items: center;
  gap: 6px;
}

.btn-execute:hover:not(:disabled) {
  background: var(--color-primary-dark);
}

.btn-execute:disabled {
  background: var(--color-border);
  cursor: not-allowed;
}

.btn-cancel {
  background: var(--color-danger);
  color: white;
  border: none;
  padding: 8px 16px;
  border-radius: 4px;
  cursor: pointer;
  font-size: 13px;
  display: flex;
  align-items: center;
  gap: 6px;
}

.btn-cancel:hover {
  opacity: 0.9;
}

.btn-format,
.btn-clear {
  background: var(--color-background);
  border: 1px solid var(--color-border);
  color: var(--color-text);
  padding: 8px 12px;
  border-radius: 4px;
  cursor: pointer;
  font-size: 13px;
}

.btn-format:hover:not(:disabled),
.btn-clear:hover:not(:disabled) {
  background: var(--color-background-hover);
}

/* 数据库选择：外层控制宽度，触发器与工具栏按钮同高 */
.database-select {
  width: 160px;
  flex-shrink: 0;
}

.database-select :deep(.custom-select .select-trigger) {
  height: 36px;
  min-height: 36px;
  padding: 0 12px;
  border-color: var(--color-border);
  border-radius: 4px;
  background: var(--color-background);
  color: var(--color-text);
  font-size: 13px;
}

/* 导出格式选择框：紧凑尺寸，与结果区按钮同高 */
.export-format-select {
  width: 112px;
  flex-shrink: 0;
}

.export-format-select :deep(.custom-select .select-trigger) {
  border-color: var(--color-border);
  border-radius: 4px;
  background: var(--color-background);
  color: var(--color-text);
  font-size: 12px;
}

/* 编辑器区域 - 可调节高度 */
.editor-section {
  display: flex;
  flex-direction: column;
  border-bottom: 1px solid var(--color-border);
  min-height: 100px;
  max-height: 500px;
}

.editor-container {
  flex: 1;
  min-height: 80px;
  position: relative;
  overflow: visible;
}

.sql-textarea {
  width: 100%;
  height: 100%;
  padding: 16px;
  border: none;
  resize: none;
  font-family: 'Consolas', 'Monaco', 'Courier New', monospace;
  font-size: 14px;
  line-height: 1.5;
  tab-size: 2;
  outline: none;
  background: var(--color-background);
  color: var(--color-text);
}

/* 拖拽分隔条 */
.resize-handle {
  height: 6px;
  background: var(--color-border);
  cursor: row-resize;
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background 0.2s;
}

.resize-handle:hover {
  background: var(--color-primary);
}

/* 分隔条中间的小横线 */
.resize-handle::before {
  content: '';
  width: 20px;
  height: 2px;
  background: var(--color-text-secondary);
  opacity: 0.5;
  border-radius: 1px;
}

.resize-handle:hover::before {
  background: white;
  opacity: 0.8;
}

.sql-textarea:focus {
  background: var(--color-background-soft);
}

.sql-textarea::placeholder {
  color: var(--color-text-secondary);
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
  background: rgba(244, 67, 54, 0.1);
}

.result-header {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 16px;
  background: var(--color-background-soft);
  border-bottom: 1px solid var(--color-border);
  flex-shrink: 0;
}

.success-icon {
  color: var(--color-success);
}

.error-icon {
  color: var(--color-danger);
}

.loading-icon {
  color: var(--color-primary);
  animation: spin 1s linear infinite;
}

@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

.execution-info {
  color: var(--color-text-secondary);
  font-size: 12px;
  margin-left: auto;
}

.btn-export {
  background: var(--color-background);
  border: 1px solid var(--color-border);
  color: var(--color-text);
  padding: 4px 12px;
  border-radius: 4px;
  cursor: pointer;
  font-size: 12px;
}

.btn-export:hover:not(:disabled) {
  background: var(--color-background-hover);
}

.error-message {
  padding: 16px;
  color: var(--color-danger);
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
  width: auto;
  border-collapse: collapse;
  table-layout: fixed;
}

.result-table th,
.result-table td {
  padding: 8px 12px;
  text-align: left;
  border-bottom: 1px solid var(--color-border);
  border-right: 1px solid var(--color-border);
  font-size: 13px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  height: 36px;
  line-height: 20px;
  vertical-align: middle;
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
  background: var(--color-background-soft);
  font-weight: 500;
  position: sticky;
  top: 0;
  z-index: 1;
}

.result-table th:last-child,
.result-table td:last-child {
  border-right: none;
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
  font-size: 11px;
  color: var(--color-text-secondary);
  font-weight: normal;
}

.result-table tr:hover {
  background: var(--color-background-hover);
}

.result-table td.is-null {
  color: var(--color-text-secondary);
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
  padding: 40px;
  text-align: center;
  color: var(--color-text-secondary);
}

.result-warning {
  padding: 10px 16px;
  background: var(--warning-50, rgba(245, 158, 11, 0.12));
  border-bottom: 1px solid var(--warning-500, #f59e0b);
  color: var(--warning-700, #b45309);
  font-size: 13px;
  flex-shrink: 0;
}

/* 补全弹窗 */
.completion-popup {
  position: absolute;
  z-index: 60;
  width: 280px;
  max-height: 220px;
  overflow-y: auto;
  background: var(--color-background);
  border: 1px solid var(--color-border);
  border-radius: 6px;
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.18);
  padding: 4px;
  font-family: 'Consolas', 'Monaco', 'Courier New', monospace;
}

.completion-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 8px;
  border-radius: 4px;
  font-size: 12px;
  cursor: pointer;
  white-space: nowrap;
}

.completion-item.active {
  background: var(--color-primary);
  color: white;
}

.completion-label {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
}

.completion-kind {
  font-size: 10px;
  padding: 1px 5px;
  border-radius: 3px;
  background: var(--color-background-soft);
  color: var(--color-text-secondary);
  flex-shrink: 0;
}

.completion-item.active .completion-kind {
  background: rgba(255, 255, 255, 0.24);
  color: white;
}

.completion-detail {
  font-size: 10px;
  color: var(--color-text-secondary);
  flex-shrink: 0;
}

.completion-item.active .completion-detail {
  color: rgba(255, 255, 255, 0.8);
}

/* 查询历史 / 已保存查询面板 */
.library-panel {
  flex: 0 0 220px;
  display: flex;
  flex-direction: column;
  border-top: 1px solid var(--color-border);
  overflow: hidden;
}

.library-header {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  background: var(--color-background-soft);
  border-bottom: 1px solid var(--color-border);
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
  color: var(--color-text-secondary);
  cursor: pointer;
}

.library-tab:hover {
  background: var(--color-background-hover);
}

.library-tab.active {
  background: var(--color-background);
  color: var(--color-text);
  font-weight: 500;
  box-shadow: inset 0 0 0 1px var(--color-border);
}

.library-search {
  flex: 1;
  min-width: 0;
  padding: 5px 8px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  font-size: 12px;
  background: var(--color-background);
  color: var(--color-text);
}

.library-search:focus {
  outline: none;
  border-color: var(--color-primary);
}

.library-empty {
  padding: 24px 16px;
  text-align: center;
  font-size: 12px;
  color: var(--color-text-secondary);
}

.library-folder {
  padding: 6px 16px;
  font-size: 11px;
  font-weight: 500;
  color: var(--color-text-secondary);
  background: var(--color-background-soft);
  border-bottom: 1px solid var(--color-border);
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
  color: var(--color-danger);
  border: 1px solid currentColor;
  border-radius: 3px;
  padding: 0 4px;
  margin-right: 8px;
  flex-shrink: 0;
}

.btn-item-delete {
  background: none;
  border: none;
  color: var(--color-text-secondary);
  cursor: pointer;
  font-size: 14px;
  line-height: 1;
  padding: 0 2px;
  margin-left: 8px;
  flex-shrink: 0;
}

.btn-item-delete:hover {
  color: var(--color-danger);
}

.btn-clear-history {
  background: none;
  border: none;
  color: var(--color-text-secondary);
  cursor: pointer;
  font-size: 12px;
  padding: 4px;
  display: flex;
  align-items: center;
}

.btn-clear-history:hover {
  color: var(--color-danger);
}

.btn-clear-history.block {
  display: block;
  width: 100%;
  padding: 8px;
  border-top: 1px solid var(--color-border);
  font-size: 12px;
}

.btn-format.is-active {
  background: var(--color-background-hover);
  border-color: var(--color-primary);
  color: var(--color-primary);
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
  border-bottom: 1px solid var(--color-border);
  font-size: 12px;
}

.history-item:hover {
  background: var(--color-background-hover);
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
  color: var(--color-text-secondary);
  font-size: 11px;
}
</style>
