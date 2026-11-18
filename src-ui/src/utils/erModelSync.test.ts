import { describe, expect, it } from 'vitest'
import { buildModelSyncPlan, collectStatements, type DbSnapshot } from './erModelSync'
import type { ErModel, ModelColumn, ModelRelation, ModelTable } from '@/stores/erModel'
import type { DriverKind } from './driver'

function col(partial: Partial<ModelColumn> & { name: string }): ModelColumn {
  return {
    id: partial.name,
    type: 'varchar(255)',
    pk: false,
    nullable: true,
    unique: false,
    ...partial,
  }
}

function table(partial: Partial<ModelTable> & { name: string; columns: ModelColumn[] }): ModelTable {
  return {
    id: partial.name,
    x: 0,
    y: 0,
    ...partial,
  }
}

function relation(partial: Partial<ModelRelation>): ModelRelation {
  return {
    id: 'rel',
    fromTableId: '',
    fromColumnId: '',
    toTableId: '',
    toColumnId: '',
    ...partial,
  }
}

function model(partial: Partial<ErModel> & { tables: ModelTable[] }): ErModel {
  return {
    id: 'm',
    name: 'Model',
    relations: [],
    updatedAt: 0,
    ...partial,
  }
}

function dbCol(partial: { name: string } & Partial<DbSnapshot['tables'][number]['columns'][number]>):
  DbSnapshot['tables'][number]['columns'][number] {
  return {
    data_type: 'varchar',
    column_type: 'varchar(255)',
    is_primary: false,
    nullable: true,
    ...partial,
  }
}

function snapshot(partial: Partial<DbSnapshot> & {
  tables?: DbSnapshot['tables'],
  foreign_keys?: DbSnapshot['foreign_keys'],
} = {}): DbSnapshot {
  return {
    driver: 'mysql' as DriverKind,
    database: 'app',
    tables: [],
    foreign_keys: [],
    ...partial,
  }
}

describe('buildModelSyncPlan：新表', () => {
  it('模型有、库没有的表按拓扑序生成 CREATE', () => {
    const m = model({
      tables: [
        table({
          name: 'orders',
          columns: [col({ name: 'id', type: 'bigint', pk: true, nullable: false }), col({ name: 'user_id', type: 'bigint' })],
        }),
        table({ name: 'users', columns: [col({ name: 'id', type: 'bigint', pk: true, nullable: false })] }),
      ],
      relations: [relation({
        id: 'r1',
        fromTableId: 'orders',
        fromColumnId: 'user_id',
        toTableId: 'users',
        toColumnId: 'id',
      })],
    })
    const plan = buildModelSyncPlan(m, snapshot())
    expect(plan.items).toHaveLength(2)
    expect(plan.items[0]!.kind).toBe('create-table')
    expect(plan.items[0]!.tableName).toBe('users')
    expect(plan.items[1]!.tableName).toBe('orders')
    // 外键随 CREATE 内联，不再生成 add-fk
    const sql = collectStatements(plan.items).join('\n')
    expect(sql).toContain('CREATE TABLE `users`')
    expect(sql).toContain('FOREIGN KEY (`user_id`) REFERENCES `users`(`id`)')
    expect(plan.items.filter((i) => i.kind === 'add-fk')).toHaveLength(0)
  })

  it('PG 方言生成 CONSTRAINT 主键与 COMMENT ON', () => {
    const m = model({
      tables: [table({
        name: 'users',
        comment: '用户表',
        columns: [col({ name: 'id', type: 'bigint', pk: true, nullable: false, comment: '主键' })],
      })],
    })
    const plan = buildModelSyncPlan(m, snapshot({ driver: 'postgres' }))
    const sql = collectStatements(plan.items).join('\n')
    expect(sql).toContain('CREATE TABLE "users"')
    expect(sql).toContain('CONSTRAINT "users_pkey" PRIMARY KEY ("id")')
    expect(sql).toContain('COMMENT ON COLUMN "users"."id" IS \'主键\'')
    expect(sql).toContain('COMMENT ON TABLE "users" IS \'用户表\'')
    // 语句不带分号（apply_schema_diff 逐条执行）
    for (const s of collectStatements(plan.items)) {
      expect(s.endsWith(';')).toBe(false)
    }
  })

  it('唯一字段随建表生成 UNIQUE INDEX', () => {
    const m = model({
      tables: [table({
        name: 'users',
        columns: [col({ name: 'id', pk: true }), col({ name: 'email', unique: true })],
      })],
    })
    const plan = buildModelSyncPlan(m, snapshot())
    const sql = collectStatements(plan.items).join('\n')
    expect(sql).toContain('UNIQUE INDEX `users_email_uk` (`email`)')
  })
})

describe('buildModelSyncPlan：列差异', () => {
  const users = table({
    name: 'users',
    columns: [
      col({ name: 'id', type: 'bigint', pk: true, nullable: false }),
      col({ name: 'age', type: 'int', nullable: false, default: '0' }),
      col({ name: 'nickname', type: 'varchar(32)', comment: '昵称' }),
    ],
  })
  const dbUsers = {
    name: 'users',
    columns: [
      dbCol({ name: 'id', data_type: 'bigint', column_type: 'bigint', is_primary: true, nullable: false }),
      // age 在库里类型不同且可空、无默认值 → alter-column
      dbCol({ name: 'age', data_type: 'int', column_type: 'int', nullable: true }),
      // nickname 在库里没有 → add-column
    ],
  }

  it('类型/非空/默认值差异生成 MODIFY COLUMN（MySQL）', () => {
    const plan = buildModelSyncPlan(model({ tables: [users] }), snapshot({ tables: [dbUsers] }))
    const alter = plan.items.find((i) => i.kind === 'alter-column')
    expect(alter?.tableName).toBe('users')
    expect(alter?.columnName).toBe('age')
    expect(collectStatements([alter!]).join('\n')).toContain(
      'ALTER TABLE `users` MODIFY COLUMN `age` int NOT NULL DEFAULT 0'
    )
    expect(plan.items.find((i) => i.kind === 'add-column')?.columnName).toBe('nickname')
  })

  it('add-column 带注释（MySQL 内联 COMMENT）', () => {
    const plan = buildModelSyncPlan(model({ tables: [users] }), snapshot({ tables: [dbUsers] }))
    const add = plan.items.find((i) => i.kind === 'add-column')!
    expect(collectStatements([add]).join('\n')).toContain("`nickname` varchar(32) COMMENT '昵称'")
  })

  it('add-column 的注释在 PG 走 COMMENT ON', () => {
    const pgUsers = {
      name: 'users',
      columns: [
        dbCol({ name: 'id', data_type: 'bigint', column_type: 'bigint', is_primary: true, nullable: false }),
      ],
    }
    const plan = buildModelSyncPlan(model({ tables: [users] }), snapshot({ driver: 'postgres', tables: [pgUsers] }))
    const add = plan.items.find((i) => i.kind === 'add-column' && i.columnName === 'nickname')!
    const sql = collectStatements([add]).join('\n')
    expect(sql).toContain('ADD COLUMN "nickname" varchar(32)')
    expect(sql).toContain('COMMENT ON COLUMN "users"."nickname" IS \'昵称\'')
  })

  it('PG 的 format_type 输出还原后与模型类型一致则不生成 ALTER', () => {
    const pgUsers = {
      name: 'users',
      columns: [
        dbCol({ name: 'id', data_type: 'int8', column_type: 'bigint', is_primary: true, nullable: false }),
        dbCol({
          name: 'nickname',
          data_type: 'character varying(32)',
          column_type: 'character varying(32)',
          nullable: true,
          comment: '昵称',
        }),
      ],
    }
    const plan = buildModelSyncPlan(model({ tables: [users] }), snapshot({ driver: 'postgres', tables: [pgUsers] }))
    expect(plan.items.filter((i) => i.kind === 'alter-column')).toHaveLength(0)
  })

  it('库有、模型没有的列默认不删，开启选项后生成 DROP COLUMN', () => {
    const dbWithExtra = {
      name: 'users',
      columns: [
        ...dbUsers.columns,
        dbCol({ name: 'legacy', data_type: 'int', column_type: 'int', nullable: true }),
      ],
    }
    const noDrop = buildModelSyncPlan(model({ tables: [users] }), snapshot({ tables: [dbWithExtra] }))
    expect(noDrop.items.filter((i) => i.kind === 'drop-column')).toHaveLength(0)

    const withDrop = buildModelSyncPlan(model({ tables: [users] }), snapshot({ tables: [dbWithExtra] }), {
      dropMissingColumns: true,
    })
    const drop = withDrop.items.find((i) => i.kind === 'drop-column')
    expect(drop?.columnName).toBe('legacy')
    expect(collectStatements([drop!])[0]).toBe('ALTER TABLE `users` DROP COLUMN `legacy`')
  })

  it('库有、模型没有的表默认不删，开启选项后生成 DROP TABLE IF EXISTS', () => {
    const dbTables = [dbUsers, { name: 'legacy_log', columns: [] }]
    const noDrop = buildModelSyncPlan(model({ tables: [users] }), snapshot({ tables: dbTables }))
    expect(noDrop.items.filter((i) => i.kind === 'drop-table')).toHaveLength(0)

    const withDrop = buildModelSyncPlan(model({ tables: [users] }), snapshot({ tables: dbTables }), {
      dropMissingTables: true,
    })
    const drop = withDrop.items.find((i) => i.kind === 'drop-table')
    expect(drop?.tableName).toBe('legacy_log')
    expect(collectStatements([drop!])[0]).toBe('DROP TABLE IF EXISTS `legacy_log`')
  })
})

describe('buildModelSyncPlan：外键', () => {
  const users = table({ name: 'users', columns: [col({ name: 'id', type: 'bigint', pk: true, nullable: false })] })
  const orders = table({
    name: 'orders',
    columns: [col({ name: 'id', pk: true }), col({ name: 'user_id', type: 'bigint' })],
  })

  it('两边都有表但库缺外键 → add-fk，命名避开已有约束', () => {
    const dbTables = [
      { name: 'users', columns: [dbCol({ name: 'id', data_type: 'bigint', column_type: 'bigint', is_primary: true, nullable: false })] },
      {
        name: 'orders',
        columns: [
          dbCol({ name: 'id', data_type: 'int', column_type: 'int', is_primary: true, nullable: false }),
          dbCol({ name: 'user_id', data_type: 'bigint', column_type: 'bigint', nullable: true }),
        ],
      },
    ]
    const m = model({
      tables: [users, orders],
      relations: [relation({ id: 'r1', fromTableId: 'orders', fromColumnId: 'user_id', toTableId: 'users', toColumnId: 'id' })],
    })

    const plan = buildModelSyncPlan(m, snapshot({ tables: dbTables }))
    const fk = plan.items.find((i) => i.kind === 'add-fk')
    expect(fk?.tableName).toBe('orders')
    expect(collectStatements([fk!])[0]).toContain(
      'ALTER TABLE `orders` ADD CONSTRAINT `orders_users_fk` FOREIGN KEY (`user_id`) REFERENCES `users`(`id`)'
    )

    // 库里已有相同外键 → 不再生成
    const plan2 = buildModelSyncPlan(m, snapshot({
      tables: dbTables,
      foreign_keys: [{ name: 'orders_users_fk', table: 'orders', column: 'user_id', ref_table: 'users', ref_column: 'id' }],
    }))
    expect(plan2.items.filter((i) => i.kind === 'add-fk')).toHaveLength(0)
  })

  it('一端表是新建表时外键不单独生成（已随 CREATE 内联）', () => {
    const dbTables = [
      { name: 'users', columns: [dbCol({ name: 'id', data_type: 'bigint', column_type: 'bigint', is_primary: true, nullable: false })] },
    ]
    const m = model({
      tables: [users, orders],
      relations: [relation({ id: 'r1', fromTableId: 'orders', fromColumnId: 'user_id', toTableId: 'users', toColumnId: 'id' })],
    })
    const plan = buildModelSyncPlan(m, snapshot({ tables: dbTables }))
    expect(plan.items.filter((i) => i.kind === 'add-fk')).toHaveLength(0)
  })

  it('同一对表的多条缺失关系用序号命名', () => {
    const users2 = table({ name: 'users', columns: [col({ name: 'id', pk: true }), col({ name: 'pid' })] })
    const orders2 = table({ name: 'orders', columns: [col({ name: 'a' }), col({ name: 'b' })] })
    const dbTables = [
      { name: 'users', columns: [dbCol({ name: 'id' }), dbCol({ name: 'pid' })] },
      { name: 'orders', columns: [dbCol({ name: 'a' }), dbCol({ name: 'b' })] },
    ]
    const m = model({
      tables: [users2, orders2],
      relations: [
        relation({ id: 'r1', fromTableId: 'orders', fromColumnId: 'a', toTableId: 'users', toColumnId: 'id' }),
        relation({ id: 'r2', fromTableId: 'orders', fromColumnId: 'b', toTableId: 'users', toColumnId: 'pid' }),
      ],
    })
    const plan = buildModelSyncPlan(m, snapshot({ tables: dbTables }))
    const fks = plan.items.filter((i) => i.kind === 'add-fk')
    expect(fks).toHaveLength(2)
    const sql = collectStatements(fks).join('\n')
    expect(sql).toContain('`orders_users_fk` FOREIGN KEY (`a`)')
    expect(sql).toContain('`orders_users_fk_2` FOREIGN KEY (`b`)')
  })
})

describe('无差异', () => {
  it('完全一致时计划为空', () => {
    const m = model({
      tables: [table({ name: 'users', columns: [col({ name: 'id', type: 'bigint', pk: true, nullable: false })] })],
    })
    const plan = buildModelSyncPlan(m, snapshot({
      tables: [{
        name: 'users',
        columns: [dbCol({ name: 'id', data_type: 'bigint', column_type: 'bigint', is_primary: true, nullable: false })],
      }],
      foreign_keys: [],
    }))
    expect(plan.items).toHaveLength(0)
  })

  it('类型大小写与括号空格差异不算变更', () => {
    const m = model({
      tables: [table({ name: 'users', columns: [col({ name: 'name', type: 'VARCHAR( 64 )' })] })],
    })
    const plan = buildModelSyncPlan(m, snapshot({
      tables: [{
        name: 'users',
        columns: [dbCol({ name: 'name', data_type: 'varchar', column_type: 'varchar(64)', nullable: true })],
      }],
    }))
    expect(plan.items).toHaveLength(0)
  })
})

describe('collectStatements', () => {
  it('按计划顺序拼接所有语句', () => {
    const items = [
      { kind: 'create-table' as const, tableName: 'a', statements: ['CREATE TABLE a', 'ALTER TABLE a COMMENT'] },
      { kind: 'add-column' as const, tableName: 'b', columnName: 'c', statements: ['ALTER TABLE b ADD COLUMN c'] },
    ]
    expect(collectStatements(items)).toEqual(['CREATE TABLE a', 'ALTER TABLE a COMMENT', 'ALTER TABLE b ADD COLUMN c'])
  })
})
