<template>
  <div class="tree-section">
    <div class="section-header">
      <h3>{{ t('tree.myConnections') }}</h3>
    </div>
    <div class="section">
      <div
        v-for="item in treeItems"
        :key="item.key"
        class="connection-group"
      >
        <!-- 离线：已保存但未连接 -->
        <div
          v-if="item.type === 'saved'"
          class="connection-item saved"
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
            </span>
            <span class="host">{{ item.host }}:{{ item.port }}</span>
          </div>
        </div>

        <!-- 活跃连接 -->
        <template v-else>
          <div
            class="connection-item"
            :class="{ 
              active: currentConnection?.id === item.active!.id, 
              expanded: expandedConnections.has(item.active!.id),
              'viewing-info': selectedInfoType === 'connection' && selectedInfoData.connectionId === item.active!.id 
            }"
            @click="emit('selectConnection', item.active!)"
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
            </div>
          </div>

          <!-- PostgreSQL 暂不支持对象树，给出使用提示 -->
          <p
            v-if="expandedConnections.has(item.active!.id) && driverOf(item) === 'postgres'"
            class="driver-hint"
          >{{ t('tree.postgresTreeHint') }}</p>

          <!-- 数据库列表 -->
          <DatabaseTree
            v-else-if="expandedConnections.has(item.active!.id)"
            :connection-id="item.active!.id"
            :databases="connectionDatabases[item.active!.id] || []"
            :expanded-categories="expandedCategories"
            :active-databases="activeDatabases"
            :selected-database="selectedDatabase"
            :selected-table="selectedTable"
            :selected-info-type="selectedInfoType"
            :selected-info-data="selectedInfoData"
            :connection-database-tables="connectionDatabaseTables[item.active!.id] || {}"
            @select-database="(db) => emit('selectDatabase', item.active!.id, db)"
            @dblclick-database="(db) => emit('dblclickDatabase', item.active!.id, db)"
            @show-database-menu="(e, db) => emit('showDatabaseMenu', e, item.active!.id, db)"
            @toggle-category="(db, cat) => emit('toggleCategory', item.active!.id, db, cat)"
            @select-table="(db, table) => emit('selectTable', item.active!.id, db, table)"
            @dblclick-table="(db, table) => emit('dblclickTable', item.active!.id, db, table)"
            @show-table-menu="(e, db, table) => emit('showTableMenu', e, item.active!.id, db, table)"
            @new-database="emit('newDatabase', item.active!.id)"
          />
        </template>
      </div>

      <div v-if="treeItems.length === 0" class="empty">
        {{ t('tree.noConnections') }}
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ChevronDown, ChevronRight } from 'lucide-vue-next'
import StatusDot from '@/components/icons/StatusDot.vue'
import DatabaseTree from './DatabaseTree.vue'
import { t } from '@/i18n'
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

defineProps<Props>()

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
  newDatabase: [connectionId: string]
}>()

/** 驱动信息来自已保存配置或活跃连接状态，旧数据按 mysql 处理 */
function driverOf(item: TreeItem): 'mysql' | 'postgres' {
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
}

.section-header {
  padding: var(--space-3);
  border-bottom: 1px solid var(--border-default);
}

.section-header h3 {
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-secondary);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.section {
  padding: var(--space-2);
}

.connection-group {
  margin-bottom: var(--space-1);
}

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
}

.connection-info {
  flex: 1;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.name {
  display: flex;
  align-items: center;
  gap: 4px;
  min-width: 0;
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
  padding: 1px 4px;
  border-radius: 4px;
  font-size: 10px;
  font-weight: 500;
  line-height: 1.4;
}

.mode-badge.readonly {
  color: var(--color-danger);
  background: rgba(231, 76, 60, 0.12);
}

.mode-badge.safe {
  color: var(--color-warning);
  background: rgba(243, 156, 18, 0.14);
}

.mode-badge.driver {
  color: var(--brand-500);
  background: rgba(59, 130, 246, 0.12);
}

.driver-hint {
  margin: var(--space-1) 0 0 var(--space-4);
  font-size: var(--text-xs);
  line-height: 1.5;
  color: var(--text-tertiary);
}

.host {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
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
