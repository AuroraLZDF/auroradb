import type { DriverKind } from '@/stores/savedConnections'

/**
 * 按方言安全地引用标识符（表名 / 列名 / 库名 / 索引名 / 外键名等）。
 *
 * - MySQL 用反引号，标识符内的反引号翻倍：quoteIdentifier('a`b') === '`a``b`'
 * - PostgreSQL 用双引号，标识符内的双引号翻倍：quoteIdentifier('a"b', 'postgres') === '"a""b"'
 *
 * PG 不接受反引号做标识符引用，混用会导致 DDL/DML 直接语法错误。
 */
export function quoteIdentifier(identifier: string, driver: DriverKind = 'mysql'): string {
  if (driver === 'postgres') {
    return '"' + identifier.replace(/"/g, '""') + '"'
  }
  return '`' + identifier.replace(/`/g, '``') + '`'
}

/**
 * 按方言引用「可能带 schema 前缀」的表名。
 *
 * - PostgreSQL：对象树里的表名是 `schema.table`，必须分段引用，
 *   否则会生成 `"crm.orders"`（被 PG 当成一个带点的表名，报 relation does not exist）。
 *   最多拆成两段：`a.b.c` 视为 schema=`a`、table=`b.c`。
 * - MySQL：保持整串引用（`` `crm.orders` ``），不改变既有行为。
 */
export function quoteQualifiedIdentifier(name: string, driver: DriverKind = 'mysql'): string {
  if (driver !== 'postgres') return quoteIdentifier(name, driver)
  const dot = name.indexOf('.')
  if (dot <= 0 || dot === name.length - 1) return quoteIdentifier(name, driver)
  const schema = name.slice(0, dot)
  const table = name.slice(dot + 1)
  return `${quoteIdentifier(schema, driver)}.${quoteIdentifier(table, driver)}`
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

/** 需要统一大写的关键字 */
const FORMAT_KEYWORDS = [
  'SELECT', 'FROM', 'WHERE', 'GROUP BY', 'ORDER BY', 'HAVING', 'LIMIT', 'OFFSET',
  'UNION ALL', 'UNION', 'LEFT OUTER JOIN', 'RIGHT OUTER JOIN', 'FULL OUTER JOIN',
  'LEFT JOIN', 'RIGHT JOIN', 'INNER JOIN', 'OUTER JOIN', 'CROSS JOIN', 'FULL JOIN',
  'JOIN', 'ON', 'AND', 'OR', 'NOT', 'INSERT INTO', 'VALUES', 'UPDATE', 'SET',
  'DELETE FROM', 'CREATE TABLE', 'ALTER TABLE', 'DROP TABLE', 'TRUNCATE TABLE',
  'DISTINCT', 'AS', 'IN', 'IS', 'NULL', 'LIKE', 'BETWEEN',
]

/** 需要另起一行的子句关键字（长关键字在前，避免被短关键字拆开） */
const CLAUSE_KEYWORDS = [
  'LEFT OUTER JOIN', 'RIGHT OUTER JOIN', 'FULL OUTER JOIN', 'LEFT JOIN',
  'RIGHT JOIN', 'INNER JOIN', 'OUTER JOIN', 'CROSS JOIN', 'FULL JOIN',
  'GROUP BY', 'ORDER BY', 'UNION ALL', 'UNION', 'SELECT', 'FROM', 'WHERE',
  'HAVING', 'LIMIT', 'OFFSET', 'JOIN', 'ON', 'INSERT INTO', 'VALUES', 'UPDATE',
  'SET', 'DELETE FROM', 'CREATE TABLE', 'ALTER TABLE', 'DROP TABLE',
  'TRUNCATE TABLE',
]

/** 占位符分隔符：NUL 不会出现在正常 SQL 文本中 */
const PLACEHOLDER = '\u0000'

function keywordPattern(keywords: string[]): string {
  return keywords
    .slice()
    .sort((a, b) => b.length - a.length)
    .map(keyword => keyword.replace(/ /g, '\\s+'))
    .join('|')
}

const KEYWORD_RE = new RegExp(`\\b(?:${keywordPattern(FORMAT_KEYWORDS)})\\b`, 'gi')
const CLAUSE_RE = new RegExp(`\\b(?:${keywordPattern(CLAUSE_KEYWORDS)})\\b`, 'g')

/**
 * 把字符串字面量、引用标识符与注释替换成占位符。
 *
 * 格式化只应作用于代码部分：直接对整段文本做大小写/空白替换会改坏字面量与注释内容。
 */
function maskSql(sql: string): { masked: string; parts: string[] } {
  const parts: string[] = []
  let masked = ''

  const mask = (text: string) => {
    masked += `${PLACEHOLDER}${parts.length}${PLACEHOLDER}`
    parts.push(text)
  }

  for (let i = 0; i < sql.length; i++) {
    const ch = sql[i]
    const next = sql[i + 1]

    if (ch === "'" || ch === '"' || ch === '`') {
      const start = i
      i++
      while (i < sql.length) {
        if (sql[i] === '\\' && ch !== '`') {
          i++
          continue
        }
        if (sql[i] === ch) {
          if (sql[i + 1] === ch) {
            i++
            continue
          }
          break
        }
        i++
      }
      mask(sql.slice(start, i + 1))
      continue
    }

    if ((ch === '-' && next === '-') || ch === '#') {
      const start = i
      while (i < sql.length && sql[i] !== '\n') i++
      // 循环结束时会再 i++，这里回退一格避免吞掉换行
      i--
      mask(sql.slice(start, i + 1))
      continue
    }

    if (ch === '/' && next === '*') {
      const start = i
      const end = sql.indexOf('*/', i + 2)
      i = end === -1 ? sql.length - 1 : end + 1
      mask(sql.slice(start, i + 1))
      continue
    }

    masked += ch
  }

  return { masked, parts }
}

/**
 * 格式化 SQL：统一大写关键字、按子句换行，同时保留字符串字面量、引用标识符、
 * 注释与已有换行的原文，不会把多行脚本压成一行。
 */
export function formatSql(sql: string): string {
  if (!sql.trim()) return sql

  const { masked, parts } = maskSql(sql)

  let formatted = masked.replace(KEYWORD_RE, match => match.toUpperCase())
  // 行内空白压成一个空格；换行由子句与语句分隔规则统一生成
  formatted = formatted.replace(/[ \t]+/g, ' ')
  formatted = formatted.replace(/[ \t]*,[ \t]*/g, ', ')
  formatted = formatted.replace(/;[ \t]*/g, ';\n')
  formatted = formatted.replace(CLAUSE_RE, match => `\n${match}`)
  // AND / OR 缩进一级
  formatted = formatted.replace(/[ \t]+(AND|OR)\b/g, '\n  $1')

  // 清理行尾空白与多余空行
  formatted = formatted
    .replace(/[ \t]+\n/g, '\n')
    .replace(/\n(?:[ \t]*\n)+/g, '\n')
    .replace(/^\n+/, '')
    .replace(/\n+$/, '')

  return formatted.replace(
    new RegExp(`${PLACEHOLDER}(\\d+)${PLACEHOLDER}`, 'g'),
    (_, index: string) => parts[Number(index)] ?? ''
  )
}
