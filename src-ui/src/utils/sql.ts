/**
 * 将 MySQL 标识符（表名 / 列名 / 库名 / 索引名 / 外键名等）安全地用反引号包裹，
 * 并对标识符内部的反引号进行翻倍转义（` → ``）。
 *
 * MySQL 以反引号作为标识符引用符，标识符内若含反引号须写成两个反引号转义。
 * 例：quoteIdentifier('a`b') === '`a``b`'
 */
export function quoteIdentifier(identifier: string): string {
  return '`' + identifier.replace(/`/g, '``') + '`'
}

/** 安全模式下需要二次确认的高风险语句关键字 */
const DANGEROUS_KEYWORDS = ['DROP', 'TRUNCATE', 'ALTER', 'RENAME', 'GRANT', 'REVOKE']

/**
 * 找出 SQL 中需要二次确认的高风险语句。
 * 仅做关键字级别的轻量识别（跳过注释与字符串字面量），用于前端提示；
 * 真正的拦截由后端安全模式兜底。
 */
export function findDangerousStatements(sql: string): string[] {
  const found = new Set<string>()
  let current = ''
  let quote: string | null = null

  const flush = () => {
    const keyword = (current.trim().split(/[\s(]+/, 1)[0] || '').toUpperCase()
    if (DANGEROUS_KEYWORDS.includes(keyword)) found.add(keyword)
    current = ''
  }

  for (let i = 0; i < sql.length; i++) {
    const ch = sql[i]

    if (quote) {
      if (ch === '\\') i++
      else if (ch === quote) {
        if (sql[i + 1] === quote) i++
        else quote = null
      }
      continue
    }

    if (ch === "'" || ch === '"' || ch === '`') {
      quote = ch
      continue
    }
    if (ch === '-' && sql[i + 1] === '-') {
      while (i < sql.length && sql[i] !== '\n') i++
      continue
    }
    if (ch === '#') {
      while (i < sql.length && sql[i] !== '\n') i++
      continue
    }
    if (ch === '/' && sql[i + 1] === '*') {
      i += 2
      while (i < sql.length && !(sql[i] === '*' && sql[i + 1] === '/')) i++
      i++
      continue
    }
    if (ch === ';') {
      flush()
      continue
    }
    current += ch
  }
  flush()

  return [...found]
}
