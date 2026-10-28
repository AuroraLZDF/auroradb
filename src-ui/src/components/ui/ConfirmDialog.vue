<template>
  <Teleport to="body">
    <Transition name="confirm-fade">
      <div
        v-if="confirmState.visible"
        class="confirm-overlay"
        @mousedown.self="cancel"
        @keydown.esc="cancel"
      >
        <div class="confirm-dialog" role="alertdialog" aria-modal="true" :aria-label="title">
          <header class="confirm-header">
            <span class="confirm-title">{{ title }}</span>
            <button
              class="confirm-close"
              type="button"
              :aria-label="t('common.close')"
              @click="cancel"
            >
              <X :size="18" />
            </button>
          </header>

          <div class="confirm-body">
            <p class="confirm-message">{{ confirmState.message }}</p>
            <label v-if="confirmState.requireAcknowledge" class="confirm-ack">
              <input v-model="acknowledged" type="checkbox" />
              <span>{{ t('confirm.acknowledge') }}</span>
            </label>
          </div>

          <footer class="confirm-footer">
            <button
              class="confirm-btn"
              :class="confirmState.danger ? 'confirm-btn--danger' : 'confirm-btn--primary'"
              type="button"
              :disabled="!canConfirm"
              @click="confirm"
            >
              {{ confirmLabel }}
            </button>
            <button
              ref="cancelRef"
              class="confirm-btn confirm-btn--secondary"
              type="button"
              @click="cancel"
            >
              {{ confirmState.cancelText || t('common.cancel') }}
            </button>
          </footer>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { X } from '@/components/icons'
import { t } from '@/i18n'
import { confirmState, resolveConfirm } from '@/composables/useConfirm'

const acknowledged = ref(false)
const cancelRef = ref<HTMLButtonElement | null>(null)

// 每次打开都重置勾选，并把焦点放到更安全的「取消」按钮上
watch(
  () => confirmState.visible,
  async visible => {
    if (!visible) return
    acknowledged.value = false
    await nextTick()
    cancelRef.value?.focus()
  },
)

const canConfirm = computed(() => !confirmState.requireAcknowledge || acknowledged.value)
const title = computed(() => confirmState.title || t('confirm.title'))
const confirmLabel = computed(
  () =>
    confirmState.confirmText ||
    (confirmState.danger ? t('common.delete') : t('common.confirm')),
)

function confirm() {
  if (!canConfirm.value) return
  resolveConfirm(true)
}

function cancel() {
  resolveConfirm(false)
}
</script>

<style scoped>
.confirm-overlay {
  position: fixed;
  inset: 0;
  z-index: calc(var(--z-modal) + 10);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: var(--space-4);
  background: var(--bg-overlay);
}

.confirm-dialog {
  width: 460px;
  max-width: 100%;
  background: var(--surface-default);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xl);
  overflow: hidden;
  animation: confirm-enter var(--duration-normal) var(--ease-out);
}

@keyframes confirm-enter {
  from {
    opacity: 0;
    transform: scale(0.96) translateY(-8px);
  }
  to {
    opacity: 1;
    transform: scale(1) translateY(0);
  }
}

.confirm-header {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: var(--space-4);
  border-bottom: 1px solid var(--border-default);
}

.confirm-title {
  font-size: var(--text-lg);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.confirm-close {
  position: absolute;
  top: 50%;
  right: var(--space-3);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: var(--space-1);
  border: none;
  border-radius: var(--radius-md);
  background: none;
  color: var(--text-secondary);
  cursor: pointer;
  transform: translateY(-50%);
  transition: var(--transition-colors);
}

.confirm-close:hover {
  background: var(--surface-hover);
  color: var(--text-primary);
}

.confirm-body {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
  padding: var(--space-5) var(--space-4);
  background: var(--surface-subtle);
}

.confirm-message {
  margin: 0;
  color: var(--text-primary);
  font-size: var(--text-sm);
  line-height: 1.6;
  text-align: center;
  word-break: break-word;
}

.confirm-ack {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
  color: var(--text-secondary);
  font-size: var(--text-sm);
  line-height: 1.5;
  cursor: pointer;
  user-select: none;
}

.confirm-ack input {
  margin-top: 2px;
  accent-color: var(--error-500);
  cursor: pointer;
}

.confirm-footer {
  display: flex;
  justify-content: center;
  gap: var(--space-3);
  padding: var(--space-4);
  background: var(--surface-subtle);
}

.confirm-btn {
  min-width: 104px;
  height: 34px;
  padding: 0 var(--space-4);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-md);
  background: var(--surface-default);
  color: var(--text-primary);
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  cursor: pointer;
  transition: var(--transition-colors);
}

.confirm-btn--secondary:hover {
  background: var(--surface-hover);
}

.confirm-btn--danger {
  border-color: var(--error-500);
  background: var(--error-500);
  color: #fff;
}

.confirm-btn--danger:hover:not(:disabled) {
  border-color: var(--error-600);
  background: var(--error-600);
}

.confirm-btn--primary {
  border-color: var(--brand-500);
  background: var(--brand-500);
  color: #fff;
}

.confirm-btn--primary:hover:not(:disabled) {
  border-color: var(--brand-600, var(--brand-500));
  background: var(--brand-600, var(--brand-500));
}

.confirm-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.confirm-fade-enter-active,
.confirm-fade-leave-active {
  transition: opacity 0.15s ease;
}

.confirm-fade-enter-from,
.confirm-fade-leave-to {
  opacity: 0;
}
</style>
