<template>
  <Teleport to="body">
    <div class="toast-container">
      <TransitionGroup name="toast">
        <div
          v-for="toast in toasts"
          :key="toast.id"
          class="toast"
          :class="toast.type"
          @click="onToastClick(toast)"
        >
          <div class="toast-icon">
            <CheckCircle v-if="toast.type === 'success'" :size="20" />
            <XCircle v-else-if="toast.type === 'error'" :size="20" />
            <AlertTriangle v-else-if="toast.type === 'warning'" :size="20" />
            <Info v-else :size="20" />
          </div>
          <!-- 错误消息可能很长：默认两行截断，点击正文或箭头展开查看全文 -->
          <span class="toast-message" :class="{ 'expandable': toast.type === 'error', 'expanded': isExpanded(toast.id) }">
            {{ toast.message }}
          </span>
          <template v-if="toast.type === 'error'">
            <button
              v-if="isExpanded(toast.id)"
              class="toast-action"
              :title="t('ui.copyToast')"
              :aria-label="t('ui.copyToast')"
              @click.stop="copyToast(toast)"
            >
              <Copy :size="14" />
            </button>
            <button
              class="toast-action"
              :title="isExpanded(toast.id) ? t('ui.collapseToast') : t('ui.expandToast')"
              :aria-label="isExpanded(toast.id) ? t('ui.collapseToast') : t('ui.expandToast')"
              @click.stop="toggleExpand(toast.id)"
            >
              <ChevronUp v-if="isExpanded(toast.id)" :size="14" />
              <ChevronDown v-else :size="14" />
            </button>
          </template>
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
import { CheckCircle, XCircle, AlertTriangle, Info, X, ChevronDown, ChevronUp, Copy } from '@/components/icons'
import { t } from '@/i18n'

interface ToastNotificationItem {
  id: string
  message: string
  type: 'success' | 'error' | 'warning' | 'info'
  /** 0 表示常驻，需手动关闭（error 默认） */
  duration: number
}

const toasts = ref<ToastNotificationItem[]>([])
const expandedIds = ref<Set<string>>(new Set())
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

  // duration <= 0 常驻：由用户通过关闭按钮移除
  if (duration > 0) {
    setTimeout(() => {
      removeToast(id)
    }, duration)
  }
}

// 移除 Toast
function removeToast(id: string) {
  const index = toasts.value.findIndex((t: ToastNotificationItem) => t.id === id)
  if (index > -1) {
    toasts.value.splice(index, 1)
  }
  expandedIds.value.delete(id)
}

function isExpanded(id: string): boolean {
  return expandedIds.value.has(id)
}

function toggleExpand(id: string) {
  if (expandedIds.value.has(id)) {
    expandedIds.value.delete(id)
  } else {
    expandedIds.value.add(id)
  }
}

// 非 error 的 toast 保持「点击即关」；error 的正文点击用于展开/收起
function onToastClick(toast: ToastNotificationItem) {
  if (toast.type === 'error') {
    toggleExpand(toast.id)
  } else {
    removeToast(toast.id)
  }
}

async function copyToast(toast: ToastNotificationItem) {
  try {
    await navigator.clipboard.writeText(toast.message)
  } catch {
    // 剪贴板不可用时静默失败，用户仍可手动选中文本复制
  }
}

// 导出方法
defineExpose({
  show,
  success: (msg: string) => show(msg, 'success'),
  error: (msg: string) => show(msg, 'error', 0),
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
  /* 常驻错误可能堆积：超过视口时容器内部滚动 */
  max-height: calc(100vh - 80px);
  overflow-y: auto;
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
  word-break: break-all;
}

/* 错误详情默认两行截断，展开后显示全文 */
.toast-message.expandable {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  overflow: hidden;
  cursor: zoom-in;
}

.toast-message.expandable.expanded {
  -webkit-line-clamp: unset;
  overflow-wrap: anywhere;
  cursor: zoom-out;
}

.toast-action {
  width: 20px;
  height: 20px;
  flex-shrink: 0;
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

.toast:hover .toast-action,
.toast-message.expandable.expanded + .toast-action {
  opacity: 1;
}

.toast-action:hover {
  color: var(--text-secondary);
  background: var(--surface-hover);
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
