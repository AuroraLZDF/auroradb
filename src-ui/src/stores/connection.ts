import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { DriverKind, SslMode, SafetyMode } from './savedConnections'

/** 统一走模块化 invoke；非 Tauri 环境（如浏览器预览）返回明确错误 */
function tauriInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!('__TAURI_INTERNALS__' in window)) {
    return Promise.reject(new Error('Not running in Tauri environment'))
  }
  return invoke<T>(cmd, args)
}

interface ConnectionConfig {
  id: string
  name: string
  /** 数据库驱动，缺省时后端按 mysql 处理 */
  driver?: DriverKind
  host: string
  port: number
  database?: string
  username: string
  password: string
  pool_config?: {
    max_connections?: number
    min_connections?: number
    connect_timeout?: number
    idle_timeout?: number
    max_lifetime?: number
    heartbeat_interval?: number
  }
  /** SSL/TLS 配置，缺省时后端按 preferred 处理 */
  ssl_config?: {
    mode?: SslMode
    ca_path?: string
    cert_path?: string
    key_path?: string
  }
  /** 访问模式，缺省时后端按 off 处理 */
  safety_mode?: SafetyMode
}

/** SSH 隧道配置，字段名需与后端 SshConfig 一致（snake_case） */
export interface SshConfig {
  host: string
  port?: number
  username: string
  auth_type: 'password' | 'private-key'
  password?: string
  private_key_path?: string
  passphrase?: string
}

export interface ConnectionStatus {
  id: string
  name: string
  /** 数据库驱动，后端状态未回带时由前端按配置补齐 */
  driver?: DriverKind
  /** 实际连接的数据库名（PG 配置未填库时为服务端默认库） */
  database?: string | null
  host: string
  port: number
  username: string
  state: 'connected' | 'disconnected' | 'reconnecting' | 'failed'
  message: string
  connected_at?: string
  last_error?: string
  stability_score: number
  health_status: 'good' | 'fair' | 'poor'
  /** 访问模式（随连接创建时确定；修改后需重新连接生效） */
  safety_mode: SafetyMode
}

export const useConnectionStore = defineStore('connection', () => {
  // 所有打开的活跃连接
  const activeConnections = ref<ConnectionStatus[]>([])
  // 当前选中的连接
  const currentConnection = ref<ConnectionStatus | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)
  /** 因取消查询导致事务被回滚的次数：各组件据此把本地「事务活跃」状态复位 */
  const transactionResetSeq = ref(0)

  // 连接列表
  const connectionList = computed(() => activeConnections.value)

  // 是否存在指定连接
  function hasConnection(connectionId: string): boolean {
    return activeConnections.value.some(c => c.id === connectionId)
  }

  // 创建新连接（支持多连接）
  // sshConfig 非空时经 SSH 隧道连接，config.host/port 由 SSH 服务器侧解析
  async function createConnection(config: ConnectionConfig, sshConfig?: SshConfig): Promise<ConnectionStatus> {
    loading.value = true
    error.value = null

    try {
      const status = await tauriInvoke<ConnectionStatus>('connect', {
        config,
        sshConfig: sshConfig ?? null,
      })
      // 合并配置信息
      const statusWithInfo: ConnectionStatus = {
        ...status,
        name: config.name,
        driver: config.driver ?? status.driver ?? 'mysql',
        host: config.host,
        port: config.port,
        username: config.username,
        safety_mode: config.safety_mode ?? 'off',
      }

      // 添加到活跃连接列表
      activeConnections.value.push(statusWithInfo)
      // 自动切换到新连接
      currentConnection.value = statusWithInfo

      return statusWithInfo
    } catch (e) {
      error.value = String(e)
      throw e
    } finally {
      loading.value = false
    }
  }

  // 断开指定连接
  async function disconnect(connectionId: string): Promise<void> {
    try {
      await tauriInvoke('disconnect', { connectionId })
      // 从活跃连接列表移除
      activeConnections.value = activeConnections.value.filter(c => c.id !== connectionId)

      // 如果断开的是当前选中的连接，切换到其他连接或置空
      if (currentConnection.value?.id === connectionId) {
        currentConnection.value = activeConnections.value[0] || null
      }
    } catch (e) {
      console.error('Failed to disconnect:', e)
      // 即使后端断开失败，也从列表中移除
      activeConnections.value = activeConnections.value.filter(c => c.id !== connectionId)
      if (currentConnection.value?.id === connectionId) {
        currentConnection.value = activeConnections.value[0] || null
      }
      throw e
    }
  }

  // 切换到指定连接
  function switchConnection(connectionId: string) {
    const conn = activeConnections.value.find(c => c.id === connectionId)
    if (conn) {
      currentConnection.value = conn
    }
  }

  // 获取连接状态
  async function getStatus(connectionId: string): Promise<ConnectionStatus> {
    return await tauriInvoke<ConnectionStatus>('get_connection_status', { connectionId })
  }

  // 执行SQL
  async function executeSql(connectionId: string, database: string, sql: string) {
    return await tauriInvoke('execute_sql', { connectionId, database, sql })
  }

  // 取消正在执行的SQL；返回是否因此回滚了进行中的事务
  async function cancelQuery(connectionId: string): Promise<boolean> {
    const rolledBack = await tauriInvoke<boolean>('cancel_query', { connectionId })
    if (rolledBack) transactionResetSeq.value++
    return rolledBack
  }

  // 设置当前连接
  function setCurrentConnection(connection: ConnectionStatus | null) {
    currentConnection.value = connection
  }

  // 更新连接状态（用于心跳检测）
  function updateConnectionStatus(connectionId: string, status: Partial<ConnectionStatus>) {
    const index = activeConnections.value.findIndex(c => c.id === connectionId)
    if (index !== -1) {
      activeConnections.value[index] = { ...activeConnections.value[index], ...status } as ConnectionStatus
      // 如果当前连接被更新，同步更新
      if (currentConnection.value?.id === connectionId) {
        currentConnection.value = activeConnections.value[index] || null
      }
    }
  }

  /** 连接状态轮询定时器（全局唯一） */
  let statusTimer: ReturnType<typeof setInterval> | null = null

  /**
   * 拉取一次所有活跃连接的后端状态并同步到列表。
   * 心跳检测到的 reconnecting/failed 状态只有主动拉取才能反映到 UI。
   */
  async function refreshStatuses(): Promise<void> {
    const ids = activeConnections.value.map(c => c.id)
    await Promise.all(ids.map(async (id) => {
      try {
        const status = await getStatus(id)
        updateConnectionStatus(id, {
          state: status.state,
          message: status.message,
          stability_score: status.stability_score,
          health_status: status.health_status,
          last_error: status.last_error,
        })
      } catch {
        // 后端已无该连接（例如已断开）时忽略，避免轮询不断产生错误
      }
    }))
  }

  // 启动连接状态轮询（重复调用无副作用）
  function startStatusPolling(intervalMs = 5000) {
    if (statusTimer) return
    statusTimer = setInterval(() => { void refreshStatuses() }, intervalMs)
  }

  // 停止连接状态轮询
  function stopStatusPolling() {
    if (statusTimer) {
      clearInterval(statusTimer)
      statusTimer = null
    }
  }

  return {
    activeConnections,
    connections: activeConnections, // 兼容旧代码
    currentConnection,
    loading,
    error,
    transactionResetSeq,
    connectionList,
    hasConnection,
    createConnection,
    disconnect,
    switchConnection,
    getStatus,
    executeSql,
    cancelQuery,
    setCurrentConnection,
    updateConnectionStatus,
    refreshStatuses,
    startStatusPolling,
    stopStatusPolling,
  }
})
