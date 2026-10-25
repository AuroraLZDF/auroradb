import { describe, expect, it } from 'vitest'
import { findDangerousStatements, quoteIdentifier } from './sql'

describe('quoteIdentifier', () => {
  it('用反引号包裹标识符', () => {
    expect(quoteIdentifier('users')).toBe('`users`')
  })

  it('内部反引号双写转义', () => {
    expect(quoteIdentifier('a`b')).toBe('`a``b`')
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
