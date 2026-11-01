import { describe, expect, it } from 'vitest'
import { baseName, isSqlFilePath } from './sqlFile'

describe('isSqlFilePath', () => {
  it('识别 .sql 与大小写混合的扩展名', () => {
    expect(isSqlFilePath('/tmp/a.sql')).toBe(true)
    expect(isSqlFilePath('C:\\data\\export.SQL')).toBe(true)
  })

  it('忽略非 SQL 文件与空白', () => {
    expect(isSqlFilePath('/tmp/a.txt')).toBe(false)
    expect(isSqlFilePath('/tmp/sql')).toBe(false)
    expect(isSqlFilePath('  ')).toBe(false)
  })
})

describe('baseName', () => {
  it('取 unix 与 windows 路径的文件名', () => {
    expect(baseName('/home/user/query.sql')).toBe('query.sql')
    expect(baseName('C:\\Users\\me\\query.sql')).toBe('query.sql')
  })

  it('路径为空时原样返回', () => {
    expect(baseName('')).toBe('')
  })
})
