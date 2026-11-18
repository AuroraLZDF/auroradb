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
    CreateTableLikeKind, Expr, FromTable, Insert, ObjectName, ObjectNamePart, Query, Select,
    SelectItem, SetExpr, Statement, TableFactor, TableObject, TableWithJoins, Update, Visit,
    Visitor,
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

/// 列级血缘中的列引用：所属表 + 列名
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineageColumnRef {
    pub table: LineageTableRef,
    pub column: String,
}

/// 单个目标列的字段级依赖（V3-5.2）
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnLineage {
    pub target: LineageColumnRef,
    pub sources: Vec<LineageColumnRef>,
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
    /// 列级依赖；无法推导列名时为空（如 `SELECT *`、UNION 主体、VALUES 插入）
    pub column_lineage: Vec<ColumnLineage>,
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
            let column_lineage = insert
                .source
                .as_deref()
                .map(|source| insert_column_lineage(insert, source, &target, default_db, ctes))
                .unwrap_or_default();
            Some(entry(operation, Some(target), sources, column_lineage))
        }
        Statement::Update(update) => {
            let target = match &update.table.relation {
                TableFactor::Table { name, .. } => object_name_ref(name, default_db),
                _ => None,
            }?;
            let sources = sweep_sources(statement, Some(&target), default_db, ctes);
            let column_lineage = update_column_lineage(update, &target, default_db, ctes);
            Some(entry("UPDATE", Some(target), sources, column_lineage))
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
            Some(entry("DELETE", Some(target), sources, Vec::new()))
        }
        Statement::Query(query) => {
            // `WITH ... INSERT/UPDATE/DELETE` 解析为 Query 包裹的 DML 语句体：
            // 目标取内层 DML，源表整棵 Query 扫描（含 CTE 体里的基表）
            let inner = match query.body.as_ref() {
                SetExpr::Insert(inner) | SetExpr::Update(inner) | SetExpr::Delete(inner) => inner,
                // 纯查询：记录涉及的表，图中不画（无目标）
                _ => {
                    return Some(entry(
                        "SELECT",
                        None,
                        sweep_sources(query, None, default_db, ctes),
                        Vec::new(),
                    ));
                }
            };
            let target = dml_target(inner, default_db)?;
            let operation = dml_operation(inner)?;
            let sources = sweep_sources(query, Some(&target), default_db, ctes);
            let column_lineage = match inner {
                Statement::Insert(insert) => insert
                    .source
                    .as_deref()
                    .map(|source| insert_column_lineage(insert, source, &target, default_db, ctes))
                    .unwrap_or_default(),
                Statement::Update(update) => update_column_lineage(update, &target, default_db, ctes),
                _ => Vec::new(),
            };
            Some(entry(operation, Some(target), sources, column_lineage))
        }
        Statement::CreateTable(create) => {
            let target = object_name_ref(&create.name, default_db)?;
            if let Some(query) = &create.query {
                let column_lineage = query_column_lineage(&target, query, default_db, ctes);
                Some(entry(
                    "CREATE_TABLE_AS",
                    Some(target),
                    sweep_sources(query, None, default_db, ctes),
                    column_lineage,
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
                    Vec::new(),
                ))
            } else {
                // CLONE：克隆源表（整表结构复制，无列级表达式）
                let sources: Option<Vec<LineageTableRef>> = create
                    .clone
                    .as_ref()
                    .map(|clone| object_name_ref(clone, default_db).into_iter().collect());
                Some(entry(
                    "CREATE_TABLE_CLONE",
                    Some(target),
                    sources?,
                    Vec::new(),
                ))
            }
        }
        Statement::CreateView(view) => {
            let target = object_name_ref(&view.name, default_db)?;
            let column_lineage = query_column_lineage(&target, &view.query, default_db, ctes);
            Some(entry(
                "CREATE_VIEW",
                Some(target),
                sweep_sources(&view.query, None, default_db, ctes),
                column_lineage,
            ))
        }
        _ => None,
    }
}

fn entry(
    operation: &str,
    target: Option<LineageTableRef>,
    sources: Vec<LineageTableRef>,
    column_lineage: Vec<ColumnLineage>,
) -> LineageStatement {
    LineageStatement {
        index: 0,
        operation: operation.to_string(),
        target,
        sources,
        column_lineage,
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

/// `ObjectName` 的标识符段（忽略非标识符部分）
fn object_name_parts(name: &ObjectName) -> Vec<&str> {
    name.0
        .iter()
        .filter_map(|part| match part {
            ObjectNamePart::Identifier(ident) => Some(ident.value.as_str()),
            _ => None,
        })
        .collect()
}

/// `ObjectName` 转表引用：末段为表名，倒数第二段为库名；
/// 只有一段时补默认库名。库名段取 `db.table` 的 `db`——
/// PostgreSQL `schema.table` 场景下即 schema 名，跨库语义按库名近似处理。
fn object_name_ref(name: &ObjectName, default_db: Option<&str>) -> Option<LineageTableRef> {
    let parts = object_name_parts(name);
    let table = parts.last()?.to_string();
    let database = if parts.len() >= 2 {
        Some(parts[parts.len() - 2].to_string())
    } else {
        default_db.map(str::to_string)
    };
    Some(LineageTableRef { database, table })
}

/// `ObjectName` 末段作为列名（INSERT 列清单 / UPDATE 赋值左侧）
fn object_name_column(name: &ObjectName) -> Option<String> {
    object_name_parts(name).last().map(|part| (*part).to_string())
}

// ---------- 列级血缘（V3-5.2） ----------

/// 收集 FROM 子句（含 JOIN）中的「别名/表名 → 表引用」映射，
/// 用于把限定列名（`u.name`、`orders.amount`）归到真实表。
fn relation_keys(
    twjs: &[TableWithJoins],
    default_db: Option<&str>,
) -> Vec<(String, LineageTableRef)> {
    let mut out = Vec::new();
    for twj in twjs {
        collect_factor_keys(&twj.relation, default_db, &mut out);
        for join in &twj.joins {
            collect_factor_keys(&join.relation, default_db, &mut out);
        }
    }
    out
}

fn collect_factor_keys(
    factor: &TableFactor,
    default_db: Option<&str>,
    out: &mut Vec<(String, LineageTableRef)>,
) {
    if let TableFactor::Table { name, alias, .. } = factor {
        if let Some(reference) = object_name_ref(name, default_db) {
            if let Some(alias) = alias {
                out.push((alias.name.value.to_lowercase(), reference.clone()));
            }
            out.push((reference.table.to_lowercase(), reference));
        }
    }
}

/// 从 SELECT 投影提取 (列名, 表达式)；通配符项丢弃。
/// 列名仅在可确定时给出：裸标识符或显式别名。
fn projection_items(select: &Select) -> Vec<(Option<String>, Option<Expr>)> {
    select
        .projection
        .iter()
        .filter_map(|item| match item {
            SelectItem::UnnamedExpr(Expr::Identifier(ident)) => {
                Some((Some(ident.value.clone()), Some(Expr::Identifier(ident.clone()))))
            }
            SelectItem::UnnamedExpr(expr) => Some((None, Some(expr.clone()))),
            SelectItem::ExprWithAlias { expr, alias } => {
                Some((Some(alias.value.clone()), Some(expr.clone())))
            }
            SelectItem::ExprWithAliases { expr, aliases } => {
                Some((aliases.first().map(|a| a.value.clone()), Some(expr.clone())))
            }
            SelectItem::QualifiedWildcard(..) | SelectItem::Wildcard(..) => None,
        })
        .collect()
}

/// 表达式中的列引用（V3-5.2）。
/// 只收集顶层作用域的引用：子查询（`IN (SELECT ...)` 等）内的列
/// 归属其自身作用域，不做解析；限定名按别名映射归属，无限定名
/// 仅在顶层只有一个关系时归属（否则歧义，跳过）。
fn expr_columns(expr: &Expr, relations: &[(String, LineageTableRef)]) -> Vec<LineageColumnRef> {
    let mut collector = ColumnRefCollector {
        relations,
        refs: Vec::new(),
        depth: 0,
    };
    let _ = expr.visit(&mut collector);
    collector.refs
}

struct ColumnRefCollector<'a> {
    relations: &'a [(String, LineageTableRef)],
    refs: Vec<LineageColumnRef>,
    /// 子查询深度：>0 表示正处于子查询内部，跳过其中的列引用
    depth: usize,
}

impl Visitor for ColumnRefCollector<'_> {
    type Break = ();

    fn pre_visit_query(&mut self, _query: &Query) -> ControlFlow<Self::Break> {
        self.depth += 1;
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, _query: &Query) -> ControlFlow<Self::Break> {
        self.depth = self.depth.saturating_sub(1);
        ControlFlow::Continue(())
    }

    fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<Self::Break> {
        if self.depth > 0 {
            return ControlFlow::Continue(());
        }
        let parts: Vec<&str> = match expr {
            Expr::Identifier(ident) => vec![ident.value.as_str()],
            Expr::CompoundIdentifier(idents) => {
                idents.iter().map(|ident| ident.value.as_str()).collect()
            }
            _ => return ControlFlow::Continue(()),
        };
        let column = match parts.last() {
            Some(column) => *column,
            None => return ControlFlow::Continue(()),
        };
        let reference = if parts.len() >= 2 {
            let qualifier = parts[parts.len() - 2];
            match resolve_qualifier(qualifier, self.relations) {
                Some(reference) => reference,
                None => return ControlFlow::Continue(()),
            }
        } else {
            // 无限定列名：仅当顶层只有一个关系时可归属
            let mut distinct: Vec<&LineageTableRef> = Vec::new();
            for (_, reference) in self.relations {
                if !distinct.contains(&reference) {
                    distinct.push(reference);
                }
            }
            match distinct.as_slice() {
                [only] => (*only).clone(),
                _ => return ControlFlow::Continue(()),
            }
        };
        push_column_unique(
            &mut self.refs,
            LineageColumnRef {
                table: reference,
                column: column.to_string(),
            },
        );
        ControlFlow::Continue(())
    }
}

fn resolve_qualifier(
    qualifier: &str,
    relations: &[(String, LineageTableRef)],
) -> Option<LineageTableRef> {
    let key = qualifier.to_lowercase();
    relations
        .iter()
        .find(|(alias, _)| *alias == key)
        .map(|(_, reference)| reference.clone())
}

/// INSERT ... SELECT 的列级血缘：INSERT 列清单与 SELECT 投影按位置对齐；
/// 无列清单时以投影名/别名作为目标列名（仅对齐可确定名称的项）。
fn insert_column_lineage(
    insert: &Insert,
    source: &Query,
    target: &LineageTableRef,
    default_db: Option<&str>,
    ctes: &HashSet<String>,
) -> Vec<ColumnLineage> {
    // UNION 等集合操作的分支投影可能不一致，不做列对齐
    let SetExpr::Select(select) = source.body.as_ref() else {
        return Vec::new();
    };
    let relations = relation_keys(&select.from, default_db);
    let items = projection_items(select);

    if insert.columns.is_empty() {
        // 无列清单：投影名即目标列名
        let mut result = Vec::new();
        for (name, expr) in items {
            if let (Some(name), Some(expr)) = (name, expr) {
                append_column_lineage(&mut result, target, &name, &expr, &relations, ctes, false);
            }
        }
        result
    } else {
        let target_columns: Vec<String> =
            insert.columns.iter().filter_map(object_name_column).collect();
        // 通配符或投影数与列数不一致时无法按位置对齐
        if items.len() != target_columns.len() {
            return Vec::new();
        }
        let mut result = Vec::new();
        for ((_, expr), column) in items.into_iter().zip(target_columns) {
            if let Some(expr) = expr {
                append_column_lineage(&mut result, target, &column, &expr, &relations, ctes, true);
            }
        }
        result
    }
}

/// UPDATE 赋值右侧表达式的列级血缘：`SET col = expr` 中 expr 的列引用 → 目标列
fn update_column_lineage(
    update: &Update,
    target: &LineageTableRef,
    default_db: Option<&str>,
    ctes: &HashSet<String>,
) -> Vec<ColumnLineage> {
    let mut twjs = vec![update.table.clone()];
    if let Some(kind) = &update.from {
        match kind {
            sqlparser::ast::UpdateTableFromKind::BeforeSet(from)
            | sqlparser::ast::UpdateTableFromKind::AfterSet(from) => {
                twjs.extend(from.iter().cloned())
            }
        }
    }
    let relations = relation_keys(&twjs, default_db);
    let mut result = Vec::new();
    for assignment in &update.assignments {
        let target_columns: Vec<String> = match &assignment.target {
            sqlparser::ast::AssignmentTarget::ColumnName(name) => {
                object_name_column(name).into_iter().collect()
            }
            sqlparser::ast::AssignmentTarget::Tuple(names) => {
                names.iter().filter_map(object_name_column).collect()
            }
        };
        for column in target_columns {
            append_column_lineage(
                &mut result,
                target,
                &column,
                &assignment.value,
                &relations,
                ctes,
                true,
            );
        }
    }
    result
}

/// CTAS / CREATE VIEW 的列级血缘：投影名/别名即目标列名
fn query_column_lineage(
    target: &LineageTableRef,
    query: &Query,
    default_db: Option<&str>,
    ctes: &HashSet<String>,
) -> Vec<ColumnLineage> {
    let SetExpr::Select(select) = query.body.as_ref() else {
        return Vec::new();
    };
    let relations = relation_keys(&select.from, default_db);
    let mut result = Vec::new();
    for (name, expr) in projection_items(select) {
        if let (Some(name), Some(expr)) = (name, expr) {
            append_column_lineage(&mut result, target, &name, &expr, &relations, ctes, false);
        }
    }
    result
}

/// 把单个「目标列 ← 表达式列引用」追加进结果；表达式无可归属列、
/// 仅指向 CTE 别名、或仅有目标列自赋值（`SET a = a`）时跳过。
fn append_column_lineage(
    result: &mut Vec<ColumnLineage>,
    target: &LineageTableRef,
    column: &str,
    expr: &Expr,
    relations: &[(String, LineageTableRef)],
    ctes: &HashSet<String>,
    filter_self: bool,
) {
    let mut sources: Vec<LineageColumnRef> = expr_columns(expr, relations)
        .into_iter()
        .filter(|source| {
            if ctes.contains(&relation_key(&source.table)) {
                return false;
            }
            !(filter_self && source.table == *target && source.column.eq_ignore_ascii_case(column))
        })
        .collect();
    sources.dedup();
    if sources.is_empty() {
        return;
    }
    result.push(ColumnLineage {
        target: LineageColumnRef {
            table: target.clone(),
            column: column.to_string(),
        },
        sources,
    });
}

fn push_column_unique(list: &mut Vec<LineageColumnRef>, reference: LineageColumnRef) {
    if !list.contains(&reference) {
        list.push(reference);
    }
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

    /// 把列级血缘压成可读字符串便于断言：`表.目标列 <- 表.源列, 表.源列`
    fn column_flows(statement: &super::LineageStatement) -> Vec<String> {
        statement
            .column_lineage
            .iter()
            .map(|flow| {
                let sources = flow
                    .sources
                    .iter()
                    .map(|source| format!("{}.{}", source.table.table, source.column))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}.{} <- {}", flow.target.table.table, flow.target.column, sources)
            })
            .collect()
    }

    #[test]
    fn insert_select_column_list_alignment() {
        let result = analyze_mysql("INSERT INTO logs (a, b) SELECT x, y FROM events");
        let statement = &result.statements[0];
        assert_eq!(column_flows(statement), vec!["logs.a <- events.x", "logs.b <- events.y"]);
    }

    #[test]
    fn insert_select_no_column_list_uses_alias_names() {
        let result = analyze_mysql(
            "INSERT INTO summary SELECT u.id AS user_id, o.amount AS amount FROM users u JOIN orders o ON u.id = o.user_id",
        );
        let statement = &result.statements[0];
        assert_eq!(
            column_flows(statement),
            vec!["summary.user_id <- users.id", "summary.amount <- orders.amount"]
        );
    }

    #[test]
    fn insert_select_no_column_list_unnamed_expr_skipped() {
        // 无别名且非裸列的投影（o.amount）无法确定目标列名，跳过
        let result = analyze_mysql(
            "INSERT INTO summary SELECT u.id AS user_id, o.amount FROM users u JOIN orders o ON u.id = o.user_id",
        );
        assert_eq!(
            column_flows(&result.statements[0]),
            vec!["summary.user_id <- users.id"]
        );
    }

    #[test]
    fn insert_select_wildcard_skips_column_lineage() {
        let result = analyze_mysql("INSERT INTO t SELECT * FROM s");
        assert!(result.statements[0].column_lineage.is_empty());
    }

    #[test]
    fn insert_select_qualified_wildcard_skips_alignment() {
        let result = analyze_mysql("INSERT INTO t (a, b) SELECT u.* FROM users u");
        assert!(result.statements[0].column_lineage.is_empty());
    }

    #[test]
    fn insert_select_column_count_mismatch_skips() {
        let result = analyze_mysql("INSERT INTO t (a, b) SELECT x FROM s");
        assert!(result.statements[0].column_lineage.is_empty());
    }

    #[test]
    fn insert_union_source_skips_column_lineage() {
        let result = analyze_mysql("INSERT INTO t (a) SELECT x FROM s UNION SELECT y FROM u");
        assert!(result.statements[0].column_lineage.is_empty());
    }

    #[test]
    fn insert_values_has_no_column_lineage() {
        let result = analyze_mysql("INSERT INTO t (a) VALUES (1)");
        assert!(result.statements[0].column_lineage.is_empty());
    }

    #[test]
    fn replace_select_wildcard_has_no_column_lineage() {
        let result = analyze_mysql("REPLACE INTO t SELECT * FROM s");
        assert!(result.statements[0].column_lineage.is_empty());
    }

    #[test]
    fn with_insert_column_lineage_filters_cte_columns() {
        let result = analyze_mysql(
            "WITH top AS (SELECT id FROM orders GROUP BY user_id) INSERT INTO ranks (rid) SELECT id FROM top",
        );
        // CTE 中间层的列归属无法展开到基表，跳过
        assert!(result.statements[0].column_lineage.is_empty());
    }

    #[test]
    fn cross_database_column_lineage() {
        let result = analyze_mysql("INSERT INTO db1.t (a) SELECT db2.s.x FROM db2.s");
        assert_eq!(column_flows(&result.statements[0]), vec!["t.a <- s.x"]);
    }

    #[test]
    fn update_assignment_column_lineage_join_alias() {
        let result = analyze_mysql(
            "UPDATE users u JOIN staging s ON u.id = s.uid SET u.name = s.full_name",
        );
        let statement = &result.statements[0];
        assert_eq!(statement.operation, "UPDATE");
        assert_eq!(column_flows(statement), vec!["users.name <- staging.full_name"]);
    }

    #[test]
    fn update_self_column_reference() {
        let result = analyze_mysql("UPDATE users SET display_name = name");
        assert_eq!(column_flows(&result.statements[0]), vec!["users.display_name <- users.name"]);
    }

    #[test]
    fn update_trivial_self_assignment_skipped() {
        let result = analyze_mysql("UPDATE users SET name = name");
        assert!(result.statements[0].column_lineage.is_empty());
    }

    #[test]
    fn update_subquery_value_columns_not_attributed() {
        let result = analyze_mysql(
            "UPDATE users SET name = (SELECT full_name FROM staging WHERE staging.uid = users.id)",
        );
        // 子查询内的列归属其自身作用域，不做解析
        assert!(result.statements[0].column_lineage.is_empty());
    }

    #[test]
    fn update_expression_columns_collected() {
        let result = analyze_mysql(
            "UPDATE accounts SET total = balance + pending WHERE id = 1",
        );
        assert_eq!(
            column_flows(&result.statements[0]),
            vec!["accounts.total <- accounts.balance, accounts.pending"]
        );
    }

    #[test]
    fn postgres_update_from_column_lineage() {
        let result = analyze_lineage(
            "UPDATE users SET name = s.full_name FROM staging_users s WHERE users.id = s.id",
            DriverKind::Postgres,
            Some("public"),
        );
        assert_eq!(
            column_flows(&result.statements[0]),
            vec!["users.name <- staging_users.full_name"]
        );
    }

    #[test]
    fn ctas_column_lineage() {
        let result = analyze_mysql("CREATE TABLE backup_users AS SELECT id AS user_id, name FROM users");
        let statement = &result.statements[0];
        assert_eq!(statement.operation, "CREATE_TABLE_AS");
        assert_eq!(
            column_flows(statement),
            vec!["backup_users.user_id <- users.id", "backup_users.name <- users.name"]
        );
    }

    #[test]
    fn create_view_column_lineage_expression() {
        let result = analyze_mysql("CREATE VIEW v_totals AS SELECT id, amount + fee AS total FROM orders");
        let statement = &result.statements[0];
        assert_eq!(statement.operation, "CREATE_VIEW");
        assert_eq!(
            column_flows(statement),
            vec![
                "v_totals.id <- orders.id",
                "v_totals.total <- orders.amount, orders.fee",
            ]
        );
    }

    #[test]
    fn delete_has_no_column_lineage() {
        let result = analyze_mysql("DELETE FROM logs WHERE created_at < '2020-01-01'");
        assert!(result.statements[0].column_lineage.is_empty());
    }
}
