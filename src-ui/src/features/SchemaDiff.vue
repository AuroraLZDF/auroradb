<template>
  <div class="schema-diff">
    <!-- 头部 -->
    <div class="diff-header">
      <div class="header-title">
        <GitCompare :size="18" class="title-icon" />
        <h2>{{ t('schemaDiff.title') }}</h2>
        <span class="header-hint">{{ t('schemaDiff.hint') }}</span>
      </div>
      <button class="icon-btn" :title="t('common.close')" @click="emit('close')" :aria-label="t('common.close')">
        <X :size="18" />
      </button>
    </div>

    <!-- 选择区 -->
    <div class="compare-bar">
      <div class="endpoint">
        <div class="endpoint-label">
          <span class="dot dot-source"></span>{{ t('schemaDiff.sourceLabel') }}
        </div>
        <div class="endpoint-fields">
          <Select
            v-model="sourceConnectionId"
            :options="connectionOptions"
            :placeholder="t('schemaDiff.selectConnection')"
          />
          <Select
            v-model="sourceDatabase"
            :options="sourceScopes"
            :disabled="!sourceConnectionId || dbLoading.source"
            :placeholder="dbLoading.source ? t('common.loading') : t('schemaDiff.selectDatabase')"
            searchable
          />
        </div>
      </div>

      <button class="swap-btn" :title="t('schemaDiff.swap')" @click="swapEndpoints" :aria-label="t('schemaDiff.swap')">
        <ArrowLeftRight :size="18" />
      </button>

      <div class="endpoint">
        <div class="endpoint-label">
          <span class="dot dot-target"></span>{{ t('schemaDiff.targetLabel') }}
        </div>
        <div class="endpoint-fields">
          <Select
            v-model="targetConnectionId"
            :options="connectionOptions"
            :placeholder="t('schemaDiff.selectConnection')"
          />
          <Select
            v-model="targetDatabase"
            :options="targetScopes"
            :disabled="!targetConnectionId || dbLoading.target"
            :placeholder="dbLoading.target ? t('common.loading') : t('schemaDiff.selectDatabase')"
            searchable
          />
        </div>
      </div>

      <Button
        variant="primary"
        :loading="comparing"
        :disabled="!canCompare"
        @click="runCompare"
      >
        <Play :size="14" /> {{ t('schemaDiff.compare') }}
      </Button>
    </div>

    <div v-if="usesScopes" class="scope-hint">
      <AlertCircle :size="14" />
      <span>{{ t('schemaDiff.pgScopeHint') }}</span>
    </div>

    <div v-if="crossDatabaseApplyBlocked" class="scope-warning">
      <AlertCircle :size="14" />
      <span>{{ t('schemaDiff.crossDatabaseApplyUnsupported') }}</span>
    </div>

    <div v-if="errorMessage" class="diff-error">
      <AlertCircle :size="16" />
      <span>{{ errorMessage }}</span>
    </div>

    <div v-if="successMessage" class="diff-success">
      <CheckCircle2 :size="16" />
      <span>{{ successMessage }}</span>
    </div>

    <!-- 空状态 -->
    <div v-if="!diff" class="diff-empty">
      <GitCompare :size="40" class="empty-icon" />
      <p>{{ t('schemaDiff.emptyTitle') }}</p>
      <span>{{ t('schemaDiff.emptyHint') }}</span>
    </div>

    <template v-else>
      <!-- 概览 -->
      <div class="summary-bar">
        <div class="summary-group">
          <span class="summary-label">{{ t('schemaDiff.summaryTables') }}</span>
          <span class="summary-chip added">{{ t('schemaDiff.addedCount', { count: diff.summary.added_tables }) }}</span>
          <span class="summary-chip changed">{{ t('schemaDiff.changedCount', { count: diff.summary.changed_tables }) }}</span>
          <span class="summary-chip removed">{{ t('schemaDiff.removedCount', { count: diff.summary.removed_tables }) }}</span>
        </div>
        <div class="summary-group">
          <span class="summary-label">{{ t('schemaDiff.summaryColumns') }}</span>
          <span class="summary-chip added">{{ diff.summary.added_columns }}</span>
          <span class="summary-chip changed">{{ diff.summary.changed_columns }}</span>
          <span class="summary-chip removed">{{ diff.summary.removed_columns }}</span>
        </div>
        <div class="summary-group">
          <span class="summary-label">{{ t('schemaDiff.summaryIndexes') }}</span>
          <span class="summary-chip added">{{ diff.summary.added_indexes }}</span>
          <span class="summary-chip changed">{{ diff.summary.changed_indexes }}</span>
          <span class="summary-chip removed">{{ diff.summary.removed_indexes }}</span>
        </div>
        <Button
          size="sm"
          :icon="Copy"
          :disabled="diff.statements.length === 0"
          @click="copyStatements(diff.statements, 'all')"
        >
          {{ copiedKey === 'all' ? t('common.copied') : t('schemaDiff.copyAll') }}
        </Button>
      </div>

      <div v-if="diff.tables.length === 0" class="diff-empty compact">
        <CheckCircle2 :size="32" class="empty-icon success" />
        <p>{{ t('schemaDiff.noDiff') }}</p>
      </div>

      <div v-else class="diff-main">
        <!-- 左侧：表列表 -->
        <div class="table-list">
          <div class="filter-tabs">
            <button
              v-for="tab in filterTabs"
              :key="tab.key"
              class="filter-tab"
              :class="{ active: filter === tab.key }"
              @click="filter = tab.key"
            >
              {{ tab.label }} {{ tab.count }}
            </button>
          </div>
          <div class="table-items">
            <div
              v-for="table in filteredTables"
              :key="table.name"
              class="table-item"
              :class="[table.change, { active: activeTable?.name === table.name }]"
              @click="selectedTableName = table.name"
            >
              <span class="table-name" :title="table.name">{{ table.name }}</span>
              <span class="badge" :class="table.change">{{ changeLabel(table.change) }}</span>
            </div>
            <div v-if="filteredTables.length === 0" class="list-empty">{{ t('schemaDiff.noMatchingTables') }}</div>
          </div>
        </div>

        <!-- 右侧：明细 -->
        <div v-if="activeTable" class="detail-pane">
          <div class="detail-header">
            <h3>{{ activeTable.name }}</h3>
            <span class="badge" :class="activeTable.change">{{ changeLabel(activeTable.change) }}</span>
            <div class="detail-actions">
              <Button
                size="xs"
                variant="danger"
                :icon="Zap"
                :loading="applying"
                :disabled="activeTable.statements.length === 0 || applying || !canApply"
                :title="!canApply ? t('schemaDiff.crossDatabaseApplyUnsupported') : ''"
                @click="applyTableDiff"
              >
                {{ applying ? t('schemaDiff.applying') : t('schemaDiff.apply') }}
              </Button>
              <Button
                size="xs"
                variant="text"
                :icon="Copy"
                @click="copyStatements(activeTable.statements, activeTable.name)"
              >
                {{ copiedKey === activeTable.name ? t('common.copied') : t('schemaDiff.copyTable') }}
              </Button>
            </div>
          </div>

          <div class="detail-body">
            <section v-if="activeTable.columns.length > 0" class="diff-section">
              <h4>{{ t('schemaDiff.columnsDiff', { count: activeTable.columns.length }) }}</h4>
              <div
                v-for="column in activeTable.columns"
                :key="column.name"
                class="diff-row"
              >
                <span class="row-name">{{ column.name }}</span>
                <span class="badge small" :class="column.change">{{ changeLabel(column.change) }}</span>
                <span class="row-detail">{{ columnDetail(column) }}</span>
              </div>
            </section>

            <section v-if="activeTable.indexes.length > 0" class="diff-section">
              <h4>{{ t('schemaDiff.indexesDiff', { count: activeTable.indexes.length }) }}</h4>
              <div
                v-for="index in activeTable.indexes"
                :key="index.name"
                class="diff-row"
              >
                <span class="row-name">{{ index.name }}</span>
                <span class="badge small" :class="index.change">{{ changeLabel(index.change) }}</span>
                <span class="row-detail">{{ indexDetail(index) }}</span>
              </div>
            </section>

            <section class="diff-section">
              <h4>{{ t('schemaDiff.statements') }}</h4>
              <pre class="statement-block" v-html="highlightStatements(activeTable.statements) || t('schemaDiff.noStatements')"></pre>
            </section>
          </div>
        </div>
      </div>
    </template>

    <!-- 底部 -->
    <div class="diff-footer">
      <span v-if="diff" class="footer-hint">
        {{ t('schemaDiff.footerBefore') }} <code>{{ diff.target_database }}</code> {{ t('schemaDiff.footerAfter') }}
      </span>
      <span v-else class="footer-hint"></span>
      <Button variant="ghost" @click="emit('close')">{{ t('common.close') }}</Button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onUnmounted, reactive, ref, watch } from 'vue'
import {
  AlertCircle,
  ArrowLeftRight,
  CheckCircle2,
  Copy,
  GitCompare,
  Play,
  X,
  Zap
} from 'lucide-vue-next'
import Select from '@/components/ui/Select.vue'
import Button from '@/components/ui/Button.vue'
import { invoke } from '@tauri-apps/api/core'
import {
  useDataBrowserStore,
  type DiffChange,
  type SchemaColumnDiff,
  type SchemaDiff,
  type SchemaIndexDiff
} from '@/stores/dataBrowser'
import type { ConnectionStatus } from '@/stores/connection'
import { t } from '@/i18n'
import { confirmDanger } from '@/composables/useConfirm'
import { useToast } from '@/composables/useToast'
import { capabilitiesOf } from '@/utils/driver'
import { formatSql } from '@/utils/sql'

const props = withDefaults(defineProps<{
  connections: ConnectionStatus[]
  initialConnectionId?: string
  initialDatabase?: string
}>(), {
  initialConnectionId: '',
  initialDatabase: ''
})

const emit = defineEmits<{
  close: []
}>()

const dataBrowser = useDataBrowserStore()
const { success: showSuccess } = useToast()

const SYSTEM_DATABASES = ['information_schema', 'mysql', 'performance_schema', 'sys']

const sourceConnectionId = ref(props.initialConnectionId || props.connections[0]?.id || '')
const targetConnectionId = ref(props.initialConnectionId || props.connections[0]?.id || '')
const sourceDatabase = ref(props.initialDatabase)
const targetDatabase = ref('')

const dbCache = reactive<Record<string, string[]>>({})
/** PG 下当前连接库内的 schema 列表（用于把两端都当作「作用域」选择） */
const schemaCache = reactive<Record<string, string[]>>({})
const dbLoading = reactive({ source: false, target: false })

const comparing = ref(false)
const applying = ref(false)
const errorMessage = ref('')
const successMessage = ref('')
const diff = ref<SchemaDiff | null>(null)
const filter = ref<DiffChange | 'all'>('all')
const selectedTableName = ref('')
const copiedKey = ref('')
let copiedTimer: ReturnType<typeof setTimeout> | undefined

onUnmounted(() => {
  if (copiedTimer) clearTimeout(copiedTimer)
})

const connectionOptions = computed(() =>
  props.connections.map((connection) => ({
    label: `${connection.name} (${connection.host}:${connection.port})`,
    value: connection.id
  }))
)


function connectionOf(connectionId: string): ConnectionStatus | undefined {
  return props.connections.find(connection => connection.id === connectionId)
}

function isSchemaScope(connectionId: string): boolean {
  return capabilitiesOf(connectionOf(connectionId)?.driver).tableQualification === 'schema'
}

/** 作用域选项：PG 把「数据库」与「当前库内的 schema」都作为可选作用域（标签区分） */
function scopeOptionsFor(connectionId: string): { value: string; label: string }[] {
  const databases = dbCache[connectionId] ?? []
  if (!isSchemaScope(connectionId)) {
    return databases.map(name => ({ value: name, label: name }))
  }
  const currentDatabase = connectionOf(connectionId)?.database ?? ''
  const databaseOption = (name: string) => ({
    value: name,
    label: t('schemaDiff.databaseScope', { name }),
  })
  const current = databases.filter(name => name === currentDatabase).map(databaseOption)
  const others = databases.filter(name => name !== currentDatabase).map(databaseOption)
  const schemas = (schemaCache[connectionId] ?? []).map(name => ({
    value: name,
    label: t('schemaDiff.schemaScope', { name }),
  }))
  // 当前连接库排最前，其次当前库内 schema，最后其它数据库
  return [...current, ...schemas, ...others]
}

const sourceScopes = computed(() => scopeOptionsFor(sourceConnectionId.value))
const targetScopes = computed(() => scopeOptionsFor(targetConnectionId.value))
const sourceUsesScopes = computed(() => isSchemaScope(sourceConnectionId.value))
const targetUsesScopes = computed(() => isSchemaScope(targetConnectionId.value))
const usesScopes = computed(() => sourceUsesScopes.value || targetUsesScopes.value)

/** 作用域类型：PG 下若所选名称是当前连接库内的 schema 则为 schema，否则按整库处理 */
function scopeKindOf(connectionId: string, scope: string): 'database' | 'schema' {
  if (!isSchemaScope(connectionId)) return 'database'
  return (schemaCache[connectionId] ?? []).includes(scope) ? 'schema' : 'database'
}

const sourceScopeKind = computed(() => scopeKindOf(sourceConnectionId.value, sourceDatabase.value))
const targetScopeKind = computed(() => scopeKindOf(targetConnectionId.value, targetDatabase.value))

/** 跨库 apply 暂不支持：后端只会把 DDL 落到连接配置库 */
const targetConnectionDatabase = computed(
  () => connectionOf(targetConnectionId.value)?.database ?? ''
)
const crossDatabaseApplyBlocked = computed(
  () =>
    targetUsesScopes.value &&
    targetScopeKind.value === 'database' &&
    Boolean(targetDatabase.value) &&
    targetDatabase.value !== targetConnectionDatabase.value
)

const canCompare = computed(() =>
  Boolean(sourceConnectionId.value && sourceDatabase.value &&
    targetConnectionId.value && targetDatabase.value) && !comparing.value
)

/** 目标作用域跨库时禁止 apply（后端只把 DDL 落到连接配置库） */
const canApply = computed(() => !crossDatabaseApplyBlocked.value)

const filterTabs = computed(() => {
  if (!diff.value) return []
  return [
    { key: 'all' as const, label: t('common.all'), count: diff.value.tables.length },
    { key: 'added' as const, label: t('schemaDiff.added'), count: diff.value.summary.added_tables },
    { key: 'changed' as const, label: t('schemaDiff.changed'), count: diff.value.summary.changed_tables },
    { key: 'removed' as const, label: t('schemaDiff.removed'), count: diff.value.summary.removed_tables }
  ]
})

const filteredTables = computed(() => {
  if (!diff.value) return []
  if (filter.value === 'all') return diff.value.tables
  return diff.value.tables.filter((table) => table.change === filter.value)
})

const activeTable = computed(() => {
  const tables = filteredTables.value
  return tables.find((table) => table.name === selectedTableName.value) ?? tables[0] ?? null
})

function changeLabel(change: DiffChange): string {
  if (change === 'added') return t('schemaDiff.added')
  if (change === 'removed') return t('schemaDiff.removed')
  return t('schemaDiff.changed')
}

function columnDetail(column: SchemaColumnDiff): string {
  if (column.details.length > 0) return column.details.join('；')
  return column.source ?? column.target ?? ''
}

function indexDetail(index: SchemaIndexDiff): string {
  return index.source ?? index.target ?? ''
}

function preferredDatabase(databases: string[], current: string): string {
  if (current && databases.includes(current)) return current
  return databases.find((name) => !SYSTEM_DATABASES.includes(name)) ?? databases[0] ?? ''
}

async function loadDatabases(connectionId: string): Promise<string[]> {
  if (!connectionId) return []
  const cached = dbCache[connectionId]
  if (cached) return cached
  try {
    const databases = await dataBrowser.listDatabases(connectionId)
    dbCache[connectionId] = databases
    return databases
  } catch (err) {
    errorMessage.value = t('schemaDiff.loadDatabasesFailed', { message: String(err) })
    return []
  }
}

/** PG 下加载当前连接库内的 schema 列表，用于作用域选择 */
async function loadScopes(connectionId: string): Promise<string[]> {
  if (!connectionId || !isSchemaScope(connectionId)) return []
  const currentDatabase = connectionOf(connectionId)?.database
  if (!currentDatabase) return []
  try {
    const objects = await invoke<{ name: string; object_type: string; schema?: string }[]>(
      'get_object_tree',
      { connectionId, database: currentDatabase }
    )
    const schemas = Array.from(
      new Set(objects.map(object => object.schema).filter((schema): schema is string => Boolean(schema)))
    ).sort()
    schemaCache[connectionId] = schemas
    return schemas
  } catch (err) {
    schemaCache[connectionId] = []
    errorMessage.value = t('schemaDiff.loadScopesFailed', { message: String(err) })
    return []
  }
}

async function syncDatabases(
  connectionId: string,
  database: { value: string },
  setLoading: (loading: boolean) => void
) {
  if (!connectionId) {
    database.value = ''
    return
  }
  setLoading(true)
  try {
    await loadDatabases(connectionId)
    await loadScopes(connectionId)
    // PG 的作用域既包含整库也包含 schema，统一按 value 列表做默认选择
    const scopes = scopeOptionsFor(connectionId).map(option => option.value)
    if (scopes.length > 0) {
      const next = preferredDatabase(scopes, database.value)
      if (next !== database.value) {
        database.value = next
      }
    }
  } finally {
    setLoading(false)
  }
}

watch(sourceConnectionId, (id) => {
  diff.value = null
  void syncDatabases(id, sourceDatabase, (value) => {
    dbLoading.source = value
  })
}, { immediate: true })

watch(targetConnectionId, (id) => {
  diff.value = null
  void syncDatabases(id, targetDatabase, (value) => {
    dbLoading.target = value
  })
}, { immediate: true })

watch([sourceDatabase, targetDatabase], () => {
  diff.value = null
})

function swapEndpoints() {
  const connection = sourceConnectionId.value
  const database = sourceDatabase.value
  sourceConnectionId.value = targetConnectionId.value
  targetConnectionId.value = connection
  sourceDatabase.value = targetDatabase.value
  targetDatabase.value = database
  diff.value = null
}

async function runCompare() {
  if (!canCompare.value) return
  if (sourceConnectionId.value === targetConnectionId.value &&
      sourceDatabase.value === targetDatabase.value) {
    errorMessage.value = t('schemaDiff.sameDatabase')
    return
  }
  // PG 两侧作用域类型必须一致：整库 vs schema 无法对比（后端也会校验，这里提前给出可读提示）
  if (usesScopes.value && sourceScopeKind.value !== targetScopeKind.value) {
    errorMessage.value = t('schemaDiff.scopeMismatch')
    return
  }
  comparing.value = true
  errorMessage.value = ''
  try {
    diff.value = await dataBrowser.compareSchemas(
      sourceConnectionId.value,
      sourceDatabase.value,
      targetConnectionId.value,
      targetDatabase.value
    )
    filter.value = 'all'
    selectedTableName.value = diff.value.tables[0]?.name ?? ''
  } catch (err) {
    diff.value = null
    errorMessage.value = t('schemaDiff.compareFailed', { message: String(err) })
  } finally {
    comparing.value = false
  }
}

async function copyStatements(statements: string[], key: string) {
  if (statements.length === 0) return
  try {
    await navigator.clipboard.writeText(statements.join('\n'))
    copiedKey.value = key
    if (copiedTimer) clearTimeout(copiedTimer)
    copiedTimer = setTimeout(() => {
      copiedKey.value = ''
    }, 1500)
  } catch (err) {
    errorMessage.value = t('schemaDiff.copyFailed', { message: String(err) })
  }
}

function isDestructive(sql: string): boolean {
  return /\b(drop|modify|change|rename)\b/i.test(sql)
}

function hasDestructiveChange(statements: string[]): boolean {
  return statements.some(isDestructive)
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}

/** 预览 DDL 高亮：PG 先用 formatSql 统一格式并高亮双引号标识符，MySQL 保持原文 */
function highlightStatements(statements: string[]): string {
  if (statements.length === 0) return ''
  let sql = statements.join('\n')
  if (targetUsesScopes.value) sql = formatSql(sql)
  const tokens: string[] = []
  // 先把字符串/引号标识符抽成占位符，避免关键字/数字高亮破坏其内容
  const masked = sql.replace(/('(?:[^']|'')*'|"(?:[^"]|"")*"|`(?:[^`]|``)*`)/g, (match) => {
    tokens.push(match)
    return `\uE000${tokens.length - 1}\uE000`
  })
  let html = escapeHtml(masked)
  html = html.replace(
    /\b(CREATE|ALTER|DROP|TABLE|INDEX|VIEW|SCHEMA|ADD|COLUMN|CONSTRAINT|PRIMARY|KEY|UNIQUE|FOREIGN|REFERENCES|ON|DELETE|UPDATE|SET|NOT|NULL|DEFAULT|USING|COMMENT|IS|RENAME|TO|TYPE|GENERATED|ALWAYS|IDENTITY|CASCADE|RESTRICT)\b/g,
    '<span class="sql-keyword">$1</span>'
  )
  html = html.replace(/\uE000(\d+)\uE000/g, (_, index: string) => {
    const token = tokens[Number(index)] ?? ''
    const cssClass = token.startsWith("'") ? 'sql-string' : 'sql-field'
    return `<span class="${cssClass}">${escapeHtml(token)}</span>`
  })
  return html
}

async function applyTableDiff() {
  const table = activeTable.value
  if (!table || table.statements.length === 0 || applying.value) return
  if (crossDatabaseApplyBlocked.value) {
    errorMessage.value = t('schemaDiff.crossDatabaseApplyUnsupported')
    return
  }

  // 仅破坏性操作（DROP/MODIFY/CHANGE/RENAME）才弹确认；纯新增直接执行
  if (hasDestructiveChange(table.statements)) {
    const confirmed = await confirmDanger({
      title: t('schemaDiff.applyConfirmTitle'),
      message: t('schemaDiff.applyConfirm', {
        table: table.name,
        count: table.statements.length,
        database: targetDatabase.value
      }),
      confirmText: t('schemaDiff.apply'),
      requireAcknowledge: true
    })
    if (!confirmed) return
  }

  applying.value = true
  errorMessage.value = ''
  successMessage.value = ''
  try {
    const result = await dataBrowser.applySchemaDiff(
      targetConnectionId.value,
      targetDatabase.value,
      table.statements
    )
    if (result.failed_at != null) {
      errorMessage.value = t('schemaDiff.applyFailedAt', {
        applied: result.applied,
        failedAt: result.failed_at,
        error: result.error ?? ''
      })
    } else {
      successMessage.value = t('schemaDiff.applySuccess', { count: result.applied })
      showSuccess(t('schemaDiff.applySuccess', { count: result.applied }))
      // 同步成功后重新对比，刷新差异视图
      await runCompare()
    }
  } catch (err) {
    errorMessage.value = t('schemaDiff.applyFailed', { message: String(err) })
  } finally {
    applying.value = false
  }
}
</script>

<style scoped>
.schema-diff {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  background: var(--surface-default);
  color: var(--text-primary);
}

/* 头部 */
.diff-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  padding: var(--space-4) var(--space-5);
  border-bottom: 1px solid var(--border-subtle);
  flex-shrink: 0;
}

.header-title {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  min-width: 0;
}

.title-icon {
  color: var(--brand-500);
}

.header-title h2 {
  margin: 0;
  font-size: var(--text-lg);
  font-weight: var(--font-semibold);
  white-space: nowrap;
}

.header-hint {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.icon-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: none;
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  transition: var(--transition-all);
  flex-shrink: 0;
}

.icon-btn:hover {
  background: var(--surface-hover);
  color: var(--text-primary);
}

/* 选择区 */
.compare-bar {
  display: flex;
  align-items: flex-end;
  gap: var(--space-3);
  padding: var(--space-4) var(--space-5);
  border-bottom: 1px solid var(--border-subtle);
  flex-shrink: 0;
}

.endpoint {
  flex: 1;
  min-width: 0;
}

.endpoint-label {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin-bottom: var(--space-2);
  font-size: var(--text-xs);
  color: var(--text-secondary);
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
}

.dot-source {
  background: var(--success-500);
}

.dot-target {
  background: var(--warning-500);
}

.endpoint-fields {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--space-2);
}

.swap-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 36px;
  height: 36px;
  margin-bottom: 2px;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  background: var(--surface-default);
  color: var(--text-secondary);
  cursor: pointer;
  transition: var(--transition-all);
  flex-shrink: 0;
}

.swap-btn:hover {
  border-color: var(--brand-400);
  color: var(--brand-500);
  background: var(--surface-hover);
}

/* 错误提示 */
.diff-error {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin: var(--space-3) var(--space-5) 0;
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--error-500);
  border-radius: var(--radius-md);
  background: rgba(239, 68, 68, 0.08);
  color: var(--error-500);
  font-size: var(--text-sm);
}

.scope-hint {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin: var(--space-3) var(--space-5) 0;
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  background: var(--surface-hover);
  color: var(--text-secondary);
  font-size: var(--text-sm);
}

.scope-warning {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin: var(--space-3) var(--space-5) 0;
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--warning-500, #f59e0b);
  border-radius: var(--radius-md);
  background: rgba(245, 158, 11, 0.08);
  color: var(--warning-500, #b45309);
  font-size: var(--text-sm);
}

.diff-success {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin: var(--space-3) var(--space-5) 0;
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--success-500);
  border-radius: var(--radius-md);
  background: rgba(16, 185, 129, 0.08);
  color: var(--success-500);
  font-size: var(--text-sm);
}

/* 空状态 */
.diff-empty {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-2);
  color: var(--text-tertiary);
  font-size: var(--text-sm);
  min-height: 0;
}

.diff-empty.compact {
  flex: 1;
  gap: var(--space-3);
}

.diff-empty p {
  margin: 0;
  color: var(--text-secondary);
  font-size: var(--text-base);
}

.empty-icon {
  color: var(--border-strong);
}

.empty-icon.success {
  color: var(--success-500);
}

/* 概览 */
.summary-bar {
  display: flex;
  align-items: center;
  gap: var(--space-5);
  padding: var(--space-3) var(--space-5);
  border-bottom: 1px solid var(--border-subtle);
  flex-shrink: 0;
  flex-wrap: wrap;
}

.summary-group {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.summary-label {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.summary-chip {
  padding: 2px 8px;
  border-radius: 999px;
  font-size: var(--text-xs);
  white-space: nowrap;
}

.summary-chip.added {
  background: rgba(16, 185, 129, 0.12);
  color: var(--success-500);
}

.summary-chip.changed {
  background: rgba(245, 158, 11, 0.12);
  color: var(--warning-500);
}

.summary-chip.removed {
  background: rgba(239, 68, 68, 0.12);
  color: var(--error-500);
}

.summary-bar .btn {
  margin-left: auto;
}

/* 主体 */
.diff-main {
  flex: 1;
  display: flex;
  min-height: 0;
}

.table-list {
  width: 280px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  border-right: 1px solid var(--border-subtle);
  min-height: 0;
}

.filter-tabs {
  display: flex;
  gap: var(--space-1);
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--border-subtle);
  flex-shrink: 0;
}

.filter-tab {
  flex: 1;
  padding: var(--space-1) var(--space-2);
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--text-xs);
  cursor: pointer;
  transition: var(--transition-all);
  white-space: nowrap;
}

.filter-tab:hover {
  background: var(--surface-hover);
}

.filter-tab.active {
  background: var(--selected-fill);
  color: var(--brand-500);
  font-weight: var(--font-medium);
}

.table-items {
  flex: 1;
  overflow-y: auto;
  padding: var(--space-2);
}

.table-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-md);
  cursor: pointer;
  border-left: 2px solid transparent;
  transition: var(--transition-all);
}

.table-item:hover {
  background: var(--surface-hover);
}

.table-item.active {
  background: var(--hover-fill);
  border-left-color: var(--brand-500);
}

.table-name {
  font-size: var(--text-sm);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.list-empty {
  padding: var(--space-4);
  text-align: center;
  color: var(--text-tertiary);
  font-size: var(--text-xs);
}

/* 徽标 */
.badge {
  padding: 1px 8px;
  border-radius: 999px;
  font-size: var(--text-xs);
  flex-shrink: 0;
  white-space: nowrap;
}

.badge.small {
  padding: 0 6px;
  font-size: 11px;
}

.badge.added {
  background: rgba(16, 185, 129, 0.12);
  color: var(--success-500);
}

.badge.changed {
  background: rgba(245, 158, 11, 0.12);
  color: var(--warning-500);
}

.badge.removed {
  background: rgba(239, 68, 68, 0.12);
  color: var(--error-500);
}

/* 明细 */
.detail-pane {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
}

.detail-header {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
  border-bottom: 1px solid var(--border-subtle);
  flex-shrink: 0;
}

.detail-header h3 {
  margin: 0;
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.detail-actions {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.detail-body {
  flex: 1;
  overflow-y: auto;
  padding: var(--space-4);
  display: flex;
  flex-direction: column;
  gap: var(--space-5);
}

.diff-section h4 {
  margin: 0 0 var(--space-2);
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  color: var(--text-tertiary);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.diff-row {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
}

.diff-row:nth-child(even) {
  background: var(--bg-secondary);
}

.row-name {
  font-family: var(--font-mono, monospace);
  font-weight: var(--font-medium);
  flex-shrink: 0;
}

.row-detail {
  color: var(--text-secondary);
  font-size: var(--text-xs);
  line-height: 1.6;
  word-break: break-word;
}

.statement-block {
  margin: 0;
  padding: var(--space-3) var(--space-4);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  background: var(--bg-secondary);
  font-family: var(--font-mono, monospace);
  font-size: var(--text-xs);
  line-height: 1.7;
  white-space: pre-wrap;
  word-break: break-word;
  overflow-x: auto;
}

/* DDL 预览高亮 */
.statement-block :deep(.sql-keyword) {
  color: var(--brand-500);
  font-weight: 600;
}

.statement-block :deep(.sql-string) {
  color: var(--success-500);
}

.statement-block :deep(.sql-field) {
  color: var(--info-500, #3b82f6);
}

/* 底部 */
.diff-footer {  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  padding: var(--space-3) var(--space-5);
  border-top: 1px solid var(--border-subtle);
  flex-shrink: 0;
}

.footer-hint {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.footer-hint code {
  padding: 1px 5px;
  border-radius: var(--radius-sm);
  background: var(--bg-secondary);
  font-family: var(--font-mono, monospace);
  color: var(--text-secondary);
}
</style>
