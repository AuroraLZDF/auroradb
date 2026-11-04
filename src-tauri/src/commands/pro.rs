//! 专业版命令：结构对比、实例监控、备份还原、ER 图、全局搜索、执行计划。

use super::connection::{
    execute_with_audit, fail, ConnectionManagerState, ImportProgressPayload,
};
use auroradb_core::audit::AuditLog;
use auroradb_core::connection::transfer::{ImportProgress, ImportReporter};
use auroradb_pro::er_diagram::{get_er_diagram_data as fetch_er_diagram, ErDiagramData};
use auroradb_pro::explain::{explain as explain_sql_plan, ExplainPlan};
use auroradb_pro::global_search::{
    global_search as run_global_search, GlobalSearchRequest, GlobalSearchResult,
};
use auroradb_pro::monitor::{
    get_process_list as fetch_process_list, get_server_metrics as fetch_server_metrics,
    kill_process as kill_server_process, ProcessInfo, ServerMetrics,
};
use auroradb_pro::schema_diff::{
    compare_schemas as diff_schemas, ApplySchemaDiffResult, SchemaDiff,
};
use std::sync::Arc;
use tauri::{command, Emitter, State};
use tauri_plugin_dialog::{DialogExt, FilePath};

/// 对比两个库的表结构，返回差异与迁移脚本
#[command]
pub async fn compare_schemas(
    state: State<'_, ConnectionManagerState>,
    source_connection_id: String,
    source_database: String,
    target_connection_id: String,
    target_database: String,
) -> Result<SchemaDiff, String> {
    let manager = state.manager.read().await;

    diff_schemas(
        &manager,
        &source_connection_id,
        &source_database,
        &target_connection_id,
        &target_database,
    )
    .await
    .map_err(|e| fail(&e))
}

/// 将 Schema 差异迁移脚本逐条应用到目标库，遇错即停并返回失败序号
#[command]
pub async fn apply_schema_diff(
    state: State<'_, ConnectionManagerState>,
    audit: State<'_, AuditLog>,
    connection_id: String,
    database: String,
    statements: Vec<String>,
) -> Result<ApplySchemaDiffResult, String> {

    let mut applied = 0usize;
    for (idx, sql) in statements.iter().enumerate() {
        if let Err(e) = execute_with_audit(&state, &audit, &connection_id, &database, sql).await {
            return Ok(ApplySchemaDiffResult {
                applied,
                failed_at: Some(idx + 1),
                error: Some(e),
            });
        }
        applied += 1;
    }

    Ok(ApplySchemaDiffResult {
        applied,
        failed_at: None,
        error: None,
    })
}

/// 读取服务端会话/进程列表
#[command]
pub async fn get_process_list(
    state: State<'_, ConnectionManagerState>,
    connection_id: String,
) -> Result<Vec<ProcessInfo>, String> {
    let manager = state.manager.read().await;

    fetch_process_list(&manager, &connection_id)
        .await
        .map_err(|e| fail(&e))
}

/// 终止指定会话
#[command]
pub async fn kill_process(
    state: State<'_, ConnectionManagerState>,
    connection_id: String,
    process_id: u64,
) -> Result<(), String> {
    let manager = state.manager.read().await;

    kill_server_process(&manager, &connection_id, process_id)
        .await
        .map_err(|e| fail(&e))
}

/// 读取服务端运行指标（版本、连接数、QPS、慢查询、缓冲池命中率等）
#[command]
pub async fn get_server_metrics(
    state: State<'_, ConnectionManagerState>,
    connection_id: String,
) -> Result<ServerMetrics, String> {
    let manager = state.manager.read().await;

    fetch_server_metrics(&manager, &connection_id)
        .await
        .map_err(|e| fail(&e))
}

/// 解释 SQL 执行计划（EXPLAIN FORMAT=JSON / EXPLAIN (FORMAT JSON)）
///
/// 前端调用：`invoke('explain_sql', { connectionId, database, sql })`
#[command]
pub async fn explain_sql(
    state: State<'_, ConnectionManagerState>,
    connection_id: String,
    database: String,
    sql: String,
) -> Result<ExplainPlan, String> {
    let manager = state.manager.read().await;

    explain_sql_plan(&manager, &connection_id, &database, &sql)
        .await
        .map_err(|e| fail(&e))
}

/// 拉取整个数据库的 ER 图数据（所有表 + 所有外键）
///
/// 前端调用：`invoke('get_er_diagram_data', { connectionId, database })`
#[command]
pub async fn get_er_diagram_data(
    state: State<'_, ConnectionManagerState>,
    connection_id: String,
    database: String,
) -> Result<ErDiagramData, String> {
    let manager = state.manager.read().await;

    fetch_er_diagram(&manager, &connection_id, &database)
        .await
        .map_err(|e| fail(&e))
}

/// 触发完整数据库备份（mysqldump 子进程）
///
/// 由用户在系统保存对话框中选择输出路径，后端直接把 dump 写入该文件。
#[command]
pub async fn backup_database(
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
        .set_title("备份数据库")
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
    auroradb_pro::backup::backup_database(&manager, &connection_id, &database, &output_path)
        .await
        .map_err(|e| fail(&e))?;

    Ok(output_path)
}

/// 从 dump 文件还原数据库
///
/// 执行过程中推送 `restore-progress` 事件，前端据此展示字节进度与逐条语句流水，
/// 避免大文件还原期间界面只有静止的按钮、用户无从判断是否卡死。
#[command]
pub async fn restore_database(
    window: tauri::Window,
    state: State<'_, ConnectionManagerState>,
    connection_id: String,
    database: String,
    dump_path: String,
) -> Result<(), String> {
    let manager = state.manager.read().await;

    let reporter: ImportReporter = Arc::new(move |progress: ImportProgress| {
        let payload = ImportProgressPayload {
            task_id: String::new(),
            bytes: progress.bytes,
            total: progress.total,
            lines: progress.lines,
            done: progress.done,
            success: progress.success,
        };
        let _ = window.emit("restore-progress", payload);
    });

    auroradb_pro::backup::restore_database(
        &manager,
        &connection_id,
        &database,
        &dump_path,
        Some(reporter),
    )
    .await
    .map_err(|e| fail(&e))
}

/// 跨库全局搜索：按表名 / 字段名 / 数据内容查找
#[command]
pub async fn global_search(
    state: State<'_, ConnectionManagerState>,
    request: GlobalSearchRequest,
) -> Result<GlobalSearchResult, String> {
    let manager = state.manager.read().await;
    run_global_search(&manager, request)
        .await
        .map_err(|e| fail(&e))
}
