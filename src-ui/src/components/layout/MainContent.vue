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
              <div
                v-for="table in filteredObjects"
                :key="table"
                class="object-item"
                @click="emit('selectTable', table)"
                @dblclick="emit('openTable', table)"
              >
                <Table2 :size="16" />
                <span>{{ table }}</span>
              </div>
              <div v-if="filteredObjects.length === 0" class="objects-empty">
              {{ t('layout.noTables') }}
            </div>
            </div>
            <div v-else class="objects-empty">
              {{ t('layout.selectDatabaseFirst') }}
            </div>
          </div>

          <!-- SQL 编辑器视图 -->
          <div v-else-if="activeTab?.type === 'sql'" class="sql-view">
            <SqlEditor :tabId="activeTab.id" />
          </div>

          <!-- 控制台视图 -->
          <div v-else-if="activeTab?.type === 'console'" class="console-view">
            <Console :tabId="activeTab.id" />
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
import { ref, computed, nextTick } from 'vue'
import { Database, Table2, FileCode, X, Terminal } from 'lucide-vue-next'
import DataTable from '@/components/DataTable.vue'
import SqlEditor from '@/components/SqlEditor.vue'
import Console from '@/components/Console.vue'
import { useDataBrowserStore } from '@/stores/dataBrowser'
import { t } from '@/i18n'
import type { ConnectionStatus } from '@/stores/connection'
import type { DatabaseObject } from '@/stores/dataBrowser'

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
}

interface ConsoleTab {
  id: string
  type: 'console'
  title: string
}

type Tab = ObjectsTab | TableTab | SqlTab | ConsoleTab

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

// 当前数据库的表列表
const currentTables = computed(() => {
  if (!props.currentConnection || !props.selectedDatabase) return []
  // selectedDatabase 格式: "connectionId.dbName"
  const dbName = props.selectedDatabase.split('.').pop()
  if (!dbName) return []
  const tables = props.connectionDatabaseTables[props.currentConnection.id]?.[dbName]
  if (!tables) return []
  // 只返回类型为 Table 的对象名称
  return tables
    .filter(obj => obj.object_type === 'Table')
    .map(obj => obj.name)
})

// 过滤后的对象列表 - 只显示表
const filteredObjects = computed(() => {
  return currentTables.value
})

// 方法
function switchTab(tabId: string) {
  activeTabId.value = tabId

  // 如果是表 Tab，通知 DataTable 切换
  const tab = tabs.value.find(t => t.id === tabId)
  if (tab?.type === 'table') {
    const tableTab = tab as TableTab
    dataTableRef.value?.openTable(tableTab.connectionId, tableTab.database, tableTab.tableName)
  }
}

const browserStore = useDataBrowserStore()

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
  }

  tabs.value.splice(index, 1)

  // 如果关闭的是当前激活的 tab，切换到前一个 tab
  if (activeTabId.value === tabId) {
    const newIndex = Math.max(0, index - 1)
    activeTabId.value = tabs.value[newIndex]?.id || 'objects'
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
    activeTabId.value = tabId
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
  activeTabId.value = tabId

  // 通知 DataTable 打开这个表（等待DOM更新）
  await nextTick()
  dataTableRef.value?.openTable(connectionId, database, tableName)
}

// 新建 SQL 查询
function newSqlQuery() {
  const tabId = `sql-${sqlTabCounter.value++}`
  const newTab: SqlTab = {
    id: tabId,
    type: 'sql',
    title: `Query ${sqlTabCounter.value - 1}`
  }
  tabs.value.push(newTab)
  activeTabId.value = tabId
}

// 新建控制台
function newConsole() {
  const tabId = `console-${consoleTabCounter.value++}`
  const newTab: ConsoleTab = {
    id: tabId,
    type: 'console',
    title: `Console ${consoleTabCounter.value - 1}`
  }
  tabs.value.push(newTab)
  activeTabId.value = tabId
}

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
    activeTabId.value = 'objects'
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
      // 通知 DataTable 关闭
      const tableTab = tab as TableTab
      const internalTabId = `${tableTab.connectionId}.${tableTab.database}.${tableTab.tableName}`
      dataTableRef.value?.closeTableById(internalTabId)
      
      tabs.value.splice(index, 1)
    }
  })
  
  // 如果当前激活的tab被关闭了，切换到对象tab
  if (tabsToClose.some(t => t.id === activeTabId.value)) {
    activeTabId.value = 'objects'
  }
}

// 暴露方法供父组件调用
defineExpose({
  dataTableRef,
  openTable,
  newSqlQuery,
  newConsole,
  closeDatabaseTabs,
  closeConnectionTabs
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
  padding: var(--space-2);
}

.object-item {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: var(--transition-colors);
}

.object-item:hover {
  background: var(--surface-hover);
}

.object-item svg {
  color: var(--text-tertiary);
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
.console-view {
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
