//! Schema 对比
//! 对比两个库的基表结构（列 + 索引），输出差异明细与可执行的迁移脚本。
//! 方向约定：source 为基准（源库），target 为待同步（目标库）；
//! Added 表示仅源库存在（需要 ADD/CREATE），Removed 表示仅目标库存在（需要 DROP）。

use anyhow::{anyhow, Result};
use auroradb_core::connection::driver::{AnyPool, DriverKind};
use auroradb_core::connection::metadata::with_metadata_timeout;
use auroradb_core::connection::validation::{escape_string, escape_string_pg, validate_identifier};
use auroradb_core::connection::{
    ConnectionManager, ObjectType, SchemaTableInfo, TableColumnInfo, TableForeignKeyInfo,
    TableStructure,
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// Schema 对比：列定义（来自 INFORMATION_SCHEMA.COLUMNS）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SchemaCompareColumn {
    pub name: String,
    pub column_type: String,
    pub is_nullable: bool,
    pub default_value: Option<String>,
    pub extra: String,
    pub comment: String,
    pub ordinal: u32,
    pub generation_expression: Option<String>,
    /// 是否主键列（PostgreSQL 用于识别主键索引；MySQL 分支不使用）
    #[serde(default)]
    pub is_primary: bool,
}

/// Schema 对比：索引定义（来自 INFORMATION_SCHEMA.STATISTICS）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SchemaCompareIndex {
    pub name: String,
    pub columns: Vec<String>,
    pub is_unique: bool,
    pub index_type: String,
}

/// Schema 对比：对象类型（基表 / 视图）。MySQL 分支只对比基表，PostgreSQL 分支会带上视图
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SchemaObjectKind {
    #[default]
    Table,
    View,
}

/// Schema 对比：外键定义（PostgreSQL 支持多列外键，因此按约束聚合成一条）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SchemaCompareForeignKey {
    pub name: String,
    pub columns: Vec<String>,
    /// 被引用表所在 schema；Database 口径下用于拼全限定名，Schema 口径下用于判断是否同一 schema
    pub ref_schema: Option<String>,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
    pub on_delete: String,
    pub on_update: String,
}

/// Schema 对比：表结构快照（MySQL 仅基表；PostgreSQL 含视图与主键/外键信息）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SchemaCompareTable {
    pub name: String,
    pub columns: Vec<SchemaCompareColumn>,
    pub indexes: Vec<SchemaCompareIndex>,
    /// 外键（MySQL 分支为空：MySQL 外键暂不参与对比）
    #[serde(default)]
    pub foreign_keys: Vec<SchemaCompareForeignKey>,
    /// 对象类型：表或视图（MySQL 分支恒为 Table）
    #[serde(default)]
    pub object_kind: SchemaObjectKind,
    /// 对象真实所在 schema（仅 PostgreSQL 口径有值）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// 生成 DDL 时是否给对象加 schema 前缀（Database 口径为 true，Schema 口径为 false）
    #[serde(default)]
    pub qualify_schema: bool,
    /// 视图的原始建视图语句（仅 PostgreSQL 视图有值，用于生成 CREATE OR REPLACE VIEW）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_statement: Option<String>,
}

/// 差异类型：Added=仅源库存在，Removed=仅目标库存在，Changed=两边都有但不一致
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffChange {
    Added,
    Removed,
    Changed,
}

/// 列级差异
#[derive(Debug, Clone, Serialize)]
pub struct SchemaColumnDiff {
    pub name: String,
    pub change: DiffChange,
    pub source: Option<String>,
    pub target: Option<String>,
    pub details: Vec<String>,
}

/// 索引级差异
#[derive(Debug, Clone, Serialize)]
pub struct SchemaIndexDiff {
    pub name: String,
    pub change: DiffChange,
    pub source: Option<String>,
    pub target: Option<String>,
}

/// 外键级差异（仅 PostgreSQL 分支填充）
#[derive(Debug, Clone, Serialize)]
pub struct SchemaForeignKeyDiff {
    pub name: String,
    pub change: DiffChange,
    pub source: Option<String>,
    pub target: Option<String>,
}

/// 表级差异
#[derive(Debug, Clone, Serialize)]
pub struct SchemaTableDiff {
    pub name: String,
    pub change: DiffChange,
    pub columns: Vec<SchemaColumnDiff>,
    pub indexes: Vec<SchemaIndexDiff>,
    /// 外键差异（MySQL 分支为空，序列化时省略以保持原有输出）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub foreign_keys: Vec<SchemaForeignKeyDiff>,
    pub statements: Vec<String>,
}

/// 差异汇总
#[derive(Debug, Clone, Default, Serialize)]
pub struct SchemaDiffSummary {
    pub added_tables: usize,
    pub removed_tables: usize,
    pub changed_tables: usize,
    pub added_columns: usize,
    pub removed_columns: usize,
    pub changed_columns: usize,
    pub added_indexes: usize,
    pub removed_indexes: usize,
    pub changed_indexes: usize,
    /// 外键计数（MySQL 分支恒为 0，序列化时省略以保持原有输出）
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub added_foreign_keys: usize,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub removed_foreign_keys: usize,
    #[serde(default, skip_serializing_if = "is_zero_usize")]
    pub changed_foreign_keys: usize,
}

fn is_zero_usize(value: &usize) -> bool {
    *value == 0
}

/// Schema 对比结果：source 为基准，target 为待同步；statements 为可直接执行的迁移脚本
#[derive(Debug, Clone, Serialize)]
pub struct SchemaDiff {
    pub source_database: String,
    pub target_database: String,
    pub tables: Vec<SchemaTableDiff>,
    pub statements: Vec<String>,
    pub summary: SchemaDiffSummary,
}

/// Schema 差异脚本执行结果：逐条应用，遇错即停；`failed_at` 为失败语句的序号（从 1 开始）
#[derive(Debug, Clone, Serialize)]
pub struct ApplySchemaDiffResult {
    pub applied: usize,
    pub failed_at: Option<usize>,
    pub error: Option<String>,
}

/// INFORMATION_SCHEMA.COLUMNS 的一行：
/// 表名、列名、类型、是否可空、默认值、EXTRA、注释、序号、生成表达式
type ColumnRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    i64,
    Option<String>,
);

/// 对比两个连接上的库结构，返回差异与迁移脚本
pub async fn compare_schemas(
    manager: &ConnectionManager,
    source_connection: &str,
    source_database: &str,
    target_connection: &str,
    target_database: &str,
) -> Result<SchemaDiff> {
    let source_driver = manager.driver_of(source_connection).await?;
    let target_driver = manager.driver_of(target_connection).await?;

    match (source_driver, target_driver) {
        (DriverKind::MySql, DriverKind::MySql) => {
            let source = load_compare_schema(manager, source_connection, source_database).await?;
            let target = load_compare_schema(manager, target_connection, target_database).await?;

            let mut diff = diff_schemas(source_database, target_database, &source, &target);

            // 新增表优先使用源库真实建表语句（含引擎/字符集/外键等），失败时保留兜底语句
            for table in diff
                .tables
                .iter_mut()
                .filter(|t| t.change == DiffChange::Added)
            {
                match show_create_table(manager, source_connection, source_database, &table.name)
                    .await
                {
                    Ok(statement) => table.statements = vec![terminate(statement)],
                    Err(e) => {
                        tracing::warn!("读取建表语句失败，改用兜底 DDL: {}: {}", table.name, e);
                    }
                }
            }

            diff.statements = diff
                .tables
                .iter()
                .flat_map(|t| t.statements.iter().cloned())
                .collect();

            Ok(diff)
        }
        (DriverKind::Postgres, DriverKind::Postgres) => {
            let (source, source_scope) =
                load_pg_compare_schema(manager, source_connection, source_database).await?;
            let (target, target_scope) =
                load_pg_compare_schema(manager, target_connection, target_database).await?;

            if source_scope.kind != target_scope.kind {
                return Err(anyhow!(
                    "PostgreSQL 两侧对比作用域不一致：源为{}（{}），目标为{}（{}）",
                    source_scope.kind.display_name(),
                    source_scope.requested,
                    target_scope.kind.display_name(),
                    target_scope.requested
                ));
            }

            Ok(diff_schemas_postgres(
                source_database,
                target_database,
                &source,
                &target,
            ))
        }
        (source_driver, target_driver) => Err(anyhow!(
            "暂不支持跨数据库类型对比：源为 {}，目标为 {}",
            source_driver.display_name(),
            target_driver.display_name()
        )),
    }
}

/// 读取参与对比的表结构：仅基表，列与索引各一条查询
async fn load_compare_schema(
    manager: &ConnectionManager,
    connection_id: &str,
    database: &str,
) -> Result<Vec<SchemaCompareTable>> {
    validate_identifier(database).map_err(|e| anyhow!("数据库名验证失败: {}", e))?;

    let snapshot = manager.connection_snapshot(connection_id).await?;
    let pool = snapshot.pool.as_mysql("Schema 对比")?;

    let table_rows: Vec<(String,)> = with_metadata_timeout(
        sqlx::query_as(
            "SELECT TABLE_NAME FROM INFORMATION_SCHEMA.TABLES
             WHERE TABLE_SCHEMA = ? AND TABLE_TYPE = 'BASE TABLE'
             ORDER BY TABLE_NAME ASC",
        )
        .bind(database)
        .fetch_all(pool),
    )
    .await?;

    let column_rows: Vec<ColumnRow> = with_metadata_timeout(
        sqlx::query_as(
            "SELECT TABLE_NAME, COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_DEFAULT,
                    EXTRA, COLUMN_COMMENT, CAST(ORDINAL_POSITION AS SIGNED),
                    GENERATION_EXPRESSION
             FROM INFORMATION_SCHEMA.COLUMNS
             WHERE TABLE_SCHEMA = ?
             ORDER BY TABLE_NAME ASC, ORDINAL_POSITION ASC",
        )
        .bind(database)
        .fetch_all(pool),
    )
    .await?;

    let index_rows: Vec<(String, String, Option<String>, i64, String, i64)> =
        with_metadata_timeout(
            sqlx::query_as(
                "SELECT TABLE_NAME, INDEX_NAME, COLUMN_NAME,
                    CAST(NON_UNIQUE AS SIGNED), INDEX_TYPE, CAST(SEQ_IN_INDEX AS SIGNED)
             FROM INFORMATION_SCHEMA.STATISTICS
             WHERE TABLE_SCHEMA = ?
             ORDER BY TABLE_NAME ASC, INDEX_NAME ASC, SEQ_IN_INDEX ASC",
            )
            .bind(database)
            .fetch_all(pool),
        )
        .await?;

    Ok(build_tables(table_rows, column_rows, index_rows))
}

/// 读取源库真实建表语句（新增表同步时使用）
async fn show_create_table(
    manager: &ConnectionManager,
    connection_id: &str,
    database: &str,
    table: &str,
) -> Result<String> {
    validate_identifier(database).map_err(|e| anyhow!("数据库名验证失败: {}", e))?;

    let snapshot = manager.connection_snapshot(connection_id).await?;
    let pool = snapshot.pool.as_mysql("Schema 对比")?;

    let (_, create_sql): (String, String) = with_metadata_timeout(
        sqlx::query_as(&format!(
            "SHOW CREATE TABLE {}.{}",
            quote(database),
            quote(table)
        ))
        .fetch_one(pool),
    )
    .await?;

    Ok(create_sql)
}

/// 把 INFORMATION_SCHEMA 的三份结果聚合成按表分组的结构
fn build_tables(
    table_rows: Vec<(String,)>,
    column_rows: Vec<ColumnRow>,
    index_rows: Vec<(String, String, Option<String>, i64, String, i64)>,
) -> Vec<SchemaCompareTable> {
    let mut tables: Vec<SchemaCompareTable> = Vec::with_capacity(table_rows.len());
    let mut table_lookup: HashMap<String, usize> = HashMap::new();

    for (name,) in table_rows {
        table_lookup.insert(name.clone(), tables.len());
        tables.push(SchemaCompareTable {
            name,
            columns: Vec::new(),
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            object_kind: SchemaObjectKind::Table,
            schema: None,
            qualify_schema: false,
            create_statement: None,
        });
    }

    for (
        table,
        name,
        column_type,
        is_nullable,
        default_value,
        extra,
        comment,
        ordinal,
        generation_expression,
    ) in column_rows
    {
        let Some(&idx) = table_lookup.get(&table) else {
            continue;
        };
        tables[idx].columns.push(SchemaCompareColumn {
            name,
            column_type,
            is_nullable: is_nullable.eq_ignore_ascii_case("YES"),
            default_value,
            extra,
            comment: comment.unwrap_or_default(),
            ordinal: ordinal.max(0) as u32,
            generation_expression,
            is_primary: false,
        });
    }

    let mut index_lookup: HashMap<(String, String), usize> = HashMap::new();
    for (table, index_name, column_name, non_unique, index_type, _seq) in index_rows {
        let Some(&table_idx) = table_lookup.get(&table) else {
            continue;
        };

        let key = (table, index_name.clone());
        let index_idx = match index_lookup.get(&key) {
            Some(idx) => *idx,
            None => {
                let idx = tables[table_idx].indexes.len();
                tables[table_idx].indexes.push(SchemaCompareIndex {
                    name: index_name.clone(),
                    columns: Vec::new(),
                    is_unique: non_unique == 0,
                    index_type,
                });
                index_lookup.insert(key, idx);
                idx
            }
        };

        // 函数索引的 COLUMN_NAME 为 NULL：跳过该列，避免生成非法 DDL
        if let Some(column) = column_name {
            tables[table_idx].indexes[index_idx].columns.push(column);
        }
    }

    for table in &mut tables {
        table.indexes.retain(|i| !i.columns.is_empty());
        table.indexes.sort_by(|a, b| {
            let a_primary = a.name == "PRIMARY";
            let b_primary = b.name == "PRIMARY";
            b_primary.cmp(&a_primary).then_with(|| a.name.cmp(&b.name))
        });
    }

    tables
}

/// 纯函数：对比两份表结构并生成差异与迁移语句
pub fn diff_schemas(
    source_database: &str,
    target_database: &str,
    source: &[SchemaCompareTable],
    target: &[SchemaCompareTable],
) -> SchemaDiff {
    let source_map: HashMap<&str, &SchemaCompareTable> =
        source.iter().map(|t| (t.name.as_str(), t)).collect();
    let target_map: HashMap<&str, &SchemaCompareTable> =
        target.iter().map(|t| (t.name.as_str(), t)).collect();

    let mut tables = Vec::new();
    let mut summary = SchemaDiffSummary::default();

    for table in source {
        match target_map.get(table.name.as_str()) {
            None => {
                summary.added_tables += 1;
                tables.push(SchemaTableDiff {
                    name: table.name.clone(),
                    change: DiffChange::Added,
                    columns: table
                        .columns
                        .iter()
                        .map(|c| SchemaColumnDiff {
                            name: c.name.clone(),
                            change: DiffChange::Added,
                            source: Some(column_definition(c)),
                            target: None,
                            details: vec!["仅源库存在".to_string()],
                        })
                        .collect(),
                    indexes: table
                        .indexes
                        .iter()
                        .map(|i| SchemaIndexDiff {
                            name: i.name.clone(),
                            change: DiffChange::Added,
                            source: Some(index_definition(i)),
                            target: None,
                        })
                        .collect(),
                    foreign_keys: Vec::new(),
                    statements: vec![create_table_statement(table)],
                });
            }
            Some(target_table) => {
                let (columns, indexes, clauses) = compare_table(table, target_table, &mut summary);
                if clauses.is_empty() {
                    continue;
                }
                summary.changed_tables += 1;
                let statement = format!(
                    "ALTER TABLE {} \n  {};",
                    quote(&table.name),
                    clauses.join(",\n  ")
                );
                tables.push(SchemaTableDiff {
                    name: table.name.clone(),
                    change: DiffChange::Changed,
                    columns,
                    indexes,
                    foreign_keys: Vec::new(),
                    statements: vec![statement],
                });
            }
        }
    }

    for table in target {
        if source_map.contains_key(table.name.as_str()) {
            continue;
        }
        summary.removed_tables += 1;
        tables.push(SchemaTableDiff {
            name: table.name.clone(),
            change: DiffChange::Removed,
            columns: table
                .columns
                .iter()
                .map(|c| SchemaColumnDiff {
                    name: c.name.clone(),
                    change: DiffChange::Removed,
                    source: None,
                    target: Some(column_definition(c)),
                    details: vec!["仅目标库存在".to_string()],
                })
                .collect(),
            indexes: table
                .indexes
                .iter()
                .map(|i| SchemaIndexDiff {
                    name: i.name.clone(),
                    change: DiffChange::Removed,
                    source: None,
                    target: Some(index_definition(i)),
                })
                .collect(),
            foreign_keys: Vec::new(),
            statements: vec![format!("DROP TABLE IF EXISTS {};", quote(&table.name))],
        });
    }

    let statements = tables
        .iter()
        .flat_map(|t| t.statements.iter().cloned())
        .collect();

    SchemaDiff {
        source_database: source_database.to_string(),
        target_database: target_database.to_string(),
        tables,
        statements,
        summary,
    }
}

/// 对比同名单表，返回列/索引差异与 ALTER 子句
fn compare_table(
    source: &SchemaCompareTable,
    target: &SchemaCompareTable,
    summary: &mut SchemaDiffSummary,
) -> (Vec<SchemaColumnDiff>, Vec<SchemaIndexDiff>, Vec<String>) {
    let mut columns = Vec::new();
    let mut indexes = Vec::new();
    let mut clauses = Vec::new();

    let target_columns: HashMap<&str, &SchemaCompareColumn> = target
        .columns
        .iter()
        .map(|c| (c.name.as_str(), c))
        .collect();

    for column in &source.columns {
        match target_columns.get(column.name.as_str()) {
            None => {
                summary.added_columns += 1;
                clauses.push(format!("ADD COLUMN {}", column_definition(column)));
                columns.push(SchemaColumnDiff {
                    name: column.name.clone(),
                    change: DiffChange::Added,
                    source: Some(column_definition(column)),
                    target: None,
                    details: vec!["仅源库存在".to_string()],
                });
            }
            Some(target_column) => {
                if column_signature(column) == column_signature(target_column) {
                    continue;
                }
                summary.changed_columns += 1;
                clauses.push(format!("MODIFY COLUMN {}", column_definition(column)));
                columns.push(SchemaColumnDiff {
                    name: column.name.clone(),
                    change: DiffChange::Changed,
                    source: Some(column_definition(column)),
                    target: Some(column_definition(target_column)),
                    details: column_details(column, target_column),
                });
            }
        }
    }

    let source_column_names: HashMap<&str, ()> = source
        .columns
        .iter()
        .map(|c| (c.name.as_str(), ()))
        .collect();
    for column in &target.columns {
        if source_column_names.contains_key(column.name.as_str()) {
            continue;
        }
        summary.removed_columns += 1;
        clauses.push(format!("DROP COLUMN {}", quote(&column.name)));
        columns.push(SchemaColumnDiff {
            name: column.name.clone(),
            change: DiffChange::Removed,
            source: None,
            target: Some(column_definition(column)),
            details: vec!["仅目标库存在".to_string()],
        });
    }

    let target_indexes: HashMap<&str, &SchemaCompareIndex> = target
        .indexes
        .iter()
        .map(|i| (i.name.as_str(), i))
        .collect();

    for index in &source.indexes {
        match target_indexes.get(index.name.as_str()) {
            None => {
                summary.added_indexes += 1;
                clauses.push(format!("ADD {}", index_definition(index)));
                indexes.push(SchemaIndexDiff {
                    name: index.name.clone(),
                    change: DiffChange::Added,
                    source: Some(index_definition(index)),
                    target: None,
                });
            }
            Some(target_index) => {
                if index_signature(index) == index_signature(target_index) {
                    continue;
                }
                summary.changed_indexes += 1;
                clauses.push(drop_index_clause(target_index));
                clauses.push(format!("ADD {}", index_definition(index)));
                indexes.push(SchemaIndexDiff {
                    name: index.name.clone(),
                    change: DiffChange::Changed,
                    source: Some(index_definition(index)),
                    target: Some(index_definition(target_index)),
                });
            }
        }
    }

    let source_index_names: HashMap<&str, ()> = source
        .indexes
        .iter()
        .map(|i| (i.name.as_str(), ()))
        .collect();
    for index in &target.indexes {
        if source_index_names.contains_key(index.name.as_str()) {
            continue;
        }
        summary.removed_indexes += 1;
        clauses.push(drop_index_clause(index));
        indexes.push(SchemaIndexDiff {
            name: index.name.clone(),
            change: DiffChange::Removed,
            source: None,
            target: Some(index_definition(index)),
        });
    }

    (columns, indexes, clauses)
}

/// 索引种类：用于判断两个索引是否等价（BTREE/HASH 等物理类型不参与比较）
#[derive(Debug, PartialEq, Eq)]
enum IndexKind {
    Primary,
    Unique,
    Fulltext,
    Spatial,
    Normal,
}

fn index_kind(index: &SchemaCompareIndex) -> IndexKind {
    if index.name.eq_ignore_ascii_case("PRIMARY") {
        IndexKind::Primary
    } else if index.index_type.eq_ignore_ascii_case("FULLTEXT") {
        IndexKind::Fulltext
    } else if index.index_type.eq_ignore_ascii_case("SPATIAL") {
        IndexKind::Spatial
    } else if index.is_unique {
        IndexKind::Unique
    } else {
        IndexKind::Normal
    }
}

fn index_signature(index: &SchemaCompareIndex) -> String {
    format!("{:?}|{}", index_kind(index), index.columns.join(","))
}

fn index_definition(index: &SchemaCompareIndex) -> String {
    let columns = index
        .columns
        .iter()
        .map(|c| quote(c))
        .collect::<Vec<_>>()
        .join(", ");

    match index_kind(index) {
        IndexKind::Primary => format!("PRIMARY KEY ({})", columns),
        IndexKind::Fulltext => format!("FULLTEXT INDEX {} ({})", quote(&index.name), columns),
        IndexKind::Spatial => format!("SPATIAL INDEX {} ({})", quote(&index.name), columns),
        IndexKind::Unique => format!("UNIQUE INDEX {} ({})", quote(&index.name), columns),
        IndexKind::Normal => format!("INDEX {} ({})", quote(&index.name), columns),
    }
}

fn drop_index_clause(index: &SchemaCompareIndex) -> String {
    if index.name.eq_ignore_ascii_case("PRIMARY") {
        "DROP PRIMARY KEY".to_string()
    } else {
        format!("DROP INDEX {}", quote(&index.name))
    }
}

/// 列定义（用于 CREATE TABLE 与 ADD/MODIFY COLUMN）
fn column_definition(column: &SchemaCompareColumn) -> String {
    let extra = column.extra.to_ascii_lowercase();
    let mut parts = vec![format!("{} {}", quote(&column.name), column.column_type)];

    if is_generated(&extra) {
        if let Some(expr) = column
            .generation_expression
            .as_deref()
            .filter(|e| !e.is_empty())
        {
            parts.push(format!("GENERATED ALWAYS AS ({})", expr));
        }
        if extra.contains("stored generated") {
            parts.push("STORED".to_string());
        } else {
            parts.push("VIRTUAL".to_string());
        }
    } else {
        parts.push(if column.is_nullable {
            "NULL".to_string()
        } else {
            "NOT NULL".to_string()
        });
        if let Some(default) = column.default_value.as_deref() {
            parts.push(format!(
                "DEFAULT {}",
                render_default(default, &column.extra)
            ));
        }
        if extra.contains("auto_increment") {
            parts.push("AUTO_INCREMENT".to_string());
        }
        if let Some(on_update) = extract_on_update(&column.extra) {
            parts.push(format!("ON UPDATE {}", on_update));
        }
    }

    if !column.comment.is_empty() {
        parts.push(format!("COMMENT '{}'", escape_string(&column.comment)));
    }

    parts.join(" ")
}

fn is_generated(extra_lower: &str) -> bool {
    extra_lower.contains("virtual generated") || extra_lower.contains("stored generated")
}

/// 渲染默认值字面量：时间函数/表达式/数字保持原样，其余按字符串加引号
fn render_default(value: &str, extra: &str) -> String {
    let lower = value.to_ascii_lowercase();

    if lower == "current_timestamp"
        || lower.starts_with("current_timestamp(")
        || lower == "now()"
        || lower == "current_date"
        || lower == "current_time"
    {
        return value.to_string();
    }

    if extra.to_ascii_lowercase().contains("default_generated") {
        return format!("({})", value);
    }

    if lower.starts_with("b'")
        || lower.starts_with("0x")
        || lower == "true"
        || lower == "false"
        || value.parse::<f64>().is_ok()
    {
        return value.to_string();
    }

    format!("'{}'", escape_string(value))
}

/// 从 EXTRA 中提取 `on update` 后的表达式（如 CURRENT_TIMESTAMP）
fn extract_on_update(extra: &str) -> Option<String> {
    const MARKER: &str = "on update ";
    let lower = extra.to_ascii_lowercase();
    let start = lower.find(MARKER)? + MARKER.len();
    let value = extra[start..].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn column_signature(column: &SchemaCompareColumn) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}",
        column.column_type,
        column.is_nullable,
        column.default_value.as_deref().unwrap_or("<none>"),
        normalize_extra(&column.extra),
        column.comment,
        column.generation_expression.as_deref().unwrap_or("")
    )
}

/// 归一化 EXTRA：去掉 DEFAULT_GENERATED 噪音并统一大小写
fn normalize_extra(extra: &str) -> String {
    extra
        .split_whitespace()
        .filter(|part| !part.eq_ignore_ascii_case("default_generated"))
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn column_details(source: &SchemaCompareColumn, target: &SchemaCompareColumn) -> Vec<String> {
    let mut details = Vec::new();

    if source.column_type != target.column_type {
        details.push(format!(
            "类型: {} → {}",
            target.column_type, source.column_type
        ));
    }
    if source.is_nullable != target.is_nullable {
        details.push(format!(
            "可空: {} → {}",
            nullable_label(target.is_nullable),
            nullable_label(source.is_nullable)
        ));
    }
    if source.default_value != target.default_value {
        details.push(format!(
            "默认值: {} → {}",
            option_label(target.default_value.as_deref()),
            option_label(source.default_value.as_deref())
        ));
    }
    let source_extra = normalize_extra(&source.extra);
    let target_extra = normalize_extra(&target.extra);
    if source_extra != target_extra {
        details.push(format!(
            "额外属性: {} → {}",
            empty_label(&target_extra),
            empty_label(&source_extra)
        ));
    }
    if source.comment != target.comment {
        details.push(format!(
            "注释: {} → {}",
            empty_label(&target.comment),
            empty_label(&source.comment)
        ));
    }
    if source.generation_expression != target.generation_expression {
        details.push("生成列表达式不一致".to_string());
    }

    details
}

fn nullable_label(nullable: bool) -> &'static str {
    if nullable {
        "允许 NULL"
    } else {
        "NOT NULL"
    }
}

fn option_label(value: Option<&str>) -> String {
    match value {
        None => "NULL".to_string(),
        Some(v) => empty_label(v),
    }
}

fn empty_label(value: &str) -> String {
    if value.is_empty() {
        "(空)".to_string()
    } else {
        value.to_string()
    }
}

/// 兜底建表语句：源库 SHOW CREATE TABLE 不可用时使用（不含引擎/字符集/外键）
fn create_table_statement(table: &SchemaCompareTable) -> String {
    let mut lines: Vec<String> = table.columns.iter().map(column_definition).collect();

    for index in table.indexes.iter().filter(|i| i.name == "PRIMARY") {
        lines.push(index_definition(index));
    }
    for index in table.indexes.iter().filter(|i| i.name != "PRIMARY") {
        lines.push(index_definition(index));
    }

    format!(
        "CREATE TABLE {} (\n  {}\n);",
        quote(&table.name),
        lines.join(",\n  ")
    )
}

fn quote(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

/// 保证脚本语句以分号结尾
fn terminate(statement: String) -> String {
    if statement.trim_end().ends_with(';') {
        statement
    } else {
        format!("{};", statement.trim_end())
    }
}

// ===========================================================================
// PostgreSQL 结构对比
//
// 与 MySQL 的差异：
// 1. PG 是「数据库 / schema」两层结构，而对比入口只有两个 database 参数；
//    因此这里把 database 参数解释为「对比作用域」：
//    - 等于当前连接库（或服务器上另一个库）→ 数据库级：跨库内全部用户 schema，
//      展示名沿用 core（public 裸名、其他 schema 为 `schema.table`），DDL 全限定；
//    - 等于当前库内已存在的 schema → schema 级：只对比该 schema，
//      展示名用裸表名，DDL 不限定 schema（由 apply 的 SET search_path 决定落点）。
// 2. 只读取系统 catalog（经 core 的 pub 元数据 API），不依赖 SHOW CREATE TABLE；
// 3. DDL 全部使用 PG 方言：双引号标识符、ALTER COLUMN ... TYPE、ADD/DROP CONSTRAINT、
//    CREATE INDEX / DROP INDEX、CREATE OR REPLACE VIEW，不产出任何 MySQL 专有语法。
// ===========================================================================

/// PostgreSQL 对比作用域
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PgScopeKind {
    /// 数据库级：跨库内全部用户 schema
    Database,
    /// schema 级：只对比当前库内的一个 schema
    Schema,
}

impl PgScopeKind {
    fn display_name(self) -> &'static str {
        match self {
            PgScopeKind::Database => "数据库级（跨全部 schema）",
            PgScopeKind::Schema => "schema 级",
        }
    }
}

/// 解析后的对比作用域：读取哪个库、是否需要按 schema 过滤、展示名如何处理
#[derive(Debug, Clone)]
struct PgScope {
    kind: PgScopeKind,
    /// 实际读取元数据的数据库名（core 会为该库建立/复用连接池）
    read_database: String,
    /// schema 级口径下只对比该 schema
    schema: Option<String>,
    /// 用户传入的原始参数（用于报告与展示）
    requested: String,
}

/// 解析 PG 对比作用域：先判断是否为当前库内 schema，再判断是否为数据库名
async fn resolve_pg_scope(
    manager: &ConnectionManager,
    connection_id: &str,
    requested: &str,
) -> Result<PgScope> {
    validate_identifier(requested).map_err(|e| anyhow!("数据库名验证失败: {}", e))?;

    let snapshot = manager.connection_snapshot(connection_id).await?;
    let pool = match &snapshot.pool {
        AnyPool::Postgres(pool) => pool.clone(),
        AnyPool::MySql(_) => {
            return Err(anyhow!("连接不是 PostgreSQL 连接: {}", connection_id));
        }
    };

    let current_database: String = with_metadata_timeout(
        sqlx::query_scalar::<_, String>("SELECT current_database()").fetch_one(&pool),
    )
    .await?;

    if requested == current_database {
        return Ok(PgScope {
            kind: PgScopeKind::Database,
            read_database: current_database,
            schema: None,
            requested: requested.to_string(),
        });
    }

    let schema_exists: bool = with_metadata_timeout(
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = $1)",
        )
        .bind(requested)
        .fetch_one(&pool),
    )
    .await?;

    if schema_exists {
        return Ok(PgScope {
            kind: PgScopeKind::Schema,
            read_database: current_database,
            schema: Some(requested.to_string()),
            requested: requested.to_string(),
        });
    }

    let database_exists: bool = with_metadata_timeout(
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM pg_database WHERE datname = $1)",
        )
        .bind(requested)
        .fetch_one(&pool),
    )
    .await?;

    if database_exists {
        return Ok(PgScope {
            kind: PgScopeKind::Database,
            read_database: requested.to_string(),
            schema: None,
            requested: requested.to_string(),
        });
    }

    Err(anyhow!("PostgreSQL 目标库或 schema 不存在: {}", requested))
}

/// 读取 PG 对比快照：作用域内所有对象（表 + 视图）的列/索引/外键/视图定义
///
/// 元数据全部来自 core 的公开 API：`get_schema_snapshot` 一次拿对象清单，
/// `get_table_structure` 逐对象补全列/索引/外键（N+1，属于元数据读取可接受开销）。
async fn load_pg_compare_schema(
    manager: &ConnectionManager,
    connection_id: &str,
    requested: &str,
) -> Result<(Vec<SchemaCompareTable>, PgScope)> {
    let scope = resolve_pg_scope(manager, connection_id, requested).await?;
    let snapshot = manager
        .get_schema_snapshot(connection_id, &scope.read_database)
        .await?;

    let mut tables = Vec::with_capacity(snapshot.tables.len());
    for info in &snapshot.tables {
        let schema = pg_schema_of_display(&info.name);
        if scope.kind == PgScopeKind::Schema {
            match &scope.schema {
                Some(expected) if &schema != expected => continue,
                _ => {}
            }
        }

        let structure = manager
            .get_table_structure(connection_id, &scope.read_database, &info.name)
            .await?;
        tables.push(build_pg_compare_table(info, &structure, &scope, &schema));
    }

    Ok((tables, scope))
}

/// 把 core 的元数据转成对比快照（含视图）
fn build_pg_compare_table(
    info: &SchemaTableInfo,
    structure: &TableStructure,
    scope: &PgScope,
    schema: &str,
) -> SchemaCompareTable {
    let is_view = matches!(info.object_type, ObjectType::View);
    let name = match scope.kind {
        PgScopeKind::Database => info.name.clone(),
        PgScopeKind::Schema => pg_bare_name(&info.name).to_string(),
    };

    let columns = structure
        .columns
        .iter()
        .enumerate()
        .map(|(index, column)| SchemaCompareColumn {
            name: column.name.clone(),
            column_type: pg_column_type_from_info(column),
            is_nullable: column.is_nullable,
            default_value: pg_default_value(column, scope, schema),
            extra: pg_column_extra(column),
            comment: column.comment.clone().unwrap_or_default(),
            ordinal: index as u32 + 1,
            generation_expression: None,
            is_primary: column.is_primary,
        })
        .collect();

    let indexes = structure
        .indexes
        .iter()
        .filter(|index| !index.columns.is_empty())
        .map(|index| SchemaCompareIndex {
            name: index.name.clone(),
            columns: index.columns.clone(),
            is_unique: index.is_unique,
            index_type: index.r#type.to_ascii_lowercase(),
        })
        .collect();

    let create_statement = if is_view {
        Some(pg_view_create_statement(
            &structure.create_statement,
            scope,
            schema,
            &name,
        ))
    } else {
        None
    };

    SchemaCompareTable {
        name,
        columns,
        indexes,
        foreign_keys: group_pg_foreign_keys(&structure.foreign_keys),
        object_kind: if is_view {
            SchemaObjectKind::View
        } else {
            SchemaObjectKind::Table
        },
        schema: Some(schema.to_string()),
        qualify_schema: scope.kind == PgScopeKind::Database,
        create_statement,
    }
}

/// 从展示名（`schema.table` 或裸表名）解析出 schema，裸表名默认 public
fn pg_schema_of_display(display_name: &str) -> String {
    match display_name.split_once('.') {
        Some((schema, _)) => schema.to_string(),
        None => "public".to_string(),
    }
}

/// 展示名去掉 schema 前缀，得到裸表名
fn pg_bare_name(display_name: &str) -> &str {
    match display_name.rsplit_once('.') {
        Some((_, name)) => name,
        None => display_name,
    }
}

/// 把 core 的基础类型 + 长度/精度还原成可执行的类型文本
fn pg_column_type_from_info(column: &TableColumnInfo) -> String {
    if column.data_type == "character varying" || column.data_type == "character" {
        if let Some(length) = column.length {
            return format!("{}({})", column.data_type, length);
        }
    }
    if column.data_type == "numeric" {
        match (column.length, column.decimals) {
            (Some(precision), Some(scale)) => format!("numeric({}, {})", precision, scale),
            (Some(precision), None) => format!("numeric({})", precision),
            _ => column.data_type.clone(),
        }
    } else {
        column.data_type.clone()
    }
}

/// 列额外属性：identity 表示 identity 列，serial 表示默认值为 nextval(...) 的列
fn pg_column_extra(column: &TableColumnInfo) -> String {
    if !column.is_auto_increment {
        return String::new();
    }
    match column.default_value.as_deref() {
        Some(value) if value.starts_with("nextval(") => "serial".to_string(),
        _ => "identity".to_string(),
    }
}

/// schema 级对比时把默认值里的本 schema 前缀去掉（serial 的 nextval('s.t_id_seq') 等），
/// 使两侧默认值可比较、且 apply 时依赖 search_path 解析到目标 schema
fn pg_default_value(column: &TableColumnInfo, scope: &PgScope, schema: &str) -> Option<String> {
    match scope.kind {
        PgScopeKind::Schema => column
            .default_value
            .as_deref()
            .map(|value| strip_pg_schema_prefix(value, schema)),
        PgScopeKind::Database => column.default_value.clone(),
    }
}

/// 去掉文本中本 schema 限定前缀（``"schema".`` 与 `schema.`）
fn strip_pg_schema_prefix(text: &str, schema: &str) -> String {
    let quoted = format!("\"{}\".", schema);
    let bare = format!("{}.", schema);
    text.replace(&quoted, "").replace(&bare, "")
}

/// 由视图的原始建视图语句组装标准化文本：`CREATE VIEW <name> AS <body>`
fn pg_view_create_statement(
    create_statement: &str,
    scope: &PgScope,
    schema: &str,
    name: &str,
) -> String {
    let body = pg_view_body(create_statement).unwrap_or(create_statement);
    let body = match scope.kind {
        PgScopeKind::Schema => strip_pg_schema_prefix(body, schema),
        PgScopeKind::Database => body.to_string(),
    };
    format!("CREATE VIEW {} AS {}", name, body)
}

/// core 的 PG 外键是「一列一行」，这里按约束名聚合成多列外键
fn group_pg_foreign_keys(rows: &[TableForeignKeyInfo]) -> Vec<SchemaCompareForeignKey> {
    let mut grouped: Vec<SchemaCompareForeignKey> = Vec::new();

    for row in rows {
        let (ref_schema, ref_table) = match row.ref_table.split_once('.') {
            Some((schema, table)) => (Some(schema.to_string()), table.to_string()),
            None => (None, row.ref_table.clone()),
        };

        match grouped.iter_mut().find(|fk| fk.name == row.name) {
            Some(fk) => {
                fk.columns.push(row.column.clone());
                fk.ref_columns.push(row.ref_column.clone());
            }
            None => grouped.push(SchemaCompareForeignKey {
                name: row.name.clone(),
                columns: vec![row.column.clone()],
                ref_schema,
                ref_table,
                ref_columns: vec![row.ref_column.clone()],
                on_delete: row.on_delete.clone(),
                on_update: row.on_update.clone(),
            }),
        }
    }

    grouped
}

// ---------------------------------------------------------------------------
// PostgreSQL DDL 辅助
// ---------------------------------------------------------------------------

/// PG 标识符：双引号包裹，内部双引号翻倍
fn pg_quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// 表引用：Database 口径全限定（`"schema"."table"`），Schema 口径依赖 search_path
fn pg_table_ref(table: &SchemaCompareTable) -> String {
    let table_name = pg_bare_name(&table.name);
    if table.qualify_schema {
        let schema = table.schema.as_deref().unwrap_or("public");
        format!("{}.{}", pg_quote(schema), pg_quote(table_name))
    } else {
        pg_quote(table_name)
    }
}

/// 索引引用（DROP INDEX 用）
fn pg_index_ref(table: &SchemaCompareTable, index_name: &str) -> String {
    if table.qualify_schema {
        let schema = table.schema.as_deref().unwrap_or("public");
        format!("{}.{}", pg_quote(schema), pg_quote(index_name))
    } else {
        pg_quote(index_name)
    }
}

fn pg_column_list(columns: &[String]) -> String {
    columns
        .iter()
        .map(|column| pg_quote(column))
        .collect::<Vec<_>>()
        .join(", ")
}

/// 列定义（CREATE TABLE / ADD COLUMN 共用）
fn pg_column_definition(column: &SchemaCompareColumn) -> String {
    let mut definition = format!("{} {}", pg_quote(&column.name), column.column_type);

    if column.extra.eq_ignore_ascii_case("identity") {
        // identity 列自带 NOT NULL，不再重复输出
        definition.push_str(" GENERATED BY DEFAULT AS IDENTITY");
        return definition;
    }

    if !column.is_nullable {
        definition.push_str(" NOT NULL");
    }
    if let Some(default_value) = column.default_value.as_deref() {
        if !default_value.is_empty() {
            definition.push_str(" DEFAULT ");
            definition.push_str(default_value);
        }
    }

    definition
}

/// 从 `CREATE VIEW "s"."t" AS\n<body>;` 中提取 body
fn pg_view_body(create_statement: &str) -> Option<&str> {
    let body = match create_statement.split_once(" AS\n") {
        Some((_, body)) => body,
        None => create_statement.split_once(" AS ")?.1,
    };
    let body = body.trim().trim_end_matches(';').trim();
    if body.is_empty() {
        None
    } else {
        Some(body)
    }
}

/// 视图体比较用归一化：折叠空白差异
fn normalize_view_text(body: &str) -> String {
    body.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 外键引用：同 schema 且 Schema 口径时只写表名，其余情况全限定
fn pg_fk_reference(fk: &SchemaCompareForeignKey, table: &SchemaCompareTable) -> String {
    let ref_schema = fk.ref_schema.as_deref().unwrap_or("public");
    let local_schema = table.schema.as_deref().unwrap_or("public");

    if table.qualify_schema || ref_schema != local_schema {
        format!("{}.{}", pg_quote(ref_schema), pg_quote(&fk.ref_table))
    } else {
        pg_quote(&fk.ref_table)
    }
}

/// 外键动作后缀：仅输出非默认（NO ACTION）的动作，与 core 的建表语句保持一致
fn pg_fk_actions_suffix(fk: &SchemaCompareForeignKey) -> String {
    let mut suffix = String::new();
    if !fk.on_delete.eq_ignore_ascii_case("NO ACTION") {
        suffix.push_str(&format!(" ON DELETE {}", fk.on_delete));
    }
    if !fk.on_update.eq_ignore_ascii_case("NO ACTION") {
        suffix.push_str(&format!(" ON UPDATE {}", fk.on_update));
    }
    suffix
}

fn pg_fk_display(fk: &SchemaCompareForeignKey, table: &SchemaCompareTable) -> String {
    format!(
        "FOREIGN KEY ({}) REFERENCES {} ({}){}",
        pg_column_list(&fk.columns),
        pg_fk_reference(fk, table),
        pg_column_list(&fk.ref_columns),
        pg_fk_actions_suffix(fk)
    )
}

fn pg_add_fk_statement(
    fk: &SchemaCompareForeignKey,
    source: &SchemaCompareTable,
    alter_ref: &str,
) -> String {
    format!(
        "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}){};",
        alter_ref,
        pg_quote(&fk.name),
        pg_column_list(&fk.columns),
        pg_fk_reference(fk, source),
        pg_column_list(&fk.ref_columns),
        pg_fk_actions_suffix(fk)
    )
}

fn pg_drop_constraint_statement(table_ref: &str, constraint: &str) -> String {
    format!(
        "ALTER TABLE {} DROP CONSTRAINT {};",
        table_ref,
        pg_quote(constraint)
    )
}

/// 外键签名：Database 口径比较全限定引用；Schema 口径下同一 schema 内的引用不计 schema 名
fn pg_fk_signature(fk: &SchemaCompareForeignKey, table: &SchemaCompareTable) -> String {
    let local_schema = table.schema.as_deref().unwrap_or("public");
    let ref_schema = fk.ref_schema.as_deref().unwrap_or("public");
    let ref_part = if table.qualify_schema || ref_schema != local_schema {
        ref_schema
    } else {
        ""
    };

    format!(
        "{}|{}|{}|{}|{}|{}",
        fk.columns.join(","),
        ref_part,
        fk.ref_table,
        fk.ref_columns.join(","),
        fk.on_delete,
        fk.on_update
    )
}

/// PG 索引归类：主键 = 唯一索引且键列等于表的主键列；其余按唯一/普通区分
fn pg_index_kind(index: &SchemaCompareIndex, table: &SchemaCompareTable) -> IndexKind {
    if !index.is_unique {
        return IndexKind::Normal;
    }

    let primary_columns: Vec<&str> = table
        .columns
        .iter()
        .filter(|column| column.is_primary)
        .map(|column| column.name.as_str())
        .collect();
    let index_columns: Vec<&str> = index.columns.iter().map(|column| column.as_str()).collect();

    if !primary_columns.is_empty() && primary_columns == index_columns {
        IndexKind::Primary
    } else {
        IndexKind::Unique
    }
}

fn pg_index_signature(index: &SchemaCompareIndex, table: &SchemaCompareTable) -> String {
    format!(
        "{:?}|{}|{}",
        pg_index_kind(index, table),
        index.columns.join(","),
        index.index_type
    )
}

fn pg_index_display(index: &SchemaCompareIndex, table: &SchemaCompareTable) -> String {
    let columns = pg_column_list(&index.columns);
    match pg_index_kind(index, table) {
        IndexKind::Primary => format!("PRIMARY KEY ({})", columns),
        IndexKind::Unique => format!("UNIQUE ({})", columns),
        _ => format!("INDEX ({})", columns),
    }
}

/// 建普通索引（普通索引无法写进 CREATE TABLE / ADD CONSTRAINT）
fn pg_create_index_statement(
    index: &SchemaCompareIndex,
    source: &SchemaCompareTable,
    target: &SchemaCompareTable,
) -> String {
    let unique = if pg_index_kind(index, source) == IndexKind::Unique {
        "CREATE UNIQUE INDEX"
    } else {
        "CREATE INDEX"
    };
    let mut statement = format!(
        "{} {} ON {}",
        unique,
        pg_index_ref(target, &index.name),
        pg_table_ref(target)
    );

    let method = index.index_type.to_ascii_lowercase();
    if !method.is_empty() && method != "btree" {
        statement.push_str(&format!(" USING {}", method));
    }
    statement.push_str(&format!(" ({});", pg_column_list(&index.columns)));
    statement
}

/// 新增索引：主键/唯一(btree) 走 ALTER TABLE ADD CONSTRAINT，其余走 CREATE INDEX
fn pg_add_index_statement(
    index: &SchemaCompareIndex,
    source: &SchemaCompareTable,
    target: &SchemaCompareTable,
) -> String {
    let columns = pg_column_list(&index.columns);
    match pg_index_kind(index, source) {
        IndexKind::Primary => format!(
            "ALTER TABLE {} ADD CONSTRAINT {} PRIMARY KEY ({});",
            pg_table_ref(target),
            pg_quote(&index.name),
            columns
        ),
        IndexKind::Unique if index.index_type.eq_ignore_ascii_case("btree") => format!(
            "ALTER TABLE {} ADD CONSTRAINT {} UNIQUE ({});",
            pg_table_ref(target),
            pg_quote(&index.name),
            columns
        ),
        _ => pg_create_index_statement(index, source, target),
    }
}

/// 删除索引：约束形式（主键/唯一 btree）走 DROP CONSTRAINT，其余走 DROP INDEX
fn pg_drop_index_statement(index: &SchemaCompareIndex, target: &SchemaCompareTable) -> String {
    let kind = pg_index_kind(index, target);
    let constraint_form = kind == IndexKind::Primary
        || (kind == IndexKind::Unique && index.index_type.eq_ignore_ascii_case("btree"));

    if constraint_form {
        format!(
            "ALTER TABLE {} DROP CONSTRAINT {};",
            pg_table_ref(target),
            pg_quote(&index.name)
        )
    } else {
        format!("DROP INDEX {};", pg_index_ref(target, &index.name))
    }
}

/// 新增表的建表脚本：CREATE TABLE（列 + 主键/唯一约束 + 外键）+ 列注释 + 普通索引
fn pg_create_table_statements(table: &SchemaCompareTable) -> Vec<String> {
    let table_ref = pg_table_ref(table);
    let mut definitions: Vec<String> = table.columns.iter().map(pg_column_definition).collect();
    let mut extra_indexes: Vec<String> = Vec::new();

    for index in &table.indexes {
        let columns = pg_column_list(&index.columns);
        match pg_index_kind(index, table) {
            IndexKind::Primary => definitions.push(format!(
                "CONSTRAINT {} PRIMARY KEY ({})",
                pg_quote(&index.name),
                columns
            )),
            IndexKind::Unique if index.index_type.eq_ignore_ascii_case("btree") => definitions
                .push(format!(
                    "CONSTRAINT {} UNIQUE ({})",
                    pg_quote(&index.name),
                    columns
                )),
            _ => extra_indexes.push(pg_create_index_statement(index, table, table)),
        }
    }

    for fk in &table.foreign_keys {
        definitions.push(format!(
            "CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}){}",
            pg_quote(&fk.name),
            pg_column_list(&fk.columns),
            pg_fk_reference(fk, table),
            pg_column_list(&fk.ref_columns),
            pg_fk_actions_suffix(fk)
        ));
    }

    let mut statements = Vec::new();
    if definitions.is_empty() {
        statements.push(format!("CREATE TABLE {} ();", table_ref));
    } else {
        statements.push(format!(
            "CREATE TABLE {} (\n  {}\n);",
            table_ref,
            definitions.join(",\n  ")
        ));
    }

    for column in &table.columns {
        if column.comment.is_empty() {
            continue;
        }
        statements.push(format!(
            "COMMENT ON COLUMN {}.{} IS '{}';",
            table_ref,
            pg_quote(&column.name),
            escape_string_pg(&column.comment)
        ));
    }

    statements.extend(extra_indexes);
    statements
}

/// 新增视图
fn pg_view_statement(table: &SchemaCompareTable) -> Vec<String> {
    match table.create_statement.as_deref().and_then(pg_view_body) {
        Some(body) => vec![format!("CREATE VIEW {} AS {};", pg_table_ref(table), body)],
        None => Vec::new(),
    }
}

/// 视图定义变化：CREATE OR REPLACE VIEW（作用在目标视图上）
fn pg_replace_view_statement(
    target: &SchemaCompareTable,
    source: &SchemaCompareTable,
) -> Vec<String> {
    match source.create_statement.as_deref().and_then(pg_view_body) {
        Some(body) => vec![format!(
            "CREATE OR REPLACE VIEW {} AS {};",
            pg_table_ref(target),
            body
        )],
        None => Vec::new(),
    }
}

/// 删除对象：视图用 DROP VIEW，表用 DROP TABLE
fn pg_drop_object_statement(table: &SchemaCompareTable) -> String {
    match table.object_kind {
        SchemaObjectKind::View => format!("DROP VIEW IF EXISTS {};", pg_table_ref(table)),
        SchemaObjectKind::Table => format!("DROP TABLE IF EXISTS {};", pg_table_ref(table)),
    }
}

fn pg_column_signature(column: &SchemaCompareColumn) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        column.column_type,
        column.is_nullable,
        column.default_value.as_deref().unwrap_or("<none>"),
        normalize_extra(&column.extra),
        column.comment
    )
}

/// 列变更对应的 ALTER COLUMN 子句
fn pg_column_alter_clauses(
    source: &SchemaCompareColumn,
    target: &SchemaCompareColumn,
) -> Vec<String> {
    let column = pg_quote(&source.name);
    let mut clauses = Vec::new();

    if source.column_type != target.column_type {
        clauses.push(format!(
            "ALTER COLUMN {} TYPE {} USING {}::{}",
            column, source.column_type, column, source.column_type
        ));
    }
    if source.is_nullable != target.is_nullable {
        clauses.push(if source.is_nullable {
            format!("ALTER COLUMN {} DROP NOT NULL", column)
        } else {
            format!("ALTER COLUMN {} SET NOT NULL", column)
        });
    }

    let source_identity = source.extra.eq_ignore_ascii_case("identity");
    let target_identity = target.extra.eq_ignore_ascii_case("identity");
    let source_default = source.default_value.as_deref().filter(|v| !v.is_empty());
    let target_default = target.default_value.as_deref().filter(|v| !v.is_empty());

    if source_identity {
        if !target_identity {
            if target_default.is_some() {
                clauses.push(format!("ALTER COLUMN {} DROP DEFAULT", column));
            }
            clauses.push(format!(
                "ALTER COLUMN {} ADD GENERATED BY DEFAULT AS IDENTITY",
                column
            ));
        }
    } else if target_identity {
        clauses.push(format!("ALTER COLUMN {} DROP IDENTITY IF EXISTS", column));
        if let Some(default_value) = source_default {
            clauses.push(format!(
                "ALTER COLUMN {} SET DEFAULT {}",
                column, default_value
            ));
        }
    } else if source_default != target_default {
        match source_default {
            Some(default_value) => clauses.push(format!(
                "ALTER COLUMN {} SET DEFAULT {}",
                column, default_value
            )),
            None => clauses.push(format!("ALTER COLUMN {} DROP DEFAULT", column)),
        }
    }

    clauses
}

/// 列注释语句（注释为空时清除）
fn pg_comment_statement(table_ref: &str, column: &str, comment: &str) -> String {
    if comment.is_empty() {
        format!(
            "COMMENT ON COLUMN {}.{} IS NULL;",
            table_ref,
            pg_quote(column)
        )
    } else {
        format!(
            "COMMENT ON COLUMN {}.{} IS '{}';",
            table_ref,
            pg_quote(column),
            escape_string_pg(comment)
        )
    }
}

/// 单表 PG 差异计划
struct PgTablePlan {
    columns: Vec<SchemaColumnDiff>,
    indexes: Vec<SchemaIndexDiff>,
    foreign_keys: Vec<SchemaForeignKeyDiff>,
    statements: Vec<String>,
}

/// 对比同名的两张 PG 表：列/索引/外键差异 + 目标侧可执行语句
fn pg_compare_table(
    source: &SchemaCompareTable,
    target: &SchemaCompareTable,
    summary: &mut SchemaDiffSummary,
) -> PgTablePlan {
    let target_ref = pg_table_ref(target);

    let mut columns = Vec::new();
    let mut indexes = Vec::new();
    let mut foreign_keys = Vec::new();
    let mut alter_clauses: Vec<String> = Vec::new();
    let mut comments: Vec<String> = Vec::new();
    let mut altered_columns: HashSet<String> = HashSet::new();

    let target_columns: HashMap<&str, &SchemaCompareColumn> = target
        .columns
        .iter()
        .map(|column| (column.name.as_str(), column))
        .collect();

    for column in &source.columns {
        match target_columns.get(column.name.as_str()) {
            None => {
                summary.added_columns += 1;
                alter_clauses.push(format!("ADD COLUMN {}", pg_column_definition(column)));
                if !column.comment.is_empty() {
                    comments.push(pg_comment_statement(
                        &target_ref,
                        &column.name,
                        &column.comment,
                    ));
                }
                columns.push(SchemaColumnDiff {
                    name: column.name.clone(),
                    change: DiffChange::Added,
                    source: Some(pg_column_definition(column)),
                    target: None,
                    details: vec!["仅源库存在".to_string()],
                });
            }
            Some(target_column) => {
                if pg_column_signature(column) == pg_column_signature(target_column) {
                    continue;
                }
                summary.changed_columns += 1;
                altered_columns.insert(column.name.clone());
                alter_clauses.extend(pg_column_alter_clauses(column, target_column));
                if column.comment != target_column.comment {
                    comments.push(pg_comment_statement(
                        &target_ref,
                        &column.name,
                        &column.comment,
                    ));
                }
                columns.push(SchemaColumnDiff {
                    name: column.name.clone(),
                    change: DiffChange::Changed,
                    source: Some(pg_column_definition(column)),
                    target: Some(pg_column_definition(target_column)),
                    details: column_details(column, target_column),
                });
            }
        }
    }

    let source_column_names: HashSet<&str> =
        source.columns.iter().map(|c| c.name.as_str()).collect();
    for column in &target.columns {
        if source_column_names.contains(column.name.as_str()) {
            continue;
        }
        summary.removed_columns += 1;
        altered_columns.insert(column.name.clone());
        alter_clauses.push(format!("DROP COLUMN {}", pg_quote(&column.name)));
        columns.push(SchemaColumnDiff {
            name: column.name.clone(),
            change: DiffChange::Removed,
            source: None,
            target: Some(pg_column_definition(column)),
            details: vec!["仅目标库存在".to_string()],
        });
    }

    // 外键：先删后建，避免列类型变更被外键阻塞
    let source_fks: HashMap<&str, &SchemaCompareForeignKey> = source
        .foreign_keys
        .iter()
        .map(|fk| (fk.name.as_str(), fk))
        .collect();
    let target_fks: HashMap<&str, &SchemaCompareForeignKey> = target
        .foreign_keys
        .iter()
        .map(|fk| (fk.name.as_str(), fk))
        .collect();

    let mut fk_drops: Vec<String> = Vec::new();
    let mut fk_adds: Vec<String> = Vec::new();

    for fk in &source.foreign_keys {
        match target_fks.get(fk.name.as_str()) {
            None => {
                summary.added_foreign_keys += 1;
                fk_adds.push(pg_add_fk_statement(fk, source, &target_ref));
                foreign_keys.push(SchemaForeignKeyDiff {
                    name: fk.name.clone(),
                    change: DiffChange::Added,
                    source: Some(pg_fk_display(fk, source)),
                    target: None,
                });
            }
            Some(target_fk) => {
                if pg_fk_signature(fk, source) == pg_fk_signature(target_fk, target) {
                    // 签名一致但本表列要被改类型：外键会阻塞 ALTER COLUMN，需先删后建
                    if fk
                        .columns
                        .iter()
                        .any(|column| altered_columns.contains(column))
                    {
                        fk_drops.push(pg_drop_constraint_statement(&target_ref, &fk.name));
                        fk_adds.push(pg_add_fk_statement(fk, source, &target_ref));
                    }
                    continue;
                }
                summary.changed_foreign_keys += 1;
                fk_drops.push(pg_drop_constraint_statement(&target_ref, &fk.name));
                fk_adds.push(pg_add_fk_statement(fk, source, &target_ref));
                foreign_keys.push(SchemaForeignKeyDiff {
                    name: fk.name.clone(),
                    change: DiffChange::Changed,
                    source: Some(pg_fk_display(fk, source)),
                    target: Some(pg_fk_display(target_fk, target)),
                });
            }
        }
    }

    for fk in &target.foreign_keys {
        if source_fks.contains_key(fk.name.as_str()) {
            continue;
        }
        summary.removed_foreign_keys += 1;
        fk_drops.push(pg_drop_constraint_statement(&target_ref, &fk.name));
        foreign_keys.push(SchemaForeignKeyDiff {
            name: fk.name.clone(),
            change: DiffChange::Removed,
            source: None,
            target: Some(pg_fk_display(fk, target)),
        });
    }

    // 索引
    let target_indexes: HashMap<&str, &SchemaCompareIndex> = target
        .indexes
        .iter()
        .map(|index| (index.name.as_str(), index))
        .collect();

    let mut index_drops: Vec<String> = Vec::new();
    let mut index_adds: Vec<String> = Vec::new();

    for index in &source.indexes {
        match target_indexes.get(index.name.as_str()) {
            None => {
                summary.added_indexes += 1;
                index_adds.push(pg_add_index_statement(index, source, target));
                indexes.push(SchemaIndexDiff {
                    name: index.name.clone(),
                    change: DiffChange::Added,
                    source: Some(pg_index_display(index, source)),
                    target: None,
                });
            }
            Some(target_index) => {
                if pg_index_signature(index, source) == pg_index_signature(target_index, target) {
                    continue;
                }
                summary.changed_indexes += 1;
                index_drops.push(pg_drop_index_statement(target_index, target));
                index_adds.push(pg_add_index_statement(index, source, target));
                indexes.push(SchemaIndexDiff {
                    name: index.name.clone(),
                    change: DiffChange::Changed,
                    source: Some(pg_index_display(index, source)),
                    target: Some(pg_index_display(target_index, target)),
                });
            }
        }
    }

    let source_index_names: HashSet<&str> = source
        .indexes
        .iter()
        .map(|index| index.name.as_str())
        .collect();
    for index in &target.indexes {
        if source_index_names.contains(index.name.as_str()) {
            continue;
        }
        summary.removed_indexes += 1;
        index_drops.push(pg_drop_index_statement(index, target));
        indexes.push(SchemaIndexDiff {
            name: index.name.clone(),
            change: DiffChange::Removed,
            source: None,
            target: Some(pg_index_display(index, target)),
        });
    }

    let mut statements = Vec::new();
    statements.extend(fk_drops);
    statements.extend(index_drops);
    if !alter_clauses.is_empty() {
        statements.push(format!(
            "ALTER TABLE {}\n  {};",
            target_ref,
            alter_clauses.join(",\n  ")
        ));
    }
    statements.extend(comments);
    statements.extend(index_adds);
    statements.extend(fk_adds);

    PgTablePlan {
        columns,
        indexes,
        foreign_keys,
        statements,
    }
}

/// 新增对象的明细（仅用于展示，不参与汇总计数，保持与 MySQL 新增表口径一致）
fn pg_object_detail_diffs(
    table: &SchemaCompareTable,
) -> (
    Vec<SchemaColumnDiff>,
    Vec<SchemaIndexDiff>,
    Vec<SchemaForeignKeyDiff>,
) {
    let columns = table
        .columns
        .iter()
        .map(|column| SchemaColumnDiff {
            name: column.name.clone(),
            change: DiffChange::Added,
            source: Some(pg_column_definition(column)),
            target: None,
            details: vec!["仅源库存在".to_string()],
        })
        .collect();
    let indexes = table
        .indexes
        .iter()
        .map(|index| SchemaIndexDiff {
            name: index.name.clone(),
            change: DiffChange::Added,
            source: Some(pg_index_display(index, table)),
            target: None,
        })
        .collect();
    let foreign_keys = table
        .foreign_keys
        .iter()
        .map(|fk| SchemaForeignKeyDiff {
            name: fk.name.clone(),
            change: DiffChange::Added,
            source: Some(pg_fk_display(fk, table)),
            target: None,
        })
        .collect();

    (columns, indexes, foreign_keys)
}

/// 纯函数：对比两份 PostgreSQL 快照并生成 PG 方言迁移语句
fn diff_schemas_postgres(
    source_database: &str,
    target_database: &str,
    source: &[SchemaCompareTable],
    target: &[SchemaCompareTable],
) -> SchemaDiff {
    let target_map: HashMap<&str, &SchemaCompareTable> = target
        .iter()
        .map(|table| (table.name.as_str(), table))
        .collect();
    let source_map: HashMap<&str, &SchemaCompareTable> = source
        .iter()
        .map(|table| (table.name.as_str(), table))
        .collect();

    let mut tables = Vec::new();
    let mut summary = SchemaDiffSummary::default();

    for table in source {
        match target_map.get(table.name.as_str()) {
            None => {
                summary.added_tables += 1;
                let (columns, indexes, foreign_keys) = pg_object_detail_diffs(table);
                let statements = match table.object_kind {
                    SchemaObjectKind::View => pg_view_statement(table),
                    SchemaObjectKind::Table => pg_create_table_statements(table),
                };
                tables.push(SchemaTableDiff {
                    name: table.name.clone(),
                    change: DiffChange::Added,
                    columns,
                    indexes,
                    foreign_keys,
                    statements,
                });
            }
            Some(target_table) => {
                // 表 <-> 视图 类型变化：删除目标对象后按源对象重建（尽力而为）
                if table.object_kind != target_table.object_kind {
                    summary.changed_tables += 1;
                    let mut statements = vec![pg_drop_object_statement(target_table)];
                    statements.extend(match table.object_kind {
                        SchemaObjectKind::View => pg_view_statement(table),
                        SchemaObjectKind::Table => pg_create_table_statements(table),
                    });
                    tables.push(SchemaTableDiff {
                        name: table.name.clone(),
                        change: DiffChange::Changed,
                        columns: Vec::new(),
                        indexes: Vec::new(),
                        foreign_keys: Vec::new(),
                        statements,
                    });
                    continue;
                }

                if table.object_kind == SchemaObjectKind::View {
                    let source_body = table
                        .create_statement
                        .as_deref()
                        .and_then(pg_view_body)
                        .map(normalize_view_text)
                        .unwrap_or_default();
                    let target_body = target_table
                        .create_statement
                        .as_deref()
                        .and_then(pg_view_body)
                        .map(normalize_view_text)
                        .unwrap_or_default();
                    if source_body == target_body {
                        continue;
                    }
                    summary.changed_tables += 1;
                    tables.push(SchemaTableDiff {
                        name: table.name.clone(),
                        change: DiffChange::Changed,
                        columns: Vec::new(),
                        indexes: Vec::new(),
                        foreign_keys: Vec::new(),
                        statements: pg_replace_view_statement(target_table, table),
                    });
                    continue;
                }

                let plan = pg_compare_table(table, target_table, &mut summary);
                if plan.columns.is_empty()
                    && plan.indexes.is_empty()
                    && plan.foreign_keys.is_empty()
                    && plan.statements.is_empty()
                {
                    continue;
                }
                summary.changed_tables += 1;
                tables.push(SchemaTableDiff {
                    name: table.name.clone(),
                    change: DiffChange::Changed,
                    columns: plan.columns,
                    indexes: plan.indexes,
                    foreign_keys: plan.foreign_keys,
                    statements: plan.statements,
                });
            }
        }
    }

    for table in target {
        if source_map.contains_key(table.name.as_str()) {
            continue;
        }
        summary.removed_tables += 1;

        let columns = table
            .columns
            .iter()
            .map(|column| SchemaColumnDiff {
                name: column.name.clone(),
                change: DiffChange::Removed,
                source: None,
                target: Some(pg_column_definition(column)),
                details: vec!["仅目标库存在".to_string()],
            })
            .collect();
        let indexes = table
            .indexes
            .iter()
            .map(|index| SchemaIndexDiff {
                name: index.name.clone(),
                change: DiffChange::Removed,
                source: None,
                target: Some(pg_index_display(index, table)),
            })
            .collect();
        let foreign_keys = table
            .foreign_keys
            .iter()
            .map(|fk| SchemaForeignKeyDiff {
                name: fk.name.clone(),
                change: DiffChange::Removed,
                source: None,
                target: Some(pg_fk_display(fk, table)),
            })
            .collect();

        tables.push(SchemaTableDiff {
            name: table.name.clone(),
            change: DiffChange::Removed,
            columns,
            indexes,
            foreign_keys,
            statements: vec![pg_drop_object_statement(table)],
        });
    }

    let statements = tables
        .iter()
        .flat_map(|table| table.statements.iter().cloned())
        .collect();

    SchemaDiff {
        source_database: source_database.to_string(),
        target_database: target_database.to_string(),
        tables,
        statements,
        summary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column(name: &str, column_type: &str) -> SchemaCompareColumn {
        SchemaCompareColumn {
            name: name.to_string(),
            column_type: column_type.to_string(),
            is_nullable: true,
            default_value: None,
            extra: String::new(),
            comment: String::new(),
            ordinal: 1,
            generation_expression: None,
            is_primary: false,
        }
    }

    fn index(name: &str, columns: &[&str], is_unique: bool) -> SchemaCompareIndex {
        SchemaCompareIndex {
            name: name.to_string(),
            columns: columns.iter().map(|c| c.to_string()).collect(),
            is_unique,
            index_type: "BTREE".to_string(),
        }
    }

    fn table(
        name: &str,
        columns: Vec<SchemaCompareColumn>,
        indexes: Vec<SchemaCompareIndex>,
    ) -> SchemaCompareTable {
        SchemaCompareTable {
            name: name.to_string(),
            columns,
            indexes,
            foreign_keys: Vec::new(),
            object_kind: SchemaObjectKind::Table,
            schema: None,
            qualify_schema: false,
            create_statement: None,
        }
    }

    #[test]
    fn identical_schemas_have_no_diff() {
        let source = vec![table(
            "users",
            vec![column("id", "int"), column("name", "varchar(50)")],
            vec![index("PRIMARY", &["id"], true)],
        )];

        let diff = diff_schemas("base", "target", &source, &source);

        assert!(diff.tables.is_empty());
        assert!(diff.statements.is_empty());
        assert_eq!(diff.summary.added_tables, 0);
    }

    #[test]
    fn detects_added_and_removed_tables() {
        let source = vec![table("users", vec![column("id", "int")], vec![])];
        let target = vec![table("legacy", vec![column("id", "int")], vec![])];

        let diff = diff_schemas("base", "target", &source, &target);

        assert_eq!(diff.summary.added_tables, 1);
        assert_eq!(diff.summary.removed_tables, 1);
        assert_eq!(diff.statements.len(), 2);
        assert!(diff.statements[0].starts_with("CREATE TABLE `users`"));
        assert_eq!(diff.statements[1], "DROP TABLE IF EXISTS `legacy`;");
        assert_eq!(diff.tables[0].change, DiffChange::Added);
        assert_eq!(diff.tables[1].change, DiffChange::Removed);
    }

    #[test]
    fn detects_column_changes() {
        let source = vec![table(
            "users",
            vec![
                column("id", "int"),
                column("name", "varchar(100)"),
                column("age", "int"),
            ],
            vec![],
        )];
        let target = vec![table(
            "users",
            vec![
                column("id", "int"),
                column("name", "varchar(50)"),
                column("legacy", "text"),
            ],
            vec![],
        )];

        let diff = diff_schemas("base", "target", &source, &target);

        assert_eq!(diff.summary.changed_tables, 1);
        assert_eq!(diff.summary.added_columns, 1);
        assert_eq!(diff.summary.removed_columns, 1);
        assert_eq!(diff.summary.changed_columns, 1);

        let statement = &diff.statements[0];
        assert!(statement.contains("MODIFY COLUMN `name` varchar(100)"));
        assert!(statement.contains("ADD COLUMN `age` int"));
        assert!(statement.contains("DROP COLUMN `legacy`"));
        assert!(!statement.contains("`id`"));
    }

    #[test]
    fn detects_index_changes() {
        let source = vec![table(
            "users",
            vec![column("id", "int"), column("email", "varchar(50)")],
            vec![
                index("PRIMARY", &["id"], true),
                index("idx_email", &["email"], true),
            ],
        )];
        let target = vec![table(
            "users",
            vec![column("id", "int"), column("email", "varchar(50)")],
            vec![
                index("PRIMARY", &["id"], true),
                index("idx_old", &["email"], false),
            ],
        )];

        let diff = diff_schemas("base", "target", &source, &target);

        assert_eq!(diff.summary.changed_tables, 1);
        assert_eq!(diff.summary.added_indexes, 1);
        assert_eq!(diff.summary.removed_indexes, 1);
        let statement = &diff.statements[0];
        assert!(statement.contains("ADD UNIQUE INDEX `idx_email` (`email`)"));
        assert!(statement.contains("DROP INDEX `idx_old`"));
        assert!(!statement.contains("PRIMARY"));
    }

    #[test]
    fn renders_column_definition_with_default_and_comment() {
        let mut col = column("status", "varchar(20)");
        col.is_nullable = false;
        col.default_value = Some("new".to_string());
        col.comment = "状态".to_string();

        let definition = column_definition(&col);

        assert_eq!(
            definition,
            "`status` varchar(20) NOT NULL DEFAULT 'new' COMMENT '状态'"
        );
    }

    #[test]
    fn keeps_numeric_and_temporal_defaults_unquoted() {
        assert_eq!(render_default("0", ""), "0");
        assert_eq!(render_default("1.5", ""), "1.5");
        assert_eq!(
            render_default("CURRENT_TIMESTAMP", "DEFAULT_GENERATED"),
            "CURRENT_TIMESTAMP"
        );
        assert_eq!(render_default("uuid()", "DEFAULT_GENERATED"), "(uuid())");
        assert_eq!(render_default("it's", ""), "'it\\'s'");
        assert_eq!(render_default("b'1'", ""), "b'1'");
    }

    #[test]
    fn renders_auto_increment_and_on_update() {
        let mut col = column("updated_at", "datetime");
        col.is_nullable = false;
        col.extra = "DEFAULT_GENERATED on update CURRENT_TIMESTAMP".to_string();

        let definition = column_definition(&col);

        assert!(definition.contains("NOT NULL"));
        assert!(definition.contains("ON UPDATE CURRENT_TIMESTAMP"));

        let mut id = column("id", "int");
        id.is_nullable = false;
        id.extra = "auto_increment".to_string();
        assert!(column_definition(&id).contains("AUTO_INCREMENT"));
    }

    #[test]
    fn ignores_default_generated_noise_in_signature() {
        let mut left = column("created_at", "datetime");
        left.extra = "DEFAULT_GENERATED".to_string();
        let mut right = column("created_at", "datetime");
        right.extra = "default_generated".to_string();

        assert_eq!(column_signature(&left), column_signature(&right));
    }

    #[test]
    fn create_table_statement_contains_primary_key_last() {
        let mut id = column("id", "int");
        id.is_nullable = false;
        let source = vec![table(
            "users",
            vec![id, column("name", "varchar(50)")],
            vec![index("PRIMARY", &["id"], true)],
        )];

        let diff = diff_schemas("base", "target", &source, &[]);
        let statement = &diff.statements[0];

        assert!(statement.starts_with("CREATE TABLE `users`"));
        assert!(statement.contains("`id` int NOT NULL"));
        assert!(statement.contains("PRIMARY KEY (`id`)"));
        assert!(statement.ends_with(");"));
    }

    // -----------------------------------------------------------------------
    // PostgreSQL 方言单元测试（不依赖真实数据库）
    // -----------------------------------------------------------------------

    use auroradb_core::connection::driver::DriverKind;
    use auroradb_core::connection::{
        ConnectionConfig, ConnectionRequest, PoolConfig, SafetyMode, SslConfig,
    };

    fn pg_column(
        name: &str,
        column_type: &str,
        nullable: bool,
        primary: bool,
    ) -> SchemaCompareColumn {
        let mut column = column(name, column_type);
        column.is_nullable = nullable;
        column.is_primary = primary;
        column
    }

    fn pg_index(name: &str, columns: &[&str], unique: bool) -> SchemaCompareIndex {
        SchemaCompareIndex {
            name: name.to_string(),
            columns: columns.iter().map(|column| column.to_string()).collect(),
            is_unique: unique,
            index_type: "btree".to_string(),
        }
    }

    fn pg_table(
        name: &str,
        schema: &str,
        columns: Vec<SchemaCompareColumn>,
        indexes: Vec<SchemaCompareIndex>,
    ) -> SchemaCompareTable {
        SchemaCompareTable {
            name: name.to_string(),
            columns,
            indexes,
            foreign_keys: Vec::new(),
            object_kind: SchemaObjectKind::Table,
            schema: Some(schema.to_string()),
            qualify_schema: false,
            create_statement: None,
        }
    }

    fn pg_view(name: &str, schema: &str, body: &str) -> SchemaCompareTable {
        SchemaCompareTable {
            name: name.to_string(),
            columns: vec![pg_column("id", "bigint", true, false)],
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            object_kind: SchemaObjectKind::View,
            schema: Some(schema.to_string()),
            qualify_schema: false,
            create_statement: Some(format!("CREATE VIEW {} AS {}", name, body)),
        }
    }

    fn pg_foreign_key(
        name: &str,
        columns: &[&str],
        ref_schema: &str,
        ref_table: &str,
        ref_columns: &[&str],
        on_delete: &str,
    ) -> SchemaCompareForeignKey {
        SchemaCompareForeignKey {
            name: name.to_string(),
            columns: columns.iter().map(|column| column.to_string()).collect(),
            ref_schema: Some(ref_schema.to_string()),
            ref_table: ref_table.to_string(),
            ref_columns: ref_columns
                .iter()
                .map(|column| column.to_string())
                .collect(),
            on_delete: on_delete.to_string(),
            on_update: "NO ACTION".to_string(),
        }
    }

    #[test]
    fn pg_create_table_statements_use_postgres_dialect() {
        let mut id = pg_column("id", "bigint", false, true);
        id.extra = "identity".to_string();
        let mut name = pg_column("name", "character varying(64)", false, false);
        name.comment = "用户名".to_string();
        let org_id = pg_column("org_id", "bigint", true, false);

        let mut users = pg_table(
            "users",
            "public",
            vec![id, name, org_id],
            vec![
                pg_index("users_pkey", &["id"], true),
                pg_index("idx_users_name", &["name"], false),
            ],
        );
        users.qualify_schema = true;
        users.foreign_keys.push(pg_foreign_key(
            "fk_users_org",
            &["org_id"],
            "public",
            "orgs",
            &["id"],
            "CASCADE",
        ));

        let statements = pg_create_table_statements(&users);
        let create = &statements[0];

        assert!(create.starts_with("CREATE TABLE \"public\".\"users\" ("));
        assert!(create.contains("\"id\" bigint GENERATED BY DEFAULT AS IDENTITY"));
        assert!(create.contains("CONSTRAINT \"users_pkey\" PRIMARY KEY (\"id\")"));
        assert!(create.contains(
            "CONSTRAINT \"fk_users_org\" FOREIGN KEY (\"org_id\") REFERENCES \"public\".\"orgs\" (\"id\") ON DELETE CASCADE"
        ));
        assert!(create.ends_with(");"));

        let script = statements.join("\n");
        assert!(script.contains("COMMENT ON COLUMN \"public\".\"users\".\"name\" IS '用户名';"));
        assert!(script.contains(
            "CREATE INDEX \"public\".\"idx_users_name\" ON \"public\".\"users\" (\"name\");"
        ));

        for forbidden in [
            "AUTO_INCREMENT",
            "MODIFY COLUMN",
            "CHANGE COLUMN",
            "ENGINE",
            "CHARSET",
            "`",
        ] {
            assert!(
                !script.contains(forbidden),
                "PG DDL 不应包含 MySQL 语法 {forbidden}: {script}"
            );
        }
    }

    #[test]
    fn pg_table_diff_generates_alter_ddl() {
        let source = vec![pg_table(
            "items",
            "schema_a",
            vec![
                pg_column("id", "bigint", false, true),
                pg_column("title", "character varying(32)", false, false),
                pg_column("legacy", "integer", true, false),
            ],
            vec![
                pg_index("items_pkey", &["id"], true),
                pg_index("idx_title", &["title"], false),
            ],
        )];
        let target = vec![pg_table(
            "items",
            "schema_b",
            vec![
                pg_column("id", "bigint", true, false),
                pg_column("title", "character varying(16)", false, false),
                pg_column("dropped", "integer", true, false),
            ],
            vec![pg_index("idx_old", &["title"], false)],
        )];

        let diff = diff_schemas_postgres("schema_a", "schema_b", &source, &target);
        let script = diff.statements.join("\n");

        assert_eq!(diff.summary.changed_tables, 1);
        assert_eq!(diff.summary.added_columns, 1);
        assert_eq!(diff.summary.removed_columns, 1);
        assert_eq!(diff.summary.changed_columns, 2);
        assert_eq!(diff.summary.added_indexes, 2);
        assert_eq!(diff.summary.removed_indexes, 1);

        assert!(script.starts_with("DROP INDEX \"idx_old\";"), "{script}");
        assert!(script.contains("ALTER TABLE \"items\""), "{script}");
        assert!(script.contains("ALTER COLUMN \"id\" SET NOT NULL"));
        assert!(script.contains(
            "ALTER COLUMN \"title\" TYPE character varying(32) USING \"title\"::character varying(32)"
        ));
        assert!(script.contains("ADD COLUMN \"legacy\" integer"));
        assert!(script.contains("DROP COLUMN \"dropped\""));
        assert!(script.contains("ADD CONSTRAINT \"items_pkey\" PRIMARY KEY (\"id\")"));
        assert!(script.contains("CREATE INDEX \"idx_title\" ON \"items\" (\"title\");"));
        assert!(script.contains("DROP INDEX \"idx_old\";"));

        for forbidden in ["AUTO_INCREMENT", "MODIFY COLUMN", "CHANGE COLUMN", "`"] {
            assert!(
                !script.contains(forbidden),
                "PG 迁移脚本不应包含 MySQL 语法 {forbidden}: {script}"
            );
        }
    }

    #[test]
    fn pg_foreign_key_change_drops_and_adds_constraint() {
        let mut source = pg_table(
            "child",
            "schema_a",
            vec![
                pg_column("id", "bigint", false, true),
                pg_column("parent_id", "bigint", true, false),
            ],
            vec![pg_index("child_pkey", &["id"], true)],
        );
        source.foreign_keys.push(pg_foreign_key(
            "fk_child_parent",
            &["parent_id"],
            "schema_a",
            "parent",
            &["id"],
            "CASCADE",
        ));

        let mut target = pg_table(
            "child",
            "schema_b",
            vec![
                pg_column("id", "bigint", false, true),
                pg_column("parent_id", "bigint", true, false),
            ],
            vec![pg_index("child_pkey", &["id"], true)],
        );
        target.foreign_keys.push(pg_foreign_key(
            "fk_child_parent",
            &["parent_id"],
            "schema_b",
            "parent",
            &["id"],
            "NO ACTION",
        ));

        let diff = diff_schemas_postgres("schema_a", "schema_b", &[source], &[target]);
        let script = diff.statements.join("\n");

        assert_eq!(diff.summary.changed_foreign_keys, 1);
        assert!(script.contains("DROP CONSTRAINT \"fk_child_parent\";"));
        assert!(script.contains(
            "ADD CONSTRAINT \"fk_child_parent\" FOREIGN KEY (\"parent_id\") REFERENCES \"parent\" (\"id\") ON DELETE CASCADE"
        ));
    }

    #[test]
    fn pg_view_diff_uses_create_or_replace() {
        let source = vec![pg_view(
            "v_users",
            "schema_a",
            "SELECT id, name FROM same_table;",
        )];
        let target = vec![pg_view("v_users", "schema_b", "SELECT id FROM same_table;")];

        let diff = diff_schemas_postgres("schema_a", "schema_b", &source, &target);

        assert_eq!(diff.summary.changed_tables, 1);
        assert_eq!(
            diff.statements,
            vec![
                "CREATE OR REPLACE VIEW \"v_users\" AS SELECT id, name FROM same_table;"
                    .to_string()
            ]
        );

        let identical = vec![pg_view("v_users", "schema_a", "SELECT id FROM same_table;")];
        let diff = diff_schemas_postgres("schema_a", "schema_b", &identical, &target);
        assert!(diff.tables.is_empty());
    }

    // -----------------------------------------------------------------------
    // PostgreSQL 真实库测试（需要设置 AURORADB_TEST_PG_* 环境变量后 --ignored 运行）
    // -----------------------------------------------------------------------

    async fn pg_test_connection() -> Option<(ConnectionManager, String, String)> {
        let host = std::env::var("AURORADB_TEST_PG_HOST").ok()?;
        let port = std::env::var("AURORADB_TEST_PG_PORT")
            .ok()?
            .parse::<u16>()
            .ok()?;
        let username = std::env::var("AURORADB_TEST_PG_USER").ok()?;
        let password = std::env::var("AURORADB_TEST_PG_PASSWORD").unwrap_or_default();
        let database = std::env::var("AURORADB_TEST_PG_DATABASE").ok()?;

        let manager = ConnectionManager::new();
        let connection_id = manager
            .connect(ConnectionRequest {
                config: ConnectionConfig {
                    id: "pg-schema-diff-test".to_string(),
                    name: "pg-schema-diff-test".to_string(),
                    driver: DriverKind::Postgres,
                    host,
                    port,
                    database: Some(database.clone()),
                    username,
                    password,
                    pool_config: PoolConfig::default(),
                    ssl_config: SslConfig::default(),
                    safety_mode: SafetyMode::Off,
                },
                ssh_config: None,
            })
            .await
            .ok()?;

        Some((manager, connection_id, database))
    }

    async fn pg_exec(
        manager: &ConnectionManager,
        connection_id: &str,
        database: &str,
        statements: &[String],
    ) {
        for statement in statements {
            if let Err(error) = manager.execute(connection_id, database, statement).await {
                panic!("执行测试 SQL 失败: {error}\n{statement}");
            }
        }
    }

    #[tokio::test]
    #[ignore = "需要真实 PostgreSQL 实例（设置 AURORADB_TEST_PG_* 后加 --ignored 运行）"]
    async fn pg_schema_diff_detects_changes_against_real_database() {
        let Some((manager, connection_id, database)) = pg_test_connection().await else {
            eprintln!("跳过：未设置 AURORADB_TEST_PG_* 环境变量");
            return;
        };

        let suffix = format!("sd_{}", std::process::id());
        let schema_a = format!("{suffix}_a");
        let schema_b = format!("{suffix}_b");
        let quote = |name: &str| pg_quote(name);

        let cleanup = vec![
            format!("DROP SCHEMA IF EXISTS {} CASCADE", quote(&schema_a)),
            format!("DROP SCHEMA IF EXISTS {} CASCADE", quote(&schema_b)),
        ];
        pg_exec(&manager, &connection_id, &database, &cleanup).await;

        let setup = vec![
            format!("CREATE SCHEMA {}", quote(&schema_a)),
            format!("CREATE SCHEMA {}", quote(&schema_b)),
            // 结构完全一致的表
            format!(
                "CREATE TABLE {}.same_table (id BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, name TEXT NOT NULL, amount NUMERIC(10, 2) DEFAULT 0)",
                quote(&schema_a)
            ),
            format!(
                "CREATE TABLE {}.same_table (id BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, name TEXT NOT NULL, amount NUMERIC(10, 2) DEFAULT 0)",
                quote(&schema_b)
            ),
            // 外键引用目标表
            format!("CREATE TABLE {}.parent (id BIGINT PRIMARY KEY)", quote(&schema_a)),
            format!("CREATE TABLE {}.parent (id BIGINT PRIMARY KEY)", quote(&schema_b)),
            // 列类型 / 可空 / 增删列 / 索引都不同
            format!(
                "CREATE TABLE {}.changed_table (id BIGINT PRIMARY KEY, title VARCHAR(32) NOT NULL, legacy_extra INTEGER)",
                quote(&schema_a)
            ),
            format!(
                "CREATE TABLE {}.changed_table (id BIGINT, title VARCHAR(16) NOT NULL, dropped_extra INTEGER)",
                quote(&schema_b)
            ),
            format!(
                "CREATE INDEX idx_changed_title_a ON {}.changed_table (title)",
                quote(&schema_a)
            ),
            format!(
                "CREATE INDEX idx_old ON {}.changed_table (title)",
                quote(&schema_b)
            ),
            // 外键动作不同（CASCADE vs NO ACTION）
            format!(
                "CREATE TABLE {}.child (id BIGINT PRIMARY KEY, parent_id BIGINT, CONSTRAINT fk_child_parent FOREIGN KEY (parent_id) REFERENCES {}.parent (id) ON DELETE CASCADE)",
                quote(&schema_a),
                quote(&schema_a)
            ),
            format!(
                "CREATE TABLE {}.child (id BIGINT, parent_id BIGINT, CONSTRAINT fk_child_parent FOREIGN KEY (parent_id) REFERENCES {}.parent (id) ON DELETE NO ACTION)",
                quote(&schema_b),
                quote(&schema_b)
            ),
            // 视图：两边定义相同（仅 schema 前缀不同，应归一化后视为一致）
            format!(
                "CREATE VIEW {}.v_users AS SELECT id, name FROM {}.same_table",
                quote(&schema_a),
                quote(&schema_a)
            ),
            format!(
                "CREATE VIEW {}.v_users AS SELECT id, name FROM {}.same_table",
                quote(&schema_b),
                quote(&schema_b)
            ),
            // 仅一侧存在的对象
            format!("CREATE TABLE {}.only_source (id BIGINT)", quote(&schema_a)),
            format!("CREATE TABLE {}.only_target (id BIGINT)", quote(&schema_b)),
        ];
        pg_exec(&manager, &connection_id, &database, &setup).await;

        let (source, source_scope) = load_pg_compare_schema(&manager, &connection_id, &schema_a)
            .await
            .expect("读取源 schema 失败");
        let (target, target_scope) = load_pg_compare_schema(&manager, &connection_id, &schema_b)
            .await
            .expect("读取目标 schema 失败");
        assert_eq!(source_scope.kind, PgScopeKind::Schema);
        assert_eq!(target_scope.kind, PgScopeKind::Schema);

        let diff = diff_schemas_postgres(&schema_a, &schema_b, &source, &target);
        let script = diff.statements.join("\n");

        // 数据库级口径：同一个库名应解析为跨 schema 的 Database 作用域
        let (database_tables, database_scope) =
            load_pg_compare_schema(&manager, &connection_id, &database)
                .await
                .expect("读取整库快照失败");
        assert_eq!(database_scope.kind, PgScopeKind::Database);
        let expected_display = format!("{}.same_table", schema_a);
        assert!(
            database_tables
                .iter()
                .any(|table| table.name == expected_display),
            "数据库级快照应包含 {expected_display}"
        );

        // 不存在的库 / schema 应报中文错误
        let missing = format!("{suffix}_missing");
        let error = load_pg_compare_schema(&manager, &connection_id, &missing)
            .await
            .expect_err("不存在的库或 schema 应当报错");
        assert!(
            error.to_string().contains("不存在"),
            "错误文案不符合预期: {error}"
        );

        // 断言完成后先清理自己创建的 schema，避免失败时留下对象
        pg_exec(&manager, &connection_id, &database, &cleanup).await;

        assert_eq!(diff.summary.added_tables, 1, "{script}");
        assert_eq!(diff.summary.removed_tables, 1, "{script}");
        assert!(diff.summary.changed_tables >= 2, "{script}");
        assert!(diff.summary.changed_foreign_keys >= 1, "{script}");

        assert!(
            script.contains("ALTER COLUMN \"title\" TYPE character varying(32) USING"),
            "{script}"
        );
        assert!(script.contains("ADD COLUMN \"legacy_extra\""), "{script}");
        assert!(script.contains("DROP COLUMN \"dropped_extra\""), "{script}");
        assert!(
            script.contains("CREATE INDEX \"idx_changed_title_a\""),
            "{script}"
        );
        assert!(script.contains("DROP INDEX \"idx_old\";"), "{script}");
        assert!(
            script.contains("ADD CONSTRAINT \"child_pkey\" PRIMARY KEY (\"id\")"),
            "{script}"
        );
        assert!(
            script.contains("DROP CONSTRAINT \"fk_child_parent\";"),
            "{script}"
        );
        assert!(
            script.contains("ADD CONSTRAINT \"fk_child_parent\" FOREIGN KEY"),
            "{script}"
        );
        assert!(script.contains("ON DELETE CASCADE"), "{script}");
        assert!(script.contains("CREATE TABLE \"only_source\""), "{script}");
        assert!(
            script.contains("DROP TABLE IF EXISTS \"only_target\";"),
            "{script}"
        );

        let changed_names: Vec<&str> = diff
            .tables
            .iter()
            .map(|table| table.name.as_str())
            .collect();
        assert!(
            !changed_names.contains(&"same_table"),
            "结构相同的表不应出现在差异中: {changed_names:?}"
        );
        assert!(
            !changed_names.contains(&"v_users"),
            "归一化后的视图定义相同，不应出现差异: {changed_names:?}"
        );

        for forbidden in [
            "AUTO_INCREMENT",
            "MODIFY COLUMN",
            "CHANGE COLUMN",
            "ENGINE=",
            "CHARSET",
            "`",
        ] {
            assert!(
                !script.contains(forbidden),
                "PG 迁移脚本不应包含 MySQL 语法 {forbidden}"
            );
        }
    }
}
