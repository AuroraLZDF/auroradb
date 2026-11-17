<template>
  <footer class="status-bar">
    <div class="status-left">
      <span class="status-dot" :class="currentConnection ? 'connected' : 'disconnected'" />
      <span class="status-text" :title="currentConnection?.name">
        {{ currentConnection ? `${currentConnection.name}${currentConnection.driver ? ` · ${currentConnection.driver.toUpperCase()}` : ''}` : t('app.statusNotConnected') }}
      </span>
      <span v-if="currentDatabase" class="status-item">
        <span class="status-label">{{ t('app.statusDatabase') }}</span>{{ currentDatabase }}
      </span>
      <span v-if="currentTable" class="status-item">
        <span class="status-label">{{ t('app.statusTable') }}</span>{{ currentTable }}
      </span>
    </div>
    <div class="status-right">
      <span v-if="hasUnsaved" class="status-unsaved">{{ t('app.statusUnsaved') }}</span>
      <span class="status-shortcut">Ctrl+K {{ t('app.statusCommandPalette') }}</span>
    </div>
  </footer>
</template>

<script setup lang="ts">
import { t } from '@/i18n'

defineProps<{
  /** 当前活跃连接（无则显示未连接；driver 可能缺省，展示时降级） */
  currentConnection: { name: string; driver?: string } | null
  currentDatabase: string
  currentTable: string
  /** 是否存在未保存的编辑（数据表/表设计器） */
  hasUnsaved: boolean
}>()
</script>

<style scoped>
.status-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  height: 24px;
  padding: 0 var(--space-3);
  background: var(--canvas);
  border-top: 1px solid var(--hairline);
  font-size: var(--text-xs);
  color: var(--text-secondary);
  flex-shrink: 0;
  overflow: hidden;
  white-space: nowrap;
}

.status-left,
.status-right {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  min-width: 0;
}

/* 左侧信息过多时优先隐藏溢出部分，不影响右侧快捷键提示 */
.status-left {
  flex: 1 1 auto;
  overflow: hidden;
}

.status-item {
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
  overflow: hidden;
  text-overflow: ellipsis;
}

.status-label {
  color: var(--text-tertiary);
}

.status-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  flex-shrink: 0;
}

.status-dot.connected {
  background: var(--success-500);
}

.status-dot.disconnected {
  background: var(--text-tertiary);
}

.status-unsaved {
  color: var(--warning-500);
}

.status-shortcut {
  color: var(--text-tertiary);
}
</style>
