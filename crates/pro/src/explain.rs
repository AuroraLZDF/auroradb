//! SQL 执行计划解释（EXPLAIN FORMAT=JSON / EXPLAIN (FORMAT JSON)）
//!
//! 两种驱动的 EXPLAIN JSON 结构不同，后端在返回时做了两件事：
//! 1. 保留原始 JSON（前端可直接展示完整内容）；
//! 2. 递归扁平化为统一的 ExplainNode 树（operation / table / rows / cost / children），
//!    前端用 ExplainTree.vue 渲染这棵树，节点颜色按 cost 高亮。

use anyhow::{anyhow, Context, Result};
use auroradb_core::connection::driver::{AnyPool, DriverKind};
use auroradb_core::connection::ConnectionManager;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Executor;

/// 统一的执行计划节点
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainNode {
    /// 人类可读的操作描述（MySQL 取 operation + message，PG 取 "Node Type" + "Relation Name"）
    pub operation: String,
    /// 表名（可能是 derived table 别名或临时表）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    /// 使用的索引（MySQL 的 `key`，PG 的 "Index Name"）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<String>,
    /// 估算行数
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<u64>,
    /// 估算 cost
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    /// 过滤比例 0.0–100.0（MySQL filtered%）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filtered: Option<f64>,
    /// 额外说明（MySQL `Extra` 拆成数组；PG 的 Filter / Join Filter / Hash Cond 等拼接）
    pub extra: Vec<String>,
    /// 子节点
    pub children: Vec<ExplainNode>,
}

impl ExplainNode {
    pub fn new(operation: impl Into<String>) -> Self {
        Self {
            operation: operation.into(),
            table: None,
            index: None,
            rows: None,
            cost: None,
            filtered: None,
            extra: vec![],
            children: vec![],
        }
    }
}

/// 统一的执行计划返回
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplainPlan {
    pub driver: DriverKind,
    /// 原始 SQL（即用户提交的 SQL，不是 EXPLAIN 包裹后的）
    pub sql: String,
    /// 树的根节点（MySQL 可能有多个 union_result，PG 通常一个）
    pub nodes: Vec<ExplainNode>,
    /// EXPLAIN 过程中产生的警告/备注
    pub warnings: Vec<String>,
    /// 后端原样返回的 JSON 字符串，供前端"查看原始 JSON"展示
    pub raw_json: String,
    /// EXPLAIN 执行耗时（毫秒，不含真正执行 ANALYZE）
    pub execution_time_ms: u64,
}

// ===================== MySQL EXPLAIN FORMAT=JSON 解析 =====================

fn mysql_operation(obj: &serde_json::Map<String, Value>) -> String {
    let mut parts: Vec<String> = vec![];
    if let Some(access_type) = obj.get("access_type").and_then(|v| v.as_str()) {
        parts.push(access_type.to_string());
    }
    if let Some(table_name) = obj.get("table_name").and_then(|v| v.as_str()) {
        parts.push(table_name.to_string());
    }
    if let Some(message) = obj.get("message").and_then(|v| v.as_str()) {
        parts.push(message.to_string());
    }
    if parts.is_empty() {
        if let Some(select_id) = obj.get("select_id").and_then(|v| v.as_u64()) {
            return format!("SELECT #{select_id}");
        }
        return "query block".to_string();
    }
    parts.join(" · ")
}

fn mysql_extra(obj: &serde_json::Map<String, Value>) -> Vec<String> {
    let mut extra = vec![];
    if let Some(msg) = obj.get("message").and_then(|v| v.as_str()) {
        for part in msg.split(';') {
            let s = part.trim();
            if !s.is_empty() {
                extra.push(s.to_string());
            }
        }
    }
    if let Some(filtered) = obj.get("filtered").and_then(|v| v.as_f64()) {
        if filtered < 100.0 {
            extra.push(format!("filtered {filtered:.0}%"));
        }
    }
    extra
}

fn mysql_to_node(obj: &Value) -> Option<ExplainNode> {
    let node = obj.as_object()?;
    let mut n = ExplainNode::new(mysql_operation(node));
    n.table = node
        .get("table_name")
        .and_then(|v| v.as_str())
        .map(String::from);
    n.index = node.get("key").and_then(|v| v.as_str()).map(String::from);
    n.rows = node.get("rows").and_then(|v| v.as_u64());
    n.filtered = node.get("filtered").and_then(|v| v.as_f64());
    n.extra = mysql_extra(node);
    if let Some(cost_obj) = node.get("cost_info").and_then(|v| v.as_object()) {
        n.cost = cost_obj.get("total_cost").and_then(|v| v.as_f64());
    }
    // children：MySQL 的 explain json 子节点嵌在 nested_loop / derived_table /
    // ordering_operation / grouping_operation 里。遍历所有 value 抓嵌套对象。
    let mut children = vec![];
    for (_k, v) in node {
        if matches!(v, Value::Object(_)) || matches!(v, Value::Array(_)) {
            if let Some(child) = mysql_to_node(v) {
                if child.operation != n.operation || child.table != n.table {
                    children.push(child);
                }
            }
        }
    }
    n.children = children;
    Some(n)
}

fn flatten_mysql_top_level(root: &Value) -> Vec<ExplainNode> {
    let obj = match root.as_object() {
        Some(o) => o,
        None => return vec![],
    };
    if let Some(qb) = obj.get("query_block") {
        return vec![mysql_to_node(qb).unwrap_or_else(|| ExplainNode::new("query_block"))];
    }
    if let Some(ur) = obj.get("union_result") {
        return match ur.as_array() {
            Some(arr) => arr.iter().filter_map(mysql_to_node).collect(),
            None => mysql_to_node(ur).into_iter().collect(),
        };
    }
    mysql_to_node(root).into_iter().collect()
}

// ===================== PostgreSQL EXPLAIN (FORMAT JSON) 解析 =====================

fn pg_to_node(plan: &Value) -> Option<ExplainNode> {
    let p = plan.as_object()?;
    let node_type = p
        .get("Node Type")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown");
    let relation = p.get("Relation Name").and_then(|v| v.as_str());
    let alias = p.get("Alias").and_then(|v| v.as_str());
    let operation = match (relation, alias) {
        (Some(r), Some(a)) if r != a => format!("{} on {} (alias {})", node_type, r, a),
        (Some(r), _) => format!("{} on {}", node_type, r),
        (_, _) => node_type.to_string(),
    };

    let mut n = ExplainNode::new(operation);
    n.table = relation.map(String::from);
    n.rows = p.get("Rows").and_then(|v| v.as_u64());
    n.cost = p.get("Total Cost").and_then(|v| v.as_f64());
    if let Some(idx) = p.get("Index Name").and_then(|v| v.as_str()) {
        n.index = Some(idx.to_string());
    }
    for key in [
        "Filter",
        "Join Filter",
        "Hash Cond",
        "Recheck Cond",
        "One-Time Filter",
    ] {
        if let Some(val) = p.get(key).and_then(|v| v.as_str()) {
            n.extra.push(format!("{key}: {val}"));
        }
    }
    if let Some(sort_key) = p.get("Sort Key").and_then(|v| v.as_array()) {
        let keys: Vec<&str> = sort_key.iter().filter_map(|v| v.as_str()).collect();
        if !keys.is_empty() {
            n.extra.push(format!("Sort Key: {}", keys.join(", ")));
        }
    }
    if let Some(arr) = p.get("Plans").and_then(|v| v.as_array()) {
        n.children = arr.iter().filter_map(pg_to_node).collect();
    }
    Some(n)
}

fn flatten_pg_top_level(root: &Value) -> Vec<ExplainNode> {
    let arr = match root.as_array() {
        Some(a) => a,
        None => return vec![],
    };
    let mut result = vec![];
    for item in arr {
        if let Some(plan) = item.get("Plan") {
            if let Some(n) = pg_to_node(plan) {
                result.push(n);
            }
        }
    }
    result
}

// ===================== 执行层 =====================

/// SQLite EXPLAIN QUERY PLAN 行 → 计划树。
///
/// 输出 4 列 (id, parent, notused, detail)：id 为行号（从 1 起）、parent 为
/// 父行号（0 表示根）。detail 形如 "SCAN t" / "SEARCH t USING INDEX idx (x=?)" /
/// "SEARCH t USING INTEGER PRIMARY KEY (rowid=?)"、"SCALAR SUBQUERY n"，尽力提取表名/索引（SQLite 不给
/// 行数与 cost，留 None）。
fn build_sqlite_plan_tree(rows: &[(i64, i64, String)]) -> Vec<ExplainNode> {
    use std::collections::HashMap;

    // 先建所有节点 + 父子关系表，再从根逐棵摘取（避免父节点先被消费）
    let mut nodes: HashMap<i64, ExplainNode> = HashMap::new();
    let mut children_of: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut root_ids: Vec<i64> = Vec::new();
    for (id, parent, detail) in rows {
        nodes.insert(*id, sqlite_detail_to_node(detail));
        if *parent > 0 && nodes.contains_key(parent) {
            children_of.entry(*parent).or_default().push(*id);
        } else {
            // parent=0 或父行缺失（不应发生）：降级为根
            root_ids.push(*id);
        }
    }

    fn take_subtree(
        nodes: &mut HashMap<i64, ExplainNode>,
        children_of: &HashMap<i64, Vec<i64>>,
        id: i64,
    ) -> Option<ExplainNode> {
        let mut node = nodes.remove(&id)?;
        if let Some(children) = children_of.get(&id) {
            for child_id in children {
                if let Some(child) = take_subtree(nodes, children_of, *child_id) {
                    node.children.push(child);
                }
            }
        }
        Some(node)
    }

    root_ids
        .iter()
        .filter_map(|id| take_subtree(&mut nodes, &children_of, *id))
        .collect()
}

/// 解析 SQLite 计划行文本："SCAN <table>"、"SEARCH <table> USING INDEX <idx> (…)"、
/// "SCAN <table> USING COVERING INDEX <idx>"、"SCALAR SUBQUERY n"、
/// "USE TEMP B-TREE FOR ORDER BY" 等。
fn sqlite_detail_to_node(detail: &str) -> ExplainNode {
    let detail = detail.trim();
    let mut node = ExplainNode::new(detail);
    let upper = detail.to_uppercase();

    // 索引：USING 之后的描述。INDEX/COVERING INDEX 取具名索引；
    // INTEGER PRIMARY KEY 表示 rowid 主键查找，保留描述文本。
    if let Some(using_pos) = upper.find("USING ") {
        let rest = detail[using_pos + "USING ".len()..].trim();
        let rest = rest.split('(').next().unwrap_or(rest).trim();
        let name = rest
            .strip_prefix("COVERING INDEX ")
            .or_else(|| rest.strip_prefix("INDEX "))
            .unwrap_or(rest)
            .trim();
        if !name.is_empty() {
            node.index = Some(name.to_string());
        }
    }

    // 表名：SEARCH/SCAN 关键字后的第一个词（SCALAR SUBQUERY / TEMP B-TREE 无表名）
    let first_word_end = detail.find(' ').unwrap_or(detail.len());
    let keyword = &detail[..first_word_end];
    if keyword.eq_ignore_ascii_case("SEARCH") || keyword.eq_ignore_ascii_case("SCAN") {
        let after = detail[first_word_end..].trim_start();
        if let Some(table_word) = after.split_whitespace().next() {
            let table_word = table_word.trim();
            if !table_word.is_empty() {
                node.table = Some(table_word.to_string());
            }
        }
    }
    node
}

fn validate_explain_sql(sql: &str, driver: DriverKind) -> Result<()> {
    // 只允许单条语句：EXPLAIN 只作用于紧随其后的第一条语句，多语句会静默忽略其余部分
    let statements = auroradb_core::sql::split_sql_statements(sql);
    if statements.len() > 1 {
        return Err(anyhow!("一次只能解释一条语句，请去掉多余的分号内容"));
    }
    let trimmed = statements.first().map(|s| s.trim()).unwrap_or("");
    if trimmed.is_empty() {
        return Err(anyhow!("SQL 为空，无法执行解释计划"));
    }
    let upper = trimmed.to_uppercase();
    // PG 不支持 REPLACE；SQLite 同样不支持 REPLACE…SELECT 之外的语句差别可接受，
    // 与 PG 保持一致（SQLite 的 EXPLAIN QUERY PLAN 只支持 SELECT 类语句）
    let allowed: &[&str] = match driver {
        DriverKind::MySql => &["SELECT", "INSERT", "UPDATE", "DELETE", "REPLACE", "WITH"],
        DriverKind::Postgres | DriverKind::Sqlite => {
            &["SELECT", "INSERT", "UPDATE", "DELETE", "WITH"]
        }
    };
    if !allowed.iter().any(|p| upper.starts_with(p)) {
        return Err(anyhow!(
            "无法解释该语句：只支持 SELECT / INSERT / UPDATE / DELETE / WITH{}",
            match driver {
                DriverKind::MySql => " / REPLACE",
                DriverKind::Postgres | DriverKind::Sqlite => "",
            }
        ));
    }
    if upper.starts_with("EXPLAIN") {
        return Err(anyhow!("请直接输入业务 SQL，EXPLAIN 关键字由程序自动添加"));
    }
    Ok(())
}

/// 解释 SQL 执行计划（统一 MySQL/PG）
pub async fn explain(
    manager: &ConnectionManager,
    connection_id: &str,
    database: &str,
    sql: &str,
) -> Result<ExplainPlan> {
    // 库名会被拼进 USE / search_path，必须先按标识符规则校验
    if !database.is_empty() {
        auroradb_core::connection::validation::validate_identifier(database)
            .map_err(|e| anyhow!("数据库名验证失败：{}", e))?;
    }

    let snapshot = manager.connection_snapshot(connection_id).await?;
    let pool = snapshot.pool;
    validate_explain_sql(sql, pool.driver())?;
    let driver = pool.driver();
    let conn_database = snapshot.config.database.clone();

    let start = std::time::Instant::now();
    let raw_json = match &pool {
        AnyPool::MySql(mysql_pool) => {
            let mut phys = mysql_pool.acquire().await.context("获取 MySQL 连接失败")?;
            if !database.is_empty() {
                let use_sql = DriverKind::MySql.switch_database_sql(database);
                Executor::execute(&mut *phys, use_sql.as_str())
                    .await
                    .map_err(|e| anyhow!("切换数据库失败：{}", e))?;
            }
            let explain_sql = format!("EXPLAIN FORMAT=JSON {}", sql);
            sqlx::query_scalar::<_, String>(explain_sql.as_str())
                .fetch_optional(&mut *phys)
                .await
                .map_err(|e| anyhow!("执行 EXPLAIN 失败：{}", auroradb_core::error::translate(&e)))?
                .ok_or_else(|| anyhow!("EXPLAIN 未返回结果"))?
        }
        AnyPool::Postgres(pg_pool) => {
            let mut phys = pg_pool
                .acquire()
                .await
                .context("获取 PostgreSQL 连接失败")?;
            if !database.is_empty() && conn_database.as_deref() != Some(database) {
                let set_sql = DriverKind::Postgres.switch_database_sql(database);
                Executor::execute(&mut *phys, set_sql.as_str())
                    .await
                    .map_err(|e| {
                        anyhow!("切换模式失败：{}", auroradb_core::error::translate(&e))
                    })?;
            }
            let explain_sql = format!("EXPLAIN (FORMAT JSON) {}", sql);
            sqlx::query_scalar::<_, String>(explain_sql.as_str())
                .fetch_optional(&mut *phys)
                .await
                .map_err(|e| anyhow!("执行 EXPLAIN 失败：{}", auroradb_core::error::translate(&e)))?
                .ok_or_else(|| anyhow!("EXPLAIN 未返回结果"))?
        }
        // SQLite：EXPLAIN QUERY PLAN 输出 4 列 (id, parent, notused, detail)，
        // 用 id/parent 在 Rust 侧重建树，detail 即人类可读计划行
        AnyPool::Sqlite(sqlite_pool) => {
            if !database.is_empty() {
                // 单文件库 database 参数就是文件路径：只允许当前连接库
                let conn_path = conn_database.as_deref().unwrap_or("");
                if database != conn_path {
                    return Err(anyhow!(
                        "SQLite 连接只能解释当前库的语句：请传空字符串或连接本身的数据文件路径"
                    ));
                }
            }
            let mut phys = sqlite_pool.acquire().await.context("获取 SQLite 连接失败")?;
            let explain_sql = format!("EXPLAIN QUERY PLAN {}", sql);
            let rows: Vec<(i64, i64, String)> = sqlx::query_as(&explain_sql)
                .fetch_all(&mut *phys)
                .await
                .map_err(|e| anyhow!("执行 EXPLAIN 失败：{}", auroradb_core::error::translate(&e)))?;
            // 非树形结构序列化：原始 JSON 保留行数组，节点树由 parse 函数构建
            let rows_json = serde_json::to_string(
                &rows.iter().map(|(id, parent, detail)| serde_json::json!({
                    "id": id, "parent": parent, "detail": detail,
                })).collect::<Vec<_>>(),
            )
            .map_err(|e| anyhow!("序列化 EXPLAIN 行失败：{e}"))?;
            return Ok(ExplainPlan {
                driver,
                sql: sql.to_string(),
                nodes: build_sqlite_plan_tree(&rows),
                warnings: vec![],
                raw_json: rows_json,
                execution_time_ms: start.elapsed().as_millis() as u64,
            });
        }
    };
    let warnings: Vec<String> = vec![];

    let parsed: Value =
        serde_json::from_str(&raw_json).map_err(|e| anyhow!("解析 EXPLAIN JSON 失败：{e}"))?;

    let nodes = match driver {
        DriverKind::MySql => flatten_mysql_top_level(&parsed),
        // SQLite 在上面已提前返回（EXPLAIN QUERY PLAN 非树形 JSON 输出）
        DriverKind::Postgres => flatten_pg_top_level(&parsed),
        DriverKind::Sqlite => flatten_pg_top_level(&parsed),
    };

    Ok(ExplainPlan {
        driver,
        sql: sql.to_string(),
        nodes,
        warnings,
        raw_json,
        execution_time_ms: start.elapsed().as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_detail_parses_scan_and_search() {
        let node = sqlite_detail_to_node("SCAN t2");
        assert_eq!(node.operation, "SCAN t2");
        assert_eq!(node.table.as_deref(), Some("t2"));
        assert!(node.index.is_none());

        let node = sqlite_detail_to_node("SEARCH t1 USING INTEGER PRIMARY KEY (rowid=?)");
        assert_eq!(node.table.as_deref(), Some("t1"));
        assert_eq!(node.index.as_deref(), Some("INTEGER PRIMARY KEY"));

        let node = sqlite_detail_to_node("SEARCH t USING INDEX idx_email (email=?)");
        assert_eq!(node.table.as_deref(), Some("t"));
        assert_eq!(node.index.as_deref(), Some("idx_email"));

        let node = sqlite_detail_to_node("SCAN t USING COVERING INDEX idx_email");
        assert_eq!(node.table.as_deref(), Some("t"));
        assert_eq!(node.index.as_deref(), Some("idx_email"));
    }

    #[test]
    fn sqlite_detail_parses_non_table_rows() {
        let node = sqlite_detail_to_node("SCALAR SUBQUERY 1");
        assert!(node.table.is_none());

        let node = sqlite_detail_to_node("USE TEMP B-TREE FOR ORDER BY");
        assert!(node.table.is_none());
        assert!(node.index.is_none());
    }

    #[test]
    fn sqlite_plan_tree_builds_parent_child_relation() {
        // 对应 JOIN 查询：SCAN t2（根）→ SEARCH t1（子）
        let rows = vec![
            (1i64, 0i64, "SCAN t2".to_string()),
            (2, 1, "SEARCH t1 USING INTEGER PRIMARY KEY (rowid=?)".to_string()),
        ];
        let roots = build_sqlite_plan_tree(&rows);
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].operation, "SCAN t2");
        assert_eq!(roots[0].children.len(), 1);
        assert_eq!(roots[0].children[0].table.as_deref(), Some("t1"));
    }
}
