import { describe, expect, it } from 'vitest'
import { buildWhereClause, collectTableChanges, formatSqlValue } from './tableChanges'

const columns = [
  { name: 'id', is_primary: true },
  { name: 'name', is_primary: false },
  { name: 'email', is_primary: false }
]

describe('formatSqlValue', () => {
  it('把 null / undefined 转成 NULL', () => {
    expect(formatSqlValue(null)).toBe('NULL')
    expect(formatSqlValue(undefined)).toBe('NULL')
  })

  it('单引号按 SQL 标准双写', () => {
    expect(formatSqlValue("O'Brien")).toBe("'O''Brien'")
  })

  it('反斜杠先于单引号转义', () => {
    expect(formatSqlValue('a\\b')).toBe("'a\\\\b'")
    expect(formatSqlValue('a\\\'b')).toBe("'a\\\\''b'")
  })

  it('布尔转 1 / 0，数字原样输出', () => {
    expect(formatSqlValue(true)).toBe('1')
    expect(formatSqlValue(false)).toBe('0')
    expect(formatSqlValue(42)).toBe('42')
  })

  it('二进制列把 Base64 还原成字节字面量', () => {
    // 直接写 Base64 文本会污染 BLOB/BYTEA 列，必须解码
    expect(formatSqlValue('AQID', { data_type: 'blob' })).toBe("FROM_BASE64('AQID')")
    expect(formatSqlValue('AQID', { data_type: 'bytea', driver: 'postgres' }))
      .toBe("decode('AQID', 'base64')")
  })

  it('PG 只翻倍单引号，不转义反斜杠', () => {
    expect(formatSqlValue('a\\b', { driver: 'postgres' })).toBe("'a\\b'")
  })
})

describe('buildWhereClause', () => {
  it('单主键生成等值条件', () => {
    expect(buildWhereClause(columns, [1, 'Ada', 'ada@example.com'])).toBe('`id` = 1')
  })

  it('复合主键用 AND 连接', () => {
    const composite = [
      { name: 'tenant_id', is_primary: true },
      { name: 'id', is_primary: true },
      { name: 'name', is_primary: false }
    ]
    expect(buildWhereClause(composite, [7, 21, 'x'])).toBe('`tenant_id` = 7 AND `id` = 21')
  })

  it('主键值为 NULL 时用 IS NULL', () => {
    expect(buildWhereClause(columns, [null, 'Ada', ''])).toBe('`id` IS NULL')
  })

  it('无主键返回空串（不允许编辑）', () => {
    const noPk = [
      { name: 'name', is_primary: false },
      { name: 'email', is_primary: false }
    ]
    expect(buildWhereClause(noPk, ['Ada', 'ada@example.com'])).toBe('')
  })

  it('主键名中的反引号被转义', () => {
    const odd = [{ name: 'we`ird', is_primary: true }]
    expect(buildWhereClause(odd, ['v'])).toBe('`we``ird` = \'v\'')
  })
})

describe('collectTableChanges', () => {
  it('同一行的多个单元格编辑合并为一条 UPDATE', () => {
    const changes = collectTableChanges({
      table: 'users',
      columns,
      edits: [
        { rowIndex: 0, colIndex: 1, oldRowData: [1, 'Ada', 'ada@example.com'], newValue: 'Ada L.' },
        { rowIndex: 0, colIndex: 2, oldRowData: [1, 'Ada', 'ada@example.com'], newValue: 'ada@new.io' }
      ],
      inserts: [],
      deletes: []
    })

    expect(changes).toHaveLength(1)
    expect(changes[0]).toMatchObject({
      kind: 'update',
      table: 'users',
      where_clause: '`id` = 1'
    })
    expect(changes[0]!.sql).toBe("UPDATE `users` SET `name` = 'Ada L.', `email` = 'ada@new.io' WHERE `id` = 1")
  })

  it('不同行的编辑分别生成 UPDATE', () => {
    const changes = collectTableChanges({
      table: 'users',
      columns,
      edits: [
        { rowIndex: 0, colIndex: 1, oldRowData: [1, 'Ada', ''], newValue: 'A' },
        { rowIndex: 1, colIndex: 1, oldRowData: [2, 'Bob', ''], newValue: 'B' }
      ],
      inserts: [],
      deletes: []
    })

    expect(changes.map(c => c.where_clause)).toEqual(['`id` = 1', '`id` = 2'])
  })

  it('无主键时跳过 UPDATE 与 DELETE', () => {
    const noPk = [
      { name: 'name', is_primary: false },
      { name: 'email', is_primary: false }
    ]
    const changes = collectTableChanges({
      table: 'logs',
      columns: noPk,
      edits: [{ rowIndex: 0, colIndex: 0, oldRowData: ['a', 'b'], newValue: 'c' }],
      inserts: [],
      deletes: [['a', 'b']]
    })

    expect(changes).toEqual([])
  })

  it('生成 INSERT 与 DELETE 语句', () => {
    const changes = collectTableChanges({
      table: 'users',
      columns,
      edits: [],
      inserts: [[3, 'Cy', null]],
      deletes: [[4, 'Di', 'di@example.com']]
    })

    expect(changes).toHaveLength(2)
    expect(changes[0]).toMatchObject({ kind: 'insert', table: 'users' })
    expect(changes[0]!.sql).toBe("INSERT INTO `users` (`id`, `name`, `email`) VALUES (3, 'Cy', NULL)")
    expect(changes[1]).toMatchObject({ kind: 'delete', where_clause: '`id` = 4' })
    expect(changes[1]!.sql).toBe('DELETE FROM `users` WHERE `id` = 4')
  })

  it('PG 下 schema.table 表名按段引用（UPDATE / INSERT / DELETE）', () => {
    const changes = collectTableChanges({
      table: 'crm.orders',
      columns,
      edits: [{ rowIndex: 0, colIndex: 1, oldRowData: [1, 'Ada', ''], newValue: 'B' }],
      inserts: [[3, 'Cy', null]],
      deletes: [[4, 'Di', 'di@example.com']],
      driver: 'postgres'
    })

    expect(changes.map(c => c.sql)).toEqual([
      'UPDATE "crm"."orders" SET "name" = \'B\' WHERE "id" = 1',
      'INSERT INTO "crm"."orders" ("id", "name", "email") VALUES (3, \'Cy\', NULL)',
      'DELETE FROM "crm"."orders" WHERE "id" = 4'
    ])
  })

  it('MySQL 下多段表名仍按单个标识符引用', () => {
    const changes = collectTableChanges({
      table: 'crm.orders',
      columns,
      edits: [],
      inserts: [],
      deletes: [[1, 'Ada', '']]
    })

    expect(changes[0]!.sql).toBe('DELETE FROM `crm.orders` WHERE `id` = 1')
  })

  it('与原始值等值的编辑不生成 SET（数字与字符串视为相同）', () => {
    const changes = collectTableChanges({
      table: 'users',
      columns,
      edits: [
        { rowIndex: 0, colIndex: 0, oldRowData: [1, 'Ada', ''], newValue: '1' },
        { rowIndex: 0, colIndex: 1, oldRowData: [1, 'Ada', ''], newValue: 'Ada' },
        { rowIndex: 0, colIndex: 2, oldRowData: [1, 'Ada', ''], newValue: 'ada@new.io' }
      ],
      inserts: [],
      deletes: []
    })

    expect(changes).toHaveLength(1)
    expect(changes[0]!.sql).toBe("UPDATE `users` SET `email` = 'ada@new.io' WHERE `id` = 1")
  })

  it('整行都没有实际变化时不生成 UPDATE', () => {
    const changes = collectTableChanges({
      table: 'users',
      columns,
      edits: [
        { rowIndex: 0, colIndex: 1, oldRowData: [1, 'Ada', ''], newValue: 'Ada' },
        { rowIndex: 0, colIndex: 2, oldRowData: [1, 'Ada', ''], newValue: '' }
      ],
      inserts: [],
      deletes: []
    })

    expect(changes).toEqual([])
  })

  it('缺少原始行数据的编辑被忽略', () => {
    const changes = collectTableChanges({
      table: 'users',
      columns,
      edits: [{ rowIndex: 0, colIndex: 1, newValue: 'x' }],
      inserts: [],
      deletes: []
    })

    expect(changes).toEqual([])
  })
})
