//! 审计日志查询与清理命令（专业版功能）

use auroradb_core::audit::{AuditEntry, AuditLog};
use tauri::{command, State};

/// 按时间倒序读取审计日志；空条件表示不过滤
#[command]
pub async fn list_audit_entries(
    audit: State<'_, AuditLog>,
    limit: Option<usize>,
    connection_id: Option<String>,
    success: Option<bool>,
    keyword: Option<String>,
) -> Result<Vec<AuditEntry>, String> {
    audit
        .list(
            limit.unwrap_or(200),
            connection_id.as_deref(),
            success,
            keyword.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())
}

/// 清空全部审计日志，返回删除的文件数
#[command]
pub async fn clear_audit_logs(
    audit: State<'_, AuditLog>,
) -> Result<usize, String> {
    audit.clear().await.map_err(|e| e.to_string())
}
