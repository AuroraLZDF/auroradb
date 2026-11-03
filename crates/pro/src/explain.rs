//! SQL 执行计划解释（EXPLAIN FORMAT=JSON / EXPLAIN (FORMAT JSON)）
//!
//! 两种驱动的 EXPLAIN JSON 结构不同，后端在返回时做了两件事：
//! 1. 保留原始 JSON（前端可直接展示完整内容）；
//! 2. 递归扁平化为统一的 ExplainNode 树（operation / table / rows / cost / children），
//!    前端用 ExplainTree.vue 渲染这棵树，节点颜色按 cost 高亮。

use auroradb_core::connection::driver::{AnyPool, DriverKind};
use auroradb_core::connection::ConnectionManager;
use anyhow::{anyhow, Context, Result};
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
            if !s.is_empty() { extra.push(s.to_string()); }
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
    n.table = node.get("table_name").and_then(|v| v.as_str()).map(String::from);
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
    let obj = match root.as_object() { Some(o) => o, None => return vec![] };
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
    let node_type = p.get("Node Type").and_then(|v| v.as_str()).unwrap_or("Unknown");
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
    for key in ["Filter", "Join Filter", "Hash Cond", "Recheck Cond", "One-Time Filter"] {
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
    let arr = match root.as_array() { Some(a) => a, None => return vec![] };
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
    // PG 不支持 REPLACE
    let allowed: &[&str] = match driver {
        DriverKind::MySql => &["SELECT", "INSERT", "UPDATE", "DELETE", "REPLACE", "WITH"],
        DriverKind::Postgres => &["SELECT", "INSERT", "UPDATE", "DELETE", "WITH"],
    };
    if !allowed.iter().any(|p| upper.starts_with(p)) {
        return Err(anyhow!(
            "无法解释该语句：只支持 SELECT / INSERT / UPDATE / DELETE / WITH{}",
            match driver {
                DriverKind::MySql => " / REPLACE",
                DriverKind::Postgres => "",
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
                .map_err(|e| {
                    anyhow!(
                        "执行 EXPLAIN 失败：{}",
                        auroradb_core::error::translate(&e)
                    )
                })?
                .ok_or_else(|| anyhow!("EXPLAIN 未返回结果"))?
        }
        AnyPool::Postgres(pg_pool) => {
            let mut phys = pg_pool.acquire().await.context("获取 PostgreSQL 连接失败")?;
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
                .map_err(|e| {
                    anyhow!(
                        "执行 EXPLAIN 失败：{}",
                        auroradb_core::error::translate(&e)
                    )
                })?
                .ok_or_else(|| anyhow!("EXPLAIN 未返回结果"))?
        }
    };
    let warnings: Vec<String> = vec![];

    let parsed: Value = serde_json::from_str(&raw_json)
        .map_err(|e| anyhow!("解析 EXPLAIN JSON 失败：{e}"))?;

    let nodes = match driver {
        DriverKind::MySql => flatten_mysql_top_level(&parsed),
        DriverKind::Postgres => flatten_pg_top_level(&parsed),
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
