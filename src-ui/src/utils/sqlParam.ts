/**
 * 仪表盘卡片 SQL 参数化（V3-1.4）。
 * 支持 `:name` 具名参数；替换时按方言做安全转义——字符串双写引号、
 * MySQL 额外转义反斜杠、数值/布尔直传，杜绝拼接注入。
 * TODO(V3-2)：若后续做 AI 助手取数，可将绑定下沉到后端 prepared statement。
 */

/** 提取 SQL 中的具名参数（排除 `::` 类型转换与 `:=` 赋值），保持出现顺序去重 */
export function extractSqlParams(sql: string): string[] {
  const re = /(?<![:\w]):([A-Za-z_]\w*)/g
  const seen = new Set<string>()
  const names: string[] = []
  for (const m of sql.matchAll(re)) {
    const name = m[1]
    if (name && !seen.has(name)) {
      seen.add(name)
      names.push(name)
    }
  }
  return names
}

/** 方言感知的值转义为 SQL 字面量 */
function quoteValue(v: string, dialect: string): string {
  const trimmed = v.trim()
  // 数值与布尔直传，其余按字符串转义
  if (/^-?\d+(\.\d+)?$/.test(trimmed) || /^(true|false)$/i.test(trimmed)) return trimmed
  const escaped = dialect.toLowerCase() === 'mysql'
    ? trimmed.replaceAll('\\', '\\\\').replaceAll("'", "''")
    : trimmed.replaceAll("'", "''")
  return `'${escaped}'`
}

/** 将 `:name` 替换为转义后的字面量；无值的参数替换为 NULL */
export function substituteParams(sql: string, values: Record<string, string>, dialect: string): string {
  return sql.replace(/(?<![:\w]):([A-Za-z_]\w*)/g, (raw, name: string) => {
    const v = values[name]
    return v !== undefined && v.trim() !== '' ? quoteValue(v, dialect) : 'NULL'
  })
}
