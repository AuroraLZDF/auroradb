<template>
  <button
    :class="buttonClasses"
    :disabled="disabled || loading"
    :type="type"
    @click="handleClick"
    v-ripple="{ color: rippleColor }"
  >
    <span v-if="loading" class="btn-spinner">
      <RefreshCw :size="iconSize" class="spin" />
    </span>
    <span v-else-if="icon" class="btn-icon">
      <component :is="icon" :size="iconSize" />
    </span>
    <span v-if="slots.default" class="btn-text">
      <slot />
    </span>
  </button>
</template>

<script setup lang="ts">
import { computed, useSlots } from 'vue'
import { RefreshCw } from '@/components/icons'
import { vRipple } from '@/directives/ripple'
import type { Component } from 'vue'

// BaseButton - 通用按钮组件

defineOptions({
  name: 'BaseButton'
})

interface Props {
  variant?: 'primary' | 'secondary' | 'danger' | 'ghost' | 'text'
  size?: 'xs' | 'sm' | 'md' | 'lg'
  type?: 'button' | 'submit' | 'reset'
  disabled?: boolean
  loading?: boolean
  icon?: Component | string
}

const props = withDefaults(defineProps<Props>(), {
  variant: 'secondary',
  size: 'md',
  type: 'button',
  disabled: false,
  loading: false
})

const emit = defineEmits<{
  click: [event: MouseEvent]
}>()

const slots = useSlots()

const buttonClasses = computed(() => [
  'btn',
  `btn--${props.variant}`,
  `btn--${props.size}`,
  {
    'is-loading': props.loading,
    'has-icon': props.icon,
    'has-text': slots.default
  }
])

const iconSize = computed(() => {
  const sizes = { xs: 12, sm: 14, md: 16, lg: 20 }
  return sizes[props.size]
})

const rippleColor = computed(() => {
  const colors = {
    primary: 'rgba(255, 255, 255, 0.3)',
    secondary: 'rgba(33, 150, 243, 0.15)',
    danger: 'rgba(255, 255, 255, 0.3)',
    ghost: 'rgba(33, 150, 243, 0.15)',
    text: 'rgba(33, 150, 243, 0.15)'
  }
  return colors[props.variant]
})

function handleClick(event: MouseEvent) {
  if (!props.disabled && !props.loading) {
    emit('click', event)
  }
}
</script>

<style scoped>
.btn {
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-2);
  border: 1px solid transparent;
  border-radius: var(--radius-md);
  font-weight: var(--font-medium);
  cursor: pointer;
  transition: var(--transition-all);
  overflow: hidden;
  user-select: none;
}

/* Size variants */
.btn--xs {
  padding: var(--space-1) var(--space-2);
  font-size: var(--text-xs);
  height: 24px;
}

.btn--sm {
  padding: var(--space-1) var(--space-2);
  font-size: var(--text-sm);
  height: 28px;
}

.btn--md {
  padding: var(--space-2) var(--space-3);
  font-size: var(--text-sm);
  height: 32px;
}

.btn--lg {
  padding: var(--space-2) var(--space-4);
  font-size: var(--text-base);
  height: 40px;
}

/* Color variants */
.btn--primary {
  background: var(--brand-500);
  border-color: var(--brand-500);
  color: white;
}

.btn--primary:hover:not(:disabled) {
  background: var(--brand-600);
  border-color: var(--brand-600);
  transform: translateY(-1px);
  box-shadow: var(--shadow-md);
}

.btn--primary:active:not(:disabled) {
  transform: translateY(0);
  box-shadow: none;
}

.btn--secondary {
  background: var(--surface-default);
  border-color: var(--border-default);
  color: var(--text-primary);
}

.btn--secondary:hover:not(:disabled) {
  background: var(--surface-hover);
  border-color: var(--border-strong);
}

.btn--danger {
  background: var(--error-500);
  border-color: var(--error-500);
  color: white;
}

.btn--danger:hover:not(:disabled) {
  background: var(--error-600);
  border-color: var(--error-600);
}

.btn--ghost {
  background: transparent;
  border-color: transparent;
  color: var(--text-secondary);
}

.btn--ghost:hover:not(:disabled) {
  background: var(--surface-hover);
  color: var(--text-primary);
}

.btn--text {
  background: transparent;
  border-color: transparent;
  color: var(--brand-500);
  padding-left: var(--space-2);
  padding-right: var(--space-2);
}

.btn--text:hover:not(:disabled) {
  background: rgba(33, 150, 243, 0.08);
}

/* Loading state */
.btn.is-loading {
  color: transparent;
  pointer-events: none;
}

.btn-spinner {
  position: absolute;
  display: flex;
  align-items: center;
  justify-content: center;
}

.spin {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

/* Disabled state */
.btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* Icon and text layout */
.btn-icon {
  display: flex;
  align-items: center;
  justify-content: center;
}

.btn-text {
  display: flex;
  align-items: center;
}
</style>
