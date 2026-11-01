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

function detectLocale(): Locale {
  const saved = localStorage.getItem(STORAGE_KEY)
  if (saved === 'zh-CN' || saved === 'en-US') return saved
  return navigator.language.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en-US'
}

export const locale = ref<Locale>(detectLocale())

function setLocale(next: Locale) {
  locale.value = next
  localStorage.setItem(STORAGE_KEY, next)
  document.documentElement.lang = next
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

// 单测在 node 环境导入本模块时没有 document，仅在浏览器环境下同步 <html lang>
if (typeof document !== 'undefined') {
  document.documentElement.lang = locale.value
}
