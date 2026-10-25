import { ref, computed } from 'vue'
import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { globalToast } from '@/composables/useToast'
import { t } from '@/i18n'

/** 专业版能力键，与后端 `core/license.rs` 中的常量保持一致 */
export type ProFeature = 'schema-diff' | 'monitor' | 'audit' | 'import-export' | 'ssh-tunnel'

interface LicenseStatus {
  activated: boolean
  edition: 'free' | 'pro'
  fingerprint: string
  license_id?: string
  issued_at?: string
  expires_at?: string
  features: string[]
  error?: string
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
  const edition = computed<'free' | 'pro'>(() => (activated.value ? 'pro' : 'free'))
  const fingerprint = computed(() => status.value?.fingerprint ?? '')

  /** 当前授权是否包含指定专业版能力 */
  function allows(feature: ProFeature): boolean {
    const current = status.value
    if (!current?.activated || current.edition !== 'pro') return false
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

  /** 停用本机激活码，回到免费版 */
  async function deactivate(): Promise<boolean> {
    if (!isTauri()) return false
    if (!confirm(t('license.deactivateConfirm'))) return false

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
    allows,
    openDialog,
    closeDialog,
    loadStatus,
    requirePro,
    handleError,
    activate,
    deactivate,
  }
})
