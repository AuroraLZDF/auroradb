/**
 * 后端 `ConnectionStatus.state` 的取值集合 → 展示用 i18n key。
 *
 * state 由后端 `session.rs::state_key` 生成，必须是稳定的小写单词；
 * 两边任一侧新增状态时，本表与 `infoPanel.state.*` 文案都要同步补齐。
 */
export const CONNECTION_STATE_KEYS: Record<string, string> = {
  connected: 'infoPanel.state.connected',
  connecting: 'infoPanel.state.connecting',
  disconnected: 'infoPanel.state.disconnected',
  reconnecting: 'infoPanel.state.reconnecting',
  failed: 'infoPanel.state.failed',
}

export const CONNECTION_STATE_UNKNOWN_KEY = 'infoPanel.state.unknown'

/** 把后端 state 映射为 i18n key；空值/未知取值回退到「未知」 */
export function connectionStateI18nKey(state?: string): string {
  if (!state) return CONNECTION_STATE_UNKNOWN_KEY
  return CONNECTION_STATE_KEYS[state] ?? CONNECTION_STATE_UNKNOWN_KEY
}
