#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod password_store;

use auroradb_core::audit::AuditLog;
use commands::connection::ConnectionManagerState;
use commands::sql_file::PendingSqlFiles;
use password_store::PasswordStore;
use tauri::Manager;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

/// 初始化日志：stdout + 按天滚动的文件日志（写入应用数据目录 `mysql-client/logs/`，保留 7 天）
///
/// 文件日志初始化失败时自动退化为仅 stdout。返回的 guard 必须在 main 期间存活，
/// 否则非阻塞写入器中的缓冲日志会丢失。
fn init_tracing() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let stdout_layer = tracing_subscriber::fmt::layer().with_filter(LevelFilter::INFO);

    let appender = dirs::data_dir()
        .map(|dir| dir.join("mysql-client").join("logs"))
        .and_then(|dir| {
            std::fs::create_dir_all(&dir).ok()?;
            tracing_appender::rolling::RollingFileAppender::builder()
                .rotation(tracing_appender::rolling::Rotation::DAILY)
                .filename_prefix("auroradb")
                .filename_suffix("log")
                // 上限为 7 天 + 1：官方说明保留数量可能略低于上限，多留一个才能稳定保留最近 7 天
                .max_log_files(8)
                .build(&dir)
                .ok()
        });

    let Some(appender) = appender else {
        tracing_subscriber::registry().with(stdout_layer).init();
        return None;
    };

    let (writer, guard) = tracing_appender::non_blocking(appender);
    tracing_subscriber::registry()
        .with(stdout_layer)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(writer)
                .with_ansi(false)
                .with_filter(LevelFilter::INFO),
        )
        .init();
    Some(guard)
}

/// 部分 Linux 环境下 WebKitGTK 无法通过 GBM 分配 DMA-BUF 缓冲（NVIDIA 专有驱动、
/// 无 3D 加速的虚拟机、DRM 节点权限受限等），会打印
/// `KMS: DRM_IOCTL_MODE_CREATE_DUMB failed` / `Failed to create GBM buffer` 并导致白屏。
/// 默认关闭该渲染路径，退回共享内存合成；确认本机支持时可设置 AURORADB_ENABLE_DMABUF=1 恢复。
#[cfg(target_os = "linux")]
fn configure_webkit_env() {
    // 用户或发行版已显式配置时不覆盖
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_some()
        || std::env::var_os("AURORADB_ENABLE_DMABUF").is_some()
    {
        return;
    }
    std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
}

fn main() {
    // 必须早于 Tauri/WebKit 初始化，WebKitWebProcess 会继承该变量
    #[cfg(target_os = "linux")]
    configure_webkit_env();

    let _log_guard = init_tracing();
    tracing::info!("AuroraDB v{} starting", env!("CARGO_PKG_VERSION"));

    // 初始化密码存储：失败时给出可诊断的提示，而不是直接 panic 掉整个进程
    let password_store = match PasswordStore::new() {
        Ok(store) => store,
        Err(e) => {
            tracing::error!("初始化密码存储失败：{}", e);
            eprintln!("无法初始化密码存储（数据目录或系统凭据库不可用）：{e}");
            std::process::exit(1);
        }
    };

    tauri::Builder::default()
        .manage(ConnectionManagerState::new())
        .manage(password_store)
        .manage(AuditLog::new())
        .manage(PendingSqlFiles::default())
        // 单实例必须最先注册：第二次双击 .sql 时不再新开进程，
        // 而是把命令行里的文件路径转发给已运行实例并聚焦窗口
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
            let paths = commands::sql_file::sql_paths_from_args(argv);
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                commands::sql_file::enqueue_sql_paths(&handle, paths).await;
            });
        }))
        .plugin(tauri_plugin_dialog::init())
        // 自动更新：更新源与签名公钥在 tauri.conf.json 的 plugins.updater 中配置
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![
            commands::connection::connect,
            commands::connection::disconnect,
            commands::connection::get_connection_status,
            commands::connection::execute_sql,
            commands::connection::split_sql_statements,
            commands::connection::apply_table_changes,
            commands::connection::query_table_data,
            commands::connection::get_object_tree,
            commands::connection::get_table_structure,
            commands::connection::get_referencing_foreign_keys,
            commands::connection::get_object_definition,
            commands::connection::get_schema_snapshot,
            commands::pro::compare_schemas,
            commands::pro::apply_schema_diff,
            commands::connection::get_database_info,
            commands::connection::begin_transaction,
            commands::connection::commit_transaction,
            commands::connection::rollback_transaction,
            commands::connection::cancel_query,
            commands::file::export_table_to_file,
            commands::connection::import_data,
            commands::connection::export_database_sql_to_file,
            commands::connection::import_database_sql,
            commands::connection::import_sql_file,
            commands::pro::get_process_list,
            commands::pro::kill_process,
            commands::pro::get_server_metrics,
            commands::pro::explain_sql,
            commands::pro::get_er_diagram_data,
            commands::pro::backup_database,
            commands::pro::restore_database,
            commands::pro::global_search,
            commands::audit::list_audit_entries,
            commands::audit::clear_audit_logs,
            password_store::save_connection_password,
            password_store::get_connection_password,
            password_store::delete_connection_password,
            password_store::find_connection_password,
            commands::file::save_file_to_dialog,
            commands::file::save_binary_file_to_dialog,
            commands::file::read_file_from_dialog,
            commands::file::pick_file_path,
            commands::file::export_to_excel,
            commands::file::export_table_to_excel,
            commands::update::get_update_install_kind,
            commands::update::download_update_package,
            commands::sql_file::take_pending_sql_files,
            commands::sql_file::read_sql_file,
            commands::sql_tools::format_sql,
        ])
        // 首次启动就带文件参数（文件关联双击）：先入队，等前端挂载后取走
        .setup(|app| {
            let paths = commands::sql_file::sql_paths_from_args(std::env::args());
            if !paths.is_empty() {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    commands::sql_file::enqueue_sql_paths(&handle, paths).await;
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
