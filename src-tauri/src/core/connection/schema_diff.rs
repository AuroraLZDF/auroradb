//! Schema 对比
//! 对比两个库的基表结构（列 + 索引），输出差异明细与可执行的迁移脚本。
//! 方向约定：source 为基准（源库），target 为待同步（目标库）；
//! Added 表示仅源库存在（需要 ADD/CREATE），Removed 表示仅目标库存在（需要 DROP）。

use super::manager::ConnectionManager;
use super::metadata::with_metadata_timeout;
use super::types::*;
use super::validation::{escape_string, validate_identifier};
use anyhow::{anyhow, Result};
use std::collections::HashMap;

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

impl ConnectionManager {
    /// 对比两个连接上的库结构，返回差异与迁移脚本
    pub async fn compare_schemas(
        &self,
        source_connection: &str,
        source_database: &str,
        target_connection: &str,
        target_database: &str,
    ) -> Result<SchemaDiff> {
        let source = self
            .load_compare_schema(source_connection, source_database)
            .await?;
        let target = self
            .load_compare_schema(target_connection, target_database)
            .await?;

        let mut diff = diff_schemas(source_database, target_database, &source, &target);

        // 新增表优先使用源库真实建表语句（含引擎/字符集/外键等），失败时保留兜底语句
        for table in diff
            .tables
            .iter_mut()
            .filter(|t| t.change == DiffChange::Added)
        {
            match self
                .show_create_table(source_connection, source_database, &table.name)
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

    /// 读取参与对比的表结构：仅基表，列与索引各一条查询
    async fn load_compare_schema(
        &self,
        connection_id: &str,
        database: &str,
    ) -> Result<Vec<SchemaCompareTable>> {
        validate_identifier(database).map_err(|e| anyhow!("数据库名验证失败: {}", e))?;

        let connections = self.connections.read().await;
        let conn = connections
            .get(connection_id)
            .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;
        let pool = conn
            .pool
            .as_ref()
            .ok_or_else(|| anyhow!("连接不可用: {}", connection_id))?
            .as_mysql("Schema 对比")?;

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
        &self,
        connection_id: &str,
        database: &str,
        table: &str,
    ) -> Result<String> {
        validate_identifier(database).map_err(|e| anyhow!("数据库名验证失败: {}", e))?;

        let connections = self.connections.read().await;
        let conn = connections
            .get(connection_id)
            .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;
        let pool = conn
            .pool
            .as_ref()
            .ok_or_else(|| anyhow!("连接不可用: {}", connection_id))?
            .as_mysql("Schema 对比")?;

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
        });
    }

    for (table, name, column_type, is_nullable, default_value, extra, comment, ordinal, generation_expression) in column_rows {
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

    let source_column_names: HashMap<&str, ()> =
        source.columns.iter().map(|c| (c.name.as_str(), ())).collect();
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

    let target_indexes: HashMap<&str, &SchemaCompareIndex> =
        target.indexes.iter().map(|i| (i.name.as_str(), i)).collect();

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

    let source_index_names: HashMap<&str, ()> =
        source.indexes.iter().map(|i| (i.name.as_str(), ())).collect();
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
            parts.push(format!("DEFAULT {}", render_default(default, &column.extra)));
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

    fn table(name: &str, columns: Vec<SchemaCompareColumn>, indexes: Vec<SchemaCompareIndex>) -> SchemaCompareTable {
        SchemaCompareTable {
            name: name.to_string(),
            columns,
            indexes,
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
            vec![index("PRIMARY", &["id"], true), index("idx_old", &["email"], false)],
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

        assert_eq!(definition, "`status` varchar(20) NOT NULL DEFAULT 'new' COMMENT '状态'");
    }

    #[test]
    fn keeps_numeric_and_temporal_defaults_unquoted() {
        assert_eq!(render_default("0", ""), "0");
        assert_eq!(render_default("1.5", ""), "1.5");
        assert_eq!(render_default("CURRENT_TIMESTAMP", "DEFAULT_GENERATED"), "CURRENT_TIMESTAMP");
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
}
