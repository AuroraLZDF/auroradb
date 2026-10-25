import { ref, computed } from 'vue'
import { defineStore } from 'pinia'
import { check, type Update } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'
import { globalToast } from '@/composables/useToast'
import { t } from '@/i18n'

/** 检查更新流程的状态，用于驱动顶部入口按钮的文案与禁用态 */
type UpdaterStatus = 'idle' | 'checking' | 'downloading' | 'installing'

function isTauri(): boolean {
  return '__TAURI_INTERNALS__' in window
}

export const useUpdaterStore = defineStore('updater', () => {
  const status = ref<UpdaterStatus>('idle')
  /** 待安装的新版本号，空串表示当前没有已发现的更新 */
  const availableVersion = ref('')
  /** 已下载字节数与总字节数（总大小未知时为 null） */
  const downloaded = ref(0)
  const totalBytes = ref<number | null>(null)

  const busy = computed(() => status.value !== 'idle')

  const progress = computed(() => {
    if (status.value !== 'downloading' || !totalBytes.value) return null
    return Math.min(100, Math.round((downloaded.value / totalBytes.value) * 100))
  })

  /** 供按钮展示的文案 */
  const label = computed(() => {
    switch (status.value) {
      case 'checking':
        return t('store.checkingUpdate')
      case 'downloading':
        return progress.value === null
          ? t('store.downloading')
          : t('store.downloadingProgress', { progress: progress.value })
      case 'installing':
        return t('store.installing')
      default:
        return t('store.checkUpdate')
    }
  })

  let pending: Update | null = null

  /** 检查是否有新版本；有更新时返回版本号，否则返回 null */
  async function checkForUpdate(): Promise<string | null> {
    if (!isTauri()) {
      globalToast.warning(t('store.desktopOnly'))
      return null
    }
    if (busy.value) return null

    status.value = 'checking'
    try {
      const update = await check()
      if (!update) {
        globalToast.success(t('store.upToDate'))
        return null
      }
      // 重复检查时释放上一次的远端资源，避免句柄泄漏
      if (pending) await pending.close()
      pending = update
      availableVersion.value = update.version
      globalToast.info(t('store.updateAvailable', { version: update.version }))
      return update.version
    } catch (error) {
      globalToast.error(
        t('store.checkFailed', {
          message: error instanceof Error ? error.message : String(error),
        }),
      )
      return null
    } finally {
      status.value = 'idle'
    }
  }

  /** 下载并安装已发现的更新；安装完成后调用方应提示重启 */
  async function downloadAndInstall(): Promise<boolean> {
    if (!pending) {
      globalToast.warning(t('store.checkFirst'))
      return false
    }

    downloaded.value = 0
    totalBytes.value = null
    status.value = 'downloading'
    try {
      await pending.downloadAndInstall((event) => {
        switch (event.event) {
          case 'Started':
            totalBytes.value = event.data.contentLength ?? null
            break
          case 'Progress':
            downloaded.value += event.data.chunkLength
            break
          case 'Finished':
            status.value = 'installing'
            break
        }
      })
      // 安装完成后必须重启，新版本才会生效
      globalToast.success(t('store.updateInstalled'))
      await relaunch()
      return true
    } catch (error) {
      status.value = 'idle'
      globalToast.error(
        t('store.updateFailed', {
          message: error instanceof Error ? error.message : String(error),
        }),
      )
      return false
    }
  }

  return {
    status,
    busy,
    progress,
    label,
    availableVersion,
    checkForUpdate,
    downloadAndInstall,
  }
})
