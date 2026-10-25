//! 审计日志
//! 记录每一次 SQL 执行、表数据保存与导入操作，按天追加写入 JSONL 文件，
//! 供事后追溯「谁 / 何时 / 哪个连接 / 什么 SQL / 影响行数」以及合规导出。
//!
//! 文件位置：`<应用数据目录>/mysql-client/audit/audit-YYYY-MM-DD.jsonl`。
//! 单条日志一行 JSON，读取时按文件从新到旧、行从后到前扫描，天然得到倒序时间线。

use anyhow::Result;
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::Mutex;

/// SQL 文本落盘上限：超出部分截断，避免超大脚本把日志文件撑爆
const MAX_SQL_CHARS: usize = 8_000;

/// 单次查询返回条数上限：防止一次 IPC 搬运过多日志
const MAX_LIST_LIMIT: usize = 1_000;

/// 单次查询最多扫描的日志条数：限制历史文件很多时的磁盘读取成本
const MAX_SCAN_ENTRIES: usize = 20_000;

/// 进程内自增序号，配合时间戳生成日志 ID（避免额外引入 uuid 依赖）
static AUDIT_SEQ: AtomicU64 = AtomicU64::new(0);

/// 一条审计记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: String,
    pub timestamp: String,
    pub connection_id: String,
    pub connection_name: String,
    /// 连接目标，形如 `127.0.0.1:3306`
    pub host: String,
    pub database: String,
    /// 操作类型：query（SQL 执行）、table-edit（表数据保存）、import（数据导入）
    pub kind: String,
    pub sql: String,
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub duration_ms: u64,
    pub rows_affected: Option<u64>,
    pub rows_returned: usize,
}

impl AuditEntry {
    /// 构造一条待写入的审计记录；id/timestamp 由 `AuditLog::record` 填充
    pub fn new(
        kind: &str,
        connection_id: &str,
        connection_name: &str,
        host: &str,
        database: &str,
        sql: &str,
    ) -> Self {
        Self {
            id: String::new(),
            timestamp: String::new(),
            connection_id: connection_id.to_string(),
            connection_name: connection_name.to_string(),
            host: host.to_string(),
            database: database.to_string(),
            kind: kind.to_string(),
            sql: sql.to_string(),
            success: true,
            error: None,
            duration_ms: 0,
            rows_affected: None,
            rows_returned: 0,
        }
    }
}

/// 审计日志存储
pub struct AuditLog {
    dir: PathBuf,
    /// 串行化追加写入，避免并发执行时多行交错
    write_lock: Mutex<()>,
}

impl AuditLog {
    /// 使用默认目录 `<应用数据目录>/mysql-client/audit`
    pub fn new() -> Self {
        let dir = dirs::data_dir()
            .map(|dir| dir.join("mysql-client").join("audit"))
            .unwrap_or_else(|| PathBuf::from("audit"));
        Self::new_in(dir)
    }

    pub fn new_in(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            write_lock: Mutex::new(()),
        }
    }

    /// 写入一条审计记录；失败只记警告，不影响业务命令结果
    pub async fn record(&self, mut entry: AuditEntry) {
        entry.timestamp = Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string();
        entry.id = format!(
            "{}-{}",
            Local::now().format("%Y%m%d%H%M%S%f"),
            AUDIT_SEQ.fetch_add(1, Ordering::Relaxed)
        );
        entry.sql = truncate_chars(&entry.sql, MAX_SQL_CHARS);
        if let Some(error) = entry.error.take() {
            entry.error = Some(truncate_chars(&error, 2_000));
        }
        if let Err(e) = self.append(&entry).await {
            tracing::warn!("写入审计日志失败: {}", e);
        }
    }

    async fn append(&self, entry: &AuditEntry) -> Result<()> {
        let line = serde_json::to_string(entry)?;
        let path = self
            .dir
            .join(format!("audit-{}.jsonl", Local::now().format("%Y-%m-%d")));

        let _guard = self.write_lock.lock().await;
        tokio::fs::create_dir_all(&self.dir).await?;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        use tokio::io::AsyncWriteExt;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
        Ok(())
    }

    /// 按时间倒序读取审计日志
    ///
    /// `connection_id` / `success` / `keyword` 为空时不做该维度过滤；
    /// `keyword` 对 SQL、连接名、库名与错误信息做不区分大小写的包含匹配。
    pub async fn list(
        &self,
        limit: usize,
        connection_id: Option<&str>,
        success: Option<bool>,
        keyword: Option<&str>,
    ) -> Result<Vec<AuditEntry>> {
        let limit = limit.clamp(1, MAX_LIST_LIMIT);
        let keyword = keyword
            .map(|k| k.trim().to_lowercase())
            .filter(|k| !k.is_empty());

        let mut files = match tokio::fs::read_dir(&self.dir).await {
            Ok(mut dir) => {
                let mut names = Vec::new();
                while let Some(entry) = dir.next_entry().await? {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.starts_with("audit-") && name.ends_with(".jsonl") {
                        names.push(name);
                    }
                }
                names.sort_unstable();
                names.reverse();
                names
            }
            // 尚未产生任何日志
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };

        let mut result = Vec::new();
        let mut scanned = 0usize;
        for name in files.drain(..) {
            if result.len() >= limit || scanned >= MAX_SCAN_ENTRIES {
                break;
            }
            let content = match tokio::fs::read_to_string(self.dir.join(&name)).await {
                Ok(content) => content,
                Err(e) => {
                    tracing::warn!("读取审计日志 {} 失败: {}", name, e);
                    continue;
                }
            };
            // 文件按时间追加，倒序读取即新→旧
            for line in content.lines().rev() {
                if result.len() >= limit || scanned >= MAX_SCAN_ENTRIES {
                    break;
                }
                scanned += 1;
                let Ok(entry) = serde_json::from_str::<AuditEntry>(line) else {
                    continue;
                };
                if matches(&entry, connection_id, success, keyword.as_deref()) {
                    result.push(entry);
                }
            }
        }

        // 跨天文件可能因时钟回拨/写入顺序略有偏差，统一按时间戳兜底排序
        result.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        Ok(result)
    }

    /// 删除全部审计日志文件，返回删除的文件数
    pub async fn clear(&self) -> Result<usize> {
        let mut dir = match tokio::fs::read_dir(&self.dir).await {
            Ok(dir) => dir,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e.into()),
        };

        let mut removed = 0usize;
        while let Some(entry) = dir.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("audit-") && name.ends_with(".jsonl") {
                tokio::fs::remove_file(entry.path()).await?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::new()
    }
}

fn matches(
    entry: &AuditEntry,
    connection_id: Option<&str>,
    success: Option<bool>,
    keyword: Option<&str>,
) -> bool {
    if let Some(id) = connection_id {
        if !id.is_empty() && entry.connection_id != id {
            return false;
        }
    }
    if let Some(expected) = success {
        if entry.success != expected {
            return false;
        }
    }
    if let Some(keyword) = keyword {
        let sql = entry.sql.to_lowercase();
        let name = entry.connection_name.to_lowercase();
        let database = entry.database.to_lowercase();
        let error = entry.error.as_deref().unwrap_or("").to_lowercase();
        if !sql.contains(keyword)
            && !name.contains(keyword)
            && !database.contains(keyword)
            && !error.contains(keyword)
        {
            return false;
        }
    }
    true
}

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut truncated: String = text.chars().take(max).collect();
    truncated.push_str("\n…（已截断）");
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "auroradb-audit-{}-{}-{}",
            tag,
            std::process::id(),
            AUDIT_SEQ.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn entry(sql: &str, database: &str) -> AuditEntry {
        AuditEntry::new("query", "c1", "本地", "127.0.0.1:3306", database, sql)
    }

    #[tokio::test]
    async fn record_then_list_returns_newest_first() {
        let dir = temp_dir("order");
        let log = AuditLog::new_in(&dir);

        log.record(entry("SELECT 1", "app")).await;
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        log.record(entry("SELECT 2", "app")).await;

        let items = log.list(10, None, None, None).await.unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].sql, "SELECT 2");
        assert_eq!(items[1].sql, "SELECT 1");
        assert!(items.iter().all(|item| !item.id.is_empty() && !item.timestamp.is_empty()));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn list_applies_filters() {
        let dir = temp_dir("filter");
        let log = AuditLog::new_in(&dir);

        let mut failure = entry("DELETE FROM users", "app");
        failure.success = false;
        failure.error = Some("permission denied".to_string());

        log.record(entry("SELECT * FROM orders", "shop")).await;
        log.record(failure).await;

        assert_eq!(log.list(10, None, Some(true), None).await.unwrap().len(), 1);
        assert_eq!(log.list(10, None, Some(false), None).await.unwrap().len(), 1);
        assert_eq!(log.list(10, Some("c1"), None, None).await.unwrap().len(), 2);
        assert_eq!(log.list(10, Some("missing"), None, None).await.unwrap().len(), 0);
        assert_eq!(log.list(10, None, None, Some("orders")).await.unwrap().len(), 1);
        assert_eq!(log.list(10, None, None, Some("PERMISSION")).await.unwrap().len(), 1);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn long_sql_is_truncated() {
        let dir = temp_dir("truncate");
        let log = AuditLog::new_in(&dir);

        let long_sql = "x".repeat(MAX_SQL_CHARS + 100);
        log.record(entry(&long_sql, "app")).await;

        let items = log.list(1, None, None, None).await.unwrap();
        assert!(items[0].sql.ends_with("…（已截断）"));
        assert!(items[0].sql.chars().count() <= MAX_SQL_CHARS + 10);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn clear_removes_log_files() {
        let dir = temp_dir("clear");
        let log = AuditLog::new_in(&dir);

        log.record(entry("SELECT 1", "app")).await;
        assert_eq!(log.clear().await.unwrap(), 1);
        assert!(log.list(10, None, None, None).await.unwrap().is_empty());
        // 目录不存在时清空视为成功
        assert_eq!(log.clear().await.unwrap(), 0);

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[test]
    fn truncate_keeps_short_text() {
        assert_eq!(truncate_chars("SELECT 1", 100), "SELECT 1");
    }
}
