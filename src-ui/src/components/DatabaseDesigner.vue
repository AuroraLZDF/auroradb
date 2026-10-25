<template>
  <div class="database-designer">
    <!-- 头部 -->
    <div class="designer-header">
      <h3>{{ isNewDatabase ? t('databaseDesigner.newDatabase') : t('databaseDesigner.editDatabase') }}</h3>
      <button class="btn-close" @click="close"><X :size="18" /></button>
    </div>

    <!-- 内容区域 -->
    <div class="designer-content">
      <!-- 加载状态 -->
      <div v-if="loading" class="loading-state">
        <div class="loading-spinner"></div>
        <p>{{ t('databaseDesigner.loadingInfo') }}</p>
      </div>

      <template v-else>
      <div class="form-group">
        <label>{{ t('databaseDesigner.nameLabel') }}</label>
        <input
          v-model="form.name"
          type="text"
          :placeholder="t('databaseDesigner.namePlaceholder')"
          :disabled="!isNewDatabase"
        />
      </div>

      <div class="form-group">
        <label>{{ t('databaseDesigner.charset') }}</label>
        <CustomSelect
          v-model="form.charset"
          :options="charsets"
          :placeholder="t('databaseDesigner.charsetPlaceholder')"
          searchable
        />
      </div>

      <div class="form-group">
        <label>{{ t('databaseDesigner.collation') }}</label>
        <CustomSelect
          v-model="form.collation"
          :options="collations"
          :placeholder="t('databaseDesigner.collationPlaceholder')"
          searchable
        />
      </div>

      <div class="form-group" v-if="!isNewDatabase">
        <label>{{ t('databaseDesigner.comment') }}</label>
        <textarea
          v-model="form.comment"
          rows="3"
          :placeholder="t('databaseDesigner.commentPlaceholder')"
        ></textarea>
      </div>

      <!-- 错误提示 -->
      <div v-if="errorMessage" class="error-message">
        {{ errorMessage }}
      </div>
      </template>
    </div>

    <!-- 底部按钮 -->
    <div class="designer-footer">
      <button class="btn-secondary" @click="close" :disabled="saving">{{ t('common.cancel') }}</button>
      <button class="btn-primary" @click="save" :disabled="!form.name.trim() || saving || loading">
        {{ saving ? t('databaseDesigner.saving') : t('common.save') }}
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, computed, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import CustomSelect from '@/components/ui/Select.vue'
import { X } from 'lucide-vue-next'
import { quoteIdentifier } from '@/utils/sql'
import { t } from '@/i18n'

const props = defineProps<{
  connectionId: string
  database?: string
  isNewDatabase?: boolean
}>()

const emit = defineEmits<{
  close: []
  saved: [databaseName: string]
}>()

// 表单数据
const form = reactive({
  name: '',
  charset: 'utf8mb4',
  collation: 'utf8mb4_general_ci',
  comment: ''
})

// 字符集与排序规则的映射关系（每个字符集只允许其对应的排序规则）
const CHARSET_COLLATIONS: Record<string, string[]> = {
  // 强烈推荐：现代应用的默认选择，支持 Emoji 和所有 Unicode 字符
  utf8mb4: [
    'utf8mb4_0900_ai_ci',      // MySQL 8.0+ 默认，Unicode 9.0 标准，准确性最高
    'utf8mb4_0900_as_cs',      // 区分重音和大小写
    'utf8mb4_0900_bin',        // 二进制排序，最快但最严格
    'utf8mb4_unicode_ci',      // 基于 UCA 5.2，兼容性好，但不推荐新项目使用
    'utf8mb4_unicode_520_ci',  // 基于 UCA 5.2，有已知的日语排序问题
    'utf8mb4_general_ci',      // 旧版默认，速度快但准确性较差
    'utf8mb4_bin',             // 二进制排序
    'utf8mb4_unicode_520_ci',
    'utf8mb4_ja_0900_as_cs',   // 日语专用排序
    'utf8mb4_zh_0900_as_cs'    // 中文专用排序
  ],
  
  // 不推荐新项目使用：不支持 Emoji，是 utf8mb3 的别名
  utf8: [
    'utf8_general_ci',
    'utf8_unicode_ci',
    'utf8_bin',
    'utf8_unicode_520_ci'
  ],
  utf8mb3: [
    'utf8mb3_general_ci',
    'utf8mb3_unicode_ci',
    'utf8mb3_bin'
  ],

  // 西欧字符集，MySQL 8.0 之前的默认字符集
  latin1: [
    'latin1_swedish_ci',   // 默认排序规则
    'latin1_general_ci',
    'latin1_general_cs',
    'latin1_bin',
    'latin1_german1_ci',
    'latin1_danish_ci'
  ],

  // 中文相关字符集
  gbk: [
    'gbk_chinese_ci',
    'gbk_bin'
  ],
  gb2312: [
    'gb2312_chinese_ci',
    'gb2312_bin'
  ],
  gb18030: [
    'gb18030_chinese_ci',
    'gb18030_bin',
    'gb18030_unicode_520_ci'
  ],
  big5: [
    'big5_chinese_ci',
    'big5_bin'
  ],

  // 其他常用字符集
  ascii: [
    'ascii_general_ci',
    'ascii_bin'
  ],
  binary: [
    'binary'
  ],
  ucs2: [
    'ucs2_general_ci',
    'ucs2_bin',
    'ucs2_unicode_ci',
    'ucs2_unicode_520_ci'
  ],
  utf16: [
    'utf16_general_ci',
    'utf16_bin',
    'utf16_unicode_ci',
    'utf16_unicode_520_ci'
  ],
  utf32: [
    'utf32_general_ci',
    'utf32_bin',
    'utf32_unicode_ci',
    'utf32_unicode_520_ci'
  ],
  
  // 西里尔/东欧字符集
  cp1251: [
    'cp1251_general_ci',
    'cp1251_general_cs',
    'cp1251_bin',
    'cp1251_bulgarian_ci',
    'cp1251_ukrainian_ci'
  ],
  koi8r: [
    'koi8r_general_ci',
    'koi8r_bin'
  ],
  koi8u: [
    'koi8u_general_ci',
    'koi8u_bin'
  ],
  
  // 日韩字符集
  sjis: [
    'sjis_japanese_ci',
    'sjis_bin'
  ],
  cp932: [
    'cp932_japanese_ci',
    'cp932_bin'
  ],
  euckr: [
    'euckr_korean_ci',
    'euckr_bin'
  ],
  eucjpms: [
    'eucjpms_japanese_ci',
    'eucjpms_bin'
  ]
};

// 字符集选项
const charsets = Object.keys(CHARSET_COLLATIONS)

// 当前字符集对应的排序规则选项
const collations = computed(() => {
  return CHARSET_COLLATIONS[form.charset] || []
})

// 状态
const loading = ref(false)
const saving = ref(false)
const errorMessage = ref('')

// 监听字符集变化，自动切换到该字符集的默认排序规则
let isInitializing = true
watch(() => form.charset, (newCharset) => {
  if (isInitializing) return
  const available = CHARSET_COLLATIONS[newCharset] || []
  // 如果当前排序规则不属于新字符集，则切换到默认（第一个）
  if (!available.includes(form.collation)) {
    form.collation = available[0] || ''
  }
})

// 加载数据库当前信息
async function loadDatabaseInfo() {
  if (!props.database || props.isNewDatabase) return

  loading.value = true
  errorMessage.value = ''
  try {
    const info = await invoke<{
      charset?: string
      collation?: string
    }>('get_database_info', {
      connectionId: props.connectionId,
      database: props.database
    })

    form.name = props.database

    // 设置字符集（若不在支持列表中，则回退到 utf8mb4）
    const loadedCharset = info.charset || 'utf8mb4'
    form.charset = CHARSET_COLLATIONS[loadedCharset] ? loadedCharset : 'utf8mb4'

    // 设置排序规则，并校验其是否属于当前字符集
    const available = CHARSET_COLLATIONS[form.charset] || []
    const loadedCollation = info.collation || ''
    if (loadedCollation && available.includes(loadedCollation)) {
      form.collation = loadedCollation
    } else {
      // 排序规则与字符集不匹配时，使用该字符集的默认排序规则
      form.collation = available[0] || ''
    }
  } catch (e) {
    errorMessage.value = t('databaseDesigner.loadFailed', { error: String(e) })
  } finally {
    loading.value = false
    // 初始化完成后允许字符集联动
    setTimeout(() => { isInitializing = false }, 0)
  }
}

// 初始化
if (props.database && !props.isNewDatabase) {
  form.name = props.database
  loadDatabaseInfo()
} else {
  isInitializing = false
}

function close() {
  emit('close')
}

async function save() {
  if (!form.name.trim()) return

  // 保存前校验字符集与排序规则的匹配性
  const available = CHARSET_COLLATIONS[form.charset] || []
  if (!form.collation || !available.includes(form.collation)) {
    errorMessage.value = t('databaseDesigner.collationMismatch', {
      collation: form.collation,
      charset: form.charset,
    })
    return
  }

  saving.value = true
  errorMessage.value = ''
  try {
    if (props.isNewDatabase) {
      await createDatabase()
    } else {
      await alterDatabase()
    }
    emit('saved', form.name.trim())
  } catch (e) {
    errorMessage.value = t('databaseDesigner.saveFailed', { error: String(e) })
  } finally {
    saving.value = false
  }
}

async function createDatabase() {
  await invoke('execute_sql', {
    connectionId: props.connectionId,
    database: 'mysql',
    sql: `CREATE DATABASE ${quoteIdentifier(form.name.trim())} CHARACTER SET ${form.charset} COLLATE ${form.collation}`
  })
}

async function alterDatabase() {
  await invoke('execute_sql', {
    connectionId: props.connectionId,
    database: form.name.trim(),
    sql: `ALTER DATABASE ${quoteIdentifier(form.name.trim())} CHARACTER SET ${form.charset} COLLATE ${form.collation}`
  })
}
</script>

<style scoped>
.database-designer {
  width: 100%;
  min-width: 480px;
  background: var(--bg-primary);
  display: flex;
  flex-direction: column;
  max-height: 90vh;
}

.designer-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 16px 20px;
  border-bottom: 1px solid var(--border-default);
}

.designer-header h3 {
  margin: 0;
  font-size: 18px;
  font-weight: 600;
}

.btn-close {
  background: none;
  border: none;
  font-size: 20px;
  cursor: pointer;
  color: var(--text-secondary);
  padding: 4px 8px;
  border-radius: 4px;
}

.btn-close:hover {
  background: var(--bg-hover);
}

.designer-content {
  flex: 1;
  padding: 20px;
  overflow-y: auto;
}

/* 加载状态 */
.loading-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 40px 20px;
  color: var(--text-secondary);
}

.loading-spinner {
  width: 36px;
  height: 36px;
  border: 3px solid var(--border-default);
  border-top-color: var(--brand-500);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
  margin-bottom: 12px;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

/* 错误提示 */
.error-message {
  padding: 10px 12px;
  background: rgba(239, 68, 68, 0.1);
  border: 1px solid rgba(239, 68, 68, 0.3);
  border-radius: 6px;
  color: var(--color-danger, #ef4444);
  font-size: 13px;
  margin-top: 8px;
}

.form-group {
  margin-bottom: 16px;
}

.form-group label {
  display: block;
  margin-bottom: 8px;
  font-size: 14px;
  font-weight: 500;
  color: var(--text-secondary);
}

.form-group input,
.form-group textarea {
  width: 100%;
  padding: 10px 12px;
  border: 1px solid var(--border-default);
  border-radius: 6px;
  font-size: 14px;
  background: var(--bg-primary);
  color: var(--text-primary);
}

.form-group input:focus,
.form-group textarea:focus {
  outline: none;
  border-color: var(--brand-500);
}

.form-group input:disabled {
  background: var(--bg-secondary);
  color: var(--text-tertiary);
}

.designer-footer {
  display: flex;
  justify-content: flex-end;
  gap: 12px;
  padding: 16px 20px;
  border-top: 1px solid var(--border-default);
}

.btn-secondary {
  padding: 10px 20px;
  border: 1px solid var(--border-default);
  border-radius: 6px;
  background: var(--bg-secondary);
  color: var(--text-primary);
  cursor: pointer;
  font-size: 14px;
}

.btn-secondary:hover {
  background: var(--bg-hover);
}

.btn-primary {
  padding: 10px 20px;
  border: none;
  border-radius: 6px;
  background: var(--brand-500);
  color: white;
  cursor: pointer;
  font-size: 14px;
}

.btn-primary:hover:not(:disabled) {
  background: var(--brand-600);
}

.btn-primary:disabled {
  background: var(--text-tertiary);
  cursor: not-allowed;
}
</style>