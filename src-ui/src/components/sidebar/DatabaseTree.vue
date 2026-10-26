<template>
  <div class="connection-databases">
    <div
      v-for="db in databases"
      :key="db.name"
      class="database-item"
      :class="{ 
        active: selectedDatabase === `${connectionId}.${db.name}`, 
        opened: activeDatabases.has(`${connectionId}.${db.name}`),
        'viewing-info': selectedInfoType === 'database' && selectedInfoData.connectionId === connectionId && selectedInfoData.database === db.name 
      }"
    >
      <div
        class="database-header"
        @click="emit('selectDatabase', db.name)"
        @dblclick="emit('dblclickDatabase', db.name)"
        @contextmenu.prevent="emit('showDatabaseMenu', $event, db.name)"
      >
        <Database class="database-icon" :size="14" />
        <span class="database-name">{{ db.name }}</span>
      </div>

      <!-- 已打开的数据库显示 Tables/Views/Functions -->
      <div v-if="activeDatabases.has(`${connectionId}.${db.name}`)" class="object-categories">
        <div
          v-for="category in objectCategories"
          :key="category.type"
          class="category-item"
          :class="{ expanded: expandedCategories.has(`${connectionId}.${db.name}.${category.type}`) }"
        >
          <div class="category-header" @click="emit('toggleCategory', db.name, category.type)">
            <span class="expand-icon">
              <component :is="expandedCategories.has(`${connectionId}.${db.name}.${category.type}`) ? ChevronDown : ChevronRight" :size="12" />
            </span>
            <component :is="category.icon" class="category-icon" :size="12" />
            <span class="category-name">{{ category.label }}</span>
          </div>

          <!-- 表列表 -->
          <div v-if="category.type === 'tables' && expandedCategories.has(`${connectionId}.${db.name}.${category.type}`)" class="object-list">
            <div
              v-for="table in (connectionDatabaseTables[db.name] || [])"
              :key="table.name"
              class="object-item"
              :class="{
                active: selectedTable === `${connectionId}.${db.name}.${table.name}`,
                'viewing-structure': selectedInfoType === 'table' && selectedInfoData.tableName === table.name
              }"
              @click="emit('selectTable', db.name, table.name)"
              @dblclick="emit('dblclickTable', db.name, table.name)"
              @contextmenu.prevent="emit('showTableMenu', $event, db.name, table.name)"
              :title="t('tree.tableHint')"
            >
              <Table2 class="object-icon" :size="12" />
              <span class="object-name">{{ table.name }}</span>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- 新建数据库入口 -->
    <div class="database-item new-database" @click="emit('newDatabase')">
      <Plus class="database-icon" :size="14" />
      <span class="database-name">{{ t('app.newDatabase') }}</span>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { Table2, Database, LayoutGrid, Wrench, Cog, ChevronDown, ChevronRight, Plus } from 'lucide-vue-next'
import type { DatabaseObject } from '@/stores/dataBrowser'
import { t } from '@/i18n'

const objectCategories = computed(() => [
  { type: 'tables', label: t('tree.tables'), icon: Table2 },
  { type: 'views', label: t('tree.views'), icon: LayoutGrid },
  { type: 'functions', label: t('tree.functions'), icon: Wrench },
  { type: 'procedures', label: t('tree.procedures'), icon: Cog },
])

interface Props {
  connectionId: string
  databases: DatabaseObject[]
  expandedCategories: Set<string>
  activeDatabases: Set<string>
  selectedDatabase: string
  selectedTable: string
  selectedInfoType: string | null
  selectedInfoData: Record<string, unknown>
  connectionDatabaseTables: Record<string, DatabaseObject[]>
}

defineProps<Props>()

const emit = defineEmits<{
  selectDatabase: [dbName: string]
  dblclickDatabase: [dbName: string]
  showDatabaseMenu: [event: MouseEvent, dbName: string]
  toggleCategory: [dbName: string, categoryType: string]
  selectTable: [dbName: string, tableName: string]
  dblclickTable: [dbName: string, tableName: string]
  showTableMenu: [event: MouseEvent, dbName: string, tableName: string]
  newDatabase: []
}>()
</script>

<style scoped>
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
  background: rgba(59, 130, 246, 0.1);
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

/* 新建数据库入口：弱化为次要操作 */
.new-database {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2);
  border-radius: var(--radius-md);
  cursor: pointer;
  font-size: var(--text-sm);
  color: var(--text-tertiary);
  transition: var(--transition-colors);
}

.new-database:hover {
  background: var(--surface-hover);
  color: var(--brand-500);
}

.database-icon {
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.database-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.object-categories {
  margin-left: var(--space-4);
  margin-top: var(--space-1);
}

.category-item {
  margin-bottom: var(--space-1);
}

.category-header {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  padding: var(--space-1);
  cursor: pointer;
  border-radius: var(--radius-sm);
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
  font-size: var(--text-xs);
  color: var(--text-secondary);
}

.object-list {
  margin-left: var(--space-4);
}

.object-item {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  padding: var(--space-1) var(--space-2);
  cursor: pointer;
  border-radius: var(--radius-sm);
  font-size: var(--text-xs);
  transition: var(--transition-colors);
}

.object-item:hover {
  background: var(--surface-hover);
}

.object-item.active {
  background: var(--brand-500);
  color: white;
}

.object-icon {
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.object-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.expand-icon {
  width: 16px;
  height: 16px;
  color: var(--text-tertiary);
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
}
</style>
