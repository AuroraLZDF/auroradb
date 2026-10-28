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
      @open-license="licenseStore.openDialog()"
      @open-about="showAbout = true"
    />

    <div class="app-body" :class="{ 'sidebar-open': sidebarOpen, 'panel-open': infoPanelOpen }">
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
      />

      <!-- 右分隔条 -->
      <div class="resizer resizer-right" @mousedown="startResize('right', $event)"></div>

      <!-- 右侧：信息面板 -->
      <div
        class="sidebar-right"
        :style="{ width: rightWidth + 'px' }"
      >
        <div class="info-panel" v-if="selectedInfoType">
          <InfoPanel
            :type="selectedInfoType"
            :data="selectedInfoData"
            @close="closeInfoPanel"
          />
        </div>
        <div v-else class="empty-panel">
          <div class="empty-content">
            <Info class="empty-icon" :size="48" />
            <p>{{ t('app.emptyPanelHint') }}</p>
          </div>
        </div>
      </div>
    </div>

    <!-- 连接表单弹窗 -->
    <div v-if="showConnectionForm" class="modal-overlay">
      <div class="modal-content modal-connection">
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
      :style="{ top: tableContextMenu.y + 'px', left: tableContextMenu.x + 'px' }"
      @click.stop
    >
      <div class="menu-item" @click="handleTableMenu('view-data')">
        <Eye class="menu-icon" :size="14" /> {{ t('app.viewData') }}
      </div>
      <div class="menu-item" @click="handleTableMenu('design-table')">
        <Settings class="menu-icon" :size="14" /> {{ t('app.designTable') }}
      </div>
      <div class="menu-item" @click="handleTableMenu('refresh-tables')">
        <RefreshCw class="menu-icon" :size="14" /> {{ t('common.refresh') }}
      </div>
      <div class="menu-divider"></div>
      <div class="menu-item" @click="handleTableMenu('export')">
        <Download class="menu-icon" :size="14" /> {{ t('app.exportData') }}
      </div>
      <div class="menu-item" @click="handleTableMenu('import')">
        <Upload class="menu-icon" :size="14" /> {{ t('app.importData') }}
      </div>
      <div class="menu-divider"></div>
      <div class="menu-item" @click="handleTableMenu('copy-name')">
        <Copy class="menu-icon" :size="14" /> {{ t('app.copyName') }}
      </div>
      <div class="menu-item" @click="handleTableMenu('copy-table')">
        <Copy class="menu-icon" :size="14" /> {{ t('app.copyTable') }}
      </div>
      <div class="menu-item" @click="handleTableMenu('clear-table')">
        <Eraser class="menu-icon" :size="14" /> {{ t('app.clearTable') }}
      </div>
      <div class="menu-item" @click="handleTableMenu('truncate-table')">
        <Trash2 class="menu-icon" :size="14" /> {{ t('app.truncateTable') }}
      </div>
      <div class="menu-item menu-item-danger" @click="handleTableMenu('delete-table')">
        <Trash2 class="menu-icon" :size="14" /> {{ t('app.deleteTable') }}
      </div>
    </div>

    <!-- 数据库右键菜单 -->
    <div
      v-if="databaseContextMenu.show"
      class="context-menu"
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
      <div class="menu-item" @click="handleDatabaseMenu('edit')">
        <Settings class="menu-icon" :size="14" /> {{ t('app.editDatabase') }}
      </div>
      <div class="menu-item" @click="handleDatabaseMenu('new-table')">
        <Plus class="menu-icon" :size="14" /> {{ t('app.newTable') }}
      </div>
      <div class="menu-item" @click="handleDatabaseMenu('refresh')">
        <RefreshCw class="menu-icon" :size="14" /> {{ t('common.refresh') }}
      </div>
      <div class="menu-divider"></div>
      <div class="menu-item" @click="handleDatabaseMenu('export-sql')">
        <Download class="menu-icon" :size="14" /> {{ t('app.exportSql') }}
      </div>
      <div class="menu-item" @click="handleDatabaseMenu('import-sql')">
        <Upload class="menu-icon" :size="14" /> {{ t('app.importSql') }}
      </div>
      <div class="menu-divider"></div>
      <div class="menu-item menu-item-danger" @click="handleDatabaseMenu('delete')">
        <Trash2 class="menu-icon" :size="14" /> {{ t('app.deleteDatabase') }}
      </div>
    </div>

    <!-- 已保存连接右键菜单 -->
    <div
      v-if="savedConnectionContextMenu.show"
      class="context-menu"
      :style="{ top: savedConnectionContextMenu.y + 'px', left: savedConnectionContextMenu.x + 'px' }"
      @click.stop
    >
      <div class="menu-item" @click="handleSavedConnectionMenu('open')">
        <Plug class="menu-icon" :size="14" /> {{ t('app.openConnection') }}
      </div>
      <div class="menu-item" @click="handleSavedConnectionMenu('edit')">
        <Settings class="menu-icon" :size="14" /> {{ t('app.editConnection') }}
      </div>
      <div class="menu-divider"></div>
      <div class="menu-item menu-item-danger" @click="handleSavedConnectionMenu('delete')">
        <Trash2 class="menu-icon" :size="14" /> {{ t('app.deleteConnection') }}
      </div>
    </div>

    <!-- 活跃连接右键菜单 -->
    <div
      v-if="activeConnectionContextMenu.show"
      class="context-menu"
      :style="{ top: activeConnectionContextMenu.y + 'px', left: activeConnectionContextMenu.x + 'px' }"
      @click.stop
    >
      <div
        v-if="activeConnectionContextMenu.driver !== 'postgres'"
        class="menu-item"
        @click="handleActiveConnectionMenu('new-database')"
      >
        <Plus class="menu-icon" :size="14" /> {{ t('app.newDatabase') }}
      </div>
      <div class="menu-divider"></div>
      <div class="menu-item menu-item-danger" @click="handleActiveConnectionMenu('disconnect')">
        <X class="menu-icon" :size="14" /> {{ t('app.disconnect') }}
      </div>
    </div>

    <!-- 授权激活弹窗 -->
    <div v-if="licenseStore.dialogVisible" class="modal-overlay">
      <div class="modal-content modal-license">
        <LicenseDialog @close="licenseStore.closeDialog()" />
      </div>
    </div>

    <!-- 关于弹窗 -->
    <div v-if="showAbout" class="modal-overlay">
      <div class="modal-content modal-about">
        <AboutDialog @close="showAbout = false" />
      </div>
    </div>

    <!-- Schema 对比弹窗 -->
    <div v-if="showSchemaDiff" class="modal-overlay">
      <div class="modal-content modal-large">
        <SchemaDiff
          :connections="activeConnections"
          :initial-connection-id="currentConnection?.id"
          :initial-database="browserStore.currentDatabase"
          @close="showSchemaDiff = false"
        />
      </div>
    </div>

    <!-- 实例监控与审计弹窗 -->
    <div v-if="showMonitor" class="modal-overlay">
      <div class="modal-content modal-large">
        <MonitorPanel
          :connections="activeConnections"
          :initial-connection-id="currentConnection?.id"
          @close="showMonitor = false"
        />
      </div>
    </div>

    <!-- 备份还原弹窗 -->
    <div v-if="showBackup" class="modal-overlay">
      <div class="modal-content modal-backup">
        <BackupDialog @close="showBackup = false" @restored="onDatabaseRestored" />
      </div>
    </div>

    <!-- 跨库全局搜索弹窗 -->
    <div v-if="showGlobalSearch" class="modal-overlay">
      <div class="modal-content modal-large">
        <GlobalSearchDialog
          :initial-connection-id="currentConnection?.id"
          :initial-database="browserStore.currentDatabase"
          @open-table="handleGlobalSearchOpenTable"
          @close="showGlobalSearch = false"
        />
      </div>
    </div>

    <!-- 表格设计器弹窗 -->
    <div v-if="showTableDesigner" class="modal-overlay">
      <div class="modal-content modal-large modal-designer">
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
      <div class="modal-content">
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
import { t, toggleLocale } from '@/i18n'
import { invoke } from '@tauri-apps/api/core'
import { useConnectionStore, type ConnectionStatus, type SshConfig } from '@/stores/connection'
import { useSavedConnectionsStore, type SavedConnection } from '@/stores/savedConnections'
import { useDataBrowserStore, type DatabaseObject } from '@/stores/dataBrowser'
import { useThemeStore } from '@/stores/theme'
import { useLicenseStore } from '@/stores/license'
import HeaderToolbar from '@/components/layout/HeaderToolbar.vue'
import MainContent from '@/components/layout/MainContent.vue'
import ConnectionTree from '@/components/sidebar/ConnectionTree.vue'
import ConnectionForm from '@/components/ConnectionForm.vue'
import InfoPanel from '@/components/InfoPanel.vue'
import TableDesigner from '@/components/TableDesigner.vue'
import DatabaseDesigner from '@/components/DatabaseDesigner.vue'
import SchemaDiff from '@/components/SchemaDiff.vue'
import MonitorPanel from '@/components/MonitorPanel.vue'
import LicenseDialog from '@/components/LicenseDialog.vue'
import AboutDialog from '@/components/AboutDialog.vue'
import ToastNotification from '@/components/ui/Toast.vue'
import ConfirmDialog from '@/components/ui/ConfirmDialog.vue'
import CommandPalette from '@/components/CommandPalette.vue'
import BackupDialog from '@/components/BackupDialog.vue'
import GlobalSearchDialog from '@/components/GlobalSearchDialog.vue'
import { useCommandRegistryStore } from '@/stores/commandRegistry'
import { useToast } from '@/composables/useToast'
import { confirmDanger } from '@/composables/useConfirm'
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
  Info,
} from '@/components/icons'

const store = useConnectionStore()
const savedStore = useSavedConnectionsStore()
const browserStore = useDataBrowserStore()
const themeStore = useThemeStore()
const licenseStore = useLicenseStore()
const registry = useCommandRegistryStore()
const mainContentRef = ref<InstanceType<typeof MainContent> | null>(null)
const toastRef = ref<InstanceType<typeof ToastNotification> | null>(null)

// 响应式断点
const { isMobile, isSmallScreen, isDesktop } = useBreakpoints()

// 响应式面板状态
const sidebarOpen = ref(false)
const infoPanelOpen = ref(true)

// 监听断点变化
watch(isDesktop, (desktop) => {
  if (desktop) {
    sidebarOpen.value = false
    infoPanelOpen.value = true
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
  tableName: ''
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
document.addEventListener('click', (e: MouseEvent) => {
  const target = e.target as HTMLElement
  // 如果点击的不是右键菜单内部，则关闭菜单
  if (!target.closest('.context-menu')) {
    tableContextMenu.value.show = false
    databaseContextMenu.value.show = false
    savedConnectionContextMenu.value.show = false
    activeConnectionContextMenu.value.show = false
  }
})

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

// 跨库全局搜索
const showGlobalSearch = ref(false)

// 关于
const showAbout = ref(false)

// 专业版入口：未激活时先弹出激活窗口
function handleOpenSchemaDiff() {
  if (licenseStore.requirePro('schema-diff', t('license.feature.schema-diff'))) {
    showSchemaDiff.value = true
  }
}

function handleOpenErDiagram() {
  if (!licenseStore.requirePro('er-diagram', t('license.feature.er-diagram'))) return
  if (!selectedDatabase.value) {
    showError(t('erDiagram.needsDatabase'))
    return
  }
  mainContentRef.value?.openErDiagramTab(selectedDatabase.value)
}

function handleOpenMonitor() {
  if (licenseStore.requirePro('monitor', t('license.feature.monitor'))) {
    showMonitor.value = true
  }
}

function handleOpenBackup() {
  if (!licenseStore.requirePro('backup', t('license.feature.backup'))) return
  showBackup.value = true
}

function handleOpenGlobalSearch() {
  if (!licenseStore.requirePro('global-search', t('license.feature.global-search'))) return
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
  licenseStore.dialogVisible ||
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
const rightWidth = ref(320)
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
  const activeNames = new Set(activeConnections.value.map(c => c.name))
  // 已保存但尚未活跃的连接（离线）
  for (const s of savedStore.savedConnections) {
    if (!activeNames.has(s.name)) {
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
  store.switchConnection(conn.id)
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

  // SSH 隧道为专业版功能，未激活时直接引导激活
  if (savedConn.sshEnabled && !licenseStore.requirePro('ssh-tunnel', t('license.feature.ssh-tunnel'))) {
    return
  }

  // 获取密码
  const password = await savedStore.getPassword(savedConn.id)

  if (!password) {
    // 没有保存密码，需要重新输入
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
    if (licenseStore.handleError(e)) return
    alert(t('app.connectFailed', { error: String(e) }))
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
    activeTool.value = 'connection'
    currentView.value = 'data'
  } catch (e) {
    console.error('Failed to disconnect:', e)
  }
}

// 加载连接的数据库列表，force=true 时忽略缓存强制重新拉取
async function loadConnectionDatabases(connectionId: string, force = false): Promise<boolean> {
  if (!force && connectionDatabases[connectionId]) return true // 已加载
  // PostgreSQL 的对象树尚未迁移，跳过请求避免无谓的后端报错
  if (activeConnections.value.find(c => c.id === connectionId)?.driver === 'postgres') return false

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
    console.error('Failed to load tables:', e)
  }
}

// 单击选中连接 - 显示连接信息
function selectConnection(conn: { id: string; name: string; host: string; port: number; username?: string }) {
  selectedInfoType.value = 'connection'
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
  // 确保信息面板打开
  infoPanelOpen.value = true
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
  selectedTable.value = `${currentConnection.value.id}.${selectedDatabase.value}.${tableName}`
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
function showTableContextMenu(event: MouseEvent, connectionId: string, database: string, tableName: string) {
  event.preventDefault()
  event.stopPropagation()

  tableContextMenu.value = {
    show: true,
    x: event.clientX,
    y: event.clientY,
    connectionId,
    database,
    tableName
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
    case 'delete':
      if (confirm(t('app.deleteConnectionConfirm', { name: connectionName }))) {
        await savedStore.deleteConnection(connectionId)
        showToast(t('app.connectionDeleted'))
      }
      break
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
        await browserStore.executeSql(connectionId, database, `DROP DATABASE ${quoteIdentifier(database)}`)
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
        alert(t('app.deleteDatabaseFailed', { error: String(e) }))
      }
      break
    }
    case 'export-sql':
      // 导出整库 SQL：让后端 mysqldump 直接写入用户选择的文件（无行数上限）
      if (!licenseStore.requirePro('import-export', t('license.feature.import-export'))) break
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
        if (licenseStore.handleError(e)) break
        const errorMessage = String(e)
        if (!errorMessage.includes('用户取消')) {
          alert(t('app.exportSqlFailed', { error: errorMessage }))
        }
      }
      break
    case 'import-sql': {
      // 导入SQL文件 - 使用 Tauri 对话框
      if (!licenseStore.requirePro('import-export', t('license.feature.import-export'))) break
      const confirmed = await confirmDanger({
        message: t('app.importSqlConfirm', { name: database }),
        requireAcknowledge: true,
      })
      if (!confirmed) break
      try {
        const filePath = await invoke<string>('pick_file_path', { format: 'sql' })
        await browserStore.importSqlFile(connectionId, database, filePath)
        // 导入脚本可能重建表结构，关闭旧 Tab 并强制刷新表列表
        mainContentRef.value?.closeDatabaseTabs(connectionId, database)
        delete connectionDatabaseTables[connectionId]?.[database]
        await loadConnectionDatabases(connectionId, true)
        await loadDatabaseTables(connectionId, database, true)
        showToast(t('app.databaseImported'))
      } catch (e: unknown) {
        if (licenseStore.handleError(e)) break
        const errorMessage = String(e)
        if (!errorMessage.includes('用户取消')) {
          alert(t('app.importSqlFailed', { error: errorMessage }))
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
      if (!licenseStore.requirePro('import-export', t('license.feature.import-export'))) break
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
      if (!licenseStore.requirePro('import-export', t('license.feature.import-export'))) break
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
        alert(t('app.copyTableFailed', { error: String(e) }))
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
        alert(t('app.clearTableFailed', { error: String(e) }))
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
        alert(t('app.truncateTableFailed', { error: String(e) }))
      }
      break
    }
    case 'delete-table': {
      // 删除表
      const confirmed = await confirmDanger({
        message: t('app.deleteTableConfirm', { name: tableName }),
        requireAcknowledge: true,
      })
      if (!confirmed) break
      try {
        await browserStore.dropTable(connectionId, database, tableName)
        // 关闭该表已打开的 Tab，避免残留失效数据
        mainContentRef.value?.dataTableRef?.closeTableById(`${connectionId}.${database}.${tableName}`)
        // 如果当前正在查看该表，清除选择
        if (selectedTable.value === `${connectionId}.${database}.${tableName}`) {
          selectedTable.value = ''
        }
        // 刷新表列表并更新缓存
        const result = await browserStore.loadObjectTree(connectionId, database)
        if (!connectionDatabaseTables[connectionId]) {
          connectionDatabaseTables[connectionId] = {}
        }
        connectionDatabaseTables[connectionId][database] = result
        showToast(t('app.deleteTableSuccess', { name: tableName }))
      } catch (e: unknown) {
        alert(t('app.deleteTableFailed', { error: String(e) }))
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
    shortcut: 'Ctrl+R',
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
    id: 'open-activation',
    title: t('commandPalette.openActivation'),
    category: t('commandPalette.categoryApp'),
    action: () => licenseStore.openDialog(),
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

function onGlobalKeydown(e: KeyboardEvent) {
  // Ctrl/Cmd+K 打开/关闭命令面板
  if ((e.ctrlKey || e.metaKey) && (e.key === 'k' || e.key === 'K')) {
    e.preventDefault()
    if (registry.open) {
      registry.closePalette()
    } else {
      registry.openPalette()
    }
  }
  // 面板打开时阻止下层组件响应方向键等
  if (registry.open) {
    if (['ArrowUp', 'ArrowDown', 'Enter', 'Escape'].includes(e.key)) {
      e.stopPropagation()
    }
  }
}

onMounted(() => {
  // 初始化主题
  themeStore.init()
  // 读取本地授权状态，供专业版入口做前置判断
  void licenseStore.loadStatus()

  registerCommands()
  // 表列表加载/刷新后同步「跳转到表」命令
  watch(connectionDatabaseTables, syncTableCommands, { deep: true })
  window.addEventListener('keydown', onGlobalKeydown)
  window.addEventListener('resize', () => {
    // 触发重新计算
  })
})

onUnmounted(() => {
  document.removeEventListener('mousemove', onResize)
  document.removeEventListener('mouseup', stopResize)
  window.removeEventListener('keydown', onGlobalKeydown)
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

.app-header {
  height: 48px;
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-default);
  color: var(--text-primary);
  display: flex;
  align-items: center;
  padding: 0 var(--space-4);
  gap: var(--space-4);
}

.app-header h1 {
  font-size: var(--text-lg);
  font-weight: var(--font-semibold);
  margin: 0;
  flex-shrink: 0;
  color: var(--brand-600);
}

/* 顶部工具栏 */
.header-toolbar {
  display: flex;
  gap: var(--space-2);
  flex: 1;
}

.header-btn {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  background: var(--surface-subtle);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  color: var(--text-primary);
  cursor: pointer;
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  transition: var(--transition-all);
}

.header-btn:hover {
  background: var(--surface-hover);
  border-color: var(--border-strong);
}

.header-btn.active {
  background: var(--brand-500);
  border-color: var(--brand-500);
  color: white;
}

.header-btn-icon {
  width: 16px;
  height: 16px;
  flex-shrink: 0;
}

.header-btn-label {
  font-weight: var(--font-medium);
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
  background: var(--bg-secondary);
  border-right: 1px solid var(--border-default);
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

/* 中间主内容 */
.main-content-wrapper {
  flex: 1;
  min-width: 300px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.main-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

/* 右侧信息面板 */
.sidebar-right {
  min-width: 200px;
  max-width: 800px;
  width: 280px;
  background: var(--bg-secondary);
  border-left: 1px solid var(--border-default);
  overflow: hidden;
  display: flex;
  flex-direction: column;
}

.info-panel {
  flex: 1;
  overflow: hidden;
}

.empty-panel {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: var(--space-5);
}

.empty-content {
  text-align: center;
  color: var(--text-secondary);
}

.empty-icon {
  color: var(--text-tertiary);
  margin-bottom: var(--space-4);
}

/* 拖拽分隔条 */
.resizer {
  width: 4px;
  background: var(--border-subtle);
  cursor: col-resize;
  transition: background var(--duration-fast);
  user-select: none;
  -webkit-user-select: none;
}

.resizer:hover {
  background: var(--brand-500);
}

/* 树形区域 */
.tree-section {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
}

.section-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: var(--space-3) var(--space-4);
  border-bottom: 1px solid var(--border-default);
}

.section-header h3 {
  margin: 0;
  font-size: var(--text-xs);
  text-transform: uppercase;
  color: var(--text-secondary);
  letter-spacing: 0.5px;
  font-weight: var(--font-semibold);
}

.section {
  padding: var(--space-3);
  border-bottom: 1px solid var(--border-default);
}

.section h3 {
  font-size: var(--text-xs);
  text-transform: uppercase;
  color: var(--text-secondary);
  margin: 0 0 var(--space-3) 0;
  letter-spacing: 0.5px;
  font-weight: var(--font-semibold);
}

/* 连接组 */
.connection-group {
  margin-bottom: var(--space-1);
}

/* 连接项样式 */
.connection-item {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2);
  border-radius: var(--radius-md);
  cursor: pointer;
  background: var(--surface-default);
  border: 1px solid var(--border-default);
  transition: var(--transition-colors);
}

.connection-item.saved {
  opacity: 0.8;
}

.connection-item:hover {
  background: var(--surface-hover);
}

.connection-item.active {
  background: var(--surface-active);
  border-color: var(--brand-500);
}

.expand-icon {
  width: 16px;
  height: 16px;
  color: var(--text-tertiary);
  flex-shrink: 0;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: transform var(--duration-fast);
}

.expand-icon.expanded {
  transform: rotate(90deg);
}

.connection-info {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.name {
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.host {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.btn-delete,
.btn-close {
  width: 20px;
  height: 20px;
  background: none;
  border: none;
  color: var(--text-tertiary);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  border-radius: var(--radius-sm);
  transition: var(--transition-colors);
}

.btn-delete:hover,
.btn-close:hover {
  color: var(--error-500);
  background: rgba(239, 68, 68, 0.1);
}

.empty {
  color: var(--text-secondary);
  font-size: var(--text-sm);
  padding: var(--space-2) 0;
  text-align: center;
}

/* 连接下的数据库列表 */
.connection-databases {
  margin-left: var(--space-5);
  margin-top: var(--space-1);
}

.database-item {
  margin-bottom: var(--space-1);
}

.database-item.active > .database-header {
  background: var(--surface-active);
}

.database-item.opened > .database-header {
  background: rgba(var(--brand-500-rgb, 59, 130, 246), 0.1);
  border-left: 3px solid var(--brand-500);
}

.database-item.opened > .database-header .database-name {
  color: var(--brand-500);
  font-weight: var(--font-medium);
}

.database-header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2);
  border-radius: var(--radius-md);
  cursor: pointer;
  font-size: var(--text-sm);
  transition: var(--transition-colors);
}

.database-header:hover {
  background: var(--surface-hover);
}

.database-icon {
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.database-name {
  flex: 1;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 对象分类 */
.object-categories {
  /*margin-left: var(--space-5);*/
  margin-top: var(--space-1);
}

.category-item {
  margin-bottom: var(--space-1);
}

.category-header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-1) var(--space-2);
  border-radius: var(--radius-md);
  cursor: pointer;
  font-size: var(--text-xs);
  transition: var(--transition-colors);
}

.category-header:hover {
  background: var(--surface-hover);
}

.category-icon {
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.category-name {
  flex: 1;
  color: var(--text-secondary);
  font-weight: var(--font-medium);
}

/* 对象列表 */
.object-list {
  margin-left: var(--space-5);
  margin-top: var(--space-1);
}

.object-item {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-1) var(--space-2);
  border-radius: var(--radius-md);
  cursor: pointer;
  font-size: var(--text-xs);
  transition: var(--transition-colors);
}

.object-item:hover {
  background: var(--surface-hover);
}

.object-item.active {
  background: var(--surface-active);
}

.object-item.viewing-structure {
  background: #87CEEB;
  border-left: 3px solid var(--warning-500);
}

.object-icon {
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.object-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 主内容区 */
.main-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

/* 视图切换标签 */
.view-tabs {
  display: flex;
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-default);
  padding: var(--space-2) var(--space-4) 0;
  gap: var(--space-1);
}

.view-tab {
  padding: var(--space-2) var(--space-3);
  background: var(--surface-subtle);
  border: 1px solid var(--border-default);
  border-bottom: none;
  border-radius: var(--radius-md) var(--radius-md) 0 0;
  cursor: pointer;
  font-size: var(--text-sm);
  display: flex;
  align-items: center;
  gap: var(--space-2);
  color: var(--text-primary);
  transition: var(--transition-colors);
}

.view-tab:hover {
  background: var(--surface-hover);
}

.view-tab.active {
  background: var(--surface-default);
  border-bottom: 2px solid var(--surface-default);
  margin-bottom: -1px;
  color: var(--brand-600);
  font-weight: var(--font-medium);
}

.view-tab-close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  margin-left: var(--space-1);
  border-radius: var(--radius-sm);
  color: var(--text-secondary);
  transition: var(--transition-colors);
}

.view-tab-close:hover {
  background: var(--surface-hover);
  color: var(--text-primary);
}

.view-content {
  flex: 1;
  display: flex;
  overflow: hidden;
}

.welcome,
.empty-view {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  color: var(--text-secondary);
}

.welcome h2,
.empty-view h2 {
  font-weight: var(--font-normal);
  margin-bottom: var(--space-3);
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
  background: var(--surface-default);
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

.modal-content.modal-license {
  max-width: 560px;
  width: 92%;
}

.modal-content.modal-about {
  max-width: 520px;
  width: 92%;
}

/* 右键菜单 */
.context-menu {
  position: fixed;
  background: var(--surface-default);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
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
  background: var(--surface-hover);
}

.menu-item-danger {
  color: var(--error-500);
}

.menu-item-danger:hover {
  background: var(--error-50);
  color: var(--error-600);
}

.menu-icon {
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.menu-divider {
  height: 1px;
  background: var(--border-subtle);
  margin: var(--space-1) 0;
}

/* ========================================
   RESPONSIVE STYLES
   ======================================== */

/* Mobile menu button */
.menu-toggle {
  display: none;
}

/* Panel toggle button */
.panel-toggle {
  display: none;
}

/* Mobile styles */
@media (max-width: 767px) {
  .menu-toggle {
    display: flex;
  }

  .app-header h1 {
    flex: 1;
  }

  .header-toolbar {
    gap: var(--space-1);
  }

  .header-btn {
    padding: var(--space-2);
  }

  /* Sidebar as drawer */
  .sidebar-left {
    position: fixed;
    left: 0;
    top: 48px;
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
    inset: 48px 0 0 0;
    background: var(--bg-overlay);
    z-index: var(--z-modal-backdrop);
  }
}

/* Tablet styles */
@media (max-width: 1023px) {
  .panel-toggle {
    display: flex;
  }

  /* Info panel as drawer */
  .sidebar-right {
    position: fixed;
    right: 0;
    top: 48px;
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
    inset: 48px 0 0 0;
    background: var(--bg-overlay);
    z-index: var(--z-modal-backdrop);
  }
}

/* Touch device optimizations */
@media (pointer: coarse) {
  .connection-item,
  .database-header,
  .category-header,
  .object-item {
    min-height: 44px;
  }

  .menu-item {
    min-height: 44px;
  }

  .header-btn {
    min-height: 44px;
    min-width: 44px;
  }
}

/* High DPI screens */
@media (-webkit-min-device-pixel-ratio: 2), (min-resolution: 192dpi) {
  .resizer {
    width: 2px;
  }

  .resize-handle {
    width: 2px;
  }
}
</style>
