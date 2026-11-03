import { ref, computed } from 'vue'
import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { globalToast } from '@/composables/useToast'
import { confirmDanger } from '@/composables/useConfirm'
import { t } from '@/i18n'

/** 专业版能力键，与后端 `core/license.rs` 中的常量保持一致 */
export type ProFeature = 'schema-diff' | 'monitor' | 'audit' | 'ssh-tunnel' | 'query-plan' | 'er-diagram' | 'backup' | 'global-search'

interface LicenseStatus {
  activated: boolean
  edition: 'free' | 'pro' | 'trial'
  fingerprint: string
  license_id?: string
  issued_at?: string
  expires_at?: string
  features: string[]
  error?: string
  /** 本机是否还能开启试用（未试用过且未过期）；后端旧版本不返回时为 undefined，按 false 处理 */
  trial_available?: boolean
  /** 当前是否处于试用中，与 `edition === 'trial'` 语义一致 */
  trial_active?: boolean
  /** 试用是否已结束（结束后 edition 回到 free） */
  trial_expired?: boolean
  trial_started_at?: string
  /** 试用到期时间，展示时只取日期部分 */
  trial_expires_at?: string
  /** 试用剩余天数，由后端按自然日计算 */
  trial_days_left?: number
}

/** 后端拒绝专业版操作时使用的错误前缀 */
const LICENSE_REQUIRED_PREFIX = 'LICENSE_REQUIRED:'

function isTauri(): boolean {
  return '__TAURI_INTERNALS__' in window
}

export const useLicenseStore = defineStore('license', () => {
  const status = ref<LicenseStatus | null>(null)
  /** 激活窗口是否可见；由 App.vue 渲染 */
  const dialogVisible = ref(false)
  const loading = ref(false)
  /** 激活/停用失败的原因，展示在激活窗口内 */
  const error = ref('')

  const activated = computed(() => status.value?.activated === true)
  /** 当前版本：后端用 free / pro / trial 区分；未取到状态时按免费版处理 */
  const edition = computed<'free' | 'pro' | 'trial'>(() => status.value?.edition ?? 'free')
  const fingerprint = computed(() => status.value?.fingerprint ?? '')

  /** 是否处于 14 天试用期（试用版解锁全部专业版能力） */
  const isTrial = computed(() => edition.value === 'trial')
  /** 本机是否还能自助开启试用（试用中/已用尽/记录不可用时均为 false） */
  const trialAvailable = computed(() => !isTrial.value && status.value?.trial_available === true)
  /** 试用是否已结束，用于免费版界面提示购买激活码 */
  const trialExpired = computed(() => status.value?.trial_expired === true)
  /** 试用剩余天数；后端字段缺失或为负时归零，避免界面出现「剩余 -1 天」 */
  const trialDaysLeft = computed(() => Math.max(0, status.value?.trial_days_left ?? 0))

  /** 当前授权是否包含指定专业版能力 */
  function allows(feature: ProFeature): boolean {
    const current = status.value
    // 专业版与试用版都能解锁全部专业版能力
    if (!current?.activated || (current.edition !== 'pro' && current.edition !== 'trial')) return false
    // features 为空表示包含全部专业版能力
    return (
      current.features.length === 0 ||
      current.features.includes('*') ||
      current.features.includes(feature)
    )
  }

  function openDialog() {
    error.value = ''
    dialogVisible.value = true
    void loadStatus()
  }

  function closeDialog() {
    dialogVisible.value = false
    error.value = ''
  }

  /** 读取并缓存授权状态（启动时与打开激活窗口时调用） */
  async function loadStatus() {
    if (!isTauri()) return
    try {
      status.value = await invoke<LicenseStatus>('get_license_status')
    } catch (e) {
      console.error('Failed to load license status:', e)
    }
  }

  /**
   * 前端入口的统一门禁：无权限时弹出激活窗口并提示。
   * 返回 false 表示调用方应终止当前操作。
   */
  function requirePro(feature: ProFeature, label: string): boolean {
    if (allows(feature)) return true
    openDialog()
    globalToast.warning(t('license.required', { feature: label }))
    return false
  }

  /**
   * 识别后端返回的 `LICENSE_REQUIRED:` 错误：弹出激活窗口并返回 true。
   * 用于兜底的场景（例如运行中授权过期、前端未拦截的调用）。
   */
  function handleError(e: unknown): boolean {
    const message = String(e ?? '')
    if (!message.includes(LICENSE_REQUIRED_PREFIX)) return false
    openDialog()
    return true
  }

  /** 激活成功返回 true；失败时把原因写入 error 供窗口内展示 */
  async function activate(code: string): Promise<boolean> {
    if (!isTauri()) {
      error.value = t('license.desktopOnly')
      return false
    }
    const trimmed = code.trim()
    if (!trimmed) {
      error.value = t('license.codeRequired')
      return false
    }

    loading.value = true
    error.value = ''
    try {
      status.value = await invoke<LicenseStatus>('activate_license', { code: trimmed })
      globalToast.success(t('license.activateSuccess'))
      dialogVisible.value = false
      return true
    } catch (e) {
      error.value = String(e)
      return false
    } finally {
      loading.value = false
    }
  }

  /**
   * 开启 14 天免费试用：成功返回 true 并缓存后端返回的最新状态。
   * 后端按机器码记录试用，每台机器只能开启一次。
   */
  async function startTrial(): Promise<boolean> {
    if (!isTauri()) {
      error.value = t('license.desktopOnly')
      return false
    }

    loading.value = true
    error.value = ''
    try {
      status.value = await invoke<LicenseStatus>('start_trial')
      globalToast.success(t('license.trialStarted'))
      return true
    } catch (e) {
      error.value = String(e)
      globalToast.error(t('license.trialStartFailed', { error: String(e) }))
      return false
    } finally {
      loading.value = false
    }
  }

  /** 停用本机激活码，回到免费版 */
  async function deactivate(): Promise<boolean> {
    if (!isTauri()) return false
    if (
      !(await confirmDanger({
        message: t('license.deactivateConfirm'),
        confirmText: t('license.deactivate'),
      }))
    )
      return false

    loading.value = true
    error.value = ''
    try {
      await invoke('deactivate_license')
      await loadStatus()
      globalToast.success(t('license.deactivateSuccess'))
      return true
    } catch (e) {
      error.value = String(e)
      globalToast.error(t('license.deactivateFailed', { error: String(e) }))
      return false
    } finally {
      loading.value = false
    }
  }

  return {
    status,
    dialogVisible,
    loading,
    error,
    activated,
    edition,
    fingerprint,
    isTrial,
    trialAvailable,
    trialExpired,
    trialDaysLeft,
    allows,
    openDialog,
    closeDialog,
    loadStatus,
    requirePro,
    handleError,
    activate,
    startTrial,
    deactivate,
  }
})
