<template>
  <div
    v-if="show"
    ref="menuRef"
    class="context-menu"
    role="menu"
    tabindex="-1"
    :style="menuStyle"
    @click.stop
    @keydown="onKeydown"
  >
    <template v-for="(entry, index) in entries" :key="index">
      <div v-if="'divider' in entry" class="menu-divider" role="separator"></div>
      <div
        v-else
        class="menu-item"
        :class="{ 'menu-item-danger': entry.danger, 'menu-item-active': index === activeIndex }"
        role="menuitem"
        :aria-disabled="entry.disabled || undefined"
        @click="select(entry, index)"
        @mouseenter="activeIndex = index"
      >
        <component :is="entry.icon" v-if="entry.icon" class="menu-icon" :size="14" />
        {{ entry.label }}
      </div>
    </template>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, nextTick, type Component } from 'vue'

export interface ContextMenuItem {
  /** 动作标识，选中后透传给父组件 */
  key: string
  label: string
  icon?: Component
  danger?: boolean
  disabled?: boolean
}

export type MenuEntry = ContextMenuItem | { divider: true }

const props = defineProps<{
  show: boolean
  x: number
  y: number
  entries: MenuEntry[]
}>()

const emit = defineEmits<{
  (e: 'select', key: string): void
  (e: 'close'): void
}>()

const menuRef = ref<HTMLElement | null>(null)
const activeIndex = ref(-1)

/** 视口边缘翻转：默认在光标右下弹出，靠近右/下边界时反向，避免被裁剪 */
const menuStyle = computed(() => {
  const flipX = props.x + 180 > window.innerWidth
  const flipY = props.y + 40 + props.entries.length * 32 > window.innerHeight
  return {
    left: `${flipX ? Math.max(8, props.x - 180) : props.x}px`,
    top: `${flipY ? Math.max(8, props.y - 8 - props.entries.length * 32) : props.y}px`,
  }
})

watch(
  () => props.show,
  show => {
    if (show) {
      activeIndex.value = -1
      // 打开即接管焦点：让 Escape / 方向键立刻生效，关闭后焦点自然归还
      nextTick(() => menuRef.value?.focus())
    }
  }
)

function select(entry: ContextMenuItem, index: number) {
  if (entry.disabled) return
  activeIndex.value = index
  emit('select', entry.key)
}

function onKeydown(e: KeyboardEvent) {
  const selectable = props.entries
    .map((entry, index) => ('divider' in entry || entry.disabled ? -1 : index))
    .filter(index => index >= 0)

  /** 在可选条目间循环移动焦点；当前无焦点时从首/尾进入 */
  function moveActive(delta: 1 | -1) {
    if (selectable.length === 0) return
    const current = selectable.indexOf(activeIndex.value)
    if (current < 0) {
      activeIndex.value = delta === 1 ? (selectable[0] ?? -1) : (selectable[selectable.length - 1] ?? -1)
      return
    }
    const next = (current + delta + selectable.length) % selectable.length
    activeIndex.value = selectable[next] ?? -1
  }

  switch (e.key) {
    case 'Escape':
      e.preventDefault()
      e.stopPropagation()
      emit('close')
      break
    case 'ArrowDown':
      e.preventDefault()
      moveActive(1)
      break
    case 'ArrowUp':
      e.preventDefault()
      moveActive(-1)
      break
    case 'Home':
      e.preventDefault()
      activeIndex.value = selectable[0] ?? -1
      break
    case 'End':
      e.preventDefault()
      activeIndex.value = selectable[selectable.length - 1] ?? -1
      break
    case 'Enter':
    case ' ': {
      e.preventDefault()
      const entry = props.entries[activeIndex.value]
      if (entry && !('divider' in entry)) select(entry, activeIndex.value)
      break
    }
  }
}
</script>

<style scoped>
.context-menu {
  position: fixed;
  background: var(--overlay);
  border: 1px solid var(--hairline-strong);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-lg);
  z-index: var(--z-popover);
  min-width: 160px;
  padding: var(--space-1) 0;
  animation: menu-enter var(--duration-fast) var(--ease-out);
  outline: none;
}

@keyframes menu-enter {
  from {
    opacity: 0;
    transform: scale(0.95);
  }
  to {
    opacity: 1;
    transform: scale(1);
  }
}

.menu-item {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-2) var(--space-4);
  cursor: pointer;
  font-size: var(--text-sm);
  color: var(--text-primary);
  transition: background var(--duration-fast);
}

.menu-item:hover,
.menu-item-active {
  background: var(--hover-fill);
}

.menu-item-danger {
  color: var(--error-500);
}

.menu-item-danger:hover,
.menu-item-danger.menu-item-active {
  background: rgba(239, 68, 68, 0.12);
  color: var(--error-500);
}

.menu-icon {
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.menu-divider {
  height: 1px;
  background: var(--hairline);
  margin: var(--space-1) 0;
}
</style>
