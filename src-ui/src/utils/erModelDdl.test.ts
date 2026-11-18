import { describe, expect, it } from 'vitest'
import { buildModelDdl, orderedTables } from './erModelDdl'
import type { ErModel, ModelColumn, ModelRelation, ModelTable } from '@/stores/erModel'

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

describe('orderedTables 拓扑排序', () => {
  it('被引用的表排在前面', () => {
    const m = model({
      tables: [
        table({ name: 'orders', columns: [col({ name: 'id', pk: true })] }),
        table({ name: 'users', columns: [col({ name: 'id', pk: true })] }),
      ],
      relations: [relation({
        id: 'r1',
        fromTableId: 'orders',
        fromColumnId: 'id',
        toTableId: 'users',
        toColumnId: 'id',
      })],
    })
    expect(orderedTables(m).map((t) => t.name)).toEqual(['users', 'orders'])
  })

  it('无关系时保持模型原顺序', () => {
    const m = model({
      tables: [
        table({ name: 'b', columns: [] }),
        table({ name: 'a', columns: [] }),
      ],
    })
    expect(orderedTables(m).map((t) => t.name)).toEqual(['b', 'a'])
  })

  it('环形引用兜底按原顺序全部输出', () => {
    const m = model({
      tables: [
        table({ name: 'a', columns: [col({ name: 'id', pk: true })] }),
        table({ name: 'b', columns: [col({ name: 'id', pk: true })] }),
      ],
      relations: [
        relation({ id: 'r1', fromTableId: 'a', fromColumnId: 'id', toTableId: 'b', toColumnId: 'id' }),
        relation({ id: 'r2', fromTableId: 'b', fromColumnId: 'id', toTableId: 'a', toColumnId: 'id' }),
      ],
    })
    // 环上无入度为零的表，走兜底分支，原序输出
    expect(orderedTables(m).map((t) => t.name)).toEqual(['a', 'b'])
  })
})

describe('buildModelDdl MySQL 方言', () => {
  it('生成 CREATE TABLE + 字段定义 + 表注释', () => {
    const m = model({
      tables: [table({
        name: 'users',
        comment: '用户表',
        columns: [
          col({ name: 'id', type: 'bigint', pk: true, nullable: false }),
          col({ name: 'email', type: 'varchar(64)', nullable: false, unique: true }),
          col({ name: 'age', type: 'int', default: '0' }),
        ],
      })],
    })
    const sql = buildModelDdl(m, 'mysql')
    expect(sql).toContain('CREATE TABLE `users` (')
    expect(sql).toContain('`id` bigint NOT NULL PRIMARY KEY')
    expect(sql).toContain('`email` varchar(64) NOT NULL')
    expect(sql).toContain('`age` int DEFAULT 0')
    expect(sql).toContain('UNIQUE INDEX `users_email_uk` (`email`)')
    expect(sql).toContain('ALTER TABLE `users` COMMENT = \'用户表\';')
  })

  it('生成外键内联约束', () => {
    const m = model({
      tables: [
        table({ name: 'users', columns: [col({ name: 'id', type: 'bigint', pk: true, nullable: false })] }),
        table({ name: 'orders', columns: [col({ name: 'user_id', type: 'bigint' })] }),
      ],
      relations: [relation({
        id: 'r1',
        fromTableId: 'orders',
        fromColumnId: 'user_id',
        toTableId: 'users',
        toColumnId: 'id',
      })],
    })
    const sql = buildModelDdl(m, 'mysql')
    expect(sql).toContain('CONSTRAINT `orders_users_fk` FOREIGN KEY (`user_id`) REFERENCES `users`(`id`) ON DELETE NO ACTION ON UPDATE NO ACTION')
  })

  it('同一对表的多条关系用序号区分约束名', () => {
    const m = model({
      tables: [
        table({ name: 'users', columns: [col({ name: 'id', pk: true }), col({ name: 'pid' })] }),
        table({ name: 'orders', columns: [col({ name: 'a' }), col({ name: 'b' })] }),
      ],
      relations: [
        relation({ id: 'r1', fromTableId: 'orders', fromColumnId: 'a', toTableId: 'users', toColumnId: 'id' }),
        relation({ id: 'r2', fromTableId: 'orders', fromColumnId: 'b', toTableId: 'users', toColumnId: 'pid' }),
      ],
    })
    const sql = buildModelDdl(m, 'mysql')
    expect(sql).toContain('`orders_users_fk` FOREIGN KEY (`a`)')
    expect(sql).toContain('`orders_users_fk_2` FOREIGN KEY (`b`)')
  })

  it('特殊字符默认值按字符串字面量输出', () => {
    const m = model({
      tables: [table({
        name: 'settings',
        columns: [col({ name: 'remark', default: "it's ok" })],
      })],
    })
    const sql = buildModelDdl(m, 'mysql')
    expect(sql).toContain("DEFAULT 'it''s ok'")
  })
})

describe('buildModelDdl PostgreSQL 方言', () => {
  it('主键/唯一走 CONSTRAINT 形式，注释走 COMMENT ON', () => {
    const m = model({
      tables: [table({
        name: 'users',
        comment: '用户表',
        columns: [
          col({ name: 'id', type: 'bigint', pk: true, nullable: false }),
          col({ name: 'email', type: 'varchar(64)', nullable: false, unique: true, comment: '登录邮箱' }),
        ],
      })],
    })
    const sql = buildModelDdl(m, 'postgres')
    expect(sql).toContain('CREATE TABLE "users" (')
    expect(sql).toContain('CONSTRAINT "users_pkey" PRIMARY KEY ("id")')
    // 非主键唯一约束在 PG 侧与表设计器一致：独立 CREATE UNIQUE INDEX
    expect(sql).toContain('CREATE UNIQUE INDEX "users_email_uk" ON "users" USING btree ("email");')
    expect(sql).toContain('COMMENT ON COLUMN "users"."email" IS \'登录邮箱\'')
    expect(sql).toContain('COMMENT ON TABLE "users" IS \'用户表\';')
    // 唯一约束不走表内 CONSTRAINT（与表设计器 PG 侧行为一致）
    expect(sql).not.toContain('UNIQUE ("email")')
  })

  it('外键为 CONSTRAINT 形式且建表语句按拓扑序排列', () => {
    const m = model({
      tables: [
        table({ name: 'orders', columns: [col({ name: 'user_id', type: 'bigint' })] }),
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
    const sql = buildModelDdl(m, 'postgres')
    const usersPos = sql.indexOf('CREATE TABLE "users"')
    const ordersPos = sql.indexOf('CREATE TABLE "orders"')
    expect(usersPos).toBeGreaterThanOrEqual(0)
    expect(ordersPos).toBeGreaterThan(usersPos)
    expect(sql).toContain('CONSTRAINT "orders_users_fk" FOREIGN KEY ("user_id") REFERENCES "users"("id")')
  })

  it('悬空关系（字段已删）不生成外键', () => {
    const m = model({
      tables: [
        table({ name: 'users', columns: [col({ name: 'id', pk: true })] }),
        table({ name: 'orders', columns: [col({ name: 'user_id' })] }),
      ],
      relations: [relation({
        id: 'r1',
        fromTableId: 'orders',
        fromColumnId: 'ghost',
        toTableId: 'users',
        toColumnId: 'id',
      })],
    })
    const sql = buildModelDdl(m, 'mysql')
    expect(sql).not.toContain('FOREIGN KEY')
  })
})
