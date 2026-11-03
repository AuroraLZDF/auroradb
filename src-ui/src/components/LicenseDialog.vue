<template>
  <div class="license-dialog">
    <div class="dialog-header">
      <div class="header-title">
        <KeyRound :size="18" class="title-icon" />
        <h2>{{ t('license.title') }}</h2>
        <span class="edition-badge" :class="store.edition">
          {{ editionLabel }}
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

      <!-- 试用中：剩余天数与到期日期 -->
      <section v-if="store.isTrial" class="section trial-active">
        <div class="trial-summary">
          <span class="trial-days">{{ t('license.trialRemaining', { days: store.trialDaysLeft }) }}</span>
          <span v-if="trialExpiresAt" class="trial-expires">
            {{ t('license.trialExpiresAt') }} {{ formatDate(trialExpiresAt) }}
          </span>
        </div>
        <p class="hint">{{ t('license.trialActiveHint') }}</p>
      </section>

      <!-- 已激活专业版：展示授权详情（试用版不展示正式授权的签发/到期字段） -->
      <template v-if="store.activated && !store.isTrial">
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
            <div v-for="item in proFeatures" :key="item.key" class="feature-item">
              <component :is="item.icon" :size="16" class="feature-icon" />
              <div class="feature-text">
                <span class="feature-name">{{ t(`license.feature.${item.key}`) }}</span>
                <span class="feature-desc">{{ t(`license.featureDesc.${item.key}`) }}</span>
              </div>
            </div>
          </div>
          <p class="hint success-hint">
            <BadgeCheck :size="14" /> {{ t('license.activatedHint') }}
          </p>
        </section>
      </template>

      <!-- 免费版：自助试用入口 / 试用过期提示 / 试用记录不可用 -->
      <template v-if="!store.activated">
        <p class="hint free-hint">{{ t('license.freeHint') }}</p>

        <!-- 本机尚未试用：展示自助开启 14 天试用卡片 -->
        <section v-if="store.trialAvailable" class="section trial-card">
          <div class="trial-card-title">
            <Sparkles :size="16" class="trial-card-icon" />
            <span>{{ t('license.trialTitle') }}</span>
          </div>
          <p class="trial-card-desc">{{ t('license.trialDesc') }}</p>
          <p class="hint">{{ t('license.trialNoCard') }}</p>
          <Button variant="primary" :loading="store.loading" @click="startTrial">
            {{ store.loading ? t('license.trialStarting') : t('license.trialStart') }}
          </Button>
        </section>

        <!-- 试用已结束：提示购买激活码 -->
        <div v-else-if="store.trialExpired" class="alert alert-warning">
          <TriangleAlert :size="16" />
          <span>{{ t('license.trialExpiredHint') }}</span>
        </div>

        <!-- 试用记录不可用（已用过或记录损坏）：只提示购买 -->
        <p v-else class="hint">{{ t('license.trialUsed') }}</p>
      </template>

      <!-- 试用中与免费版都保留激活码输入（购买后输入激活码继续使用） -->
      <template v-if="!store.activated || store.isTrial">
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
          <p class="hint">{{ store.isTrial ? t('license.buyHint') : t('license.applyHint') }}</p>
        </section>

        <div v-if="store.error" class="alert alert-error">
          <CircleAlert :size="16" />
          <span>{{ store.error }}</span>
        </div>
      </template>
    </div>

    <div class="dialog-footer">
      <template v-if="store.activated && !store.isTrial">
        <Button variant="danger" :loading="store.loading" @click="store.deactivate()">
          {{ t('license.deactivate') }}
        </Button>
        <Button variant="secondary" @click="emit('close')">{{ t('common.close') }}</Button>
      </template>
      <template v-else>
        <Button variant="secondary" @click="emit('close')">
          {{ store.isTrial ? t('common.close') : t('common.cancel') }}
        </Button>
        <Button variant="primary" :loading="activating" @click="submit">
          {{ activating ? t('license.activating') : t('license.activate') }}
        </Button>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref, onMounted, type Component } from 'vue'
import {
  KeyRound,
  X,
  Copy,
  Check,
  TriangleAlert,
  CircleAlert,
  BadgeCheck,
  Sparkles,
  GitCompare,
  GitBranch,
  ListTree,
  Activity,
  ScrollText,
  HardDrive,
  Search,
  Network,
} from 'lucide-vue-next'
import Button from '@/components/ui/Button.vue'
import { useLicenseStore, type ProFeature } from '@/stores/license'
import { globalToast } from '@/composables/useToast'
import { t } from '@/i18n'

const emit = defineEmits<{ close: [] }>()

const store = useLicenseStore()
const code = ref('')
const copied = ref(false)
/** 仅表示「激活码提交中」，避免开启试用的 loading 让底部按钮误显示「激活中」 */
const activating = ref(false)

interface FeatureItem {
  key: ProFeature
  icon: Component
}

const proFeatures: FeatureItem[] = [
  { key: 'schema-diff', icon: GitCompare },
  { key: 'er-diagram', icon: GitBranch },
  { key: 'query-plan', icon: ListTree },
  { key: 'monitor', icon: Activity },
  { key: 'audit', icon: ScrollText },
  { key: 'backup', icon: HardDrive },
  { key: 'global-search', icon: Search },
  { key: 'ssh-tunnel', icon: Network },
]

/** 顶部徽标文案随授权状态变化：专业版 / 试用中 / 免费版 */
const editionLabel = computed(() => {
  if (store.edition === 'pro') return t('license.pro')
  if (store.isTrial) return t('license.trial')
  return t('license.free')
})

/** 试用到期日期：放到 computed 里避免模板中可选链的窄化问题 */
const trialExpiresAt = computed(() => store.status?.trial_expires_at ?? '')

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
  // 试用开启等操作进行中时忽略重复提交，避免并发调用后端
  if (store.loading) return
  activating.value = true
  try {
    const ok = await store.activate(code.value)
    if (ok) code.value = ''
  } finally {
    activating.value = false
  }
}

/** 自助开启试用：成功后清空输入与错误，界面随 store 状态切到「试用中」 */
async function startTrial() {
  if (store.loading) return
  const ok = await store.startTrial()
  if (ok) {
    code.value = ''
    store.error = ''
  }
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

/* 试用中：琥珀色，与专业版的品牌蓝、免费版的中性灰区分开 */
.edition-badge.trial {
  background: var(--warning-100, rgba(245, 158, 11, 0.16));
  color: var(--warning-700, #b45309);
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

/* 试用中：剩余天数 + 到期日期 */
.trial-active {
  padding: var(--space-3);
  border: 1px solid var(--warning-200, rgba(245, 158, 11, 0.35));
  border-radius: var(--radius-md);
  background: var(--warning-50, rgba(245, 158, 11, 0.08));
}

.trial-summary {
  display: flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: var(--space-2) var(--space-4);
}

.trial-days {
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--warning-700, #b45309);
}

.trial-expires {
  font-size: var(--text-xs);
  color: var(--text-secondary);
}

/* 免费版的自助试用卡片 */
.trial-card {
  padding: var(--space-3);
  border: 1px solid var(--warning-200, rgba(245, 158, 11, 0.35));
  border-radius: var(--radius-md);
  background: var(--warning-50, rgba(245, 158, 11, 0.08));
}

.trial-card-title {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--warning-700, #b45309);
}

.trial-card-icon {
  flex-shrink: 0;
  color: var(--warning-500, #f59e0b);
}

.trial-card-desc {
  margin: 0;
  font-size: var(--text-sm);
  color: var(--text-primary);
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
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: var(--space-2);
}

.feature-item {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  transition: border-color 0.15s ease, background-color 0.15s ease, box-shadow 0.15s ease, transform 0.15s ease;
}

.feature-item:hover {
  border-color: var(--brand-500);
  background: rgba(33, 150, 243, 0.06);
  box-shadow: 0 2px 10px rgba(0, 0, 0, 0.1);
  transform: translateY(-1px);
}

.feature-icon {
  flex-shrink: 0;
  margin-top: 1px;
  color: var(--brand-500);
}

.feature-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.feature-name {
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  color: var(--text-primary);
}

.feature-desc {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  line-height: 1.4;
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
  padding: var(--space-3) var(--space-5);
  border-top: 1px solid var(--border-default);
}
</style>
