<template>
  <div class="ai-settings">
    <header class="ai-settings-head">
      <h2><Bot :size="18" class="head-icon" />{{ t('aiSettings.title') }}</h2>
      <button class="ai-settings-close" :title="t('common.close')" :aria-label="t('common.close')" @click="emit('close')">
        <X :size="16" />
      </button>
    </header>
    <p class="ai-settings-desc">{{ t('aiSettings.desc') }}</p>

    <div class="ai-settings-form">
      <label class="ai-field">
        <span>{{ t('aiSettings.baseUrl') }}</span>
        <input v-model="baseUrl" type="text" spellcheck="false" :placeholder="'https://api.openai.com/v1'" />
        <span class="ai-field-hint">{{ t('aiSettings.baseUrlHint') }}</span>
      </label>
      <label class="ai-field">
        <span>{{ t('aiSettings.model') }}</span>
        <input v-model="model" type="text" spellcheck="false" placeholder="gpt-4o-mini" />
      </label>
      <label class="ai-field">
        <span>{{ t('aiSettings.apiKey') }}</span>
        <input
          v-model="apiKey"
          type="password"
          autocomplete="off"
          :placeholder="hasKey ? t('aiSettings.keySaved') : t('aiSettings.keyPlaceholder')"
        />
        <span class="ai-field-hint">{{ t('aiSettings.keyHint') }}</span>
      </label>
    </div>

    <p v-if="testMessage" :class="['ai-test-msg', testOk ? 'ok' : 'fail']">{{ testMessage }}</p>

    <footer class="ai-settings-foot">
      <button class="ai-btn" :disabled="testing" @click="runTest">
        <PlugZap :size="14" :class="{ spinning: testing }" />
        {{ testing ? t('aiSettings.testing') : t('aiSettings.test') }}
      </button>
      <span class="ai-foot-spacer"></span>
      <button class="ai-btn" @click="emit('close')">{{ t('common.cancel') }}</button>
      <button class="ai-btn primary" :disabled="saving" @click="save">
        {{ saving ? t('aiSettings.saving') : t('common.save') }}
      </button>
    </footer>
  </div>
</template>

<script setup lang="ts">
/**
 * BYOK 设置页（V3-2.1）：OpenAI 兼容端点 + API Key + 模型名，带连通性测试。
 * Key 存系统密钥环（沿用连接密码方案），端点请求直连用户配置，不经中间服务。
 */
import { onMounted, ref } from 'vue'
import { PlugZap, Bot, X } from 'lucide-vue-next'
import { invoke } from '@tauri-apps/api/core'
import { globalToast } from '@/composables/useToast'
import { t } from '@/i18n'

const emit = defineEmits<{ close: [] }>()

const baseUrl = ref('')
const model = ref('')
const apiKey = ref('')
const hasKey = ref(false)

const testing = ref(false)
const saving = ref(false)
const testOk = ref(false)
const testMessage = ref('')

interface AiConfigView {
  base_url: string
  model: string
  has_key: boolean
}

onMounted(async () => {
  try {
    const cfg = await invoke<AiConfigView>('ai_get_config')
    baseUrl.value = cfg.base_url
    model.value = cfg.model
    hasKey.value = cfg.has_key
  } catch (e) {
    testMessage.value = String(e)
    testOk.value = false
  }
})

async function runTest() {
  testing.value = true
  testMessage.value = ''
  try {
    const models = await invoke<string[]>('ai_test_connection', {
      baseUrl: baseUrl.value,
      apiKey: apiKey.value.trim() || null,
    })
    testOk.value = true
    const preview = models.slice(0, 5).join(', ')
    testMessage.value = t('aiSettings.testOk', { count: models.length, preview })
  } catch (e) {
    testOk.value = false
    testMessage.value = String(e)
  } finally {
    testing.value = false
  }
}

async function save() {
  saving.value = true
  try {
    await invoke('ai_save_config', {
      baseUrl: baseUrl.value,
      model: model.value,
      apiKey: apiKey.value.trim() || null,
    })
    globalToast.success(t('aiSettings.saved'))
    emit('close')
  } catch (e) {
    testOk.value = false
    testMessage.value = String(e)
  } finally {
    saving.value = false
  }
}
</script>

<style scoped>
.ai-settings {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  min-height: 0;
  /* modal-content 不带内边距，各弹窗自行撑开 */
  padding: var(--space-4) var(--space-5);
}

.ai-settings-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.ai-settings-head h2 {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin: 0;
  font-size: var(--text-lg);
  color: var(--text-primary);
}

.head-icon {
  color: var(--brand-500);
}

.ai-settings-close {
  display: flex;
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  padding: 4px;
  border-radius: var(--radius-sm);
}

.ai-settings-close:hover {
  color: var(--text-primary);
  background: var(--surface-hover);
}

.ai-settings-desc {
  margin: 0;
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  line-height: 1.6;
}

.ai-settings-form {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.ai-field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

.ai-field > span:first-child {
  font-weight: var(--font-medium);
}

.ai-field input {
  padding: 8px 10px;
  border: 1px solid var(--hairline);
  border-radius: var(--radius-sm);
  background: var(--panel);
  color: var(--text-primary);
  font-size: var(--text-sm);
  transition: border-color var(--duration-fast, 0.15s) ease, box-shadow var(--duration-fast, 0.15s) ease;
}

.ai-field input::placeholder {
  color: var(--text-tertiary);
}

.ai-field input:focus {
  outline: none;
  border-color: var(--brand-500);
  box-shadow: 0 0 0 3px rgba(76, 154, 255, 0.15);
}

.ai-field-hint {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.ai-test-msg {
  margin: 0;
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-sm);
  font-size: var(--text-xs);
  line-height: 1.6;
  word-break: break-all;
  max-height: 120px;
  overflow: auto;
}

.ai-test-msg.ok {
  background: rgba(52, 211, 153, 0.12);
  color: var(--success, #059669);
}

.ai-test-msg.fail {
  background: rgba(229, 72, 77, 0.1);
  color: var(--danger, #e5484d);
}

.ai-settings-foot {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin-top: var(--space-2);
  padding-top: var(--space-3);
  border-top: 1px solid var(--hairline);
}

.ai-foot-spacer {
  flex: 1;
}

.ai-btn {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 6px var(--space-3);
  border: 1px solid var(--hairline);
  border-radius: var(--radius-sm);
  background: var(--panel);
  color: var(--text-secondary);
  font-size: var(--text-sm);
  cursor: pointer;
  transition: background var(--duration-fast, 0.15s) ease, border-color var(--duration-fast, 0.15s) ease;
}

.ai-btn:hover:not(:disabled) {
  background: var(--surface-hover);
}

.ai-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.ai-btn.primary {
  background: var(--brand-500);
  color: #fff;
  border-color: var(--brand-500);
}

.ai-btn.primary:hover:not(:disabled) {
  background: var(--brand-600);
  border-color: var(--brand-600);
}

.spinning {
  animation: ai-spin 1s linear infinite;
}

@keyframes ai-spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}
</style>
