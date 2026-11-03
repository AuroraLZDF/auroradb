//! 免费构建中的专业版命令占位实现。
//!
//! 命令名与参数保持与专业版一致，调用时直接返回升级提示，
//! 避免前端拿到「命令不存在」这类无法理解的错误。

use tauri::command;

const PRO_REQUIRED: &str = "该功能属于 AuroraDB 专业版，请升级后使用";

fn pro_required() -> String {
    PRO_REQUIRED.to_string()
}

#[command]
pub async fn compare_schemas() -> Result<(), String> {
    Err(pro_required())
}

#[command]
pub async fn apply_schema_diff() -> Result<(), String> {
    Err(pro_required())
}

#[command]
pub async fn get_process_list() -> Result<(), String> {
    Err(pro_required())
}

#[command]
pub async fn kill_process() -> Result<(), String> {
    Err(pro_required())
}

#[command]
pub async fn get_server_metrics() -> Result<(), String> {
    Err(pro_required())
}

#[command]
pub async fn explain_sql() -> Result<(), String> {
    Err(pro_required())
}

#[command]
pub async fn get_er_diagram_data() -> Result<(), String> {
    Err(pro_required())
}

#[command]
pub async fn backup_database() -> Result<(), String> {
    Err(pro_required())
}

#[command]
pub async fn restore_database() -> Result<(), String> {
    Err(pro_required())
}

#[command]
pub async fn global_search() -> Result<(), String> {
    Err(pro_required())
}
