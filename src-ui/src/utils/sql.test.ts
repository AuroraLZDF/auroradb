import { describe, expect, it } from 'vitest'
import { findDangerousStatements, formatSql, quoteIdentifier } from './sql'

describe('quoteIdentifier', () => {
  it('用反引号包裹标识符', () => {
    expect(quoteIdentifier('users')).toBe('`users`')
  })

  it('内部反引号双写转义', () => {
    expect(quoteIdentifier('a`b')).toBe('`a``b`')
  })

  it('PG 使用双引号并双写内部双引号', () => {
    expect(quoteIdentifier('users', 'postgres')).toBe('"users"')
    expect(quoteIdentifier('a"b', 'postgres')).toBe('"a""b"')
  })
})

describe('findDangerousStatements', () => {
  it('识别高分险语句关键字（大小写不敏感）', () => {
    expect(findDangerousStatements('drop table users')).toEqual(['DROP'])
    expect(findDangerousStatements('TRUNCATE TABLE logs;')).toEqual(['TRUNCATE'])
  })

  it('忽略字符串字面量与注释中的关键字', () => {
    expect(findDangerousStatements("INSERT INTO t VALUES ('drop table x')")).toEqual([])
    expect(findDangerousStatements('-- DROP TABLE x\nSELECT 1')).toEqual([])
    expect(findDangerousStatements('/* ALTER TABLE x */ SELECT 1')).toEqual([])
  })

  it('多语句去重返回', () => {
    expect(findDangerousStatements('DROP TABLE a; DROP TABLE b;')).toEqual(['DROP'])
  })

  it('普通查询不返回结果', () => {
    expect(findDangerousStatements('SELECT * FROM users WHERE id = 1')).toEqual([])
  })
})

describe('formatSql', () => {
  it('大写关键字并按子句换行', () => {
    expect(formatSql('select a,b from t where x=1 and y=2')).toBe(
      ['SELECT a, b', 'FROM t', 'WHERE x=1', '  AND y=2'].join('\n')
    )
  })

  it('不改动字符串字面量内的内容', () => {
    expect(formatSql("select 'from where and' as note")).toBe(
      ["SELECT 'from where and' AS note"].join('\n')
    )
  })

  it('保留注释原文与已有换行，不压成一行', () => {
    const formatted = formatSql('-- keep From here\nselect a\nfrom t')
    expect(formatted).toBe(['-- keep From here', 'SELECT a', 'FROM t'].join('\n'))
  })

  it('不改动反引号 / 双引号标识符', () => {
    expect(formatSql('select `from`, "where" from t')).toBe(
      ['SELECT `from`, "where"', 'FROM t'].join('\n')
    )
  })

  it('保留块注释与字面量中的逗号', () => {
    expect(formatSql("/* a, b */ select 'x,y' from t")).toBe(
      ['/* a, b */', "SELECT 'x,y'", 'FROM t'].join('\n')
    )
  })
})
