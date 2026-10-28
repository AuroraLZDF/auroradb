import { reactive, readonly } from 'vue'

export interface ConfirmOptions {
  /** 提示内容 */
  message: string
  /** 标题，默认「警告」 */
  title?: string
  /** 确认按钮文案，默认「删除」 */
  confirmText?: string
  /** 取消按钮文案，默认「取消」 */
  cancelText?: string
  /** 是否要求勾选「我已知晓此操作不可恢复」后才能确认 */
  requireAcknowledge?: boolean
  /** 确认按钮是否使用危险（红色）样式，默认 true */
  danger?: boolean
}

interface ConfirmState {
  visible: boolean
  title: string
  message: string
  confirmText: string
  cancelText: string
  requireAcknowledge: boolean
  danger: boolean
}

const state = reactive<ConfirmState>({
  visible: false,
  title: '',
  message: '',
  confirmText: '',
  cancelText: '',
  requireAcknowledge: false,
  danger: true,
})

/** 供 ConfirmDialog 组件读取的只读状态 */
export const confirmState = readonly(state)

let resolver: ((confirmed: boolean) => void) | null = null

/**
 * 弹出二次确认框，返回用户是否确认。
 * 同一时间只保留一个确认框，旧的按取消处理，避免 Promise 悬挂。
 */
export function confirmDanger(options: ConfirmOptions): Promise<boolean> {
  if (resolver) {
    resolver(false)
    resolver = null
  }

  state.title = options.title ?? ''
  state.message = options.message
  state.confirmText = options.confirmText ?? ''
  state.cancelText = options.cancelText ?? ''
  state.requireAcknowledge = options.requireAcknowledge ?? false
  state.danger = options.danger ?? true
  state.visible = true

  return new Promise<boolean>(resolve => {
    resolver = resolve
  })
}

/**
 * 普通二次确认（非破坏性操作，如刷新、放弃搜索），返回用户是否确认。
 */
export function confirmAction(options: ConfirmOptions): Promise<boolean> {
  return confirmDanger({ ...options, danger: false })
}

/** 由 ConfirmDialog 组件调用，结束当前确认流程 */
export function resolveConfirm(confirmed: boolean) {
  if (!resolver) return
  const resolve = resolver
  resolver = null
  state.visible = false
  resolve(confirmed)
}
