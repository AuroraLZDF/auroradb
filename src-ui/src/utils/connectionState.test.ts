import { describe, expect, it } from 'vitest'
import infoPanelMessages from '@/i18n/messages/infoPanel'
import {
  CONNECTION_STATE_KEYS,
  CONNECTION_STATE_UNKNOWN_KEY,
  connectionStateI18nKey,
} from './connectionState'

/** 与后端 `session.rs::state_key` 一一对应；任一侧新增状态都必须同步本表 */
const BACKEND_STATES = ['disconnected', 'connecting', 'connected', 'reconnecting', 'failed']

describe('connectionStateI18nKey', () => {
  it('后端每个状态都映射到各自专属的文案 key', () => {
    for (const state of BACKEND_STATES) {
      expect(connectionStateI18nKey(state)).toBe(CONNECTION_STATE_KEYS[state])
      expect(connectionStateI18nKey(state)).not.toBe(CONNECTION_STATE_UNKNOWN_KEY)
    }
  })

  it('缺失或未知状态回退到「未知」', () => {
    expect(connectionStateI18nKey()).toBe(CONNECTION_STATE_UNKNOWN_KEY)
    expect(connectionStateI18nKey('')).toBe(CONNECTION_STATE_UNKNOWN_KEY)
    // 枚举 Debug 输出这类非约定取值同样回退，不再静默显示成「未知」以外的错误文案
    expect(connectionStateI18nKey('reconnecting { attempt: 2 }')).toBe(
      CONNECTION_STATE_UNKNOWN_KEY,
    )
  })
})

describe('状态文案双语完整性', () => {
  const keys = [...Object.values(CONNECTION_STATE_KEYS), CONNECTION_STATE_UNKNOWN_KEY]

  for (const locale of ['zh-CN', 'en-US'] as const) {
    it(`${locale} 覆盖全部状态文案`, () => {
      const dict = infoPanelMessages[locale] as Record<string, string>
      for (const key of keys) {
        expect(dict[key], `${locale} 缺少 ${key}`).toBeTruthy()
      }
    })
  }
})
