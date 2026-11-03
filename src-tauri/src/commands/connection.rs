use auroradb_core::audit::{AuditEntry, AuditLog};
use auroradb_core::connection::transfer::{ImportLogLine, ImportProgress, ImportReporter};
use auroradb_core::connection::{
    ConnectionConfig, ConnectionManager, ConnectionRequest, ConnectionStatus, DataQueryRequest,
    SshConfig,
};
use auroradb_core::license::{LicenseManager, FEATURE_SSH_TUNNEL};
use tauri::{command, Emitter, State};
use tauri_plugin_dialog::{DialogExt, FilePath};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

pub struct ConnectionManagerState {
    pub manager: Arc<RwLock<ConnectionManager>>,
}

impl ConnectionManagerState {
    pub fn new() -> Self {
        // SSH 隧道实现位于专业版 crate，启动时注入工厂；免费构建保持 None
        #[cfg(feature = "pro")]
        let manager = ConnectionManager::with_tunnel_factory(auroradb_pro::tunnel::factory());
        #[cfg(not(feature = "pro"))]
        let manager = ConnectionManager::new();
        Self {
            manager: Arc::new(RwLock::new(manager)),
        }
    }
}

/// 统一错误出口：驱动错误翻译为中文可读提示，其余错误保留原信息
pub(crate) fn fail(e: &anyhow::Error) -> String {
    auroradb_core::error::user_message(e)
}

/// 审计用连接信息：连接已断开时退化为连接 ID，host 留空
pub(crate) async fn connection_meta(
    state: &ConnectionManagerState,
    connection_id: &str,
) -> (String, String) {
    let manager = state.manager.read().await;
    match manager.connection_meta(connection_id).await {
        Some((name, host, port)) => (name, format!("{}:{}", host, port)),
        None => (connection_id.to_string(), String::new()),
    }
}

/// 执行 SQL 并写入审计日志（成功与失败都记录）
pub(crate) async fn execute_with_audit(
    state: &ConnectionManagerState,
    audit: &AuditLog,
    connection_id: &str,
    database: &str,
    sql: &str,
) -> Result<auroradb_core::connection::QueryResult, String> {
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
) -> Result<auroradb_core::connection::QueryResult, String> {
    execute_with_audit(&state, &audit, &connection_id, &database, &sql).await
}

/// 按语句切分 SQL（引号、注释、DELIMITER 块内的分号不会被误切）。
///
/// 多语句执行时前端据此逐条执行、按语句分栏展示结果，
/// 避免后端只保留第一个结果集导致其余结果被丢弃。
#[command]
pub fn split_sql_statements(sql: String) -> Vec<String> {
    auroradb_core::sql::split_sql_statements(&sql)
}

/// 在单个事务中批量应用表数据变更（任一条失败即整体回滚）
#[command]
pub async fn apply_table_changes(
    state: State<'_, ConnectionManagerState>,
    audit: State<'_, AuditLog>,
    connection_id: String,
    database: String,
    changes: Vec<auroradb_core::connection::TableChange>,
) -> Result<auroradb_core::connection::ApplyChangesResult, String> {
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
) -> Result<auroradb_core::connection::DataQueryResponse, String> {
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
) -> Result<Vec<auroradb_core::connection::DatabaseObject>, String> {
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
) -> Result<auroradb_core::connection::TableStructure, String> {
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
) -> Result<auroradb_core::connection::SchemaSnapshot, String> {
    let manager = state.manager.read().await;

    match manager.get_schema_snapshot(&connection_id, &database).await {
        Ok(snapshot) => Ok(snapshot),
        Err(e) => Err(fail(&e)),
    }
}

/// 获取数据库信息
#[command]
pub async fn get_database_info(
    state: State<'_, ConnectionManagerState>,
    connection_id: String,
    database: String,
) -> Result<auroradb_core::connection::DatabaseInfo, String> {
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

/// 取消当前连接上正在执行的查询，返回是否因此回滚了进行中的事务
#[command]
pub async fn cancel_query(
    state: State<'_, ConnectionManagerState>,
    connection_id: String,
) -> Result<bool, String> {
    let manager = state.manager.read().await;

    match manager.cancel_query(&connection_id).await {
        Ok(rolled_back) => Ok(rolled_back),
        Err(e) => Err(fail(&e)),
    }
}

/// 导入数据
#[command]
pub async fn import_data(
    state: State<'_, ConnectionManagerState>,
    audit: State<'_, AuditLog>,
    connection_id: String,
    database: String,
    table: String,
    format: String, // csv, sql
    data: String,
) -> Result<u64, String> {
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

/// 导出整库 SQL 到用户选择的位置（mysqldump 直写文件）
///
/// 不把整库拼成字符串经 IPC 传输，因此没有行数上限，也不会在单表超时后静默跳过，
/// 且包含视图 / 触发器 / 存储过程 / 事件。
#[command]
pub async fn export_database_sql_to_file(
    window: tauri::Window,
    state: State<'_, ConnectionManagerState>,
    connection_id: String,
    database: String,
    default_filename: String,
) -> Result<String, String> {
    let (tx, rx) = tokio::sync::oneshot::channel::<Option<FilePath>>();
    window
        .dialog()
        .file()
        .set_title("导出数据库 SQL")
        .set_file_name(&default_filename)
        .add_filter("SQL 文件", &["sql"])
        .add_filter("所有文件", &["*"])
        .save_file(move |path| {
            let _ = tx.send(path);
        });

    let output_path = match rx.await {
        Ok(Some(path)) => path.to_string(),
        Ok(None) => return Err("用户取消了保存操作".to_string()),
        Err(_) => return Err("对话框操作被取消".to_string()),
    };

    let manager = state.manager.read().await;
    manager
        .export_database_sql_to_file(&connection_id, &database, &output_path)
        .await
        .map_err(|e| fail(&e))?;

    Ok(output_path)
}

/// 导入数据库SQL
#[command]
pub async fn import_database_sql(
    state: State<'_, ConnectionManagerState>,
    audit: State<'_, AuditLog>,
    connection_id: String,
    database: String,
    sql: String,
) -> Result<(), String> {
    // 不把整份 SQL 写进审计日志（可能包含敏感数据），只记录体量
    let summary = format!("-- 导入 SQL 脚本（目标库：{}，{} 字节）", database, sql.len());
    let (name, host) = connection_meta(&state, &connection_id).await;
    let started = Instant::now();
    let outcome = {
        let manager = state.manager.read().await;
        manager
            .import_database_sql(&connection_id, &database, &sql)
            .await
    };

    let mut entry = AuditEntry::new("import", &connection_id, &name, &host, &database, &summary);
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

/// 导入进度事件负载：附带 taskId，便于前端区分并发任务
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportProgressPayload {
    pub(crate) task_id: String,
    pub(crate) bytes: u64,
    pub(crate) total: u64,
    pub(crate) lines: Vec<ImportLogLine>,
    pub(crate) done: bool,
    pub(crate) success: bool,
}

/// 从文件导入数据库 SQL
///
/// 只接收文件路径，由后端把文件流式写入 `mysql` 客户端执行，内容不经过 IPC、
/// 不进入内存，因此没有 10MB 上限，也能正确执行含 `DELIMITER` 的脚本。
/// 执行过程中逐条推送 `import-progress` 事件，前端据此展示实时流水。
#[command]
pub async fn import_sql_file(
    window: tauri::Window,
    state: State<'_, ConnectionManagerState>,
    audit: State<'_, AuditLog>,
    connection_id: String,
    database: String,
    file_path: String,
    task_id: String,
) -> Result<(), String> {
    let (name, host) = connection_meta(&state, &connection_id).await;
    let started = Instant::now();
    let reporter: ImportReporter = Arc::new(move |progress: ImportProgress| {
        let payload = ImportProgressPayload {
            task_id: task_id.clone(),
            bytes: progress.bytes,
            total: progress.total,
            lines: progress.lines,
            done: progress.done,
            success: progress.success,
        };
        let _ = window.emit("import-progress", payload);
    });
    let outcome = {
        let manager = state.manager.read().await;
        manager
            .import_sql_file(&connection_id, &database, &file_path, Some(reporter))
            .await
    };

    // 审计只记录来源文件，不把整份 SQL 写进日志
    let summary = format!("-- 从文件导入: {}", file_path);
    let mut entry = AuditEntry::new("import", &connection_id, &name, &host, &database, &summary);
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
