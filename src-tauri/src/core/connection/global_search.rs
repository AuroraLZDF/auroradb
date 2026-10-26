//! 跨库全局搜索（Pro）
//!
//! 表名 / 字段名走 `INFORMATION_SCHEMA` 精确匹配；数据内容按表逐条 COUNT，
//! 并受「最多扫描表数 + 单表超时 + 总时间预算」三重约束，避免打爆线上库。
//!
//! 仅支持 MySQL：PostgreSQL 连接池经 `as_mysql` 会返回带功能名的明确错误。

use super::executor::is_searchable_type;
use super::manager::ConnectionManager;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// 表名 / 字段名命中的返回上限（超出则标记截断）
const MAX_NAME_MATCHES: usize = 200;

/// 数据搜索最多扫描的表数量
const MAX_DATA_TABLES: usize = 100;

/// 数据搜索总时间预算：超出后停止扫描并明确告知用户
const DATA_SEARCH_BUDGET: Duration = Duration::from_secs(20);

/// 单表数据查询超时：慢表跳过并记入警告，不静默吞掉
const PER_TABLE_TIMEOUT: Duration = Duration::from_secs(5);

/// 单表命中行数上限：达到此值后只显示「N+」，避免大表全量 COUNT
const MATCHED_ROWS_CAP: i64 = 1000;

/// 系统库不参与搜索
const SYSTEM_SCHEMAS: &str = "'information_schema','mysql','performance_schema','sys'";

/// 搜索模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchMode {
    /// 包含匹配（LIKE），用户输入的通配符按字面量处理
    Like,
    /// 正则匹配（REGEXP）
    Regex,
}

/// 全局搜索请求
#[derive(Debug, Clone, Deserialize)]
pub struct GlobalSearchRequest {
    pub connection_id: String,
    /// 限定数据库；`None` 表示该连接下全部业务库
    pub database: Option<String>,
    pub keyword: String,
    /// `like`（默认）或 `regex`
    pub mode: Option<String>,
    /// 是否搜索表名，默认 true
    pub search_tables: Option<bool>,
    /// 是否搜索字段名，默认 true
    pub search_columns: Option<bool>,
    /// 是否搜索数据内容，默认 false（代价高，需用户显式勾选）
    pub search_data: Option<bool>,
}

/// 表名命中
#[derive(Debug, Clone, Serialize)]
pub struct GlobalSearchTableMatch {
    pub database: String,
    pub table: String,
    /// `table` / `view`
    pub object_type: String,
}

/// 字段名命中
#[derive(Debug, Clone, Serialize)]
pub struct GlobalSearchColumnMatch {
    pub database: String,
    pub table: String,
    pub column: String,
    pub data_type: String,
}

/// 数据内容命中
#[derive(Debug, Clone, Serialize)]
pub struct GlobalSearchDataMatch {
    pub database: String,
    pub table: String,
    /// 命中行数（上限 `MATCHED_ROWS_CAP`）
    pub matched_rows: u64,
    /// 行数是否因上限被截断
    pub count_capped: bool,
}

/// 全局搜索结果
#[derive(Debug, Clone, Serialize)]
pub struct GlobalSearchResult {
    pub tables: Vec<GlobalSearchTableMatch>,
    pub columns: Vec<GlobalSearchColumnMatch>,
    pub data: Vec<GlobalSearchDataMatch>,
    /// 数据搜索实际扫描的表数量
    pub scanned_tables: usize,
    /// 是否有结果/扫描范围被截断
    pub truncated: bool,
    /// 需要提示用户的说明（超时、失败、扫描范围受限等）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    pub execution_time_ms: u64,
}

/// LIKE 模式转义：只处理通配符。
///
/// 关键字本身走绑定参数，无需转义引号；MySQL 默认以反斜杠作为 LIKE 转义符。
fn like_pattern(keyword: &str) -> String {
    let escaped = keyword
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{}%", escaped)
}

/// 标识符反引号包装。
///
/// 名称全部来自服务端的 `INFORMATION_SCHEMA`，不是用户输入；这里只做反引号转义，
/// 不套用 `validate_identifier`（否则 `order` 这类合法表名会被误拒）。
fn quote_name(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

/// 按模式生成列匹配片段，关键字统一走绑定参数
fn match_expr(column: &str, mode: SearchMode) -> String {
    let quoted = quote_name(column);
    match mode {
        SearchMode::Like => format!("{} LIKE ?", quoted),
        SearchMode::Regex => format!("{} REGEXP ?", quoted),
    }
}

/// 关键字按模式转换为绑定值
fn bind_value(mode: SearchMode, keyword: &str) -> String {
    match mode {
        SearchMode::Like => like_pattern(keyword),
        SearchMode::Regex => keyword.to_string(),
    }
}

impl ConnectionManager {
    /// 跨库全局搜索：表名 / 字段名 / 数据内容
    pub async fn global_search(&self, request: GlobalSearchRequest) -> Result<GlobalSearchResult> {
        let started = Instant::now();
        let keyword = request.keyword.trim();
        if keyword.is_empty() {
            return Err(anyhow!("请输入搜索关键字"));
        }
        if keyword.chars().count() > 200 {
            return Err(anyhow!("搜索关键字不能超过 200 个字符"));
        }

        let mode = match request.mode.as_deref().unwrap_or("like") {
            "like" => SearchMode::Like,
            "regex" => SearchMode::Regex,
            other => return Err(anyhow!("不支持的搜索模式：{}", other)),
        };

        let database = request
            .database
            .as_deref()
            .map(str::trim)
            .filter(|db| !db.is_empty());

        let pool = {
            let connections = self.connections.read().await;
            let conn = connections.get(&request.connection_id).ok_or_else(|| {
                anyhow!("连接 {} 不存在或已断开", request.connection_id)
            })?;
            conn.pool
                .as_ref()
                .ok_or_else(|| anyhow!("连接 {} 未建立", request.connection_id))?
                .as_mysql("跨库全局搜索")?
                .clone()
        };

        let mut notes: Vec<String> = Vec::new();
        let mut truncated = false;

        let (tables, table_truncated) = if request.search_tables.unwrap_or(true) {
            search_table_names(&pool, database, mode, keyword).await?
        } else {
            (Vec::new(), false)
        };
        truncated |= table_truncated;

        let (columns, column_truncated) = if request.search_columns.unwrap_or(true) {
            search_column_names(&pool, database, mode, keyword).await?
        } else {
            (Vec::new(), false)
        };
        truncated |= column_truncated;

        let mut scanned_tables = 0usize;
        let data = if request.search_data.unwrap_or(false) {
            let (matches, scanned, data_truncated, data_notes) =
                search_data_content(&pool, database, mode, keyword, started).await?;
            scanned_tables = scanned;
            truncated |= data_truncated;
            notes.extend(data_notes);
            matches
        } else {
            Vec::new()
        };

        if truncated && notes.is_empty() {
            notes.push(format!(
                "结果已截断：表名/字段名最多返回 {} 条，数据搜索最多扫描 {} 张表",
                MAX_NAME_MATCHES, MAX_DATA_TABLES
            ));
        }

        Ok(GlobalSearchResult {
            tables,
            columns,
            data,
            scanned_tables,
            truncated,
            warning: if notes.is_empty() {
                None
            } else {
                Some(notes.join("；"))
            },
            execution_time_ms: started.elapsed().as_millis() as u64,
        })
    }
}

/// 数据库过滤片段与绑定值
fn database_filter(database: Option<&str>) -> (&'static str, Option<String>) {
    match database {
        Some(db) => (" AND TABLE_SCHEMA = ?", Some(db.to_string())),
        None => ("", None),
    }
}

/// 搜索表名 / 视图名
async fn search_table_names(
    pool: &MySqlPool,
    database: Option<&str>,
    mode: SearchMode,
    keyword: &str,
) -> Result<(Vec<GlobalSearchTableMatch>, bool)> {
    let (db_clause, db_bind) = database_filter(database);
    let name_op = match mode {
        SearchMode::Like => "TABLE_NAME LIKE ?",
        SearchMode::Regex => "TABLE_NAME REGEXP ?",
    };
    let sql = format!(
        "SELECT TABLE_SCHEMA, TABLE_NAME, TABLE_TYPE FROM INFORMATION_SCHEMA.TABLES \
         WHERE TABLE_SCHEMA NOT IN ({}){} AND {} \
         ORDER BY TABLE_SCHEMA, TABLE_NAME LIMIT {}",
        SYSTEM_SCHEMAS,
        db_clause,
        name_op,
        MAX_NAME_MATCHES + 1
    );

    let mut query = sqlx::query_as::<_, (String, String, String)>(&sql);
    if let Some(db) = db_bind {
        query = query.bind(db);
    }
    let rows = query.bind(bind_value(mode, keyword)).fetch_all(pool).await?;

    let truncated = rows.len() > MAX_NAME_MATCHES;
    let matches = rows
        .into_iter()
        .take(MAX_NAME_MATCHES)
        .map(|(database, table, table_type)| GlobalSearchTableMatch {
            database,
            table,
            object_type: if table_type.eq_ignore_ascii_case("VIEW") {
                "view".to_string()
            } else {
                "table".to_string()
            },
        })
        .collect();
    Ok((matches, truncated))
}

/// 搜索字段名
async fn search_column_names(
    pool: &MySqlPool,
    database: Option<&str>,
    mode: SearchMode,
    keyword: &str,
) -> Result<(Vec<GlobalSearchColumnMatch>, bool)> {
    let (db_clause, db_bind) = database_filter(database);
    let name_op = match mode {
        SearchMode::Like => "COLUMN_NAME LIKE ?",
        SearchMode::Regex => "COLUMN_NAME REGEXP ?",
    };
    let sql = format!(
        "SELECT TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME, DATA_TYPE \
         FROM INFORMATION_SCHEMA.COLUMNS \
         WHERE TABLE_SCHEMA NOT IN ({}){} AND {} \
         ORDER BY TABLE_SCHEMA, TABLE_NAME, ORDINAL_POSITION LIMIT {}",
        SYSTEM_SCHEMAS,
        db_clause,
        name_op,
        MAX_NAME_MATCHES + 1
    );

    let mut query = sqlx::query_as::<_, (String, String, String, String)>(&sql);
    if let Some(db) = db_bind {
        query = query.bind(db);
    }
    let rows = query.bind(bind_value(mode, keyword)).fetch_all(pool).await?;

    let truncated = rows.len() > MAX_NAME_MATCHES;
    let matches = rows
        .into_iter()
        .take(MAX_NAME_MATCHES)
        .map(|(database, table, column, data_type)| GlobalSearchColumnMatch {
            database,
            table,
            column,
            data_type,
        })
        .collect();
    Ok((matches, truncated))
}

/// 逐表搜索数据内容，返回（命中表列表, 扫描表数, 是否截断, 警告）
async fn search_data_content(
    pool: &MySqlPool,
    database: Option<&str>,
    mode: SearchMode,
    keyword: &str,
    started: Instant,
) -> Result<(Vec<GlobalSearchDataMatch>, usize, bool, Vec<String>)> {
    let (db_clause, db_bind) = database_filter(database);

    // 只扫描基础表；视图可能聚合多表，代价不可控
    let table_sql = format!(
        "SELECT TABLE_SCHEMA, TABLE_NAME FROM INFORMATION_SCHEMA.TABLES \
         WHERE TABLE_TYPE = 'BASE TABLE' AND TABLE_SCHEMA NOT IN ({}){} \
         ORDER BY TABLE_SCHEMA, TABLE_NAME LIMIT {}",
        SYSTEM_SCHEMAS,
        db_clause,
        MAX_DATA_TABLES + 1
    );
    let mut table_query = sqlx::query_as::<_, (String, String)>(&table_sql);
    if let Some(db) = db_bind.as_deref() {
        table_query = table_query.bind(db);
    }
    let mut table_rows = table_query.fetch_all(pool).await?;

    let mut truncated = table_rows.len() > MAX_DATA_TABLES;
    table_rows.truncate(MAX_DATA_TABLES);

    // 一次性取回所有可搜索列，按 (库, 表) 分组
    let column_sql = format!(
        "SELECT TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME, DATA_TYPE \
         FROM INFORMATION_SCHEMA.COLUMNS \
         WHERE TABLE_SCHEMA NOT IN ({}){} \
         ORDER BY TABLE_SCHEMA, TABLE_NAME, ORDINAL_POSITION",
        SYSTEM_SCHEMAS, db_clause
    );
    let mut column_query = sqlx::query_as::<_, (String, String, String, String)>(&column_sql);
    if let Some(db) = db_bind.as_deref() {
        column_query = column_query.bind(db);
    }

    let mut columns: HashMap<(String, String), Vec<String>> = HashMap::new();
    for (schema, table, column, data_type) in column_query.fetch_all(pool).await? {
        if is_searchable_type(&data_type) {
            columns
                .entry((schema, table))
                .or_default()
                .push(column);
        }
    }

    let mut notes: Vec<String> = Vec::new();
    let mut matches: Vec<GlobalSearchDataMatch> = Vec::new();
    let mut scanned = 0usize;

    for (schema, table) in table_rows {
        if started.elapsed() >= DATA_SEARCH_BUDGET {
            truncated = true;
            notes.push(format!(
                "数据搜索已达到 {} 秒时间预算，仅扫描了 {} 张表",
                DATA_SEARCH_BUDGET.as_secs(),
                scanned
            ));
            break;
        }

        let Some(table_columns) = columns.get(&(schema.clone(), table.clone())) else {
            continue;
        };
        if table_columns.is_empty() {
            continue;
        }

        scanned += 1;
        let label = format!("{}.{}", schema, table);
        match count_matching_rows(pool, &schema, &table, table_columns, mode, keyword).await {
            Ok(Some(count)) => matches.push(GlobalSearchDataMatch {
                database: schema,
                table,
                matched_rows: count.min(MATCHED_ROWS_CAP) as u64,
                count_capped: count >= MATCHED_ROWS_CAP,
            }),
            Ok(None) => {}
            Err(reason) => notes.push(format!("{}：{}", label, reason)),
        }
    }

    Ok((matches, scanned, truncated, notes))
}

/// 统计单表命中行数（最多 `MATCHED_ROWS_CAP + 1` 行）。
///
/// 用派生表 + LIMIT 代替全表 COUNT：命中很多时提前停止，代价可控。
/// 返回 `Ok(None)` 表示没有命中；`Err` 为可展示的失败/超时原因。
async fn count_matching_rows(
    pool: &MySqlPool,
    schema: &str,
    table: &str,
    columns: &[String],
    mode: SearchMode,
    keyword: &str,
) -> std::result::Result<Option<i64>, String> {
    let conditions: Vec<String> = columns
        .iter()
        .map(|column| match_expr(column, mode))
        .collect();
    let sql = format!(
        "SELECT COUNT(*) FROM (SELECT 1 FROM {}.{} WHERE {} LIMIT {}) AS `_aurora_search`",
        quote_name(schema),
        quote_name(table),
        conditions.join(" OR "),
        MATCHED_ROWS_CAP + 1
    );

    let mut query = sqlx::query_scalar::<_, i64>(&sql);
    let bind = bind_value(mode, keyword);
    for _ in 0..columns.len() {
        query = query.bind(bind.clone());
    }

    match tokio::time::timeout(PER_TABLE_TIMEOUT, query.fetch_one(pool)).await {
        Ok(Ok(count)) if count > 0 => Ok(Some(count)),
        Ok(Ok(_)) => Ok(None),
        Ok(Err(e)) => Err(format!("查询失败：{}", e)),
        Err(_) => Err(format!("查询超时（{} 秒）", PER_TABLE_TIMEOUT.as_secs())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_pattern_escapes_wildcards() {
        assert_eq!(like_pattern("abc"), "%abc%");
        assert_eq!(like_pattern("50%"), "%50\\%%");
        assert_eq!(like_pattern("a_b"), "%a\\_b%");
        assert_eq!(like_pattern("c:\\x"), "%c:\\\\x%");
    }

    #[test]
    fn quote_name_escapes_backticks() {
        assert_eq!(quote_name("users"), "`users`");
        assert_eq!(quote_name("we`ird"), "`we``ird`");
    }

    #[test]
    fn match_expr_uses_bound_placeholder() {
        assert_eq!(match_expr("name", SearchMode::Like), "`name` LIKE ?");
        assert_eq!(match_expr("name", SearchMode::Regex), "`name` REGEXP ?");
        assert_eq!(bind_value(SearchMode::Like, "50%"), "%50\\%%");
        // 关键字中的引号属于参数内容，不做 SQL 字符串转义
        assert_eq!(bind_value(SearchMode::Like, "it's"), "%it's%");
    }
}
