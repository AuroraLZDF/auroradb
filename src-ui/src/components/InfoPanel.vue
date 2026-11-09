<template>
  <div class="info-panel">
    <!-- 头部 -->
    <div class="panel-header">
      <h3>
        <component :is="typeIcon" class="header-icon" :size="18" />
        {{ title }}
      </h3>
      <button class="btn-close" @click="close" :title="t('common.close')" :aria-label="t('common.close')"><X :size="18" /></button>
    </div>

    <!-- 内容区（可横向滚动） -->
    <div class="panel-content" ref="contentRef">
      <!-- 连接信息 -->
      <template v-if="type === 'connection'">
        <div class="info-section">
          <h4>{{ t('infoPanel.connectionInfo') }}</h4>
          <div class="info-grid">
            <div class="info-item">
              <span class="info-label">{{ t('infoPanel.connectionName') }}:</span>
              <span class="info-value">{{ connectionInfo?.name || '-' }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">{{ t('common.type') }}:</span>
              <span class="info-value">{{ connectionInfo?.driver === 'postgres' ? 'PostgreSQL' : 'MySQL' }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">{{ t('infoPanel.host') }}:</span>
              <span class="info-value">{{ connectionInfo?.host || '-' }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">{{ t('infoPanel.port') }}:</span>
              <span class="info-value">{{ connectionInfo?.port || '-' }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">{{ t('infoPanel.username') }}:</span>
              <span class="info-value">{{ connectionInfo?.username || '-' }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">{{ t('common.status') }}:</span>
              <span class="info-value">
                <span class="status-badge" :class="connectionInfo?.state">{{ connectionStateText }}</span>
              </span>
            </div>
            <div class="info-item">
              <span class="info-label">{{ t('infoPanel.databaseCount') }}:</span>
              <span class="info-value">{{ databaseCount }}</span>
            </div>
          </div>
        </div>
      </template>

      <!-- 数据库信息 -->
      <template v-if="type === 'database'">
        <div class="info-section">
          <h4>{{ t('infoPanel.databaseInfo') }}</h4>
          <div class="info-grid">
            <div class="info-item">
              <span class="info-label">{{ t('infoPanel.databaseName') }}:</span>
              <span class="info-value">{{ data.database || '-' }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">{{ t('infoPanel.charset') }}:</span>
              <span class="info-value">{{ databaseInfo?.charset || '-' }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">{{ t('infoPanel.collation') }}:</span>
              <span class="info-value">{{ databaseInfo?.collation || '-' }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">{{ t('infoPanel.tableCount') }}:</span>
              <span class="info-value">{{ tableCount }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">{{ t('infoPanel.dataSize') }}:</span>
              <span class="info-value">{{ formatBytes(databaseInfo?.data_size) }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">{{ t('infoPanel.indexSize') }}:</span>
              <span class="info-value">{{ formatBytes(databaseInfo?.index_size) }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">{{ t('infoPanel.totalSize') }}:</span>
              <span class="info-value">{{ formatBytes(databaseInfo?.total_size) }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">{{ t('infoPanel.createTime') }}:</span>
              <span class="info-value">{{ formatDate(databaseInfo?.create_time) }}</span>
            </div>
          </div>
        </div>

      </template>

      <!-- 表信息 -->
      <template v-if="type === 'table' && tableStructure">
        <div class="info-section">
          <h4>{{ t('infoPanel.basicInfo') }}</h4>
          <div class="info-grid">
            <div class="info-item">
              <span class="info-label">{{ t('infoPanel.tableName') }}:</span>
              <span class="info-value">{{ data.tableName || '-' }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">{{ t('infoPanel.engine') }}:</span>
              <span class="info-value">{{ tableStructure.engine || '-' }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">{{ t('infoPanel.charset') }}:</span>
              <span class="info-value">{{ tableStructure.charset || '-' }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">{{ t('infoPanel.collation') }}:</span>
              <span class="info-value">{{ tableStructure.collation || '-' }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">{{ t('infoPanel.rowCount') }}:</span>
              <span class="info-value">{{ formatNumber(tableStructure.row_count) }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">{{ t('infoPanel.dataSize') }}:</span>
              <span class="info-value">{{ formatBytes(tableStructure.data_size) }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">{{ t('infoPanel.indexSize') }}:</span>
              <span class="info-value">{{ formatBytes(tableStructure.index_size) }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">{{ t('infoPanel.totalSize') }}:</span>
              <span class="info-value">{{ formatBytes((tableStructure.data_size || 0) + (tableStructure.index_size || 0)) }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">{{ t('infoPanel.createTime') }}:</span>
              <span class="info-value">{{ formatDate(tableStructure.create_time) }}</span>
            </div>
            <div class="info-item">
              <span class="info-label">{{ t('infoPanel.updateTime') }}:</span>
              <span class="info-value">{{ formatDate(tableStructure.update_time) }}</span>
            </div>
          </div>
        </div>

        <!-- DDL -->
        <div class="info-section">
          <div class="section-header-with-action">
            <h4>{{ t('infoPanel.createStatement') }}</h4>
            <button 
              class="btn-copy" 
              :class="{ success: copySuccess }"
              @click="copySQL" 
              :title="copySuccess ? t('common.copied') : t('infoPanel.copyCreateStatement')"
            >
              <Copy v-if="!copySuccess" :size="14" />
              <CheckCircle v-else :size="14" />
              {{ copySuccess ? t('common.copied') : t('common.copy') }}
            </button>
          </div>
          <div 
            class="ddl-container" 
            ref="sqlContainerRef"
            tabindex="0"
            @keydown.ctrl.a.prevent="selectSQL"
          >
            <pre class="ddl-code" v-html="formatSQL(tableStructure.createStatement)"></pre>
          </div>
        </div>
      </template>

      <!-- PostgreSQL 元数据（表结构 / 模式详情）暂未提供 -->
      <div v-if="isPostgres && type !== 'connection'" class="info-section">
        <h4>{{ type === 'table' ? t('infoPanel.tableInfo') : t('infoPanel.databaseInfo') }}</h4>
        <p class="info-hint">{{ t('infoPanel.postgresNotSupported') }}</p>
      </div>

      <!-- 加载状态 -->
      <div v-if="loading" class="loading-overlay">
        <div class="loading-spinner"></div>
        <p>{{ t('common.loading') }}</p>
      </div>

      <!-- 错误状态 -->
      <div v-if="error" class="error-overlay">
        <XCircle class="error-icon" :size="24" />
        <p>{{ error }}</p>
        <button class="btn-retry" @click="loadData">{{ t('common.retry') }}</button>
      </div>
    </div>

    <!-- 底部横向滚动拖拽条 -->
    <div
      v-if="showScrollbar"
      class="scrollbar-container"
      @mousedown="startHorizontalScroll"
    >
      <div
        class="scrollbar-thumb"
        :style="{
          width: thumbWidth + 'px',
          left: thumbLeft + 'px'
        }"
      ></div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useConnectionStore } from '@/stores/connection'
import { useDataBrowserStore, type TableStructure, type DatabaseInfo } from '@/stores/dataBrowser'
import { t } from '@/i18n'
import { connectionStateI18nKey } from '@/utils/connectionState'
import { Table, Copy, Plug, Database, Info, X, CheckCircle, XCircle } from 'lucide-vue-next'

const props = defineProps<{
  type: 'connection' | 'database' | 'table'
  data: {
    connectionId?: string
    connectionName?: string
    database?: string
    tableName?: string
  }
}>()

const emit = defineEmits<{
  close: []
}>()

const connectionStore = useConnectionStore()
const browserStore = useDataBrowserStore()

// 状态
const loading = ref(false)
const error = ref('')
const tableStructure = ref<TableStructure | null>(null)
const databaseInfo = ref<DatabaseInfo | null>(null)

// 横向滚动相关
const contentRef = ref<HTMLElement | null>(null)
const sqlContainerRef = ref<HTMLElement | null>(null)
const showScrollbar = ref(false)
const thumbWidth = ref(0)
const thumbLeft = ref(0)
const isDragging = ref(false)
const dragStartX = ref(0)
const dragStartLeft = ref(0)

// 复制状态
const copySuccess = ref(false)
let copyTimeout: ReturnType<typeof setTimeout> | null = null
let scrollbarTimer: ReturnType<typeof setTimeout> | null = null

/** 延迟刷新滚动条，等待内容渲染完成；重复调用时重置上一个定时器 */
function scheduleScrollbarUpdate() {
  if (scrollbarTimer) clearTimeout(scrollbarTimer)
  scrollbarTimer = setTimeout(updateScrollbar, 100)
}

// Tauri invoke（模块化导入，不依赖 withGlobalTauri）
function tauriInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!('__TAURI_INTERNALS__' in window)) {
    return Promise.reject(new Error('Not running in Tauri environment'))
  }
  return invoke<T>(cmd, args)
}

// 图标组件
const typeIcon = computed(() => {
  switch (props.type) {
    case 'connection': return Plug
    case 'database': return Database
    case 'table': return Table
    default: return Info
  }
})

// 标题
const title = computed(() => {
  switch (props.type) {
    case 'connection': return props.data.connectionName || t('infoPanel.connectionInfo')
    case 'database': return props.data.database || t('infoPanel.databaseInfo')
    case 'table': return props.data.tableName || t('infoPanel.tableInfo')
    default: return t('infoPanel.info')
  }
})

// 连接信息
const connectionInfo = computed(() => {
  if (props.type === 'connection' && props.data.connectionId) {
    return connectionStore.activeConnections.find(c => c.id === props.data.connectionId)
  }
  return null
})

const connectionStateText = computed(() =>
  t(connectionStateI18nKey(connectionInfo.value?.state)),
)

// 数据库数量
const databaseCount = computed(() => {
  if (props.type === 'connection' && props.data.connectionId) {
    const dbs = (browserStore.objectTree as unknown as Record<string, unknown[]>)[props.data.connectionId]
    return dbs?.length || 0
  }
  return 0
})

// 表数量
const tableCount = computed(() => {
  if (props.type === 'database' && props.data.database) {
    const dbTables = browserStore.databaseTables[props.data.database]
    return dbTables?.filter(item => item.object_type === 'Table').length || 0
  }
  return 0
})

// 当前连接是否为 PostgreSQL（PG 的表结构 / 模式详情尚未提供）
const isPostgres = computed(() =>
  connectionStore.connections.some(c => c.id === props.data.connectionId && c.driver === 'postgres')
)

// 加载数据
async function loadData() {
  if (!props.data.connectionId) return

  loading.value = true
  error.value = ''

  try {
    // PostgreSQL 统一走页面上的说明，不调用 MySQL 专用的元数据命令
    if (isPostgres.value && props.type !== 'connection') return
    if (props.type === 'table' && props.data.database && props.data.tableName) {
      const result = await tauriInvoke<TableStructure>('get_table_structure', {
        connectionId: props.data.connectionId,
        database: props.data.database,
        table: props.data.tableName
      })
      tableStructure.value = result
    } else if (props.type === 'database' && props.data.database) {
      // 获取数据库信息
      const result = await tauriInvoke<DatabaseInfo>('get_database_info', {
        connectionId: props.data.connectionId,
        database: props.data.database
      })
      databaseInfo.value = result
    }
  } catch (e) {
    error.value = String(e)
    console.error('Failed to load info:', e)
  } finally {
    loading.value = false
  }
}

// 关闭面板
function close() {
  emit('close')
}

// 复制 SQL
function copySQL() {
  if (tableStructure.value?.createStatement) {
    navigator.clipboard.writeText(tableStructure.value.createStatement)
    
    // 显示成功状态
    copySuccess.value = true
    
    // 清除之前的定时器
    if (copyTimeout) {
      clearTimeout(copyTimeout)
    }
    
    // 5秒后恢复原始状态
    copyTimeout = setTimeout(() => {
      copySuccess.value = false
    }, 5000)
  }
}

// 选中 SQL 代码
function selectSQL() {
  const container = sqlContainerRef.value
  if (!container) return
  
  // 创建选区
  const range = document.createRange()
  range.selectNodeContents(container)
  
  const selection = window.getSelection()
  if (selection) {
    selection.removeAllRanges()
    selection.addRange(range)
  }
}

// 格式化
function formatNumber(num?: number): string {
  if (num === undefined || num === null) return '-'
  return num.toLocaleString()
}

function formatBytes(bytes?: number): string {
  if (bytes === undefined || bytes === null) return '-'
  if (bytes === 0) return '0 B'
  const k = 1024
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.floor(Math.log(bytes) / Math.log(k))
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i]
}
// 格式化日期
function formatDate(date?: string): string {
  if (!date) return '-'
  const d = new Date(date)
  if (isNaN(d.getTime())) return date
  return d.toLocaleString('zh-CN')
}

// SQL 语法高亮 - 保持原有格式，仅添加颜色
function formatSQL(sql?: string): string {
  if (!sql) return `<span class="sql-comment">-- ${t('infoPanel.ddlEmpty')}</span>`
  
  // 转义 HTML
  let formatted = sql
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
  
  // 高亮字符串（单引号）- 先处理避免内部被替换
  formatted = formatted.replace(/'([^']*)'/g, '<span class="sql-string">\'$1\'</span>')
  
  // 高亮字段名（反引号）
  formatted = formatted.replace(/`([^`]*)`/g, '<span class="sql-field">`$1`</span>')
  
  // 高亮关键字
  const keywords = [
    'CREATE', 'TABLE', 'PRIMARY', 'KEY', 'UNIQUE', 'INDEX', 'FOREIGN',
    'NOT', 'NULL', 'AUTO_INCREMENT', 'DEFAULT', 'COMMENT', 'ENGINE',
    'CHARSET', 'COLLATE', 'CURRENT_TIMESTAMP', 'ON', 'UPDATE', 'DELETE',
    'CASCADE', 'RESTRICT', 'SET', 'CONSTRAINT', 'REFERENCES',
    'BIGINT', 'INT', 'VARCHAR', 'TEXT', 'DATETIME', 'TIMESTAMP', 'TINYINT',
    'CHAR', 'DECIMAL', 'FLOAT', 'DOUBLE', 'BOOLEAN', 'JSON', 'ENUM',
    'UNSIGNED', 'SIGNED', 'CHECK', 'longtext'
  ]
  
  keywords.forEach(keyword => {
    const regex = new RegExp(`\\b${keyword}\\b`, 'gi')
    formatted = formatted.replace(regex, `<span class="sql-keyword">$&</span>`)
  })
  
  // 高亮函数
  const functions = ['CURRENT_TIMESTAMP', 'NOW', 'UUID', 'json_valid']
  functions.forEach(func => {
    const regex = new RegExp(`\\b${func}\\b`, 'gi')
    formatted = formatted.replace(regex, `<span class="sql-function">$&</span>`)
  })
  
  // 高亮数字
  formatted = formatted.replace(/\b(\d+)\b/g, '<span class="sql-number">$1</span>')
  
  return formatted
}

// 横向滚动处理
function updateScrollbar() {
  if (!contentRef.value) return
  const content = contentRef.value
  const scrollWidth = content.scrollWidth
  const clientWidth = content.clientWidth

  // 只有在内容超出时才显示滚动条
  showScrollbar.value = scrollWidth > clientWidth

  if (showScrollbar.value) {
    // 计算滚动条宽度比例
    const ratio = clientWidth / scrollWidth
    thumbWidth.value = Math.max(50, clientWidth * ratio * 0.8)
    updateThumbPosition()
  }
}

function updateThumbPosition() {
  if (!contentRef.value || !showScrollbar.value) return
  const content = contentRef.value
  const scrollLeft = content.scrollLeft
  const scrollWidth = content.scrollWidth - content.clientWidth
  const thumbScrollWidth = content.clientWidth - thumbWidth.value
  thumbLeft.value = (scrollLeft / scrollWidth) * thumbScrollWidth
}

function startHorizontalScroll(e: MouseEvent) {
  if (!contentRef.value || !showScrollbar.value) return

  e.preventDefault()

  const target = e.target as HTMLElement
  if (target.classList.contains('scrollbar-thumb')) {
    isDragging.value = true
    dragStartX.value = e.clientX
    dragStartLeft.value = thumbLeft.value
    document.addEventListener('mousemove', onDrag)
    document.addEventListener('mouseup', stopDrag)
  } else {
    // 点击滚动条背景，跳到对应位置
    const container = e.currentTarget as HTMLElement
    const rect = container.getBoundingClientRect()
    const clickX = e.clientX - rect.left
    const ratio = clickX / rect.width
    const scrollWidth = contentRef.value.scrollWidth - contentRef.value.clientWidth
    contentRef.value.scrollLeft = ratio * scrollWidth
  }
}

function onDrag(e: MouseEvent) {
  if (!isDragging.value || !contentRef.value) return

  e.preventDefault()

  const deltaX = e.clientX - dragStartX.value
  const containerWidth = contentRef.value.clientWidth
  const thumbScrollWidth = containerWidth - thumbWidth.value
  const newLeft = Math.max(0, Math.min(thumbScrollWidth, dragStartLeft.value + deltaX))
  const ratio = newLeft / thumbScrollWidth
  const scrollWidth = contentRef.value.scrollWidth - contentRef.value.clientWidth
  contentRef.value.scrollLeft = ratio * scrollWidth
}

function stopDrag() {
  isDragging.value = false
  document.removeEventListener('mousemove', onDrag)
  document.removeEventListener('mouseup', stopDrag)
}

// 监听内容滚动
function onContentScroll() {
  updateThumbPosition()
}

onMounted(() => {
  loadData()
  if (contentRef.value) {
    contentRef.value.addEventListener('scroll', onContentScroll)
    // 延迟更新滚动条，等待内容渲染
    scheduleScrollbarUpdate()
  }
  window.addEventListener('resize', updateScrollbar)
})

onUnmounted(() => {
  if (contentRef.value) {
    contentRef.value.removeEventListener('scroll', onContentScroll)
  }
  window.removeEventListener('resize', updateScrollbar)
  document.removeEventListener('mousemove', onDrag)
  
  // 清理复制状态定时器
  if (copyTimeout) {
    clearTimeout(copyTimeout)
  }
  if (scrollbarTimer) {
    clearTimeout(scrollbarTimer)
  }
  document.removeEventListener('mouseup', stopDrag)
})

// 监听数据变化，重新加载
watch(() => [props.type, props.data], () => {
  loadData()
  scheduleScrollbarUpdate()
}, { deep: true })
</script>

<style scoped>
.info-panel {
  flex: 1;
  height: 100%;
  min-width: 0;
  display: flex;
  flex-direction: column;
  background: var(--panel);
  overflow: hidden;
}

/* 头部 */
.panel-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  height: var(--toolbar-height);
  padding: 0 var(--space-3);
  background: var(--panel);
  border-bottom: 1px solid var(--hairline);
  flex-shrink: 0;
}

.panel-header h3 {
  margin: 0;
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  display: flex;
  align-items: center;
  gap: var(--space-2);
  color: var(--text-primary);
  min-width: 0;
}

.header-icon {
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.btn-close {
  width: var(--control-height-sm);
  height: var(--control-height-sm);
  background: transparent;
  border: none;
  border-radius: var(--radius-sm);
  cursor: pointer;
  font-size: 14px;
  color: var(--text-secondary);
  display: flex;
  align-items: center;
  justify-content: center;
}

.btn-close:hover {
  background: var(--hover-fill);
  color: var(--text-primary);
}

/* 内容区 */
.panel-content {
  flex: 1;
  overflow: auto;
  padding: 0 var(--space-3);
  position: relative;
  /* 允许选中/复制信息值（如库名、表名、字符集等） */
  user-select: text;
  -webkit-user-select: text;
}

/* 信息区块 */
.info-section {
  padding: var(--space-3) 0;
  background: transparent;
  border: none;
  border-bottom: 1px solid var(--hairline);
  border-radius: 0;
}

.info-section:last-child {
  border-bottom: none;
}

.info-section h4 {
  margin: 0;
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  color: var(--text-secondary);
  letter-spacing: 0.2px;
}

.info-hint {
  margin: var(--space-2) 0 0;
  font-size: var(--text-xs);
  line-height: 1.6;
  color: var(--text-secondary);
}

.section-header-with-action {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: var(--space-2);
  border-bottom: 1px solid var(--hairline);
  padding-bottom: var(--space-2);
}

.btn-copy {
  height: var(--control-height-sm);
  padding: 0 var(--space-2);
  background: transparent;
  color: var(--text-secondary);
  border: none;
  border-radius: var(--radius-sm);
  cursor: pointer;
  font-size: var(--text-xs);
  display: flex;
  align-items: center;
  gap: var(--space-1);
  transition: var(--transition-colors);
}

.btn-copy:hover {
  background: var(--hover-fill);
  color: var(--text-primary);
}

.btn-copy.success {
  color: var(--success-500);
}

.btn-copy.success:hover {
  background: rgba(16, 185, 129, 0.12);
}

/* DDL容器 - 可聚焦 */
.ddl-container {
  outline: none;
}

.ddl-container:focus {
  box-shadow: 0 0 0 2px var(--brand-500);
}

/* 基本信息网格 */
.info-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
  gap: var(--space-2) var(--space-3);
}

.info-item {
  display: flex;
  gap: var(--space-2);
  font-size: var(--text-xs);
  line-height: 18px;
}

.info-label {
  color: var(--text-tertiary);
  flex-shrink: 0;
  min-width: 64px;
}

.info-value {
  color: var(--text-primary);
  word-break: break-all;
}

/* 状态徽章 */
.status-badge {
  display: inline-block;
  padding: 0 6px;
  border-radius: var(--radius-xs);
  font-size: 10px;
  font-weight: 500;
  line-height: 16px;
}

.status-badge.connected {
  background: rgba(16, 185, 129, 0.14);
  color: var(--success-500);
}

.status-badge.disconnected {
  background: rgba(239, 68, 68, 0.12);
  color: var(--error-500);
}

.status-badge.reconnecting {
  background: rgba(245, 158, 11, 0.14);
  color: var(--warning-500);
}

.status-badge.failed {
  background: var(--hover-fill);
  color: var(--text-secondary);
}

/* DDL代码 - 格式化显示 */
:deep(.ddl-container) {
  background: var(--canvas);
  border-radius: var(--radius-sm);
  overflow: auto;
  max-height: 400px;
  border: 1px solid var(--hairline);
}

:deep(.ddl-code) {
  color: var(--text-primary);
  padding: var(--space-3) var(--space-4);
  font-size: var(--text-xs);
  line-height: 1.6;
  white-space: pre;
  word-break: normal;
  margin: 0;
  font-family: 'JetBrains Mono', 'Fira Code', 'Consolas', 'Monaco', monospace;
  user-select: text;
  -webkit-user-select: text;
}

/* SQL 语法高亮（主题感知 token） */
:deep(.sql-keyword) {
  color: var(--code-keyword) !important;
  font-weight: 600;
}

:deep(.sql-function) {
  color: var(--code-function) !important;
}

:deep(.sql-string) {
  color: var(--code-string) !important;
}

:deep(.sql-field) {
  /* color: #9cdcfe !important; */
}

:deep(.sql-number) {
  color: var(--code-number) !important;
}

:deep(.sql-comment) {
  color: var(--code-comment) !important;
  font-style: italic;
}

/* 加载和错误状态 */
.loading-overlay,
.error-overlay {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  /* 使用更柔和的遮罩层背景，保留内容可见性 */
  background: rgba(var(--bg-primary-rgb, 30, 30, 30), 0.4);
  backdrop-filter: blur(2px);
  /* 淡入动画 */
  animation: fade-in 0.3s ease-out;
}

/* 加载遮罩层特定样式 - 更轻量 */
.loading-overlay {
  background: rgba(var(--bg-primary-rgb, 30, 30, 30), 0.35);
}

/* 错误遮罩层 - 稍深以突出错误信息 */
.error-overlay {
  background: rgba(var(--bg-primary-rgb, 30, 30, 30), 0.5);
}

/* 淡入动画 */
@keyframes fade-in {
  from {
    opacity: 0;
    backdrop-filter: blur(0px);
  }
  to {
    opacity: 1;
    backdrop-filter: blur(2px);
  }
}

/* 淡出动画类 */
.fade-out {
  animation: fade-out 0.3s ease-out forwards;
}

@keyframes fade-out {
  from {
    opacity: 1;
    backdrop-filter: blur(2px);
  }
  to {
    opacity: 0;
    backdrop-filter: blur(0px);
  }
}

/* 旋转加载动画 */
.loading-spinner {
  width: 40px;
  height: 40px;
  border: 3px solid rgba(255, 255, 255, 0.1);
  border-top-color: var(--brand-500);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
  margin-bottom: 16px;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

.error-icon {
  font-size: 36px;
  margin-bottom: 12px;
}

.error-overlay p {
  color: var(--error-500);
  font-size: 12px;
  margin-bottom: 12px;
}

.btn-retry {
  padding: 6px 16px;
  background: var(--brand-500);
  color: white;
  border: none;
  border-radius: 4px;
  cursor: pointer;
  font-size: 12px;
}

.btn-retry:hover {
  background: var(--brand-500);
}

/* 底部横向滚动条 */
.scrollbar-container {
  height: 12px;
  background: var(--hairline);
  border-top: 1px solid var(--hairline);
  position: relative;
  cursor: pointer;
  user-select: none;
}

.scrollbar-thumb {
  height: 100%;
  background: var(--text-secondary);
  border-radius: 6px;
  position: absolute;
  top: 0;
  cursor: grab;
  transition: background 0.2s;
}

.scrollbar-thumb:hover {
  background: var(--text-primary);
}

.scrollbar-thumb:active {
  cursor: grabbing;
  background: var(--brand-500);
}

/* 选中状态 */
.viewing-info {
  background: rgba(243, 156, 18, 0.2) !important;
  border-left: 3px solid var(--warning-500) !important;
}
</style>
