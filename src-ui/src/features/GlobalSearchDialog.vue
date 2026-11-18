<template>
  <div class="global-search">
    <!-- 头部 -->
    <div class="dialog-header">
      <div class="header-title">
        <Search :size="18" class="title-icon" />
        <div>
          <h2>{{ t('search.title') }}</h2>
          <p class="subtitle">{{ t('search.subtitle') }}</p>
        </div>
      </div>
      <button class="icon-btn" :title="t('common.close')" @click="emit('close')" :aria-label="t('common.close')">
        <X :size="18" />
      </button>
    </div>

    <div class="dialog-body">
      <!-- 搜索表单 -->
      <div class="form-grid">
        <div class="form-row">
          <label>{{ t('search.connection') }}</label>
          <Select
            v-model="connectionId"
            :options="connectionOptions"
            :placeholder="t('search.connection')"
          />
        </div>
        <div class="form-row">
          <label>{{ t('search.database') }}</label>
          <Select
            v-model="database"
            :options="databaseOptions"
            :disabled="!connectionId || dbLoading || !supportsAllDatabaseSearch"
            :placeholder="dbLoading ? t('common.loading') : t('search.allDatabases')"
            searchable
          />
          <p v-if="connectionId && !supportsAllDatabaseSearch" class="hint scope-hint">
            <AlertTriangle :size="14" />
            {{ t('search.pgScopeHint') }}
          </p>
        </div>
        <div class="form-row full-width">
          <label>{{ t('search.keyword') }}</label>
          <input
            v-model="keyword"
            type="text"
            class="text-input"
            maxlength="200"
            :placeholder="t('search.keywordPlaceholder')"
            @keydown.enter="runSearch"
          />
        </div>
      </div>

      <div class="options-row">
        <div class="option-group">
          <span class="option-label">{{ t('search.mode') }}</span>
          <div class="segmented">
            <button
              :class="['segment-btn', { active: mode === 'like' }]"
              @click="mode = 'like'"
            >
              {{ t('search.modeLike') }}
            </button>
            <button
              :class="['segment-btn', { active: mode === 'regex' }]"
              @click="mode = 'regex'"
            >
              {{ t('search.modeRegex') }}
            </button>
          </div>
        </div>
        <div class="option-group">
          <span class="option-label">{{ t('search.scope') }}</span>
          <label class="checkbox">
            <input v-model="searchTables" type="checkbox" />
            {{ t('search.scopeTables') }}
          </label>
          <label class="checkbox">
            <input v-model="searchColumns" type="checkbox" />
            {{ t('search.scopeColumns') }}
          </label>
          <label class="checkbox">
            <input v-model="searchData" type="checkbox" />
            {{ t('search.scopeData') }}
          </label>
        </div>
      </div>

      <p v-if="searchData" class="hint">
        <AlertTriangle :size="14" />
        {{ t('search.dataHint') }}
      </p>

      <div class="actions">
        <Button variant="primary" :loading="searching" :disabled="!canSearch" @click="runSearch">
          <Search :size="14" />
          {{ searching ? t('search.searching') : t('search.start') }}
        </Button>
        <span v-if="result" class="meta">
          {{ t('search.scanned', { count: result.scanned_tables }) }} ·
          {{ t('search.elapsed', { ms: result.execution_time_ms }) }}
        </span>
      </div>

      <p v-if="error" class="error-msg">
        <AlertCircle :size="14" /> {{ error }}
      </p>

      <p v-if="result?.warning" class="warning-msg">
        <AlertTriangle :size="14" /> {{ result.warning }}
      </p>

      <!-- 结果 -->
      <div v-if="result" class="results">
        <p class="summary">
          {{ t('search.summary', {
            tables: result.tables.length,
            columns: result.columns.length,
            data: result.data.length,
          }) }}
          <span v-if="result.truncated" class="truncated">{{ t('search.truncated') }}</span>
        </p>

        <div v-if="isEmpty" class="empty-state">
          <Search :size="36" class="empty-icon" />
          {{ t('search.empty') }}
        </div>

        <template v-else>
          <section v-if="result.tables.length" class="result-section">
            <h3>
              <Table2 :size="14" />
              {{ t('search.sectionTables') }}
              <span class="count">{{ result.tables.length }}</span>
            </h3>
            <ul class="result-list">
              <li v-for="item in result.tables" :key="`t-${item.database}.${item.table}`">
                <button class="result-item" @click="openTable(item.database, item.table)">
                  <span class="item-db">{{ item.database }}</span>
                  <span class="item-sep">.</span>
                  <span class="item-name">{{ item.table }}</span>
                  <span v-if="item.object_type === 'view'" class="badge">{{ t('search.view') }}</span>
                  <ChevronRight :size="14" class="item-arrow" />
                </button>
              </li>
            </ul>
          </section>

          <section v-if="result.columns.length" class="result-section">
            <h3>
              <Columns3 :size="14" />
              {{ t('search.sectionColumns') }}
              <span class="count">{{ result.columns.length }}</span>
            </h3>
            <ul class="result-list">
              <li v-for="item in result.columns" :key="`c-${item.database}.${item.table}.${item.column}`">
                <button class="result-item" @click="openTable(item.database, item.table)">
                  <span class="item-db">{{ item.database }}</span>
                  <span class="item-sep">.</span>
                  <span class="item-name">{{ item.table }}</span>
                  <span class="item-column">.{{ item.column }}</span>
                  <span class="badge">{{ item.data_type }}</span>
                  <ChevronRight :size="14" class="item-arrow" />
                </button>
              </li>
            </ul>
          </section>

          <section v-if="result.data.length" class="result-section">
            <h3>
              <Rows3 :size="14" />
              {{ t('search.sectionData') }}
              <span class="count">{{ result.data.length }}</span>
            </h3>
            <ul class="result-list">
              <li v-for="item in result.data" :key="`d-${item.database}.${item.table}`">
                <button class="result-item" @click="openTable(item.database, item.table)">
                  <span class="item-db">{{ item.database }}</span>
                  <span class="item-sep">.</span>
                  <span class="item-name">{{ item.table }}</span>
                  <span class="rows">
                    {{
                      item.count_capped
                        ? t('search.rowsCapped', { count: item.matched_rows })
                        : t('search.rows', { count: item.matched_rows })
                    }}
                  </span>
                  <ChevronRight :size="14" class="item-arrow" />
                </button>
              </li>
            </ul>
          </section>
        </template>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { t } from '@/i18n'
import { useConnectionStore } from '@/stores/connection'
import { capabilitiesOf } from '@/utils/driver'
import Select from '@/components/ui/Select.vue'
import Button from '@/components/ui/Button.vue'
import {
  Search,
  X,
  AlertCircle,
  AlertTriangle,
  Table2,
  Columns3,
  Rows3,
  ChevronRight,
} from 'lucide-vue-next'

const props = withDefaults(defineProps<{
  initialConnectionId?: string
  initialDatabase?: string
}>(), {
  initialConnectionId: '',
  initialDatabase: '',
})

const emit = defineEmits<{
  close: []
  'open-table': [connectionId: string, database: string, table: string]
}>()

const connStore = useConnectionStore()

interface TableMatch { database: string; table: string; object_type: string }
interface ColumnMatch { database: string; table: string; column: string; data_type: string }
interface DataMatch { database: string; table: string; matched_rows: number; count_capped: boolean }

interface SearchResult {
  tables: TableMatch[]
  columns: ColumnMatch[]
  data: DataMatch[]
  scanned_tables: number
  truncated: boolean
  warning?: string
  execution_time_ms: number
}

interface DbObject {
  name: string
  object_type: string
  children?: DbObject[]
}

const connectionId = ref('')
const database = ref('')
const keyword = ref('')
const mode = ref<'like' | 'regex'>('like')
const searchTables = ref(true)
const searchColumns = ref(true)
const searchData = ref(false)
const searching = ref(false)
const error = ref('')
const result = ref<SearchResult | null>(null)

const connectionOptions = computed(() =>
  connStore.activeConnections.map(c => ({
    value: c.id,
    label: `${c.name} (${c.host}:${c.port})`,
  }))
)

const databases = ref<string[]>([])
const dbLoading = ref(false)

const connectionDriver = computed(
  () => connStore.activeConnections.find(connection => connection.id === connectionId.value)?.driver
)
/** PG 连接的搜索范围固定为当前数据库，禁止跨库搜索 */
const supportsAllDatabaseSearch = computed(
  () => capabilitiesOf(connectionDriver.value).supportsAllDatabaseSearch
)

const databaseOptions = computed(() => [
  {
    value: '',
    label: supportsAllDatabaseSearch.value ? t('search.allDatabases') : t('search.currentDatabaseOnly'),
  },
  ...(databases.value.map(name => ({ value: name, label: name }))),
])

const canSearch = computed(() =>
  !!connectionId.value &&
  keyword.value.trim().length > 0 &&
  (searchTables.value || searchColumns.value || searchData.value) &&
  !searching.value
)

const isEmpty = computed(() => {
  const r = result.value
  if (!r) return true
  return r.tables.length === 0 && r.columns.length === 0 && r.data.length === 0
})

async function loadDatabases(connId: string) {
  databases.value = []
  if (!connId) return
  dbLoading.value = true
  try {
    const objects = await invoke<DbObject[]>('get_object_tree', { connectionId: connId })
    const names: string[] = []
    const walk = (list: DbObject[]) => {
      for (const obj of list) {
        if (obj.object_type === 'Database') names.push(obj.name)
        if (obj.children) walk(obj.children)
      }
    }
    walk(objects)
    databases.value = names.sort()
  } catch {
    databases.value = []
  } finally {
    dbLoading.value = false
  }
}

async function runSearch() {
  error.value = ''
  if (!connectionId.value) {
    error.value = t('search.needConnection')
    return
  }
  if (!keyword.value.trim()) {
    error.value = t('search.noKeyword')
    return
  }
  if (!searchTables.value && !searchColumns.value && !searchData.value) {
    error.value = t('search.noScope')
    return
  }

  searching.value = true
  try {
    result.value = await invoke<SearchResult>('global_search', {
      request: {
        connection_id: connectionId.value,
        // PG 只允许当前数据库：搜索范围固定为连接库
        database: supportsAllDatabaseSearch.value ? database.value || null : null,
        keyword: keyword.value.trim(),
        mode: mode.value,
        search_tables: searchTables.value,
        search_columns: searchColumns.value,
        search_data: searchData.value,
      },
    })
  } catch (e) {
    error.value = t('search.failed', { error: String(e) })
    result.value = null
  } finally {
    searching.value = false
  }
}

function openTable(db: string, table: string) {
  emit('open-table', connectionId.value, db, table)
}

watch(connectionId, async (id, old) => {
  if (id === old) return
  database.value = ''
  result.value = null
  await loadDatabases(id)
})

onMounted(async () => {
  const initial = props.initialConnectionId
  const fallback = connectionOptions.value[0]?.value ?? ''
  connectionId.value = initial || fallback
  await loadDatabases(connectionId.value)
  if (props.initialDatabase && databases.value.includes(props.initialDatabase)) {
    database.value = props.initialDatabase
  }
})
</script>

<style scoped>
.global-search {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.dialog-header {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  padding: var(--space-4);
  border-bottom: 1px solid var(--border-default);
}

.header-title {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
}

.title-icon {
  color: var(--brand-500);
  margin-top: 2px;
}

.header-title h2 {
  margin: 0;
  font-size: var(--text-lg);
}

.subtitle {
  margin: 2px 0 0;
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.icon-btn {
  background: none;
  border: none;
  padding: var(--space-1);
  cursor: pointer;
  color: var(--text-secondary);
  border-radius: var(--radius-md);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  transition: var(--transition-colors);
}

.icon-btn:hover {
  background: var(--surface-hover);
  color: var(--text-primary);
}

.dialog-body {
  flex: 1;
  overflow: auto;
  padding: var(--space-4);
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.form-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-4);
}

.form-row {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  min-width: 0;
}

.form-row.full-width {
  grid-column: 1 / -1;
}

.form-row label {
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

.text-input {
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  background: var(--surface-default);
  color: var(--text-primary);
  font-size: var(--text-sm);
  outline: none;
  min-height: 40px;
  transition: var(--transition-colors);
}

.text-input:focus {
  border-color: var(--brand-500);
}

.options-row {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-5);
  align-items: center;
}

.option-group {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.option-label {
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

.segmented {
  display: inline-flex;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  overflow: hidden;
}

.segment-btn {
  padding: var(--space-1) var(--space-3);
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--text-sm);
  cursor: pointer;
  transition: var(--transition-colors);
}

.segment-btn:hover {
  background: var(--surface-hover);
}

.segment-btn.active {
  background: var(--brand-500);
  color: white;
}

.checkbox {
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
  font-size: var(--text-sm);
  color: var(--text-primary);
  cursor: pointer;
  user-select: none;
}

.hint,
.warning-msg,
.error-msg {
  display: flex;
  gap: var(--space-2);
  align-items: center;
  padding: var(--space-3);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  margin: 0;
}

.hint {
  background: var(--surface-hover);
  color: var(--text-secondary);
}

.scope-hint {
  margin-top: var(--space-2);
  font-size: var(--text-xs);
  align-items: flex-start;
}

.warning-msg {
  background: rgba(245, 158, 11, 0.14);
  border: 1px solid rgba(245, 158, 11, 0.35);
  color: var(--warning-500);
  align-items: flex-start;
}

.error-msg {
  background: rgba(239, 68, 68, 0.12);
  border: 1px solid rgba(239, 68, 68, 0.35);
  color: var(--error-500);
}

.actions {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.meta {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.results {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  border-top: 1px solid var(--border-default);
  padding-top: var(--space-4);
}

.summary {
  margin: 0;
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

.truncated {
  margin-left: var(--space-2);
  padding: 1px var(--space-2);
  border-radius: var(--radius-sm);
  background: rgba(245, 158, 11, 0.14);
  color: var(--warning-500);
  font-size: var(--text-xs);
}

.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-3);
  padding: var(--space-8);
  color: var(--text-tertiary);
}

.empty-icon {
  color: var(--text-tertiary);
}

.result-section h3 {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin: 0 0 var(--space-2);
  font-size: var(--text-sm);
  color: var(--text-secondary);
  font-weight: var(--font-medium);
}

.count {
  padding: 0 var(--space-2);
  border-radius: var(--radius-sm);
  background: var(--surface-hover);
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.result-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
}

.result-item {
  width: 100%;
  display: flex;
  align-items: center;
  gap: 2px;
  padding: var(--space-2) var(--space-3);
  border: none;
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--text-primary);
  font-size: var(--text-sm);
  text-align: left;
  cursor: pointer;
  transition: var(--transition-colors);
}

.result-item:hover {
  background: var(--surface-hover);
}

.item-db {
  color: var(--text-tertiary);
}

.item-sep {
  color: var(--text-tertiary);
}

.item-name {
  font-weight: var(--font-medium);
}

.item-column {
  color: var(--brand-500);
}

.badge {
  margin-left: var(--space-2);
  padding: 0 var(--space-2);
  border-radius: var(--radius-sm);
  background: var(--surface-hover);
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

.rows {
  margin-left: auto;
  color: var(--text-secondary);
  font-size: var(--text-xs);
}

.item-arrow {
  margin-left: var(--space-2);
  color: var(--text-tertiary);
}

.result-item:hover .item-arrow {
  color: var(--brand-500);
}

@media (max-width: 768px) {
  .form-grid {
    grid-template-columns: 1fr;
  }
}
</style>
