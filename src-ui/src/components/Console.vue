<template>
  <div class="console-wrapper" @click="focusInput">
    <!-- 终端输出区域 -->
    <div ref="outputRef" class="console-output">
      <div
        v-for="(entry, index) in entries"
        :key="index"
        class="console-entry"
        :class="entry.type"
      >
        <!-- 输入命令 -->
        <div v-if="entry.type === 'input'" class="entry-input">
          <span class="prompt">{{ getPrompt(entry.database) }}</span>
          <span class="command">{{ entry.command }}</span>
        </div>

        <!-- 结果表格（ASCII 风格，支持自动换行） -->
        <div v-else-if="entry.type === 'table'" class="entry-table">
          <!-- 特殊处理 SHOW CREATE TABLE - 格式化显示 -->
          <div v-if="isShowCreateTable(entry)" class="create-table-output">
            <pre class="formatted-sql">{{ formatCreateTable(entry) }}</pre>
          </div>
          <div v-else class="ascii-table">
            <!-- 表头上边框 -->
            <div class="ascii-border">{{ getTopBorder(entry.columns) }}</div>
            <!-- 列名 -->
            <div class="ascii-row ascii-header">
              <span v-for="col in entry.columns" :key="col.name" class="ascii-cell">
                {{ padCenter(col.name, col.maxWidth) }}
              </span>
            </div>
            <!-- 表头下边框 -->
            <div class="ascii-border">{{ getMidBorder(entry.columns) }}</div>
            <!-- 数据行（支持自动换行） -->
            <template v-for="(wrappedRow, rowIdx) in wrapRows(entry.rows, entry.columns)" :key="rowIdx">
              <div class="ascii-row">
                <span v-for="(cell, cellIdx) in wrappedRow" :key="cellIdx" class="ascii-cell" :title="cell.fullText">
                  {{ padRight(cell.text, entry.columns[cellIdx]?.maxWidth || 10) }}
                </span>
              </div>
            </template>
            <!-- 表格下边框 -->
            <div class="ascii-border">{{ getBottomBorder(entry.columns) }}</div>
          </div>
          <!-- 统计信息 -->
          <div class="result-stats">
            {{ t('console.rowCountInSet', { count: entry.rowCount, time: formatTime(entry.executionTime) }) }}
          </div>
          <!-- 结果集截断提示 -->
          <div v-if="entry.warning" class="result-warning">
            {{ entry.warning }}
          </div>
        </div>

        <!-- 消息结果 -->
        <div v-else-if="entry.type === 'message'" class="entry-message">
          <div class="message-text">{{ entry.message }}</div>
          <div v-if="entry.executionTime !== undefined" class="result-stats">
            ({{ formatTime(entry.executionTime) }})
          </div>
        </div>

        <!-- 错误 -->
        <div v-else-if="entry.type === 'error'" class="entry-error">
          <div class="error-text">ERROR: {{ entry.message }}</div>
        </div>
      </div>

      <!-- 执行中提示 -->
      <div v-if="running" class="console-entry running-entry">
        <span class="running-text">{{ t('console.running') }}</span>
        <button class="btn-cancel" @click="cancelQuery">{{ t('common.cancel') }}</button>
      </div>

      <!-- 当前输入行 -->
      <div class="console-entry input-entry">
        <span class="prompt">{{ currentPrompt }}</span>
        <input
          ref="inputRef"
          v-model="currentCommand"
          class="console-input"
          spellcheck="false"
          @keydown="handleKeydown"
          placeholder=""
        />
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, nextTick, onMounted, watch } from 'vue'
import { useConnectionStore } from '@/stores/connection'
import { useDataBrowserStore } from '@/stores/dataBrowser'
import { useQueryLibraryStore } from '@/stores/queryLibrary'
import { useSqlCompletion, invalidateSchemaCache } from '@/composables/useSqlCompletion'
import { findDangerousStatements } from '@/utils/sql'
import { globalToast } from '@/composables/useToast'
import { confirmDanger } from '@/composables/useConfirm'
import { t } from '@/i18n'
import { resolveTabConnectionId } from '@/utils/tabConnection'

// Props
const props = defineProps<{
  tabId?: string
  /** 本 Tab 绑定的连接 id；为空时回退全局当前连接 */
  connectionId?: string
}>()

interface ColumnInfo {
  name: string
  dataType: string
  maxWidth: number
}

interface TableEntry {
  type: 'table'
  database: string
  command: string
  columns: ColumnInfo[]
  rows: unknown[][]
  rowCount: number
  executionTime: number
  warning?: string
}

interface MessageEntry {
  type: 'message'
  database: string
  command: string
  message: string
  executionTime?: number
}

interface ErrorEntry {
  type: 'error'
  database: string
  command: string
  message: string
}

interface InputEntry {
  type: 'input'
  database: string
  command: string
}

type ConsoleEntry = InputEntry | TableEntry | MessageEntry | ErrorEntry

// 判断是否是 SHOW CREATE TABLE 结果
function isShowCreateTable(entry: TableEntry): boolean {
  return entry.command.toLowerCase().includes('show create table')
}

// 格式化 CREATE TABLE 语句，按字段分行显示
function formatCreateTable(entry: TableEntry): string {
  if (entry.rows.length === 0) return ''
  
  // 找到 Create Table 列的索引
  const createTableColIndex = entry.columns.findIndex(col => 
    col.name.toLowerCase() === 'create table'
  )
  
  if (createTableColIndex === -1) return ''
  
  const result: string[] = []
  
  for (const row of entry.rows) {
    const cell = row[createTableColIndex]
    const sql = formatCell(cell)
    
    // 格式化 SQL：在关键位置添加换行
    const formatted = sql
      // 1. CREATE TABLE 表名后换行并添加 (
      .replace(/CREATE TABLE `([^`]+)`\s*\(/i, 'CREATE TABLE `$1` (\n  ')
      // 2. 字段定义之间的逗号后换行（匹配 `字段名` 类型...）
      // 使用非贪婪匹配，避免跨越多行
      .replace(/,\s*(`[\w_]+`\s+(?:int|tinyint|smallint|bigint|varchar|char|text|datetime|timestamp|date|time|decimal|float|double|json|binary|varbinary|blob|mediumtext|longtext|enum|set)(?:\([^)]*\))?(?:\s+unsigned)?(?:\s+NOT\s+NULL|\s+NULL)?(?:\s+DEFAULT\s+[^,]+)?(?:\s+AUTO_INCREMENT)?(?:\s+COMMENT\s+'[^']*')?)/gi, ',\n  $1')
      // 3. 在索引定义前换行（单换行）- 索引定义保持在一行
      .replace(/,\s*(PRIMARY\s+KEY\s+\([^)]+\)|UNIQUE\s+KEY\s+`[^`]+`\s+\([^)]+\)|KEY\s+`[^`]+`\s+\([^)]+\))/gi, ',\n  $1')
      // 4. 在最后一个右括号前换行（表定义结束）
      .replace(/\s*\)\s*ENGINE=/i, '\n)\nENGINE=')
      // 5. 表选项换行
      .replace(/\s+(COLLATE=)/i, '\nCOLLATE=')
      .replace(/\s+(DEFAULT\s+CHARSET=)/i, '\nDEFAULT CHARSET=')
      .replace(/\s+(AUTO_INCREMENT=)/i, '\nAUTO_INCREMENT=')
    
    result.push(formatted)
  }
  
  return result.join('\n\n')
}

const connectionStore = useConnectionStore()
const browserStore = useDataBrowserStore()
const queryLibrary = useQueryLibraryStore()
const completionEngine = useSqlCompletion()

const entries = ref<ConsoleEntry[]>([])
const running = ref(false)
const currentCommand = ref('')
const inputRef = ref<HTMLInputElement | null>(null)
const outputRef = ref<HTMLElement | null>(null)

// 命令历史
const commandHistory = ref<string[]>([])
const historyIndex = ref(-1)

// 多行命令缓冲
const multiLineBuffer = ref('')
const isInMultiLine = ref(false)

// 本 Tab 实际使用的连接 id：Tab 绑定的连接优先（prop），未绑定时回退全局当前连接
const effectiveConnectionId = computed(() =>
  resolveTabConnectionId({ connectionId: props.connectionId }, connectionStore.currentConnection?.id)
)

// 连接对象：优先按 id 从活跃连接中查找，连接已断开时回退全局当前连接
const effectiveConnection = computed(
  () =>
    connectionStore.activeConnections.find(conn => conn.id === effectiveConnectionId.value) ??
    connectionStore.currentConnection,
)

// 用户通过 USE 手动切换的库（优先于侧栏正在浏览的库）
const manualDb = ref('')

// 控制台当前库：USE 手动切换 > 侧栏浏览的库 > 连接自身的默认库。
// 三者都为空说明用户还没选库，此时不能回落到系统库 mysql，
// 否则破坏性语句可能直接落在系统库上。
const currentDb = computed(
  () =>
    manualDb.value ||
    browserStore.currentDatabase ||
    effectiveConnection.value?.database ||
    ''
)

// 切换连接时清空手动切换的库，避免沿用上一条连接的库名
watch(effectiveConnectionId, () => {
  manualDb.value = ''
})

// 补全所需的库名列表与 schema 快照
const databaseNames = computed(() => browserStore.objectTree.map(db => db.name))

watch(
  [effectiveConnectionId, currentDb],
  ([connectionId, database]) => {
    if (connectionId && database) {
      completionEngine.loadSchema(connectionId, database)
    }
  },
  { immediate: true }
)

const currentPrompt = computed(() => {
  const db = isInMultiLine.value ? '    ' : currentDb.value
  return `mysql [${db}]> `
})

function getPrompt(database: string) {
  return `mysql [${database || 'none'}]> `
}

function formatTime(ms: number): string {
  if (ms < 1000) {
    return `${ms.toFixed(2)} ms`
  }
  return `${(ms / 1000).toFixed(2)} sec`
}

function formatCell(cell: unknown): string {
  if (cell === null || cell === undefined) {
    return 'NULL'
  }
  return String(cell)
}

// 将文本按最大宽度换行
function wrapText(text: string, maxWidth: number): string[] {
  if (text.length <= maxWidth) {
    return [text]
  }

  const lines: string[] = []
  let remaining = text

  while (remaining.length > 0) {
    // 尝试在单词边界处换行
    let cutPoint = maxWidth
    if (remaining.length > maxWidth) {
      // 查找最后一个空格位置
      const lastSpace = remaining.lastIndexOf(' ', maxWidth)
      if (lastSpace > 0 && lastSpace > maxWidth * 0.5) {
        cutPoint = lastSpace
      }
    }
    lines.push(remaining.substring(0, cutPoint))
    remaining = remaining.substring(cutPoint).trimStart()
  }

  return lines.length > 0 ? lines : ['']
}

interface WrappedCell {
  text: string
  fullText: string
}

type WrappedRow = WrappedCell[]

// 包装行数据，支持自动换行
function wrapRows(rows: unknown[][], columns: ColumnInfo[]): WrappedRow[] {
  if (rows.length === 0) return []

  const result: WrappedRow[] = []

  for (let i = 0; i < rows.length; i++) {
    const originalRow = rows[i]
    if (!originalRow) continue

    // 将每个单元格内容按列宽换行
    const wrappedRow: string[][] = []
    let maxLines = 1

    for (let colIdx = 0; colIdx < originalRow.length; colIdx++) {
      const cell = originalRow[colIdx]
      const text = formatCell(cell)
      const maxWidth = columns[colIdx]?.maxWidth || 50
      const lines = wrapText(text, maxWidth)
      wrappedRow[colIdx] = lines
      maxLines = Math.max(maxLines, lines.length)
    }

    // 生成展开后的行
    for (let lineIdx = 0; lineIdx < maxLines; lineIdx++) {
      const newRow: WrappedCell[] = []
      for (let colIdx = 0; colIdx < originalRow.length; colIdx++) {
        const lines = wrappedRow[colIdx]
        const lineText = lines && lineIdx < lines.length ? lines[lineIdx] : ''
        const fullText = formatCell(originalRow[colIdx])
        newRow.push({
          text: lineText || '',
          fullText: fullText || ''
        })
      }
      result.push(newRow)
    }
  }

  return result
}

// 计算列的最大宽度
function calculateColumnWidths(columns: { name: string; data_type: string }[], rows: unknown[][]): ColumnInfo[] {
  const infos: ColumnInfo[] = columns.map(col => ({
    name: col.name,
    dataType: col.data_type,
    maxWidth: Math.max(col.name.length, 10) // 最小宽度 10
  }))

  // 遍历所有数据行，计算每列最大宽度
  for (const row of rows) {
    row.forEach((cell, idx) => {
      const cellStr = formatCell(cell)
      if (infos[idx]) {
        infos[idx].maxWidth = Math.min(Math.max(infos[idx].maxWidth, cellStr.length), 50) // 最大限制 50
      }
    })
  }

  return infos
}

// ASCII 表格边框
function getTopBorder(columns: ColumnInfo[]): string {
  return '+' + columns.map(col => '-'.repeat(col.maxWidth + 2)).join('+') + '+'
}

function getMidBorder(columns: ColumnInfo[]): string {
  return '+' + columns.map(col => '-'.repeat(col.maxWidth + 2)).join('+') + '+'
}

function getBottomBorder(columns: ColumnInfo[]): string {
  return '+' + columns.map(col => '-'.repeat(col.maxWidth + 2)).join('+') + '+'
}

// 文本对齐
function padCenter(str: string, width: number): string {
  const len = str.length
  if (len >= width) return str
  const left = Math.floor((width - len) / 2)
  const right = width - len - left
  return ' '.repeat(left) + str + ' '.repeat(right)
}

function padRight(str: string, width: number): string {
  const len = str.length
  if (len >= width) return str
  return str + ' '.repeat(width - len)
}

// 滚动到底部
async function scrollToBottom() {
  await nextTick()
  if (outputRef.value) {
    outputRef.value.scrollTop = outputRef.value.scrollHeight
  }
}

// 聚焦输入框
function focusInput() {
  inputRef.value?.focus()
}

// 执行命令
async function executeCommand() {
  const command = currentCommand.value.trim()
  if (!command) return

  const db = currentDb.value

  // 记录输入
  entries.value.push({
    type: 'input',
    database: db,
    command: command
  })

  // 保存到历史
  if (commandHistory.value[commandHistory.value.length - 1] !== command) {
    commandHistory.value.push(command)
  }
  historyIndex.value = -1

  // 清空输入
  currentCommand.value = ''

  // 处理特殊命令
  if (command.toLowerCase() === 'clear' || command.toLowerCase() === 'cls') {
    entries.value = []
    await scrollToBottom()
    return
  }

  // 检查是否为多行命令（以 \ 结尾）
  if (command.endsWith('\\')) {
    multiLineBuffer.value += command.slice(0, -1) + '\n'
    isInMultiLine.value = true
    await scrollToBottom()
    return
  }

  // 合并多行缓冲
  const fullCommand = multiLineBuffer.value + command
  if (isInMultiLine.value) {
    multiLineBuffer.value = ''
    isInMultiLine.value = false
  }

  // 检查 SQL 完整性
  if (!fullCommand.endsWith(';')) {
    multiLineBuffer.value = fullCommand + ' '
    isInMultiLine.value = true
    await scrollToBottom()
    return
  }

  // 安全模式下对高风险语句二次确认（无 WHERE 的 UPDATE/DELETE 由后端直接拦截）
  if (effectiveConnection.value?.safety_mode === 'safe') {
    const dangerous = findDangerousStatements(fullCommand)
    if (
      dangerous.length > 0 &&
      !(await confirmDanger({
        message: t('console.safetyConfirm', { statements: dangerous.join(', ') }),
        confirmText: t('common.confirm'),
      }))
    ) {
      await scrollToBottom()
      return
    }
  }

  // 处理 USE 命令（切换当前数据库）
  const useMatch = fullCommand.match(/^use\s+([^;]+);$/i)
  if (useMatch && useMatch[1]) {
    const newDb = useMatch[1].trim().replace(/^`|`$/g, '') // 去除首尾的反引号

    if (!effectiveConnection.value) {
      entries.value.push({
        type: 'error',
        database: db,
        command: fullCommand,
        message: t('console.notConnected')
      })
      await scrollToBottom()
      return
    }

    // 先发送到后端执行 USE 语句
    try {
      await connectionStore.executeSql(effectiveConnection.value.id, newDb, fullCommand)
      // 后端执行成功后，更新前端当前数据库
      manualDb.value = newDb
      recordHistory(fullCommand, newDb, true)
      entries.value.push({
        type: 'message',
        database: newDb,
        command: fullCommand,
        message: t('console.databaseChanged', { database: newDb }),
        executionTime: 0
      })
    } catch (err: unknown) {
      recordHistory(fullCommand, db, false, undefined, String(err))
      entries.value.push({
        type: 'error',
        database: db,
        command: fullCommand,
        message: String(err)
      })
    }
    await scrollToBottom()
    return
  }

  // 执行 SQL
  if (!effectiveConnection.value) {
    entries.value.push({
      type: 'error',
      database: db,
      command: fullCommand,
      message: t('console.notConnected')
    })
    await scrollToBottom()
    return
  }

  // 没有可用的库时不要擅自回落到系统库（MySQL 必须显式指定库）
  if (!db && effectiveConnection.value.driver !== 'postgres') {
    entries.value.push({
      type: 'error',
      database: db,
      command: fullCommand,
      message: t('console.noDatabase')
    })
    await scrollToBottom()
    return
  }

  const startTime = performance.now()
  running.value = true

  try {
    const response = await connectionStore.executeSql(effectiveConnection.value.id, db, fullCommand) as {
      columns: { name: string; data_type: string }[]
      rows: unknown[][]
      execution_time_ms: number
      rows_affected?: number
      rows_returned: number
      truncated?: boolean
      warning?: string
    }
    const executionTime = performance.now() - startTime
    recordHistory(fullCommand, db, true, executionTime)

    // DDL 后补全快照失效
    if (/^\s*(create|alter|drop|rename|truncate)\b/i.test(fullCommand)) {
      invalidateSchemaCache()
      completionEngine.loadSchema(effectiveConnectionId.value, currentDb.value)
    }

    if (response.rows && response.rows.length > 0) {
      // 表格结果
      const columns = calculateColumnWidths(response.columns, response.rows)
      entries.value.push({
        type: 'table',
        database: db,
        command: fullCommand,
        columns,
        rows: response.rows,
        rowCount: response.rows.length,
        executionTime,
        warning: response.warning
      })
    } else if (response.columns && response.columns.length > 0) {
      // SELECT 返回 0 行
      entries.value.push({
        type: 'message',
        database: db,
        command: fullCommand,
        message: t('console.emptySet'),
        executionTime
      })
    } else {
      // 非查询语句（DML/DDL）
      entries.value.push({
        type: 'message',
        database: db,
        command: fullCommand,
        message: response.rows_affected != null
          ? t('console.queryOkAffected', { count: response.rows_affected })
          : t('console.queryOk'),
        executionTime
      })
    }
  } catch (err: unknown) {
    const message = String(err)
    if (message.includes('用户取消')) {
      entries.value.push({
        type: 'message',
        database: db,
        command: fullCommand,
        message: t('console.queryCancelled')
      })
    } else {
      recordHistory(fullCommand, db, false, undefined, message)
      entries.value.push({
        type: 'error',
        database: db,
        command: fullCommand,
        message
      })
    }
  } finally {
    running.value = false
  }

  await scrollToBottom()
}

// 取消执行
async function cancelQuery() {
  if (!effectiveConnection.value) return
  try {
    const rolledBack = await connectionStore.cancelQuery(effectiveConnection.value.id)
    // 取消会把事务连接一并丢弃（后端自动回滚），必须提示用户
    if (rolledBack) {
      globalToast.warning(t('console.transactionRolledBack'))
    }
  } catch (e) {
    console.error('取消查询失败:', e)
  }
}

// 键盘处理
function handleKeydown(e: KeyboardEvent) {
  if (e.ctrlKey && e.code === 'Space') {
    e.preventDefault()
    autocomplete()
  } else if (e.key === 'Enter') {
    e.preventDefault()
    executeCommand()
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    navigateHistory(-1)
  } else if (e.key === 'ArrowDown') {
    e.preventDefault()
    navigateHistory(1)
  } else if (e.key === 'Tab') {
    e.preventDefault()
    autocomplete()
  }
}

// 历史导航
function navigateHistory(direction: number) {
  if (commandHistory.value.length === 0) return

  historyIndex.value += direction

  if (historyIndex.value < 0) {
    historyIndex.value = -1
    currentCommand.value = ''
  } else if (historyIndex.value >= commandHistory.value.length) {
    historyIndex.value = commandHistory.value.length - 1
  }

  if (historyIndex.value >= 0 && historyIndex.value < commandHistory.value.length) {
    const historyItem = commandHistory.value[commandHistory.value.length - 1 - historyIndex.value]
    if (historyItem) {
      currentCommand.value = historyItem
    }
  }
}

function recordHistory(
  command: string,
  database: string,
  success: boolean,
  durationMs?: number,
  error?: string
) {
  const connectionId = effectiveConnectionId.value
  if (!connectionId) return
  queryLibrary.addHistory({ sql: command, connectionId, database, success, durationMs, error })
}

// 自动补全：与 SQL 编辑器共用同一套 schema 感知引擎，Tab 接受第一个候选
function autocomplete() {
  const el = inputRef.value
  const value = currentCommand.value
  const caret = el?.selectionStart ?? value.length

  const result = completionEngine.complete(value, caret, databaseNames.value, currentDb.value)
  const item = result.items[0]
  if (!item) return

  currentCommand.value = value.slice(0, result.replaceStart) + item.label + value.slice(result.replaceEnd)

  nextTick(() => {
    const position = result.replaceStart + item.label.length
    el?.focus()
    el?.setSelectionRange(position, position)
  })
}

onMounted(() => {
  // 欢迎消息
  entries.value.push({
    type: 'message',
    database: '',
    command: '',
    message: t('console.welcome'),
    executionTime: undefined
  })

  focusInput()
})
</script>

<style scoped>
.console-wrapper {
  height: 100%;
  background: var(--canvas);
  color: var(--text-primary);
  font-family: var(--font-mono);
  font-size: var(--text-sm);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.console-output {
  flex: 1;
  padding: var(--space-2) var(--space-3);
  overflow-y: auto;
  overflow-x: auto;
  white-space: pre;
}

.console-entry {
  margin-bottom: 4px;
}

.entry-input {
  color: var(--text-primary);
}

.prompt {
  color: var(--accent);
  font-weight: var(--font-medium);
}

.command {
  color: var(--text-primary);
  font-size: inherit;
}

.input-entry {
  display: flex;
  align-items: center;
}

.console-input {
  flex: 1;
  background: transparent;
  border: none;
  outline: none;
  color: var(--text-primary);
  font-family: inherit;
  font-size: inherit;
  padding: 0;
  margin: 0;
  caret-color: var(--accent);
}

/* 表格样式 */
.entry-table {
  margin: var(--space-2) 0;
}

.ascii-table {
  display: inline-block;
}

/* SHOW CREATE TABLE 格式化输出 */
.create-table-output {
  margin: var(--space-2) 0;
  padding: var(--space-2) var(--space-3);
  background: var(--raised);
  border: 1px solid var(--hairline);
  border-radius: var(--radius-sm);
}

.formatted-sql {
  margin: 0;
  padding: 0;
  color: var(--text-primary);
  font-family: var(--font-mono);
  font-size: var(--text-xs);
  line-height: 1.5;
  white-space: pre-wrap;
  word-wrap: break-word;
}

.ascii-border {
  color: var(--text-tertiary);
}

.ascii-row {
  display: flex;
}

.ascii-header {
  font-weight: var(--font-semibold);
  color: var(--code-function);
}

.ascii-cell {
  padding: 0 var(--space-2);
  color: var(--code-string);
}

.result-stats {
  color: var(--code-comment);
  margin-top: 4px;
  font-size: var(--text-xs);
}

.result-warning {
  color: var(--warning-500);
  margin-top: 2px;
  font-size: var(--text-xs);
  white-space: pre-wrap;
}

/* 消息样式 */
.entry-message {
  color: var(--text-primary);
}

.message-text {
  white-space: pre-wrap;
}

/* 错误样式 */
.entry-error {
  color: var(--error-500);
}

.error-text {
  white-space: pre-wrap;
}

/* 执行中提示 */
.running-entry {
  color: var(--text-secondary);
  font-style: italic;
}

.running-text {
  animation: pulse 1.5s infinite;
}

.btn-cancel {
  margin-left: var(--space-2);
  height: var(--control-height-sm);
  padding: 0 var(--space-2);
  background: transparent;
  color: var(--error-500);
  border: none;
  border-radius: var(--radius-sm);
  cursor: pointer;
  font-size: var(--text-xs);
  font-style: normal;
  transition: var(--transition-colors);
}

.btn-cancel:hover {
  background: rgba(239, 68, 68, 0.12);
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.4; }
}

/* 滚动条 */
.console-output::-webkit-scrollbar {
  width: 8px;
  height: 8px;
}

.console-output::-webkit-scrollbar-track {
  background: transparent;
}

.console-output::-webkit-scrollbar-thumb {
  background: var(--border-default);
  border-radius: var(--radius-sm);
}

.console-output::-webkit-scrollbar-thumb:hover {
  background: var(--border-strong);
}
</style>
