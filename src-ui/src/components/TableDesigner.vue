<template>
  <div class="table-designer">
    <!-- 头部工具栏 -->
    <div class="designer-header">
      <div class="header-left">
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
        <button class="btn-save" @click="saveChanges" :disabled="!hasChanges">
          <Save class="icon" :size="16" /> {{ t('common.save') }}
        </button>
        <button class="btn-add-field" @click="addField">
          <Plus class="icon" :size="16" /> {{ t('tableDesigner.addField') }}
        </button>
        <button class="btn-insert-field" @click="insertField" :disabled="selectedFieldIndex === null">
          <CornerDownLeft class="icon" :size="16" /> {{ t('tableDesigner.insertField') }}
        </button>
        <button class="btn-delete-field" @click="deleteField" :disabled="selectedFieldIndex === null">
          <Trash2 class="icon" :size="16" /> {{ t('tableDesigner.deleteField') }}
        </button>
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
              :class="{ selected: selectedFieldIndex === index, modified: field.isModified, new: field.isNew }"
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
                <select v-model="field.type" @change="onTypeChange(index)">
                  <option v-for="type in dataTypes" :key="type" :value="type">{{ type }}</option>
                </select>
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
                <span v-if="field.isPrimary" class="key-icon primary">🔑</span>
                <span v-else-if="field.isUnique" class="key-icon unique">U</span>
                <span v-else-if="field.isIndex" class="key-icon index">I</span>
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
        <div v-if="fields.length === 0" class="empty-state">
          {{ t('tableDesigner.emptyFields') }}
        </div>

        <!-- 选中字段属性面板 -->
        <div v-if="selectedField" class="field-properties-panel">
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
                <select v-model="index.columns" multiple @change="markIndexModified(idx)">
                  <option v-for="field in fields" :key="field.name" :value="field.name">
                    {{ field.name }}
                  </option>
                </select>
              </td>
              <td class="col-type">
                <select v-model="index.type" @change="markIndexModified(idx)">
                  <option value="BTREE">BTREE</option>
                  <option value="HASH">HASH</option>
                  <option value="FULLTEXT">FULLTEXT</option>
                </select>
              </td>
              <td class="col-unique">
                <input type="checkbox" v-model="index.isUnique" @change="markIndexModified(idx)" />
              </td>
            </tr>
          </tbody>
        </table>
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
                <select v-model="fk.column" @change="markFkModified(idx)">
                  <option v-for="field in fields" :key="field.name" :value="field.name">
                    {{ field.name }}
                  </option>
                </select>
              </td>
              <td class="col-ref-table">
                <input v-model="fk.refTable" type="text" @change="markFkModified(idx)" />
              </td>
              <td class="col-ref-field">
                <input v-model="fk.refColumn" type="text" @change="markFkModified(idx)" />
              </td>
              <td class="col-on-delete">
                <select v-model="fk.onDelete" @change="markFkModified(idx)">
                  <option value="RESTRICT">RESTRICT</option>
                  <option value="CASCADE">CASCADE</option>
                  <option value="SET NULL">SET NULL</option>
                  <option value="NO ACTION">NO ACTION</option>
                </select>
              </td>
              <td class="col-on-update">
                <select v-model="fk.onUpdate" @change="markFkModified(idx)">
                  <option value="RESTRICT">RESTRICT</option>
                  <option value="CASCADE">CASCADE</option>
                  <option value="SET NULL">SET NULL</option>
                  <option value="NO ACTION">NO ACTION</option>
                </select>
              </td>
            </tr>
          </tbody>
        </table>
        <div v-if="foreignKeys.length === 0" class="empty-state">
          {{ t('tableDesigner.emptyForeignKeys') }}
        </div>
      </div>

      <!-- SQL 预览标签页 -->
      <div v-if="activeTab === 'sql'" class="tab-panel">
        <div class="sql-preview-header">
          <span class="label">{{ t('tableDesigner.sqlPreviewTitle') }}</span>
          <button class="btn-copy" @click="copySql">
            <Copy class="icon" :size="16" /> {{ t('common.copy') }}
          </button>
        </div>
        <pre class="sql-code">{{ generatedSql }}</pre>
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
import {
  Save, Plus, CornerDownLeft, Trash2, Key, ArrowUp, ArrowDown, X, Copy
} from 'lucide-vue-next'
import { quoteIdentifier } from '@/utils/sql'
import { t } from '@/i18n'

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
}

interface Index {
  name: string
  columns: string[]
  type: string
  isUnique: boolean
  isModified?: boolean
  isNew?: boolean
  isDeleted?: boolean
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

// 是否有修改
const hasChanges = computed(() => {
  if (props.isNewTable) {
    return newTableName.value && fields.value.length > 0
  }
  return fields.value.some(f => f.isModified || f.isNew || f.isDeleted) ||
         indexes.value.some(i => i.isModified || i.isNew || i.isDeleted) ||
         foreignKeys.value.some(fk => fk.isModified || fk.isNew || fk.isDeleted)
})

// 生成 SQL
const generatedSql = computed(() => {
  const tableName = props.isNewTable ? newTableName.value : props.tableName
  if (!tableName) return t('tableDesigner.sqlNoTableName')

  if (props.isNewTable) {
    // 创建新表
    const fieldDefs = fields.value
      .filter(f => !f.isDeleted)
      .map(fieldToSql)
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

  // 字段修改
  fields.value.forEach(field => {
    if (field.isNew && !field.isDeleted) {
      sqls.push(`ALTER TABLE ${quoteIdentifier(tableName)} ADD COLUMN ${fieldToSql(field)};`)
    } else if (field.isModified && !field.isDeleted && !field.isNew) {
      sqls.push(`ALTER TABLE ${quoteIdentifier(tableName)} MODIFY COLUMN ${fieldToSql(field)};`)
    } else if (field.isDeleted && !field.isNew) {
      sqls.push(`ALTER TABLE ${quoteIdentifier(tableName)} DROP COLUMN ${quoteIdentifier(field.name)};`)
    }
  })

  // 索引修改
  indexes.value.forEach(index => {
    if (index.isNew && !index.isDeleted) {
      const unique = index.isUnique ? 'UNIQUE ' : ''
      sqls.push(`CREATE ${unique}INDEX ${quoteIdentifier(index.name)} ON ${quoteIdentifier(tableName)} (${index.columns.map(c => quoteIdentifier(c)).join(', ')});`)
    } else if (index.isDeleted && !index.isNew) {
      sqls.push(`DROP INDEX ${quoteIdentifier(index.name)} ON ${quoteIdentifier(tableName)};`)
    }
  })

  // 外键修改
  foreignKeys.value.forEach(fk => {
    if (fk.isNew && !fk.isDeleted) {
      sqls.push(`ALTER TABLE ${quoteIdentifier(tableName)} ADD CONSTRAINT ${quoteIdentifier(fk.name)} FOREIGN KEY (${quoteIdentifier(fk.column)}) REFERENCES ${quoteIdentifier(fk.refTable)}(${quoteIdentifier(fk.refColumn)}) ON DELETE ${fk.onDelete} ON UPDATE ${fk.onUpdate};`)
    } else if (fk.isDeleted && !fk.isNew) {
      sqls.push(`ALTER TABLE ${quoteIdentifier(tableName)} DROP FOREIGN KEY ${quoteIdentifier(fk.name)};`)
    }
  })

  return sqls.join('\n\n') || t('tableDesigner.sqlNoChanges')
})

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
    fields.value = structure.columns.map((col: { name: string; data_type: string; length?: number; decimals?: number; is_nullable: boolean; default_value?: string; comment?: string; is_primary: boolean; is_auto_increment: boolean; unsigned?: boolean; on_update_current_timestamp?: boolean }) => ({
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
      isDeleted: false
    }))

    // 转换索引
    indexes.value = structure.indexes.map((idx: { name: string; columns: string[]; type?: string; is_unique: boolean }) => ({
      name: idx.name,
      columns: idx.columns,
      type: idx.type || 'BTREE',
      isUnique: idx.is_unique,
      isModified: false,
      isNew: false,
      isDeleted: false
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
      isDeleted: false
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
function fieldToSql(field: Field): string {
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
  if (field.isPrimary) sql += ' PRIMARY KEY'
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
function onTypeChange(index: number) {
  const field = fields.value[index]!
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
function deleteField() {
  if (selectedFieldIndex.value === null) return
  const field = fields.value[selectedFieldIndex.value]!
  if (field.isNew) {
    fields.value.splice(selectedFieldIndex.value, 1)
  } else {
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
function deleteIndex() {
  if (selectedIndexIndex.value === null) return
  const index = indexes.value[selectedIndexIndex.value]!
  if (index.isNew) {
    indexes.value.splice(selectedIndexIndex.value, 1)
  } else {
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
function deleteForeignKey() {
  if (selectedFkIndex.value === null) return
  const fk = foreignKeys.value[selectedFkIndex.value]!
  if (fk.isNew) {
    foreignKeys.value.splice(selectedFkIndex.value, 1)
  } else {
    fk.isDeleted = true
  }
  selectedFkIndex.value = null
}

// 保存修改
async function saveChanges() {
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
    
    emit('saved')
    
    // 如果是编辑模式，重新加载结构
    if (!props.isNewTable) {
      await loadTableStructure()
    }
  } catch (e: unknown) {
    error.value = t('tableDesigner.saveFailed', { error: String(e) })
  }
}

// 复制 SQL
async function copySql() {
  await navigator.clipboard.writeText(generatedSql.value)
}

// 关闭设计器
function closeDesigner() {
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
  background: var(--color-background);
}

.designer-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 12px;
  background: var(--color-background-soft);
  border-bottom: 1px solid var(--color-border);
}

.header-left {
  display: flex;
  align-items: center;
  min-width: 200px;
}

.table-name-input {
  display: flex;
  align-items: center;
  gap: 8px;
}

.table-name-input label {
  font-weight: 500;
  color: var(--color-text);
}

.table-name-field {
  padding: 6px 10px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  font-size: 14px;
  width: 180px;
  background: var(--color-background);
  color: var(--color-text);
}

.table-name-field:focus {
  border-color: var(--color-primary);
  outline: none;
}

.table-title {
  font-weight: 600;
  font-size: 16px;
  color: var(--color-text);
}

.header-center {
  display: flex;
  gap: 8px;
  flex: 1;
  justify-content: center;
}

.header-center button, .header-left button {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 6px 12px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  background: var(--color-background);
  color: var(--color-text);
  cursor: pointer;
  font-size: 13px;
}

.header-left button:hover:not(:disabled) {
  background: var(--color-background-hover);
}

.header-left button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.btn-save {
  background: var(--color-success) !important;
  color: white !important;
  border-color: var(--color-success) !important;
}

.btn-save:hover:not(:disabled) {
  background: var(--color-primary-dark) !important;
}

.btn-close {
  background: transparent;
  border: none;
  font-size: 20px;
  cursor: pointer;
  color: var(--color-text-secondary);
}

.btn-close:hover {
  color: var(--color-text);
}

.designer-tabs {
  display: flex;
  border-bottom: 1px solid var(--color-border);
  background: var(--color-background-soft);
}

.tab-item {
  padding: 10px 20px;
  cursor: pointer;
  border-right: 1px solid var(--color-border);
  font-size: 13px;
  color: var(--color-text-secondary);
}

.tab-item:hover {
  background: var(--color-background-hover);
}

.tab-item.active {
  background: var(--color-background);
  color: var(--color-primary);
  border-bottom: 2px solid var(--color-primary);
}

.designer-content {
  flex: 1;
  overflow: auto;
  padding: 0;
}

.tab-panel {
  height: 100%;
}

/* 字段表格 */
.fields-table,
.indexes-table,
.fk-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 13px;
}

.fields-table th,
.fields-table td,
.indexes-table th,
.indexes-table td,
.fk-table th,
.fk-table td {
  border: 1px solid var(--color-border);
  padding: 6px 8px;
  text-align: left;
}

.fields-table th,
.indexes-table th,
.fk-table th {
  background: var(--color-background-soft);
  font-weight: 500;
  color: var(--color-text);
}

.fields-table tbody tr:hover,
.indexes-table tbody tr:hover,
.fk-table tbody tr:hover {
  background: var(--color-background-hover);
}

.fields-table tbody tr.selected,
.indexes-table tbody tr.selected,
.fk-table tbody tr.selected {
  background: rgba(25, 118, 210, 0.15);
}

.fields-table tbody tr.modified {
  background: rgba(243, 156, 18, 0.15);
}

.fields-table tbody tr.new {
  background: rgba(76, 175, 80, 0.15);
}

.fields-table tbody tr.deleted {
  opacity: 0.5;
  text-decoration: line-through;
}

.col-selected {
  width: 30px;
  text-align: center;
}

.col-name {
  width: 150px;
}

.col-type {
  width: 120px;
}

.col-length,
.col-decimals {
  width: 80px;
}

.col-nullable,
.col-virtual {
  width: 50px;
  text-align: center;
}

.col-key {
  width: 60px;
  text-align: center;
}

.fields-table input[type="text"],
.fields-table input[type="number"],
.fields-table select,
.indexes-table input[type="text"],
.indexes-table select,
.fk-table input[type="text"],
.fk-table select {
  width: 100%;
  padding: 4px;
  border: 1px solid var(--color-border);
  border-radius: 2px;
  font-size: 12px;
  background: var(--color-background);
  color: var(--color-text);
}

.fields-table input:focus,
.fields-table select:focus,
.indexes-table input:focus,
.indexes-table select:focus,
.fk-table input:focus,
.fk-table select:focus {
  border-color: var(--color-primary);
  outline: none;
}

.key-icon {
  font-size: 12px;
}

.key-icon.primary {
  color: var(--color-warning);
}

.key-icon.unique {
  color: var(--color-success);
  font-weight: bold;
}

.key-icon.index {
  color: var(--color-info);
}

/* 面板工具栏 */
.panel-toolbar {
  display: flex;
  gap: 8px;
  padding: 8px 12px;
  background: var(--color-background-soft);
  border-bottom: 1px solid var(--color-border);
}

.panel-toolbar button {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 5px 10px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  background: var(--color-background);
  color: var(--color-text);
  cursor: pointer;
  font-size: 12px;
}

.panel-toolbar button:hover:not(:disabled) {
  background: var(--color-background-hover);
}

.panel-toolbar button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* SQL 预览 */
.sql-preview-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px;
  background: var(--color-background-soft);
  border-bottom: 1px solid var(--color-border);
}

.sql-preview-header .label {
  font-weight: 500;
  color: var(--color-text);
}

.btn-copy {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 5px 10px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  background: var(--color-background);
  color: var(--color-text);
  cursor: pointer;
  font-size: 12px;
}

.sql-code {
  padding: 16px;
  margin: 0;
  background: #263238;
  color: #aed581;
  font-family: 'Consolas', 'Monaco', monospace;
  font-size: 13px;
  line-height: 1.5;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-all;
}

/* 错误提示 */
.error-message {
  padding: 12px;
  background: rgba(244, 67, 54, 0.1);
  color: var(--color-danger);
  border-top: 1px solid var(--color-danger);
  font-size: 13px;
}

.empty-state {
  padding: 40px;
  text-align: center;
  color: var(--color-text-secondary);
  font-size: 14px;
}

/* 字段属性面板 */
.field-properties-panel {
  margin-top: 16px;
  padding: 16px;
  background: var(--color-background-soft);
  border: 1px solid var(--color-border);
  border-radius: 4px;
}

.field-properties-panel .panel-title {
  font-weight: 600;
  font-size: 14px;
  color: var(--color-text);
  margin-bottom: 12px;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--color-border);
}

.field-properties-panel .properties-form {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.field-properties-panel .property-row {
  display: flex;
  align-items: center;
  gap: 12px;
}

.field-properties-panel .property-row label {
  min-width: 140px;
  font-size: 13px;
  color: var(--color-text);
  display: flex;
  align-items: center;
  gap: 8px;
}

.field-properties-panel .property-row input[type="text"] {
  flex: 1;
  max-width: 300px;
  padding: 6px 10px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  font-size: 13px;
  background: var(--color-background);
  color: var(--color-text);
}

.field-properties-panel .property-row input[type="text"]:focus {
  border-color: var(--color-primary);
  outline: none;
}

.field-properties-panel .property-row input[type="checkbox"] {
  width: 16px;
  height: 16px;
  cursor: pointer;
}
</style>
