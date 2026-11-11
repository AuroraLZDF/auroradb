//! ER 图数据批量查询
//!
//! 一次调用返回数据库下所有表的核心 ER 字段（列名/类型/主键）+ 所有外键关系，
//! 前端据此渲染 SVG ER 图。复用 metadata 模块的 INFORMATION_SCHEMA 查询路径
//! 与超时兜底，避免和 get_table_structure 逐表调用重复打 N 次元数据。

use anyhow::{anyhow, Context, Result};
use auroradb_core::connection::driver::{AnyPool, DriverKind};
use auroradb_core::connection::validation::validate_identifier;
use auroradb_core::connection::ConnectionManager;
use serde::Serialize;
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::timeout;

/// ER 图节点（表）精简信息
#[derive(Debug, Clone, Serialize)]
pub struct ErTableNode {
    pub name: String,
    pub columns: Vec<ErColumn>,
}

/// ER 图节点内的列
#[derive(Debug, Clone, Serialize)]
pub struct ErColumn {
    pub name: String,
    pub data_type: String,
    /// 含长度的完整类型，如 varchar(255)、decimal(10,2)
    pub column_type: String,
    pub is_primary: bool,
    pub is_unique: bool,
    pub nullable: bool,
    pub default_value: Option<String>,
    pub comment: String,
}

/// ER 图的外键边
#[derive(Debug, Clone, Serialize)]
pub struct ErForeignKey {
    pub name: String,
    pub table: String,
    pub column: String,
    pub ref_table: String,
    pub ref_column: String,
}

/// ER 图完整数据
#[derive(Debug, Clone, Serialize)]
pub struct ErDiagramData {
    pub driver: DriverKind,
    pub database: String,
    pub tables: Vec<ErTableNode>,
    pub foreign_keys: Vec<ErForeignKey>,
}

const ER_QUERY_TIMEOUT: Duration = Duration::from_secs(30);

/// 拉取整个数据库的 ER 图数据（所有表 + 所有外键）
pub async fn get_er_diagram_data(
    manager: &ConnectionManager,
    connection_id: &str,
    database: &str,
) -> Result<ErDiagramData> {
    validate_identifier(database).map_err(|e| anyhow!("数据库名验证失败: {}", e))?;

    let snapshot = manager.connection_snapshot(connection_id).await?;
    let pool = snapshot.pool;
    let driver = pool.driver();

    // PG 的「库」与「schema」是两个层级：前端能拿到的只有库名，
    // 直接当 schema 用（旧实现）会查不到任何表，ER 图永远是空的。
    let (tables, foreign_keys, scope) = match &pool {
        AnyPool::MySql(mysql_pool) => {
            let (tables, foreign_keys) = fetch_mysql_er(mysql_pool, database).await?;
            (tables, foreign_keys, database.to_string())
        }
        AnyPool::Postgres(pg_pool) => {
            let schema = resolve_pg_schema(pg_pool, database).await?;
            let (tables, foreign_keys) = fetch_pg_er(pg_pool, &schema).await?;
            (tables, foreign_keys, schema)
        }
        // SQLite 为本地单文件库，ER 图（跨表外键关系图）暂不支持
        AnyPool::Sqlite(_) => {
            return Err(anyhow!("SQLite 本地文件连接暂不支持 ER 图"));
        }
    };
    Ok(ErDiagramData {
        driver,
        database: scope,
        tables,
        foreign_keys,
    })
}

/// 解析 PG 下要查询的 schema。
///
/// 传入值恰好是已存在的 schema 时直接使用；否则（通常是库名）回落到 `current_schema()`，
/// 避免把库名当 schema 导致 ER 图空白。
async fn resolve_pg_schema(pool: &sqlx::postgres::PgPool, database: &str) -> Result<String> {
    if !database.is_empty() {
        let existing: Option<String> = timeout(
            ER_QUERY_TIMEOUT,
            sqlx::query_scalar(
                "SELECT nspname FROM pg_catalog.pg_namespace
                 WHERE nspname = $1
                   AND nspname NOT IN ('pg_catalog', 'information_schema')
                   AND nspname NOT LIKE 'pg_%'",
            )
            .bind(database)
            .fetch_optional(pool),
        )
        .await
        .context("ER 图查询超时")??;
        if let Some(name) = existing {
            return Ok(name);
        }
        tracing::debug!("ER 图：`{}` 不是 schema，回落到 current_schema()", database);
    }

    let current: Option<String> = timeout(
        ER_QUERY_TIMEOUT,
        sqlx::query_scalar("SELECT current_schema()::text").fetch_optional(pool),
    )
    .await
    .context("ER 图查询超时")??;
    Ok(current
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "public".to_string()))
}

async fn fetch_mysql_er(
    pool: &sqlx::mysql::MySqlPool,
    database: &str,
) -> Result<(Vec<ErTableNode>, Vec<ErForeignKey>)> {
    // 1. 拉取所有表
    let table_names: Vec<(String,)> = timeout(
        ER_QUERY_TIMEOUT,
        sqlx::query_as(
            "SELECT TABLE_NAME FROM INFORMATION_SCHEMA.TABLES
                 WHERE TABLE_SCHEMA = ? AND TABLE_TYPE = 'BASE TABLE'
                 ORDER BY TABLE_NAME ASC",
        )
        .bind(database)
        .fetch_all(pool),
    )
    .await
    .context("ER 图查询超时")??;

    // 2. 拉取所有列（含主键/唯一键标记、可空、默认值、注释）
    type MySqlErColumn = (
        String,
        String,
        String,
        String,
        bool,
        bool,
        Option<String>,
        String,
        bool,
    );
    let columns: Vec<MySqlErColumn> = timeout(
        ER_QUERY_TIMEOUT,
        sqlx::query_as(
            "SELECT c.TABLE_NAME, c.COLUMN_NAME, c.DATA_TYPE, c.COLUMN_TYPE,
                        (c.COLUMN_KEY = 'PRI') AS is_pri,
                        (c.IS_NULLABLE = 'YES') AS nullable,
                        c.COLUMN_DEFAULT,
                        COALESCE(c.COLUMN_COMMENT, '') AS comment,
                        (c.COLUMN_KEY = 'UNI') AS is_uni
                 FROM INFORMATION_SCHEMA.COLUMNS c
                 WHERE c.TABLE_SCHEMA = ?
                 ORDER BY c.TABLE_NAME, c.ORDINAL_POSITION",
        )
        .bind(database)
        .fetch_all(pool),
    )
    .await
    .context("ER 图查询超时")??;

    // 3. 拉取所有外键
    let fks: Vec<(String, String, String, String, String)> = timeout(
        ER_QUERY_TIMEOUT,
        sqlx::query_as(
            "SELECT kcu.CONSTRAINT_NAME, kcu.TABLE_NAME, kcu.COLUMN_NAME,
                        kcu.REFERENCED_TABLE_NAME, kcu.REFERENCED_COLUMN_NAME
                 FROM INFORMATION_SCHEMA.KEY_COLUMN_USAGE kcu
                 WHERE kcu.TABLE_SCHEMA = ? AND kcu.REFERENCED_TABLE_NAME IS NOT NULL
                 ORDER BY kcu.TABLE_NAME, kcu.COLUMN_NAME",
        )
        .bind(database)
        .fetch_all(pool),
    )
    .await
    .context("ER 图查询超时")??;

    let mut table_map: HashMap<String, ErTableNode> = HashMap::new();
    for (table_name,) in table_names {
        table_map.insert(
            table_name.clone(),
            ErTableNode {
                name: table_name,
                columns: vec![],
            },
        );
    }
    for (table, col, dtype, column_type, is_pri, nullable, default_value, comment, is_uni) in
        columns
    {
        if let Some(node) = table_map.get_mut(&table) {
            node.columns.push(ErColumn {
                name: col,
                data_type: dtype,
                column_type,
                is_primary: is_pri,
                is_unique: is_uni,
                nullable,
                default_value,
                comment,
            });
        }
    }

    let mut tables: Vec<ErTableNode> = table_map.into_values().collect();
    tables.sort_by(|a, b| a.name.cmp(&b.name));

    let foreign_keys: Vec<ErForeignKey> = fks
        .into_iter()
        .map(
            |(name, table, column, ref_table, ref_column)| ErForeignKey {
                name,
                table,
                column,
                ref_table,
                ref_column,
            },
        )
        .collect();

    Ok((tables, foreign_keys))
}

async fn fetch_pg_er(
    pool: &sqlx::postgres::PgPool,
    database: &str,
) -> Result<(Vec<ErTableNode>, Vec<ErForeignKey>)> {
    // PG 下 database 表示 schema（public 等），为空时用 search_path 中的 public
    let schema = if database.is_empty() {
        "public"
    } else {
        database
    };

    let table_names: Vec<(String,)> = timeout(
        ER_QUERY_TIMEOUT,
        sqlx::query_as(
            "SELECT TABLE_NAME FROM INFORMATION_SCHEMA.TABLES
                 WHERE TABLE_SCHEMA = $1 AND TABLE_TYPE = 'BASE TABLE'
                 ORDER BY TABLE_NAME ASC",
        )
        .bind(schema)
        .fetch_all(pool),
    )
    .await
    .context("ER 图查询超时")??;

    type PgErColumn = (
        String,
        String,
        String,
        String,
        bool,
        bool,
        Option<String>,
        String,
        bool,
    );
    let columns: Vec<PgErColumn> = timeout(
            ER_QUERY_TIMEOUT,
            sqlx::query_as(
                "SELECT c.TABLE_NAME, c.COLUMN_NAME, c.DATA_TYPE,
                        CASE
                            WHEN c.CHARACTER_MAXIMUM_LENGTH IS NOT NULL
                                THEN c.DATA_TYPE || '(' || c.CHARACTER_MAXIMUM_LENGTH::text || ')'
                            WHEN c.NUMERIC_PRECISION IS NOT NULL AND c.NUMERIC_SCALE IS NOT NULL
                                THEN c.DATA_TYPE || '(' || c.NUMERIC_PRECISION::text || ',' || c.NUMERIC_SCALE::text || ')'
                            WHEN c.NUMERIC_PRECISION IS NOT NULL
                                THEN c.DATA_TYPE || '(' || c.NUMERIC_PRECISION::text || ')'
                            WHEN c.DATETIME_PRECISION IS NOT NULL
                                THEN c.DATA_TYPE || '(' || c.DATETIME_PRECISION::text || ')'
                            ELSE c.DATA_TYPE
                        END AS column_type,
                        EXISTS(
                            SELECT 1 FROM INFORMATION_SCHEMA.KEY_COLUMN_USAGE kcu
                            WHERE kcu.TABLE_SCHEMA = c.TABLE_SCHEMA
                              AND kcu.TABLE_NAME = c.TABLE_NAME
                              AND kcu.COLUMN_NAME = c.COLUMN_NAME
                              AND kcu.CONSTRAINT_NAME LIKE '%_pkey'
                        ) AS is_pri,
                        (c.IS_NULLABLE = 'YES') AS nullable,
                        c.COLUMN_DEFAULT,
                        COALESCE(pgd.description, '') AS comment,
                        EXISTS(
                            SELECT 1
                            FROM INFORMATION_SCHEMA.TABLE_CONSTRAINTS tc
                            JOIN INFORMATION_SCHEMA.KEY_COLUMN_USAGE kcu
                              ON kcu.CONSTRAINT_NAME = tc.CONSTRAINT_NAME
                             AND kcu.TABLE_SCHEMA = tc.TABLE_SCHEMA
                             AND kcu.TABLE_NAME = tc.TABLE_NAME
                            WHERE tc.TABLE_SCHEMA = c.TABLE_SCHEMA
                              AND tc.TABLE_NAME = c.TABLE_NAME
                              AND tc.CONSTRAINT_TYPE = 'UNIQUE'
                              AND kcu.COLUMN_NAME = c.COLUMN_NAME
                        ) AS is_uni
                 FROM INFORMATION_SCHEMA.COLUMNS c
                 LEFT JOIN pg_catalog.pg_statio_all_tables st
                   ON st.schemaname = c.TABLE_SCHEMA AND st.relname = c.TABLE_NAME
                 LEFT JOIN pg_catalog.pg_description pgd
                   ON pgd.objoid = st.relid AND pgd.objsubid = c.ORDINAL_POSITION
                 WHERE c.TABLE_SCHEMA = $1
                 ORDER BY c.TABLE_NAME, c.ORDINAL_POSITION",
            )
            .bind(schema)
            .fetch_all(pool),
        )
        .await
        .context("ER 图查询超时")??;

    let fks: Vec<(String, String, String, String, String)> = timeout(
        ER_QUERY_TIMEOUT,
        sqlx::query_as(
            "SELECT tc.CONSTRAINT_NAME, kcu.TABLE_NAME, kcu.COLUMN_NAME,
                        kcu.REFERENCED_TABLE_NAME, kcu.REFERENCED_COLUMN_NAME
                 FROM INFORMATION_SCHEMA.TABLE_CONSTRAINTS tc
                 JOIN INFORMATION_SCHEMA.KEY_COLUMN_USAGE kcu
                   ON tc.CONSTRAINT_NAME = kcu.CONSTRAINT_NAME
                  AND tc.TABLE_SCHEMA = kcu.TABLE_SCHEMA
                 WHERE tc.TABLE_SCHEMA = $1
                   AND tc.CONSTRAINT_TYPE = 'FOREIGN KEY'
                 ORDER BY kcu.TABLE_NAME, kcu.COLUMN_NAME",
        )
        .bind(schema)
        .fetch_all(pool),
    )
    .await
    .context("ER 图查询超时")??;

    let mut table_map: HashMap<String, ErTableNode> = HashMap::new();
    for (table_name,) in table_names {
        table_map.insert(
            table_name.clone(),
            ErTableNode {
                name: table_name,
                columns: vec![],
            },
        );
    }
    for (table, col, dtype, column_type, is_pri, nullable, default_value, comment, is_uni) in
        columns
    {
        if let Some(node) = table_map.get_mut(&table) {
            node.columns.push(ErColumn {
                name: col,
                data_type: dtype,
                column_type,
                is_primary: is_pri,
                is_unique: is_uni,
                nullable,
                default_value,
                comment,
            });
        }
    }

    let mut tables: Vec<ErTableNode> = table_map.into_values().collect();
    tables.sort_by(|a, b| a.name.cmp(&b.name));

    let foreign_keys: Vec<ErForeignKey> = fks
        .into_iter()
        .map(
            |(name, table, column, ref_table, ref_column)| ErForeignKey {
                name,
                table,
                column,
                ref_table,
                ref_column,
            },
        )
        .collect();

    Ok((tables, foreign_keys))
}
