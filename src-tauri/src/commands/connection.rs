use crate::core::audit::{AuditEntry, AuditLog};
use crate::core::connection::{
    ConnectionConfig, ConnectionManager, ConnectionRequest, ConnectionStatus, DataQueryRequest,
    ErDiagramData, ExplainPlan, SshConfig,
};
use crate::core::license::{
    LicenseManager, FEATURE_ER_DIAGRAM, FEATURE_IMPORT_EXPORT, FEATURE_MONITOR, FEATURE_QUERY_PLAN,
    FEATURE_SCHEMA_DIFF, FEATURE_SSH_TUNNEL,
};
use tauri::{command, State};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

pub struct ConnectionManagerState {
    pub manager: Arc<RwLock<ConnectionManager>>,
}

impl ConnectionManagerState {
    pub fn new() -> Self {
        Self {
            manager: Arc::new(RwLock::new(ConnectionManager::new())),
        }
    }
}

/// 统一错误出口：驱动错误翻译为中文可读提示，其余错误保留原信息
fn fail(e: &anyhow::Error) -> String {
    crate::core::error::user_message(e)
}

/// 审计用连接信息：连接已断开时退化为连接 ID，host 留空
async fn connection_meta(state: &ConnectionManagerState, connection_id: &str) -> (String, String) {
    let manager = state.manager.read().await;
    match manager.connection_meta(connection_id).await {
        Some((name, host, port)) => (name, format!("{}:{}", host, port)),
        None => (connection_id.to_string(), String::new()),
    }
}

/// 执行 SQL 并写入审计日志（成功与失败都记录）
async fn execute_with_audit(
    state: &ConnectionManagerState,
    audit: &AuditLog,
    connection_id: &str,
    database: &str,
    sql: &str,
) -> Result<crate::core::connection::QueryResult, String> {
    let (name, host) = connection_meta(state, connection_id).await;
    let started = Instant::now();
    let outcome = {
        let manager = state.manager.read().await;
        manager.execute(connection_id, database, sql).await
    };

    let mut entry = AuditEntry::new("query", connection_id, &name, &host, database, sql);
    entry.duration_ms = started.elapsed().as_millis() as u64;
    match outcome {
        Ok(result) => {
            entry.rows_affected = result.rows_affected;
            entry.rows_returned = result.rows_returned;
            audit.record(entry).await;
            Ok(result)
        }
        Err(e) => {
            entry.success = false;
            entry.error = Some(e.to_string());
            audit.record(entry).await;
            Err(fail(&e))
        }
    }
}

/// 建立连接
///
/// `ssh_config` 非空时先建立 SSH 隧道，`config.host/port` 视为 SSH 服务器侧的数据库地址。
#[command]
pub async fn connect(
    state: State<'_, ConnectionManagerState>,
    license: State<'_, LicenseManager>,
    config: ConnectionConfig,
    ssh_config: Option<SshConfig>,
) -> Result<ConnectionStatus, String> {
    // SSH 隧道属于专业版能力；直连不受限制
    if ssh_config.is_some() {
        license.ensure(FEATURE_SSH_TUNNEL, "SSH 隧道").await?;
    }
    let manager = state.manager.read().await;

    let request = ConnectionRequest {
        config,
        ssh_config,
    };

    match manager.connect(request).await {
        Ok(id) => {
            // 获取连接状态
            match manager.get_status(&id).await {
                Ok(status) => Ok(status),
                Err(e) => Err(fail(&e)),
            }
        }
        Err(e) => Err(fail(&e)),
    }
}

/// 断开连接
#[command]
pub async fn disconnect(
    state: State<'_, ConnectionManagerState>,
    connection_id: String,
) -> Result<(), String> {
    let manager = state.manager.read().await;

    match manager.disconnect(&connection_id).await {
        Ok(_) => Ok(()),
        Err(e) => Err(fail(&e)),
    }
}

/// 获取连接状态
#[command]
pub async fn get_connection_status(
    state: State<'_, ConnectionManagerState>,
    connection_id: String,
) -> Result<ConnectionStatus, String> {
    let manager = state.manager.read().await;

    match manager.get_status(&connection_id).await {
        Ok(status) => Ok(status),
        Err(e) => Err(fail(&e)),
    }
}

/// 执行SQL
#[command]
pub async fn execute_sql(
    state: State<'_, ConnectionManagerState>,
    audit: State<'_, AuditLog>,
    connection_id: String,
    database: String,
    sql: String,
) -> Result<crate::core::connection::QueryResult, String> {
    execute_with_audit(&state, &audit, &connection_id, &database, &sql).await
}

/// 在单个事务中批量应用表数据变更（任一条失败即整体回滚）
#[command]
pub async fn apply_table_changes(
    state: State<'_, ConnectionManagerState>,
    audit: State<'_, AuditLog>,
    connection_id: String,
    database: String,
    changes: Vec<crate::core::connection::TableChange>,
) -> Result<crate::core::connection::ApplyChangesResult, String> {
    // 审计记录聚合后的 SQL 文本；changes 随后会移交给执行层
    let sql_summary = changes
        .iter()
        .map(|change| change.sql.as_str())
        .collect::<Vec<_>>()
        .join(";\n");

    let (name, host) = connection_meta(&state, &connection_id).await;
    let started = Instant::now();
    let outcome = {
        let manager = state.manager.read().await;
        manager
            .apply_changes(&connection_id, &database, changes)
            .await
    };

    let mut entry = AuditEntry::new(
        "table-edit",
        &connection_id,
        &name,
        &host,
        &database,
        &sql_summary,
    );
    entry.duration_ms = started.elapsed().as_millis() as u64;
    match outcome {
        Ok(result) => {
            entry.rows_affected = Some(result.applied as u64);
            audit.record(entry).await;
            Ok(result)
        }
        Err(e) => {
            entry.success = false;
            entry.error = Some(e.to_string());
            audit.record(entry).await;
            Err(fail(&e))
        }
    }
}

/// 查询表数据
#[command]
pub async fn query_table_data(
    state: State<'_, ConnectionManagerState>,
    request: DataQueryRequest,
) -> Result<crate::core::connection::DataQueryResponse, String> {
    let manager = state.manager.read().await;

    let conn_id = request.connection_id.clone();
    let db = request.database.clone();
    let table = request.table.clone();

    match manager.query_table(
        &conn_id,
        &db,
        &table,
        request,
    ).await {
        Ok(result) => Ok(result),
        Err(e) => Err(fail(&e)),
    }
}

/// 获取对象树
#[command]
pub async fn get_object_tree(
    state: State<'_, ConnectionManagerState>,
    connection_id: String,
    database: Option<String>,
) -> Result<Vec<crate::core::connection::DatabaseObject>, String> {
    let manager = state.manager.read().await;

    match manager.get_object_tree(&connection_id, database.as_deref()).await {
        Ok(objects) => Ok(objects),
        Err(e) => Err(fail(&e)),
    }
}

/// 获取表结构
#[command]
pub async fn get_table_structure(
    state: State<'_, ConnectionManagerState>,
    connection_id: String,
    database: String,
    table: String,
) -> Result<crate::core::connection::TableStructure, String> {
    let manager = state.manager.read().await;

    match manager.get_table_structure(&connection_id, &database, &table).await {
        Ok(structure) => Ok(structure),
        Err(e) => Err(fail(&e)),
    }
}

/// 获取 schema 快照（表/视图 + 列），供 SQL 编辑器补全
#[command]
pub async fn get_schema_snapshot(
    state: State<'_, ConnectionManagerState>,
    connection_id: String,
    database: String,
) -> Result<crate::core::connection::SchemaSnapshot, String> {
    let manager = state.manager.read().await;

    match manager.get_schema_snapshot(&connection_id, &database).await {
        Ok(snapshot) => Ok(snapshot),
        Err(e) => Err(fail(&e)),
    }
}

/// 对比两个库的表结构，返回差异与迁移脚本
#[command]
pub async fn compare_schemas(
    state: State<'_, ConnectionManagerState>,
    license: State<'_, LicenseManager>,
    source_connection_id: String,
    source_database: String,
    target_connection_id: String,
    target_database: String,
) -> Result<crate::core::connection::SchemaDiff, String> {
    license.ensure(FEATURE_SCHEMA_DIFF, "结构对比").await?;
    let manager = state.manager.read().await;

    match manager
        .compare_schemas(
            &source_connection_id,
            &source_database,
            &target_connection_id,
            &target_database,
        )
        .await
    {
        Ok(diff) => Ok(diff),
        Err(e) => Err(fail(&e)),
    }
}

/// 获取数据库信息
#[command]
pub async fn get_database_info(
    state: State<'_, ConnectionManagerState>,
    connection_id: String,
    database: String,
) -> Result<crate::core::connection::DatabaseInfo, String> {
    let manager = state.manager.read().await;

    match manager.get_database_info(&connection_id, &database).await {
        Ok(info) => Ok(info),
        Err(e) => Err(fail(&e)),
    }
}

/// 开始事务
#[command]
pub async fn begin_transaction(
    state: State<'_, ConnectionManagerState>,
    connection_id: String,
) -> Result<(), String> {
    let manager = state.manager.read().await;

    match manager.begin_transaction(&connection_id).await {
        Ok(_) => Ok(()),
        Err(e) => Err(fail(&e)),
    }
}

/// 提交事务
#[command]
pub async fn commit_transaction(
    state: State<'_, ConnectionManagerState>,
    connection_id: String,
) -> Result<(), String> {
    let manager = state.manager.read().await;

    match manager.commit_transaction(&connection_id).await {
        Ok(_) => Ok(()),
        Err(e) => Err(fail(&e)),
    }
}

/// 回滚事务
#[command]
pub async fn rollback_transaction(
    state: State<'_, ConnectionManagerState>,
    connection_id: String,
) -> Result<(), String> {
    let manager = state.manager.read().await;

    match manager.rollback_transaction(&connection_id).await {
        Ok(_) => Ok(()),
        Err(e) => Err(fail(&e)),
    }
}

/// 取消当前连接上正在执行的查询
#[command]
pub async fn cancel_query(
    state: State<'_, ConnectionManagerState>,
    connection_id: String,
) -> Result<(), String> {
    let manager = state.manager.read().await;

    match manager.cancel_query(&connection_id).await {
        Ok(_) => Ok(()),
        Err(e) => Err(fail(&e)),
    }
}

/// 导出表数据
#[command]
pub async fn export_table_data(
    state: State<'_, ConnectionManagerState>,
    license: State<'_, LicenseManager>,
    connection_id: String,
    database: String,
    table: String,
    format: String, // csv, sql, json
) -> Result<String, String> {
    license.ensure(FEATURE_IMPORT_EXPORT, "导出数据").await?;
    let manager = state.manager.read().await;

    match manager.export_table_data(&connection_id, &database, &table, &format).await {
        Ok(data) => Ok(data),
        Err(e) => Err(fail(&e)),
    }
}

/// 导入数据
#[command]
pub async fn import_data(
    state: State<'_, ConnectionManagerState>,
    audit: State<'_, AuditLog>,
    license: State<'_, LicenseManager>,
    connection_id: String,
    database: String,
    table: String,
    format: String, // csv, sql
    data: String,
) -> Result<u64, String> {
    license.ensure(FEATURE_IMPORT_EXPORT, "导入数据").await?;
    // 不把整份导入内容写进审计日志，只记录来源与体量
    let summary = format!(
        "-- 导入 `{}`.`{}`（格式：{}，{} 字节）",
        database, table, format, data.len()
    );
    let (name, host) = connection_meta(&state, &connection_id).await;
    let started = Instant::now();
    let outcome = {
        let manager = state.manager.read().await;
        manager
            .import_data(&connection_id, &database, &table, &format, &data)
            .await
    };

    let mut entry = AuditEntry::new("import", &connection_id, &name, &host, &database, &summary);
    entry.duration_ms = started.elapsed().as_millis() as u64;
    match outcome {
        Ok(count) => {
            entry.rows_affected = Some(count);
            audit.record(entry).await;
            Ok(count)
        }
        Err(e) => {
            entry.success = false;
            entry.error = Some(e.to_string());
            audit.record(entry).await;
            Err(fail(&e))
        }
    }
}

/// 导出数据库SQL
#[command]
pub async fn export_database_sql(
    state: State<'_, ConnectionManagerState>,
    license: State<'_, LicenseManager>,
    connection_id: String,
    database: String,
) -> Result<String, String> {
    license.ensure(FEATURE_IMPORT_EXPORT, "导出数据库").await?;
    tracing::info!("export_database_sql command received: {}.{}", connection_id, database);

    // 克隆 Arc 避免长时间持有锁
    let manager = Arc::clone(&state.manager);

    // 在内部作用域中获取锁并立即释放
    let result = {
        let mgr = manager.read().await;
        tracing::info!("Got manager lock");
        mgr.export_database_sql(&connection_id, &database).await
    };

    match result {
        Ok(data) => {
            tracing::info!("Export completed, data length: {}", data.len());
            Ok(data)
        }
        Err(e) => {
            tracing::error!("Export failed: {}", e);
            Err(fail(&e))
        }
    }
}

/// 导入数据库SQL
#[command]
pub async fn import_database_sql(
    state: State<'_, ConnectionManagerState>,
    audit: State<'_, AuditLog>,
    license: State<'_, LicenseManager>,
    connection_id: String,
    database: String,
    sql: String,
) -> Result<(), String> {
    license.ensure(FEATURE_IMPORT_EXPORT, "导入数据库").await?;
    let (name, host) = connection_meta(&state, &connection_id).await;
    let started = Instant::now();
    let outcome = {
        let manager = state.manager.read().await;
        manager
            .import_database_sql(&connection_id, &database, &sql)
            .await
    };

    let mut entry = AuditEntry::new("import", &connection_id, &name, &host, &database, &sql);
    entry.duration_ms = started.elapsed().as_millis() as u64;
    match outcome {
        Ok(_) => {
            audit.record(entry).await;
            Ok(())
        }
        Err(e) => {
            entry.success = false;
            entry.error = Some(e.to_string());
            audit.record(entry).await;
            Err(fail(&e))
        }
    }
}

/// 读取服务端会话/进程列表
#[command]
pub async fn get_process_list(
    state: State<'_, ConnectionManagerState>,
    license: State<'_, LicenseManager>,
    connection_id: String,
) -> Result<Vec<crate::core::connection::ProcessInfo>, String> {
    license.ensure(FEATURE_MONITOR, "实例监控").await?;
    let manager = state.manager.read().await;

    match manager.get_process_list(&connection_id).await {
        Ok(processes) => Ok(processes),
        Err(e) => Err(fail(&e)),
    }
}

/// 终止指定会话
#[command]
pub async fn kill_process(
    state: State<'_, ConnectionManagerState>,
    license: State<'_, LicenseManager>,
    connection_id: String,
    process_id: u64,
) -> Result<(), String> {
    license.ensure(FEATURE_MONITOR, "实例监控").await?;
    let manager = state.manager.read().await;

    match manager.kill_process(&connection_id, process_id).await {
        Ok(_) => Ok(()),
        Err(e) => Err(fail(&e)),
    }
}

/// 读取服务端运行指标（版本、连接数、QPS、慢查询、缓冲池命中率等）
#[command]
pub async fn get_server_metrics(
    state: State<'_, ConnectionManagerState>,
    license: State<'_, LicenseManager>,
    connection_id: String,
) -> Result<crate::core::connection::ServerMetrics, String> {
    license.ensure(FEATURE_MONITOR, "实例监控").await?;
    let manager = state.manager.read().await;

    match manager.get_server_metrics(&connection_id).await {
        Ok(metrics) => Ok(metrics),
        Err(e) => Err(fail(&e)),
    }
}

/// 解释 SQL 执行计划（EXPLAIN FORMAT=JSON / EXPLAIN (FORMAT JSON)）
///
/// 前端调用：`invoke('explain_sql', { connectionId, database, sql })`
#[command]
pub async fn explain_sql(
    state: State<'_, ConnectionManagerState>,
    license: State<'_, LicenseManager>,
    connection_id: String,
    database: String,
    sql: String,
) -> Result<ExplainPlan, String> {
    license.ensure(FEATURE_QUERY_PLAN, "执行计划解释").await?;
    let manager = state.manager.read().await;

    match manager.explain(&connection_id, &database, &sql).await {
        Ok(plan) => Ok(plan),
        Err(e) => Err(fail(&e)),
    }
}

/// 拉取整个数据库的 ER 图数据（所有表 + 所有外键）
///
/// 前端调用：`invoke('get_er_diagram_data', { connectionId, database })`
#[command]
pub async fn get_er_diagram_data(
    state: State<'_, ConnectionManagerState>,
    license: State<'_, LicenseManager>,
    connection_id: String,
    database: String,
) -> Result<ErDiagramData, String> {
    license.ensure(FEATURE_ER_DIAGRAM, "ER 图").await?;
    let manager = state.manager.read().await;

    match manager.get_er_diagram_data(&connection_id, &database).await {
        Ok(data) => Ok(data),
        Err(e) => Err(fail(&e)),
    }
}
