import { describe, expect, it } from 'vitest'
import {
  buildAlterTableSql,
  buildAlterTableStatements,
  buildCreateDatabaseSql,
  buildCreateSchemaSql,
  buildCreateTableSql,
  buildCreateTableStatements,
  buildDropSchemaSql,
  buildFieldDefinition,
  buildRenameSchemaSql,
  columnHasDecimals,
  columnHasLength,
  columnTypeExpression,
  isIntegerColumnType,
  isTimestampColumnType,
  normalizeColumnType,
  splitSqlStatements,
  type DdlField,
  type DdlForeignKey,
  type DdlIndex,
  type TableDdlInput,
} from './ddl'

function field(overrides: Partial<DdlField> & Pick<DdlField, 'name' | 'type'>): DdlField {
  return {
    ...overrides,
    name: overrides.name,
    type: overrides.type,
    notNull: overrides.notNull ?? false,
    isPrimary: overrides.isPrimary ?? false,
    isAutoIncrement: overrides.isAutoIncrement ?? false,
    virtual: overrides.virtual ?? false,
  }
}

function index(overrides: Partial<DdlIndex> & Pick<DdlIndex, 'name' | 'columns'>): DdlIndex {
  return {
    ...overrides,
    name: overrides.name,
    columns: overrides.columns,
    type: overrides.type ?? 'BTREE',
    isUnique: overrides.isUnique ?? false,
  }
}

function foreignKey(
  overrides: Partial<DdlForeignKey> & Pick<DdlForeignKey, 'name' | 'column' | 'refTable' | 'refColumn'>
): DdlForeignKey {
  return {
    ...overrides,
    name: overrides.name,
    column: overrides.column,
    refTable: overrides.refTable,
    refColumn: overrides.refColumn,
    onDelete: overrides.onDelete ?? 'RESTRICT',
    onUpdate: overrides.onUpdate ?? 'NO ACTION',
  }
}

function input(overrides: Partial<TableDdlInput> & Pick<TableDdlInput, 'tableName' | 'fields'>): TableDdlInput {
  return {
    ...overrides,
    tableName: overrides.tableName,
    fields: overrides.fields,
    indexes: overrides.indexes ?? [],
    foreignKeys: overrides.foreignKeys ?? [],
    primaryKeyName: overrides.primaryKeyName,
  }
}

describe('normalizeColumnType', () => {
  it('MySQL 类型名原样返回', () => {
    expect(normalizeColumnType('varchar', 'mysql')).toEqual({ type: 'varchar' })
    expect(normalizeColumnType('tinyint', 'mysql')).toEqual({ type: 'tinyint' })
  })

  it('把 PostgreSQL format_type 结果映射到设计器类型集', () => {
    expect(normalizeColumnType('character varying(32)', 'postgres')).toEqual({ type: 'varchar', length: 32, decimals: undefined })
    expect(normalizeColumnType('timestamp without time zone', 'postgres')).toEqual({ type: 'timestamp', length: undefined, decimals: undefined })
    expect(normalizeColumnType('timestamp with time zone', 'postgres')).toEqual({ type: 'timestamptz', length: undefined, decimals: undefined })
    expect(normalizeColumnType('numeric(10,2)', 'postgres')).toEqual({ type: 'numeric', length: 10, decimals: 2 })
    expect(normalizeColumnType('int8', 'postgres')).toEqual({ type: 'bigint', length: undefined, decimals: undefined })
    expect(normalizeColumnType('double precision', 'postgres')).toEqual({ type: 'double precision', length: undefined, decimals: undefined })
    expect(normalizeColumnType('boolean', 'postgres')).toEqual({ type: 'boolean', length: undefined, decimals: undefined })
    expect(normalizeColumnType('bytea', 'postgres')).toEqual({ type: 'bytea', length: undefined, decimals: undefined })
  })
})

describe('类型能力判断', () => {
  it('MySQL 与 PostgreSQL 的长度/小数位/整数/时间戳类型集不同', () => {
    expect(columnHasLength('varchar', 'mysql')).toBe(true)
    expect(columnHasLength('varchar', 'postgres')).toBe(true)
    expect(columnHasLength('tinyint', 'mysql')).toBe(true)
    expect(columnHasLength('tinyint', 'postgres')).toBe(false)
    expect(columnHasDecimals('numeric', 'postgres')).toBe(true)
    expect(columnHasDecimals('float', 'mysql')).toBe(true)
    expect(isIntegerColumnType('serial', 'postgres')).toBe(true)
    expect(isIntegerColumnType('int', 'postgres')).toBe(false)
    expect(isTimestampColumnType('timestamptz', 'postgres')).toBe(true)
    expect(isTimestampColumnType('datetime', 'mysql')).toBe(true)
    expect(columnTypeExpression({ type: 'varchar', length: 32 }, 'postgres')).toBe('varchar(32)')
    expect(columnTypeExpression({ type: 'numeric', length: 10, decimals: 2 }, 'postgres')).toBe('numeric(10,2)')
  })
})

describe('buildFieldDefinition', () => {
  it('MySQL 保持历史行为（UNSIGNED / AUTO_INCREMENT / PRIMARY KEY / COMMENT）', () => {
    const def = buildFieldDefinition(
      field({
        name: 'id',
        type: 'int',
        notNull: true,
        isPrimary: true,
        isAutoIncrement: true,
        unsigned: true,
      }),
      'mysql'
    )
    expect(def).toBe('`id` int UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY')

    const commented = buildFieldDefinition(
      field({ name: 'name', type: 'varchar', length: 50, comment: "na'me" }),
      'mysql'
    )
    expect(commented).toBe("`name` varchar(50) COMMENT 'na''me'")
  })

  it('PostgreSQL 使用 IDENTITY、不输出 UNSIGNED / AUTO_INCREMENT / MySQL 注释', () => {
    const identity = buildFieldDefinition(
      field({ name: 'id', type: 'integer', notNull: true, isAutoIncrement: true }),
      'postgres'
    )
    expect(identity).toBe('"id" integer GENERATED BY DEFAULT AS IDENTITY')

    const serial = buildFieldDefinition(
      field({ name: 'id', type: 'serial', notNull: true, isAutoIncrement: true }),
      'postgres'
    )
    expect(serial).toBe('"id" serial NOT NULL')

    const text = buildFieldDefinition(
      field({ name: 'name', type: 'varchar', length: 100, defaultValue: 'abc', comment: 'x' }),
      'postgres'
    )
    expect(text).toBe('"name" varchar(100) DEFAULT \'abc\'')
  })

  it('PostgreSQL 默认值按表达式/字面量处理', () => {
    expect(buildFieldDefinition(field({ name: 'a', type: 'timestamptz', defaultValue: 'CURRENT_TIMESTAMP' }), 'postgres'))
      .toBe('"a" timestamptz DEFAULT CURRENT_TIMESTAMP')
    expect(buildFieldDefinition(field({ name: 'b', type: 'integer', defaultValue: '0' }), 'postgres'))
      .toBe('"b" integer DEFAULT 0')
    expect(buildFieldDefinition(field({ name: 'c', type: 'uuid', defaultValue: 'gen_random_uuid()' }), 'postgres'))
      .toBe('"c" uuid DEFAULT gen_random_uuid()')
    expect(buildFieldDefinition(field({ name: 'd', type: 'text', defaultValue: "it's" }), 'postgres'))
      .toBe('"d" text DEFAULT \'it\'\'s\'')
  })
})

describe('PostgreSQL CREATE TABLE', () => {
  const createInput = input({
    tableName: 'crm.orders',
    fields: [
      field({ name: 'id', type: 'integer', notNull: true, isPrimary: true, isAutoIncrement: true }),
      field({ name: 'customer_id', type: 'integer', notNull: true }),
      field({ name: 'name', type: 'varchar', length: 100, notNull: true, defaultValue: 'x' }),
      field({ name: 'total', type: 'numeric', length: 10, decimals: 2, notNull: true, defaultValue: '0' }),
      field({ name: 'email', type: 'char', length: 120, isUnique: true, comment: '邮箱' }),
      field({ name: 'created_at', type: 'timestamptz', notNull: true, defaultValue: 'CURRENT_TIMESTAMP' }),
    ],
    indexes: [index({ name: 'idx_orders_name', columns: ['name'], type: 'gin' })],
    foreignKeys: [
      foreignKey({
        name: 'fk_orders_customer',
        column: 'customer_id',
        refTable: 'crm.customers',
        refColumn: 'id',
        onDelete: 'CASCADE',
      }),
    ],
  })

  it('使用 CONSTRAINT 形式的主键/唯一/外键，schemas 分段引用表名', () => {
    const statements = buildCreateTableStatements(createInput, 'postgres')
    expect(statements[0]).toBe(
      'CREATE TABLE "crm"."orders" (\n' +
      '  "id" integer GENERATED BY DEFAULT AS IDENTITY,\n' +
      '  "customer_id" integer NOT NULL,\n' +
      '  "name" varchar(100) NOT NULL DEFAULT \'x\',\n' +
      '  "total" numeric(10,2) NOT NULL DEFAULT 0,\n' +
      '  "email" char(120),\n' +
      '  "created_at" timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP,\n' +
      '  CONSTRAINT "orders_pkey" PRIMARY KEY ("id"),\n' +
      '  CONSTRAINT "orders_email_key" UNIQUE ("email"),\n' +
      '  CONSTRAINT "fk_orders_customer" FOREIGN KEY ("customer_id") REFERENCES "crm"."customers"("id") ON DELETE CASCADE ON UPDATE NO ACTION\n' +
      ')'
    )
    expect(statements[1]).toBe('CREATE INDEX "idx_orders_name" ON "crm"."orders" USING gin ("name")')
    expect(statements[2]).toBe('COMMENT ON COLUMN "crm"."orders"."email" IS \'邮箱\'')
  })

  it('不会生成 MySQL 专属语法', () => {
    const sql = buildCreateTableSql(createInput, 'postgres')
    expect(sql).not.toMatch(/AUTO_INCREMENT|UNSIGNED|ENGINE=|CHARSET=|COLLATE=|CHANGE COLUMN|MODIFY COLUMN/)
    expect(sql.endsWith(';')).toBe(true)
  })
})

describe('MySQL CREATE TABLE 回归', () => {
  it('保持旧的字段定义、内联索引与外键输出', () => {
    const mysqlInput = input({
      tableName: 'users',
      fields: [
        field({ name: 'id', type: 'int', notNull: true, isPrimary: true, isAutoIncrement: true }),
        field({ name: 'name', type: 'varchar', length: 50, notNull: true, defaultValue: 'abc', comment: "na'me" }),
        field({ name: 'role_id', type: 'int' }),
      ],
      indexes: [index({ name: 'idx_name', columns: ['name'] })],
      foreignKeys: [
        foreignKey({ name: 'fk_role', column: 'role_id', refTable: 'roles', refColumn: 'id', onUpdate: 'CASCADE' }),
      ],
    })

    expect(buildCreateTableSql(mysqlInput, 'mysql')).toBe(
      'CREATE TABLE `users` (\n' +
      '  `id` int NOT NULL AUTO_INCREMENT PRIMARY KEY,\n' +
      '  `name` varchar(50) NOT NULL DEFAULT \'abc\' COMMENT \'na\'\'me\',\n' +
      '  `role_id` int,\n' +
      '  INDEX `idx_name` (`name`),\n' +
      '  CONSTRAINT `fk_role` FOREIGN KEY (`role_id`) REFERENCES `roles`(`id`) ON DELETE RESTRICT ON UPDATE CASCADE\n' +
      ');'
    )
  })
})

describe('PostgreSQL ALTER TABLE', () => {
  const alterInput = input({
    tableName: 'crm.orders',
    primaryKeyName: 'orders_pkey',
    fields: [
      field({
        name: 'name',
        type: 'varchar',
        length: 120,
        notNull: false,
        isModified: true,
        originalName: 'name',
        originalNotNull: true,
        originalDefaultValue: 'x',
        defaultValue: 'y',
        originalComment: 'old',
        comment: 'new',
        originalTypeSignature: 'varchar(100)',
      }),
      field({
        name: 'quantity',
        type: 'integer',
        notNull: false,
        isModified: true,
        originalName: 'qty',
        originalNotNull: false,
        originalTypeSignature: 'integer',
      }),
      field({ name: 'status', type: 'varchar', length: 20, notNull: true, defaultValue: 'new', isNew: true, comment: '状态' }),
      field({ name: 'legacy', type: 'text', isDeleted: true, originalName: 'legacy' }),
      field({
        name: 'id',
        type: 'integer',
        notNull: true,
        isPrimary: false,
        isModified: true,
        originalName: 'id',
        originalPrimary: true,
        originalTypeSignature: 'integer',
      }),
    ],
    indexes: [
      index({ name: 'idx_old', columns: ['name'], isDeleted: true, originalName: 'idx_old' }),
      index({ name: 'idx_new', columns: ['quantity'], type: 'gin', isNew: true }),
    ],
    foreignKeys: [
      foreignKey({ name: 'fk_old', column: 'customer_id', refTable: 'crm.customers', refColumn: 'id', isDeleted: true, originalName: 'fk_old' }),
      foreignKey({ name: 'fk_new', column: 'quantity', refTable: 'crm.customers', refColumn: 'id', isNew: true }),
    ],
  })

  it('生成 ALTER COLUMN TYPE / NOT NULL / DEFAULT / RENAME / COMMENT 等 PG 语法', () => {
    const statements = buildAlterTableStatements(alterInput, 'postgres')
    expect(statements).toEqual([
      'ALTER TABLE "crm"."orders" ALTER COLUMN "name" TYPE varchar(120)',
      'ALTER TABLE "crm"."orders" ALTER COLUMN "name" DROP NOT NULL',
      'ALTER TABLE "crm"."orders" ALTER COLUMN "name" SET DEFAULT \'y\'',
      'COMMENT ON COLUMN "crm"."orders"."name" IS \'new\'',
      'ALTER TABLE "crm"."orders" RENAME COLUMN "qty" TO "quantity"',
      'ALTER TABLE "crm"."orders" ADD COLUMN "status" varchar(20) NOT NULL DEFAULT \'new\'',
      'COMMENT ON COLUMN "crm"."orders"."status" IS \'状态\'',
      'ALTER TABLE "crm"."orders" DROP COLUMN "legacy"',
      'ALTER TABLE "crm"."orders" DROP CONSTRAINT "orders_pkey"',
      'DROP INDEX "crm"."idx_old"',
      'CREATE INDEX "idx_new" ON "crm"."orders" USING gin ("quantity")',
      'ALTER TABLE "crm"."orders" DROP CONSTRAINT "fk_old"',
      'ALTER TABLE "crm"."orders" ADD CONSTRAINT "fk_new" FOREIGN KEY ("quantity") REFERENCES "crm"."customers"("id") ON DELETE RESTRICT ON UPDATE NO ACTION',
    ])
    const sql = buildAlterTableSql(alterInput, 'postgres')
    expect(sql).not.toMatch(/CHANGE COLUMN|MODIFY COLUMN| FIRST| AFTER|DROP FOREIGN KEY|ADD PRIMARY KEY/)
  })

  it('跨类型族转换时追加 USING，整数加宽不需要', () => {
    const using = buildAlterTableStatements(input({
      tableName: 't',
      fields: [field({
        name: 'amount',
        type: 'integer',
        isModified: true,
        originalName: 'amount',
        originalTypeSignature: 'text',
      })],
    }), 'postgres')
    expect(using).toEqual([
      'ALTER TABLE "t" ALTER COLUMN "amount" TYPE integer USING "amount"::integer',
    ])

    const widening = buildAlterTableStatements(input({
      tableName: 't',
      fields: [field({
        name: 'count',
        type: 'bigint',
        isModified: true,
        originalName: 'count',
        originalTypeSignature: 'integer',
      })],
    }), 'postgres')
    expect(widening).toEqual(['ALTER TABLE "t" ALTER COLUMN "count" TYPE bigint'])
  })

  it('自增开关生成 ADD / DROP GENERATED ... IDENTITY', () => {
    const addIdentity = buildAlterTableStatements(input({
      tableName: 't',
      fields: [field({
        name: 'id',
        type: 'integer',
        isModified: true,
        isAutoIncrement: true,
        originalName: 'id',
        originalAutoIncrement: false,
        originalTypeSignature: 'integer',
      })],
    }), 'postgres')
    expect(addIdentity).toEqual([
      'ALTER TABLE "t" ALTER COLUMN "id" ADD GENERATED BY DEFAULT AS IDENTITY',
    ])

    const dropIdentity = buildAlterTableStatements(input({
      tableName: 't',
      fields: [field({
        name: 'id',
        type: 'integer',
        isModified: true,
        isAutoIncrement: false,
        originalName: 'id',
        originalAutoIncrement: true,
        originalTypeSignature: 'integer',
      })],
    }), 'postgres')
    expect(dropIdentity).toEqual(['ALTER TABLE "t" ALTER COLUMN "id" DROP IDENTITY'])
  })
})

describe('MySQL ALTER TABLE 回归', () => {
  it('保持 ADD COLUMN FIRST/AFTER、CHANGE/MODIFY COLUMN、DROP INDEX ... ON、DROP FOREIGN KEY 输出', () => {
    const mysqlInput = input({
      tableName: 'users',
      fields: [
        field({ name: 'created_at', type: 'timestamp', notNull: true, defaultValue: 'CURRENT_TIMESTAMP', isNew: true }),
        field({
          name: 'name',
          type: 'varchar',
          length: 100,
          notNull: true,
          isModified: true,
          originalName: 'old_name',
        }),
        field({ name: 'legacy', type: 'text', isDeleted: true, originalName: 'legacy' }),
      ],
      indexes: [
        index({ name: 'idx_name', columns: ['name'], isNew: true }),
        index({ name: 'idx_old', columns: ['legacy'], isDeleted: true, originalName: 'idx_old' }),
      ],
      foreignKeys: [
        foreignKey({ name: 'fk_role', column: 'role_id', refTable: 'roles', refColumn: 'id', isNew: true }),
        foreignKey({ name: 'fk_old', column: 'legacy', refTable: 'roles', refColumn: 'id', isDeleted: true, originalName: 'fk_old' }),
      ],
    })

    expect(buildAlterTableStatements(mysqlInput, 'mysql')).toEqual([
      'ALTER TABLE `users` ADD COLUMN `created_at` timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP FIRST',
      'ALTER TABLE `users` CHANGE COLUMN `old_name` `name` varchar(100) NOT NULL',
      'ALTER TABLE `users` DROP COLUMN `legacy`',
      'CREATE INDEX `idx_name` ON `users` (`name`)',
      'DROP INDEX `idx_old` ON `users`',
      'ALTER TABLE `users` ADD CONSTRAINT `fk_role` FOREIGN KEY (`role_id`) REFERENCES `roles`(`id`) ON DELETE RESTRICT ON UPDATE NO ACTION',
      'ALTER TABLE `users` DROP FOREIGN KEY `fk_old`',
    ])
  })
})

describe('splitSqlStatements', () => {
  it('按分号拆分并忽略字符串/引用标识符/注释中的分号', () => {
    expect(splitSqlStatements("SELECT 1; SELECT 'a;b'; SELECT \"c;d\"; SELECT `e;f`;"))
      .toEqual(['SELECT 1', "SELECT 'a;b'", 'SELECT "c;d"', 'SELECT `e;f`'])
    expect(splitSqlStatements('/* ; */ SELECT 1; -- tail; comment\n'))
      .toEqual(['/* ; */ SELECT 1', '-- tail; comment'])
    expect(splitSqlStatements("SELECT 'it''s;ok';")).toEqual(["SELECT 'it''s;ok'"])
    expect(splitSqlStatements('   ;;  ')).toEqual([])
  })
})

describe('schema / database DDL', () => {
  it('PostgreSQL schema 语句', () => {
    expect(buildCreateSchemaSql('crm')).toBe('CREATE SCHEMA "crm"')
    expect(buildRenameSchemaSql('crm', 'sales')).toBe('ALTER SCHEMA "crm" RENAME TO "sales"')
    expect(buildDropSchemaSql('crm')).toBe('DROP SCHEMA "crm" CASCADE')
  })

  it('MySQL 数据库语句保持不变', () => {
    expect(buildCreateDatabaseSql('shop', 'utf8mb4', 'utf8mb4_unicode_ci'))
      .toBe('CREATE DATABASE `shop` CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci')
  })
})
