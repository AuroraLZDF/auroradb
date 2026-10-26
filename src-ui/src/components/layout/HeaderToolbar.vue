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
        <button
          class="header-btn"
          @click="emit('openNewConsoleTab')"
          :title="t('header.console')"
          :disabled="!currentConnection"
        >
          <Terminal class="header-btn-icon" :size="16" />
          <span class="header-btn-label u-hidden-mobile">{{ t('header.console') }}</span>
        </button>
        <button
          class="header-btn"
          @click="emit('openSchemaDiff')"
          :title="t('header.schemaDiffHint')"
          :disabled="connStore.activeConnections.length === 0"
        >
          <GitCompare class="header-btn-icon" :size="16" />
          <span class="header-btn-label u-hidden-mobile">{{ t('header.schemaDiff') }}</span>
        </button>
        <button
          class="header-btn"
          @click="emit('openErDiagram')"
          :title="t('header.erDiagramHint')"
          :disabled="connStore.activeConnections.length === 0 || !licenseStore.allows('er-diagram')"
        >
          <Network class="header-btn-icon" :size="16" />
          <span class="header-btn-label u-hidden-mobile">{{ t('header.erDiagram') }}</span>
        </button>
        <button
          class="header-btn"
          @click="emit('openMonitor')"
          :title="t('header.monitorHint')"
          :disabled="connStore.activeConnections.length === 0"
        >
          <Activity class="header-btn-icon" :size="16" />
          <span class="header-btn-label u-hidden-mobile">{{ t('header.monitor') }}</span>
        </button>
        <button
          class="header-btn"
          @click="emit('openBackup')"
          :title="t('header.backupHint')"
          :disabled="connStore.activeConnections.length === 0 || !licenseStore.allows('backup')"
        >
          <HardDrive class="header-btn-icon" :size="16" />
          <span class="header-btn-label u-hidden-mobile">{{ t('header.backup') }}</span>
        </button>
        <button
          class="header-btn"
          :class="{ 'license-active': licenseStore.activated }"
          @click="emit('openLicense')"
          :title="t('header.licenseHint')"
        >
          <KeyRound class="header-btn-icon" :size="16" />
          <span class="header-btn-label u-hidden-mobile">
            {{ licenseStore.activated ? t('header.licensePro') : t('header.license') }}
          </span>
        </button>
      </div>
      <button
        class="header-btn update-btn"
        @click="handleUpdateClick"
        :title="updateStore.busy ? updateStore.label : t('header.checkUpdate')"
        :disabled="updateStore.busy"
      >
        <RefreshCw
          class="header-btn-icon"
          :class="{ spinning: updateStore.busy }"
          :size="16"
        />
        <span class="header-btn-label u-hidden-mobile">{{ updateStore.label }}</span>
      </button>
      <button
        class="header-btn lang-btn"
        @click="toggleLocale"
        :title="t('header.switchLanguage')"
      >
        <Languages class="header-btn-icon" :size="16" />
        <span class="header-btn-label u-hidden-mobile">{{ languageLabel }}</span>
      </button>
      <button
        class="header-btn theme-btn"
        @click="themeStore.toggleTheme"
        :title="themeTooltip"
      >
        <component :is="themeIcon" class="header-btn-icon" :size="16" />
        <span class="header-btn-label u-hidden-mobile">{{ t('header.switchTheme') }}</span>
      </button>
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
import { computed } from 'vue'
import { useThemeStore } from '@/stores/theme'
import { useConnectionStore } from '@/stores/connection'
import { useUpdaterStore } from '@/stores/updater'
import { useLicenseStore } from '@/stores/license'
import { t, locale, toggleLocale } from '@/i18n'
import { Menu, Plug, FileCode, Moon, Sun, PanelRight, Terminal, RefreshCw, GitCompare, Activity, Languages, KeyRound, Network, HardDrive } from 'lucide-vue-next'

const themeStore = useThemeStore()
const connStore = useConnectionStore()
const updateStore = useUpdaterStore()
const licenseStore = useLicenseStore()

const currentConnection = computed(() => connStore.currentConnection)

const themeIcon = computed(() => {
  return themeStore.effectiveTheme === 'dark' ? Moon : Sun
})

const themeTooltip = computed(() => {
  return themeStore.effectiveTheme === 'dark' ? t('header.themeDark') : t('header.themeLight')
})

// 按钮上显示「切换到另一种语言」的简称，避免用户看不出可点击
const languageLabel = computed(() => (locale.value === 'zh-CN' ? 'EN' : '中文'))

// 已发现更新则直接下载安装，否则先检查；下载前由用户确认
async function handleUpdateClick() {
  if (updateStore.busy) return
  if (updateStore.availableVersion) {
    await updateStore.downloadAndInstall()
    return
  }
  const version = await updateStore.checkForUpdate()
  if (version && confirm(t('header.updateConfirm', { version }))) {
    await updateStore.downloadAndInstall()
  }
}

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
  openLicense: []
}>()
</script>

<style scoped>
.app-header {
  height: var(--header-height);
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
}

.header-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex: 1;
  gap: var(--space-2);
  margin-left: var(--space-4);
}

.toolbar-left {
  display: flex;
  gap: var(--space-2);
}

.update-btn {
  margin-left: auto;
}

.theme-btn {
  margin-left: 0;
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

/* 已激活：图标与文字用品牌色，与「激活」入口区分 */
.header-btn.license-active {
  border-color: var(--brand-500);
  color: var(--brand-500);
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

.menu-toggle {
  margin-right: 0;
}

.panel-toggle {
  margin-left: 0;
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
