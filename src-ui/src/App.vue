<template>
  <div class="app" :class="{ 'modal-open': hasOpenModal }">
    <HeaderToolbar
      :active-tool="activeTool"
      :info-panel-open="infoPanelOpen"
      @toggle-sidebar="sidebarOpen = !sidebarOpen"
      @toggle-panel="infoPanelOpen = !infoPanelOpen"
      @open-connection-form="openConnectionForm"
      @open-new-query-tab="openNewQueryTab"
      @open-new-console-tab="openNewConsoleTab"
      @open-schema-diff="handleOpenSchemaDiff"
      @open-er-diagram="handleOpenErDiagram"
      @open-monitor="handleOpenMonitor"
      @open-backup="handleOpenBackup"
      @open-global-search="handleOpenGlobalSearch"
      @open-history="showHistory = true"
      @open-about="showAbout = true"
    />

    <div
      class="app-body"
      :class="{ 'sidebar-open': sidebarOpen, 'panel-open': infoPanelOpen && !isDesktop }"
      @click.self="closeDrawers"
    >
      <!-- 左侧：连接树 -->
      <div
        class="sidebar-left"
        :style="{ width: leftWidth + 'px' }"
      >
        <ConnectionTree
          :tree-items="connectionTree"
          :current-connection="currentConnection"
          :expanded-connections="expandedConnections"
          :expanded-categories="expandedCategories"
          :active-databases="activeDatabases"
          :selected-database="selectedDatabase"
          :selected-table="selectedTable"
          :selected-info-type="selectedInfoType"
          :selected-info-data="selectedInfoData"
          :connection-databases="connectionDatabases"
          :connection-database-tables="connectionDatabaseTables"
          @open-saved-connection="openSavedConnection"
          @show-saved-connection-menu="showSavedConnectionContextMenu"
          @select-connection="selectConnection"
          @show-active-connection-menu="showActiveConnectionContextMenu"
          @toggle-connection="toggleConnection"
          @select-database="selectDatabase"
          @dblclick-database="handleDatabaseDblClick"
          @show-database-menu="showDatabaseContextMenu"
          @toggle-category="toggleCategory"
          @select-table="selectTable"
          @dblclick-table="openTable"
          @show-table-menu="showTableContextMenu"
          @select-object="selectObject"
          @dblclick-object="openObjectDefinition"
          @new-database="handleNewDatabase"
        />
      </div>

      <!-- 左分隔条 -->
      <div class="resizer resizer-left" @mousedown="startResize('left', $event)"></div>

      <!-- 中间：主内容区 -->
      <MainContent
        :width="middleWidth"
        :current-connection="currentConnection"
        :selected-database="selectedDatabase"
        :connection-database-tables="connectionDatabaseTables"
        ref="mainContentRef"
        @new-query="openNewQueryTab"
        @select-table="onSelectTableFromObjects"
        @open-table="onOpenTableFromObjects"
        @show-table-menu="showTableContextMenu"
        @table-tab-closed="onTableTabClosed"
      />

      <!-- 右分隔条 -->
      <div
        v-if="showInfoPanel"
        class="resizer resizer-right"
        @mousedown="startResize('right', $event)"
      ></div>

      <!-- 右侧：信息面板（按需出现，选中连接/库/表时展开） -->
      <div
        v-if="showInfoPanel"
        class="sidebar-right"
        :style="{ width: rightWidth + 'px' }"
      >
        <InfoPanel
          v-if="selectedInfoType"
          :type="selectedInfoType"
          :data="selectedInfoData"
          @close="closeInfoPanel"
        />
      </div>
    </div>

    <!-- 连接表单弹窗 -->
    <div v-if="showConnectionForm" class="modal-overlay" role="presentation">
      <div class="modal-content modal-connection" role="dialog" aria-modal="true">
        <ConnectionForm
          :edit-connection="editingConnection"
          @success="onConnectionSuccess"
          @cancel="showConnectionForm = false; editingConnection = undefined"
        />
      </div>
    </div>

    <!-- 表右键菜单 -->
    <div
      v-if="tableContextMenu.show"
      class="context-menu"
      role="menu"
      :style="{ top: tableContextMenu.y + 'px', left: tableContextMenu.x + 'px' }"
      @click.stop
    >
      <!-- 多选时仅提供批量删除，避免单表操作含义不明 -->
      <template v-if="tableContextMenu.tableNames.length > 1">
        <div class="menu-item menu-item-danger" role="menuitem" @click="handleTableMenu('delete-table')">
          <Trash2 class="menu-icon" :size="14" />
          {{ t('app.deleteSelectedTables', { count: tableContextMenu.tableNames.length }) }}
        </div>
      </template>
      <template v-else>
        <div class="menu-item" role="menuitem" @click="handleTableMenu('view-data')">
          <Eye class="menu-icon" :size="14" /> {{ t('app.viewData') }}
        </div>
        <div class="menu-item" role="menuitem" @click="handleTableMenu('design-table')">
          <Settings class="menu-icon" :size="14" /> {{ t('app.designTable') }}
        </div>
        <div class="menu-item" role="menuitem" @click="handleTableMenu('refresh-tables')">
          <RefreshCw class="menu-icon" :size="14" /> {{ t('common.refresh') }}
        </div>
        <div class="menu-divider"></div>
        <div class="menu-item" role="menuitem" @click="handleTableMenu('export')">
          <Download class="menu-icon" :size="14" /> {{ t('app.exportData') }}
        </div>
        <div class="menu-item" role="menuitem" @click="handleTableMenu('import')">
          <Upload class="menu-icon" :size="14" /> {{ t('app.importData') }}
        </div>
        <div class="menu-divider"></div>
        <div class="menu-item" role="menuitem" @click="handleTableMenu('copy-name')">
          <Copy class="menu-icon" :size="14" /> {{ t('app.copyName') }}
        </div>
        <div class="menu-item" role="menuitem" @click="handleTableMenu('copy-table')">
          <Copy class="menu-icon" :size="14" /> {{ t('app.copyTable') }}
        </div>
        <div class="menu-item" role="menuitem" @click="handleTableMenu('clear-table')">
          <Eraser class="menu-icon" :size="14" /> {{ t('app.clearTable') }}
        </div>
        <div class="menu-item" role="menuitem" @click="handleTableMenu('truncate-table')">
          <Trash2 class="menu-icon" :size="14" /> {{ t('app.truncateTable') }}
        </div>
        <div class="menu-item menu-item-danger" role="menuitem" @click="handleTableMenu('delete-table')">
          <Trash2 class="menu-icon" :size="14" /> {{ t('app.deleteTable') }}
        </div>
      </template>
    </div>

    <!-- 数据库右键菜单 -->
    <div
      v-if="databaseContextMenu.show"
      class="context-menu"
      role="menu"
      :style="{ top: databaseContextMenu.y + 'px', left: databaseContextMenu.x + 'px' }"
      @click.stop
    >
      <div v-if="!databaseContextMenu.isOpened" class="menu-item" @click="handleDatabaseMenu('open')">
        <Database class="menu-icon" :size="14" /> {{ t('app.openDatabase') }}
      </div>
      <div v-if="databaseContextMenu.isOpened" class="menu-item" @click="handleDatabaseMenu('close')">
        <X class="menu-icon" :size="14" /> {{ t('app.closeDatabase') }}
      </div>
      <div class="menu-divider"></div>
      <div class="menu-item" role="menuitem" @click="handleDatabaseMenu('edit')">
        <Settings class="menu-icon" :size="14" /> {{ t('app.editDatabase') }}
      </div>
      <div class="menu-item" role="menuitem" @click="handleDatabaseMenu('new-table')">
        <Plus class="menu-icon" :size="14" /> {{ t('app.newTable') }}
      </div>
      <div class="menu-item" role="menuitem" @click="handleDatabaseMenu('refresh')">
        <RefreshCw class="menu-icon" :size="14" /> {{ t('common.refresh') }}
      </div>
      <div class="menu-divider"></div>
      <div class="menu-item" role="menuitem" @click="handleDatabaseMenu('export-sql')">
        <Download class="menu-icon" :size="14" /> {{ t('app.exportSql') }}
      </div>
      <div class="menu-item" role="menuitem" @click="handleDatabaseMenu('import-sql')">
        <Upload class="menu-icon" :size="14" /> {{ t('app.importSql') }}
      </div>
      <div class="menu-divider"></div>
      <div class="menu-item menu-item-danger" role="menuitem" @click="handleDatabaseMenu('delete')">
        <Trash2 class="menu-icon" :size="14" /> {{ t('app.deleteDatabase') }}
      </div>
    </div>

    <!-- 已保存连接右键菜单 -->
    <div
      v-if="savedConnectionContextMenu.show"
      class="context-menu"
      role="menu"
      :style="{ top: savedConnectionContextMenu.y + 'px', left: savedConnectionContextMenu.x + 'px' }"
      @click.stop
    >
      <div class="menu-item" role="menuitem" @click="handleSavedConnectionMenu('open')">
        <Plug class="menu-icon" :size="14" /> {{ t('app.openConnection') }}
      </div>
      <div class="menu-item" role="menuitem" @click="handleSavedConnectionMenu('edit')">
        <Settings class="menu-icon" :size="14" /> {{ t('app.editConnection') }}
      </div>
      <div class="menu-divider"></div>
      <div class="menu-item menu-item-danger" role="menuitem" @click="handleSavedConnectionMenu('delete')">
        <Trash2 class="menu-icon" :size="14" /> {{ t('app.deleteConnection') }}
      </div>
    </div>

    <!-- 活跃连接右键菜单 -->
    <div
      v-if="activeConnectionContextMenu.show"
      class="context-menu"
      role="menu"
      :style="{ top: activeConnectionContextMenu.y + 'px', left: activeConnectionContextMenu.x + 'px' }"
      @click.stop
    >
      <div class="menu-item menu-item-danger" role="menuitem" @click="handleActiveConnectionMenu('disconnect')">
        <X class="menu-icon" :size="14" /> {{ t('app.disconnect') }}
      </div>
      <template v-if="activeConnectionContextMenu.driver !== 'postgres'">
        <div class="menu-divider"></div>
        <div
          class="menu-item"
          @click="handleActiveConnectionMenu('new-database')"
        >
          <Plus class="menu-icon" :size="14" /> {{ t('app.newDatabase') }}
        </div>
      </template>
    </div>

    <!-- 关于弹窗 -->
    <div v-if="showAbout" class="modal-overlay" role="presentation">
      <div class="modal-content modal-about" role="dialog" aria-modal="true">
        <AboutDialog @close="showAbout = false" />
      </div>
    </div>

    <!-- Schema 对比弹窗 -->
    <div v-if="showSchemaDiff" class="modal-overlay" role="presentation">
      <div class="modal-content modal-large" role="dialog" aria-modal="true">
        <component
          :is="ProUi.SchemaDiff"
          v-if="ProUi.SchemaDiff"
          :connections="activeConnections"
          :initial-connection-id="currentConnection?.id"
          :initial-database="browserStore.currentDatabase"
          @close="showSchemaDiff = false"
        />
      </div>
    </div>

    <!-- 实例监控与审计弹窗 -->
    <div v-if="showMonitor" class="modal-overlay" role="presentation">
      <div class="modal-content modal-large" role="dialog" aria-modal="true">
        <component
          :is="ProUi.MonitorPanel"
          v-if="ProUi.MonitorPanel"
          :connections="activeConnections"
          :initial-connection-id="currentConnection?.id"
          @close="showMonitor = false"
        />
      </div>
    </div>

    <!-- 备份还原弹窗 -->
    <div v-if="showBackup" class="modal-overlay" role="presentation">
      <div class="modal-content modal-backup" role="dialog" aria-modal="true">
        <component
          :is="ProUi.BackupDialog"
          v-if="ProUi.BackupDialog"
          @close="showBackup = false"
          @restored="onDatabaseRestored"
        />
      </div>
    </div>

    <!-- 导入 SQL 进度弹窗：导入结束前即可看到逐条语句的执行结果 -->
    <div v-if="importTask" class="modal-overlay">
      <div class="modal-content modal-import" role="dialog" aria-modal="true">
        <ImportProgressDialog
          :file-name="importTask.fileName"
          :database="importTask.database"
          :bytes="importTask.bytes"
          :total="importTask.total"
          :lines="importTask.lines"
          :done="importTask.done"
          :success="importTask.success"
          :error="importTask.error"
          @close="importTask = null"
        />
      </div>
    </div>

    <!-- 全局执行历史（跨标签页） -->
    <HistoryDialog :open="showHistory" @close="showHistory = false" @rerun="rerunHistory" />

    <!-- 跨库全局搜索弹窗 -->
    <div v-if="showGlobalSearch" class="modal-overlay" role="presentation">
      <div class="modal-content modal-large" role="dialog" aria-modal="true">
        <component
          :is="ProUi.GlobalSearchDialog"
          v-if="ProUi.GlobalSearchDialog"
          :initial-connection-id="currentConnection?.id"
          :initial-database="browserStore.currentDatabase"
          @open-table="handleGlobalSearchOpenTable"
          @close="showGlobalSearch = false"
        />
      </div>
    </div>

    <!-- 表格设计器弹窗 -->
    <div v-if="showTableDesigner" class="modal-overlay">
      <div class="modal-content modal-large modal-designer" role="dialog" aria-modal="true">
        <TableDesigner
          :connection-id="tableDesignerData.connectionId"
          :database="tableDesignerData.database"
          :table-name="tableDesignerData.tableName"
          :is-new-table="!tableDesignerData.tableName"
          @close="showTableDesigner = false"
          @saved="onTableDesignerSaved"
        />
      </div>
    </div>

    <!-- 数据库设计器弹窗 -->
    <div v-if="showDatabaseDesigner" class="modal-overlay">
      <div class="modal-content" role="dialog" aria-modal="true">
        <DatabaseDesigner
          :connection-id="databaseDesignerData.connectionId"
          :database="databaseDesignerData.database"
          :is-new-database="databaseDesignerData.isNewDatabase"
          @close="showDatabaseDesigner = false"
          @saved="onDatabaseDesignerSaved"
        />
      </div>
    </div>

    <!-- Toast 通知 -->
    <ToastNotification ref="toastRef" />

    <!-- 危险操作二次确认弹框 -->
    <ConfirmDialog />

    <!-- 命令面板 Ctrl+K -->
    <CommandPalette />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, reactive, onMounted, onUnmounted, watch } from 'vue'
import { quoteIdentifier } from '@/utils/sql'
import { t, toggleLocale, locale } from '@/i18n'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { isSqlFilePath } from '@/utils/sqlFile'
import { useConnectionStore, type ConnectionStatus, type SshConfig } from '@/stores/connection'
import { useSavedConnectionsStore, type SavedConnection } from '@/stores/savedConnections'
import { useDataBrowserStore, type DatabaseObject } from '@/stores/dataBrowser'
import { useThemeStore } from '@/stores/theme'
import HeaderToolbar from '@/components/layout/HeaderToolbar.vue'
import MainContent from '@/components/layout/MainContent.vue'
import ConnectionTree from '@/components/sidebar/ConnectionTree.vue'
import ConnectionForm from '@/components/ConnectionForm.vue'
import InfoPanel from '@/components/InfoPanel.vue'
import TableDesigner from '@/components/TableDesigner.vue'
import DatabaseDesigner from '@/components/DatabaseDesigner.vue'
import AboutDialog from '@/components/AboutDialog.vue'
import ToastNotification from '@/components/ui/Toast.vue'
import ConfirmDialog from '@/components/ui/ConfirmDialog.vue'
import CommandPalette from '@/components/CommandPalette.vue'
import HistoryDialog from '@/components/HistoryDialog.vue'
import type { QueryHistoryEntry } from '@/stores/queryLibrary'
import ImportProgressDialog from '@/components/ImportProgressDialog.vue'
import { ProUi } from '@/utils/pro'
import { useCommandRegistryStore } from '@/stores/commandRegistry'
import { useToast } from '@/composables/useToast'
import { confirmDanger } from '@/composables/useConfirm'
import { hasUnsavedSources } from '@/composables/useUnsavedGuard'
import { useBreakpoints } from '@/composables/useBreakpoints'
import {
  Database,
  X,
  Plus,
  Eye,
  Settings,
  RefreshCw,
  Download,
  Upload,
  Copy,
  Trash2,
  Eraser,
  Plug,
} from '@/components/icons'

const store = useConnectionStore()
const savedStore = useSavedConnectionsStore()
const browserStore = useDataBrowserStore()
const themeStore = useThemeStore()
const registry = useCommandRegistryStore()
const mainContentRef = ref<InstanceType<typeof MainContent> | null>(null)
const toastRef = ref<InstanceType<typeof ToastNotification> | null>(null)

// 响应式断点
const { isMobile, isSmallScreen, isDesktop } = useBreakpoints()

// 响应式面板状态
const sidebarOpen = ref(false)
// 信息面板默认收起（DBX 风格：选中连接/库/表时才出现）
const infoPanelOpen = ref(false)
// 面板渲染条件：有选中对象且未被用户收起
const showInfoPanel = computed(() => infoPanelOpen.value && !!selectedInfoType.value)

// 监听断点变化
watch(isDesktop, (desktop) => {
  if (desktop) {
    sidebarOpen.value = false
    // 桌面端保留用户当前的面板开合意图，不做强制切换
  }
}, { immediate: true })

watch(isSmallScreen, (small) => {
  if (small && !isMobile.value) {
    sidebarOpen.value = false
    infoPanelOpen.value = false
  }
}, { immediate: true })

watch(isMobile, (mobile) => {
  if (mobile) {
    sidebarOpen.value = false
    infoPanelOpen.value = false
  }
}, { immediate: true })

// Toast 通知
const { setToastRef, success: showToast, error: showError } = useToast()
onMounted(() => {
  if (toastRef.value) {
    setToastRef(toastRef.value)
  }
})

// 表右键菜单
const tableContextMenu = ref({
  show: false,
  x: 0,
  y: 0,
  connectionId: '',
  database: '',
  tableName: '',
  // 中间面板多选时包含全部选中表；左侧连接树右键只有一项
  tableNames: [] as string[]
})

// 数据库右键菜单
const databaseContextMenu = ref({
  show: false,
  x: 0,
  y: 0,
  connectionId: '',
  database: '',
  isOpened: false
})

// 已保存连接右键菜单
const savedConnectionContextMenu = ref({
  show: false,
  x: 0,
  y: 0,
  connectionId: '',
  connectionName: ''
})

// 活跃连接右键菜单
const activeConnectionContextMenu = ref({
  show: false,
  x: 0,
  y: 0,
  connectionId: '',
  connectionName: '',
  driver: 'mysql'
})

// 监听点击关闭菜单（排除右键菜单自身）
function handleDocumentClick(e: MouseEvent) {
  const target = e.target as HTMLElement
  // 如果点击的不是右键菜单内部，则关闭菜单
  if (!target.closest('.context-menu')) {
    tableContextMenu.value.show = false
    databaseContextMenu.value.show = false
    savedConnectionContextMenu.value.show = false
    activeConnectionContextMenu.value.show = false
  }
}

const showConnectionForm = ref(false)
const editingConnection = ref<SavedConnection | undefined>(undefined)
const selectedDatabase = ref('')
const selectedTable = ref('')
const viewingTableStructure = ref('')
const currentView = ref<'data' | 'sql'>('data')
const activeTool = ref<'connection' | 'query'>('connection')

// 表格设计器
const showTableDesigner = ref(false)
const tableDesignerData = ref({
  connectionId: '',
  database: '',
  tableName: ''
})

// Schema 对比
const showSchemaDiff = ref(false)

// 实例监控与审计
const showMonitor = ref(false)

// 备份还原
const showBackup = ref(false)

// 导入 SQL 实时流水
interface ImportLogLine {
  no?: number | null
  level: 'success' | 'error' | 'warn'
  text: string
  elapsedMs?: number | null
}
interface ImportTaskState {
  id: string
  fileName: string
  database: string
  connectionId: string
  bytes: number
  total: number
  lines: ImportLogLine[]
  done: boolean
  success: boolean
  error: string
}
const importTask = ref<ImportTaskState | null>(null)
let importUnlisten: UnlistenFn | null = null
// 流水最多保留的行数，避免超大脚本把内存吃满
const IMPORT_LOG_LIMIT = 2000

// 跨库全局搜索
const showGlobalSearch = ref(false)

// 全局执行历史
const showHistory = ref(false)

/** 从历史重跑：新建查询页、带原参数并自动执行 */
function rerunHistory(entry: QueryHistoryEntry) {
  showHistory.value = false
  mainContentRef.value?.openSqlWithContent(
    entry.sql,
    t('history.rerunTitle'),
    entry.connectionId,
    entry.params,
    true,
  )
}

// 关于
const showAbout = ref(false)

// 功能入口：直接打开对应界面
function handleOpenSchemaDiff() {
  showSchemaDiff.value = true
}

function handleOpenErDiagram() {
  if (!selectedDatabase.value) {
    showError(t('erDiagram.needsDatabase'))
    return
  }
  mainContentRef.value?.openErDiagramTab(selectedDatabase.value)
}

function handleOpenMonitor() {
  showMonitor.value = true
}

function handleOpenBackup() {
  showBackup.value = true
}

function handleOpenGlobalSearch() {
  showGlobalSearch.value = true
}

/** 全局搜索结果跳转：关闭弹窗并打开对应表 */
function handleGlobalSearchOpenTable(connectionId: string, database: string, table: string) {
  showGlobalSearch.value = false
  openTable(connectionId, database, table)
}

// 数据库设计器
const showDatabaseDesigner = ref(false)
const databaseDesignerData = ref({
  connectionId: '',
  database: '',
  isNewDatabase: false
})

// 是否有弹层打开：用于隐藏背景区域的滚动条，避免其残影绘制在遮罩之上
const hasOpenModal = computed(() =>
  showConnectionForm.value ||
  showSchemaDiff.value ||
  showMonitor.value ||
  showBackup.value ||
  showGlobalSearch.value ||
  showAbout.value ||
  showTableDesigner.value ||
  showDatabaseDesigner.value
)

/** 打开「新建数据库」设计器（侧边栏 + 连接右键菜单共用） */
function handleNewDatabase(connectionId: string) {
  databaseDesignerData.value = { connectionId, database: '', isNewDatabase: true }
  showDatabaseDesigner.value = true
}

// 三栏宽度
const leftWidth = ref(280)
const rightWidth = ref(280)
const middleWidth = computed(() => {
  // 中间区域宽度 = 总宽度 - 左 - 右
  return Math.max(400, window.innerWidth - leftWidth.value - rightWidth.value - 8) // 8px for resizers
})

// 右侧信息面板状态
const selectedInfoType = ref<'connection' | 'database' | 'table' | null>(null)
const selectedInfoData = ref<{
  connectionId?: string
  connectionName?: string
  database?: string
  tableName?: string
}>({})

// 拖拽调整
let resizing: 'left' | 'right' | null = null
let startX = 0
let startLeftWidth = 0
let startRightWidth = 0

function startResize(side: 'left' | 'right', e: MouseEvent) {
  e.preventDefault()
  resizing = side
  startX = e.clientX
  startLeftWidth = leftWidth.value
  startRightWidth = rightWidth.value

  // 拖拽分隔条期间禁止文本选中，避免误选左右内容
  document.body.style.userSelect = 'none'
  document.body.style.webkitUserSelect = 'none'

  document.addEventListener('mousemove', onResize)
  document.addEventListener('mouseup', stopResize)
}

function onResize(e: MouseEvent) {
  if (!resizing) return

  e.preventDefault()

  const delta = e.clientX - startX
  
  if (resizing === 'left') {
    leftWidth.value = Math.max(200, Math.min(500, startLeftWidth + delta))
  } else if (resizing === 'right') {
    const newRightWidth = startRightWidth - delta
    rightWidth.value = Math.max(200, Math.min(800, newRightWidth))
  }
}

function stopResize() {
  resizing = null
  document.body.style.userSelect = ''
  document.body.style.webkitUserSelect = ''
  document.removeEventListener('mousemove', onResize)
  document.removeEventListener('mouseup', stopResize)
}

// 活跃连接列表
const activeConnections = computed(() => store.activeConnections)
// 当前选中的连接
const currentConnection = computed(() => store.currentConnection)
// 统一连接列表：已保存（离线）与活跃连接合并展示，按名称排序保证位置稳定
type ConnectionTreeItem =
  | { type: 'saved'; key: string; name: string; host: string; port: number; saved: SavedConnection }
  | { type: 'active'; key: string; name: string; host: string; port: number; active: ConnectionStatus }

const connectionTree = computed<ConnectionTreeItem[]>(() => {
  const items: ConnectionTreeItem[] = []
  // 以「名称 + 主机 + 端口 + 驱动」作为同一连接的身份，
  // 只用名称去重会把不同服务器上的同名连接误判成同一条而隐藏掉
  const identity = (c: { name: string; host: string; port: number; driver?: string }) =>
    `${c.name}|${c.host}|${c.port}|${c.driver ?? 'mysql'}`
  const activeKeys = new Set(activeConnections.value.map(identity))
  // 已保存但尚未活跃的连接（离线）
  for (const s of savedStore.savedConnections) {
    if (!activeKeys.has(identity(s))) {
      items.push({ type: 'saved', key: s.id, name: s.name, host: s.host, port: s.port, saved: s })
    }
  }
  // 活跃连接
  for (const a of activeConnections.value) {
    items.push({ type: 'active', key: a.id, name: a.name, host: a.host, port: a.port, active: a })
  }
  return items.sort((x, y) => x.name.localeCompare(y.name))
})

// 每个连接的数据库列表（按connectionId分组）
const connectionDatabases = reactive<Record<string, DatabaseObject[]>>({})
const connectionDatabaseTables = reactive<Record<string, Record<string, DatabaseObject[]>>>({})

// 展开状态
const expandedConnections = reactive<Set<string>>(new Set())
const expandedDatabases = reactive<Set<string>>(new Set())
const expandedCategories = reactive<Set<string>>(new Set())

// 活跃数据库（已打开的数据库）
const activeDatabases = reactive<Set<string>>(new Set())

// 切换连接展开
function toggleConnection(connectionId: string) {
  if (expandedConnections.has(connectionId)) {
    expandedConnections.delete(connectionId)
  } else {
    expandedConnections.add(connectionId)
    // 展开时加载数据库
    loadConnectionDatabases(connectionId)
  }
}

// 切换到指定连接
async function switchToConnection(conn: { id: string; name: string; host: string; port: number }) {
  const changed = store.currentConnection?.id !== conn.id
  store.switchConnection(conn.id)
  // 侧栏的库选择属于上一条连接，切连接后必须清掉，
  // 否则编辑器/控制台会沿用旧库名，把语句发到错误的库
  if (changed) browserStore.currentDatabase = ''
}

// 打开已保存的连接
async function openSavedConnection(savedConn: SavedConnection) {
  // 检查是否已存在同名连接
  const existing = activeConnections.value.find(c =>
    c.name === savedConn.name &&
    c.host === savedConn.host &&
    c.port === savedConn.port &&
    (c.driver ?? 'mysql') === (savedConn.driver ?? 'mysql')
  )

  if (existing) {
    // 切换到已存在的连接并展开
    switchToConnection(existing)
    expandedConnections.add(existing.id)
    return
  }

  // 获取密码
  const password = await savedStore.getPassword(savedConn.id)

  if (!password) {
    // 没有保存密码，需要重新输入；带上已保存的连接信息回填表单
    editingConnection.value = savedConn
    showConnectionForm.value = true
    return
  }

  // 组装 SSH 隧道配置；密码认证缺少已保存的 SSH 密码时回到表单重新输入
  let sshConfig: SshConfig | undefined
  if (savedConn.sshEnabled) {
    const authType = savedConn.sshAuthType ?? 'password'
    let sshSecret: string | null = null

    if (authType === 'password' || savedConn.hasSshSecret) {
      sshSecret = await savedStore.getSshSecret(savedConn.id)
    }
    if (authType === 'password' && !sshSecret) {
      editingConnection.value = savedConn
      showConnectionForm.value = true
      return
    }

    sshConfig = {
      host: savedConn.sshHost || '',
      port: savedConn.sshPort ?? 22,
      username: savedConn.sshUsername || '',
      auth_type: authType,
      password: authType === 'password' ? sshSecret || undefined : undefined,
      private_key_path: authType === 'private-key' ? savedConn.sshPrivateKeyPath : undefined,
      passphrase: authType === 'private-key' ? sshSecret || undefined : undefined,
    }
  }

  // 创建新连接
  try {
    const newConn = await store.createConnection({
      id: `conn_${Date.now()}`,
      name: savedConn.name,
      driver: savedConn.driver ?? 'mysql',
      host: savedConn.host,
      port: savedConn.port,
      database: savedConn.database || '',
      username: savedConn.username,
      password: password,
      safety_mode: savedConn.safetyMode ?? 'off',
      ssl_config: {
        mode: savedConn.sslMode ?? 'preferred',
        ca_path: savedConn.sslCaPath || undefined,
        cert_path: savedConn.sslCertPath || undefined,
        key_path: savedConn.sslKeyPath || undefined,
      },
      pool_config: {
        max_connections: 10,
        min_connections: 2,
        connect_timeout: 10,
        idle_timeout: 300,
        max_lifetime: 1800,
        heartbeat_interval: 30,
      },
    }, sshConfig)
    // 连接成功后自动展开数据库树
    expandedConnections.add(newConn.id)
    // 加载该连接的数据库列表
    await loadConnectionDatabases(newConn.id)
  } catch (e) {
    console.error('Connection failed:', e)
    showError(t('app.connectFailed', { error: String(e) }))
  }
}

// 断开指定连接
async function disconnectConnection(connectionId: string) {
  try {
    // 关闭该连接的所有表格
    if (mainContentRef.value?.closeConnectionTabs) {
      mainContentRef.value.closeConnectionTabs(connectionId)
    }

    await store.disconnect(connectionId)
    // 清理该连接的相关状态
    delete connectionDatabases[connectionId]
    delete connectionDatabaseTables[connectionId]
    expandedConnections.delete(connectionId)

    // 清理所有包含该连接ID的展开状态
    for (const key of expandedDatabases) {
      if (key.startsWith(connectionId)) expandedDatabases.delete(key)
    }
    for (const key of expandedCategories) {
      if (key.startsWith(connectionId)) expandedCategories.delete(key)
    }

    selectedDatabase.value = ''
    selectedTable.value = ''
    viewingTableStructure.value = ''
    // 右侧信息面板若显示该连接相关对象，同步关闭
    if (selectedInfoData.value.connectionId === connectionId) {
      closeInfoPanel()
    }
    activeTool.value = 'connection'
    currentView.value = 'data'
  } catch (e) {
    console.error('Failed to disconnect:', e)
    showError(t('app.disconnectFailed', { error: String(e) }))
  }
}

// 加载连接的数据库列表，force=true 时忽略缓存强制重新拉取
async function loadConnectionDatabases(connectionId: string, force = false): Promise<boolean> {
  if (!force && connectionDatabases[connectionId]) return true // 已加载

  try {
    const result = await browserStore.loadObjectTree(connectionId)
    connectionDatabases[connectionId] = result
    // 只清理已被删除的数据库的展开状态，保留仍然存在的数据库
    const prefix = connectionId + '.'
    const names = result.filter(obj => obj.object_type === 'Database').map(obj => obj.name)
    const stillExists = (key: string) =>
      names.some(name => key === prefix + name || key.startsWith(prefix + name + '.'))
    for (const key of Array.from(expandedDatabases)) {
      if (key.startsWith(prefix) && !stillExists(key)) expandedDatabases.delete(key)
    }
    for (const key of Array.from(expandedCategories)) {
      if (key.startsWith(prefix) && !stillExists(key)) expandedCategories.delete(key)
    }
    for (const key of Array.from(activeDatabases)) {
      if (key.startsWith(prefix) && !stillExists(key)) activeDatabases.delete(key)
    }
    return true
  } catch (e) {
    console.error('Failed to load databases:', e)
    return false
  }
}

// 双击打开数据库（展开并选中）
async function openDatabase(connectionId: string, dbName: string) {
  const key = `${connectionId}.${dbName}`
  // 设置selectedDatabase，让中间panel显示表列表
  selectedDatabase.value = key
  // 选中数据库（显示信息）
  selectDatabase(connectionId, dbName)
  // 展开数据库
  if (!expandedDatabases.has(key)) {
    expandedDatabases.add(key)
    await loadDatabaseTables(connectionId, dbName)
  }
}

// 关闭数据库（收起）
function closeDatabase(connectionId: string, dbName: string) {
  const key = `${connectionId}.${dbName}`
  // 从活跃数据库中移除
  activeDatabases.delete(key)
  expandedDatabases.delete(key)
  // 同时收起该数据库下的所有分类
  for (const catKey of expandedCategories) {
    if (catKey.startsWith(key)) expandedCategories.delete(catKey)
  }
  // 如果当前选中的是该数据库，清空选中
  if (selectedDatabase.value === key) {
    selectedDatabase.value = ''
  }
  // 右侧信息面板若显示该库或其下的表，同步关闭
  if (
    (selectedInfoType.value === 'database' || selectedInfoType.value === 'table') &&
    selectedInfoData.value.connectionId === connectionId &&
    selectedInfoData.value.database === dbName
  ) {
    closeInfoPanel()
  }
  // 关闭该数据库下所有打开的表Tab
  mainContentRef.value?.closeDatabaseTabs(connectionId, dbName)
}

// 切换分类展开
function toggleCategory(connectionId: string, dbName: string, categoryType: string) {
  const key = `${connectionId}.${dbName}.${categoryType}`
  if (expandedCategories.has(key)) {
    expandedCategories.delete(key)
  } else {
    expandedCategories.add(key)
  }
}

// 处理数据库双击：未打开则打开，已打开则不做处理
async function handleDatabaseDblClick(connectionId: string, dbName: string) {
  const key = `${connectionId}.${dbName}`
  if (!activeDatabases.has(key)) {
    // 未打开，执行打开操作
    activeDatabases.add(key)
    // 同时选中该数据库
    selectedDatabase.value = key
    // 加载表列表
    await loadDatabaseTables(connectionId, dbName)
  }
}

// 加载数据库表列表
async function loadDatabaseTables(connectionId: string, dbName: string, force = false) {
  if (!connectionDatabaseTables[connectionId]) {
    connectionDatabaseTables[connectionId] = {}
  }
  if (!force && connectionDatabaseTables[connectionId][dbName]) return // 已加载

  try {
    const result = await browserStore.loadObjectTree(connectionId, dbName)
    connectionDatabaseTables[connectionId][dbName] = result
  } catch (e) {
    // 库打不开时（无权限、库已被删除等）必须可见，否则树里只会显示空分类
    console.error('Failed to load tables:', e)
    showError(`加载数据库 ${dbName} 失败：${e}`)
  }
}

// 单击选中连接 - 显示连接信息
function selectConnection(conn: { id: string; name: string; host: string; port: number; username?: string }) {
  selectedInfoType.value = 'connection'
  // 桌面端自动展开；窄屏由用户点顶部面板按钮显式打开，避免抽屉遮罩每次点击都弹出来
  if (isDesktop.value) infoPanelOpen.value = true
  selectedInfoData.value = {
    connectionId: conn.id,
    connectionName: conn.name,
  }
}

// 单击选中数据库 - 显示数据库信息
async function selectDatabase(connectionId: string, dbName: string) {
  // 只显示数据库信息，不设置selectedDatabase（避免中间panel显示表列表）
  selectedInfoType.value = 'database'
  selectedInfoData.value = {
    connectionId,
    database: dbName,
  }
  // 确保信息面板打开（同 selectConnection：窄屏不自动展开）
  if (isDesktop.value) infoPanelOpen.value = true
  // 更新当前数据库，供 SQL 编辑器使用
  browserStore.currentDatabase = dbName
  
  // 确保侧边栏表列表已加载
  if (!connectionDatabaseTables[connectionId]?.[dbName]) {
    await loadDatabaseTables(connectionId, dbName)
  }
}

// 单击选中表 - 显示表结构
function selectTable(connectionId: string, dbName: string, tableName: string) {
  selectedTable.value = `${connectionId}.${dbName}.${tableName}`
  selectedInfoType.value = 'table'
  if (isDesktop.value) infoPanelOpen.value = true
  selectedInfoData.value = {
    connectionId,
    database: dbName,
    tableName,
  }
  // 更新当前数据库，供 SQL 编辑器使用
  browserStore.currentDatabase = dbName
}

// 关闭信息面板
function closeInfoPanel() {
  selectedInfoType.value = null
  selectedInfoData.value = {}
  infoPanelOpen.value = false
}

/** 窄屏抽屉遮罩点击：关闭左侧连接树抽屉与右侧信息抽屉 */
function closeDrawers() {
  sidebarOpen.value = false
  if (infoPanelOpen.value) closeInfoPanel()
}

// 表 Tab 关闭后同步右侧信息面板：若右侧正显示该表，则关闭面板
function onTableTabClosed(tab: { connectionId: string; database: string; tableName: string }) {
  if (
    selectedInfoType.value === 'table' &&
    selectedInfoData.value.connectionId === tab.connectionId &&
    selectedInfoData.value.database === tab.database &&
    selectedInfoData.value.tableName === tab.tableName
  ) {
    closeInfoPanel()
  }
}

// 表格设计器保存后的回调
async function onTableDesignerSaved() {
  // 刷新表列表并更新缓存
  const { connectionId, database } = tableDesignerData.value
  const result = await browserStore.loadObjectTree(connectionId, database)
  if (!connectionDatabaseTables[connectionId]) {
    connectionDatabaseTables[connectionId] = {}
  }
  connectionDatabaseTables[connectionId][database] = result
}

// 数据库设计器保存后的回调
async function onDatabaseDesignerSaved(databaseName: string) {
  const { connectionId, isNewDatabase } = databaseDesignerData.value
  // 关闭弹窗
  showDatabaseDesigner.value = false
  // 强制刷新数据库列表，确保新建的库立即出现在侧边栏
  await loadConnectionDatabases(connectionId, true)
  // 如果是新建数据库，自动展开
  if (isNewDatabase) {
    delete connectionDatabaseTables[connectionId]?.[databaseName]
    await openDatabase(connectionId, databaseName)
  } else {
    // 编辑现有数据库：刷新右侧信息面板，显示最新字符集/排序规则
    if (selectedInfoType.value === 'database' && selectedInfoData.value.database === databaseName) {
      // 通过重新赋值触发 InfoPanel 的 watch 重新加载
      selectedInfoData.value = {
        connectionId,
        database: databaseName,
      }
    }
  }
  showToast(isNewDatabase
    ? t('app.databaseCreated', { name: databaseName })
    : t('app.databaseSaved'))
}

// 备份还原完成后刷新受影响的数据库
async function onDatabaseRestored(connectionId: string, database: string) {
  // 旧表可能已被还原脚本删除，关闭该库下的表 Tab，避免残留失效数据
  mainContentRef.value?.closeDatabaseTabs(connectionId, database)
  delete connectionDatabaseTables[connectionId]?.[database]
  await loadConnectionDatabases(connectionId, true)
  await loadDatabaseTables(connectionId, database, true)
}

// 打开表（双击）
function openTable(connectionId: string, dbName: string, tableName: string) {
  selectedTable.value = `${connectionId}.${dbName}.${tableName}`
  // 更新当前数据库，供 SQL 编辑器使用
  browserStore.currentDatabase = dbName
  // 调用 MainContent 的 openTable 来管理 Tab
  if (mainContentRef.value?.openTable) {
    mainContentRef.value.openTable(connectionId, dbName, tableName)
  }
}

// 单击选中视图/存储过程/函数/触发器/事件：仅同步当前数据库
function selectObject(connectionId: string, dbName: string) {
  selectedDatabase.value = `${connectionId}.${dbName}`
  browserStore.currentDatabase = dbName
}

// 双击对象：读取创建语句并在新 SQL 标签页打开，可直接修改后执行
async function openObjectDefinition(connectionId: string, dbName: string, object: DatabaseObject) {
  try {
    browserStore.currentDatabase = dbName
    const definition = await invoke<string>('get_object_definition', {
      connectionId,
      database: dbName,
      schema: object.schema ?? null,
      name: object.name,
      objectType: object.object_type,
    })
    mainContentRef.value?.openSqlWithContent(definition, object.name, connectionId)
  } catch (e) {
    showError(`${object.name}: ${e}`)
  }
}

// 新建/编辑连接成功回调
function onConnectionSuccess() {
  showConnectionForm.value = false
  editingConnection.value = undefined
  activeTool.value = 'connection'
}

// 打开连接表单
function openConnectionForm() {
  activeTool.value = 'connection'
  editingConnection.value = undefined
  showConnectionForm.value = true
}

// 打开新的查询标签页
function openNewQueryTab() {
  if (!currentConnection.value) {
    showError(t('app.connectRequired'))
    return
  }
  // 调用 MainContent 组件的方法新建 SQL 查询 Tab
  mainContentRef.value?.newSqlQuery()
  // 关闭信息面板
  selectedInfoType.value = null
  selectedInfoData.value = {}
}

// 打开新的控制台标签页
function openNewConsoleTab() {
  if (!currentConnection.value) {
    showError(t('app.connectRequired'))
    return
  }
  // 调用 MainContent 组件的方法新建控制台 Tab
  mainContentRef.value?.newConsole()
  // 关闭信息面板
  selectedInfoType.value = null
  selectedInfoData.value = {}
}

// 从对象列表单击选择表
function onSelectTableFromObjects(tableName: string) {
  if (!currentConnection.value || !selectedDatabase.value) return
  // selectedDatabase 格式: "connectionId.dbName"
  const dbName = selectedDatabase.value.split('.').pop()
  if (!dbName) return
  selectTable(currentConnection.value.id, dbName, tableName)
}

// 从对象列表双击打开表
function onOpenTableFromObjects(tableName: string) {
  if (!currentConnection.value || !selectedDatabase.value) return
  // selectedDatabase 格式: "connectionId.dbName"
  const dbName = selectedDatabase.value.split('.').pop()
  if (!dbName) return
  mainContentRef.value?.openTable(
    currentConnection.value.id,
    dbName,
    tableName
  )
}

// 显示表右键菜单
function showTableContextMenu(
  event: MouseEvent,
  connectionId: string,
  database: string,
  tableName: string,
  tableNames?: string[]
) {
  event.preventDefault()
  event.stopPropagation()

  tableContextMenu.value = {
    show: true,
    x: event.clientX,
    y: event.clientY,
    connectionId,
    database,
    tableName,
    // 未传多选列表（左侧连接树）时只作用于当前表
    tableNames: tableNames?.length ? tableNames : [tableName]
  }
}

// 显示数据库右键菜单
function showDatabaseContextMenu(event: MouseEvent, connectionId: string, database: string) {
  event.preventDefault()
  event.stopPropagation()

  const key = `${connectionId}.${database}`
  databaseContextMenu.value = {
    show: true,
    x: event.clientX,
    y: event.clientY,
    connectionId,
    database,
    isOpened: activeDatabases.has(key)
  }
}

// 显示已保存连接右键菜单
function showSavedConnectionContextMenu(event: MouseEvent, conn: SavedConnection) {
  event.preventDefault()
  event.stopPropagation()

  savedConnectionContextMenu.value = {
    show: true,
    x: event.clientX,
    y: event.clientY,
    connectionId: conn.id,
    connectionName: conn.name
  }
}

// 显示活跃连接右键菜单
function showActiveConnectionContextMenu(event: MouseEvent, conn: ConnectionStatus) {
  event.preventDefault()
  event.stopPropagation()

  activeConnectionContextMenu.value = {
    show: true,
    x: event.clientX,
    y: event.clientY,
    connectionId: conn.id,
    connectionName: conn.name,
    driver: conn.driver ?? 'mysql'
  }
}

// 处理活跃连接菜单操作
async function handleActiveConnectionMenu(action: string) {
  const { connectionId } = activeConnectionContextMenu.value
  if (!connectionId) return

  activeConnectionContextMenu.value.show = false

  if (action === 'disconnect') {
    await disconnectConnection(connectionId)
  } else if (action === 'new-database') {
    // 新建数据库：走数据库设计器（isNewDatabase = true）
    databaseDesignerData.value = { connectionId, database: '', isNewDatabase: true }
    showDatabaseDesigner.value = true
  }
}

// 处理已保存连接菜单操作
async function handleSavedConnectionMenu(action: string) {
  const { connectionId, connectionName } = savedConnectionContextMenu.value
  if (!connectionId) return
  
  savedConnectionContextMenu.value.show = false

  switch (action) {
    case 'open':
      const conn = savedStore.savedConnections.find(c => c.id === connectionId)
      if (conn) {
        await openSavedConnection(conn)
      }
      break
    case 'edit':
      const connToEdit = savedStore.savedConnections.find(c => c.id === connectionId)
      if (connToEdit) {
        editingConnection.value = connToEdit
        showConnectionForm.value = true
      }
      break
    case 'delete': {
      const confirmed = await confirmDanger({
        message: t('app.deleteConnectionConfirm', { name: connectionName }),
        confirmText: t('common.delete'),
      })
      if (!confirmed) break
      try {
        await savedStore.deleteConnection(connectionId)
        showToast(t('app.connectionDeleted'))
      } catch (e) {
        showError(t('app.deleteConnectionFailed', { error: String(e) }))
      }
      break
    }
  }
}

/**
 * 从文件导入 SQL：对话框先弹出，再流式接收后端 import-progress 事件，
 * 让用户在导入结束前就能看到每条语句的执行结果
 */
async function startImportSql(connectionId: string, database: string, filePath: string) {
  const taskId = `import-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
  importTask.value = {
    id: taskId,
    fileName: filePath.split(/[\\/]/).pop() || filePath,
    database,
    connectionId,
    bytes: 0,
    total: 0,
    lines: [],
    done: false,
    success: false,
    error: '',
  }

  importUnlisten?.()
  importUnlisten = await listen<{
    taskId: string
    bytes: number
    total: number
    lines: ImportLogLine[]
    done: boolean
    success: boolean
  }>('import-progress', (event) => {
    const payload = event.payload
    const task = importTask.value
    if (!task || payload.taskId !== task.id) return
    task.bytes = payload.bytes
    task.total = payload.total
    if (payload.lines.length) {
      task.lines.push(...payload.lines)
      if (task.lines.length > IMPORT_LOG_LIMIT) {
        task.lines.splice(0, task.lines.length - IMPORT_LOG_LIMIT)
      }
    }
    if (payload.done) {
      task.done = true
      task.success = payload.success
    }
  })

  try {
    await browserStore.importSqlFile(connectionId, database, filePath, taskId)
    if (importTask.value) {
      importTask.value.done = true
      importTask.value.success = true
    }
    // 导入脚本可能重建表结构，关闭旧 Tab 并强制刷新表列表
    mainContentRef.value?.closeDatabaseTabs(connectionId, database)
    delete connectionDatabaseTables[connectionId]?.[database]
    await loadConnectionDatabases(connectionId, true)
    await loadDatabaseTables(connectionId, database, true)
  } catch (e: unknown) {
    const errorMessage = String(e)
    // 用户取消不是错误；真正的失败保留在弹窗里，便于对照流水
    if (importTask.value) {
      importTask.value.done = true
      importTask.value.success = false
      importTask.value.error = errorMessage.includes('用户取消') ? '' : errorMessage
    }
  } finally {
    importUnlisten?.()
    importUnlisten = null
  }
}

// 处理数据库菜单操作
async function handleDatabaseMenu(action: string) {
  const { connectionId, database } = databaseContextMenu.value
  if (!connectionId || !database) {
    console.error('Missing connectionId or database')
    return
  }
  databaseContextMenu.value.show = false

  switch (action) {
    case 'open':
      // 打开数据库（加入活跃列表并展开）
      {
        const key = `${connectionId}.${database}`
        activeDatabases.add(key)
        expandedDatabases.add(key)
        // 同时选中该数据库
        selectedDatabase.value = key
        // 加载表列表
        await loadDatabaseTables(connectionId, database)
      }
      break
    case 'close':
      // 关闭数据库（从活跃列表移除）
      closeDatabase(connectionId, database)
      break
    case 'edit':
      // 编辑数据库（打开数据库设计器）
      databaseDesignerData.value = { connectionId, database, isNewDatabase: false }
      showDatabaseDesigner.value = true
      break
    case 'new-table':
      // 打开新建表格设计器
      tableDesignerData.value = { connectionId, database, tableName: '' }
      showTableDesigner.value = true
      break
    case 'refresh':
      // 刷新数据库表列表
      try {
        await loadDatabaseTables(connectionId, database, true)
        showToast(t('app.refreshSuccess'))
      } catch (e) {
        console.error('Failed to refresh database:', e)
        showError(t('app.refreshFailed'))
      }
      break
    case 'delete': {
      // 删除数据库
      const confirmed = await confirmDanger({
        message: t('app.deleteDatabaseConfirm', { name: database }),
        requireAcknowledge: true,
      })
      if (!confirmed) break
      try {
        const driver = store.activeConnections.find(c => c.id === connectionId)?.driver ?? 'mysql'
        await browserStore.executeSql(connectionId, database, `DROP DATABASE ${quoteIdentifier(database, driver)}`)
        const key = `${connectionId}.${database}`
        // 关闭该数据库下已打开的表 Tab，再清理所有相关缓存与选中状态
        mainContentRef.value?.closeDatabaseTabs(connectionId, database)
        expandedDatabases.delete(key)
        activeDatabases.delete(key)
        for (const catKey of Array.from(expandedCategories)) {
          if (catKey.startsWith(key + '.')) expandedCategories.delete(catKey)
        }
        delete connectionDatabaseTables[connectionId]?.[database]
        if (selectedDatabase.value === key) selectedDatabase.value = ''
        if (selectedInfoType.value === 'database' && selectedInfoData.value.database === database) {
          closeInfoPanel()
        }
        if (browserStore.currentDatabase === database) {
          browserStore.currentDatabase = ''
        }
        // 重新加载数据库列表
        await loadConnectionDatabases(connectionId, true)
        showToast(t('app.deleteDatabaseSuccess', { name: database }))
      } catch (e: unknown) {
        showError(t('app.deleteDatabaseFailed', { error: String(e) }))
      }
      break
    }
    case 'export-sql':
      // 导出整库 SQL：让后端 mysqldump 直接写入用户选择的文件（无行数上限）
      try {
        const defaultFilename = `${database}_${new Date().toISOString().slice(0, 10)}.sql`
        await invoke<string>('export_database_sql_to_file', {
          connectionId,
          database,
          defaultFilename
        })
        showToast(t('app.databaseExported'))
      } catch (e: unknown) {
        console.error('Export SQL failed:', e)
        const errorMessage = String(e)
        if (!errorMessage.includes('用户取消')) {
          showError(t('app.exportSqlFailed', { error: errorMessage }))
        }
      }
      break
    case 'import-sql': {
      // 导入SQL文件 - 使用 Tauri 对话框
      const confirmed = await confirmDanger({
        title: t('app.importSql'),
        message: t('app.importSqlConfirm', { name: database }),
        confirmText: t('common.import'),
        requireAcknowledge: true,
      })
      if (!confirmed) break
      try {
        const filePath = await invoke<string>('pick_file_path', { format: 'sql' })
        // 选好文件后立刻弹出进度框，导入过程中的成功/失败逐条实时展示
        await startImportSql(connectionId, database, filePath)
      } catch (e: unknown) {
        const errorMessage = String(e)
        if (!errorMessage.includes('用户取消')) {
          showError(t('app.importSqlFailed', { error: errorMessage }))
        }
      }
      break
    }
  }
}

// 处理表菜单操作
async function handleTableMenu(action: string) {
  const { connectionId, database, tableName } = tableContextMenu.value
  tableContextMenu.value.show = false

  switch (action) {
    case 'view-data':
      // 切换到数据浏览视图
      currentView.value = 'data'
      selectedDatabase.value = `${connectionId}.${database}`
      // 先选中表
      selectTable(connectionId, database, tableName)
      // 再打开表
      openTable(connectionId, database, tableName)
      break
    case 'design-table':
      // 打开表格设计器
      tableDesignerData.value = { connectionId, database, tableName }
      showTableDesigner.value = true
      break
    case 'export':
      // 切换到数据浏览视图并打开表
      currentView.value = 'data'
      selectedDatabase.value = `${connectionId}.${database}`
      selectTable(connectionId, database, tableName)
      openTable(connectionId, database, tableName)
      // 调用 DataTable 的导出方法
      setTimeout(() => {
        mainContentRef.value?.dataTableRef?.showExportDialog?.()
      }, 100)
      break
    case 'import':
      // 切换到数据浏览视图并打开表
      currentView.value = 'data'
      selectedDatabase.value = `${connectionId}.${database}`
      selectTable(connectionId, database, tableName)
      openTable(connectionId, database, tableName)
      // 调用 DataTable 的导入方法
      setTimeout(() => {
        mainContentRef.value?.dataTableRef?.showImportDialog?.()
      }, 100)
      break
    case 'copy-name':
      await navigator.clipboard.writeText(tableName)
      break
    case 'refresh-tables':
      // 刷新表列表
      try {
        const result = await browserStore.loadObjectTree(connectionId, database)
        // 更新本地缓存
        if (!connectionDatabaseTables[connectionId]) {
          connectionDatabaseTables[connectionId] = {}
        }
        connectionDatabaseTables[connectionId][database] = result
        showToast(t('app.refreshSuccess'))
      } catch (e) {
        console.error('Failed to refresh tables:', e)
        showError(t('app.refreshFailed'))
      }
      break
    case 'copy-table': {
      // 复制表结构和数据
      const newTableName = tableName + '_copy'
      try {
        await browserStore.copyTable(connectionId, database, tableName, newTableName)
        // 刷新表列表并更新缓存
        const result = await browserStore.loadObjectTree(connectionId, database)
        if (!connectionDatabaseTables[connectionId]) {
          connectionDatabaseTables[connectionId] = {}
        }
        connectionDatabaseTables[connectionId][database] = result
        showToast(t('app.copyTableSuccess', { name: newTableName }))
      } catch (e: unknown) {
        showError(t('app.copyTableFailed', { error: String(e) }))
      }
      break
    }
    case 'clear-table': {
      // 清空表（DELETE - 不清除ID自增值）
      const confirmed = await confirmDanger({
        message: t('app.clearTableConfirm', { name: tableName }),
        requireAcknowledge: true,
      })
      if (!confirmed) break
      try {
        await browserStore.clearTable(connectionId, database, tableName)
        // 如果当前正在查看该表数据，刷新数据
        if (selectedTable.value === `${connectionId}.${database}.${tableName}`) {
          mainContentRef.value?.dataTableRef?.refreshData?.()
        }
        showToast(t('app.clearTableSuccess', { name: tableName }))
      } catch (e: unknown) {
        showError(t('app.clearTableFailed', { error: String(e) }))
      }
      break
    }
    case 'truncate-table': {
      // 截断表（清空表数据）
      const confirmed = await confirmDanger({
        message: t('app.truncateTableConfirm', { name: tableName }),
        requireAcknowledge: true,
      })
      if (!confirmed) break
      try {
        await browserStore.truncateTable(connectionId, database, tableName)
        // 如果当前正在查看该表数据，刷新数据
        if (selectedTable.value === `${connectionId}.${database}.${tableName}`) {
          mainContentRef.value?.dataTableRef?.refreshData?.()
        }
        showToast(t('app.truncateTableSuccess', { name: tableName }))
      } catch (e: unknown) {
        showError(t('app.truncateTableFailed', { error: String(e) }))
      }
      break
    }
    case 'delete-table': {
      // 删除表（中间面板多选时批量删除）
      const names = tableContextMenu.value.tableNames.length
        ? tableContextMenu.value.tableNames
        : [tableName]
      const confirmed = await confirmDanger({
        message:
          names.length > 1
            ? t('app.deleteTablesConfirm', { count: names.length, names: names.join(', ') })
            : t('app.deleteTableConfirm', { name: tableName }),
        requireAcknowledge: true,
      })
      if (!confirmed) break
      try {
        await browserStore.dropTables(connectionId, database, names)
        // 关闭这些表已打开的 Tab，避免残留失效数据
        for (const name of names) {
          mainContentRef.value?.dataTableRef?.closeTableById(`${connectionId}.${database}.${name}`)
        }
        // 如果当前正在查看其中一张表，清除选择
        if (names.some(name => selectedTable.value === `${connectionId}.${database}.${name}`)) {
          selectedTable.value = ''
        }
        // 刷新表列表并更新缓存
        const result = await browserStore.loadObjectTree(connectionId, database)
        if (!connectionDatabaseTables[connectionId]) {
          connectionDatabaseTables[connectionId] = {}
        }
        connectionDatabaseTables[connectionId][database] = result
        showToast(
          names.length > 1
            ? t('app.deleteTablesSuccess', { count: names.length })
            : t('app.deleteTableSuccess', { name: tableName })
        )
      } catch (e: unknown) {
        showError(t('app.deleteTableFailed', { error: String(e) }))
      }
      break
    }
  }
}

// 窗口大小变化时重新计算
function registerCommands() {
  registry.register({
    id: 'new-query',
    title: t('commandPalette.newQuery'),
    category: t('commandPalette.categoryTabs'),
    shortcut: 'Ctrl+N',
    available: () => !!currentConnection.value,
    action: openNewQueryTab,
  })
  registry.register({
    id: 'new-console',
    title: t('commandPalette.newConsole'),
    category: t('commandPalette.categoryTabs'),
    shortcut: 'Ctrl+Shift+T',
    available: () => !!currentConnection.value,
    action: openNewConsoleTab,
  })
  registry.register({
    id: 'close-current-tab',
    title: t('commandPalette.closeCurrentTab'),
    category: t('commandPalette.categoryTabs'),
    action: () => {
      const active = mainContentRef.value?.activeTabId
      if (active && active !== 'objects') {
        mainContentRef.value?.closeTab(active)
      }
    },
  })
  registry.register({
    id: 'refresh-object-tree',
    title: t('commandPalette.refreshObjectTree'),
    category: t('commandPalette.categoryNav'),
    // 未绑定全局快捷键（Ctrl+R 可能在 WebView 中触发重载，不抢占），不显示假提示
    available: () => !!currentConnection.value,
    action: () => {
      if (currentConnection.value) {
        void loadConnectionDatabases(currentConnection.value.id, true).then(ok => {
          if (ok) showToast(t('app.refreshSuccess'))
          else showError(t('app.refreshFailed'))
        })
      }
    },
  })
  registry.register({
    id: 'toggle-theme',
    title: themeStore.theme === 'dark' ? t('commandPalette.switchLight') : t('commandPalette.switchDark'),
    category: t('commandPalette.categoryApp'),
    shortcut: 'Ctrl+Shift+L',
    action: () => { themeStore.toggleTheme(); registry.updateCommandTitle('toggle-theme', themeStore.theme === 'dark' ? t('commandPalette.switchLight') : t('commandPalette.switchDark')) },
  })
  registry.register({
    id: 'toggle-language',
    title: t('commandPalette.switchLanguage'),
    category: t('commandPalette.categoryApp'),
    action: () => { toggleLocale(); registry.updateCommandTitle('toggle-language', t('commandPalette.switchLanguage')) },
  })
  registry.register({
    id: 'open-connection-form',
    title: t('commandPalette.openConnectionForm'),
    category: t('commandPalette.categoryApp'),
    action: openConnectionForm,
  })
}

// 已注册的表命令 id，用于清理失效项
const tableCommandIds = new Set<string>()

/**
 * 把已加载的表注册成命令面板里的「跳转到表」命令。
 *
 * 只覆盖当前已展开过的库，不做额外后端请求；表列表变化时整体对齐一次。
 */
function syncTableCommands() {
  const current = new Set<string>()
  for (const [connId, tablesByDb] of Object.entries(connectionDatabaseTables)) {
    for (const [dbName, tables] of Object.entries(tablesByDb)) {
      for (const table of tables) {
        const id = `table:${connId}.${dbName}.${table.name}`
        current.add(id)
        registry.register({
          id,
          title: table.name,
          category: `${t('commandPalette.categoryTables')} · ${dbName}`,
          action: () => openTable(connId, dbName, table.name),
        })
      }
    }
  }
  // 表被删除或连接断开后，清掉残留命令
  for (const id of tableCommandIds) {
    if (!current.has(id)) registry.unregister(id)
  }
  tableCommandIds.clear()
  for (const id of current) tableCommandIds.add(id)
}

// 语言切换后重建命令面板：registerCommands 里的 t() 是注册时快照，不重建会停留在旧语言
watch(locale, () => {
  registry.reset()
  tableCommandIds.clear()
  registerCommands()
  syncTableCommands()
})

/** 焦点的可编辑判定：`el?.closest` 在个别宿主环境（如非 Element 事件目标）不存在 */
function isEditableTarget(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null
  if (!el || typeof el.closest !== 'function') return false
  return el.closest('input, textarea, select, [contenteditable="true"]') !== null
}

/** 焦点在数据表格内时，让位给 DataTable 自己的快捷键（Ctrl+N 新增行 / Ctrl+S 保存） */
function isInDataTable(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null
  if (!el || typeof el.closest !== 'function') return false
  return el.closest('.data-table') !== null
}

function onGlobalKeydown(e: KeyboardEvent) {
  const mod = e.ctrlKey || e.metaKey
  // Ctrl/Cmd+K 打开/关闭命令面板
  if (mod && (e.key === 'k' || e.key === 'K')) {
    e.preventDefault()
    if (registry.open) {
      registry.closePalette()
    } else {
      registry.openPalette()
    }
  }
  // 命令面板里展示的 Ctrl+N / Ctrl+Shift+T / Ctrl+Shift+L 在此真实绑定，
  // commandRegistry 的 shortcut 字段只登记这些已经生效的键
  if (
    mod &&
    !e.altKey &&
    !isEditableTarget(e.target) &&
    !isInDataTable(e.target) &&
    !hasOpenModal.value &&
    !registry.open
  ) {
    const key = e.key.toLowerCase()
    if (e.shiftKey) {
      if (key === 't' && registry.execute('new-console')) {
        e.preventDefault()
        return
      }
      if (key === 'l' && registry.execute('toggle-theme')) {
        e.preventDefault()
        return
      }
    } else if (key === 'n' && registry.execute('new-query')) {
      e.preventDefault()
      return
    }
  }
  // Esc 关闭最顶层弹窗（不抢合并态快捷键，仅在没有其它浮层接管时生效）
  if (e.key === 'Escape' && !e.defaultPrevented) {
    const closers: Array<[boolean, () => void]> = [
      [showGlobalSearch.value, () => (showGlobalSearch.value = false)],
      [showHistory.value, () => (showHistory.value = false)],
      [showTableDesigner.value, () => (showTableDesigner.value = false)],
      [showBackup.value, () => (showBackup.value = false)],
      [showMonitor.value, () => (showMonitor.value = false)],
      [showSchemaDiff.value, () => (showSchemaDiff.value = false)],
      [showAbout.value, () => (showAbout.value = false)],
      [showConnectionForm.value, () => (showConnectionForm.value = false)]
    ]
    const hit = closers.find(([open]) => open)
    if (hit) {
      hit[1]()
      e.preventDefault()
      return
    }
  }

  // 面板打开时阻止下层组件响应方向键等
  if (registry.open) {
    if (['ArrowUp', 'ArrowDown', 'Enter', 'Escape'].includes(e.key)) {
      e.stopPropagation()
    }
  }
}

// 关闭窗口/刷新前兜底：存在未保存的数据表编辑或表结构修改时提示用户确认
function handleBeforeUnload(e: BeforeUnloadEvent) {
  if (!hasUnsavedSources()) return
  e.preventDefault()
  e.returnValue = ''
}

// ===== `.sql` 文件打开：文件关联 / 单实例转发 / 拖拽 =====
interface OpenedSqlFile {
  path: string
  name: string
  content: string
}

let sqlFileUnlisten: UnlistenFn | null = null
let dragDropUnlisten: UnlistenFn | null = null

function openSqlFiles(files: OpenedSqlFile[]) {
  for (const file of files) {
    // 每个文件各开一个新查询页：不覆盖当前编辑中的内容，天然不触发未保存守卫
    mainContentRef.value?.openSqlFile(file.name, file.content)
  }
}

/** 取走后端排队的文件（启动参数与第二实例转发都入队到这里） */
async function drainPendingSqlFiles() {
  try {
    const files = await invoke<OpenedSqlFile[]>('take_pending_sql_files')
    if (files.length > 0) openSqlFiles(files)
  } catch {
    // 浏览器预览等非 Tauri 环境没有该命令，忽略
  }
}

async function setupSqlFileOpeners() {
  try {
    // 事件只作唤醒：文件可能早于前端就绪就已入队，统一从队列取，避免重复打开
    sqlFileUnlisten = await listen('open-sql-file', () => { void drainPendingSqlFiles() })
    await drainPendingSqlFiles()
  } catch {
    // 非 Tauri 环境忽略
  }

  try {
    dragDropUnlisten = await getCurrentWebview().onDragDropEvent(async (event) => {
      if (event.payload.type !== 'drop') return
      for (const path of event.payload.paths.filter(isSqlFilePath)) {
        try {
          const file = await invoke<OpenedSqlFile>('read_sql_file', { path })
          openSqlFiles([file])
        } catch (error) {
          showError(String(error))
        }
      }
    })
  } catch {
    // 非 Tauri 环境忽略
  }
}

onMounted(() => {
  // 初始化主题
  themeStore.init()

  registerCommands()
  // 表列表加载/刷新后同步「跳转到表」命令
  watch(connectionDatabaseTables, syncTableCommands, { deep: true })
  window.addEventListener('keydown', onGlobalKeydown)
  window.addEventListener('beforeunload', handleBeforeUnload)
  document.addEventListener('click', handleDocumentClick)
  // 定时拉取后端连接状态：心跳检测出的断线/重连需要主动同步到 UI
  store.startStatusPolling()
  // .sql 文件关联双击 / 第二实例转发 / 拖拽打开
  void setupSqlFileOpeners()
})

onUnmounted(() => {
  store.stopStatusPolling()
  importUnlisten?.()
  sqlFileUnlisten?.()
  dragDropUnlisten?.()
  document.removeEventListener('mousemove', onResize)
  document.removeEventListener('mouseup', stopResize)
  window.removeEventListener('keydown', onGlobalKeydown)
  window.removeEventListener('beforeunload', handleBeforeUnload)
  document.removeEventListener('click', handleDocumentClick)
  registry.reset()
})
</script>

<style>
* {
  box-sizing: border-box;
}

body {
  margin: 0;
  font-family: var(--font-sans);
  font-size: var(--text-base);
  color: var(--text-primary);
  background: var(--bg-primary);
}
</style>

<style scoped>
.app {
  height: 100vh;
  display: flex;
  flex-direction: column;
}

.app-body {
  flex: 1;
  display: flex;
  overflow: hidden;
}

/* 左侧边栏 */
.sidebar-left {
  min-width: 200px;
  max-width: 400px;
  width: 240px;
  background: var(--panel);
  border-right: none;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

/* 右侧信息面板 */
.sidebar-right {
  min-width: 200px;
  max-width: 800px;
  width: 280px;
  background: var(--panel);
  border-left: none;
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

/* 拖拽分隔条 */
.resizer {
  width: 3px;
  background: transparent;
  cursor: col-resize;
  transition: background var(--duration-fast);
  user-select: none;
  -webkit-user-select: none;
}

.resizer:hover {
  background: var(--brand-500);
}

/* 弹窗 */
.modal-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: var(--bg-overlay);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: var(--z-modal);
  padding: var(--space-4);
}

.modal-content {
  background: var(--overlay);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xl);
  max-width: 480px;
  width: 90%;
  max-height: 90vh;
  overflow: auto;
  animation: modal-enter var(--duration-normal) var(--ease-out);
}

@keyframes modal-enter {
  from {
    opacity: 0;
    transform: scale(0.95) translateY(-10px);
  }
  to {
    opacity: 1;
    transform: scale(1) translateY(0);
  }
}

.modal-content.modal-large {
  max-width: 1200px;
  width: 95%;
  height: 85vh;
}

/* 表格设计器：弹层本身不滚动，由内部 .designer-content 单层滚动 */
.modal-content.modal-designer {
  overflow: hidden;
}

/* 弹层打开时隐藏背景布局的滚动条，避免其在遮罩之上留下残影 */
.app.modal-open .app-body * {
  scrollbar-width: none;
}

.app.modal-open .app-body *::-webkit-scrollbar {
  display: none;
}

.modal-content.modal-connection {
  max-width: 760px;
  width: 92%;
}

/* 备份还原：内容较少，使用紧凑宽度，并保证下拉列表有足够展开空间 */
.modal-content.modal-backup {
  max-width: 560px;
  width: 92%;
  min-height: 480px;
}

/* 导入进度：需要容纳逐条执行流水 */
.modal-content.modal-import {
  max-width: 680px;
  width: 92%;
}

.modal-content.modal-about {
  max-width: 520px;
  width: 92%;
}

/* 右键菜单 */
.context-menu {
  position: fixed;
  background: var(--overlay);
  border: 1px solid var(--hairline-strong);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-lg);
  z-index: var(--z-popover);
  min-width: 160px;
  padding: var(--space-1) 0;
  animation: menu-enter var(--duration-fast) var(--ease-out);
}

@keyframes menu-enter {
  from {
    opacity: 0;
    transform: scale(0.95);
  }
  to {
    opacity: 1;
    transform: scale(1);
  }
}

.menu-item {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-2) var(--space-4);
  cursor: pointer;
  font-size: var(--text-sm);
  color: var(--text-primary);
  transition: background var(--duration-fast);
}

.menu-item:hover {
  background: var(--hover-fill);
}

.menu-item-danger {
  color: var(--error-500);
}

.menu-item-danger:hover {
  background: rgba(239, 68, 68, 0.12);
  color: var(--error-500);
}

.menu-icon {
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.menu-divider {
  height: 1px;
  background: var(--hairline);
  margin: var(--space-1) 0;
}

/* Mobile styles */
@media (max-width: 767px) {

  /* Sidebar as drawer */
  .sidebar-left {
    position: fixed;
    left: 0;
    top: var(--header-height);
    bottom: 0;
    width: 280px !important;
    min-width: 280px !important;
    z-index: var(--z-modal);
    transform: translateX(-100%);
    transition: transform var(--duration-normal);
  }

  .app-body.sidebar-open .sidebar-left {
    transform: translateX(0);
  }

  /* Add backdrop */
  .app-body.sidebar-open::before {
    content: '';
    position: fixed;
    inset: var(--header-height) 0 0 0;
    background: var(--bg-overlay);
    z-index: var(--z-modal-backdrop);
  }
}

/* Tablet styles */
@media (max-width: 1023px) {

  /* Info panel as drawer */
  .sidebar-right {
    position: fixed;
    right: 0;
    top: var(--header-height);
    bottom: 0;
    width: 320px !important;
    min-width: 320px !important;
    z-index: var(--z-modal);
    transform: translateX(100%);
    transition: transform var(--duration-normal);
    box-shadow: none;
  }

  .app-body.panel-open .sidebar-right {
    transform: translateX(0);
    box-shadow: var(--shadow-xl);
  }

  /* Add backdrop for panel */
  .app-body.panel-open::after {
    content: '';
    position: fixed;
    inset: var(--header-height) 0 0 0;
    background: var(--bg-overlay);
    z-index: var(--z-modal-backdrop);
  }
}

/* Touch device optimizations */
@media (pointer: coarse) {

  .menu-item {
    min-height: 44px;
  }
}

/* High DPI screens */
@media (-webkit-min-device-pixel-ratio: 2), (min-resolution: 192dpi) {
  .resizer {
    width: 2px;
  }
}
</style>
