#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod core;
mod password_store;

use commands::connection::ConnectionManagerState;
use core::audit::AuditLog;
use core::license::LicenseManager;
use password_store::PasswordStore;
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

    // 初始化密码存储
    let password_store = PasswordStore::new().expect("Failed to initialize password store");

    tauri::Builder::default()
        .manage(ConnectionManagerState::new())
        .manage(password_store)
        .manage(AuditLog::new())
        .manage(LicenseManager::new())
        .plugin(tauri_plugin_dialog::init())
        // 自动更新：更新源与签名公钥在 tauri.conf.json 的 plugins.updater 中配置
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![
            commands::connection::connect,
            commands::connection::disconnect,
            commands::connection::get_connection_status,
            commands::connection::execute_sql,
            commands::connection::apply_table_changes,
            commands::connection::query_table_data,
            commands::connection::get_object_tree,
            commands::connection::get_table_structure,
            commands::connection::get_schema_snapshot,
            commands::connection::compare_schemas,
            commands::connection::get_database_info,
            commands::connection::begin_transaction,
            commands::connection::commit_transaction,
            commands::connection::rollback_transaction,
            commands::connection::cancel_query,
            commands::connection::export_table_data,
            commands::connection::import_data,
            commands::connection::export_database_sql_to_file,
            commands::connection::import_database_sql,
            commands::connection::get_process_list,
            commands::connection::kill_process,
            commands::connection::get_server_metrics,
            commands::connection::explain_sql,
            commands::connection::get_er_diagram_data,
            commands::connection::backup_database,
            commands::connection::restore_database,
            commands::connection::list_backup_history,
            commands::connection::delete_backup_record,
            commands::audit::list_audit_entries,
            commands::audit::clear_audit_logs,
            password_store::save_connection_password,
            password_store::get_connection_password,
            password_store::delete_connection_password,
            password_store::find_connection_password,
            commands::file::save_file_to_dialog,
            commands::file::read_file_from_dialog,
            commands::file::export_to_excel,
            commands::file::export_table_to_excel,
            commands::license::get_license_status,
            commands::license::activate_license,
            commands::license::deactivate_license,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
