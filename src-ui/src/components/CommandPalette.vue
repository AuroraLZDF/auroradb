<template>
  <Teleport to="body">
    <div
      v-if="registry.open"
      class="palette-overlay"
      @click.self="registry.closePalette()"
      @keydown.escape="registry.closePalette()"
    >
      <div class="palette" role="dialog" aria-label="Command Palette">
        <div class="palette-input-wrap">
          <Search :size="16" class="palette-search-icon" />
          <input
            ref="inputRef"
            v-model="registry.query"
            class="palette-input"
            :placeholder="t('commandPalette.placeholder')"
            autofocus
            @keydown.arrow-down.prevent="registry.moveActive(1)"
            @keydown.arrow-up.prevent="registry.moveActive(-1)"
            @keydown.enter.prevent="registry.runActive()"
          />
          <kbd class="palette-kbd">Ctrl + K</kbd>
        </div>

        <div class="palette-list" v-if="registry.filtered.length > 0">
          <div
            v-for="(cmd, index) in registry.filtered"
            :key="cmd.id"
            class="palette-item"
            :class="{ active: index === registry.activeIndex }"
            @mouseenter="registry.activeIndex = index"
            @click="registry.runCommand(cmd)"
          >
            <div class="palette-title">
              <span class="palette-name">{{ cmd.title }}</span>
              <span v-if="cmd.category" class="palette-cat">{{ cmd.category }}</span>
            </div>
            <kbd v-if="cmd.shortcut" class="palette-item-kbd">{{ cmd.shortcut }}</kbd>
          </div>
        </div>

        <div v-else class="palette-empty">{{ t('commandPalette.noResults') }}</div>
      </div>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { ref, nextTick, watch } from 'vue'
import { Search } from 'lucide-vue-next'
import { useCommandRegistryStore } from '@/stores/commandRegistry'
import { t } from '@/i18n'

const registry = useCommandRegistryStore()
const inputRef = ref<HTMLInputElement>()

// 每次打开把光标聚焦到输入框
watch(
  () => registry.open,
  (open) => {
    if (open) nextTick(() => { inputRef.value?.focus() })
  }
)

// 过滤结果变化时把 activeIndex 钳到合法范围，避免越界
watch(
  () => registry.filtered.length,
  (len) => {
    if (registry.activeIndex >= len) {
      registry.activeIndex = Math.max(0, len - 1)
    }
  }
)
</script>

<style scoped>
.palette-overlay {
  position: fixed;
  inset: 0;
  background: var(--bg-overlay);
  display: flex;
  align-items: flex-start;
  justify-content: center;
  padding-top: 12vh;
  z-index: var(--z-modal);
  backdrop-filter: blur(2px);
}

.palette {
  width: 540px;
  max-width: 92vw;
  max-height: 70vh;
  background: var(--overlay);
  border: 1px solid var(--hairline-strong);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xl);
  display: flex;
  flex-direction: column;
  overflow: hidden;
  animation: palette-in 120ms ease-out;
}

@keyframes palette-in {
  from { transform: translateY(-6px); opacity: 0; }
  to   { transform: translateY(0);     opacity: 1; }
}

.palette-input-wrap {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 10px var(--space-3);
  border-bottom: 1px solid var(--hairline);
  background: var(--overlay);
}

.palette-search-icon {
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.palette-input {
  flex: 1;
  min-width: 0;
  border: none;
  outline: none;
  background: transparent;
  font-size: var(--text-sm);
  color: var(--text-primary);
}

.palette-input::placeholder {
  color: var(--text-tertiary);
}

.palette-kbd {
  font-size: 10px;
  color: var(--text-tertiary);
  border: 1px solid var(--hairline-strong);
  padding: 1px 5px;
  border-radius: var(--radius-xs);
  font-family: inherit;
  flex-shrink: 0;
}

.palette-list {
  overflow-y: auto;
  padding: 4px;
}

.palette-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
  min-height: var(--tree-row-height);
  padding: 3px var(--space-2);
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: var(--transition-colors);
}

.palette-item:hover,
.palette-item.active {
  background: var(--selected-fill);
}

.palette-title {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-width: 0;
}

.palette-name {
  color: var(--text-primary);
  font-size: var(--text-sm);
}

.palette-cat {
  color: var(--text-tertiary);
  font-size: 10px;
  padding: 0 6px;
  border-radius: var(--radius-xs);
  background: var(--hover-fill);
  flex-shrink: 0;
}

.palette-item-kbd {
  font-size: 10px;
  color: var(--text-tertiary);
  border: 1px solid var(--hairline-strong);
  padding: 1px 5px;
  border-radius: var(--radius-xs);
  font-family: inherit;
  flex-shrink: 0;
}

.palette-empty {
  padding: var(--space-6) var(--space-4);
  text-align: center;
  color: var(--text-tertiary);
  font-size: var(--text-sm);
}
</style>
