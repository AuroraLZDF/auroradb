<template>
  <Teleport to="body">
    <div class="toast-container">
      <TransitionGroup name="toast">
        <div
          v-for="toast in toasts"
          :key="toast.id"
          class="toast"
          :class="toast.type"
          @click="removeToast(toast.id)"
        >
          <div class="toast-icon">
            <CheckCircle v-if="toast.type === 'success'" :size="20" />
            <XCircle v-else-if="toast.type === 'error'" :size="20" />
            <AlertTriangle v-else-if="toast.type === 'warning'" :size="20" />
            <Info v-else :size="20" />
          </div>
          <span class="toast-message">{{ toast.message }}</span>
          <button
            class="toast-close"
            :title="t('ui.closeToast')"
            :aria-label="t('ui.closeToast')"
            @click.stop="removeToast(toast.id)"
          >
            <X :size="14" />
          </button>
        </div>
      </TransitionGroup>
    </div>
  </Teleport>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { CheckCircle, XCircle, AlertTriangle, Info, X } from '@/components/icons'
import { t } from '@/i18n'

interface ToastNotificationItem {
  id: string
  message: string
  type: 'success' | 'error' | 'warning' | 'info'
  duration: number
}

const toasts = ref<ToastNotificationItem[]>([])
let toastId = 0

// 显示 Toast
function show(message: string, type: ToastNotificationItem['type'] = 'info', duration = 3000) {
  const id = `toast-${++toastId}`
  const toast: ToastNotificationItem = {
    id,
    message,
    type,
    duration
  }
  toasts.value.push(toast)

  // 自动移除
  setTimeout(() => {
    removeToast(id)
  }, duration)
}

// 移除 Toast
function removeToast(id: string) {
  const index = toasts.value.findIndex((t: ToastNotificationItem) => t.id === id)
  if (index > -1) {
    toasts.value.splice(index, 1)
  }
}

// 导出方法
defineExpose({
  show,
  success: (msg: string) => show(msg, 'success'),
  error: (msg: string) => show(msg, 'error'),
  warning: (msg: string) => show(msg, 'warning'),
  info: (msg: string) => show(msg, 'info')
})
</script>

<style scoped>
.toast-container {
  position: fixed;
  top: 60px;
  right: 20px;
  z-index: var(--z-toast);
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  pointer-events: none;
}

.toast {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
  background: var(--surface-default);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-lg);
  min-width: 280px;
  max-width: 400px;
  pointer-events: auto;
  cursor: pointer;
}

.toast.success {
  border-left: 4px solid var(--success-500);
}

.toast.error {
  border-left: 4px solid var(--error-500);
}

.toast.warning {
  border-left: 4px solid var(--warning-500);
}

.toast.info {
  border-left: 4px solid var(--brand-500);
}

.toast-icon {
  flex-shrink: 0;
}

.toast.success .toast-icon {
  color: var(--success-500);
}

.toast.error .toast-icon {
  color: var(--error-500);
}

.toast.warning .toast-icon {
  color: var(--warning-500);
}

.toast.info .toast-icon {
  color: var(--brand-500);
}

.toast-message {
  flex: 1;
  font-size: var(--text-sm);
  color: var(--text-primary);
  line-height: var(--leading-normal);
}

.toast-close {
  width: 20px;
  height: 20px;
  background: none;
  border: none;
  color: var(--text-tertiary);
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-sm);
  opacity: 0;
  transition: var(--transition-all);
}

.toast:hover .toast-close {
  opacity: 1;
}

.toast-close:hover {
  color: var(--text-secondary);
  background: var(--surface-hover);
}

/* Toast 入场/出场动画 */
.toast-enter-active,
.toast-leave-active {
  transition: all var(--duration-slow) var(--ease-out);
}

.toast-enter-from {
  opacity: 0;
  transform: translateX(100%) scale(0.9);
}

.toast-leave-to {
  opacity: 0;
  transform: translateX(100%);
}

/* 列表动画 */
.toast-move {
  transition: transform var(--duration-normal);
}
</style>
