<template>
  <div class="main-content-wrapper" :style="{ width: width + 'px' }">
    <div class="main-content">
      <template v-if="currentConnection">
        <!-- Tab 栏 -->
        <div class="tab-bar">
          <!-- 对象列表 Tab（始终存在） -->
          <div
            class="tab-item"
            :class="{ active: activeTab?.type === 'objects' }"
            @click="switchTab('objects')"
          >
            <Database :size="14" />
            <span>{{ t('layout.objectsTab') }}</span>
          </div>

          <!-- 数据表 Tabs -->
          <div
            v-for="tab in tableTabs"
            :key="tab.id"
            class="tab-item"
            :class="{ active: activeTab?.id === tab.id }"
            @click="switchTab(tab.id)"
            @mousedown.middle="closeTab(tab.id)"
          >
            <Table2 :size="14" />
            <span class="tab-title">{{ tab.tableName }}</span>
            <span class="tab-close" :title="t('common.close')" @click.stop="closeTab(tab.id)">
              <X :size="12" />
            </span>
          </div>

          <!-- SQL 查询 Tabs -->
          <div
            v-for="tab in sqlTabs"
            :key="tab.id"
            class="tab-item"
            :class="{ active: activeTab?.id === tab.id }"
            @click="switchTab(tab.id)"
            @mousedown.middle="closeTab(tab.id)"
          >
            <FileCode :size="14" />
            <span class="tab-title">{{ tab.title }}</span>
            <span class="tab-close" :title="t('common.close')" @click.stop="closeTab(tab.id)">
              <X :size="12" />
            </span>
          </div>

          <!-- 控制台 Tabs -->
          <div
            v-for="tab in consoleTabs"
            :key="tab.id"
            class="tab-item"
            :class="{ active: activeTab?.id === tab.id }"
            @click="switchTab(tab.id)"
            @mousedown.middle="closeTab(tab.id)"
          >
            <Terminal :size="14" />
            <span class="tab-title">{{ tab.title }}</span>
            <span class="tab-close" :title="t('common.close')" @click.stop="closeTab(tab.id)">
              <X :size="12" />
            </span>
          </div>

          <!-- ER 图 Tabs -->
          <div
            v-for="tab in erTabs"
            :key="tab.id"
            class="tab-item"
            :class="{ active: activeTab?.id === tab.id }"
            @click="switchTab(tab.id)"
            @mousedown.middle="closeTab(tab.id)"
          >
            <Network :size="14" />
            <span class="tab-title">{{ t('erDiagram.title') }} · {{ tab.database.split('.').pop() }}</span>
            <span class="tab-close" :title="t('common.close')" @click.stop="closeTab(tab.id)">
              <X :size="12" />
            </span>
          </div>
        </div>

        <!-- 内容区域 -->
        <div class="content-area">
          <!-- 数据表视图（常驻挂载，用 v-show 隐藏而非销毁，避免切换视图时状态丢失与重复加载） -->
          <div v-show="activeTab?.type === 'table'" class="table-view">
            <DataTable ref="dataTableRef" @table-closed="onTableClosed" />
          </div>

          <!-- 对象列表视图 -->
          <div v-if="activeTab?.type === 'objects'" class="objects-view">
            <div class="objects-header">
    <h3>{{ selectedDatabase ? selectedDatabase.split('.').pop() : t('layout.noDatabaseSelected') }}</h3>
  </div>
            <div class="objects-list" v-if="selectedDatabase">
              <div class="objects-table">
                <div class="objects-row objects-head">
                  <span class="col-name">{{ t('layout.tableNameColumn') }}</span>
                  <span class="col-auto-inc">{{ t('layout.autoIncrementColumn') }}</span>
                  <span class="col-updated">{{ t('layout.modifiedAtColumn') }}</span>
                  <span class="col-size">{{ t('layout.tableSizeColumn') }}</span>
                </div>
                <div
                  v-for="table in tableObjects"
                  :key="table.id"
                  class="objects-row object-item"
                  :class="{ selected: selectedTableNames.includes(table.name) }"
                  @click="onObjectClick($event, table.name)"
                  @dblclick="emit('openTable', table.name)"
                  @contextmenu.prevent="onObjectContextMenu($event, table.name)"
                >
                  <span class="col-name">
                    <Table2 :size="16" />
                    <span class="name-text" :title="table.name">{{ table.name }}</span>
                  </span>
                  <span class="col-auto-inc">{{ table.auto_increment ?? '—' }}</span>
                  <span class="col-updated">{{ table.updated_at || '—' }}</span>
                  <span class="col-size">{{ formatBytes(table.size_bytes) }}</span>
                </div>
              </div>
              <div v-if="tableObjects.length === 0" class="objects-empty">
              {{ t('layout.noTables') }}
            </div>
            </div>
            <div v-else class="objects-empty">
              {{ t('layout.selectDatabaseFirst') }}
            </div>
          </div>

          <!-- SQL 编辑器视图 -->
          <div v-else-if="activeTab?.type === 'sql'" class="sql-view">
            <!-- 用 tab id 作为 key：切换 SQL Tab 时重新初始化编辑器，避免上一个 Tab 的 SQL/结果串到新 Tab -->
            <SqlEditor :key="activeTab.id" :tabId="activeTab.id" :connection-id="(activeTab as SqlTab).connectionId" />
          </div>

          <!-- 控制台视图 -->
          <div v-else-if="activeTab?.type === 'console'" class="console-view">
            <Console :tabId="activeTab.id" :connection-id="(activeTab as ConsoleTab).connectionId" />
          </div>

          <!-- ER 图视图 -->
          <div v-else-if="activeTab?.type === 'er'" class="er-view">
            <component
              :is="ProUi.ErDiagram"
              v-if="ProUi.ErDiagram"
              :database="(activeTab as ErTab).database.split('.').pop() || ''"
            />
          </div>

          <!-- 空状态 -->
          <div v-else-if="activeTab?.type !== 'table'" class="empty-view">
            <h2>{{ t('layout.welcomeTitle') }}</h2>
            <p>{{ t('layout.welcomeHint') }}</p>
          </div>
        </div>
      </template>

      <!-- 未连接状态 -->
      <template v-else>
        <div class="welcome">
          <h2>{{ t('layout.welcomeTitle') }}</h2>
          <p>{{ t('layout.welcomeSavedHint') }}</p>
        </div>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, nextTick, watch, onMounted, onUnmounted } from 'vue'
import { Database, Table2, FileCode, X, Terminal, Network } from 'lucide-vue-next'
import DataTable from '@/components/DataTable.vue'
import SqlEditor from '@/components/SqlEditor.vue'
import Console from '@/components/Console.vue'
import { ProUi } from '@/utils/pro'
import { useDataBrowserStore } from '@/stores/dataBrowser'
import { useEditorTabsStore } from '@/stores/editorTabs'
import { confirmDanger } from '@/composables/useConfirm'
import { globalToast } from '@/composables/useToast'
import { t } from '@/i18n'
import { useConnectionStore, type ConnectionStatus } from '@/stores/connection'
import type { DatabaseObject } from '@/stores/dataBrowser'
import { nextSqlTabId, nextTabCounter, resolveTabConnectionId } from '@/utils/tabConnection'

// Tab 类型定义
interface ObjectsTab {
  id: 'objects'
  type: 'objects'
}

interface TableTab {
  id: string
  type: 'table'
  connectionId: string
  database: string
  tableName: string
}

interface SqlTab {
  id: string
  type: 'sql'
  title: string
  /** 该 Tab 打开时绑定的连接 id（空串表示未绑定，回退全局当前连接） */
  connectionId: string
}

interface ConsoleTab {
  id: string
  type: 'console'
  title: string
  /** 该 Tab 打开时绑定的连接 id（空串表示未绑定，回退全局当前连接） */
  connectionId: string
}

interface ErTab {
  id: string
  type: 'er'
  database: string
}

type Tab = ObjectsTab | TableTab | SqlTab | ConsoleTab | ErTab

interface Props {
  width: number
  currentConnection: ConnectionStatus | null
  selectedDatabase: string
  connectionDatabaseTables: Record<string, Record<string, DatabaseObject[]>>
}

const props = defineProps<Props>()

const emit = defineEmits<{
  newQuery: []
  newConsole: []
  selectTable: [tableName: string]
  openTable: [tableName: string]
  // 对象列表右键菜单：与左侧连接树共用同一套表操作菜单
  // tableNames 为当前多选中的表（右键未选中行时仅含该行）
  showTableMenu: [event: MouseEvent, connectionId: string, database: string, tableName: string, tableNames?: string[]]
  // 表 Tab 被真正移除后通知父组件，用于同步右侧信息面板
  tableTabClosed: [tab: { connectionId: string; database: string; tableName: string }]
}>()

const dataTableRef = ref<InstanceType<typeof DataTable> | null>(null)

// Tabs 状态
const tabs = ref<Tab[]>([{ id: 'objects', type: 'objects' }])
const activeTabId = ref<string>('objects')
const sqlTabCounter = ref(1)
const consoleTabCounter = ref(1)

// 计算属性
const activeTab = computed(() => {
  return tabs.value.find(t => t.id === activeTabId.value) || null
})

const tableTabs = computed(() => {
  return tabs.value.filter(t => t.type === 'table') as TableTab[]
})

const sqlTabs = computed(() => {
  return tabs.value.filter(t => t.type === 'sql') as SqlTab[]
})

const consoleTabs = computed(() => {
  return tabs.value.filter(t => t.type === 'console') as ConsoleTab[]
})

const erTabs = computed(() => {
  return tabs.value.filter(t => t.type === 'er') as ErTab[]
})

// 当前数据库的表对象（含自增值、修改时间、占用空间，供对象列表展示）
const tableObjects = computed<DatabaseObject[]>(() => {
  if (!props.currentConnection || !props.selectedDatabase) return []
  // selectedDatabase 格式: "connectionId.dbName"
  const dbName = props.selectedDatabase.split('.').pop()
  if (!dbName) return []
  const tables = props.connectionDatabaseTables[props.currentConnection.id]?.[dbName]
  if (!tables) return []
  // 只保留类型为 Table 的对象（视图没有这些统计信息）
  return tables.filter(obj => obj.object_type === 'Table')
})

// 字节数转可读大小；无统计信息时显示占位符
function formatBytes(bytes?: number | null): string {
  if (bytes == null) return '—'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`
  return `${(bytes / 1024 / 1024 / 1024).toFixed(1)} GB`
}

// 对象列表多选：Ctrl/Cmd 点击切换单行，Shift 点击按上次点击位置连选
const selectedTableNames = ref<string[]>([])
// Shift 连选的起点（上次普通点击或 Ctrl 点击的行）
let selectionAnchor = ''

// 表列表刷新后剔除已不存在的表，避免选择残留
watch(tableObjects, tables => {
  const names = new Set(tables.map(t => t.name))
  selectedTableNames.value = selectedTableNames.value.filter(name => names.has(name))
})

function onObjectClick(event: MouseEvent, tableName: string) {
  const names = tableObjects.value.map(t => t.name)
  if (event.shiftKey && selectionAnchor && names.includes(selectionAnchor)) {
    const from = names.indexOf(selectionAnchor)
    const to = names.indexOf(tableName)
    const start = Math.min(from, to)
    const end = Math.max(from, to)
    selectedTableNames.value = names.slice(start, end + 1)
  } else if (event.ctrlKey || event.metaKey) {
    selectedTableNames.value = selectedTableNames.value.includes(tableName)
      ? selectedTableNames.value.filter(name => name !== tableName)
      : [...selectedTableNames.value, tableName]
    selectionAnchor = tableName
  } else {
    selectedTableNames.value = [tableName]
    selectionAnchor = tableName
  }

  // 选中（或被追加选中）时刷新右侧信息面板，取消选中则不动
  if (selectedTableNames.value.includes(tableName)) {
    emit('selectTable', tableName)
  }
}

// Ctrl/Cmd+A 全选当前库的所有表：仅在对象列表视图、焦点不在输入框时生效
function handleObjectsKeydown(event: KeyboardEvent) {
  if (activeTab.value?.type !== 'objects') return
  if (!(event.ctrlKey || event.metaKey) || event.key.toLowerCase() !== 'a') return
  if (tableObjects.value.length === 0) return
  const target = event.target as HTMLElement | null
  if (target?.closest('input, textarea, [contenteditable="true"]')) return
  // 弹窗打开时让出全选，避免抢走输入框/编辑器里的文本全选
  if (document.querySelector('.modal-overlay')) return
  event.preventDefault()
  selectedTableNames.value = tableObjects.value.map(t => t.name)
  selectionAnchor = selectedTableNames.value[0] ?? ''
}

onMounted(() => document.addEventListener('keydown', handleObjectsKeydown))
onUnmounted(() => document.removeEventListener('keydown', handleObjectsKeydown))

// 对象列表中的表右键菜单，交由外层复用左侧连接树的表菜单
function onObjectContextMenu(event: MouseEvent, tableName: string) {
  if (!props.currentConnection || !props.selectedDatabase) return
  const dbName = props.selectedDatabase.split('.').pop()
  if (!dbName) return
  // 右键未选中的行时改为单选，避免菜单里的单表操作误作用到其它选中的表
  if (!selectedTableNames.value.includes(tableName)) {
    selectedTableNames.value = [tableName]
    selectionAnchor = tableName
  }
  emit(
    'showTableMenu',
    event,
    props.currentConnection.id,
    dbName,
    tableName,
    [...selectedTableNames.value]
  )
}

// 方法
function switchTab(tabId: string) {
  // 激活 Tab 时同步全局连接（表/查询/控制台 Tab 各自绑定连接）
  setActiveTab(tabId)

  // 如果是表 Tab，通知 DataTable 切换
  const tab = tabs.value.find(t => t.id === tabId)
  if (tab?.type === 'table') {
    const tableTab = tab as TableTab
    dataTableRef.value?.openTable(tableTab.connectionId, tableTab.database, tableTab.tableName)
  }
}

const browserStore = useDataBrowserStore()
const editorTabs = useEditorTabsStore()
const connStore = useConnectionStore()

// 激活 Tab 时同步全局连接：Tab 绑定了连接就切回它自己的连接，
// 否则（对象 Tab / 旧数据无绑定）保持当前连接不动
function syncTabConnection(tab: Tab | null | undefined) {
  if (!tab || !('connectionId' in tab)) return
  const connectionId = resolveTabConnectionId(tab, props.currentConnection?.id)
  if (connectionId) connStore.switchConnection(connectionId)
}

// 统一的 Tab 激活入口：设置激活 id 并同步该 Tab 绑定的连接
function setActiveTab(tabId: string) {
  activeTabId.value = tabId
  syncTabConnection(tabs.value.find(t => t.id === tabId))
}

// 分配下一个 SQL Tab id：按现有 Tab（含启动时恢复的草稿）已用最大序号 +1，
// 避免新建 Tab 与恢复的 id 撞号；同时把计数器推进到下一个可用序号
function allocateSqlTabId(): { id: string; index: number } {
  const existingIds = tabs.value.map(t => t.id)
  const index = nextTabCounter('sql-', existingIds)
  sqlTabCounter.value = index + 1
  return { id: nextSqlTabId('sql-', existingIds), index }
}

function closeTab(tabId: string) {
  const tab = tabs.value.find(t => t.id === tabId)
  if (!tab) return

  // 如果是表 Tab，先让 DataTable 决定能否立即关闭：
  // 有未保存修改时它会弹确认框并保留该表，此时外层 Tab 也先不动，
  // 等确认关闭后由 table-closed 事件收尾，避免两边状态不同步
  if (tab.type === 'table') {
    const tableTab = tab as TableTab
    const internalTabId = `${tableTab.connectionId}.${tableTab.database}.${tableTab.tableName}`
    if (dataTableRef.value?.closeTableById(internalTabId) === false) return
  }

  // SQL Tab 有未保存内容时先确认，避免误关丢失草稿
  if (tab.type === 'sql' && editorTabs.tabs[tabId]?.dirty) {
    void confirmDanger({
      message: t('layout.closeDirtyQueryConfirm'),
      confirmText: t('layout.closeDirtyQuery'),
    }).then(confirmed => {
      if (confirmed) removeTab(tabId)
    })
    return
  }

  removeTab(tabId)
}

// 真正移除外层 Tab（表关闭可能延后到确认框结果之后）
function removeTab(tabId: string) {
  const index = tabs.value.findIndex(t => t.id === tabId)
  if (index === -1) return

  const tab = tabs.value[index]

  // 如果是 SQL Tab，清除缓存
  if (tab?.type === 'sql') {
    browserStore.clearQueryResult(tabId)
    editorTabs.remove(tabId)
  }

  // 表 Tab 关闭时通知父组件，父组件据此同步右侧信息面板
  if (tab?.type === 'table') {
    emit('tableTabClosed', {
      connectionId: tab.connectionId,
      database: tab.database,
      tableName: tab.tableName,
    })
  }

  tabs.value.splice(index, 1)

  // 如果关闭的是当前激活的 tab，切换到前一个 tab
  if (activeTabId.value === tabId) {
    const newIndex = Math.max(0, index - 1)
    setActiveTab(tabs.value[newIndex]?.id || 'objects')
  }
}

function onTableClosed(tab: { connectionId: string; database: string; tableName: string }) {
  removeTab(`table-${tab.connectionId}-${tab.database}-${tab.tableName}`)
}

// 打开表
async function openTable(connectionId: string, database: string, tableName: string) {
  const tabId = `table-${connectionId}-${database}-${tableName}`

  // 检查是否已打开
  const existingTab = tabs.value.find(t => t.id === tabId)
  if (existingTab) {
    setActiveTab(tabId)
    // 通知 DataTable 切换到这个表
    await nextTick()
    dataTableRef.value?.openTable(connectionId, database, tableName)
    return
  }

  // 创建新 tab
  const newTab: TableTab = {
    id: tabId,
    type: 'table',
    connectionId,
    database,
    tableName
  }
  tabs.value.push(newTab)
  setActiveTab(tabId)

  // 通知 DataTable 打开这个表（等待DOM更新）
  await nextTick()
  dataTableRef.value?.openTable(connectionId, database, tableName)
}

// 新建 SQL 查询
function newSqlQuery() {
  const { id: tabId, index } = allocateSqlTabId()
  const newTab: SqlTab = {
    id: tabId,
    type: 'sql',
    title: `Query ${index}`,
    // 记录创建时的连接，之后切到其它连接也不影响本 Tab 的执行目标
    connectionId: props.currentConnection?.id ?? '',
  }
  tabs.value.push(newTab)
  setActiveTab(tabId)
}

// 用 .sql 文件内容新建查询页（文件关联双击 / 拖拽打开共用）
function openSqlFile(fileName: string, content: string) {
  const { id: tabId, index } = allocateSqlTabId()
  const newTab: SqlTab = {
    id: tabId,
    type: 'sql',
    title: fileName || `Query ${index}`,
    connectionId: props.currentConnection?.id ?? '',
  }
  // 先写入内容再挂 Tab：SqlEditor 以 tab id 为 key 重新挂载时从 store 读取
  editorTabs.setSql(tabId, content)
  editorTabs.markClean(tabId)
  tabs.value.push(newTab)
  setActiveTab(tabId)
}

// 新建控制台
function newConsole() {
  const tabId = `console-${consoleTabCounter.value++}`
  const newTab: ConsoleTab = {
    id: tabId,
    type: 'console',
    title: `Console ${consoleTabCounter.value - 1}`,
    connectionId: props.currentConnection?.id ?? '',
  }
  tabs.value.push(newTab)
  setActiveTab(tabId)
}

// 打开 ER 图 tab（用当前选中的数据库）
function openErDiagramTab(database: string) {
  if (!database) {
    // 优先用 props.selectedDatabase（已连接但未从 header 传入 database 参数的兜底）
    const fallback = props.selectedDatabase
    if (fallback) database = fallback
  }
  if (!database) {
    globalToast.warning(t('erDiagram.needsDatabase'))
    return
  }
  const tabId = `er-${database}`
  const existingTab = tabs.value.find(t => t.id === tabId)
  if (existingTab) {
    setActiveTab(tabId)
    return
  }
  const newTab: ErTab = { id: tabId, type: 'er', database }
  tabs.value.push(newTab)
  setActiveTab(tabId)
}

// 启动时从 localStorage 恢复上次未关闭的 SQL tab（P0-3 草稿自动保存）
function restoreDrafts() {
  if (!props.currentConnection) return
  editorTabs.restore()
  const savedIds = editorTabs.restoredTabIds()
  if (savedIds.length === 0) return

  // 草稿里没有历史连接信息，恢复时统一绑定当前连接
  const connectionId = props.currentConnection?.id ?? ''
  // 避免重复恢复：tab 已存在就跳过
  const existingIds = new Set(tabs.value.map(t => t.id))
  let activated = false
  for (const id of savedIds) {
    if (existingIds.has(id)) continue
    const newTab: SqlTab = { id, type: 'sql', title: 'Query', connectionId }
    tabs.value.push(newTab)
    if (!activated) {
      setActiveTab(id)
      activated = true
    }
  }
  // 恢复的 id 可能带较大的数字后缀（如上个会话开过 sql-9），推进计数器避免新建时撞号
  sqlTabCounter.value = nextTabCounter('sql-', tabs.value.map(t => t.id))
}

// 仅在连接刚建立时恢复一次（反复切换连接不会重复恢复，草稿是跨连接的）
let draftsRestored = false
watch(
  () => props.currentConnection,
  (conn) => {
    if (conn && !draftsRestored) {
      draftsRestored = true
      restoreDrafts()
    }
  }
)
onMounted(() => {
  if (props.currentConnection && !draftsRestored) {
    draftsRestored = true
    restoreDrafts()
  }
})

// 关闭指定数据库下的所有表Tab
function closeDatabaseTabs(connectionId: string, database: string) {
  const tabsToClose = tabs.value.filter(t => 
    t.type === 'table' && 
    (t as TableTab).connectionId === connectionId && 
    (t as TableTab).database === database
  )
  
  tabsToClose.forEach(tab => {
    const index = tabs.value.findIndex(t => t.id === tab.id)
    if (index !== -1) {
      // 通知 DataTable 关闭；有未保存修改时会延后到确认结果，外层 Tab 由 table-closed 收尾
      const tableTab = tab as TableTab
      const internalTabId = `${tableTab.connectionId}.${tableTab.database}.${tableTab.tableName}`
      if (dataTableRef.value?.closeTableById(internalTabId) === false) return

      tabs.value.splice(index, 1)
    }
  })
  
  // 如果当前激活的tab被关闭了，切换到对象tab
  if (tabsToClose.some(t => t.id === activeTabId.value)) {
    setActiveTab('objects')
  }
}

// 关闭指定连接的所有表Tab
function closeConnectionTabs(connectionId: string) {
  const tabsToClose = tabs.value.filter(t => 
    t.type === 'table' && 
    (t as TableTab).connectionId === connectionId
  )
  
  tabsToClose.forEach(tab => {
    const index = tabs.value.findIndex(t => t.id === tab.id)
    if (index !== -1) {
      // 通知 DataTable 关闭；有未保存修改时会延后到确认结果，外层 Tab 由 table-closed 收尾
      const tableTab = tab as TableTab
      const internalTabId = `${tableTab.connectionId}.${tableTab.database}.${tableTab.tableName}`
      if (dataTableRef.value?.closeTableById(internalTabId) === false) return

      tabs.value.splice(index, 1)
    }
  })
  
  // 如果当前激活的tab被关闭了，切换到对象tab
  if (tabsToClose.some(t => t.id === activeTabId.value)) {
    setActiveTab('objects')
  }
}

// 暴露方法供父组件调用
defineExpose({
  dataTableRef,
  openTable,
  newSqlQuery,
  openSqlFile,
  newConsole,
  openErDiagramTab,
  closeTab,
  closeDatabaseTabs,
  closeConnectionTabs,
  activeTabId,
})
</script>

<style scoped>
.main-content-wrapper {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  min-width: 300px;
}

.main-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background: var(--surface-default);
}

/* Tab 栏 */
.tab-bar {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  padding: var(--space-2);
  background: var(--surface-secondary);
  border-bottom: 1px solid var(--border-default);
  overflow-x: auto;
}

.tab-item {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  background: var(--surface-default);
  color: var(--text-secondary);
  font-size: var(--text-sm);
  cursor: pointer;
  transition: var(--transition-colors);
  white-space: nowrap;
  flex-shrink: 0;
}

.tab-item:hover {
  background: var(--surface-hover);
}

.tab-item.active {
  background: var(--brand-500);
  border-color: var(--brand-500);
  color: white;
}

.tab-title {
  max-width: 120px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tab-close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border-radius: var(--radius-sm);
  opacity: 0.6;
  transition: opacity 0.2s;
}

.tab-close:hover {
  opacity: 1;
  background: rgba(255, 255, 255, 0.2);
}

/* 内容区域 */
.content-area {
  flex: 1;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

/* 对象列表视图 */
.objects-view {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.objects-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-3) var(--space-4);
  border-bottom: 1px solid var(--border-default);
  background: var(--surface-secondary);
}

.objects-header h3 {
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.objects-filters {
  display: flex;
  gap: var(--space-1);
}

.filter-btn {
  padding: var(--space-1) var(--space-3);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  background: var(--surface-default);
  color: var(--text-secondary);
  font-size: var(--text-sm);
  cursor: pointer;
  transition: var(--transition-colors);
}

.filter-btn:hover {
  background: var(--surface-hover);
}

.filter-btn.active {
  background: var(--brand-500);
  border-color: var(--brand-500);
  color: white;
}

.objects-list {
  flex: 1;
  overflow: auto;
  /* 行内自带 padding，容器不留内边距，避免 sticky 表头被裁切 */
  padding: 0;
}

/* 表列表按列展示：名称、自增值、修改时间、占用空间 */
.objects-table {
  min-width: 520px;
}

.objects-row {
  display: grid;
  grid-template-columns: minmax(140px, 1fr) 96px 148px 88px;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-2) var(--space-3);
  font-size: var(--text-sm);
}

.objects-head {
  position: sticky;
  top: 0;
  z-index: 1;
  background: var(--surface-secondary);
  border-bottom: 1px solid var(--border-default);
  color: var(--text-secondary);
  font-weight: var(--font-semibold);
  font-size: var(--text-xs);
  white-space: nowrap;
}

.object-item {
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: var(--transition-colors);
}

.object-item:hover {
  background: var(--surface-hover);
}

/* 多选高亮：与表头/悬停背景区分开 */
.object-item.selected {
  background: var(--brand-100);
}

.object-item .col-name {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-width: 0;
}

.object-item .col-name svg {
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.name-text {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.col-auto-inc,
.col-updated,
.col-size {
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.objects-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100px;
  color: var(--text-tertiary);
  font-size: var(--text-sm);
}

/* 表格、SQL 和控制台视图 */
.table-view,
.sql-view,
.console-view,
.er-view {
  flex: 1;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  min-height: 0;
}

/* 空状态 */
.empty-view,
.welcome {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  color: var(--text-secondary);
  text-align: center;
}

.empty-view h2,
.welcome h2 {
  font-size: var(--text-xl);
  font-weight: var(--font-semibold);
  margin-bottom: var(--space-2);
}

.welcome h2 {
  font-size: var(--text-2xl);
  color: var(--text-primary);
}
</style>
