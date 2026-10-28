<template>
  <div class="about-dialog">
    <div class="dialog-header">
      <div class="header-title">
        <Info :size="18" class="title-icon" />
        <h2>{{ t('about.title') }}</h2>
      </div>
      <button class="icon-btn" :title="t('common.close')" @click="emit('close')">
        <X :size="18" />
      </button>
    </div>

    <div class="dialog-body">
      <div class="brand">
        <div class="brand-logo">
          <Database :size="28" />
        </div>
        <div class="brand-text">
          <div class="brand-name">AuroraDB</div>
          <span class="version-badge">{{ versionLabel }}</span>
        </div>
      </div>

      <p class="description">{{ t('about.description') }}</p>

      <div class="meta-list">
        <div class="meta-item">
          <span class="meta-label">{{ t('about.version') }}</span>
          <span class="meta-value">{{ versionLabel }}</span>
        </div>
      </div>

      <p class="copyright">{{ t('about.copyright') }}</p>
    </div>

    <div class="dialog-footer">
      <Button variant="secondary" @click="emit('close')">{{ t('common.close') }}</Button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { Database, Info, X } from 'lucide-vue-next'
import { getVersion } from '@tauri-apps/api/app'
import Button from '@/components/ui/Button.vue'
import { t } from '@/i18n'

const emit = defineEmits<{ close: [] }>()

const version = ref('')

const versionLabel = computed(() => (version.value ? `v${version.value}` : '—'))

onMounted(async () => {
  // 浏览器预览下没有 Tauri 环境，读取失败时保留占位符
  if (!('__TAURI_INTERNALS__' in window)) return
  try {
    version.value = await getVersion()
  } catch {
    version.value = ''
  }
})
</script>

<style scoped>
.about-dialog {
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

.brand {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.brand-logo {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 56px;
  height: 56px;
  border-radius: var(--radius-lg);
  background: rgba(33, 150, 243, 0.12);
  color: var(--brand-500);
  flex-shrink: 0;
}

.brand-text {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.brand-name {
  font-size: var(--text-xl);
  font-weight: var(--font-bold);
  color: var(--text-primary);
}

.version-badge {
  padding: 2px var(--space-2);
  border-radius: var(--radius-full, 999px);
  background: var(--surface-hover);
  color: var(--text-secondary);
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
}

.description {
  margin: 0;
  font-size: var(--text-sm);
  line-height: 1.6;
  color: var(--text-secondary);
}

.meta-list {
  display: flex;
  flex-direction: column;
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  overflow: hidden;
}

.meta-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  padding: var(--space-2) var(--space-3);
  font-size: var(--text-sm);
}

.meta-item + .meta-item {
  border-top: 1px solid var(--border-default);
}

.meta-label {
  color: var(--text-secondary);
}

.meta-value {
  color: var(--text-primary);
}

.copyright {
  margin: 0;
  font-size: var(--text-xs);
  color: var(--text-tertiary, var(--text-secondary));
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
  padding: var(--space-3) var(--space-5);
  border-top: 1px solid var(--border-default);
}
</style>
