<template>
  <div class="license-dialog">
    <div class="dialog-header">
      <div class="header-title">
        <KeyRound :size="18" class="title-icon" />
        <h2>{{ t('license.title') }}</h2>
        <span class="edition-badge" :class="store.activated ? 'pro' : 'free'">
          {{ store.activated ? t('license.pro') : t('license.free') }}
        </span>
      </div>
      <button class="icon-btn" :title="t('common.close')" @click="emit('close')">
        <X :size="18" />
      </button>
    </div>

    <div class="dialog-body">
      <p class="subtitle">{{ t('license.subtitle') }}</p>

      <!-- 已保存的激活码校验失败（过期、换机、损坏） -->
      <div v-if="store.status?.error" class="alert alert-warning">
        <TriangleAlert :size="16" />
        <span>{{ t('license.error') }}：{{ store.status.error }}</span>
      </div>

      <!-- 机器码：申请激活码时提供给发行方 -->
      <section class="section">
        <div class="section-label">{{ t('license.machineCode') }}</div>
        <div class="fingerprint-row">
          <code class="fingerprint">{{ store.fingerprint || '—' }}</code>
          <button class="copy-btn" :title="t('license.copyMachineCode')" @click="copyFingerprint">
            <component :is="copied ? Check : Copy" :size="14" />
          </button>
        </div>
        <p class="hint">{{ t('license.machineCodeHint') }}</p>
      </section>

      <!-- 已激活：展示授权详情 -->
      <template v-if="store.activated">
        <section class="section">
          <div class="detail-grid">
            <div class="detail-item">
              <span class="detail-label">{{ t('license.licenseId') }}</span>
              <span class="detail-value">{{ store.status?.license_id || '—' }}</span>
            </div>
            <div class="detail-item">
              <span class="detail-label">{{ t('license.expiresAt') }}</span>
              <span class="detail-value">
                {{ store.status?.expires_at ? formatDate(store.status.expires_at) : t('license.permanent') }}
              </span>
            </div>
            <div class="detail-item">
              <span class="detail-label">{{ t('license.issuedAt') }}</span>
              <span class="detail-value">
                {{ store.status?.issued_at ? formatDate(store.status.issued_at) : '—' }}
              </span>
            </div>
          </div>
        </section>

        <section class="section">
          <div class="section-label">{{ t('license.featuresTitle') }}</div>
          <div class="feature-list">
            <span v-for="feature in proFeatures" :key="feature" class="feature-chip">
              {{ t(`license.feature.${feature}`) }}
            </span>
          </div>
          <p class="hint success-hint">
            <BadgeCheck :size="14" /> {{ t('license.activatedHint') }}
          </p>
        </section>
      </template>

      <!-- 未激活：输入激活码 -->
      <template v-else>
        <p class="hint free-hint">{{ t('license.freeHint') }}</p>

        <section class="section">
          <label class="section-label" for="license-code">{{ t('license.code') }}</label>
          <textarea
            id="license-code"
            v-model="code"
            class="code-input"
            rows="3"
            spellcheck="false"
            :placeholder="t('license.codePlaceholder')"
            :disabled="store.loading"
          ></textarea>
          <p class="hint">{{ t('license.applyHint') }}</p>
        </section>

        <div v-if="store.error" class="alert alert-error">
          <CircleAlert :size="16" />
          <span>{{ store.error }}</span>
        </div>
      </template>
    </div>

    <div class="dialog-footer">
      <template v-if="store.activated">
        <Button variant="danger" :loading="store.loading" @click="store.deactivate()">
          {{ t('license.deactivate') }}
        </Button>
        <Button variant="secondary" @click="emit('close')">{{ t('common.close') }}</Button>
      </template>
      <template v-else>
        <Button variant="secondary" @click="emit('close')">{{ t('common.cancel') }}</Button>
        <Button variant="primary" :loading="store.loading" @click="submit">
          {{ store.loading ? t('license.activating') : t('license.activate') }}
        </Button>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { KeyRound, X, Copy, Check, TriangleAlert, CircleAlert, BadgeCheck } from 'lucide-vue-next'
import Button from '@/components/ui/Button.vue'
import { useLicenseStore, type ProFeature } from '@/stores/license'
import { globalToast } from '@/composables/useToast'
import { t } from '@/i18n'

const emit = defineEmits<{ close: [] }>()

const store = useLicenseStore()
const code = ref('')
const copied = ref(false)

const proFeatures: ProFeature[] = [
  'schema-diff',
  'monitor',
  'audit',
  'import-export',
  'ssh-tunnel',
]

onMounted(() => {
  store.error = ''
  void store.loadStatus()
})

async function copyFingerprint() {
  if (!store.fingerprint) return
  try {
    await navigator.clipboard.writeText(store.fingerprint)
    copied.value = true
    globalToast.success(t('license.machineCodeCopied'))
    setTimeout(() => {
      copied.value = false
    }, 1500)
  } catch {
    globalToast.error(t('license.copyFailed'))
  }
}

async function submit() {
  const ok = await store.activate(code.value)
  if (ok) code.value = ''
}

/** 只展示到日期，避免不同时区显示差异 */
function formatDate(value: string): string {
  return value.slice(0, 10)
}
</script>

<style scoped>
.license-dialog {
  display: flex;
  flex-direction: column;
}

.dialog-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-4) var(--space-5);
  border-bottom: 1px solid var(--border-default);
}

.header-title {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.title-icon {
  color: var(--brand-500);
}

.header-title h2 {
  margin: 0;
  font-size: var(--text-lg);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.edition-badge {
  padding: 2px var(--space-2);
  border-radius: var(--radius-full, 999px);
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
}

.edition-badge.pro {
  background: rgba(33, 150, 243, 0.12);
  color: var(--brand-500);
}

.edition-badge.free {
  background: var(--surface-hover);
  color: var(--text-secondary);
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
  transition: var(--transition-colors);
}

.icon-btn:hover {
  background: var(--surface-hover);
  color: var(--text-primary);
}

.dialog-body {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  padding: var(--space-5);
}

.subtitle {
  margin: 0;
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

.section {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.section-label {
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  color: var(--text-secondary);
  text-transform: none;
}

.fingerprint-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.fingerprint {
  flex: 1;
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  background: var(--surface-hover);
  font-family: var(--font-mono, monospace);
  font-size: var(--text-sm);
  letter-spacing: 0.08em;
  color: var(--text-primary);
}

.copy-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  background: var(--surface-default);
  color: var(--text-secondary);
  cursor: pointer;
  transition: var(--transition-colors);
}

.copy-btn:hover {
  border-color: var(--brand-500);
  color: var(--brand-500);
}

.hint {
  margin: 0;
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  line-height: 1.5;
}

.free-hint {
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-md);
  background: var(--surface-hover);
}

.success-hint {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  color: var(--success-500, #2e7d32);
}

.code-input {
  width: 100%;
  resize: vertical;
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  background: var(--surface-default);
  color: var(--text-primary);
  font-family: var(--font-mono, monospace);
  font-size: var(--text-xs);
  line-height: 1.6;
}

.code-input:focus {
  outline: none;
  border-color: var(--brand-500);
}

.alert {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-radius: var(--radius-md);
  font-size: var(--text-xs);
  line-height: 1.5;
}

.alert svg {
  flex-shrink: 0;
  margin-top: 1px;
}

.alert-error {
  background: rgba(211, 47, 47, 0.1);
  color: var(--error-600, #c62828);
}

.alert-warning {
  background: rgba(237, 108, 2, 0.1);
  color: var(--warning-600, #e65100);
}

.detail-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-2) var(--space-4);
}

.detail-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.detail-label {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.detail-value {
  font-size: var(--text-sm);
  color: var(--text-primary);
  word-break: break-all;
}

.feature-list {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
}

.feature-chip {
  padding: 2px var(--space-2);
  border-radius: var(--radius-full, 999px);
  background: rgba(33, 150, 243, 0.1);
  color: var(--brand-500);
  font-size: var(--text-xs);
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
  padding: var(--space-3) var(--space-5);
  border-top: 1px solid var(--border-default);
}
</style>
