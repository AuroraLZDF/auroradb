//! 表级数据血缘提取（V3-5.1）
//!
//! 解析 SQL AST，推导每条语句的「源表 → 目标表」依赖。
//! 纯文本处理，无连接依赖；单条语句解析失败只计数不中断，
//! 其余语句正常出结果（fail-open 便于对整个脚本做血缘分析）。
//!
//! 支持的语句类型：
//! - `INSERT ... SELECT` / `REPLACE ... SELECT`（`INSERT ... VALUES/SET` 只记目标）
//! - `UPDATE`（JOIN / 多表 FROM / WHERE 子查询中的表都计入源表）
//! - `DELETE`（MySQL 多表删除 `DELETE t1 FROM t1 JOIN t2`、PG `DELETE ... USING`）
//! - `CREATE TABLE ... AS SELECT`（CTAS）/ `CREATE TABLE ... LIKE` / `... CLONE`
//! - `CREATE VIEW ... AS SELECT`
//! - `WITH` CTE 前缀 DML（CTE 别名不计入源表）
//! - 纯 `SELECT` 记录为无目标的查询（明细可见，图中不画）

use std::collections::HashSet;
use std::ops::ControlFlow;

use serde::Serialize;

use crate::connection::driver::DriverKind;
use crate::sql::split_sql_statements;
use sqlparser::ast::{
    CreateTableLikeKind, FromTable, ObjectName, ObjectNamePart, Query, SetExpr, Statement,
    TableFactor, TableObject, Visit, Visitor,
};
use sqlparser::dialect::{MySqlDialect, PostgreSqlDialect};
use sqlparser::parser::Parser;

/// 血缘中的表引用；`database` 为 `None` 表示无法从 SQL 推断所属库
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineageTableRef {
    pub database: Option<String>,
    pub table: String,
}

/// 单条语句的血缘分析结果
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineageStatement {
    /// 语句序号（按可识别语句顺序从 0 计数）
    pub index: usize,
    /// 操作类型：INSERT / REPLACE / UPDATE / DELETE / CREATE_TABLE_AS /
    /// CREATE_TABLE_LIKE / CREATE_TABLE_CLONE / CREATE_VIEW / SELECT
    pub operation: String,
    pub target: Option<LineageTableRef>,
    pub sources: Vec<LineageTableRef>,
    pub warnings: Vec<String>,
}

/// 整段 SQL 的血缘分析结果
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineageAnalysis {
    pub statements: Vec<LineageStatement>,
    /// 解析失败、无法推导的语句条数
    pub unparsed_count: usize,
}

/// 分析一段 SQL 脚本的表级血缘。
///
/// - `driver` 决定解析方言：PostgreSQL 用 PG 方言，其余（MySQL / SQLite）用 MySQL 方言
/// - `default_database` 用于给未限定库名的表补库名前缀
pub fn analyze_lineage(
    sql: &str,
    driver: DriverKind,
    default_database: Option<&str>,
) -> LineageAnalysis {
    let mut analysis = LineageAnalysis::default();
    for raw in split_sql_statements(sql) {
        match parse_statement(&raw, driver) {
            Some(statement) => {
                let mut ctes = HashSet::new();
                cte_names_statement(&statement, &mut ctes);
                if let Some(mut entry) = extract_statement(&statement, default_database, &ctes) {
                    entry.index = analysis.statements.len();
                    analysis.statements.push(entry);
                }
            }
            None => analysis.unparsed_count += 1,
        }
    }
    analysis
}

/// 按驱动选择方言解析单条语句
fn parse_statement(sql: &str, driver: DriverKind) -> Option<Statement> {
    let parsed = match driver {
        DriverKind::Postgres => Parser::parse_sql(&PostgreSqlDialect {}, sql),
        DriverKind::MySql | DriverKind::Sqlite => Parser::parse_sql(&MySqlDialect {}, sql),
    };
    parsed
        .ok()
        .and_then(|statements| statements.into_iter().next())
}

/// 提取单条语句的血缘；与本批无关的语句（SET / SHOW / 普通 DDL 等）返回 `None`。
/// `ctes` 是语句级（可能带 `WITH` 前缀）定义的 CTE 别名，需从源表中排除。
fn extract_statement(
    statement: &Statement,
    default_db: Option<&str>,
    ctes: &HashSet<String>,
) -> Option<LineageStatement> {
    match statement {
        Statement::Insert(insert) => {
            let target = match &insert.table {
                TableObject::TableName(name) => object_name_ref(name, default_db),
                TableObject::TableFunction(_) | TableObject::TableQuery(_) => None,
            }?;
            let operation = if insert.replace_into { "REPLACE" } else { "INSERT" };
            // 整语句扫描（覆盖源查询与 CTE 体），排除目标表与 CTE 别名；
            // INSERT ... VALUES / INSERT ... SET 没有其它关系，自然得到空源表
            let sources = sweep_sources(statement, Some(&target), default_db, ctes);
            Some(entry(operation, Some(target), sources))
        }
        Statement::Update(update) => {
            let target = match &update.table.relation {
                TableFactor::Table { name, .. } => object_name_ref(name, default_db),
                _ => None,
            }?;
            let sources = sweep_sources(statement, Some(&target), default_db, ctes);
            Some(entry("UPDATE", Some(target), sources))
        }
        Statement::Delete(delete) => {
            // MySQL 多表删除 `DELETE t1, t2 FROM t1 JOIN t2`：tables 非空时即目标
            let targets: Vec<LineageTableRef> = if !delete.tables.is_empty() {
                delete
                    .tables
                    .iter()
                    .filter_map(|name| object_name_ref(name, default_db))
                    .collect()
            } else {
                match &delete.from {
                    FromTable::WithFromKeyword(tables) | FromTable::WithoutKeyword(tables) => {
                        tables
                            .first()
                            .and_then(|twj| match &twj.relation {
                                TableFactor::Table { name, .. } => object_name_ref(name, default_db),
                                _ => None,
                            })
                            .into_iter()
                            .collect()
                    }
                }
            };
            let target = targets.first()?.clone();
            let sources = sweep_sources(statement, Some(&target), default_db, ctes);
            Some(entry("DELETE", Some(target), sources))
        }
        Statement::Query(query) => {
            // `WITH ... INSERT/UPDATE/DELETE` 解析为 Query 包裹的 DML 语句体：
            // 目标取内层 DML，源表整棵 Query 扫描（含 CTE 体里的基表）
            let inner = match query.body.as_ref() {
                SetExpr::Insert(inner) | SetExpr::Update(inner) | SetExpr::Delete(inner) => inner,
                // 纯查询：记录涉及的表，图中不画（无目标）
                _ => {
                    return Some(entry("SELECT", None, sweep_sources(query, None, default_db, ctes)));
                }
            };
            let target = dml_target(inner, default_db)?;
            let operation = dml_operation(inner)?;
            let sources = sweep_sources(query, Some(&target), default_db, ctes);
            Some(entry(operation, Some(target), sources))
        }
        Statement::CreateTable(create) => {
            let target = object_name_ref(&create.name, default_db)?;
            if let Some(query) = &create.query {
                Some(entry(
                    "CREATE_TABLE_AS",
                    Some(target),
                    sweep_sources(query, None, default_db, ctes),
                ))
            } else if let Some(like) = &create.like {
                let source = match like {
                    CreateTableLikeKind::Plain(like) | CreateTableLikeKind::Parenthesized(like) => {
                        &like.name
                    }
                };
                Some(entry(
                    "CREATE_TABLE_LIKE",
                    Some(target),
                    object_name_ref(source, default_db).into_iter().collect(),
                ))
            } else if let Some(clone) = &create.clone {
                Some(entry(
                    "CREATE_TABLE_CLONE",
                    Some(target),
                    object_name_ref(clone, default_db).into_iter().collect(),
                ))
            } else {
                None
            }
        }
        Statement::CreateView(view) => Some(entry(
            "CREATE_VIEW",
            Some(object_name_ref(&view.name, default_db)?),
            sweep_sources(&view.query, None, default_db, ctes),
        )),
        _ => None,
    }
}

fn entry(
    operation: &str,
    target: Option<LineageTableRef>,
    sources: Vec<LineageTableRef>,
) -> LineageStatement {
    LineageStatement {
        index: 0,
        operation: operation.to_string(),
        target,
        sources,
        warnings: Vec::new(),
    }
}

/// 收集节点（语句或查询）中出现的关系（含 JOIN / 子查询 / 表达式内子查询 /
/// CTE 体），排除目标表与 CTE 别名后作为源表。
fn sweep_sources<T: Visit>(
    node: &T,
    target: Option<&LineageTableRef>,
    default_db: Option<&str>,
    ctes: &HashSet<String>,
) -> Vec<LineageTableRef> {
    let mut collector = RelationCollector::default();
    let _ = node.visit(&mut collector);

    let mut sources = Vec::new();
    for name in collector.names {
        let reference = match object_name_ref(&name, default_db) {
            Some(reference) => reference,
            None => continue,
        };
        if ctes.contains(&relation_key(&reference)) {
            continue;
        }
        if let Some(target) = target {
            if same_table(&reference, target) {
                continue;
            }
        }
        push_unique(&mut sources, reference);
    }
    sources
}

/// DML 语句的写入目标表（WITH 前缀场景下从内层语句取）
fn dml_target(statement: &Statement, default_db: Option<&str>) -> Option<LineageTableRef> {
    match statement {
        Statement::Insert(insert) => match &insert.table {
            TableObject::TableName(name) => object_name_ref(name, default_db),
            TableObject::TableFunction(_) | TableObject::TableQuery(_) => None,
        },
        Statement::Update(update) => match &update.table.relation {
            TableFactor::Table { name, .. } => object_name_ref(name, default_db),
            _ => None,
        },
        Statement::Delete(delete) => {
            if !delete.tables.is_empty() {
                object_name_ref(&delete.tables[0], default_db)
            } else {
                match &delete.from {
                    FromTable::WithFromKeyword(tables) | FromTable::WithoutKeyword(tables) => {
                        tables.first().and_then(|twj| match &twj.relation {
                            TableFactor::Table { name, .. } => object_name_ref(name, default_db),
                            _ => None,
                        })
                    }
                }
            }
        }
        _ => None,
    }
}

/// DML 语句的操作名（WITH 前缀场景下从内层语句取）
fn dml_operation(statement: &Statement) -> Option<&'static str> {
    match statement {
        Statement::Insert(insert) => {
            if insert.replace_into {
                Some("REPLACE")
            } else {
                Some("INSERT")
            }
        }
        Statement::Update(_) => Some("UPDATE"),
        Statement::Delete(_) => Some("DELETE"),
        _ => None,
    }
}

#[derive(Default)]
struct RelationCollector {
    names: Vec<ObjectName>,
}

impl Visitor for RelationCollector {
    type Break = ();

    fn pre_visit_relation(&mut self, relation: &ObjectName) -> ControlFlow<Self::Break> {
        self.names.push(relation.clone());
        ControlFlow::Continue(())
    }
}

/// 收集语句内定义的全部 CTE 别名（含嵌套 CTE）
fn cte_names_statement(statement: &Statement, names: &mut HashSet<String>) {
    match statement {
        Statement::Query(query) => cte_names_query(query, names),
        Statement::Insert(insert) => {
            if let Some(source) = &insert.source {
                cte_names_query(source, names);
            }
        }
        Statement::CreateTable(create) => {
            if let Some(query) = &create.query {
                cte_names_query(query, names);
            }
        }
        Statement::CreateView(view) => cte_names_query(&view.query, names),
        _ => {}
    }
}

fn cte_names_query(query: &Query, names: &mut HashSet<String>) {
    if let Some(with) = &query.with {
        for cte in &with.cte_tables {
            names.insert(cte.alias.name.value.to_lowercase());
            cte_names_query(&cte.query, names);
        }
    }
    cte_names_set_expr(query.body.as_ref(), names);
}

fn cte_names_set_expr(set_expr: &SetExpr, names: &mut HashSet<String>) {
    match set_expr {
        SetExpr::Query(nested) => cte_names_query(nested, names),
        SetExpr::SetOperation { left, right, .. } => {
            cte_names_set_expr(left, names);
            cte_names_set_expr(right, names);
        }
        SetExpr::Insert(inner) | SetExpr::Update(inner) | SetExpr::Delete(inner) => {
            cte_names_statement(inner, names)
        }
        _ => {}
    }
}

/// `ObjectName` 转表引用：末段为表名，倒数第二段为库名；
/// 只有一段时补默认库名。库名段取 `db.table` 的 `db`——
/// PostgreSQL `schema.table` 场景下即 schema 名，跨库语义按库名近似处理。
fn object_name_ref(name: &ObjectName, default_db: Option<&str>) -> Option<LineageTableRef> {
    let parts: Vec<&str> = name
        .0
        .iter()
        .filter_map(|part| match part {
            ObjectNamePart::Identifier(ident) => Some(ident.value.as_str()),
            _ => None,
        })
        .collect();
    let table = parts.last()?.to_string();
    let database = if parts.len() >= 2 {
        Some(parts[parts.len() - 2].to_string())
    } else {
        default_db.map(str::to_string)
    };
    Some(LineageTableRef { database, table })
}

/// 同表判断：表名忽略大小写，库名一致（均未知视为一致）
fn same_table(a: &LineageTableRef, b: &LineageTableRef) -> bool {
    a.table.eq_ignore_ascii_case(&b.table) && a.database == b.database
}

/// CTE 排除键：表名小写即可（CTE 别名总是不带库名引用）
fn relation_key(reference: &LineageTableRef) -> String {
    reference.table.to_lowercase()
}

fn push_unique(list: &mut Vec<LineageTableRef>, reference: LineageTableRef) {
    if !list.contains(&reference) {
        list.push(reference);
    }
}

#[cfg(test)]
mod tests {
    use super::{analyze_lineage, LineageTableRef};
    use crate::connection::driver::DriverKind;

    fn db(table: &str) -> LineageTableRef {
        LineageTableRef {
            database: Some("mydb".to_string()),
            table: table.to_string(),
        }
    }

    fn plain(table: &str) -> LineageTableRef {
        LineageTableRef {
            database: None,
            table: table.to_string(),
        }
    }

    fn analyze_mysql(sql: &str) -> super::LineageAnalysis {
        analyze_lineage(sql, DriverKind::MySql, Some("mydb"))
    }

    fn assert_sources(actual: &[LineageTableRef], expected: Vec<LineageTableRef>) {
        assert_eq!(actual, &expected, "sources mismatch");
    }

    #[test]
    fn insert_select_basic() {
        let result = analyze_mysql("INSERT INTO logs (a, b) SELECT x, y FROM events");
        assert_eq!(result.unparsed_count, 0);
        assert_eq!(result.statements.len(), 1);
        let statement = &result.statements[0];
        assert_eq!(statement.operation, "INSERT");
        assert_eq!(statement.target.as_ref().unwrap(), &db("logs"));
        assert_sources(&statement.sources, vec![db("events")]);
    }

    #[test]
    fn insert_select_join_collects_all_sources() {
        let result = analyze_mysql(
            "INSERT INTO summary SELECT u.id, o.amount FROM users u JOIN orders o ON u.id = o.user_id",
        );
        let statement = &result.statements[0];
        assert_sources(&statement.sources, vec![db("users"), db("orders")]);
    }

    #[test]
    fn insert_values_has_no_source() {
        let result = analyze_mysql("INSERT INTO t (a) VALUES (1)");
        let statement = &result.statements[0];
        assert_eq!(statement.operation, "INSERT");
        assert_eq!(statement.target.as_ref().unwrap(), &db("t"));
        assert!(statement.sources.is_empty());
    }

    #[test]
    fn insert_set_has_no_source() {
        let result = analyze_mysql("INSERT INTO t SET a = 1");
        let statement = &result.statements[0];
        assert_eq!(statement.target.as_ref().unwrap(), &db("t"));
        assert!(statement.sources.is_empty());
    }

    #[test]
    fn replace_into_detected() {
        let result = analyze_mysql("REPLACE INTO t SELECT * FROM s");
        let statement = &result.statements[0];
        assert_eq!(statement.operation, "REPLACE");
        assert_eq!(statement.target.as_ref().unwrap(), &db("t"));
        assert_sources(&statement.sources, vec![db("s")]);
    }

    #[test]
    fn update_join_excludes_target_from_sources() {
        let result = analyze_mysql(
            "UPDATE users JOIN orders ON users.id = orders.user_id SET users.name = 'x' WHERE orders.amount > 0",
        );
        let statement = &result.statements[0];
        assert_eq!(statement.operation, "UPDATE");
        assert_eq!(statement.target.as_ref().unwrap(), &db("users"));
        assert_sources(&statement.sources, vec![db("orders")]);
    }

    #[test]
    fn update_subquery_table_is_source() {
        let result = analyze_mysql(
            "UPDATE users SET banned = 1 WHERE id IN (SELECT user_id FROM violations)",
        );
        let statement = &result.statements[0];
        assert_eq!(statement.target.as_ref().unwrap(), &db("users"));
        assert_sources(&statement.sources, vec![db("violations")]);
    }

    #[test]
    fn postgres_update_from() {
        let result = analyze_lineage(
            "UPDATE users SET name = s.name FROM staging_users s WHERE users.id = s.id",
            DriverKind::Postgres,
            Some("public"),
        );
        let statement = &result.statements[0];
        assert_eq!(statement.operation, "UPDATE");
        assert_eq!(
            statement.target.as_ref().unwrap(),
            &LineageTableRef {
                database: Some("public".to_string()),
                table: "users".to_string(),
            }
        );
        assert_sources(
            &statement.sources,
            vec![LineageTableRef {
                database: Some("public".to_string()),
                table: "staging_users".to_string(),
            }],
        );
    }

    #[test]
    fn delete_single_table() {
        let result = analyze_mysql("DELETE FROM logs WHERE created_at < '2020-01-01'");
        let statement = &result.statements[0];
        assert_eq!(statement.operation, "DELETE");
        assert_eq!(statement.target.as_ref().unwrap(), &db("logs"));
        assert!(statement.sources.is_empty());
    }

    #[test]
    fn mysql_multi_table_delete() {
        let result = analyze_mysql("DELETE t1 FROM t1 JOIN t2 ON t1.id = t2.id WHERE t2.stale = 1");
        let statement = &result.statements[0];
        assert_eq!(statement.target.as_ref().unwrap(), &db("t1"));
        assert_sources(&statement.sources, vec![db("t2")]);
    }

    #[test]
    fn postgres_delete_using() {
        let result = analyze_lineage(
            "DELETE FROM products USING obsolete WHERE products.id = obsolete.id",
            DriverKind::Postgres,
            None,
        );
        let statement = &result.statements[0];
        assert_eq!(statement.operation, "DELETE");
        assert_eq!(statement.target.as_ref().unwrap(), &plain("products"));
        assert_sources(&statement.sources, vec![plain("obsolete")]);
    }

    #[test]
    fn create_table_as_select() {
        let result = analyze_mysql("CREATE TABLE backup_users AS SELECT * FROM users");
        let statement = &result.statements[0];
        assert_eq!(statement.operation, "CREATE_TABLE_AS");
        assert_eq!(statement.target.as_ref().unwrap(), &db("backup_users"));
        assert_sources(&statement.sources, vec![db("users")]);
    }

    #[test]
    fn ctas_excludes_cte_alias() {
        let result = analyze_mysql(
            "CREATE TABLE hot_users AS WITH recent AS (SELECT * FROM logins) SELECT * FROM recent",
        );
        let statement = &result.statements[0];
        assert_eq!(statement.operation, "CREATE_TABLE_AS");
        assert_sources(&statement.sources, vec![db("logins")]);
        assert!(!statement.sources.iter().any(|s| s.table == "recent"));
    }

    #[test]
    fn create_table_like_and_clone() {
        let result = analyze_mysql("CREATE TABLE t_copy LIKE users");
        let statement = &result.statements[0];
        assert_eq!(statement.operation, "CREATE_TABLE_LIKE");
        assert_eq!(statement.target.as_ref().unwrap(), &db("t_copy"));
        assert_sources(&statement.sources, vec![db("users")]);

        let result = analyze_mysql("CREATE TABLE t_clone CLONE users");
        let statement = &result.statements[0];
        assert_eq!(statement.operation, "CREATE_TABLE_CLONE");
        assert_sources(&statement.sources, vec![db("users")]);
    }

    #[test]
    fn create_view_as_select() {
        let result = analyze_mysql("CREATE VIEW v_active AS SELECT * FROM users WHERE active = 1");
        let statement = &result.statements[0];
        assert_eq!(statement.operation, "CREATE_VIEW");
        assert_eq!(statement.target.as_ref().unwrap(), &db("v_active"));
        assert_sources(&statement.sources, vec![db("users")]);
    }

    #[test]
    fn with_insert_excludes_cte_alias() {
        let result = analyze_mysql(
            "WITH top AS (SELECT id FROM orders GROUP BY user_id) INSERT INTO ranks SELECT * FROM top",
        );
        let statement = &result.statements[0];
        assert_eq!(statement.operation, "INSERT");
        assert_eq!(statement.target.as_ref().unwrap(), &db("ranks"));
        assert_sources(&statement.sources, vec![db("orders")]);
    }

    #[test]
    fn with_update_excludes_cte_alias() {
        let result = analyze_mysql(
            "WITH stale AS (SELECT id FROM retired) UPDATE users SET gone = 1 WHERE id IN (SELECT id FROM stale)",
        );
        let statement = &result.statements[0];
        assert_eq!(statement.operation, "UPDATE");
        assert_eq!(statement.target.as_ref().unwrap(), &db("users"));
        assert_sources(&statement.sources, vec![db("retired")]);
    }

    #[test]
    fn cross_database_references_preserved() {
        let result = analyze_mysql("INSERT INTO db1.t SELECT * FROM db2.s");
        let statement = &result.statements[0];
        assert_eq!(
            statement.target.as_ref().unwrap(),
            &LineageTableRef {
                database: Some("db1".to_string()),
                table: "t".to_string(),
            }
        );
        assert_sources(
            &statement.sources,
            vec![LineageTableRef {
                database: Some("db2".to_string()),
                table: "s".to_string(),
            }],
        );
    }

    #[test]
    fn pure_select_recorded_without_target() {
        let result = analyze_mysql("SELECT * FROM users");
        assert_eq!(result.statements.len(), 1);
        let statement = &result.statements[0];
        assert_eq!(statement.operation, "SELECT");
        assert!(statement.target.is_none());
        assert_sources(&statement.sources, vec![db("users")]);
    }

    #[test]
    fn unparsed_statements_counted_without_breaking_batch() {
        let result = analyze_mysql("SELECT * FROM; INSERT INTO t SELECT * FROM s");
        assert_eq!(result.unparsed_count, 1);
        assert_eq!(result.statements.len(), 1);
        assert_eq!(result.statements[0].operation, "INSERT");
    }

    #[test]
    fn unrelated_statements_skipped() {
        let result = analyze_mysql("SET NAMES utf8mb4; SHOW TABLES; TRUNCATE TABLE t");
        assert_eq!(result.unparsed_count, 0);
        assert!(result.statements.is_empty());
    }

    #[test]
    fn statement_indexes_are_sequential() {
        let result = analyze_mysql("INSERT INTO a SELECT * FROM b; UPDATE a SET x = 1; SELECT 1");
        assert_eq!(result.statements.len(), 3);
        for (index, statement) in result.statements.iter().enumerate() {
            assert_eq!(statement.index, index);
        }
    }
}
