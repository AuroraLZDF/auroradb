//! 数据库完整备份与还原（专业版）。
//!
//! 复用核心 crate 的外部客户端调用能力（`mysqldump` / `mysql`），
//! 这里只负责从连接管理器取连接快照并调度。

use anyhow::{anyhow, Result};
use auroradb_core::connection::driver::DriverKind;
use auroradb_core::connection::transfer::{
    run_mysql_script_file, run_mysqldump_to_file, ImportReporter,
};
use auroradb_core::connection::ConnectionManager;
use std::path::PathBuf;

/// 触发完整备份：根据驱动调用 mysqldump / pg_dump，写入用户选择的路径。
pub async fn backup_database(
    manager: &ConnectionManager,
    connection_id: &str,
    database: &str,
    output_path: &str,
) -> Result<u64> {
    let snapshot = manager.connection_snapshot(connection_id).await?;

    if snapshot.config.driver == DriverKind::Postgres {
        tracing::warn!("PostgreSQL 备份还原尚未实现，跳过 pg_dump 调用");
        return Err(anyhow!(
            "PostgreSQL 完整备份将在后续版本提供，请先使用导出 SQL 功能"
        ));
    }

    // SSH 隧道存在时走本地转发端口
    let (host, port) = match snapshot.tunnel_local_port {
        Some(local_port) => ("127.0.0.1".to_string(), local_port),
        None => (snapshot.config.host.clone(), snapshot.config.port),
    };

    // 确保输出目录存在（用户可能在对话框中输入了尚不存在的目录）
    let output_path = PathBuf::from(output_path);
    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).ok();
        }
    }

    run_mysqldump_to_file(
        &host,
        port,
        database,
        &snapshot.config.username,
        &snapshot.config.password,
        &snapshot.ssl_config,
        &output_path,
    )
    .await
}

/// 从 dump 文件还原数据库（MySQL 先用 stdin 管道实现）
///
/// 传入 `reporter` 时推送字节进度与逐条语句流水，前端据此展示还原进度。
pub async fn restore_database(
    manager: &ConnectionManager,
    connection_id: &str,
    database: &str,
    dump_path: &str,
    reporter: Option<ImportReporter>,
) -> Result<()> {
    let snapshot = manager.connection_snapshot(connection_id).await?;

    if snapshot.config.driver == DriverKind::Postgres {
        return Err(anyhow!("PostgreSQL 还原将在后续版本提供"));
    }

    let (host, port) = match snapshot.tunnel_local_port {
        Some(local_port) => ("127.0.0.1".to_string(), local_port),
        None => (snapshot.config.host.clone(), snapshot.config.port),
    };

    let path = PathBuf::from(dump_path);
    if !path.is_file() {
        return Err(anyhow!("备份文件不存在：{}", dump_path));
    }

    run_mysql_script_file(
        &host,
        port,
        database,
        &snapshot.config.username,
        &snapshot.config.password,
        &snapshot.ssl_config,
        &path,
        "还原",
        reporter,
    )
    .await?;
    // 还原脚本可能重建表结构，清空缓存让刷新能拿到最新列表
    manager.invalidate_metadata_cache(connection_id).await;
    Ok(())
}
