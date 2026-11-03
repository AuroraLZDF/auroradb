import { ref, computed } from 'vue'
import { defineStore } from 'pinia'
import { check, type Update } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { globalToast } from '@/composables/useToast'
import { t } from '@/i18n'

/** 检查更新流程的状态，用于驱动顶部入口按钮的文案与禁用态 */
type UpdaterStatus = 'idle' | 'checking' | 'downloading' | 'installing'

/**
 * 安装形态：`auto` 由内置更新器原地替换（AppImage），
 * `deb` 只能把 deb 安装包下载到本地后由用户手动安装。
 */
type InstallKind = 'auto' | 'deb'

/** 更新包下载进度事件，由 Rust 侧 download_update_package 发送 */
interface PackageProgress {
  downloaded: number
  total: number | null
}

const PROGRESS_EVENT = 'updater://package-progress'

function isTauri(): boolean {
  return '__TAURI_INTERNALS__' in window
}

/** deb 安装包信息：地址与期望的 SHA-256（由 tools/publish/linux.sh 写入顶层 deb 字段） */
interface DebPackage {
  url: string
  /** 清单未提供时为 undefined，下载命令会告警但不阻断（兼容旧清单） */
  sha256?: string
}

/** 从更新清单里取 deb 安装包地址与校验值 */
function debPackage(raw: Record<string, unknown>): DebPackage {
  const deb = raw.deb as { url?: unknown; sha256?: unknown } | undefined
  const sha256 = typeof deb?.sha256 === 'string' ? deb.sha256.trim() : ''
  return {
    url: typeof deb?.url === 'string' ? deb.url : '',
    sha256: sha256 || undefined,
  }
}

/** 从下载地址里取文件名，兼作落盘名 */
function packageFileName(url: string): string {
  const base = url.replace(/[`\s]/g, '').split('/').pop() ?? ''
  try {
    return decodeURIComponent(base) || 'auroradb-update.deb'
  } catch {
    return base || 'auroradb-update.deb'
  }
}

export const useUpdaterStore = defineStore('updater', () => {
  const status = ref<UpdaterStatus>('idle')
  /** 待安装的新版本号，空串表示当前没有已发现的更新 */
  const availableVersion = ref('')
  /** 已下载字节数与总字节数（总大小未知时为 null） */
  const downloaded = ref(0)
  const totalBytes = ref<number | null>(null)
  /** 当前安装形态，决定「下载并安装」走内置更新器还是手动安装包 */
  const installKind = ref<InstallKind>('auto')
  /** deb 更新包地址，仅 installKind 为 deb 时使用 */
  const packageUrl = ref('')
  /** deb 更新包期望的 SHA-256（清单未提供时为空串，下载时不校验） */
  const packageSha256 = ref('')

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
      installKind.value = (await invoke<string>('get_update_install_kind')) === 'deb' ? 'deb' : 'auto'
      const update = await check()
      if (!update) {
        globalToast.success(t('store.upToDate'))
        return null
      }
      // 重复检查时释放上一次的远端资源，避免句柄泄漏
      if (pending) await pending.close()
      pending = update
      availableVersion.value = update.version
      packageUrl.value = ''
      packageSha256.value = ''

      if (installKind.value === 'deb') {
        const pkg = debPackage(update.rawJson)
        // deb 走手动安装，不保留内置更新器的资源
        await update.close()
        pending = null
        if (!pkg.url) {
          availableVersion.value = ''
          globalToast.warning(t('store.debPackageMissing'))
          return null
        }
        packageUrl.value = pkg.url
        packageSha256.value = pkg.sha256 ?? ''
      }

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
    if (installKind.value === 'deb') return downloadPackage()

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

  /** deb 安装：把更新包下载到系统下载目录，提示用户手动安装 */
  async function downloadPackage(): Promise<boolean> {
    if (!packageUrl.value) {
      globalToast.warning(t('store.checkFirst'))
      return false
    }

    downloaded.value = 0
    totalBytes.value = null
    status.value = 'downloading'
    const unlisten = await listen<PackageProgress>(PROGRESS_EVENT, ({ payload }) => {
      downloaded.value = payload.downloaded
      totalBytes.value = payload.total
    })
    try {
      const path = await invoke<string>('download_update_package', {
        url: packageUrl.value,
        fileName: packageFileName(packageUrl.value),
        // 清单未提供校验值时不传，Rust 侧会记录告警但不阻断
        sha256: packageSha256.value || undefined,
      })
      globalToast.success(t('store.updatePackageDownloaded', { path }))
      // 安装包已落盘，清空待更新状态，下次点击重新检查版本
      availableVersion.value = ''
      packageUrl.value = ''
      packageSha256.value = ''
      return true
    } catch (error) {
      globalToast.error(
        t('store.updateFailed', {
          message: error instanceof Error ? error.message : String(error),
        }),
      )
      return false
    } finally {
      unlisten()
      status.value = 'idle'
    }
  }

  return {
    status,
    busy,
    progress,
    label,
    availableVersion,
    installKind,
    downloaded,
    totalBytes,
    checkForUpdate,
    downloadAndInstall,
  }
})
