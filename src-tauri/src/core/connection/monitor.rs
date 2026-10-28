//! 会话与实例监控
//! `SHOW FULL PROCESSLIST` 进程列表、`KILL CONNECTION` 终止会话，
//! 以及基于 `SHOW GLOBAL STATUS` / `SHOW GLOBAL VARIABLES` 的服务端指标。

use super::manager::ConnectionManager;
use super::metadata::with_metadata_timeout;
use super::types::*;
use anyhow::{anyhow, Result};
use sqlx::mysql::MySqlRow;
use sqlx::{MySqlPool, Row};
use std::collections::HashMap;

impl ConnectionManager {
    /// 读取服务端全部会话（需要 PROCESS 权限，否则只能看到当前用户的会话）
    pub async fn get_process_list(&self, connection_id: &str) -> Result<Vec<ProcessInfo>> {
        let pool = self.pool_of(connection_id, "进程监控").await?;

        let rows: Vec<MySqlRow> =
            with_metadata_timeout(sqlx::query("SHOW FULL PROCESSLIST").fetch_all(&pool)).await?;

        Ok(rows.iter().map(process_from_row).collect())
    }

    /// 终止指定会话（`KILL CONNECTION <id>`）；process_id 为 u64，天然避免注入
    pub async fn kill_process(&self, connection_id: &str, process_id: u64) -> Result<()> {
        if process_id == 0 {
            return Err(anyhow!("无效的会话 ID"));
        }

        let pool = self.pool_of(connection_id, "进程监控").await?;

        with_metadata_timeout(
            sqlx::query(&format!("KILL CONNECTION {}", process_id)).execute(&pool),
        )
        .await?;
        Ok(())
    }

    /// 读取服务端版本、连接数、QPS、慢查询与 InnoDB 缓冲池命中率等指标
    pub async fn get_server_metrics(&self, connection_id: &str) -> Result<ServerMetrics> {
        let pool = self.pool_of(connection_id, "服务监控").await?;

        let status_rows: Vec<(String, Option<String>)> = with_metadata_timeout(
            sqlx::query_as("SHOW GLOBAL STATUS").fetch_all(&pool),
        )
        .await?;
        let variable_rows: Vec<(String, Option<String>)> = with_metadata_timeout(
            sqlx::query_as(
                "SHOW GLOBAL VARIABLES WHERE Variable_name IN
                 ('version', 'max_connections', 'slow_query_log', 'long_query_time')",
            )
            .fetch_all(&pool),
        )
        .await?;

        let status = normalize_pairs(status_rows);
        let variables = normalize_pairs(variable_rows);
        Ok(build_metrics(&status, &variables))
    }

    /// 取出连接池后立刻释放连接表读锁，避免监控查询期间阻塞心跳等写操作
    async fn pool_of(&self, connection_id: &str, feature: &str) -> Result<MySqlPool> {
        let connections = self.connections.read().await;
        let conn = connections
            .get(connection_id)
            .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;
        conn.mysql_pool(feature)
    }
}

/// 把 `SHOW ...` 的 (名称, 值) 行转成小写键的 map，忽略 NULL 值
fn normalize_pairs(rows: Vec<(String, Option<String>)>) -> HashMap<String, String> {
    rows.into_iter()
        .filter_map(|(name, value)| value.map(|v| (name.to_lowercase(), v)))
        .collect()
}

/// `SHOW FULL PROCESSLIST` 走文本协议，sqlx 只填 `columns` 不填 `column_names`，
/// 因此这里按列序号取值（顺序固定为 Id/User/Host/db/Command/Time/State/Info）。
fn process_from_row(row: &MySqlRow) -> ProcessInfo {
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

/// 由状态/变量 map 组装指标；拆分出来便于单测（不依赖真实数据库）
fn build_metrics(status: &HashMap<String, String>, variables: &HashMap<String, String>) -> ServerMetrics {
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

        let metrics = build_metrics(&status, &variables);
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
        let metrics = build_metrics(&HashMap::new(), &HashMap::new());
        assert_eq!(metrics.version, "");
        assert_eq!(metrics.uptime_seconds, 0);
        assert_eq!(metrics.qps, 0.0);
        assert!(!metrics.slow_query_log);
        assert_eq!(metrics.buffer_pool_hit_rate, None);

        let status = map(&[("questions", "not-a-number"), ("uptime", "-5")]);
        let metrics = build_metrics(&status, &HashMap::new());
        assert_eq!(metrics.qps, 0.0);
    }
}
