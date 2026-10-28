<template>
  <div class="connection-form">
    <div class="form-header">
      <h2>{{ isEditMode ? t('connectionForm.editTitle') : t('connectionForm.newTitle') }}</h2>
      <button type="button" class="btn-close" @click="emit('cancel')" :title="t('common.close')">×</button>
    </div>
    <form @submit.prevent="handleSubmit">
      <div class="form-tabs" role="tablist">
        <button
          type="button"
          role="tab"
          class="form-tab"
          :class="{ active: activeTab === 'basic' }"
          :aria-selected="activeTab === 'basic'"
          @click="selectTab('basic')"
        >
          {{ t('connectionForm.tabBasic') }}
        </button>
        <button
          type="button"
          role="tab"
          class="form-tab"
          :class="{ active: activeTab === 'advanced' }"
          :aria-selected="activeTab === 'advanced'"
          @click="selectTab('advanced')"
        >
          {{ t('connectionForm.tabAdvanced') }}
          <span v-if="advancedConfigured" class="tab-dot" />
        </button>
      </div>

      <div v-show="activeTab === 'basic'">
        <div class="form-grid">
          <div class="form-group col-4">
            <label>{{ t('connectionForm.driver') }}</label>
            <Select
              :model-value="form.driver"
              :options="driverOptions"
              @update:model-value="(value) => (form.driver = value as DriverKind)"
            />
          </div>

          <div class="form-group col-8">
            <label>{{ t('connectionForm.name') }}</label>
            <input v-model="form.name" type="text" :placeholder="t('connectionForm.namePlaceholder')" />
          </div>

          <div class="form-group col-8">
            <label>{{ t('connectionForm.host') }}</label>
            <input v-model="form.host" type="text" placeholder="localhost" />
          </div>
          <div class="form-group col-4">
            <label>{{ t('connectionForm.port') }}</label>
            <input v-model="form.port" type="number" :placeholder="String(defaultPort)" />
          </div>

          <div class="form-group col-6">
            <label>{{ t('connectionForm.username') }}</label>
            <input v-model="form.username" type="text" />
          </div>
          <div class="form-group col-6">
            <label>{{ t('connectionForm.password') }}</label>
            <div class="password-input-wrapper">
              <input
                v-model="form.password"
                :type="showPassword ? 'text' : 'password'"
                :placeholder="isEditMode ? t('connectionForm.passwordKeepPlaceholder') : t('connectionForm.passwordPlaceholder')"
              />
              <button
                type="button"
                class="btn-toggle-password"
                @click="showPassword = !showPassword"
                :title="showPassword ? t('connectionForm.hidePassword') : t('connectionForm.showPassword')"
              >
                <Eye v-if="showPassword" :size="16" />
                <EyeOff v-else :size="16" />
              </button>
            </div>
          </div>

          <div class="form-group col-6">
            <label>{{ t('common.database') }}</label>
            <input v-model="form.database" type="text" :placeholder="t('common.optional')" />
          </div>
          <div class="form-group col-6">
            <label>{{ t('connectionForm.safetyMode') }}</label>
            <Select
              :model-value="form.safetyMode"
              :options="safetyModeOptions"
              @update:model-value="(value) => (form.safetyMode = value as SafetyMode)"
            />
          </div>
        </div>

        <p v-if="form.driver === 'postgres'" class="form-hint">{{ t('connectionForm.postgresHint') }}</p>
        <p v-if="isEditMode" class="form-hint">{{ t('connectionForm.safetyModeHint') }}</p>
      </div>

      <div v-show="activeTab === 'advanced'">
        <section class="form-section">
          <h3 class="section-title">{{ t('connectionForm.sslSection') }}</h3>
          <div class="form-grid">
            <div class="form-group col-6">
              <label>{{ t('connectionForm.sslMode') }}</label>
              <Select
                :model-value="form.sslMode"
                :options="sslModeOptions"
                @update:model-value="(value) => (form.sslMode = value as SslMode)"
              />
            </div>

            <template v-if="form.sslMode !== 'disabled'">
              <div class="form-group col-12">
                <label>{{ t('connectionForm.sslCaPath') }}</label>
                <input v-model="form.sslCaPath" type="text" :placeholder="t('connectionForm.sslCaPathPlaceholder')" />
              </div>
              <div class="form-group col-6">
                <label>{{ t('connectionForm.sslCertPath') }}</label>
                <input v-model="form.sslCertPath" type="text" :placeholder="t('connectionForm.sslCertPathPlaceholder')" />
              </div>
              <div class="form-group col-6">
                <label>{{ t('connectionForm.sslKeyPath') }}</label>
                <input v-model="form.sslKeyPath" type="text" :placeholder="t('connectionForm.sslKeyPathPlaceholder')" />
              </div>
            </template>
          </div>
        </section>

        <section class="form-section">
          <h3 class="section-title">{{ t('connectionForm.sshSection') }}</h3>
          <label class="checkbox-label">
            <input v-model="form.sshEnabled" type="checkbox" />
            <span>{{ t('connectionForm.sshEnabled') }}</span>
          </label>

        <template v-if="form.sshEnabled">
          <div class="form-grid section-body">
            <div class="form-group col-8">
              <label>{{ t('connectionForm.sshHost') }}</label>
              <input v-model="form.sshHost" type="text" placeholder="jump.example.com" />
            </div>
            <div class="form-group col-4">
              <label>{{ t('connectionForm.sshPort') }}</label>
              <input v-model.number="form.sshPort" type="number" placeholder="22" />
            </div>

            <div class="form-group col-6">
              <label>{{ t('connectionForm.sshUsername') }}</label>
              <input v-model="form.sshUsername" type="text" />
            </div>
            <div class="form-group col-6">
              <label>{{ t('connectionForm.sshAuthType') }}</label>
              <Select
                :model-value="form.sshAuthType"
                :options="sshAuthTypeOptions"
                @update:model-value="(value) => (form.sshAuthType = value as SshAuthType)"
              />
            </div>

            <div v-if="form.sshAuthType === 'password'" class="form-group col-12">
              <label>{{ t('connectionForm.sshPassword') }}</label>
              <div class="password-input-wrapper">
                <input
                  v-model="form.sshPassword"
                  :type="showSshPassword ? 'text' : 'password'"
                  :placeholder="isEditMode && editHasSshSecret ? t('connectionForm.sshSecretKeepPlaceholder') : t('connectionForm.sshPasswordPlaceholder')"
                />
                <button
                  type="button"
                  class="btn-toggle-password"
                  @click="showSshPassword = !showSshPassword"
                  :title="showSshPassword ? t('connectionForm.hidePassword') : t('connectionForm.showPassword')"
                >
                  <Eye v-if="showSshPassword" :size="16" />
                  <EyeOff v-else :size="16" />
                </button>
              </div>
            </div>

            <template v-else>
              <div class="form-group col-8">
                <label>{{ t('connectionForm.sshPrivateKeyPath') }}</label>
                <input v-model="form.sshPrivateKeyPath" type="text" :placeholder="t('connectionForm.sshPrivateKeyPathPlaceholder')" />
              </div>
              <div class="form-group col-4">
                <label>{{ t('connectionForm.sshPassphrase') }}</label>
                <input
                  v-model="form.sshPassphrase"
                  type="password"
                  :placeholder="isEditMode && editHasSshSecret ? t('connectionForm.sshSecretKeepPlaceholder') : t('connectionForm.sshPassphrasePlaceholder')"
                />
              </div>
            </template>
          </div>

          <p class="form-hint">
            {{ t('connectionForm.sshHint') }}
          </p>
        </template>
        </section>
      </div>

      <div v-if="error" class="error-message">
        {{ error }}
      </div>

      <div v-if="duplicateWarning" class="warning-message">
        {{ duplicateWarning }}
      </div>

      <div class="form-footer">
        <label class="checkbox-label">
          <input v-model="savePassword" type="checkbox" />
          <span>{{ t('connectionForm.rememberPassword') }}</span>
        </label>
        <div class="form-actions">
          <button type="button" @click="emit('cancel')">{{ t('common.cancel') }}</button>
          <button type="submit" :disabled="loading">
            {{ loading
              ? (isEditMode ? t('connectionForm.saving') : t('connectionForm.connecting'))
              : (isEditMode ? t('common.save') : t('connectionForm.connect')) }}
          </button>
        </div>
      </div>
    </form>
  </div>
</template>

<script setup lang="ts">
import { reactive, ref, computed, watch, onMounted } from 'vue'
import { Eye, EyeOff } from 'lucide-vue-next'
import Select from '@/components/ui/Select.vue'
import { useConnectionStore } from '@/stores/connection'
import { useSavedConnectionsStore } from '@/stores/savedConnections'
import { useLicenseStore } from '@/stores/license'
import { t } from '@/i18n'
import type { DriverKind, SavedConnection, SafetyMode, SshAuthType, SslMode } from '@/stores/savedConnections'

const props = defineProps<{
  editConnection?: SavedConnection
}>()

const emit = defineEmits<{
  success: []
  cancel: []
}>()

const store = useConnectionStore()
const savedStore = useSavedConnectionsStore()
const licenseStore = useLicenseStore()

const loading = ref(false)
const error = ref('')
const duplicateName = ref('')
const duplicateWarning = computed(() =>
  duplicateName.value ? t('connectionForm.duplicateWarning', { name: duplicateName.value }) : ''
)
const savePassword = ref(true)
const isEditMode = ref(false)
const showPassword = ref(false)
const showSshPassword = ref(false)
const editHasSshSecret = ref(false)
/** 当前 Tab：基础连接 / 高级设置（SSL、SSH） */
const activeTab = ref<'basic' | 'advanced'>('basic')

/** 各驱动的默认端口：切换驱动时若端口仍是旧默认值则自动跟随 */
const DEFAULT_PORTS: Record<DriverKind, number> = { mysql: 3306, postgres: 5432 }

const form = reactive({
  driver: 'mysql' as DriverKind,
  name: '',
  host: 'localhost',
  port: DEFAULT_PORTS.mysql,
  database: '',
  username: '',
  password: '',
  safetyMode: 'off' as SafetyMode,
  sslMode: 'preferred' as SslMode,
  sslCaPath: '',
  sslCertPath: '',
  sslKeyPath: '',
  sshEnabled: false,
  sshHost: '',
  sshPort: 22,
  sshUsername: '',
  sshAuthType: 'password' as SshAuthType,
  sshPassword: '',
  sshPrivateKeyPath: '',
  sshPassphrase: '',
})

const defaultPort = computed(() => DEFAULT_PORTS[form.driver])

/** 下拉选项（随语言切换更新文案） */
const driverOptions = computed(() => [
  { label: t('connectionForm.driverMysql'), value: 'mysql' },
  { label: t('connectionForm.driverPostgres'), value: 'postgres' },
])

const safetyModeOptions = computed(() => [
  { label: t('connectionForm.safetyModeOff'), value: 'off' },
  { label: t('connectionForm.safetyModeSafe'), value: 'safe' },
  { label: t('connectionForm.safetyModeReadOnly'), value: 'read-only' },
])

const sslModeOptions = computed(() => [
  { label: t('connectionForm.sslPreferred'), value: 'preferred' },
  { label: t('connectionForm.sslDisabled'), value: 'disabled' },
  { label: t('connectionForm.sslRequired'), value: 'required' },
  { label: t('connectionForm.sslVerifyCa'), value: 'verify-ca' },
  { label: t('connectionForm.sslVerifyIdentity'), value: 'verify-identity' },
])

const sshAuthTypeOptions = computed(() => [
  { label: t('connectionForm.sshAuthPassword'), value: 'password' },
  { label: t('connectionForm.sshAuthPrivateKey'), value: 'private-key' },
])

/** 高级设置里是否已有内容，用于在 Tab 上显示标记 */
const advancedConfigured = computed(
  () => form.sshEnabled || !!(form.sslCaPath || form.sslCertPath || form.sslKeyPath)
)

/** 手动切换 Tab 时清掉上一个 Tab 的校验提示 */
function selectTab(tab: 'basic' | 'advanced') {
  activeTab.value = tab
  error.value = ''
}

// 端口联动：只在端口仍是上一驱动的默认值时跟随切换，避免覆盖用户自定义端口
watch(
  () => form.driver,
  (driver, prev) => {
    if (Number(form.port) === DEFAULT_PORTS[prev]) {
      form.port = DEFAULT_PORTS[driver]
    }
  }
)

// 如果是编辑模式，加载现有连接数据
onMounted(async () => {
  if (props.editConnection) {
    isEditMode.value = true
    form.driver = props.editConnection.driver ?? 'mysql'
    form.name = props.editConnection.name
    form.host = props.editConnection.host
    form.port = props.editConnection.port
    form.database = props.editConnection.database || ''
    form.username = props.editConnection.username
    form.safetyMode = props.editConnection.safetyMode ?? 'off'
    form.sslMode = props.editConnection.sslMode ?? 'preferred'
    form.sslCaPath = props.editConnection.sslCaPath || ''
    form.sslCertPath = props.editConnection.sslCertPath || ''
    form.sslKeyPath = props.editConnection.sslKeyPath || ''
    form.sshEnabled = props.editConnection.sshEnabled === true
    form.sshHost = props.editConnection.sshHost || ''
    form.sshPort = props.editConnection.sshPort ?? 22
    form.sshUsername = props.editConnection.sshUsername || ''
    form.sshAuthType = props.editConnection.sshAuthType ?? 'password'
    form.sshPrivateKeyPath = props.editConnection.sshPrivateKeyPath || ''

    // 有已保存的凭据时默认勾选「记住密码」
    let hasSavedSecret = false

    // 尝试获取保存的密码
    try {
      const savedPassword = await savedStore.getPassword(props.editConnection.id)
      if (savedPassword) {
        form.password = savedPassword
        hasSavedSecret = true
      } else {
        form.password = ''
      }
    } catch {
      form.password = ''
    }

    // 回填 SSH 密码或私钥口令
    if (form.sshEnabled && props.editConnection.hasSshSecret) {
      try {
        const secret = await savedStore.getSshSecret(props.editConnection.id)
        if (secret) {
          editHasSshSecret.value = true
          hasSavedSecret = true
          if (form.sshAuthType === 'password') {
            form.sshPassword = secret
          } else {
            form.sshPassphrase = secret
          }
        }
      } catch {
        // 读取失败时按未保存处理，用户可重新输入
      }
    }

    savePassword.value = hasSavedSecret
  }
})

// 监听表单变化，检测重复
watch(
  () => ({ driver: form.driver, host: form.host, port: form.port, username: form.username, database: form.database }),
  (newConfig) => {
    const existing = savedStore.findExistingConnection?.(newConfig)
    if (existing) {
      duplicateName.value = existing.name
    } else {
      duplicateName.value = ''
    }
  },
  { deep: true }
)

/** 必填项校验：返回首个缺失项所在 Tab，便于切回并提示（隐藏 Tab 里的原生校验不可见） */
function findMissingRequired(): { tab: 'basic' | 'advanced'; field: string } | null {
  if (!form.name.trim()) return { tab: 'basic', field: t('connectionForm.name') }
  if (!form.host.trim()) return { tab: 'basic', field: t('connectionForm.host') }
  if (!String(form.port).trim()) return { tab: 'basic', field: t('connectionForm.port') }
  if (!form.username.trim()) return { tab: 'basic', field: t('connectionForm.username') }
  if (!isEditMode.value && !form.password) return { tab: 'basic', field: t('connectionForm.password') }

  if (form.sshEnabled) {
    if (!form.sshHost.trim()) return { tab: 'advanced', field: t('connectionForm.sshHost') }
    if (!String(form.sshPort).trim()) return { tab: 'advanced', field: t('connectionForm.sshPort') }
    if (!form.sshUsername.trim()) return { tab: 'advanced', field: t('connectionForm.sshUsername') }
    if (!isEditMode.value && form.sshAuthType === 'password' && !form.sshPassword) {
      return { tab: 'advanced', field: t('connectionForm.sshPassword') }
    }
    if (form.sshAuthType === 'private-key' && !form.sshPrivateKeyPath.trim()) {
      return { tab: 'advanced', field: t('connectionForm.sshPrivateKeyPath') }
    }
  }

  return null
}

async function handleSubmit() {
  error.value = ''

  if (form.sshEnabled && !licenseStore.requirePro('ssh-tunnel', t('license.feature.ssh-tunnel'))) {
    return
  }

  const missing = findMissingRequired()
  if (missing) {
    activeTab.value = missing.tab
    error.value = t('connectionForm.fieldRequired', { field: missing.field })
    return
  }

  loading.value = true

  try {
    const sslConfig = {
      mode: form.sslMode,
      ca_path: form.sslCaPath.trim() || undefined,
      cert_path: form.sslCertPath.trim() || undefined,
      key_path: form.sslKeyPath.trim() || undefined,
    }

    // SSH 隧道配置：启用时数据库主机/端口由 SSH 服务器侧解析
    const sshPort = Number(form.sshPort) || 22
    const sshConfig = form.sshEnabled
      ? {
          host: form.sshHost.trim(),
          port: sshPort,
          username: form.sshUsername.trim(),
          auth_type: form.sshAuthType,
          password: form.sshAuthType === 'password' ? form.sshPassword || undefined : undefined,
          private_key_path:
            form.sshAuthType === 'private-key' ? form.sshPrivateKeyPath.trim() || undefined : undefined,
          passphrase: form.sshAuthType === 'private-key' ? form.sshPassphrase || undefined : undefined,
        }
      : undefined

    // 保存到本地时一并记录 SSH 配置；secret 仅在勾选「记住密码」时持久化
    const sshSaved = {
      sshEnabled: form.sshEnabled,
      sshHost: form.sshHost,
      sshPort,
      sshUsername: form.sshUsername,
      sshAuthType: form.sshAuthType,
      sshPrivateKeyPath: form.sshPrivateKeyPath,
      sshSecret: savePassword.value
        ? (form.sshAuthType === 'password' ? form.sshPassword : form.sshPassphrase) || undefined
        : undefined,
    }

    // 检查是否已存在相同连接（仅新建模式）
    const existing = !isEditMode.value ? savedStore.findExistingConnection?.({
      driver: form.driver,
      host: form.host,
      port: form.port,
      username: form.username,
      database: form.database
    }) : null

    if (isEditMode.value && props.editConnection) {
      // 编辑模式：更新现有连接配置
      await savedStore.saveConnection({
        id: props.editConnection.id,
        name: form.name,
        driver: form.driver,
        host: form.host,
        port: form.port,
        database: form.database,
        username: form.username,
        password: savePassword.value && form.password ? form.password : undefined,
        sslMode: form.sslMode,
        sslCaPath: form.sslCaPath,
        sslCertPath: form.sslCertPath,
        sslKeyPath: form.sslKeyPath,
        safetyMode: form.safetyMode,
        ...sshSaved,
      })
    } else {
      // 新建模式：创建新连接
      // 生成新的连接ID
      const connectionId = `conn_${Date.now()}`

      // 创建连接
      await store.createConnection({
        id: connectionId,
        name: form.name,
        driver: form.driver,
        host: form.host,
        port: form.port,
        database: form.database,
        username: form.username,
        password: form.password,
        safety_mode: form.safetyMode,
        ssl_config: sslConfig,
        pool_config: {
          max_connections: 10,
          min_connections: 2,
          connect_timeout: 10,
          idle_timeout: 300,
          max_lifetime: 1800,
          heartbeat_interval: 30,
        },
      }, sshConfig)

      // 保存连接配置（含密码）- 关键修复：await + 密码传递
      await savedStore.saveConnection({
        id: existing?.id, // 如果是重复连接，使用原ID
        name: form.name,
        driver: form.driver,
        host: form.host,
        port: form.port,
        database: form.database,
        username: form.username,
        password: savePassword.value ? form.password : undefined,
        sslMode: form.sslMode,
        sslCaPath: form.sslCaPath,
        sslCertPath: form.sslCertPath,
        sslKeyPath: form.sslKeyPath,
        safetyMode: form.safetyMode,
        ...sshSaved,
      })
    }

    emit('success')
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
}
</script>

<style scoped>
.connection-form {
  padding: 20px;
  width: 100%;
}

.form-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 20px;
}

.form-header h2 {
  margin: 0;
  font-size: 18px;
}

.btn-close {
  background: none;
  border: none;
  color: var(--color-text-secondary);
  cursor: pointer;
  font-size: 20px;
  padding: 0;
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 4px;
}

.btn-close:hover {
  background: var(--color-background-hover);
  color: var(--color-text);
}

.form-tabs {
  display: flex;
  gap: 4px;
  margin-bottom: 18px;
  border-bottom: 1px solid var(--color-border);
}

.form-tab {
  position: relative;
  display: flex;
  align-items: center;
  gap: 6px;
  margin-bottom: -1px;
  padding: 8px 14px;
  border: none;
  border-bottom: 2px solid transparent;
  border-radius: 0;
  background: none;
  color: var(--color-text-secondary);
  font-size: 13px;
  cursor: pointer;
}

.form-tab:hover {
  color: var(--color-text);
  background: none;
}

.form-tab.active {
  color: var(--color-primary);
  border-bottom-color: var(--color-primary);
}

.tab-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--color-success);
}

.form-group {
  margin-bottom: 0;
}

.form-grid {
  display: grid;
  grid-template-columns: repeat(12, minmax(0, 1fr));
  gap: 14px 16px;
  align-items: start;
}

.col-4 {
  grid-column: span 4;
}

.col-6 {
  grid-column: span 6;
}

.col-8 {
  grid-column: span 8;
}

.col-12 {
  grid-column: span 12;
}

.form-section + .form-section {
  margin-top: 18px;
  padding-top: 18px;
  border-top: 1px solid var(--color-border);
}

.section-title {
  margin: 0 0 10px;
  font-size: 13px;
  font-weight: 600;
  color: var(--color-text);
}

.section-body {
  margin-top: 10px;
}

.form-group label {
  display: block;
  margin-bottom: 4px;
  font-weight: 500;
  font-size: 13px;
  color: var(--color-text);
}

.form-group input {
  width: 100%;
  padding: 8px 12px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  font-size: 14px;
  background: var(--color-background);
  color: var(--color-text);
}

/* 自定义下拉框：触发器与同组输入框保持相同的尺寸与配色 */
.form-group :deep(.custom-select .select-trigger) {
  width: 100%;
  height: 38px;
  min-height: 38px;
  padding: 0 12px;
  border-color: var(--color-border);
  border-radius: 4px;
  background: var(--color-background);
  color: var(--color-text);
  font-size: 14px;
}

.password-input-wrapper {
  position: relative;
  display: flex;
  align-items: center;
}

.password-input-wrapper input {
  flex: 1;
  padding-right: 40px;
}

.btn-toggle-password {
  position: absolute;
  right: 8px;
  background: none;
  border: none;
  cursor: pointer;
  color: var(--color-text-secondary);
  padding: 4px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 4px;
}

.btn-toggle-password:hover {
  color: var(--color-text);
  background: var(--color-background-hover);
}

.form-footer {
  position: sticky;
  bottom: 0;
  z-index: 1;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
  margin: 22px -20px -20px;
  padding: 16px 20px;
  border-top: 1px solid var(--color-border);
  background: var(--surface-default);
}

.form-actions {
  display: flex;
  gap: 12px;
  justify-content: flex-end;
  margin-top: 0;
}

.checkbox-label {
  display: flex !important;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  font-weight: normal !important;
  color: var(--color-text);
}

.checkbox-label input {
  width: auto !important;
}

.form-hint {
  margin: 10px 0 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--color-text-secondary);
}

button {
  padding: 8px 16px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  background: var(--color-background);
  color: var(--color-text);
  cursor: pointer;
  font-size: 14px;
}

button[type="submit"] {
  background: var(--color-success);
  color: white;
  border-color: var(--color-success);
}

button:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.error-message {
  color: var(--color-danger);
  margin-top: 12px;
  padding: 8px;
  background: rgba(244, 67, 54, 0.1);
  border-radius: 4px;
  font-size: 13px;
}

.warning-message {
  color: var(--color-warning);
  margin-top: 12px;
  padding: 8px;
  background: rgba(243, 156, 18, 0.1);
  border-radius: 4px;
  font-size: 13px;
}

@media (max-width: 640px) {
  .col-4,
  .col-6,
  .col-8 {
    grid-column: span 12;
  }
}
</style>
