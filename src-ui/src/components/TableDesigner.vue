<template>
  <div class="table-designer">
    <!-- 头部工具栏 -->
    <div class="designer-header">
      <div class="header-left">
        <Table2 class="title-icon" :size="16" />
        <div v-if="isNewTable" class="table-name-input">
          <label>{{ t('tableDesigner.tableNameLabel') }}</label>
          <input
            v-model="newTableName"
            type="text"
            :placeholder="t('tableDesigner.tableNamePlaceholder')"
            class="table-name-field"
          />
        </div>
        <div v-else class="table-title">
          {{ tableName }}
        </div>
      </div>
      <div class="header-center">
        <button class="btn-save" @click="saveChanges" :disabled="!hasChanges || saving">
          <Save class="icon" :size="16" /> {{ saving ? t('tableDesigner.saving') : t('common.save') }}
        </button>
        <span class="action-divider"></span>
        <button class="btn-add-field" @click="addField">
          <Plus class="icon" :size="16" /> {{ t('tableDesigner.addField') }}
        </button>
        <button class="btn-insert-field" @click="insertField" :disabled="selectedFieldIndex === null">
          <CornerDownLeft class="icon" :size="16" /> {{ t('tableDesigner.insertField') }}
        </button>
        <button class="btn-delete-field" @click="deleteField" :disabled="selectedFieldIndex === null">
          <Trash2 class="icon" :size="16" /> {{ t('tableDesigner.deleteField') }}
        </button>
        <span class="action-divider"></span>
        <button class="btn-primary-key" @click="togglePrimaryKey" :disabled="selectedFieldIndex === null">
          <Key class="icon" :size="16" /> {{ t('tableDesigner.primaryKey') }}
        </button>
        <button class="btn-move-up" @click="moveFieldUp" :disabled="selectedFieldIndex === null || selectedFieldIndex === 0">
          <ArrowUp class="icon" :size="16" /> {{ t('tableDesigner.moveUp') }}
        </button>
        <button class="btn-move-down" @click="moveFieldDown" :disabled="selectedFieldIndex === null || selectedFieldIndex === fields.length - 1">
          <ArrowDown class="icon" :size="16" /> {{ t('tableDesigner.moveDown') }}
        </button>
      </div>
      <div class="header-right">
        <button class="btn-close" @click="closeDesigner"><X :size="18" /></button>
      </div>
    </div>

    <!-- 标签页 -->
    <div class="designer-tabs">
      <div
        v-for="tab in tabs"
        :key="tab.key"
        class="tab-item"
        :class="{ active: activeTab === tab.key }"
        @click="activeTab = tab.key"
      >
        {{ tab.label }}
      </div>
    </div>

    <!-- 内容区域 -->
    <div class="designer-content">
      <!-- 字段标签页 -->
      <div v-if="activeTab === 'fields'" class="tab-panel">
        <div class="table-wrap">
          <table class="fields-table">
            <thead>
              <tr>
                <th class="col-selected"></th>
                <th class="col-name">{{ t('common.name') }}</th>
                <th class="col-type">{{ t('common.type') }}</th>
                <th class="col-length">{{ t('tableDesigner.length') }}</th>
                <th class="col-decimals">{{ t('tableDesigner.decimals') }}</th>
                <th class="col-nullable">{{ t('tableDesigner.notNull') }}</th>
                <th class="col-virtual">{{ t('tableDesigner.virtual') }}</th>
                <th class="col-key">{{ t('tableDesigner.key') }}</th>
                <th class="col-comment">{{ t('tableDesigner.comment') }}</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="(field, index) in fields"
                :key="field.id"
                :class="{
                  selected: selectedFieldIndex === index,
                  modified: field.isModified,
                  new: field.isNew,
                  deleted: field.isDeleted,
                }"
                @click="selectField(index)"
              >
                <td class="col-selected">
                  <input type="radio" :checked="selectedFieldIndex === index" />
                </td>
                <td class="col-name">
                  <input
                    v-model="field.name"
                    type="text"
                    :placeholder="t('tableDesigner.fieldName')"
                    @change="markFieldModified(index)"
                  />
                </td>
                <td class="col-type">
                  <Select
                    :model-value="field.type"
                    :options="dataTypes"
                    size="sm"
                    @update:model-value="(value) => onTypeChange(index, value)"
                  />
                </td>
                <td class="col-length">
                  <input
                    v-if="hasLength(field.type)"
                    v-model.number="field.length"
                    type="number"
                    min="0"
                    @change="markFieldModified(index)"
                  />
                </td>
                <td class="col-decimals">
                  <input
                    v-if="hasDecimals(field.type)"
                    v-model.number="field.decimals"
                    type="number"
                    min="0"
                    max="30"
                    @change="markFieldModified(index)"
                  />
                </td>
                <td class="col-nullable">
                  <input
                    type="checkbox"
                    v-model="field.notNull"
                    @change="markFieldModified(index)"
                  />
                </td>
                <td class="col-virtual">
                  <input
                    type="checkbox"
                    v-model="field.virtual"
                    @change="markFieldModified(index)"
                  />
                </td>
                <td class="col-key">
                  <span v-if="field.isPrimary" class="key-badge primary">PK</span>
                  <span v-else-if="field.isUnique" class="key-badge unique">UQ</span>
                  <span v-else-if="field.isIndex" class="key-badge index">IDX</span>
                </td>
                <td class="col-comment">
                  <input
                    v-model="field.comment"
                    type="text"
                    :placeholder="t('tableDesigner.comment')"
                    @change="markFieldModified(index)"
                  />
                </td>
              </tr>
            </tbody>
          </table>
        </div>
        <div v-if="fields.length === 0" class="empty-state">
          {{ t('tableDesigner.emptyFields') }}
        </div>
      </div>

      <!-- 索引标签页 -->
      <div v-if="activeTab === 'indexes'" class="tab-panel">
        <div class="panel-toolbar">
          <button class="btn-add" @click="addIndex">
            <Plus class="icon" :size="16" /> {{ t('tableDesigner.addIndex') }}
          </button>
          <button class="btn-delete" @click="deleteIndex" :disabled="selectedIndexIndex === null">
            <Trash2 class="icon" :size="16" /> {{ t('tableDesigner.deleteIndex') }}
          </button>
        </div>
        <div class="table-wrap">
          <table class="indexes-table">
            <thead>
              <tr>
                <th class="col-selected"></th>
                <th class="col-name">{{ t('tableDesigner.indexName') }}</th>
                <th class="col-fields">{{ t('tableDesigner.field') }}</th>
                <th class="col-type">{{ t('common.type') }}</th>
                <th class="col-unique">{{ t('tableDesigner.unique') }}</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="(index, idx) in indexes"
                :key="index.name"
                :class="{ selected: selectedIndexIndex === idx }"
                @click="selectIndex(idx)"
              >
                <td class="col-selected">
                  <input type="radio" :checked="selectedIndexIndex === idx" />
                </td>
                <td class="col-name">
                  <input v-model="index.name" type="text" @change="markIndexModified(idx)" />
                </td>
                <td class="col-fields">
                  <Select
                    :multi-value="index.columns"
                    :options="fieldNameOptions"
                    multiple
                    size="sm"
                    @update:multi-value="updateIndexColumns(idx, $event)"
                  />
                </td>
                <td class="col-type">
                  <Select
                    v-model="index.type"
                    :options="indexTypes"
                    size="sm"
                    @update:model-value="markIndexModified(idx)"
                  />
                </td>
                <td class="col-unique">
                  <input type="checkbox" v-model="index.isUnique" @change="markIndexModified(idx)" />
                </td>
              </tr>
            </tbody>
          </table>
        </div>
        <div v-if="indexes.length === 0" class="empty-state">
          {{ t('tableDesigner.emptyIndexes') }}
        </div>
      </div>

      <!-- 外键标签页 -->
      <div v-if="activeTab === 'foreignKeys'" class="tab-panel">
        <div class="panel-toolbar">
          <button class="btn-add" @click="addForeignKey">
            <Plus class="icon" :size="16" /> {{ t('tableDesigner.addForeignKey') }}
          </button>
          <button class="btn-delete" @click="deleteForeignKey" :disabled="selectedFkIndex === null">
            <Trash2 class="icon" :size="16" /> {{ t('tableDesigner.deleteForeignKey') }}
          </button>
        </div>
        <div class="table-wrap">
          <table class="fk-table">
            <thead>
              <tr>
                <th class="col-selected"></th>
                <th class="col-name">{{ t('tableDesigner.fkName') }}</th>
                <th class="col-field">{{ t('tableDesigner.field') }}</th>
                <th class="col-ref-table">{{ t('tableDesigner.refTable') }}</th>
                <th class="col-ref-field">{{ t('tableDesigner.refField') }}</th>
                <th class="col-on-delete">{{ t('tableDesigner.onDelete') }}</th>
                <th class="col-on-update">{{ t('tableDesigner.onUpdate') }}</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="(fk, idx) in foreignKeys"
                :key="fk.name"
                :class="{ selected: selectedFkIndex === idx }"
                @click="selectForeignKey(idx)"
              >
                <td class="col-selected">
                  <input type="radio" :checked="selectedFkIndex === idx" />
                </td>
                <td class="col-name">
                  <input v-model="fk.name" type="text" @change="markFkModified(idx)" />
                </td>
                <td class="col-field">
                  <Select
                    v-model="fk.column"
                    :options="fieldNameOptions"
                    size="sm"
                    @update:model-value="markFkModified(idx)"
                  />
                </td>
                <td class="col-ref-table">
                  <input v-model="fk.refTable" type="text" @change="markFkModified(idx)" />
                </td>
                <td class="col-ref-field">
                  <input v-model="fk.refColumn" type="text" @change="markFkModified(idx)" />
                </td>
                <td class="col-on-delete">
                  <Select
                    v-model="fk.onDelete"
                    :options="fkActions"
                    size="sm"
                    @update:model-value="markFkModified(idx)"
                  />
                </td>
                <td class="col-on-update">
                  <Select
                    v-model="fk.onUpdate"
                    :options="fkActions"
                    size="sm"
                    @update:model-value="markFkModified(idx)"
                  />
                </td>
              </tr>
            </tbody>
          </table>
        </div>
        <div v-if="foreignKeys.length === 0" class="empty-state">
          {{ t('tableDesigner.emptyForeignKeys') }}
        </div>
      </div>

      <!-- SQL 预览标签页 -->
      <div v-if="activeTab === 'sql'" class="tab-panel">
        <div class="sql-preview">
          <div class="sql-preview-header">
            <span class="label">{{ t('tableDesigner.sqlPreviewTitle') }}</span>
            <button class="btn-copy" @click="copySql">
              <Copy class="icon" :size="16" /> {{ t('common.copy') }}
            </button>
          </div>
          <pre class="sql-code">{{ generatedSql }}</pre>
        </div>
      </div>
    </div>

    <!-- 选中字段属性面板（固定在弹窗底部） -->
    <div v-if="activeTab === 'fields' && selectedField" class="field-properties-panel">
      <div class="panel-title">{{ t('tableDesigner.fieldProperties', { name: selectedField.name }) }}</div>
      <div class="properties-form">
        <!-- 默认值 -->
        <div class="property-row">
          <label>{{ t('tableDesigner.defaultValue') }}</label>
          <input
            v-model="selectedField.defaultValue"
            type="text"
            placeholder="NULL"
            @change="markFieldModified(selectedFieldIndex!)"
          />
        </div>

        <!-- Unsigned (仅整数类型) -->
        <div class="property-row" v-if="isIntegerType(selectedField.type)">
          <label>{{ t('tableDesigner.unsigned') }}</label>
          <input
            type="checkbox"
            v-model="selectedField.unsigned"
            @change="markFieldModified(selectedFieldIndex!)"
          />
        </div>

        <!-- 自增 (仅整数类型) -->
        <div class="property-row" v-if="isIntegerType(selectedField.type)">
          <label>{{ t('tableDesigner.autoIncrement') }}</label>
          <input
            type="checkbox"
            v-model="selectedField.isAutoIncrement"
            @change="onAutoIncrementChange"
          />
        </div>

        <!-- On Update Current_Timestamp (仅时间戳类型) -->
        <div class="property-row" v-if="isTimestampType(selectedField.type)">
          <label>
            <input
              type="checkbox"
              v-model="selectedField.onUpdateCurrentTimestamp"
              @change="markFieldModified(selectedFieldIndex!)"
            />
            On Update CURRENT_TIMESTAMP
          </label>
        </div>
      </div>
    </div>

    <!-- 错误提示 -->
    <div v-if="error" class="error-message">
      {{ error }}
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useDataBrowserStore } from '@/stores/dataBrowser'
import { useToast } from '@/composables/useToast'
import { confirmDanger } from '@/composables/useConfirm'
import { useUnsavedGuard } from '@/composables/useUnsavedGuard'
import {
  Save, Plus, CornerDownLeft, Trash2, Key, ArrowUp, ArrowDown, X, Copy, Table2
} from 'lucide-vue-next'
import { quoteIdentifier } from '@/utils/sql'
import { t } from '@/i18n'
import Select from '@/components/ui/Select.vue'

interface Field {
  id: string
  name: string
  type: string
  length?: number
  decimals?: number
  notNull: boolean
  defaultValue?: string
  comment?: string
  isPrimary: boolean
  isUnique: boolean
  isIndex: boolean
  isAutoIncrement: boolean
  virtual: boolean
  unsigned?: boolean
  onUpdateCurrentTimestamp?: boolean
  isModified?: boolean
  isNew?: boolean
  isDeleted?: boolean
  /** 数据库中原始的列名，用于生成 CHANGE COLUMN 与识别重命名 */
  originalName?: string
  /** 加载时的列顺序，用于判断是否需要生成 FIRST / AFTER 位置子句 */
  originalOrder?: number
  /** 加载时是否为主键，用于判断主键变更 */
  originalPrimary?: boolean
}

interface Index {
  name: string
  columns: string[]
  type: string
  isUnique: boolean
  isModified?: boolean
  isNew?: boolean
  isDeleted?: boolean
  /** 数据库中原始的索引名，重命名时需先 DROP 旧索引 */
  originalName?: string
}

interface ForeignKey {
  name: string
  column: string
  refTable: string
  refColumn: string
  onDelete: string
  onUpdate: string
  isModified?: boolean
  isNew?: boolean
  isDeleted?: boolean
  /** 数据库中原始的外键名，重命名时需先 DROP 旧外键 */
  originalName?: string
}

const props = defineProps<{
  connectionId: string
  database: string
  tableName?: string
  isNewTable?: boolean
}>()

const emit = defineEmits<{
  close: []
  saved: []
}>()

const browserStore = useDataBrowserStore()
const { success: showToast } = useToast()

// 标签页
const tabs = computed(() => [
  { key: 'fields', label: t('tableDesigner.tabFields') },
  { key: 'indexes', label: t('tableDesigner.tabIndexes') },
  { key: 'foreignKeys', label: t('tableDesigner.tabForeignKeys') },
  { key: 'sql', label: t('tableDesigner.tabSql') },
])
const activeTab = ref('fields')

// 数据类型
const dataTypes = [
  'bigint', 'binary', 'bit', 'blob', 'boolean', 'char', 'date', 'datetime',
  'decimal', 'double', 'enum', 'float', 'int', 'json', 'longblob', 'longtext',
  'mediumblob', 'mediumint', 'mediumtext', 'set', 'smallint', 'text',
  'time', 'timestamp', 'tinyblob', 'tinyint', 'tinytext', 'varbinary',
  'varchar', 'year'
]

// 索引类型
const indexTypes = ['BTREE', 'HASH', 'FULLTEXT']

// 外键动作
const fkActions = ['RESTRICT', 'CASCADE', 'SET NULL', 'NO ACTION']

// 数据
const fields = ref<Field[]>([])
const indexes = ref<Index[]>([])
const foreignKeys = ref<ForeignKey[]>([])
const originalStructure = ref<unknown>(null)
const newTableName = ref('')

// 选中状态
const selectedFieldIndex = ref<number | null>(null)
const selectedIndexIndex = ref<number | null>(null)
const selectedFkIndex = ref<number | null>(null)

// 错误提示
const error = ref('')

// 当前选中字段
const selectedField = computed(() => {
  if (selectedFieldIndex.value === null) return null
  return fields.value[selectedFieldIndex.value]
})

// 字段名选项（外键引用列）
const fieldNameOptions = computed(() => fields.value.map(f => f.name))

// 是否有修改
const hasChanges = computed(() => {
  if (props.isNewTable) {
    return newTableName.value && fields.value.length > 0
  }
  return fields.value.some(f => f.isModified || f.isNew || f.isDeleted) ||
         indexes.value.some(i => i.isModified || i.isNew || i.isDeleted) ||
         foreignKeys.value.some(fk => fk.isModified || fk.isNew || fk.isDeleted)
})

// 关闭窗口/刷新前的兜底提示，避免直接丢失未保存的表结构修改
useUnsavedGuard('table-designer', computed(() => Boolean(hasChanges.value)))

// 生成 SQL
const generatedSql = computed(() => {
  const tableName = props.isNewTable ? newTableName.value : props.tableName
  if (!tableName) return t('tableDesigner.sqlNoTableName')

  if (props.isNewTable) {
    // 创建新表
    const fieldDefs = fields.value
      .filter(f => !f.isDeleted)
      .map(f => fieldToSql(f))
      .join(',\n  ')
    
    // 索引（排除主键，因为主键已在字段定义中添加）
    const indexDefs = indexes.value
      .filter(i => !i.isDeleted)
      .map(idx => {
        const unique = idx.isUnique ? 'UNIQUE ' : ''
        return `  ${unique}INDEX ${quoteIdentifier(idx.name)} (${idx.columns.map(c => quoteIdentifier(c)).join(', ')})`
      })
    
    // 外键
    const fkDefs = foreignKeys.value
      .filter(fk => !fk.isDeleted)
      .map(fk => `  CONSTRAINT ${quoteIdentifier(fk.name)} FOREIGN KEY (${quoteIdentifier(fk.column)}) REFERENCES ${quoteIdentifier(fk.refTable)}(${quoteIdentifier(fk.refColumn)}) ON DELETE ${fk.onDelete} ON UPDATE ${fk.onUpdate}`)
    
    const allConstraints = [...indexDefs, ...fkDefs]
    const constraints = allConstraints.length > 0 ? ',\n' + allConstraints.join(',\n') : ''
    
    return `CREATE TABLE ${quoteIdentifier(tableName)} (\n  ${fieldDefs}${constraints}\n);`
  }

  // 修改现有表
  const sqls: string[] = []
  const table = quoteIdentifier(tableName!)

  // 字段修改
  const pkDrops: string[] = []
  const pkAdds: string[] = []
  fields.value.forEach((field, index) => {
    if (field.isNew && !field.isDeleted) {
      sqls.push(`ALTER TABLE ${table} ADD COLUMN ${fieldToSql(field)}${positionClause(index)};`)
    } else if (field.isDeleted && !field.isNew) {
      // 删除时用数据库中的原始列名，避免改过名后又删除导致 DROP 不存在的列
      sqls.push(`ALTER TABLE ${table} DROP COLUMN ${quoteIdentifier(field.originalName ?? field.name)};`)
    } else if (field.isModified && !field.isNew) {
      // 列定义里的 PRIMARY KEY 不能用于 ALTER：已存在主键时重复声明会报错，
      // 因此主键增删单独生成 ADD/DROP PRIMARY KEY
      const definition = fieldToSql(field, false)
      const position = movedPositionClause(field, index)
      const renamed = !!field.originalName && field.originalName !== field.name
      if (renamed) {
        sqls.push(
          `ALTER TABLE ${table} CHANGE COLUMN ${quoteIdentifier(field.originalName!)} ${definition}${position};`
        )
      } else {
        sqls.push(`ALTER TABLE ${table} MODIFY COLUMN ${definition}${position};`)
      }
      if (field.isPrimary !== field.originalPrimary) {
        if (field.isPrimary) {
          pkAdds.push(`ALTER TABLE ${table} ADD PRIMARY KEY (${quoteIdentifier(field.name)});`)
        } else {
          pkDrops.push(`ALTER TABLE ${table} DROP PRIMARY KEY;`)
        }
      }
    }
  })
  sqls.push(...pkDrops, ...pkAdds)

  // 索引修改
  indexes.value.forEach(index => {
    if (index.isNew && !index.isDeleted) {
      const unique = index.isUnique ? 'UNIQUE ' : ''
      const using = index.type && index.type !== 'BTREE' ? ` USING ${index.type}` : ''
      sqls.push(`CREATE ${unique}INDEX ${quoteIdentifier(index.name)} ON ${table} (${index.columns.map(c => quoteIdentifier(c)).join(', ')})${using};`)
    } else if (index.isDeleted && !index.isNew) {
      sqls.push(`DROP INDEX ${quoteIdentifier(index.originalName ?? index.name)} ON ${table};`)
    } else if (index.isModified && !index.isNew) {
      // MySQL 没有通用的“修改索引”语法：先删除旧索引再按新定义创建
      sqls.push(`DROP INDEX ${quoteIdentifier(index.originalName ?? index.name)} ON ${table};`)
      const unique = index.isUnique ? 'UNIQUE ' : ''
      const using = index.type && index.type !== 'BTREE' ? ` USING ${index.type}` : ''
      sqls.push(`CREATE ${unique}INDEX ${quoteIdentifier(index.name)} ON ${table} (${index.columns.map(c => quoteIdentifier(c)).join(', ')})${using};`)
    }
  })

  // 外键修改
  foreignKeys.value.forEach(fk => {
    if (fk.isNew && !fk.isDeleted) {
      sqls.push(`ALTER TABLE ${table} ADD CONSTRAINT ${quoteIdentifier(fk.name)} FOREIGN KEY (${quoteIdentifier(fk.column)}) REFERENCES ${quoteIdentifier(fk.refTable)}(${quoteIdentifier(fk.refColumn)}) ON DELETE ${fk.onDelete} ON UPDATE ${fk.onUpdate};`)
    } else if (fk.isDeleted && !fk.isNew) {
      sqls.push(`ALTER TABLE ${table} DROP FOREIGN KEY ${quoteIdentifier(fk.originalName ?? fk.name)};`)
    } else if (fk.isModified && !fk.isNew) {
      // 外键改名/改引用关系同样需要先删除旧约束
      sqls.push(`ALTER TABLE ${table} DROP FOREIGN KEY ${quoteIdentifier(fk.originalName ?? fk.name)};`)
      sqls.push(`ALTER TABLE ${table} ADD CONSTRAINT ${quoteIdentifier(fk.name)} FOREIGN KEY (${quoteIdentifier(fk.column)}) REFERENCES ${quoteIdentifier(fk.refTable)}(${quoteIdentifier(fk.refColumn)}) ON DELETE ${fk.onDelete} ON UPDATE ${fk.onUpdate};`)
    }
  })

  return sqls.join('\n\n') || t('tableDesigner.sqlNoChanges')
})

/** 生成列位置子句：插到前一个未删除字段之后，否则放到最前 */
function positionClause(index: number): string {
  for (let i = index - 1; i >= 0; i--) {
    const prev = fields.value[i]
    if (prev && !prev.isDeleted) return ` AFTER ${quoteIdentifier(prev.name)}`
  }
  return ' FIRST'
}

/**
 * 字段相对原有顺序是否发生移动。
 * 只在「原有字段之间」比较顺序，避免新增/删除字段导致其余字段被误判为移动。
 */
function movedPositionClause(field: Field, index: number): string {
  if (field.originalOrder === undefined) return ''
  const originals = fields.value.filter(
    f => !f.isDeleted && !f.isNew && f.originalOrder !== undefined
  )
  const currentPos = originals.indexOf(field)
  if (currentPos < 0) return ''
  const originalPos = originals.filter(f => f.originalOrder! < field.originalOrder!).length
  if (currentPos === originalPos) return ''
  return positionClause(index)
}

// 加载表结构
async function loadTableStructure() {
  // 新表模式，不需要加载
  if (props.isNewTable) {
    fields.value = []
    indexes.value = []
    foreignKeys.value = []
    newTableName.value = ''
    return
  }

  try {
    if (!props.tableName) return
    const structure = await browserStore.getTableStructure(props.connectionId, props.database, props.tableName)
    originalStructure.value = structure

    // 转换字段
    fields.value = structure.columns.map((col: { name: string; data_type: string; length?: number; decimals?: number; is_nullable: boolean; default_value?: string; comment?: string; is_primary: boolean; is_auto_increment: boolean; unsigned?: boolean; on_update_current_timestamp?: boolean }, order: number) => ({
      id: Math.random().toString(36).substr(2, 9),
      name: col.name,
      type: col.data_type,
      length: col.length,
      decimals: col.decimals,
      notNull: !col.is_nullable,
      defaultValue: col.default_value,
      comment: col.comment,
      isPrimary: col.is_primary,
      isUnique: false,
      isIndex: false,
      isAutoIncrement: col.is_auto_increment,
      unsigned: col.unsigned,
      onUpdateCurrentTimestamp: col.on_update_current_timestamp,
      virtual: false,
      isModified: false,
      isNew: false,
      isDeleted: false,
      originalName: col.name,
      originalOrder: order,
      originalPrimary: col.is_primary
    }))

    // 转换索引
    indexes.value = structure.indexes.map((idx: { name: string; columns: string[]; type?: string; is_unique: boolean }) => ({
      name: idx.name,
      columns: idx.columns,
      type: idx.type || 'BTREE',
      isUnique: idx.is_unique,
      isModified: false,
      isNew: false,
      isDeleted: false,
      originalName: idx.name
    }))

    // 转换外键
    foreignKeys.value = structure.foreign_keys.map((fk: { name: string; column: string; ref_table: string; ref_column: string; on_delete: string; on_update: string }) => ({
      name: fk.name,
      column: fk.column,
      refTable: fk.ref_table,
      refColumn: fk.ref_column,
      onDelete: fk.on_delete,
      onUpdate: fk.on_update,
      isModified: false,
      isNew: false,
      isDeleted: false,
      originalName: fk.name
    }))

    // 根据索引信息标记字段的唯一/普通索引图标（主键已在字段映射中处理）
    for (const idx of indexes.value) {
      if (idx.name === 'PRIMARY') continue
      const singleUnique = idx.isUnique && idx.columns.length === 1
      for (const colName of idx.columns) {
        const field = fields.value.find(f => f.name === colName)
        if (!field) continue
        if (singleUnique) {
          field.isUnique = true
        } else {
          field.isIndex = true
        }
      }
    }
  } catch (e: unknown) {
    error.value = t('tableDesigner.loadStructureFailed', { error: String(e) })
  }
}

// 字段转 SQL
// includePrimaryKey=false 用于 ALTER 场景：列定义中重复声明 PRIMARY KEY 会报错
function fieldToSql(field: Field, includePrimaryKey = true): string {
  let sql = `${quoteIdentifier(field.name)} ${field.type}`
  
  // 长度/精度/显示宽度，如 varchar(255)、decimal(10,2)、int(11)
  if (field.length) {
    sql += `(${field.length}${field.decimals !== undefined ? ',' + field.decimals : ''})`
  }
  
  // Unsigned (仅整数类型，位于长度之后)
  if (field.unsigned && isIntegerType(field.type)) {
    sql += ' UNSIGNED'
  }
  
  // AUTO_INCREMENT 列必须为 NOT NULL
  if (field.notNull || field.isAutoIncrement) sql += ' NOT NULL'
  
  // 处理默认值
  if (field.defaultValue !== undefined && field.defaultValue !== null && field.defaultValue !== '') {
    const upperDefault = field.defaultValue.toUpperCase().trim()
    // 特殊值不需要引号
    if (upperDefault === 'NULL' || upperDefault === 'CURRENT_TIMESTAMP' || 
        /^\d+$/.test(field.defaultValue) || /^[\d.]+$/.test(field.defaultValue)) {
      sql += ` DEFAULT ${field.defaultValue}`
    } else {
      // 字符串值需要引号
      sql += ` DEFAULT '${field.defaultValue.replace(/'/g, "''")}'`
    }
  }
  
  // On Update CURRENT_TIMESTAMP (仅时间戳类型)
  if (field.onUpdateCurrentTimestamp && isTimestampType(field.type)) {
    sql += ' ON UPDATE CURRENT_TIMESTAMP'
  }
  
  if (field.isAutoIncrement) sql += ' AUTO_INCREMENT'
  if (includePrimaryKey && field.isPrimary) sql += ' PRIMARY KEY'
  if (field.comment) sql += ` COMMENT '${field.comment.replace(/'/g, "''")}'`
  return sql
}

// 类型是否有长度/显示宽度
function hasLength(type: string): boolean {
  return ['char', 'varchar', 'binary', 'varbinary', 'decimal', 'numeric', 'float', 'double',
          'tinyint', 'smallint', 'mediumint', 'int', 'bigint'].includes(type.toLowerCase())
}

// 类型是否有小数位
function hasDecimals(type: string): boolean {
  return ['decimal', 'numeric', 'float', 'double'].includes(type.toLowerCase())
}

// 是否为整数类型
function isIntegerType(type: string): boolean {
  return ['tinyint', 'smallint', 'mediumint', 'int', 'bigint'].includes(type.toLowerCase())
}

// 是否为时间戳类型
function isTimestampType(type: string): boolean {
  return ['timestamp', 'datetime'].includes(type.toLowerCase())
}

// 类型变更
function onTypeChange(index: number, type: string) {
  const field = fields.value[index]!
  field.type = type
  // 切换类型时重置长度/小数位/显示宽度，避免旧类型数值残留
  field.length = undefined
  field.decimals = undefined
  if (['char', 'varchar', 'binary', 'varbinary'].includes(field.type.toLowerCase())) {
    field.length = 255
  }
  // 非整数类型不能是无符号/自增
  if (!isIntegerType(field.type)) {
    field.unsigned = undefined
    field.isAutoIncrement = false
  }
  markFieldModified(index)
}

// 标记字段修改
function markFieldModified(index: number) {
  fields.value[index]!.isModified = true
}

// 标记索引修改
function markIndexModified(index: number) {
  indexes.value[index]!.isModified = true
}

// 更新索引字段列
function updateIndexColumns(index: number, columns: string[]) {
  indexes.value[index]!.columns = columns
  markIndexModified(index)
}

// 标记外键修改
function markFkModified(index: number) {
  foreignKeys.value[index]!.isModified = true
}

// 选择字段
function selectField(index: number) {
  selectedFieldIndex.value = index
}

// 选择索引
function selectIndex(index: number) {
  selectedIndexIndex.value = index
}

// 选择外键
function selectForeignKey(index: number) {
  selectedFkIndex.value = index
}

// 添加字段
function addField() {
  const newField: Field = {
    id: Math.random().toString(36).substr(2, 9),
    name: '',
    type: 'varchar',
    length: 255,
    decimals: undefined,
    notNull: false,
    defaultValue: undefined,
    comment: '',
    isPrimary: false,
    isUnique: false,
    isIndex: false,
    isAutoIncrement: false,
    virtual: false,
    unsigned: false,
    onUpdateCurrentTimestamp: false,
    isModified: false,
    isNew: true,
    isDeleted: false
  }
  fields.value.push(newField)
  selectedFieldIndex.value = fields.value.length - 1
}

// 插入字段
function insertField() {
  if (selectedFieldIndex.value === null) return
  const newField: Field = {
    id: Math.random().toString(36).substr(2, 9),
    name: '',
    type: 'varchar',
    length: 255,
    decimals: undefined,
    notNull: false,
    defaultValue: undefined,
    comment: '',
    isPrimary: false,
    isUnique: false,
    isIndex: false,
    isAutoIncrement: false,
    virtual: false,
    unsigned: false,
    onUpdateCurrentTimestamp: false,
    isModified: false,
    isNew: true,
    isDeleted: false
  }
  fields.value.splice(selectedFieldIndex.value + 1, 0, newField)
  selectedFieldIndex.value = selectedFieldIndex.value + 1
}

// 删除字段
async function deleteField() {
  if (selectedFieldIndex.value === null) return
  const field = fields.value[selectedFieldIndex.value]!
  if (field.isNew) {
    fields.value.splice(selectedFieldIndex.value, 1)
  } else {
    const confirmed = await confirmDanger({
      message: t('tableDesigner.deleteFieldConfirm', { name: field.name }),
      requireAcknowledge: true,
    })
    if (!confirmed) return
    field.isDeleted = true
  }
  selectedFieldIndex.value = null
}

// 切换主键
function togglePrimaryKey() {
  if (selectedFieldIndex.value === null) return
  const field = fields.value[selectedFieldIndex.value]!
  field.isPrimary = !field.isPrimary
  // MySQL 要求 AUTO_INCREMENT 列必须是某个索引（通常为主键），取消主键时同步取消自增
  if (!field.isPrimary) {
    field.isAutoIncrement = false
  }
  markFieldModified(selectedFieldIndex.value)
}

// 切换自增
function onAutoIncrementChange() {
  if (selectedFieldIndex.value === null) return
  const field = fields.value[selectedFieldIndex.value]!
  // AUTO_INCREMENT 列必须为 NOT NULL
  if (field.isAutoIncrement) {
    field.notNull = true
  }
  markFieldModified(selectedFieldIndex.value)
}

// 上移字段
function moveFieldUp() {
  if (selectedFieldIndex.value === null || selectedFieldIndex.value === 0) return
  const idx = selectedFieldIndex.value
  const temp = fields.value[idx]!
  fields.value[idx] = fields.value[idx - 1]!
  fields.value[idx - 1] = temp
  fields.value[idx]!.isModified = true
  fields.value[idx - 1]!.isModified = true
  selectedFieldIndex.value = idx - 1
}

// 下移字段
function moveFieldDown() {
  if (selectedFieldIndex.value === null || selectedFieldIndex.value === fields.value.length - 1) return
  const idx = selectedFieldIndex.value
  const temp = fields.value[idx]!
  fields.value[idx] = fields.value[idx + 1]!
  fields.value[idx + 1] = temp
  fields.value[idx]!.isModified = true
  fields.value[idx + 1]!.isModified = true
  selectedFieldIndex.value = idx + 1
}

// 添加索引
function addIndex() {
  const newIndex: Index = {
    name: `idx_${Date.now()}`,
    columns: [],
    type: 'BTREE',
    isUnique: false,
    isModified: false,
    isNew: true,
    isDeleted: false
  }
  indexes.value.push(newIndex)
  selectedIndexIndex.value = indexes.value.length - 1
}

// 删除索引
async function deleteIndex() {
  if (selectedIndexIndex.value === null) return
  const index = indexes.value[selectedIndexIndex.value]!
  if (index.isNew) {
    indexes.value.splice(selectedIndexIndex.value, 1)
  } else {
    const confirmed = await confirmDanger({
      message: t('tableDesigner.deleteIndexConfirm', { name: index.name }),
      requireAcknowledge: true,
    })
    if (!confirmed) return
    index.isDeleted = true
  }
  selectedIndexIndex.value = null
}

// 添加外键
function addForeignKey() {
  const newFk: ForeignKey = {
    name: `fk_${Date.now()}`,
    column: '',
    refTable: '',
    refColumn: '',
    onDelete: 'RESTRICT',
    onUpdate: 'RESTRICT',
    isModified: false,
    isNew: true,
    isDeleted: false
  }
  foreignKeys.value.push(newFk)
  selectedFkIndex.value = foreignKeys.value.length - 1
}

// 删除外键
async function deleteForeignKey() {
  if (selectedFkIndex.value === null) return
  const fk = foreignKeys.value[selectedFkIndex.value]!
  if (fk.isNew) {
    foreignKeys.value.splice(selectedFkIndex.value, 1)
  } else {
    const confirmed = await confirmDanger({
      message: t('tableDesigner.deleteForeignKeyConfirm', { name: fk.name }),
      requireAcknowledge: true,
    })
    if (!confirmed) return
    fk.isDeleted = true
  }
  selectedFkIndex.value = null
}

// 保存修改
const saving = ref(false)

async function saveChanges() {
  // 防重复提交：执行期间禁用保存按钮，避免重复执行 ALTER
  if (saving.value) return
  saving.value = true
  try {
    error.value = ''
    
    // 新表模式，表名不能为空
    if (props.isNewTable && !newTableName.value.trim()) {
      error.value = t('tableDesigner.tableNameRequired')
      return
    }
    
    // 新表模式，至少需要一个字段
    if (props.isNewTable && fields.value.filter(f => !f.isDeleted).length === 0) {
      error.value = t('tableDesigner.atLeastOneField')
      return
    }
    
    const sql = generatedSql.value
    if (sql.startsWith('--')) {
      error.value = t('tableDesigner.nothingToExecute')
      return
    }
    
    await browserStore.executeSql(props.connectionId, props.database, sql)
    showToast(t('tableDesigner.saveSuccess'))

    emit('saved')
    
    // 编辑模式：重新加载结构；新建模式：保存成功后直接关闭设计器，
    // 避免仍在新建模式下重复编辑导致再次执行 CREATE TABLE 报“表已存在”
    if (props.isNewTable) {
      emit('close')
    } else {
      await loadTableStructure()
    }
  } catch (e: unknown) {
    error.value = t('tableDesigner.saveFailed', { error: String(e) })
  } finally {
    saving.value = false
  }
}

// 复制 SQL
async function copySql() {
  await navigator.clipboard.writeText(generatedSql.value)
}

// 关闭设计器：有未保存修改时先确认，避免误关丢失改动
async function closeDesigner() {
  if (saving.value) return
  if (hasChanges.value) {
    const confirmed = await confirmDanger({
      message: t('tableDesigner.unsavedChangesConfirm'),
      confirmText: t('tableDesigner.discardChanges'),
    })
    if (!confirmed) return
  }
  emit('close')
}

onMounted(() => {
  loadTableStructure()
})
</script>

<style scoped>
.table-designer {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg-primary);
  color: var(--text-primary);
}

/* ---------- 头部 ---------- */
.designer-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
  background: var(--surface-default);
  border-bottom: 1px solid var(--border-subtle);
  box-shadow: var(--shadow-sm);
}

.header-left {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-width: 0;
}

.title-icon {
  flex-shrink: 0;
  color: var(--brand-600);
}

.table-title {
  font-size: var(--text-md);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.table-name-input {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.table-name-input label {
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  color: var(--text-secondary);
  white-space: nowrap;
}

.table-name-field {
  width: 200px;
  height: 30px;
  padding: 0 var(--space-3);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  background: var(--surface-default);
  color: var(--text-primary);
  font-family: inherit;
  font-size: var(--text-sm);
  transition: var(--transition-colors), box-shadow var(--duration-fast) var(--ease-in-out);
}

.table-name-field::placeholder {
  color: var(--text-tertiary);
}

.table-name-field:hover {
  border-color: var(--border-strong);
}

.table-name-field:focus {
  outline: none;
  border-color: var(--brand-500);
  box-shadow: 0 0 0 3px rgba(76, 154, 255, 0.2);
}

.header-center {
  display: flex;
  align-items: center;
  justify-content: center;
  flex-wrap: wrap;
  gap: var(--space-1);
  flex: 1;
}

.header-right {
  display: flex;
  align-items: center;
  flex-shrink: 0;
}

/* ---------- 按钮 ---------- */
.header-center button,
.panel-toolbar button,
.btn-copy {
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
  height: 30px;
  padding: 0 var(--space-3);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  background: var(--surface-default);
  color: var(--text-secondary);
  font-family: inherit;
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  white-space: nowrap;
  cursor: pointer;
  transition: var(--transition-colors);
}

.header-center button:hover:not(:disabled),
.panel-toolbar button:hover:not(:disabled),
.btn-copy:hover:not(:disabled) {
  background: var(--surface-hover);
  border-color: var(--border-strong);
  color: var(--text-primary);
}

.header-center button:active:not(:disabled),
.panel-toolbar button:active:not(:disabled),
.btn-copy:active:not(:disabled) {
  background: var(--surface-active);
}

.header-center button:disabled,
.panel-toolbar button:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.header-center button:focus-visible,
.panel-toolbar button:focus-visible,
.btn-copy:focus-visible,
.btn-close:focus-visible {
  outline: none;
  box-shadow: var(--shadow-focus);
}

.header-center .btn-delete-field:hover:not(:disabled),
.panel-toolbar .btn-delete:hover:not(:disabled) {
  background: var(--error-50);
  border-color: var(--error-300);
  color: var(--error-600);
}

.header-center .btn-save {
  border-color: var(--brand-500);
  background: var(--brand-500);
  color: #fff;
  box-shadow: var(--shadow-sm);
}

.header-center .btn-save:hover:not(:disabled) {
  border-color: var(--brand-600);
  background: var(--brand-600);
  color: #fff;
  box-shadow: var(--shadow-md);
}

.header-center .btn-save:disabled {
  background: var(--brand-500);
  border-color: var(--brand-500);
  color: #fff;
  opacity: 0.45;
  box-shadow: none;
}

.action-divider {
  width: 1px;
  height: 18px;
  margin: 0 var(--space-1);
  background: var(--border-subtle);
}

.btn-close {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  transition: var(--transition-colors);
}

.btn-close:hover {
  background: var(--surface-hover);
  color: var(--text-primary);
}

/* ---------- 标签页 ---------- */
.designer-tabs {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  padding: 0 var(--space-4);
  background: var(--surface-default);
  border-bottom: 1px solid var(--border-subtle);
}

.tab-item {
  position: relative;
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-sm) var(--radius-sm) 0 0;
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  color: var(--text-secondary);
  cursor: pointer;
  user-select: none;
  transition: var(--transition-colors);
}

.tab-item:hover {
  background: var(--surface-hover);
  color: var(--text-primary);
}

.tab-item.active {
  color: var(--brand-600);
}

.tab-item.active::after {
  content: '';
  position: absolute;
  left: var(--space-2);
  right: var(--space-2);
  bottom: -1px;
  height: 2px;
  border-radius: var(--radius-full);
  background: var(--brand-500);
}

.designer-content {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: var(--space-3) var(--space-4) var(--space-4);
}

.tab-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

/* ---------- 表格卡片 ---------- */
.table-wrap {
  background: var(--surface-default);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-sm);
  overflow-x: auto;
}

.fields-table,
.indexes-table,
.fk-table {
  width: 100%;
  border-collapse: separate;
  border-spacing: 0;
  font-size: var(--text-sm);
}

/* 固定列宽布局，避免内容撑宽表格产生横向滚动条 */
.fields-table {
  table-layout: fixed;
}

.fields-table th,
.fields-table td,
.indexes-table th,
.indexes-table td,
.fk-table th,
.fk-table td {
  padding: var(--space-1) var(--space-2);
  text-align: left;
  vertical-align: middle;
  border-bottom: 1px solid var(--border-subtle);
}

.fields-table tbody tr:last-child td,
.indexes-table tbody tr:last-child td,
.fk-table tbody tr:last-child td {
  border-bottom: none;
}

.fields-table th,
.indexes-table th,
.fk-table th {
  height: 32px;
  background: var(--surface-subtle);
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  letter-spacing: 0.02em;
  color: var(--text-tertiary);
  white-space: nowrap;
}

.fields-table thead th:first-child,
.indexes-table thead th:first-child,
.fk-table thead th:first-child {
  border-top-left-radius: var(--radius-md);
}

.fields-table thead th:last-child,
.indexes-table thead th:last-child,
.fk-table thead th:last-child {
  border-top-right-radius: var(--radius-md);
}

.fields-table tbody tr,
.indexes-table tbody tr,
.fk-table tbody tr {
  cursor: pointer;
  transition: background-color var(--duration-fast) var(--ease-in-out);
}

.fields-table tbody tr.modified,
.indexes-table tbody tr.modified,
.fk-table tbody tr.modified {
  background: rgba(245, 158, 11, 0.08);
}

.fields-table tbody tr.new,
.indexes-table tbody tr.new,
.fk-table tbody tr.new {
  background: rgba(16, 185, 129, 0.08);
}

.fields-table tbody tr:hover,
.indexes-table tbody tr:hover,
.fk-table tbody tr:hover {
  background: var(--surface-hover);
}

.fields-table tbody tr.selected,
.indexes-table tbody tr.selected,
.fk-table tbody tr.selected {
  background: var(--surface-selected);
}

.fields-table tbody tr.selected td:first-child,
.indexes-table tbody tr.selected td:first-child,
.fk-table tbody tr.selected td:first-child {
  box-shadow: inset 2px 0 0 var(--brand-500);
}

.fields-table tbody tr.deleted {
  opacity: 0.55;
}

.fields-table tbody tr.deleted input[type="text"] {
  text-decoration: line-through;
}

/* ---------- 列宽 ---------- */
.col-selected {
  width: 36px;
  text-align: center;
}

.col-name {
  width: 170px;
}

.col-type {
  width: 130px;
}

.col-length,
.col-decimals {
  width: 76px;
}

.col-nullable,
.col-virtual {
  width: 56px;
  text-align: center;
}

.col-key {
  width: 64px;
  text-align: center;
}

/* ---------- 单元格控件 ---------- */
.fields-table input[type="text"],
.fields-table input[type="number"],
.indexes-table input[type="text"],
.fk-table input[type="text"] {
  width: 100%;
  height: 26px;
  padding: 0 var(--space-2);
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-primary);
  font-family: inherit;
  font-size: var(--text-sm);
  cursor: text;
  transition: var(--transition-colors), box-shadow var(--duration-fast) var(--ease-in-out);
}

.fields-table input[type="text"]::placeholder,
.indexes-table input[type="text"]::placeholder,
.fk-table input[type="text"]::placeholder {
  color: var(--text-tertiary);
}

.fields-table input[type="text"]:hover:not(:focus),
.fields-table input[type="number"]:hover:not(:focus),
.indexes-table input[type="text"]:hover:not(:focus),
.fk-table input[type="text"]:hover:not(:focus) {
  border-color: var(--border-default);
  background: var(--surface-default);
}

.fields-table input:focus,
.indexes-table input:focus,
.fk-table input:focus {
  outline: none;
  border-color: var(--brand-500);
  background: var(--surface-default);
  box-shadow: 0 0 0 3px rgba(76, 154, 255, 0.2);
}

.fields-table input[type="checkbox"],
.indexes-table input[type="checkbox"],
.fk-table input[type="checkbox"] {
  width: 14px;
  height: 14px;
  accent-color: var(--brand-500);
  cursor: pointer;
}

.fields-table input[type="radio"],
.indexes-table input[type="radio"],
.fk-table input[type="radio"] {
  width: 13px;
  height: 13px;
  accent-color: var(--brand-500);
  cursor: pointer;
}

/* 单元格内自定义下拉框：与文本输入框保持一致的“无边框”外观 */
.fields-table :deep(.custom-select .select-trigger),
.indexes-table :deep(.custom-select .select-trigger),
.fk-table :deep(.custom-select .select-trigger) {
  width: 100%;
  border-color: transparent;
  background: transparent;
}

.fields-table :deep(.select-trigger:hover:not(.open)),
.indexes-table :deep(.select-trigger:hover:not(.open)),
.fk-table :deep(.select-trigger:hover:not(.open)) {
  border-color: var(--border-default);
  background: var(--surface-default);
}

.fields-table :deep(.select-trigger.open),
.indexes-table :deep(.select-trigger.open),
.fk-table :deep(.select-trigger.open) {
  border-color: var(--brand-500);
  background: var(--surface-default);
  box-shadow: 0 0 0 3px rgba(76, 154, 255, 0.2);
}

/* ---------- 关键字段徽章 ---------- */
.key-badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  height: 18px;
  padding: 0 5px;
  border-radius: var(--radius-sm);
  font-size: 10px;
  font-weight: var(--font-bold);
  letter-spacing: 0.03em;
  line-height: 1;
}

.key-badge.primary {
  background: var(--warning-100);
  color: var(--warning-700);
}

.key-badge.unique {
  background: var(--success-100);
  color: var(--success-700);
}

.key-badge.index {
  background: var(--brand-100);
  color: var(--brand-700);
}

[data-theme="dark"] .key-badge.primary {
  background: rgba(245, 158, 11, 0.18);
  color: var(--warning-300);
}

[data-theme="dark"] .key-badge.unique {
  background: rgba(16, 185, 129, 0.18);
  color: var(--success-300);
}

[data-theme="dark"] .key-badge.index {
  background: var(--selected-fill);
  color: var(--brand-300);
}

/* 面板工具栏 */
.panel-toolbar {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

/* ---------- SQL 预览 ---------- */
.sql-preview {
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-sm);
  overflow: hidden;
}

.sql-preview-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-2) var(--space-3);
  background: var(--surface-subtle);
  border-bottom: 1px solid var(--border-subtle);
}

.sql-preview-header .label {
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-secondary);
}

.btn-copy {
  height: 26px;
  padding: 0 var(--space-2);
}

.sql-code {
  margin: 0;
  padding: var(--space-4);
  max-height: 60vh;
  background: var(--canvas);
  color: var(--text-primary);
  font-family: var(--font-mono);
  font-size: var(--text-base);
  line-height: var(--leading-relaxed);
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-word;
}

/* ---------- 错误提示 ---------- */
.error-message {
  margin: 0 var(--space-4) var(--space-4);
  padding: var(--space-2) var(--space-3);
  background: var(--error-50);
  border: 1px solid var(--error-200);
  border-radius: var(--radius-sm);
  color: var(--error-700);
  font-size: var(--text-sm);
  line-height: var(--leading-snug);
}

[data-theme="dark"] .error-message {
  background: rgba(239, 68, 68, 0.12);
  border-color: rgba(239, 68, 68, 0.4);
  color: var(--error-300);
}

/* ---------- 空状态 ---------- */
.empty-state {
  padding: var(--space-10) var(--space-4);
  background: var(--surface-subtle);
  border: 1px dashed var(--border-default);
  border-radius: var(--radius-md);
  text-align: center;
  color: var(--text-tertiary);
  font-size: var(--text-sm);
}

/* ---------- 字段属性面板 ---------- */
.field-properties-panel {
  flex-shrink: 0;
  margin: var(--space-3) var(--space-4) var(--space-3);
  padding: var(--space-3) var(--space-4);
  background: var(--surface-default);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-sm);
}

.field-properties-panel .panel-title {
  margin-bottom: var(--space-3);
  padding-bottom: var(--space-2);
  border-bottom: 1px solid var(--border-subtle);
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.field-properties-panel .properties-form {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: var(--space-3) var(--space-4);
}

.field-properties-panel .property-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-width: 0;
}

.field-properties-panel .property-row label {
  min-width: 140px;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

.field-properties-panel .property-row input[type="text"] {
  flex: 1;
  min-width: 0;
  height: 30px;
  padding: 0 var(--space-2);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-sm);
  background: var(--surface-default);
  color: var(--text-primary);
  font-family: inherit;
  font-size: var(--text-sm);
  transition: var(--transition-colors), box-shadow var(--duration-fast) var(--ease-in-out);
}

.field-properties-panel .property-row input[type="text"]::placeholder {
  color: var(--text-tertiary);
}

.field-properties-panel .property-row input[type="text"]:hover {
  border-color: var(--border-strong);
}

.field-properties-panel .property-row input[type="text"]:focus {
  outline: none;
  border-color: var(--brand-500);
  box-shadow: 0 0 0 3px rgba(76, 154, 255, 0.2);
}

.field-properties-panel .property-row input[type="checkbox"] {
  width: 15px;
  height: 15px;
  accent-color: var(--brand-500);
  cursor: pointer;
}
</style>
