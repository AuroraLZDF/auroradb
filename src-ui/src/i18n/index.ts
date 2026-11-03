import { ref } from 'vue'

/**
 * 轻量 i18n：不引入 vue-i18n，只做「字典拼装 + 插值 + 语言切换」。
 * 每个 `messages/*.ts` 文件默认导出 `{ 'zh-CN': {...}, 'en-US': {...} }`，
 * 由 import.meta.glob 自动收集，新增组件文案时无需改动本文件。
 */

type Locale = 'zh-CN' | 'en-US'

type Dict = Record<string, string>
type MessageModule = { default: Record<Locale, Dict> }

const STORAGE_KEY = 'auroradb.locale'

const modules = import.meta.glob<MessageModule>('./messages/*.ts', { eager: true })

const messages: Record<Locale, Dict> = { 'zh-CN': {}, 'en-US': {} }
for (const mod of Object.values(modules)) {
  for (const [loc, dict] of Object.entries(mod.default)) {
    Object.assign(messages[loc as Locale], dict)
  }
}

/** 读取已保存语言；存储被禁用/不可用时返回 null，不影响启动 */
function readStoredLocale(): Locale | null {
  try {
    const saved = localStorage.getItem(STORAGE_KEY)
    return saved === 'zh-CN' || saved === 'en-US' ? saved : null
  } catch {
    // 隐私模式或沙箱环境里 localStorage 可能直接抛错
    return null
  }
}

/** 已保存语言优先，其次按系统语言推断（中文环境 zh-CN，其余 en-US） */
function detectLocale(): Locale {
  const saved = readStoredLocale()
  if (saved) return saved
  try {
    return navigator.language.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en-US'
  } catch {
    return 'zh-CN'
  }
}

/** 把当前语言同步到 `<html lang>`；单测的 node 环境没有 document */
function applyDocumentLang() {
  if (typeof document !== 'undefined') {
    document.documentElement.lang = locale.value
  }
}

export const locale = ref<Locale>(detectLocale())

export function setLocale(next: Locale) {
  locale.value = next
  try {
    localStorage.setItem(STORAGE_KEY, next)
  } catch {
    // 写失败（配额/禁用）时本次切换仍生效，只是不持久化
  }
  applyDocumentLang()
}

/** 切换语言：`zh-CN` ⇄ `en-US` */
export function toggleLocale() {
  setLocale(locale.value === 'zh-CN' ? 'en-US' : 'zh-CN')
}

/**
 * 取文案：`t('dataTable.save')`；支持 `{name}` 占位符。
 * 当前语言缺失时回落到中文，再缺失则原样返回 key，便于发现漏翻。
 */
export function t(key: string, params?: Record<string, string | number>): string {
  const text = messages[locale.value][key] ?? messages['zh-CN'][key] ?? key
  if (!params) return text
  return Object.entries(params).reduce(
    (acc, [name, value]) => acc.split(`{${name}}`).join(String(value)),
    text,
  )
}

// 模块加载即把初始语言落到 `<html lang>`，避免首屏 lang 与词典不一致
applyDocumentLang()
