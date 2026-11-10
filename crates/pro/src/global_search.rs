//! 跨库全局搜索（Pro）
//!
//! 表名 / 字段名：MySQL 走 `INFORMATION_SCHEMA`，PostgreSQL 走 `pg_catalog`；
//! 数据内容按表逐条 COUNT，并受「最多扫描表数 + 单表超时 + 总时间预算」三重约束，
//! 避免打爆线上库。
//!
//! PostgreSQL 的数据库在建立连接时固定、无法中途切换：搜索范围固定为当前连接的数据库
//! （覆盖其全部用户 schema），指定其它数据库时明确报错。

use anyhow::{anyhow, Result};
use auroradb_core::connection::driver::AnyPool;
use auroradb_core::connection::executor::is_searchable_type;
use auroradb_core::connection::ConnectionManager;
use serde::{Deserialize, Serialize};
use sqlx::Acquire;
use sqlx::postgres::PgPool;
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

/// 跨库全局搜索：表名 / 字段名 / 数据内容
pub async fn global_search(
    manager: &ConnectionManager,
    request: GlobalSearchRequest,
) -> Result<GlobalSearchResult> {
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

    let snapshot = manager.connection_snapshot(&request.connection_id).await?;
    // PostgreSQL 走 pg_catalog 分支；MySQL 保持原有 INFORMATION_SCHEMA 实现不变
    let pool = match snapshot.pool {
        AnyPool::Postgres(pg_pool) => {
            return global_search_postgres(&pg_pool, &request, database, mode, keyword, started)
                .await;
        }
        AnyPool::MySql(mysql_pool) => mysql_pool,
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
    let rows = query
        .bind(bind_value(mode, keyword))
        .fetch_all(pool)
        .await?;

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
    let rows = query
        .bind(bind_value(mode, keyword))
        .fetch_all(pool)
        .await?;

    let truncated = rows.len() > MAX_NAME_MATCHES;
    let matches = rows
        .into_iter()
        .take(MAX_NAME_MATCHES)
        .map(
            |(database, table, column, data_type)| GlobalSearchColumnMatch {
                database,
                table,
                column,
                data_type,
            },
        )
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
            columns.entry((schema, table)).or_default().push(column);
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

/// PostgreSQL catalog 查询超时：catalog 查询应很快，超出说明数据库异常
const PG_NAME_QUERY_TIMEOUT: Duration = Duration::from_secs(10);

/// 客户端超时余量：服务端 `statement_timeout` 会先中止语句，
/// 客户端再多等这么久只作为网络/驱动卡死的兜底
const PG_TIMEOUT_MARGIN: Duration = Duration::from_secs(2);

/// PG 标识符双引号包装。
///
/// 名称全部来自 `pg_catalog`，不是用户输入，因此不套 `validate_identifier`
/// （否则 `order`、中文列名等合法标识符会被误拒），只做双引号转义。
fn pg_quote_name(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// PG 对象展示名：public 直出，其余带 schema 前缀（与对象树 / 表结构一致）
fn pg_display_name(schema: &str, name: &str) -> String {
    if schema == "public" {
        name.to_string()
    } else {
        format!("{}.{}", schema, name)
    }
}

/// PG 名称匹配片段：关键字统一走 `$1` 绑定参数，名称一律转 text
fn pg_name_match(column: &str, mode: SearchMode) -> String {
    match mode {
        SearchMode::Like => format!("{}::text ILIKE $1", column),
        SearchMode::Regex => format!("{}::text ~* $1", column),
    }
}

/// 判断 PG 列类型是否参与数据内容搜索。
///
/// 数据匹配统一 `::text`：bytea 的文本表示没有意义，bit 串与数组同理，直接跳过。
fn is_searchable_pg_type(base_type: &str) -> bool {
    let t = base_type.trim().to_ascii_lowercase();
    !(t == "bytea" || t.starts_with("bit") || t.contains("[]"))
}

/// 构造 `SET LOCAL statement_timeout` 语句（毫秒），只对当前事务生效
fn pg_statement_timeout_sql(timeout: Duration) -> String {
    format!("SET LOCAL statement_timeout = '{}ms'", timeout.as_millis())
}

/// PostgreSQL 语句超时（SQLSTATE 57014）
fn is_pg_statement_timeout(error: &sqlx::Error) -> bool {
    matches!(
        error.as_database_error().and_then(|db| db.code()),
        Some(code) if code.as_ref() == "57014"
    )
}

/// PostgreSQL 正则语法错误（SQLSTATE 2201B）
fn is_pg_invalid_regex(error: &sqlx::Error) -> bool {
    matches!(
        error.as_database_error().and_then(|db| db.code()),
        Some(code) if code.as_ref() == "2201B"
    )
}

/// 带服务端超时执行 PG 查询（表名 / 字段名 / 候选表共用），错误转成可展示消息。
///
/// 超时不能只靠 `tokio::time::timeout`：放弃客户端 future 不会取消服务端语句，
/// 慢查询仍会占住连接直到执行完。这里把查询放进单连接事务，先用
/// `SET LOCAL statement_timeout` 让 PostgreSQL 自己中止语句，客户端只多等
/// [`PG_TIMEOUT_MARGIN`] 作为驱动卡死的兜底。
async fn pg_fetch_all<T>(
    pool: &PgPool,
    sql: &str,
    bind: Option<&str>,
    what: &str,
) -> Result<Vec<T>>
where
    T: for<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> + Send + Unpin,
{
    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| anyhow!("{}失败：{}", what, e))?;
    let mut tx = conn
        .begin()
        .await
        .map_err(|e| anyhow!("{}失败：{}", what, e))?;
    let timeout_sql = pg_statement_timeout_sql(PG_NAME_QUERY_TIMEOUT);
    sqlx::query(&timeout_sql)
        .execute(&mut *tx)
        .await
        .map_err(|e| anyhow!("{}失败：{}", what, e))?;

    let query = sqlx::query_as::<_, T>(sql);
    let query = match bind {
        Some(value) => query.bind(value),
        None => query,
    };
    let fetched = tokio::time::timeout(
        PG_NAME_QUERY_TIMEOUT + PG_TIMEOUT_MARGIN,
        query.fetch_all(&mut *tx),
    )
    .await;
    match fetched {
        Ok(Ok(rows)) => {
            let _ = tx.commit().await;
            Ok(rows)
        }
        // 服务端 statement_timeout 先触发，这里只做文案归一
        Ok(Err(e)) if is_pg_statement_timeout(&e) => Err(anyhow!(
            "{}超时（{} 秒）",
            what,
            PG_NAME_QUERY_TIMEOUT.as_secs()
        )),
        Ok(Err(e)) => Err(anyhow!("{}失败：{}", what, e)),
        Err(_) => Err(anyhow!(
            "{}超时（{} 秒）",
            what,
            PG_NAME_QUERY_TIMEOUT.as_secs()
        )),
    }
}

/// 正则模式在入口处先试编译一次。
///
/// PG 的正则错误要到执行 SQL 时才暴露：不预校验的话，名称搜索会整体报错，
/// 数据搜索则会对最多 [`MAX_DATA_TABLES`] 张表各报一次同样的错误。
async fn pg_validate_regex(pool: &PgPool, pattern: &str) -> Result<()> {
    let compiled = tokio::time::timeout(
        PG_NAME_QUERY_TIMEOUT,
        sqlx::query_scalar::<_, bool>("SELECT '' ~* $1")
            .bind(pattern)
            .fetch_one(pool),
    )
    .await;
    match compiled {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) if is_pg_invalid_regex(&e) => Err(anyhow!("无效的正则表达式：{}", e)),
        Ok(Err(e)) => Err(anyhow!("校验正则表达式失败：{}", e)),
        Err(_) => Err(anyhow!(
            "校验正则表达式超时（{} 秒）",
            PG_NAME_QUERY_TIMEOUT.as_secs()
        )),
    }
}

/// 当前连接的 PG 数据库名
async fn pg_current_database(pool: &PgPool) -> Result<String> {
    match tokio::time::timeout(
        PG_NAME_QUERY_TIMEOUT,
        sqlx::query_scalar::<_, String>("SELECT current_database()::text").fetch_one(pool),
    )
    .await
    {
        Ok(Ok(database)) => Ok(database),
        Ok(Err(e)) => Err(anyhow!("查询 PostgreSQL 当前数据库失败：{}", e)),
        Err(_) => Err(anyhow!(
            "查询 PostgreSQL 当前数据库超时（{} 秒）",
            PG_NAME_QUERY_TIMEOUT.as_secs()
        )),
    }
}

/// PostgreSQL 全局搜索：范围固定为当前连接的数据库（覆盖其全部用户 schema）
async fn global_search_postgres(
    pool: &PgPool,
    request: &GlobalSearchRequest,
    database: Option<&str>,
    mode: SearchMode,
    keyword: &str,
    started: Instant,
) -> Result<GlobalSearchResult> {
    let current_database = pg_current_database(pool).await?;
    if let Some(requested) = database {
        if requested != current_database {
            return Err(anyhow!(
                "PostgreSQL 连接的搜索范围固定为当前数据库 {}，不支持跨库搜索（请求的数据库：{}）；请切换到该数据库后再搜索",
                current_database,
                requested
            ));
        }
    }

    // 非法正则在入口统一报错：避免名称搜索整体失败、数据搜索逐表各报一次
    if mode == SearchMode::Regex {
        pg_validate_regex(pool, keyword).await?;
    }

    let mut notes: Vec<String> = Vec::new();
    let mut truncated = false;
    // MySQL 的「不限库」会扫描服务端全部业务库；PG 只能在连接库内搜索，需明确告知范围
    if database.is_none() {
        notes.push(format!(
            "PostgreSQL 连接的搜索范围固定为当前数据库 {}，已在该库全部 schema 中搜索",
            current_database
        ));
    }

    let (tables, table_truncated) = if request.search_tables.unwrap_or(true) {
        pg_search_table_names(pool, &current_database, mode, keyword).await?
    } else {
        (Vec::new(), false)
    };
    truncated |= table_truncated;

    let (columns, column_truncated) = if request.search_columns.unwrap_or(true) {
        pg_search_column_names(pool, &current_database, mode, keyword).await?
    } else {
        (Vec::new(), false)
    };
    truncated |= column_truncated;

    let mut scanned_tables = 0usize;
    let data = if request.search_data.unwrap_or(false) {
        let (matches, scanned, data_truncated, data_notes) =
            pg_search_data_content(pool, &current_database, mode, keyword, started).await?;
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

/// 搜索 PG 表名 / 视图名（只查用户 schema）
async fn pg_search_table_names(
    pool: &PgPool,
    database: &str,
    mode: SearchMode,
    keyword: &str,
) -> Result<(Vec<GlobalSearchTableMatch>, bool)> {
    // 外部表（relkind='f'）依赖 FDW，打开/查询代价不可控，名称与字段搜索一并排除
    let sql = format!(
        "SELECT n.nspname::text, c.relname::text, c.relkind::text \
         FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE c.relkind IN ('r', 'p', 'v', 'm') \
           AND n.nspname NOT IN ('pg_catalog', 'information_schema') \
           AND n.nspname NOT LIKE 'pg\\_%' \
           AND {} \
         ORDER BY n.nspname, c.relname LIMIT {}",
        pg_name_match("c.relname", mode),
        MAX_NAME_MATCHES + 1
    );
    let rows: Vec<(String, String, String)> =
        pg_fetch_all(pool, &sql, Some(&bind_value(mode, keyword)), "搜索表名").await?;

    let truncated = rows.len() > MAX_NAME_MATCHES;
    let matches = rows
        .into_iter()
        .take(MAX_NAME_MATCHES)
        .map(|(schema, table, relkind)| GlobalSearchTableMatch {
            database: database.to_string(),
            table: pg_display_name(&schema, &table),
            // 物化视图同样作为视图展示
            object_type: if relkind == "v" || relkind == "m" {
                "view".to_string()
            } else {
                "table".to_string()
            },
        })
        .collect();
    Ok((matches, truncated))
}

/// 搜索 PG 字段名（只查用户 schema）
async fn pg_search_column_names(
    pool: &PgPool,
    database: &str,
    mode: SearchMode,
    keyword: &str,
) -> Result<(Vec<GlobalSearchColumnMatch>, bool)> {
    let sql = format!(
        "SELECT n.nspname::text, c.relname::text, a.attname::text, \
                format_type(a.atttypid, NULL) \
         FROM pg_catalog.pg_attribute a \
         JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE c.relkind IN ('r', 'p', 'v', 'm') \
           AND a.attnum > 0 AND NOT a.attisdropped \
           AND n.nspname NOT IN ('pg_catalog', 'information_schema') \
           AND n.nspname NOT LIKE 'pg\\_%' \
           AND {} \
         ORDER BY n.nspname, c.relname, a.attnum LIMIT {}",
        pg_name_match("a.attname", mode),
        MAX_NAME_MATCHES + 1
    );
    let rows: Vec<(String, String, String, String)> =
        pg_fetch_all(pool, &sql, Some(&bind_value(mode, keyword)), "搜索字段名").await?;

    let truncated = rows.len() > MAX_NAME_MATCHES;
    let matches = rows
        .into_iter()
        .take(MAX_NAME_MATCHES)
        .map(|(schema, table, column, data_type)| GlobalSearchColumnMatch {
            database: database.to_string(),
            table: pg_display_name(&schema, &table),
            column,
            data_type,
        })
        .collect();
    Ok((matches, truncated))
}

/// 逐表搜索 PG 数据内容，返回（命中表, 扫描表数, 是否截断, 警告）
async fn pg_search_data_content(
    pool: &PgPool,
    database: &str,
    mode: SearchMode,
    keyword: &str,
    started: Instant,
) -> Result<(Vec<GlobalSearchDataMatch>, usize, bool, Vec<String>)> {
    // 只扫描基础表；视图 / 物化视图 / 外部表的代价不可控
    let table_sql = format!(
        "SELECT n.nspname::text, c.relname::text \
         FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE c.relkind IN ('r', 'p') \
           AND n.nspname NOT IN ('pg_catalog', 'information_schema') \
           AND n.nspname NOT LIKE 'pg\\_%' \
         ORDER BY n.nspname, c.relname LIMIT {}",
        MAX_DATA_TABLES + 1
    );
    let mut table_rows: Vec<(String, String)> =
        pg_fetch_all(pool, &table_sql, None, "搜索候选表").await?;

    let mut truncated = table_rows.len() > MAX_DATA_TABLES;
    table_rows.truncate(MAX_DATA_TABLES);

    // 一次性取回所有列，按 (schema, 表) 分组；bytea / bit / 数组不参与匹配
    let column_sql = "SELECT n.nspname::text, c.relname::text, a.attname::text, \
                             format_type(a.atttypid, NULL) \
                      FROM pg_catalog.pg_attribute a \
                      JOIN pg_catalog.pg_class c ON c.oid = a.attrelid \
                      JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
                      WHERE c.relkind IN ('r', 'p') \
                        AND a.attnum > 0 AND NOT a.attisdropped \
                        AND n.nspname NOT IN ('pg_catalog', 'information_schema') \
                        AND n.nspname NOT LIKE 'pg\\_%' \
                      ORDER BY n.nspname, c.relname, a.attnum";
    let column_rows: Vec<(String, String, String, String)> =
        pg_fetch_all(pool, column_sql, None, "搜索候选列").await?;

    let mut columns: HashMap<(String, String), Vec<String>> = HashMap::new();
    for (schema, table, column, data_type) in column_rows {
        if is_searchable_pg_type(&data_type) {
            columns.entry((schema, table)).or_default().push(column);
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
        match pg_count_matching_rows(pool, &schema, &table, table_columns, mode, keyword).await {
            Ok(Some(count)) => matches.push(GlobalSearchDataMatch {
                database: database.to_string(),
                table: pg_display_name(&schema, &table),
                matched_rows: count.min(MATCHED_ROWS_CAP) as u64,
                count_capped: count >= MATCHED_ROWS_CAP,
            }),
            Ok(None) => {}
            Err(reason) => notes.push(format!("{}：{}", label, reason)),
        }
    }

    Ok((matches, scanned, truncated, notes))
}

/// 统计单张 PG 表命中行数（最多 `MATCHED_ROWS_CAP + 1` 行）。
///
/// 与 MySQL 分支同语义：用派生表 + LIMIT 代替全表 COUNT，命中很多时提前停止。
/// 返回 `Ok(None)` 表示没有命中；`Err` 为可展示的失败/超时原因。
async fn pg_count_matching_rows(
    pool: &PgPool,
    schema: &str,
    table: &str,
    columns: &[String],
    mode: SearchMode,
    keyword: &str,
) -> std::result::Result<Option<i64>, String> {
    let conditions: Vec<String> = columns
        .iter()
        .map(|column| {
            let quoted = pg_quote_name(column);
            match mode {
                SearchMode::Like => format!("{}::text ILIKE $1", quoted),
                SearchMode::Regex => format!("{}::text ~* $1", quoted),
            }
        })
        .collect();
    let sql = format!(
        "SELECT COUNT(*) FROM (SELECT 1 FROM {}.{} WHERE {} LIMIT {}) AS _aurora_search",
        pg_quote_name(schema),
        pg_quote_name(table),
        conditions.join(" OR "),
        MATCHED_ROWS_CAP + 1
    );

    // 与名称查询一致：让服务端 statement_timeout 中止慢 COUNT，
    // 避免只放弃客户端 future、语句仍在服务端占住池连接
    let mut conn = pool.acquire().await.map_err(|e| format!("查询失败：{}", e))?;
    let mut tx = conn
        .begin()
        .await
        .map_err(|e| format!("查询失败：{}", e))?;
    let timeout_sql = pg_statement_timeout_sql(PER_TABLE_TIMEOUT);
    sqlx::query(&timeout_sql)
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("查询失败：{}", e))?;

    let query = sqlx::query_scalar::<_, i64>(&sql).bind(bind_value(mode, keyword));
    let fetched = tokio::time::timeout(
        PER_TABLE_TIMEOUT + PG_TIMEOUT_MARGIN,
        query.fetch_one(&mut *tx),
    )
    .await;
    match fetched {
        Ok(Ok(count)) if count > 0 => {
            let _ = tx.commit().await;
            Ok(Some(count))
        }
        Ok(Ok(_)) => {
            let _ = tx.commit().await;
            Ok(None)
        }
        // 服务端 statement_timeout 先触发，这里只做文案归一
        Ok(Err(e)) if is_pg_statement_timeout(&e) => {
            Err(format!("查询超时（{} 秒）", PER_TABLE_TIMEOUT.as_secs()))
        }
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

    #[test]
    fn pg_quote_name_escapes_double_quotes() {
        assert_eq!(pg_quote_name("users"), "\"users\"");
        assert_eq!(pg_quote_name("we\"ird"), "\"we\"\"ird\"");
    }

    #[test]
    fn pg_display_name_drops_public_prefix() {
        assert_eq!(pg_display_name("public", "users"), "users");
        assert_eq!(pg_display_name("sales", "orders"), "sales.orders");
    }

    #[test]
    fn pg_name_match_uses_bound_placeholder() {
        assert_eq!(
            pg_name_match("c.relname", SearchMode::Like),
            "c.relname::text ILIKE $1"
        );
        assert_eq!(
            pg_name_match("a.attname", SearchMode::Regex),
            "a.attname::text ~* $1"
        );
        // 与 MySQL 分支共用 bind_value：通配符按字面量处理，关键字走参数
        assert_eq!(bind_value(SearchMode::Like, "a_b"), "%a\\_b%");
    }

    #[test]
    fn pg_searchable_type_skips_binary_and_arrays() {
        assert!(is_searchable_pg_type("text"));
        assert!(is_searchable_pg_type("numeric"));
        assert!(is_searchable_pg_type("character varying"));
        assert!(!is_searchable_pg_type("bytea"));
        assert!(!is_searchable_pg_type("bit varying"));
        assert!(!is_searchable_pg_type("integer[]"));
    }

    #[test]
    fn pg_statement_timeout_sql_uses_milliseconds() {
        assert_eq!(
            pg_statement_timeout_sql(Duration::from_secs(5)),
            "SET LOCAL statement_timeout = '5000ms'"
        );
        assert_eq!(
            pg_statement_timeout_sql(Duration::from_millis(1500)),
            "SET LOCAL statement_timeout = '1500ms'"
        );
    }

    /// 本地 PostgreSQL 测试连接：未配置 AURORADB_TEST_PG_HOST 时返回 None 静默跳过；
    /// 已配置但缺项或连不上直接 panic，避免测试环境坏了却“通过”。
    async fn pg_test_connection() -> Option<(ConnectionManager, String, String)> {
        use auroradb_core::connection::driver::DriverKind;
        use auroradb_core::connection::{
            ConnectionConfig, ConnectionRequest, PoolConfig, SafetyMode, SslConfig,
        };

        // 只把「完全没配置」当作跳过；配置了 host 就要求其余项齐全、能连上
        let host = std::env::var("AURORADB_TEST_PG_HOST").ok()?;
        let port = std::env::var("AURORADB_TEST_PG_PORT")
            .expect("设置了 AURORADB_TEST_PG_HOST 时必须同时设置 AURORADB_TEST_PG_PORT")
            .parse::<u16>()
            .expect("AURORADB_TEST_PG_PORT 必须是合法端口号");
        let username = std::env::var("AURORADB_TEST_PG_USER")
            .expect("设置了 AURORADB_TEST_PG_HOST 时必须同时设置 AURORADB_TEST_PG_USER");
        let password = std::env::var("AURORADB_TEST_PG_PASSWORD").unwrap_or_default();
        let database = std::env::var("AURORADB_TEST_PG_DATABASE")
            .expect("设置了 AURORADB_TEST_PG_HOST 时必须同时设置 AURORADB_TEST_PG_DATABASE");

        let manager = ConnectionManager::new();
        let id = "qa-pg-global-search".to_string();
        manager
            .connect(ConnectionRequest {
                config: ConnectionConfig {
                    id: id.clone(),
                    name: id.clone(),
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
            .unwrap_or_else(|e| {
                panic!("连接 PostgreSQL 测试库失败（已配置 AURORADB_TEST_PG_*）：{}", e)
            });
        Some((manager, id, database))
    }

    async fn exec_sql(
        manager: &ConnectionManager,
        connection_id: &str,
        database: &str,
        sql: &str,
    ) {
        manager
            .execute(connection_id, database, sql)
            .await
            .unwrap_or_else(|e| panic!("执行 SQL 失败：{}；SQL：{}", e, sql));
    }

    /// 真实 PostgreSQL 上验证：表名 / 字段名 / 数据内容三类命中、大小写不敏感、
    /// LIKE 通配符字面量化、命中行数封顶、无命中为空、范围限制与跨库报错。
    #[tokio::test]
    #[ignore = "需要真实 PostgreSQL 测试库：设置 AURORADB_TEST_PG_* 后执行 cargo test -- --ignored"]
    async fn pg_global_search_finds_names_and_data() {
        let Some((manager, connection_id, database)) = pg_test_connection().await else {
            return;
        };

        let pid = std::process::id();
        let table = format!("qa_gs_table_{}", pid);
        let view = format!("qa_gs_view_{}", pid);
        let needle_col = format!("needle_col_{}", pid);

        // 测试并行 + 可能残留上次失败的对象，先清理
        let _ = manager
            .execute(
                &connection_id,
                &database,
                &format!("DROP VIEW IF EXISTS {}", pg_quote_name(&view)),
            )
            .await;
        let _ = manager
            .execute(
                &connection_id,
                &database,
                &format!("DROP TABLE IF EXISTS {} CASCADE", pg_quote_name(&table)),
            )
            .await;

        exec_sql(
            &manager,
            &connection_id,
            &database,
            &format!(
                "CREATE TABLE {} (\
                   id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY, \
                   {} VARCHAR(64) NOT NULL, \
                   amount NUMERIC(10,2) DEFAULT 0.00, \
                   memo TEXT\
                 )",
                pg_quote_name(&table),
                pg_quote_name(&needle_col)
            ),
        )
        .await;
        exec_sql(
            &manager,
            &connection_id,
            &database,
            &format!(
                "INSERT INTO {} ({}, amount, memo) VALUES \
                 ('NeedleValue', 1.50, 'alpha'), \
                 ('a_b', 2.50, 'underscore literal'), \
                 ('axb', 3.50, 'wildcard probe'), \
                 ('50%_x', 4.50, 'special chars')",
                pg_quote_name(&table),
                pg_quote_name(&needle_col)
            ),
        )
        .await;
        // 超过命中行数上限（1000）的数据，用于验证 count_capped
        exec_sql(
            &manager,
            &connection_id,
            &database,
            &format!(
                "INSERT INTO {} ({}, amount, memo) \
                 SELECT 'capneedle', 1.00, 'cap' FROM generate_series(1, {})",
                pg_quote_name(&table),
                pg_quote_name(&needle_col),
                MATCHED_ROWS_CAP + 5
            ),
        )
        .await;
        exec_sql(
            &manager,
            &connection_id,
            &database,
            &format!(
                "CREATE VIEW {} AS SELECT id, {} FROM {}",
                pg_quote_name(&view),
                pg_quote_name(&needle_col),
                pg_quote_name(&table)
            ),
        )
        .await;

        let request = |database: Option<&str>, keyword: &str, search_data: bool| {
            GlobalSearchRequest {
                connection_id: connection_id.clone(),
                database: database.map(str::to_string),
                keyword: keyword.to_string(),
                mode: None,
                search_tables: Some(true),
                search_columns: Some(true),
                search_data: Some(search_data),
            }
        };

        // 表名 / 视图名命中：ILIKE 大小写不敏感（关键字全大写）
        let result = global_search(
            &manager,
            request(Some(&database), &table.to_uppercase(), false),
        )
        .await
        .expect("表名搜索应成功");
        assert!(
            result
                .tables
                .iter()
                .any(|m| m.table == table && m.object_type == "table" && m.database == database),
            "表名应命中：{:?}",
            result.tables
        );

        let result = global_search(
            &manager,
            request(Some(&database), &view.to_uppercase(), false),
        )
        .await
        .expect("视图名搜索应成功");
        assert!(
            result
                .tables
                .iter()
                .any(|m| m.table == view && m.object_type == "view"),
            "视图名应命中且标记为 view：{:?}",
            result.tables
        );

        // 字段名命中
        let result = global_search(&manager, request(Some(&database), &needle_col, false))
            .await
            .expect("字段名搜索应成功");
        assert!(
            result.columns.iter().any(
                |m| m.table == table && m.column == needle_col && m.database == database
            ),
            "字段名应命中：{:?}",
            result.columns
        );

        // 数据内容命中：大小写不敏感
        let result = global_search(&manager, request(Some(&database), "needlevalue", true))
            .await
            .expect("数据搜索应成功");
        let hit = result
            .data
            .iter()
            .find(|m| m.table == table)
            .unwrap_or_else(|| panic!("数据内容应命中：{:?}", result.data));
        assert_eq!(hit.database, database);
        assert_eq!(hit.matched_rows, 1, "只应命中一行人可读值");
        assert!(!hit.count_capped);
        assert!(result.scanned_tables >= 1);

        // LIKE 通配符按字面量处理："a_b" 不应命中 "axb"
        let result = global_search(&manager, request(Some(&database), "a_b", true))
            .await
            .expect("通配符字面量搜索应成功");
        let hit = result
            .data
            .iter()
            .find(|m| m.table == table)
            .unwrap_or_else(|| panic!("a_b 应命中：{:?}", result.data));
        assert_eq!(hit.matched_rows, 1, "\"a_b\" 只能命中字面量那一行");

        // 数值列转 text 后可搜索：金额 2.50 的文本包含 "2.5"
        let result = global_search(&manager, request(Some(&database), "2.5", true))
            .await
            .expect("数值搜索应成功");
        assert!(
            result.data.iter().any(|m| m.table == table && m.matched_rows == 1),
            "数值列应可搜索：{:?}",
            result.data
        );

        // 命中行数封顶
        let result = global_search(&manager, request(Some(&database), "capneedle", true))
            .await
            .expect("封顶搜索应成功");
        let hit = result
            .data
            .iter()
            .find(|m| m.table == table)
            .unwrap_or_else(|| panic!("capneedle 应命中：{:?}", result.data));
        assert_eq!(hit.matched_rows, MATCHED_ROWS_CAP as u64);
        assert!(hit.count_capped, "超过上限应标记 count_capped");

        // 无命中：三类结果均为空
        let result = global_search(
            &manager,
            request(Some(&database), &format!("qa_gs_nomatch_{}", pid), true),
        )
        .await
        .expect("无命中搜索应成功");
        assert!(result.tables.is_empty(), "无命中时不应有表名结果");
        assert!(result.columns.is_empty(), "无命中时不应有字段名结果");
        assert!(result.data.is_empty(), "无命中时不应有数据结果");
        assert!(result.warning.is_none(), "无命中不需要警告");

        // 未指定数据库：搜索当前连接库，并提示范围
        let result = global_search(&manager, request(None, &table, false))
            .await
            .expect("不指定数据库搜索应成功");
        assert!(
            result.tables.iter().any(|m| m.table == table),
            "不指定数据库时应搜索当前连接库：{:?}",
            result.tables
        );
        let warning = result.warning.expect("应提示 PG 搜索范围");
        assert!(
            warning.contains("搜索范围固定为当前数据库") && warning.contains(&database),
            "警告应包含当前数据库名：{}",
            warning
        );

        // 指定其它数据库：明确报错，不静默返回空结果
        let error = global_search(
            &manager,
            request(Some(&format!("qa_gs_missing_db_{}", pid)), &table, false),
        )
        .await
        .expect_err("跨库搜索应报错");
        let message = error.to_string();
        assert!(
            message.contains("不支持跨库搜索") && message.contains(&database),
            "错误应说明范围与当前库：{}",
            message
        );

        // 非法正则在搜索入口即报错：不应在数据搜索里对每张表各报一次
        let mut regex_request = request(Some(&database), "[", true);
        regex_request.mode = Some("regex".to_string());
        let error = global_search(&manager, regex_request)
            .await
            .expect_err("非法正则应报错");
        let message = error.to_string();
        assert!(
            message.contains("无效的正则表达式"),
            "错误应指明正则非法：{}",
            message
        );

        // 合法正则走同一路径
        let mut regex_request = request(Some(&database), "needlevalue", true);
        regex_request.mode = Some("regex".to_string());
        let result = global_search(&manager, regex_request)
            .await
            .expect("合法正则搜索应成功");
        assert!(
            result
                .data
                .iter()
                .any(|m| m.table == table && m.matched_rows == 1),
            "正则数据搜索应命中：{:?}",
            result.data
        );

        let _ = manager
            .execute(
                &connection_id,
                &database,
                &format!("DROP VIEW IF EXISTS {}", pg_quote_name(&view)),
            )
            .await;
        let _ = manager
            .execute(
                &connection_id,
                &database,
                &format!("DROP TABLE IF EXISTS {} CASCADE", pg_quote_name(&table)),
            )
            .await;
    }
}
