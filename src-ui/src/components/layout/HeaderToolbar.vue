<template>
  <header class="app-header">
    <!-- 移动端菜单按钮 -->
    <button
      class="header-btn menu-toggle u-visible-mobile"
      @click="emit('toggleSidebar')"
      :title="t('header.toggleSidebar')"
    >
      <Menu :size="20" />
    </button>

    <h1>AuroraDB</h1>

    <!-- 顶部工具栏 -->
    <div class="header-toolbar">
      <div class="toolbar-left">
        <button
          class="header-btn"
          @click="emit('openConnectionForm')"
          :title="t('header.newConnection')"
        >
          <Plug class="header-btn-icon" :size="16" />
          <span class="header-btn-label u-hidden-mobile">{{ t('header.connection') }}</span>
        </button>
        <button
          class="header-btn"
          :class="{ active: activeTool === 'query' }"
          @click="emit('openNewQueryTab')"
          :title="t('header.newQuery')"
          :disabled="!currentConnection"
        >
          <FileCode class="header-btn-icon" :size="16" />
          <span class="header-btn-label u-hidden-mobile">{{ t('header.newQuery') }}</span>
        </button>
        <div class="tools-menu" ref="toolsMenuRef">
          <button
            class="header-btn"
            :class="{ active: toolsOpen }"
            :title="t('header.tools')"
            aria-haspopup="menu"
            :aria-expanded="toolsOpen"
            @click.stop="toggleMenu('tools')"
          >
            <Wrench class="header-btn-icon" :size="16" />
            <span class="header-btn-label u-hidden-mobile">{{ t('header.tools') }}</span>
            <ChevronDown
              class="tools-chevron"
              :class="{ open: toolsOpen }"
              :size="14"
            />
          </button>
          <div v-if="toolsOpen" class="tools-dropdown" role="menu">
            <button
              class="tools-item"
              role="menuitem"
              :title="t('header.schemaDiffHint')"
              :disabled="connStore.activeConnections.length === 0"
              @click="openTool('openSchemaDiff')"
            >
              <GitCompare :size="16" />
              <span>{{ t('header.schemaDiff') }}</span>
            </button>
            <button
              class="tools-item"
              role="menuitem"
              :title="t('header.erDiagramHint')"
              :disabled="connStore.activeConnections.length === 0"
              @click="openTool('openErDiagram')"
            >
              <Network :size="16" />
              <span>{{ t('header.erDiagram') }}</span>
            </button>
            <button
              class="tools-item"
              role="menuitem"
              :title="t('header.monitorHint')"
              :disabled="connStore.activeConnections.length === 0"
              @click="openTool('openMonitor')"
            >
              <Activity :size="16" />
              <span>{{ t('header.monitor') }}</span>
            </button>
            <button
              class="tools-item"
              role="menuitem"
              :title="t('header.backupHint')"
              :disabled="connStore.activeConnections.length === 0"
              @click="openTool('openBackup')"
            >
              <HardDrive :size="16" />
              <span>{{ t('header.backup') }}</span>
            </button>
            <button
              class="tools-item"
              role="menuitem"
              :title="t('header.globalSearchHint')"
              :disabled="connStore.activeConnections.length === 0"
              @click="openTool('openGlobalSearch')"
            >
              <Search :size="16" />
              <span>{{ t('header.globalSearch') }}</span>
            </button>
            <div class="tools-divider"></div>
            <button
              class="tools-item"
              role="menuitem"
              :title="t('header.console')"
              :disabled="!currentConnection"
              @click="openTool('openNewConsoleTab')"
            >
              <Terminal :size="16" />
              <span>{{ t('header.console') }}</span>
            </button>
          </div>
        </div>
        <div class="tools-menu" ref="settingsMenuRef">
          <button
            class="header-btn"
            :class="{ active: settingsOpen }"
            :title="t('header.settings')"
            aria-haspopup="menu"
            :aria-expanded="settingsOpen"
            @click.stop="toggleMenu('settings')"
          >
            <Settings class="header-btn-icon" :size="16" />
            <span class="header-btn-label u-hidden-mobile">{{ t('header.settings') }}</span>
            <ChevronDown
              class="tools-chevron"
              :class="{ open: settingsOpen }"
              :size="14"
            />
          </button>
          <div v-if="settingsOpen" class="tools-dropdown" role="menu">
            <button
              class="tools-item"
              role="menuitem"
              :title="t('header.checkUpdate')"
              :disabled="updateStore.busy"
              @click="handleUpdateClick"
            >
              <RefreshCw :class="{ spinning: updateStore.busy }" :size="16" />
              <span>{{ updateStore.label }}</span>
            </button>
            <button
              class="tools-item"
              role="menuitem"
              :title="t('header.licenseHint')"
              @click="openTool('openLicense')"
            >
              <KeyRound :size="16" />
              <span>{{ licenseLabel }}</span>
            </button>
            <div class="tools-divider"></div>
            <button
              class="tools-item"
              role="menuitem"
              :title="t('header.switchLanguage')"
              @click="closeMenus(); toggleLocale()"
            >
              <Languages :size="16" />
              <span>{{ t('header.switchLanguage') }}</span>
              <span class="tools-item-hint">{{ languageLabel }}</span>
            </button>
            <button
              class="tools-item"
              role="menuitem"
              :title="themeTooltip"
              @click="closeMenus(); themeStore.toggleTheme()"
            >
              <component :is="themeIcon" :size="16" />
              <span>{{ t('header.switchTheme') }}</span>
            </button>
            <div class="tools-divider"></div>
            <button
              class="tools-item"
              role="menuitem"
              :title="t('header.aboutHint')"
              @click="openTool('openAbout')"
            >
              <Info :size="16" />
              <span>{{ t('header.about') }}</span>
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- 更新包下载进度：菜单会因点击而收起，进度必须常驻在头部，否则用户无感知 -->
    <div
      v-if="showUpdateProgress"
      class="update-progress"
      :title="updateStore.label"
      role="progressbar"
      :aria-valuemin="0"
      :aria-valuemax="100"
      :aria-valuenow="updateStore.progress ?? undefined"
    >
      <span class="update-progress-label">{{ updateStore.label }}</span>
      <div
        class="update-progress-track"
        :class="{ indeterminate: updateStore.progress === null }"
      >
        <div
          class="update-progress-bar"
          :style="updateStore.progress === null ? undefined : { width: `${updateStore.progress}%` }"
        />
      </div>
      <span v-if="updateStore.totalBytes" class="update-progress-size u-hidden-mobile">
        {{ formatBytes(updateStore.downloaded) }} / {{ formatBytes(updateStore.totalBytes) }}
      </span>
    </div>

    <!-- 面板切换按钮（平板/小屏） -->
    <button
      class="header-btn panel-toggle u-visible-tablet"
      :class="{ active: infoPanelOpen }"
      @click="emit('togglePanel')"
      :title="t('header.togglePanel')"
    >
      <PanelRight :size="20" />
    </button>
  </header>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useThemeStore } from '@/stores/theme'
import { useConnectionStore } from '@/stores/connection'
import { useUpdaterStore } from '@/stores/updater'
import { useLicenseStore } from '@/stores/license'
import { t, locale, toggleLocale } from '@/i18n'
import { confirmAction } from '@/composables/useConfirm'
import { Menu, Plug, FileCode, Moon, Sun, PanelRight, Terminal, RefreshCw, GitCompare, Activity, Languages, KeyRound, Network, HardDrive, Search, Wrench, Settings, ChevronDown, Info } from 'lucide-vue-next'

const themeStore = useThemeStore()
const connStore = useConnectionStore()
const updateStore = useUpdaterStore()
const licenseStore = useLicenseStore()

interface Props {
  activeTool?: 'connection' | 'query'
  infoPanelOpen?: boolean
}

withDefaults(defineProps<Props>(), {
  activeTool: 'connection',
  infoPanelOpen: true
})

const emit = defineEmits<{
  toggleSidebar: []
  togglePanel: []
  openConnectionForm: []
  openNewQueryTab: []
  openNewConsoleTab: []
  openSchemaDiff: []
  openErDiagram: []
  openMonitor: []
  openBackup: []
  openGlobalSearch: []
  openLicense: []
  openAbout: []
}>()

const currentConnection = computed(() => connStore.currentConnection)

/** 设置菜单里的授权入口文案：试用中 / 专业版 / 免费版（激活） */
const licenseLabel = computed(() => {
  if (licenseStore.isTrial) return t('header.licenseTrial')
  return licenseStore.activated ? t('header.licensePro') : t('header.license')
})

// 「工具」「设置」下拉菜单
const toolsMenuRef = ref<HTMLElement | null>(null)
const settingsMenuRef = ref<HTMLElement | null>(null)
const toolsOpen = ref(false)
const settingsOpen = ref(false)

function toggleMenu(menu: 'tools' | 'settings') {
  if (menu === 'tools') {
    toolsOpen.value = !toolsOpen.value
    settingsOpen.value = false
  } else {
    settingsOpen.value = !settingsOpen.value
    toolsOpen.value = false
  }
}

function closeMenus() {
  toolsOpen.value = false
  settingsOpen.value = false
}

type ToolEvent =
  | 'openSchemaDiff'
  | 'openErDiagram'
  | 'openMonitor'
  | 'openBackup'
  | 'openGlobalSearch'
  | 'openNewConsoleTab'
  | 'openLicense'
  | 'openAbout'

const toolActions: Record<ToolEvent, () => void> = {
  openSchemaDiff: () => emit('openSchemaDiff'),
  openErDiagram: () => emit('openErDiagram'),
  openMonitor: () => emit('openMonitor'),
  openBackup: () => emit('openBackup'),
  openGlobalSearch: () => emit('openGlobalSearch'),
  openNewConsoleTab: () => emit('openNewConsoleTab'),
  openLicense: () => emit('openLicense'),
  openAbout: () => emit('openAbout')
}

function openTool(name: ToolEvent) {
  closeMenus()
  toolActions[name]()
}

function handleClickOutside(e: MouseEvent) {
  const target = e.target as Node
  const inTools = toolsMenuRef.value?.contains(target)
  const inSettings = settingsMenuRef.value?.contains(target)
  if (!inTools && !inSettings) closeMenus()
}

function handleKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') closeMenus()
}

onMounted(() => {
  document.addEventListener('click', handleClickOutside)
  document.addEventListener('keydown', handleKeydown)
})

onUnmounted(() => {
  document.removeEventListener('click', handleClickOutside)
  document.removeEventListener('keydown', handleKeydown)
})

const themeIcon = computed(() => {
  return themeStore.effectiveTheme === 'dark' ? Moon : Sun
})

/** 仅在真正有产物传输/安装时展示进度，检查阶段用菜单里的转圈即可 */
const showUpdateProgress = computed(
  () => updateStore.status === 'downloading' || updateStore.status === 'installing'
)

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

const themeTooltip = computed(() => {
  return themeStore.effectiveTheme === 'dark' ? t('header.themeDark') : t('header.themeLight')
})

// 按钮上显示「切换到另一种语言」的简称，避免用户看不出可点击
const languageLabel = computed(() => (locale.value === 'zh-CN' ? 'EN' : '中文'))

// 已发现更新则直接下载安装，否则先检查；下载前由用户确认
async function handleUpdateClick() {
  closeMenus()
  if (updateStore.busy) return
  if (updateStore.availableVersion) {
    await updateStore.downloadAndInstall()
    return
  }
  const version = await updateStore.checkForUpdate()
  if (
    version &&
    (await confirmAction({
      message:
        updateStore.installKind === 'deb'
          ? t('header.updateDownloadConfirm', { version })
          : t('header.updateConfirm', { version }),
      confirmText: t('common.confirm'),
    }))
  ) {
    await updateStore.downloadAndInstall()
  }
}
</script>

<style scoped>
.app-header {
  height: 48px;
  flex-shrink: 0;
  background: var(--surface-default);
  border-bottom: 1px solid var(--border-default);
  display: flex;
  align-items: center;
  padding: 0 var(--space-4);
  gap: var(--space-3);
  position: relative;
  z-index: 100;
}

.app-header h1 {
  font-size: var(--text-lg);
  font-weight: var(--font-bold);
  color: var(--brand-500);
  margin: 0;
  flex-shrink: 0;
}

.header-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex: 1;
  min-width: 0;
  gap: var(--space-2);
  margin-left: var(--space-4);
}

.toolbar-left {
  display: flex;
  min-width: 0;
  gap: var(--space-2);
}

/* 「工具」下拉菜单 */
.tools-menu {
  position: relative;
  display: flex;
}

.tools-chevron {
  flex-shrink: 0;
  transition: transform 0.15s ease;
}

.tools-chevron.open {
  transform: rotate(180deg);
}

.tools-dropdown {
  position: absolute;
  top: calc(100% + 6px);
  left: 0;
  min-width: 180px;
  padding: var(--space-1);
  display: flex;
  flex-direction: column;
  background: var(--surface-default);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-lg);
  z-index: 200;
}

.tools-item {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--text-sm);
  text-align: left;
  white-space: nowrap;
  cursor: pointer;
  transition: var(--transition-colors);
}

.tools-item:hover:not(:disabled) {
  background: var(--surface-hover);
  color: var(--brand-500);
}

.tools-item:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.tools-divider {
  height: 1px;
  margin: var(--space-1) 0;
  background: var(--border-default);
}

.tools-item-hint {
  margin-left: auto;
  padding-left: var(--space-3);
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.header-btn {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  background: var(--surface-default);
  color: var(--text-secondary);
  font-size: var(--text-sm);
  cursor: pointer;
  transition: var(--transition-colors);
  /* 禁止按钮文案换行：英文文案较长时换行会把导航栏撑高、按钮高度不一致 */
  white-space: nowrap;
  flex-shrink: 0;
}

.header-btn-label {
  white-space: nowrap;
}

.header-btn:hover:not(:disabled) {
  background: var(--surface-hover);
  border-color: var(--brand-500);
  color: var(--brand-500);
}

.header-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.header-btn.active {
  background: var(--brand-500);
  border-color: var(--brand-500);
  color: white;
}

.header-btn-icon {
  flex-shrink: 0;
}

.spinning {
  animation: header-spin 1s linear infinite;
}

@keyframes header-spin {
  to {
    transform: rotate(360deg);
  }
}

/* 更新下载进度：常驻头部，避免菜单收起后进度不可见 */
.update-progress {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-1) var(--space-3);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  background: var(--surface-hover);
  flex-shrink: 0;
}

.update-progress-label {
  font-size: var(--text-xs);
  color: var(--text-secondary);
  white-space: nowrap;
}

.update-progress-track {
  position: relative;
  width: 96px;
  height: 6px;
  border-radius: 999px;
  background: var(--border-default);
  overflow: hidden;
}

.update-progress-bar {
  height: 100%;
  width: 0;
  border-radius: inherit;
  background: var(--brand-500);
  transition: width 0.2s ease;
}

/* 总大小未知时用往复滑动表示「正在进行」，避免停在 0% 像卡死 */
.update-progress-track.indeterminate .update-progress-bar {
  width: 40%;
  animation: update-progress-slide 1.1s ease-in-out infinite;
}

@keyframes update-progress-slide {
  0% {
    transform: translateX(-100%);
  }
  100% {
    transform: translateX(280%);
  }
}

.update-progress-size {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.menu-toggle {
  margin-right: 0;
}

.panel-toggle {
  margin-left: 0;
}

/* 窗口变窄时收起所有按钮文案，只留图标 + tooltip，避免导航栏溢出 */
@media (max-width: 1200px) {
  .header-btn-label {
    display: none !important;
  }
}

@media (max-width: 768px) {
  .u-hidden-mobile {
    display: none !important;
  }
}

@media (min-width: 769px) {
  .u-visible-mobile {
    display: none !important;
  }
}

@media (min-width: 1025px) {
  .u-visible-tablet {
    display: none !important;
  }
}
</style>
