<template>
  <div class="tree-section" role="tree" :aria-label="t('tree.myConnections')">
    <div class="section-header">
      <h3>{{ t('tree.myConnections') }}</h3>
      <div class="filter-wrap">
        <Search class="filter-icon" :size="12" />
        <input
          v-model="filter"
          type="text"
          class="filter-input"
          :placeholder="t('tree.filterPlaceholder')"
        />
      </div>
    </div>
    <div class="section">
      <div
        v-for="group in groupedItems"
        :key="group.name || '__ungrouped__'"
        class="connection-group"
      >
        <!-- 分组标题：未分组不显示标题，保持原有平铺外观 -->
        <div v-if="group.name" class="group-header">
          <span class="group-name">{{ group.name }}</span>
          <span class="group-count">{{ group.items.length }}</span>
        </div>
        <!-- 离线：已保存但未连接 -->
        <div
          v-for="item in group.items"
          :key="item.key"
        >
        <div
          v-if="item.type === 'saved'"
          class="connection-item saved"
          role="treeitem"
          tabindex="0"
          :aria-selected="false"
          :aria-label="`${item.name} ${item.host}:${item.port}`"
          :style="colorStyleOf(item)"
          @keydown.enter.prevent="emit('openSavedConnection', item.saved!)"
          @keydown.space.prevent="emit('openSavedConnection', item.saved!)"
          @dblclick="emit('openSavedConnection', item.saved!)"
          @contextmenu.prevent="emit('showSavedConnectionMenu', $event, item.saved!)"
          :title="t('tree.openSavedHint')"
        >
          <StatusDot status="offline" :size="8" />
          <div class="connection-info">
            <span class="name">
              <span class="name-text">{{ item.name }}</span>
              <span
                v-if="driverOf(item) === 'postgres'"
                class="mode-badge driver"
                :title="t('tree.postgresHint')"
              >{{ t('tree.postgresBadge') }}</span>
              <span
                v-if="item.saved!.safetyMode === 'read-only'"
                class="mode-badge readonly"
                :title="t('tree.readonlyHint')"
              >{{ t('tree.readonly') }}</span>
              <span
                v-else-if="item.saved!.safetyMode === 'safe'"
                class="mode-badge safe"
                :title="t('tree.safeModeHint')"
              >{{ t('tree.safeMode') }}</span>
            </span>
            <span class="host">{{ item.host }}:{{ item.port }}</span>
            <span v-if="driverOf(item) === 'postgres' && item.saved!.database" class="database-hint">
              {{ t('tree.currentDatabase', { name: item.saved!.database }) }}
            </span>
          </div>
        </div>

        <!-- 活跃连接 -->
        <template v-else>
          <div
            class="connection-item"
            role="treeitem"
            tabindex="0"
            :aria-selected="currentConnection?.id === item.active!.id"
            :aria-expanded="expandedConnections.has(item.active!.id)"
            :aria-label="`${item.name} ${item.host}:${item.port}`"
            :style="colorStyleOf(item)"
            :class="{
              active: currentConnection?.id === item.active!.id,
              expanded: expandedConnections.has(item.active!.id),
              'viewing-info': selectedInfoType === 'connection' && selectedInfoData.connectionId === item.active!.id
            }"
            @click="emit('selectConnection', item.active!)"
            @keydown.enter.prevent="emit('selectConnection', item.active!)"
            @keydown.space.prevent="emit('toggleConnection', item.active!.id)"
            @contextmenu.prevent="emit('showActiveConnectionMenu', $event, item.active!)"
          >
            <span class="expand-icon" @click.stop="emit('toggleConnection', item.active!.id)">
              <component :is="expandedConnections.has(item.active!.id) ? ChevronDown : ChevronRight" :size="12" />
            </span>
            <StatusDot :status="getConnectionStatus(item.active!.state)" :size="8" />
            <div class="connection-info">
              <span class="name">
                <span class="name-text">{{ item.name }}</span>
                <span
                  v-if="driverOf(item) === 'postgres'"
                  class="mode-badge driver"
                  :title="t('tree.postgresHint')"
                >{{ t('tree.postgresBadge') }}</span>
                <span
                  v-if="item.active!.safety_mode === 'read-only'"
                  class="mode-badge readonly"
                  :title="t('tree.readonlyHint')"
                >{{ t('tree.readonly') }}</span>
                <span
                  v-else-if="item.active!.safety_mode === 'safe'"
                  class="mode-badge safe"
                  :title="t('tree.safeModeHint')"
                >{{ t('tree.safeMode') }}</span>
              </span>
              <span class="host">{{ item.host }}:{{ item.port }}</span>
              <span v-if="driverOf(item) === 'postgres' && item.active!.database" class="database-hint">
                {{ t('tree.currentDatabase', { name: item.active!.database }) }}
              </span>
            </div>
          </div>

          <!-- 数据库列表（PostgreSQL 下这一层展示的是 schema） -->
          <DatabaseTree
            v-if="expandedConnections.has(item.active!.id)"
            :connection-id="item.active!.id"
            :databases="connectionDatabases[item.active!.id] || []"
            :expanded-categories="expandedCategories"
            :active-databases="activeDatabases"
            :selected-database="selectedDatabase"
            :selected-table="selectedTable"
            :selected-info-type="selectedInfoType"
            :selected-info-data="selectedInfoData"
            :connection-database-tables="connectionDatabaseTables[item.active!.id] || {}"
            :driver="driverOf(item)"
            :filter="filter"
            @select-database="(db) => emit('selectDatabase', item.active!.id, db)"
            @dblclick-database="(db) => emit('dblclickDatabase', item.active!.id, db)"
            @show-database-menu="(e, db) => emit('showDatabaseMenu', e, item.active!.id, db)"
            @toggle-category="(db, cat) => emit('toggleCategory', item.active!.id, db, cat)"
            @select-table="(db, table) => emit('selectTable', item.active!.id, db, table)"
            @dblclick-table="(db, table) => emit('dblclickTable', item.active!.id, db, table)"
            @show-table-menu="(e, db, table) => emit('showTableMenu', e, item.active!.id, db, table)"
            @select-object="(db, obj) => emit('selectObject', item.active!.id, db, obj)"
            @dblclick-object="(db, obj) => emit('dblclickObject', item.active!.id, db, obj)"
            @new-database="emit('newDatabase', item.active!.id)"
          />
        </template>
        </div>
      </div>

      <div v-if="visibleItems.length === 0" class="empty">
        {{ filter.trim() ? t('tree.noMatch') : t('tree.noConnections') }}
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { ChevronDown, ChevronRight, Search } from 'lucide-vue-next'
import StatusDot from '@/components/icons/StatusDot.vue'
import DatabaseTree from './DatabaseTree.vue'
import { t } from '@/i18n'
import { useSavedConnectionsStore } from '@/stores/savedConnections'
import type { DriverKind } from '@/stores/savedConnections'
import type { ConnectionStatus } from '@/stores/connection'
import type { SavedConnection } from '@/stores/savedConnections'
import type { DatabaseObject } from '@/stores/dataBrowser'

interface TreeItem {
  type: 'saved' | 'active'
  key: string
  name: string
  host: string
  port: number
  saved?: SavedConnection
  active?: ConnectionStatus
}

interface Props {
  treeItems: TreeItem[]
  currentConnection: ConnectionStatus | null
  expandedConnections: Set<string>
  expandedCategories: Set<string>
  activeDatabases: Set<string>
  selectedDatabase: string
  selectedTable: string
  selectedInfoType: string | null
  selectedInfoData: Record<string, string | undefined>
  connectionDatabases: Record<string, DatabaseObject[]>
  connectionDatabaseTables: Record<string, Record<string, DatabaseObject[]>>
}

const props = defineProps<Props>()

// 侧边栏过滤：连接名 / 库名 / 表名任一命中即保留该连接
const filter = ref('')
const keyword = computed(() => filter.value.trim().toLowerCase())

function connectionMatches(item: TreeItem): boolean {
  const k = keyword.value
  if (!k) return true
  if (item.name.toLowerCase().includes(k)) return true
  const connId = item.type === 'active' ? item.active!.id : null
  if (!connId) return false
  const databases = props.connectionDatabases[connId] || []
  if (databases.some(db => db.name.toLowerCase().includes(k))) return true
  const tablesByDb = props.connectionDatabaseTables[connId] || {}
  return Object.entries(tablesByDb).some(
    ([dbName, tables]) =>
      dbName.toLowerCase().includes(k) || tables.some(table => table.name.toLowerCase().includes(k))
  )
}

const visibleItems = computed(() => props.treeItems.filter(connectionMatches))

// 分组 / 颜色来自已保存连接配置；活跃连接与已保存记录共用同一 id
const savedStore = useSavedConnectionsStore()

const savedMetaById = computed(() => {
  const map = new Map<string, SavedConnection>()
  for (const conn of savedStore.savedConnections) {
    map.set(conn.id, conn)
  }
  return map
})

function groupOf(item: TreeItem): string {
  return item.saved?.group || savedMetaById.value.get(item.key)?.group || ''
}

function colorOf(item: TreeItem): string {
  return item.saved?.color || savedMetaById.value.get(item.key)?.color || ''
}

/** 连接颜色标识：渲染为条目左侧色条 */
function colorStyleOf(item: TreeItem): Record<string, string> | undefined {
  const color = colorOf(item)
  // DBX 式语义色：不再用整条 3px 边框，改为 StatusDot 前的细色点（box-shadow 内描）
  return color ? { boxShadow: `inset 3px 0 0 ${color}` } : undefined
}

/** 按分组聚合可见连接：未分组排最前（无标题），其余按名称排序 */
const groupedItems = computed(() => {
  const groups = new Map<string, TreeItem[]>()
  for (const item of visibleItems.value) {
    const group = groupOf(item)
    const list = groups.get(group) ?? []
    list.push(item)
    groups.set(group, list)
  }

  const ungrouped = groups.get('') ?? []
  groups.delete('')
  const named = [...groups.entries()]
    .sort(([a], [b]) => a.localeCompare(b, 'zh-CN'))
    .map(([name, items]) => ({ name, items }))

  return ungrouped.length > 0 ? [{ name: '', items: ungrouped }, ...named] : named
})

const emit = defineEmits<{
  openSavedConnection: [conn: SavedConnection]
  showSavedConnectionMenu: [event: MouseEvent, conn: SavedConnection]
  selectConnection: [conn: ConnectionStatus]
  showActiveConnectionMenu: [event: MouseEvent, conn: ConnectionStatus]
  toggleConnection: [id: string]
  selectDatabase: [connectionId: string, dbName: string]
  dblclickDatabase: [connectionId: string, dbName: string]
  showDatabaseMenu: [event: MouseEvent, connectionId: string, dbName: string]
  toggleCategory: [connectionId: string, dbName: string, categoryType: string]
  selectTable: [connectionId: string, dbName: string, tableName: string]
  dblclickTable: [connectionId: string, dbName: string, tableName: string]
  showTableMenu: [event: MouseEvent, connectionId: string, dbName: string, tableName: string]
  selectObject: [connectionId: string, dbName: string, object: DatabaseObject]
  dblclickObject: [connectionId: string, dbName: string, object: DatabaseObject]
  newDatabase: [connectionId: string]
}>()

/** 驱动信息来自已保存配置或活跃连接状态，旧数据按 mysql 处理 */
function driverOf(item: TreeItem): DriverKind {
  return item.saved?.driver ?? item.active?.driver ?? 'mysql'
}

function getConnectionStatus(state: string): 'online' | 'offline' | 'reconnecting' | 'error' {
  const statusMap: Record<string, 'online' | 'offline' | 'reconnecting' | 'error'> = {
    connected: 'online',
    disconnected: 'offline',
    failed: 'error',
    reconnecting: 'reconnecting'
  }
  return statusMap[state] || 'offline'
}
</script>

<style scoped>
.tree-section {
  flex: 1;
  overflow: auto;
  user-select: none;
  -webkit-user-select: none;
}

.section-header {
  padding: var(--space-2) var(--space-3);
}

.section-header h3 {
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  color: var(--text-tertiary);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.filter-wrap {
  position: relative;
  margin-top: var(--space-2);
}

.filter-icon {
  position: absolute;
  left: var(--space-2);
  top: 50%;
  transform: translateY(-50%);
  color: var(--text-tertiary);
  pointer-events: none;
}

.filter-input {
  width: 100%;
  height: var(--control-height-sm);
  padding: 0 var(--space-2) 0 calc(var(--space-2) * 2 + 10px);
  font-size: var(--text-xs);
  color: var(--text-primary);
  background: var(--raised);
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  outline: none;
  transition: var(--transition-colors);
  user-select: text;
  -webkit-user-select: text;
}

.filter-input:focus {
  background: var(--canvas);
  border-color: var(--brand-500);
}

.filter-input::placeholder {
  color: var(--text-tertiary);
}

.section {
  padding: var(--space-2);
}

.connection-group {
  margin-bottom: var(--space-1);
}

/* 分组标题行 */
.group-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-1) var(--space-2);
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  color: var(--text-secondary);
  text-transform: uppercase;
  letter-spacing: 0.3px;
}

.group-count {
  font-weight: 400;
  color: var(--text-tertiary);
}

.connection-item {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-height: var(--tree-row-height);
  padding: 2px var(--space-2);
  border-radius: var(--radius-sm);
  cursor: pointer;
  background: transparent;
  border: none;
  transition: var(--transition-colors);
}

.connection-item.saved {
  opacity: 0.9;
}

.connection-item:hover {
  background: var(--hover-fill);
}

.connection-item.active {
  background: var(--selected-fill);
  border-color: transparent;
}

.connection-item.viewing-info {
  background: var(--selected-fill);
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
}

.connection-info {
  flex: 1;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  overflow: hidden;
  min-width: 0;
}

.name {
  display: flex;
  align-items: center;
  gap: 4px;
  min-width: 0;
  flex-shrink: 1;
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
}

.name-text {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mode-badge {
  flex: none;
  padding: 0 4px;
  border-radius: 3px;
  font-size: 10px;
  font-weight: 500;
  line-height: 1.5;
}

.mode-badge.readonly {
  color: var(--error-500);
  background: rgba(239, 68, 68, 0.12);
}

.mode-badge.safe {
  color: var(--warning-500);
  background: rgba(245, 158, 11, 0.14);
}

.mode-badge.driver {
  color: var(--text-secondary);
  background: var(--hover-fill);
}

.host {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  flex-shrink: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 当前连接的数据库名：PG 下树第一层是 schema，需在此标注真正连的是哪个库 */
.database-hint {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  flex-shrink: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.empty {
  color: var(--text-secondary);
  font-size: var(--text-sm);
  padding: var(--space-2) 0;
  text-align: center;
}
</style>
