import { ref } from 'vue'
import type ToastNotification from '@/components/ui/Toast.vue'

type ToastType = 'success' | 'error' | 'warning' | 'info'
type ToastInstance = InstanceType<typeof ToastNotification>

interface ToastOptions {
  duration?: number
}

// Toast 实例
const toastRef = ref<ToastInstance | null>(null)

export function useToast() {
  // 设置 Toast 实例
  function setToastRef(ref: ToastInstance) {
    toastRef.value = ref
  }

  // 显示 Toast
  function show(message: string, type: ToastType = 'info', options: ToastOptions = {}) {
    if (toastRef.value) {
      const duration = options.duration || 3000
      toastRef.value.show(message, type, duration)
    }
  }

  // 快捷方法
  function success(message: string, options?: ToastOptions) {
    show(message, 'success', options)
  }

  function error(message: string, options?: ToastOptions) {
    // 错误信息常驻（duration 0），由用户手动关闭；显式传入 duration 时优先使用
    const duration = options?.duration ?? 0
    show(message, 'error', { duration })
  }

  function warning(message: string, options?: ToastOptions) {
    show(message, 'warning', options)
  }

  function info(message: string, options?: ToastOptions) {
    show(message, 'info', options)
  }

  return {
    setToastRef,
    show,
    success,
    error,
    warning,
    info
  }
}

// 全局 Toast 实例
export const globalToast = {
  show(message: string, type: ToastType = 'info', duration = 3000) {
    if (toastRef.value) {
      toastRef.value.show(message, type, duration)
    }
  },
  success(message: string) {
    this.show(message, 'success')
  },
  error(message: string) {
    // 错误信息常驻，由用户手动关闭
    this.show(message, 'error', 0)
  },
  warning(message: string) {
    this.show(message, 'warning')
  },
  info(message: string) {
    this.show(message, 'info')
  }
}
