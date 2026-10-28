import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'

/** SSL/TLS 模式，值需与后端 SslMode 的 serde 命名（kebab-case）一致 */
export type SslMode = 'disabled' | 'preferred' | 'required' | 'verify-ca' | 'verify-identity'

/** SSH 认证方式，值需与后端 SshAuthType 的 serde 命名（kebab-case）一致 */
export type SshAuthType = 'password' | 'private-key'

/** 访问模式，值需与后端 SafetyMode 的 serde 命名（kebab-case）一致 */
export type SafetyMode = 'off' | 'safe' | 'read-only'

/** 数据库驱动，值需与后端 DriverKind 的 serde 命名（lowercase）一致 */
export type DriverKind = 'mysql' | 'postgres'

/** SSH 配置中可持久化的部分（密码/口令不写 localStorage） */
interface SshSavedFields {
  sshEnabled?: boolean
  sshHost?: string
  sshPort?: number
  sshUsername?: string
  sshAuthType?: SshAuthType
  sshPrivateKeyPath?: string
  /** SSH 密码或私钥口令是否已存入后端加密存储 */
  hasSshSecret?: boolean
}

export interface SavedConnection extends SshSavedFields {
  id: string
  name: string
  /** 数据库驱动，旧数据无此字段时按 mysql 处理 */
  driver?: DriverKind
  host: string
  port: number
  database?: string
  username: string
  // 密码现在存储在后端加密存储中
  hasPassword: boolean // 标记是否有存储的密码
  createdAt: string
  updatedAt: string
  // SSL/TLS 配置（仅保存证书文件路径，旧数据无这些字段时按默认处理）
  sslMode?: SslMode
  sslCaPath?: string
  sslCertPath?: string
  sslKeyPath?: string
  /** 访问模式，旧数据无此字段时按 off 处理 */
  safetyMode?: SafetyMode
}

interface StoredPassword {
  connection_id: string
  host: string
  port: number
  username: string
  password: string
}

const STORAGE_KEY = 'mysql-client-saved-connections'

/** 统一走模块化 invoke；非 Tauri 环境（如浏览器预览）返回明确错误 */
function tauriInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!('__TAURI_INTERNALS__' in window)) {
    return Promise.reject(new Error('Not running in Tauri environment'))
  }
  return invoke<T>(cmd, args)
}

export const useSavedConnectionsStore = defineStore('savedConnections', () => {
  const savedConnections = ref<SavedConnection[]>([])
  const passwordsCache = ref<Record<string, string>>({}) // 运行时密码缓存

  // 从 localStorage 加载（连接信息，不含密码）
  function loadFromStorage() {
    try {
      const stored = localStorage.getItem(STORAGE_KEY)
      if (stored) {
        savedConnections.value = JSON.parse(stored)
      }
    } catch (e) {
      console.error('Failed to load saved connections:', e)
    }
  }

  // 保存到 localStorage
  function saveToStorage() {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(savedConnections.value))
    } catch (e) {
      console.error('Failed to save connections:', e)
    }
  }

  /**
   * 生成连接的唯一标识（用于重复检测）
   */
  function generateConnectionFingerprint(config: {
    driver?: DriverKind
    host: string
    port: number
    username: string
    database?: string
  }): string {
    const dbPart = config.database ? `/${config.database}` : ''
    // 不同驱动即使地址相同也是两个连接
    return `${config.driver ?? 'mysql'}://${config.username}@${config.host}:${config.port}${dbPart}`
  }

  /**
   * 查找已存在的相同连接
   */
  function findExistingConnection(config: {
    driver?: DriverKind
    host: string
    port: number
    username: string
    database?: string
  }): SavedConnection | undefined {
    const fingerprint = generateConnectionFingerprint(config)
    return savedConnections.value.find(conn => {
      return generateConnectionFingerprint(conn) === fingerprint
    })
  }

  /**
   * 检查是否重复
   */
  function isDuplicate(config: {
    driver?: DriverKind
    host: string
    port: number
    username: string
    database?: string
  }, excludeId?: string): boolean {
    const existing = findExistingConnection(config)
    return existing !== undefined && existing.id !== excludeId
  }

  // 添加或更新连接
  async function saveConnection(config: {
    id?: string
    name: string
    driver?: DriverKind
    host: string
    port: number
    database?: string
    username: string
    password?: string
    sslMode?: SslMode
    sslCaPath?: string
    sslCertPath?: string
    sslKeyPath?: string
    /** SSH 配置；`sshSecret` 仅在用户勾选「记住密码」时传入 */
    sshEnabled?: boolean
    sshHost?: string
    sshPort?: number
    sshUsername?: string
    sshAuthType?: SshAuthType
    sshPrivateKeyPath?: string
    sshSecret?: string
    safetyMode?: SafetyMode
  }): Promise<SavedConnection> {
    const now = new Date().toISOString()

    // 证书路径为空串时视为未配置，避免把空路径交给后端
    const ssl = {
      sslMode: config.sslMode,
      sslCaPath: config.sslCaPath?.trim() || undefined,
      sslCertPath: config.sslCertPath?.trim() || undefined,
      sslKeyPath: config.sslKeyPath?.trim() || undefined,
    }

    // SSH 字段：未启用时清空，避免残留旧配置
    const ssh = {
      sshEnabled: config.sshEnabled === true ? true : undefined,
      sshHost: config.sshEnabled ? config.sshHost?.trim() || undefined : undefined,
      sshPort: config.sshEnabled ? config.sshPort : undefined,
      sshUsername: config.sshEnabled ? config.sshUsername?.trim() || undefined : undefined,
      sshAuthType: config.sshEnabled ? config.sshAuthType : undefined,
      sshPrivateKeyPath:
        config.sshEnabled && config.sshAuthType === 'private-key'
          ? config.sshPrivateKeyPath?.trim() || undefined
          : undefined,
    }

    // 编辑已有连接：按 id 就地更新，否则会插入同 id 的重复记录且新配置丢失
    const edited = config.id
      ? savedConnections.value.find(c => c.id === config.id)
      : undefined
    if (edited) {
      const updated: SavedConnection = {
        ...edited,
        name: config.name || edited.name,
        driver: config.driver ?? edited.driver,
        host: config.host,
        port: config.port,
        database: config.database,
        username: config.username,
        hasPassword: config.password ? true : edited.hasPassword,
        updatedAt: now,
        ...ssl,
        ...ssh,
        hasSshSecret: config.sshSecret ? true : (ssh.sshEnabled ? edited.hasSshSecret : undefined),
        safetyMode: config.safetyMode ?? edited.safetyMode ?? 'off',
      }
      const index = savedConnections.value.findIndex(c => c.id === edited.id)
      savedConnections.value[index] = updated

      if (config.password) {
        await savePasswordToBackend({
          connection_id: edited.id,
          host: config.host,
          port: config.port,
          username: config.username,
          password: config.password
        })
        passwordsCache.value[edited.id] = config.password
      }

      if (config.sshSecret) {
        await saveSshSecret(edited.id, ssh.sshHost, ssh.sshPort, ssh.sshUsername, config.sshSecret)
      }

      saveToStorage()
      return updated
    }

    // 检查重复
    if (isDuplicate(config, config.id)) {
      const existing = findExistingConnection(config)!
      // 更新现有连接而不是创建新的
      const updated: SavedConnection = {
        ...existing,
        name: config.name || existing.name,
        driver: config.driver ?? existing.driver,
        updatedAt: now,
        ...ssl,
        ...ssh,
        hasSshSecret: config.sshSecret ? true : (ssh.sshEnabled ? existing.hasSshSecret : undefined),
        safetyMode: config.safetyMode ?? existing.safetyMode ?? 'off',
      }

      const index = savedConnections.value.findIndex(c => c.id === existing.id)
      if (index >= 0) {
        savedConnections.value[index] = updated
      }

      // 保存密码到后端加密存储
      if (config.password) {
        await savePasswordToBackend({
          connection_id: existing.id,
          host: config.host,
          port: config.port,
          username: config.username,
          password: config.password
        })
        passwordsCache.value[existing.id] = config.password
      }

      if (config.sshSecret) {
        await saveSshSecret(existing.id, ssh.sshHost, ssh.sshPort, ssh.sshUsername, config.sshSecret)
      }

      saveToStorage()
      return updated
    }

    // 创建新连接
    const newId = config.id || `conn_${Date.now()}`
    const newConnection: SavedConnection = {
      id: newId,
      name: config.name,
      driver: config.driver ?? 'mysql',
      host: config.host,
      port: config.port,
      database: config.database,
      username: config.username,
      hasPassword: !!config.password,
      createdAt: now,
      updatedAt: now,
      ...ssl,
      ...ssh,
      hasSshSecret: !!config.sshSecret,
      safetyMode: config.safetyMode ?? 'off',
    }

    savedConnections.value.push(newConnection)

    // 保存密码到后端加密存储
    if (config.password) {
      await savePasswordToBackend({
        connection_id: newId,
        host: config.host,
        port: config.port,
        username: config.username,
        password: config.password
      })
      passwordsCache.value[newId] = config.password
    }

    if (config.sshSecret) {
      await saveSshSecret(newId, ssh.sshHost, ssh.sshPort, ssh.sshUsername, config.sshSecret)
    }

    saveToStorage()
    return newConnection
  }

  // 保存密码到后端
  async function savePasswordToBackend(stored: StoredPassword): Promise<void> {
    try {
      await tauriInvoke('save_connection_password', {
        connectionId: stored.connection_id,
        host: stored.host,
        port: stored.port,
        username: stored.username,
        password: stored.password
      })
    } catch (e) {
      console.error('Failed to save password:', e)
      throw e
    }
  }

  /** SSH 密码/私钥口令使用独立 keyring 条目（`<id>::ssh`），避免与数据库密码混用 */
  function sshSecretKey(connectionId: string): string {
    return `${connectionId}::ssh`
  }

  // 保存 SSH 密码或私钥口令到后端加密存储
  async function saveSshSecret(
    connectionId: string,
    host: string | undefined,
    port: number | undefined,
    username: string | undefined,
    secret: string
  ): Promise<void> {
    const key = sshSecretKey(connectionId)
    await savePasswordToBackend({
      connection_id: key,
      host: host || '',
      port: port || 22,
      username: username || '',
      password: secret
    })
    passwordsCache.value[key] = secret
  }

  // 读取已保存的 SSH 密码/私钥口令
  async function getSshSecret(connectionId: string): Promise<string | null> {
    return getPassword(sshSecretKey(connectionId))
  }

  // 从后端获取密码
  async function getPassword(connectionId: string): Promise<string | null> {
    // 先检查运行时缓存
    if (passwordsCache.value[connectionId]) {
      return passwordsCache.value[connectionId]
    }

    try {
      const result = await tauriInvoke<StoredPassword | null>('get_connection_password', {
        connectionId
      })
      if (result) {
        passwordsCache.value[connectionId] = result.password
        return result.password
      }
    } catch (e) {
      console.error('Failed to get password:', e)
    }
    return null
  }

  // 根据连接信息查找密码
  async function findPassword(host: string, port: number, username: string): Promise<string | null> {
    try {
      const result = await tauriInvoke<StoredPassword | null>('find_connection_password', {
        host,
        port,
        username
      })
      if (result) {
        passwordsCache.value[result.connection_id] = result.password
        return result.password
      }
    } catch (e) {
      console.error('Failed to find password:', e)
    }
    return null
  }

  // 删除连接
  async function deleteConnection(id: string) {
    const index = savedConnections.value.findIndex(c => c.id === id)
    if (index < 0) return

    const removed = savedConnections.value[index]
    savedConnections.value.splice(index, 1)
    delete passwordsCache.value[id]
    delete passwordsCache.value[sshSecretKey(id)]
    saveToStorage()

    // 删除后端存储的密码（含 SSH 密码/口令）。失败时把记录放回并抛错，
    // 否则界面显示「已删除」但凭据仍留在 keyring 里
    try {
      await tauriInvoke('delete_connection_password', { connectionId: id })
      await tauriInvoke('delete_connection_password', { connectionId: sshSecretKey(id) })
    } catch (e) {
      if (removed) savedConnections.value.splice(index, 0, removed)
      saveToStorage()
      console.error('Failed to delete password:', e)
      throw e
    }
  }

  /** 删除后端保存的数据库密码，并清除本地「已保存密码」标记 */
  async function deletePassword(connectionId: string): Promise<void> {
    delete passwordsCache.value[connectionId]
    const conn = savedConnections.value.find(c => c.id === connectionId)
    if (conn && conn.hasPassword) {
      conn.hasPassword = false
      conn.updatedAt = new Date().toISOString()
      saveToStorage()
    }
    try {
      await tauriInvoke('delete_connection_password', { connectionId })
    } catch (e) {
      console.error('Failed to delete password:', e)
      throw e
    }
  }

  /** 删除后端保存的 SSH 密码/私钥口令，并清除本地标记 */
  async function deleteSshSecret(connectionId: string): Promise<void> {
    const key = sshSecretKey(connectionId)
    delete passwordsCache.value[key]
    const conn = savedConnections.value.find(c => c.id === connectionId)
    if (conn && conn.hasSshSecret) {
      conn.hasSshSecret = false
      conn.updatedAt = new Date().toISOString()
      saveToStorage()
    }
    try {
      await tauriInvoke('delete_connection_password', { connectionId: key })
    } catch (e) {
      console.error('Failed to delete SSH secret:', e)
      throw e
    }
  }

  // 获取连接
  function getConnection(id: string): SavedConnection | undefined {
    return savedConnections.value.find(c => c.id === id)
  }

  // 初始化加载
  loadFromStorage()

  return {
    savedConnections,
    passwordsCache,
    saveConnection,
    deleteConnection,
    deletePassword,
    deleteSshSecret,
    getConnection,
    getPassword,
    getSshSecret,
    findPassword,
    loadFromStorage,
    isDuplicate,
    findExistingConnection
  }
})
