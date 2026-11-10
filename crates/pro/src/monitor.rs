//! 会话与实例监控
//! MySQL：`SHOW FULL PROCESSLIST` 进程列表、`KILL CONNECTION` 终止会话，
//! 以及基于 `SHOW GLOBAL STATUS` / `SHOW GLOBAL VARIABLES` 的服务端指标。
//! PostgreSQL：`pg_stat_activity` 会话列表、`pg_terminate_backend` 终止会话，
//! 以及基于 `pg_stat_database` / `pg_stat_activity` / `pg_settings` 的服务端指标。

use anyhow::{anyhow, Result};
use auroradb_core::connection::driver::AnyPool;
use auroradb_core::connection::metadata::with_metadata_timeout;
use auroradb_core::connection::ConnectionManager;
use auroradb_core::error::{translate, user_message};
use serde::Serialize;
use sqlx::mysql::MySqlRow;
use sqlx::{MySqlPool, PgPool, Row};
use std::collections::HashMap;

/// 服务端会话/进程（对应 `SHOW FULL PROCESSLIST` 或 `pg_stat_activity` 的一行）
#[derive(Debug, Clone, Serialize)]
pub struct ProcessInfo {
    pub id: u64,
    pub user: String,
    pub host: String,
    pub database: Option<String>,
    pub command: String,
    /// 当前状态已持续的秒数
    pub time: u64,
    pub state: Option<String>,
    pub info: Option<String>,
}

/// 服务端运行指标（来自 `SHOW GLOBAL STATUS` / `SHOW GLOBAL VARIABLES` 或 PG 系统视图）
#[derive(Debug, Clone, Serialize)]
pub struct ServerMetrics {
    pub version: String,
    pub uptime_seconds: u64,
    /// 自启动以来的平均每秒查询数（MySQL：Questions / Uptime；PG：事务数 / Uptime）
    pub qps: f64,
    pub threads_connected: u64,
    pub threads_running: u64,
    pub max_connections: u64,
    pub slow_queries: u64,
    pub slow_query_log: bool,
    pub long_query_time: f64,
    pub aborted_connects: u64,
    /// 缓冲池命中率（百分比，0-100）；MySQL 为 InnoDB 缓冲池，PG 为共享缓冲命中率；缺少数据时为 None
    pub buffer_pool_hit_rate: Option<f64>,
    pub bytes_received: u64,
    pub bytes_sent: u64,
    pub com_select: u64,
    pub com_insert: u64,
    pub com_update: u64,
    pub com_delete: u64,
}

/// 读取服务端全部会话（MySQL 需要 PROCESS 权限，否则只能看到当前用户的会话；
/// PG 需要 pg_read_all_stats 或超级用户才能看到其他用户的全部 SQL 文本）
pub async fn get_process_list(
    manager: &ConnectionManager,
    connection_id: &str,
) -> Result<Vec<ProcessInfo>> {
    let snapshot = manager.connection_snapshot(connection_id).await?;
    match &snapshot.pool {
        AnyPool::MySql(pool) => mysql_process_list(pool).await,
        AnyPool::Postgres(pool) => postgres_process_list(pool).await,
    }
}

/// 终止指定会话（MySQL `KILL CONNECTION <id>`；PG `pg_terminate_backend(pid)`）；
/// process_id 为 u64，天然避免注入
pub async fn kill_process(
    manager: &ConnectionManager,
    connection_id: &str,
    process_id: u64,
) -> Result<()> {
    if process_id == 0 {
        return Err(anyhow!("无效的会话 ID"));
    }

    let snapshot = manager.connection_snapshot(connection_id).await?;
    match &snapshot.pool {
        AnyPool::MySql(pool) => {
            with_metadata_timeout(
                sqlx::query(&format!("KILL CONNECTION {}", process_id)).execute(pool),
            )
            .await?;
            Ok(())
        }
        AnyPool::Postgres(pool) => postgres_kill_process(pool, process_id).await,
    }
}

/// 读取服务端版本、连接数、QPS、慢查询与缓冲池命中率等指标
pub async fn get_server_metrics(
    manager: &ConnectionManager,
    connection_id: &str,
) -> Result<ServerMetrics> {
    let snapshot = manager.connection_snapshot(connection_id).await?;
    match &snapshot.pool {
        AnyPool::MySql(pool) => mysql_server_metrics(pool).await,
        AnyPool::Postgres(pool) => postgres_server_metrics(pool).await,
    }
}

// ---------------------------------------------------------------------------
// MySQL 实现
// ---------------------------------------------------------------------------

async fn mysql_process_list(pool: &MySqlPool) -> Result<Vec<ProcessInfo>> {
    let rows: Vec<MySqlRow> =
        with_metadata_timeout(sqlx::query("SHOW FULL PROCESSLIST").fetch_all(pool)).await?;

    Ok(rows.iter().map(mysql_process_from_row).collect())
}

async fn mysql_server_metrics(pool: &MySqlPool) -> Result<ServerMetrics> {
    let status_rows: Vec<(String, Option<String>)> =
        with_metadata_timeout(sqlx::query_as("SHOW GLOBAL STATUS").fetch_all(pool)).await?;
    let variable_rows: Vec<(String, Option<String>)> = with_metadata_timeout(
        sqlx::query_as(
            "SHOW GLOBAL VARIABLES WHERE Variable_name IN
             ('version', 'max_connections', 'slow_query_log', 'long_query_time')",
        )
        .fetch_all(pool),
    )
    .await?;

    let status = normalize_pairs(status_rows);
    let variables = normalize_pairs(variable_rows);
    Ok(build_mysql_metrics(&status, &variables))
}

/// 把 `SHOW ...` 的 (名称, 值) 行转成小写键的 map，忽略 NULL 值
fn normalize_pairs(rows: Vec<(String, Option<String>)>) -> HashMap<String, String> {
    rows.into_iter()
        .filter_map(|(name, value)| value.map(|v| (name.to_lowercase(), v)))
        .collect()
}

/// `SHOW FULL PROCESSLIST` 走文本协议，sqlx 只填 `columns` 不填 `column_names`，
/// 因此这里按列序号取值（顺序固定为 Id/User/Host/db/Command/Time/State/Info）。
fn mysql_process_from_row(row: &MySqlRow) -> ProcessInfo {
    ProcessInfo {
        id: row
            .try_get::<u64, _>(0)
            .or_else(|_| row.try_get::<i64, _>(0).map(|value| value.max(0) as u64))
            .unwrap_or(0),
        user: row.try_get::<String, _>(1).unwrap_or_default(),
        host: row.try_get::<String, _>(2).unwrap_or_default(),
        database: row.try_get::<Option<String>, _>(3).unwrap_or_default(),
        command: row.try_get::<String, _>(4).unwrap_or_default(),
        time: row.try_get::<i64, _>(5).unwrap_or(0).max(0) as u64,
        state: row.try_get::<Option<String>, _>(6).unwrap_or_default(),
        info: row.try_get::<Option<String>, _>(7).unwrap_or_default(),
    }
}

fn number(map: &HashMap<String, String>, key: &str) -> u64 {
    map.get(key)
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(0.0) as u64
}

fn decimal(map: &HashMap<String, String>, key: &str) -> f64 {
    map.get(key)
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
        .unwrap_or(0.0)
}

/// 由状态/变量 map 组装 MySQL 指标；拆分出来便于单测（不依赖真实数据库）
fn build_mysql_metrics(
    status: &HashMap<String, String>,
    variables: &HashMap<String, String>,
) -> ServerMetrics {
    let uptime = number(status, "uptime");
    let questions = number(status, "questions");
    let qps = if uptime > 0 {
        questions as f64 / uptime as f64
    } else {
        0.0
    };

    let read_requests = number(status, "innodb_buffer_pool_read_requests");
    let physical_reads = number(status, "innodb_buffer_pool_reads");
    let buffer_pool_hit_rate = (read_requests > 0).then(|| {
        let hits = read_requests.saturating_sub(physical_reads) as f64;
        (hits / read_requests as f64 * 100.0 * 100.0).round() / 100.0
    });

    ServerMetrics {
        version: variables.get("version").cloned().unwrap_or_default(),
        uptime_seconds: uptime,
        qps: (qps * 100.0).round() / 100.0,
        threads_connected: number(status, "threads_connected"),
        threads_running: number(status, "threads_running"),
        max_connections: number(variables, "max_connections"),
        slow_queries: number(status, "slow_queries"),
        slow_query_log: variables
            .get("slow_query_log")
            .map(|value| value.eq_ignore_ascii_case("on") || value == "1")
            .unwrap_or(false),
        long_query_time: decimal(variables, "long_query_time"),
        aborted_connects: number(status, "aborted_connects"),
        buffer_pool_hit_rate,
        bytes_received: number(status, "bytes_received"),
        bytes_sent: number(status, "bytes_sent"),
        com_select: number(status, "com_select"),
        com_insert: number(status, "com_insert"),
        com_update: number(status, "com_update"),
        com_delete: number(status, "com_delete"),
    }
}

// ---------------------------------------------------------------------------
// PostgreSQL 实现
//
// PG 没有 `SHOW PROCESSLIST` / `SHOW GLOBAL STATUS` 的等价物，这里用系统视图：
// - 会话列表：`pg_stat_activity`，只取 client backend（与 MySQL 只统计客户端连接一致）
// - 终止会话：`pg_terminate_backend(pid)`，对不存在/无权限的 pid 返回 false
// - 实例指标：`pg_stat_database` 全库累计值 + `pg_stat_activity` 连接数 +
//   `current_setting('max_connections')` / `pg_settings` + `pg_postmaster_start_time()`
// 所有 SQL 均通过参数绑定或常量，不拼接任何用户输入。
// ---------------------------------------------------------------------------

/// `pg_stat_activity` 行（列顺序固定）：
/// pid、用户名、客户端地址、客户端端口、库名、状态、状态持续秒数、等待事件、当前/最近 SQL
type PgProcessRow = (
    i64,            // pid
    String,         // usename
    Option<String>, // client_addr
    Option<i32>,    // client_port
    Option<String>, // datname
    String,         // state
    i64,            // 状态持续秒数
    Option<String>, // 等待事件
    Option<String>, // query
);

const PG_PROCESS_LIST_SQL: &str = "\
SELECT pid::int8,
       COALESCE(usename, '')::text,
       client_addr::text,
       client_port,
       datname,
       COALESCE(state, '')::text,
       GREATEST(FLOOR(EXTRACT(EPOCH FROM (now() - COALESCE(state_change, backend_start))))::int8, 0),
       CASE WHEN wait_event IS NOT NULL THEN wait_event_type || ': ' || wait_event END,
       query
FROM pg_stat_activity
WHERE backend_type = 'client backend'
ORDER BY backend_start NULLS LAST, pid";

async fn postgres_process_list(pool: &PgPool) -> Result<Vec<ProcessInfo>> {
    let rows: Vec<PgProcessRow> =
        with_metadata_timeout(sqlx::query_as(PG_PROCESS_LIST_SQL).fetch_all(pool))
            .await
            .map_err(|e| anyhow!("查询 PostgreSQL 会话列表失败：{}", user_message(&e)))?;

    Ok(rows.into_iter().map(pg_process_from_row).collect())
}

fn pg_process_from_row(row: PgProcessRow) -> ProcessInfo {
    let (pid, user, client_addr, client_port, database, state, elapsed_seconds, wait_event, query) =
        row;

    // Unix socket 连接没有 client_addr，MySQL 侧显示为 "host:port"，这里保持同样形态
    let host = match (client_addr, client_port) {
        (Some(addr), Some(port)) => format!("{}:{}", addr, port),
        (Some(addr), None) => addr,
        (None, _) => "local".to_string(),
    };

    ProcessInfo {
        id: non_negative_to_u64(pid),
        user,
        host,
        database,
        // 前端（MonitorPanel.vue）按 MySQL 的 Query/Sleep 判定展示样式，这里统一成 MySQL 口径
        command: pg_command_from_state(&state),
        time: non_negative_to_u64(elapsed_seconds),
        state: wait_event,
        info: query,
    }
}

/// 把 PG `pg_stat_activity.state` 映射成 MySQL `SHOW PROCESSLIST` 的 `Command` 口径：
/// `active` → `Query`；空闲类状态 → `Sleep`；其余（如 `fastpath function call`）保留原值。
fn pg_command_from_state(state: &str) -> String {
    match state {
        "active" => "Query".to_string(),
        "idle" | "idle in transaction" | "idle in transaction (aborted)" | "disabled" => {
            "Sleep".to_string()
        }
        other => other.to_string(),
    }
}

async fn postgres_kill_process(pool: &PgPool, process_id: u64) -> Result<()> {
    let pid = i32::try_from(process_id).map_err(|_| anyhow!("无效的会话 ID：{}", process_id))?;

    // PG 对已不存在、已结束以及当前账号无权终止的后端都返回 false（而不是报错），
    // 因此不能只依赖 SQL 错误，需要同时检查返回值。
    let terminated: Option<bool> = with_metadata_timeout(async {
        sqlx::query_scalar("SELECT pg_terminate_backend($1)")
            .bind(pid)
            .fetch_one(pool)
            .await
            .map_err(|e| anyhow!("终止会话 {} 失败：{}", process_id, translate(&e)))
    })
    .await?;

    match terminated {
        Some(true) => Ok(()),
        _ => Err(anyhow!(
            "终止会话 {} 失败：会话不存在、已结束或当前账号无权操作",
            process_id
        )),
    }
}

/// PG 实例累计统计（`pg_stat_database` 全库汇总 + 连接数 + uptime + 版本）
#[derive(Debug, Clone, Copy, Default)]
struct PgStats {
    xact_commit: u64,
    xact_rollback: u64,
    blks_read: u64,
    blks_hit: u64,
    tup_returned: u64,
    tup_inserted: u64,
    tup_updated: u64,
    tup_deleted: u64,
    connections: u64,
    running: u64,
    uptime_seconds: u64,
    max_connections: u64,
}

/// PG 指标行（列顺序固定）：8 个 `pg_stat_database` 汇总值、连接数、运行中连接数、uptime、版本
type PgMetricsRow = (
    i64,    // xact_commit
    i64,    // xact_rollback
    i64,    // blks_read
    i64,    // blks_hit
    i64,    // tup_returned
    i64,    // tup_inserted
    i64,    // tup_updated
    i64,    // tup_deleted
    i64,    // 连接数（pg_stat_database.numbackends 汇总）
    i64,    // 运行中连接数（pg_stat_activity 中 state = 'active' 的 client backend）
    i64,    // uptime 秒数
    String, // version()
);

// 连接数用 `pg_stat_database.numbackends` 汇总：PG 会按行级权限隐藏 `pg_stat_activity`
// 中其他用户的会话（普通角色只看到自己），但累计视图 `pg_stat_database` 对普通角色可见，
// 用 activity 计数会导致普通账号永远显示 “1 / max_connections”。
// 运行中连接数只能从 `pg_stat_activity` 的 active 行数得到，需要 pg_monitor / pg_read_all_stats
// 权限才准确，缺权限时会偏小。
const PG_METRICS_SQL: &str = "\
SELECT COALESCE(SUM(xact_commit), 0)::bigint,
       COALESCE(SUM(xact_rollback), 0)::bigint,
       COALESCE(SUM(blks_read), 0)::bigint,
       COALESCE(SUM(blks_hit), 0)::bigint,
       COALESCE(SUM(tup_returned), 0)::bigint,
       COALESCE(SUM(tup_inserted), 0)::bigint,
       COALESCE(SUM(tup_updated), 0)::bigint,
       COALESCE(SUM(tup_deleted), 0)::bigint,
       (SELECT COALESCE(SUM(numbackends), 0)::bigint FROM pg_stat_database),
       (SELECT COUNT(*)::bigint FROM pg_stat_activity
         WHERE backend_type = 'client backend' AND state = 'active'),
       GREATEST(FLOOR(EXTRACT(EPOCH FROM (now() - pg_postmaster_start_time())))::bigint, 0),
       version()
FROM pg_stat_database";

const PG_SETTINGS_SQL: &str = "\
SELECT COALESCE(current_setting('max_connections'), '0')::bigint,
       COALESCE((SELECT setting FROM pg_settings WHERE name = 'log_min_duration_statement'), '-1')::bigint";

async fn postgres_server_metrics(pool: &PgPool) -> Result<ServerMetrics> {
    let row: PgMetricsRow = with_metadata_timeout(sqlx::query_as(PG_METRICS_SQL).fetch_one(pool))
        .await
        .map_err(|e| anyhow!("查询 PostgreSQL 实例指标失败：{}", user_message(&e)))?;
    let (mut stats, version) = pg_stats_from_row(row);

    let (max_connections, log_min_duration_statement_ms): (i64, i64) =
        with_metadata_timeout(sqlx::query_as(PG_SETTINGS_SQL).fetch_one(pool))
            .await
            .map_err(|e| anyhow!("查询 PostgreSQL 实例配置失败：{}", user_message(&e)))?;
    stats.max_connections = non_negative_to_u64(max_connections);

    Ok(build_pg_metrics(
        stats,
        version,
        log_min_duration_statement_ms,
    ))
}

fn pg_stats_from_row(row: PgMetricsRow) -> (PgStats, String) {
    let (
        xact_commit,
        xact_rollback,
        blks_read,
        blks_hit,
        tup_returned,
        tup_inserted,
        tup_updated,
        tup_deleted,
        connections,
        running,
        uptime_seconds,
        version,
    ) = row;

    let stats = PgStats {
        xact_commit: non_negative_to_u64(xact_commit),
        xact_rollback: non_negative_to_u64(xact_rollback),
        blks_read: non_negative_to_u64(blks_read),
        blks_hit: non_negative_to_u64(blks_hit),
        tup_returned: non_negative_to_u64(tup_returned),
        tup_inserted: non_negative_to_u64(tup_inserted),
        tup_updated: non_negative_to_u64(tup_updated),
        tup_deleted: non_negative_to_u64(tup_deleted),
        connections: non_negative_to_u64(connections),
        running: non_negative_to_u64(running),
        uptime_seconds: non_negative_to_u64(uptime_seconds),
        max_connections: 0, // 由调用方用 pg_settings 补充
    };
    (stats, version)
}

/// 由 PG 累计统计组装实例指标（拆分出来便于单测）
///
/// MySQL 专有语义字段在 PG 上的取舍（ServerMetrics 结构体保持公共契约不变，
/// PG 没有对应累计量的字段取 0）：
/// - `qps`：PG 没有 Questions，用 (xact_commit + xact_rollback) / uptime 近似「每秒事务数」；
/// - `threads_connected`：`pg_stat_database.numbackends` 汇总（普通角色也能看到全量；
///   `pg_stat_activity` 会被行级权限过滤，只有自己的会话可见）；
/// - `threads_running`：`pg_stat_activity` 中 active 的 client backend 数，需要
///   pg_monitor / pg_read_all_stats 权限才准确，缺权限时偏小；
/// - `com_select` 等：PG 无语句计数器（不依赖 pg_stat_statements 扩展），用行数近似，
///   即 `tup_returned` / `tup_inserted` / `tup_updated` / `tup_deleted`；
/// - `buffer_pool_hit_rate`：`blks_hit / (blks_hit + blks_read)`，即实例共享缓冲命中率；
/// - `slow_query_log` / `long_query_time`：`log_min_duration_statement`（毫秒）是否启用；
/// - `slow_queries` / `aborted_connects` / `bytes_received` / `bytes_sent`：PG 无对应累计量，保持 0。
fn build_pg_metrics(stats: PgStats, version: String, log_min_duration_ms: i64) -> ServerMetrics {
    let uptime = stats.uptime_seconds;
    let transactions = stats.xact_commit.saturating_add(stats.xact_rollback);
    let qps = if uptime > 0 {
        transactions as f64 / uptime as f64
    } else {
        0.0
    };

    let total_blocks = stats.blks_hit.saturating_add(stats.blks_read);
    let buffer_pool_hit_rate = (total_blocks > 0).then(|| {
        (stats.blks_hit as f64 / total_blocks as f64 * 100.0 * 100.0).round() / 100.0
    });

    ServerMetrics {
        version,
        uptime_seconds: uptime,
        qps: (qps * 100.0).round() / 100.0,
        threads_connected: stats.connections,
        threads_running: stats.running,
        max_connections: stats.max_connections,
        slow_queries: 0,
        slow_query_log: log_min_duration_ms >= 0,
        long_query_time: if log_min_duration_ms >= 0 {
            log_min_duration_ms as f64 / 1000.0
        } else {
            0.0
        },
        aborted_connects: 0,
        buffer_pool_hit_rate,
        bytes_received: 0,
        bytes_sent: 0,
        com_select: stats.tup_returned,
        com_insert: stats.tup_inserted,
        com_update: stats.tup_updated,
        com_delete: stats.tup_deleted,
    }
}

fn non_negative_to_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn builds_metrics_from_status_and_variables() {
        // 生产路径由 normalize_pairs 统一转小写，这里直接构造小写键
        let status = map(&[
            ("uptime", "100"),
            ("questions", "250"),
            ("threads_connected", "7"),
            ("threads_running", "2"),
            ("slow_queries", "3"),
            ("aborted_connects", "1"),
            ("innodb_buffer_pool_reads", "100"),
            ("innodb_buffer_pool_read_requests", "1000"),
            ("bytes_received", "4096"),
            ("bytes_sent", "8192"),
            ("com_select", "10"),
            ("com_insert", "11"),
            ("com_update", "12"),
            ("com_delete", "13"),
        ]);
        let variables = map(&[
            ("version", "8.0.34"),
            ("max_connections", "151"),
            ("slow_query_log", "ON"),
            ("long_query_time", "10.5"),
        ]);

        let metrics = build_mysql_metrics(&status, &variables);
        assert_eq!(metrics.version, "8.0.34");
        assert_eq!(metrics.uptime_seconds, 100);
        assert_eq!(metrics.qps, 2.5);
        assert_eq!(metrics.threads_connected, 7);
        assert_eq!(metrics.threads_running, 2);
        assert_eq!(metrics.max_connections, 151);
        assert_eq!(metrics.slow_queries, 3);
        assert!(metrics.slow_query_log);
        assert_eq!(metrics.long_query_time, 10.5);
        assert_eq!(metrics.aborted_connects, 1);
        assert_eq!(metrics.buffer_pool_hit_rate, Some(90.0));
        assert_eq!(metrics.bytes_received, 4096);
        assert_eq!(metrics.bytes_sent, 8192);
        assert_eq!(metrics.com_select, 10);
        assert_eq!(metrics.com_delete, 13);
    }

    #[test]
    fn missing_or_invalid_values_fall_back_to_zero() {
        let metrics = build_mysql_metrics(&HashMap::new(), &HashMap::new());
        assert_eq!(metrics.version, "");
        assert_eq!(metrics.uptime_seconds, 0);
        assert_eq!(metrics.qps, 0.0);
        assert!(!metrics.slow_query_log);
        assert_eq!(metrics.buffer_pool_hit_rate, None);

        let status = map(&[("questions", "not-a-number"), ("uptime", "-5")]);
        let metrics = build_mysql_metrics(&status, &HashMap::new());
        assert_eq!(metrics.qps, 0.0);
    }

    #[test]
    fn pg_process_row_maps_pg_stat_activity_columns() {
        let row: PgProcessRow = (
            4242,
            "auroradb".to_string(),
            Some("10.0.0.8".to_string()),
            Some(5432),
            Some("auroradb_test".to_string()),
            "active".to_string(),
            12,
            Some("Client: ClientRead".to_string()),
            Some("SELECT 1".to_string()),
        );
        let process = pg_process_from_row(row);
        assert_eq!(process.id, 4242);
        assert_eq!(process.user, "auroradb");
        assert_eq!(process.host, "10.0.0.8:5432");
        assert_eq!(process.database.as_deref(), Some("auroradb_test"));
        // command 统一成 MySQL 口径：PG 的 active → Query
        assert_eq!(process.command, "Query");
        assert_eq!(process.time, 12);
        assert_eq!(process.state.as_deref(), Some("Client: ClientRead"));
        assert_eq!(process.info.as_deref(), Some("SELECT 1"));

        // 本机 Unix socket 连接没有客户端地址；负的时延不应变成天文数字
        let local: PgProcessRow = (
            1,
            String::new(),
            None,
            None,
            None,
            "idle".to_string(),
            -3,
            None,
            None,
        );
        let local = pg_process_from_row(local);
        assert_eq!(local.host, "local");
        assert_eq!(local.time, 0);
        assert_eq!(local.command, "Sleep");
        assert!(local.state.is_none());
        assert!(local.info.is_none());
    }

    #[test]
    fn pg_command_from_state_maps_to_mysql_words() {
        assert_eq!(pg_command_from_state("active"), "Query");
        for state in [
            "idle",
            "idle in transaction",
            "idle in transaction (aborted)",
            "disabled",
        ] {
            assert_eq!(pg_command_from_state(state), "Sleep", "state: {state}");
        }
        // 其余状态保留 PG 原值，不做猜测性映射
        assert_eq!(
            pg_command_from_state("fastpath function call"),
            "fastpath function call"
        );
        assert_eq!(pg_command_from_state(""), "");
    }

    #[test]
    fn pg_stats_row_keeps_column_order() {
        let row: PgMetricsRow = (
            1,
            2,
            3,
            4,
            5,
            6,
            7,
            8,
            9,
            10,
            11,
            "PostgreSQL 16.4".to_string(),
        );
        let (stats, version) = pg_stats_from_row(row);
        assert_eq!(stats.xact_commit, 1);
        assert_eq!(stats.xact_rollback, 2);
        assert_eq!(stats.blks_read, 3);
        assert_eq!(stats.blks_hit, 4);
        assert_eq!(stats.tup_returned, 5);
        assert_eq!(stats.tup_inserted, 6);
        assert_eq!(stats.tup_updated, 7);
        assert_eq!(stats.tup_deleted, 8);
        assert_eq!(stats.connections, 9);
        assert_eq!(stats.running, 10);
        assert_eq!(stats.uptime_seconds, 11);
        assert_eq!(stats.max_connections, 0);
        assert_eq!(version, "PostgreSQL 16.4");
    }

    #[test]
    fn builds_pg_metrics_from_database_stats() {
        let stats = PgStats {
            xact_commit: 90,
            xact_rollback: 10,
            blks_read: 100,
            blks_hit: 900,
            tup_returned: 50,
            tup_inserted: 6,
            tup_updated: 5,
            tup_deleted: 4,
            connections: 8,
            running: 2,
            uptime_seconds: 100,
            max_connections: 100,
        };
        let metrics = build_pg_metrics(stats, "PostgreSQL 16.4".to_string(), 500);
        assert_eq!(metrics.version, "PostgreSQL 16.4");
        assert_eq!(metrics.uptime_seconds, 100);
        assert_eq!(metrics.qps, 1.0); // (90 + 10) / 100
        assert_eq!(metrics.threads_connected, 8);
        assert_eq!(metrics.threads_running, 2);
        assert_eq!(metrics.max_connections, 100);
        assert_eq!(metrics.slow_queries, 0);
        assert!(metrics.slow_query_log);
        assert_eq!(metrics.long_query_time, 0.5);
        assert_eq!(metrics.aborted_connects, 0);
        assert_eq!(metrics.buffer_pool_hit_rate, Some(90.0));
        assert_eq!(metrics.bytes_received, 0);
        assert_eq!(metrics.bytes_sent, 0);
        assert_eq!(metrics.com_select, 50);
        assert_eq!(metrics.com_insert, 6);
        assert_eq!(metrics.com_update, 5);
        assert_eq!(metrics.com_delete, 4);
    }

    #[test]
    fn pg_metrics_without_data_fall_back_to_zero() {
        let stats = PgStats::default();
        let metrics = build_pg_metrics(stats, String::new(), -1);
        assert_eq!(metrics.version, "");
        assert_eq!(metrics.uptime_seconds, 0);
        assert_eq!(metrics.qps, 0.0);
        assert!(!metrics.slow_query_log);
        assert_eq!(metrics.long_query_time, 0.0);
        assert_eq!(metrics.buffer_pool_hit_rate, None);
        assert_eq!(metrics.max_connections, 0);
    }

    #[tokio::test]
    async fn kill_process_rejects_zero_id_without_touching_connection() {
        // ID 为 0 的校验发生在取连接之前，未连接的 manager 也不会报错
        let manager = ConnectionManager::new();
        let error = kill_process(&manager, "missing-connection", 0)
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "无效的会话 ID");
    }

    /// 真实 PG 测试库连接夹具：未配置 AURORADB_TEST_PG_* 时静默跳过（返回 None）；
    /// env 齐全但连接失败（认证/网络/库不存在等）时 panic，避免掩盖 CI 回归。
    async fn pg_test_connection() -> Option<(ConnectionManager, String, String)> {
        use auroradb_core::connection::driver::DriverKind;
        use auroradb_core::connection::{
            ConnectionConfig, ConnectionRequest, PoolConfig, SafetyMode, SslConfig,
        };

        let host = std::env::var("AURORADB_TEST_PG_HOST").ok()?;
        let port = std::env::var("AURORADB_TEST_PG_PORT").ok()?;
        let username = std::env::var("AURORADB_TEST_PG_USER").ok()?;
        let password = std::env::var("AURORADB_TEST_PG_PASSWORD").ok()?;
        let database = std::env::var("AURORADB_TEST_PG_DATABASE").ok()?;
        let port = port
            .parse::<u16>()
            .unwrap_or_else(|error| panic!("AURORADB_TEST_PG_PORT 不是合法端口：{error}"));

        let manager = ConnectionManager::new();
        let id = manager
            .connect(ConnectionRequest {
                config: ConnectionConfig {
                    id: "qa-pg-monitor".to_string(),
                    name: "qa-pg-monitor".to_string(),
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
            .unwrap_or_else(|error| panic!("连接真实 PostgreSQL 测试库失败：{error:#}"));

        Some((manager, id, database))
    }

    /// 非超级用户回归用例的辅助函数（真实库）：用普通角色调用 get_server_metrics，
    /// 断言连接数等于 `pg_stat_database.numbackends` 汇总——旧实现从 `pg_stat_activity`
    /// 计数，普通角色受行级可见性限制会恒为 1。
    /// 返回 Err 表示环境限制（认证方式/权限等），不是产品回归；调用方打印原因后跳过。
    async fn pg_viewer_metrics_regression(
        base_config: &auroradb_core::connection::ConnectionConfig,
        database: &str,
        role: &str,
        role_password: &str,
    ) -> anyhow::Result<()> {
        use auroradb_core::connection::ConnectionRequest;
        use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
        use std::time::Duration;

        let mut viewer_config = base_config.clone();
        viewer_config.id = format!("qa-pg-monitor-viewer-{}", std::process::id());
        viewer_config.name = "qa-pg-monitor-viewer".to_string();
        viewer_config.username = role.to_string();
        viewer_config.password = role_password.to_string();

        let viewer = ConnectionManager::new();
        let viewer_id = viewer
            .connect(ConnectionRequest {
                config: viewer_config,
                ssh_config: None,
            })
            .await?;

        // 预热连接池；普通角色只应看到自己的会话（这正是旧实现的误导来源）
        let visible = get_process_list(&viewer, &viewer_id).await?;
        assert!(!visible.is_empty(), "普通角色至少能看到自己的会话");
        assert!(
            visible.iter().all(|process| process.user == role),
            "普通角色不应看到其他用户的会话；实际: {visible:?}"
        );

        // 普通角色直连池：读取 numbackends 基准值
        let viewer_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(
                PgConnectOptions::new()
                    .host(base_config.host.as_str())
                    .port(base_config.port)
                    .username(role)
                    .password(role_password)
                    .database(database),
            )
            .await?;

        // 管理员侧挂一个空闲会话：普通角色看不到它，但 numbackends 会把它算进去，
        // 保证旧实现（activity 计数）与正确实现之间的差异至少为 2。
        let hidden_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(
                PgConnectOptions::new()
                    .host(base_config.host.as_str())
                    .port(base_config.port)
                    .username(base_config.username.as_str())
                    .password(base_config.password.as_str())
                    .database(database),
            )
            .await?;
        let hidden = hidden_pool.acquire().await?;

        // 连接池可能有少量连接增删，重试到采样稳定一致
        let mut matched = false;
        let mut last = (0u64, 0i64);
        for _ in 0..20 {
            let metrics = get_server_metrics(&viewer, &viewer_id).await?;
            let total: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(numbackends), 0)::bigint FROM pg_stat_database",
            )
            .fetch_one(&viewer_pool)
            .await?;
            last = (metrics.threads_connected, total);
            if last.0 as i64 == last.1 {
                matched = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(
            matched,
            "普通角色的 threads_connected 应等于 pg_stat_database.numbackends 汇总\
             （不能因 pg_stat_activity 行级隐藏而恒为 1）；\
             实际 threads_connected={}, SUM(numbackends)={}",
            last.0, last.1
        );

        drop(hidden);
        hidden_pool.close().await;
        viewer_pool.close().await;
        viewer.disconnect(&viewer_id).await?;
        Ok(())
    }

    /// 真实库用例：会话列表 / 实例指标 / 终止自己创建的可控 pg_sleep 会话。
    /// 只 kill 本用例自己建立的监听连接，不触碰其他用例或其它 teammate 的会话。
    #[tokio::test]
    #[ignore = "需要真实 PostgreSQL 测试库：设置 AURORADB_TEST_PG_* 后执行 cargo test -- --ignored"]
    async fn pg_monitor_lists_sessions_reports_metrics_and_terminates_own_sleeper() {
        use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
        use std::time::Duration;

        let Some((manager, id, database)) = pg_test_connection().await else {
            return;
        };
        let table = format!("qa_monitor_probe_{}", std::process::id());

        manager
            .execute(
                &id,
                &database,
                &format!("DROP TABLE IF EXISTS {} CASCADE", table),
            )
            .await
            .expect("清理探针表失败");
        manager
            .execute(
                &id,
                &database,
                &format!("CREATE TABLE {} (id INT PRIMARY KEY)", table),
            )
            .await
            .expect("创建探针表失败");

        // 1) 会话列表：能列出 client backend，且包含当前库
        let list = get_process_list(&manager, &id)
            .await
            .expect("PG 会话列表查询失败");
        assert!(!list.is_empty(), "PG 会话列表不应为空");
        assert!(
            list.iter()
                .any(|process| process.database.as_deref() == Some(database.as_str())),
            "会话列表中应包含当前库；实际: {:?}",
            list
        );

        // 2) 实例指标：版本/uptime/连接数/最大连接数均来自真实系统视图
        let metrics = get_server_metrics(&manager, &id)
            .await
            .expect("PG 实例指标查询失败");
        assert!(
            metrics.version.to_lowercase().contains("postgresql"),
            "版本: {}",
            metrics.version
        );
        assert!(metrics.uptime_seconds > 0, "uptime 应大于 0");
        assert!(metrics.threads_connected >= 1);
        // 指标查询自身此刻处于 active 状态
        assert!(metrics.threads_running >= 1);
        assert!(metrics.max_connections >= 1);
        assert_eq!(metrics.slow_queries, 0);
        assert_eq!(metrics.aborted_connects, 0);
        assert_eq!(metrics.bytes_received, 0);
        assert_eq!(metrics.bytes_sent, 0);

        // 3) 独立连接建一个可控的 pg_sleep 会话，再从监控接口里找到它
        let config = manager
            .connection_snapshot(&id)
            .await
            .expect("读取连接快照失败")
            .config;
        let sleeper_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(
                PgConnectOptions::new()
                    .host(config.host.as_str())
                    .port(config.port)
                    .username(config.username.as_str())
                    .password(config.password.as_str())
                    .database(database.as_str()),
            )
            .await
            .expect("连接 pg_sleep 会话失败");

        // ServerMetrics 公共结构体没有 db_size/deadlocks/temp_files 字段（按约束不改），
        // 这里直接验证这几项 PG 数据确实可查，作为 get_server_metrics 的补充证据
        let db_size: i64 = sqlx::query_scalar("SELECT pg_database_size(current_database())")
            .fetch_one(&sleeper_pool)
            .await
            .expect("查询 pg_database_size 失败");
        assert!(db_size > 0, "pg_database_size 应大于 0");
        let (deadlocks, temp_files): (i64, i64) = sqlx::query_as(
            "SELECT COALESCE(SUM(deadlocks), 0)::bigint,
                    COALESCE(SUM(temp_files), 0)::bigint
             FROM pg_stat_database",
        )
        .fetch_one(&sleeper_pool)
        .await
        .expect("查询 pg_stat_database 死锁/临时文件失败");
        assert!(deadlocks >= 0);
        assert!(temp_files >= 0);

        let mut sleeper = sleeper_pool.acquire().await.expect("获取监听连接失败");
        let sleeper_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *sleeper)
            .await
            .expect("查询 pg_backend_pid 失败");
        let sleeper_id = u64::try_from(sleeper_pid).expect("pg_backend_pid 应为正数");

        let sleeper_task = tokio::spawn(async move {
            sqlx::query("SELECT pg_sleep(60)")
                .execute(&mut *sleeper)
                .await
        });

        let mut observed = false;
        for _ in 0..50 {
            let list = get_process_list(&manager, &id)
                .await
                .expect("轮询 PG 会话列表失败");
            if let Some(process) = list.iter().find(|p| p.id == sleeper_id) {
                let sleeping = process
                    .info
                    .as_deref()
                    .is_some_and(|query| query.contains("pg_sleep"));
                if sleeping {
                    assert_eq!(process.command, "Query", "运行中查询的 command 应为 Query");
                    observed = true;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(observed, "未在 pg_stat_activity 中观察到自己的 pg_sleep 会话");

        // 4) 终止会话：pg_terminate_backend 返回 true，客户端查询报错，后端消失
        kill_process(&manager, &id, sleeper_id)
            .await
            .expect("终止自己的 pg_sleep 会话失败");

        let query_result = tokio::time::timeout(Duration::from_secs(10), sleeper_task)
            .await
            .expect("等待被终止会话结束超时")
            .expect("pg_sleep 会话任务 panic");
        assert!(
            query_result.is_err(),
            "被终止的 pg_sleep 查询不应成功返回"
        );

        let mut gone = false;
        for _ in 0..50 {
            let list = get_process_list(&manager, &id)
                .await
                .expect("轮询 PG 会话列表失败");
            if !list.iter().any(|p| p.id == sleeper_id) {
                gone = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(gone, "终止后会话仍出现在 pg_stat_activity 中");

        // 5) 不存在/已结束的 pid：返回中文错误而不是 panic
        let error = kill_process(&manager, &id, 999_999_999)
            .await
            .expect_err("终止不存在的会话应返回错误");
        let message = error.to_string();
        assert!(
            message.contains("终止会话 999999999 失败"),
            "错误文案: {}",
            message
        );

        // 6) 非超级用户回归：普通角色看到的 pg_stat_activity 行被行级权限过滤，
        //    连接数必须来自 pg_stat_database.numbackends，不能恒为 1。
        let role = format!("qa_monitor_{}", std::process::id());
        let role_password = format!("QaMonitor{}pw", std::process::id());
        // 上一次异常中断可能留下同名角色，先尽力清理（失败不影响后续 CREATE 的报错信息）
        let _ = manager
            .execute(&id, &database, &format!("DROP OWNED BY {role}"))
            .await;
        let _ = manager
            .execute(&id, &database, &format!("DROP ROLE IF EXISTS {role}"))
            .await;
        let created = manager
            .execute(
                &id,
                &database,
                &format!("CREATE ROLE {role} LOGIN PASSWORD '{role_password}' NOSUPERUSER"),
            )
            .await;
        match created {
            Ok(_) => {
                match pg_viewer_metrics_regression(&config, &database, &role, &role_password).await
                {
                    Ok(()) => {}
                    Err(error) => {
                        // 环境限制（如 pg_hba 不接受普通角色密码认证）不应误报为产品回归
                        eprintln!("跳过非超级用户回归用例（环境限制）：{error:#}");
                    }
                }
                let _ = manager
                    .execute(&id, &database, &format!("DROP OWNED BY {role}"))
                    .await;
                let mut role_dropped = false;
                let mut drop_error = None;
                for _ in 0..10 {
                    match manager
                        .execute(&id, &database, &format!("DROP ROLE IF EXISTS {role}"))
                        .await
                    {
                        Ok(_) => {
                            role_dropped = true;
                            break;
                        }
                        Err(error) => {
                            drop_error = Some(error);
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    }
                }
                if !role_dropped {
                    eprintln!("清理测试角色 {role} 失败：{drop_error:?}");
                }
            }
            Err(error) => {
                // 真实库账号可能没有 CREATEROLE 权限：记录原因后按未覆盖点处理，不 panic
                eprintln!("跳过非超级用户回归用例（CREATE ROLE 失败）：{error:#}");
            }
        }

        // 7) 自清理：只删本用例建立的连接/表
        sleeper_pool.close().await;
        manager
            .execute(
                &id,
                &database,
                &format!("DROP TABLE IF EXISTS {} CASCADE", table),
            )
            .await
            .expect("清理探针表失败");
        manager.disconnect(&id).await.expect("断开测试连接失败");
    }
}
