use tauri::command;
use tauri::State;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_dialog::FilePath;
use rust_xlsxwriter::Workbook;
use serde::{Deserialize, Serialize};

use crate::commands::connection::ConnectionManagerState;
use crate::core::connection::export::ExportColumn;
use crate::core::connection::CellValue;
use crate::core::license::{LicenseManager, FEATURE_IMPORT_EXPORT};

/// 列定义
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ExcelColumn {
    pub name: String,
    pub data_type: String,
}

/// 导出数据为 Excel 文件
#[command]
pub async fn export_to_excel(
    window: tauri::Window,
    license: State<'_, LicenseManager>,
    columns: Vec<ExcelColumn>,
    data: Vec<serde_json::Map<String, serde_json::Value>>,
    filename: String,
) -> Result<String, String> {
    license.ensure(FEATURE_IMPORT_EXPORT, "导出 Excel").await?;
    // 创建临时文件路径
    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join(&filename);
    
    // 创建工作簿
    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    
    // 设置表头样式（无背景色）
    let header_format = rust_xlsxwriter::Format::new()
        .set_bold();
    
    // 写入表头
    for (col_idx, col) in columns.iter().enumerate() {
        worksheet.write_string_with_format(0, col_idx as u16, &col.name, &header_format)
            .map_err(|e| format!("写入表头失败: {}", e))?;
    }
    
    // 写入数据
    for (row_idx, row) in data.iter().enumerate() {
        let row_num = (row_idx + 1) as u32; // 从第2行开始（第1行是表头）
        
        for (col_idx, col) in columns.iter().enumerate() {
            let value = row.get(&col.name).cloned().unwrap_or(serde_json::Value::Null);
            
            match value {
                serde_json::Value::Null => {
                    worksheet.write_string(row_num, col_idx as u16, "NULL")
                        .map_err(|e| format!("写入数据失败: {}", e))?;
                }
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        worksheet.write_number(row_num, col_idx as u16, i as f64)
                            .map_err(|e| format!("写入数据失败: {}", e))?;
                    } else if let Some(f) = n.as_f64() {
                        worksheet.write_number(row_num, col_idx as u16, f)
                            .map_err(|e| format!("写入数据失败: {}", e))?;
                    } else {
                        worksheet.write_string(row_num, col_idx as u16, &n.to_string())
                            .map_err(|e| format!("写入数据失败: {}", e))?;
                    }
                }
                serde_json::Value::Bool(b) => {
                    worksheet.write_string(row_num, col_idx as u16, &b.to_string())
                        .map_err(|e| format!("写入数据失败: {}", e))?;
                }
                serde_json::Value::String(s) => {
                    worksheet.write_string(row_num, col_idx as u16, &s)
                        .map_err(|e| format!("写入数据失败: {}", e))?;
                }
                _ => {
                    worksheet.write_string(row_num, col_idx as u16, &value.to_string())
                        .map_err(|e| format!("写入数据失败: {}", e))?;
                }
            }
        }
    }
    
    // 自动调整列宽
    for (col_idx, col) in columns.iter().enumerate() {
        let width = calculate_column_width(col, &data);
        worksheet.set_column_width(col_idx as u16, width as f64)
            .map_err(|e| format!("设置列宽失败: {}", e))?;
    }
    
    // 保存到临时文件
    workbook.save(&temp_path)
        .map_err(|e| format!("保存 Excel 文件失败: {}", e))?;
    
    // 显示保存对话框
    let (tx, rx) = tokio::sync::oneshot::channel::<Option<FilePath>>();
    
    window.dialog()
        .file()
        .set_title("保存 Excel 文件")
        .set_file_name(&filename)
        .add_filter("Excel 文件", &["xlsx"])
        .add_filter("所有文件", &["*"])
        .save_file(move |path| {
            let _ = tx.send(path);
        });
    
    // 等待对话框结果
    match rx.await {
        Ok(Some(path)) => {
            let target_path_str = path.to_string();
            let target_path = std::path::PathBuf::from(&target_path_str);
            
            // 复制临时文件到目标路径
            match tokio::fs::copy(&temp_path, &target_path).await {
                Ok(_) => {
                    // 删除临时文件
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    Ok(target_path_str)
                }
                Err(e) => {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    Err(format!("保存文件失败: {}", e))
                }
            }
        }
        Ok(None) => {
            // 用户取消，删除临时文件
            let _ = tokio::fs::remove_file(&temp_path).await;
            Err("用户取消了保存操作".to_string())
        }
        Err(_) => {
            let _ = tokio::fs::remove_file(&temp_path).await;
            Err("对话框操作被取消".to_string())
        }
    }
}

/// 导出表数据为 Excel 文件
///
/// 全表数据在后端直接读取并写入工作簿，避免大表经 IPC 传输到前端。
#[command]
pub async fn export_table_to_excel(
    window: tauri::Window,
    state: State<'_, ConnectionManagerState>,
    license: State<'_, LicenseManager>,
    connection_id: String,
    database: String,
    table: String,
    filename: String,
) -> Result<String, String> {
    license.ensure(FEATURE_IMPORT_EXPORT, "导出 Excel").await?;
    let data = {
        let manager = state.manager.read().await;
        manager
            .fetch_table_export(&connection_id, &database, &table)
            .await
            .map_err(|e| crate::core::error::user_message(&e))?
    };

    let mut workbook = Workbook::new();
    let worksheet = workbook.add_worksheet();
    let header_format = rust_xlsxwriter::Format::new().set_bold();

    for (col_idx, col) in data.columns.iter().enumerate() {
        worksheet
            .write_string_with_format(0, col_idx as u16, &col.name, &header_format)
            .map_err(|e| format!("写入表头失败: {}", e))?;
    }

    for (row_idx, row) in data.rows.iter().enumerate() {
        let row_num = (row_idx + 1) as u32;
        for (col_idx, cell) in row.iter().enumerate() {
            write_excel_cell(worksheet, row_num, col_idx as u16, cell)
                .map_err(|e| format!("写入数据失败: {}", e))?;
        }
    }

    for (col_idx, col) in data.columns.iter().enumerate() {
        let width = excel_column_width(col_idx, col, &data.rows);
        worksheet
            .set_column_width(col_idx as u16, width as f64)
            .map_err(|e| format!("设置列宽失败: {}", e))?;
    }

    // 用户传入的文件名只允许纯文件名，避免路径穿越
    let safe_name = if filename.is_empty() || filename.contains(|c| c == '/' || c == '\\') {
        format!("{}.xlsx", table)
    } else {
        filename
    };

    save_workbook_with_dialog(window, workbook, &safe_name).await
}

/// 按类型写入 Excel 单元格：NULL 留空，数字/布尔保留原生类型，文本按字符串写入
fn write_excel_cell(
    worksheet: &mut rust_xlsxwriter::Worksheet,
    row: u32,
    col: u16,
    cell: &CellValue,
) -> Result<(), rust_xlsxwriter::XlsxError> {
    match cell {
        // 空单元格即 NULL，避免把 "NULL" 当作文本写入
        CellValue::Null => Ok(()),
        CellValue::Integer(value) => {
            // Excel 数字精度的安全上限为 2^53，超出时按文本写入避免失真
            const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_992;
            if value.unsigned_abs() <= MAX_SAFE_INTEGER {
                worksheet.write_number(row, col, *value as f64).map(|_| ())
            } else {
                worksheet
                    .write_string(row, col, value.to_string())
                    .map(|_| ())
            }
        }
        CellValue::Float(value) => worksheet.write_number(row, col, *value).map(|_| ()),
        CellValue::Boolean(value) => worksheet.write_boolean(row, col, *value).map(|_| ()),
        CellValue::String(text) | CellValue::DateTime(text) | CellValue::Binary(text) => {
            worksheet.write_string(row, col, text).map(|_| ())
        }
    }
}

/// 计算 Excel 列宽：按表头与数据中最长文本估算，限制在 10-50
fn excel_column_width(index: usize, col: &ExportColumn, rows: &[Vec<CellValue>]) -> usize {
    let mut max_len = col.name.chars().count();
    for row in rows {
        let Some(cell) = row.get(index) else { continue };
        let len = match cell {
            CellValue::Null => 0,
            CellValue::Boolean(_) => 5,
            CellValue::Integer(value) => value.to_string().chars().count(),
            CellValue::Float(value) => value.to_string().chars().count(),
            CellValue::String(text) | CellValue::DateTime(text) | CellValue::Binary(text) => {
                text.chars().count()
            }
        };
        max_len = max_len.max(len);
    }
    (max_len + 2).clamp(10, 50)
}

/// 将工作簿保存到临时文件，再通过保存对话框复制到用户选择的位置
async fn save_workbook_with_dialog(
    window: tauri::Window,
    mut workbook: Workbook,
    filename: &str,
) -> Result<String, String> {
    let temp_path = std::env::temp_dir().join(format!(
        "auroradb-export-{}.xlsx",
        chrono::Utc::now().timestamp_millis()
    ));

    workbook
        .save(&temp_path)
        .map_err(|e| format!("保存 Excel 文件失败: {}", e))?;

    let (tx, rx) = tokio::sync::oneshot::channel::<Option<FilePath>>();

    window
        .dialog()
        .file()
        .set_title("导出表数据")
        .set_file_name(filename)
        .add_filter("Excel 文件", &["xlsx"])
        .add_filter("所有文件", &["*"])
        .save_file(move |path| {
            let _ = tx.send(path);
        });

    match rx.await {
        Ok(Some(path)) => {
            let target_path_str = path.to_string();
            let target_path = std::path::PathBuf::from(&target_path_str);
            match tokio::fs::copy(&temp_path, &target_path).await {
                Ok(_) => {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    Ok(target_path_str)
                }
                Err(e) => {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    Err(format!("保存文件失败: {}", e))
                }
            }
        }
        Ok(None) => {
            let _ = tokio::fs::remove_file(&temp_path).await;
            Err("用户取消了保存操作".to_string())
        }
        Err(_) => {
            let _ = tokio::fs::remove_file(&temp_path).await;
            Err("对话框操作被取消".to_string())
        }
    }
}

/// 计算列宽
fn calculate_column_width(
    col: &ExcelColumn,
    data: &[serde_json::Map<String, serde_json::Value>],
) -> usize {
    let header_len = col.name.len();
    let max_data_len = data
        .iter()
        .map(|row| {
            let value = row.get(&col.name).cloned().unwrap_or(serde_json::Value::Null);
            match value {
                serde_json::Value::String(s) => s.len(),
                _ => value.to_string().len(),
            }
        })
        .max()
        .unwrap_or(0);
    
    let max_len = header_len.max(max_data_len);
    // 最小宽度 10，最大宽度 50
    (max_len + 2).clamp(10, 50)
}

/// 保存文件到用户选择的对话框路径
#[command]
pub async fn save_file_to_dialog(
    window: tauri::Window,
    content: String,
    default_filename: String,
    format: String,
) -> Result<String, String> {
    // 根据格式设置文件过滤器和扩展名
    let (filter_name, extensions) = match format.as_str() {
        "csv" => ("CSV 文件", vec!["csv"]),
        "sql" => ("SQL 文件", vec!["sql"]),
        "json" => ("JSON 文件", vec!["json"]),
        "xlsx" => ("Excel 文件", vec!["xlsx"]),
        _ => ("文本文件", vec!["txt"]),
    };

    // 确保文件名有正确的扩展名
    let default_filename = if default_filename.contains('.') {
        default_filename
    } else {
        format!("{}.{}", default_filename, extensions[0])
    };

    // 使用 Tauri v2 的文件对话框 - 使用通道接收结果
    let (tx, rx) = tokio::sync::oneshot::channel::<Option<FilePath>>();

    window.dialog()
        .file()
        .set_title("保存文件")
        .set_file_name(&default_filename)
        .add_filter(filter_name, &extensions)
        .add_filter("所有文件", &["*"])
        .save_file(move |path| {
            let _ = tx.send(path);
        });

    // 等待对话框结果
    match rx.await {
        Ok(Some(path)) => {
            // 写入文件内容 - 从 FilePath 获取路径字符串
            let path_str = path.to_string();
            let path_buf = std::path::PathBuf::from(&path_str);
            match tokio::fs::write(&path_buf, content).await {
                Ok(_) => Ok(path_str),
                Err(e) => Err(format!("保存文件失败: {}", e)),
            }
        }
        Ok(None) => Err("用户取消了保存操作".to_string()),
        Err(_) => Err("对话框操作被取消".to_string()),
    }
}

/// 从文件选择对话框读取文件内容
#[command]
pub async fn read_file_from_dialog(
    window: tauri::Window,
    format: String,
) -> Result<(String, String), String> {
    // 根据格式设置文件过滤器
    let (filter_name, extensions) = dialog_filter(&format);

    // 使用 Tauri v2 的文件对话框
    let (tx, rx) = tokio::sync::oneshot::channel::<Option<FilePath>>();

    window.dialog()
        .file()
        .set_title("选择要导入的文件")
        .add_filter(filter_name, &extensions)
        .add_filter("所有文件", &["*"])
        .pick_file(move |path| {
            let _ = tx.send(path);
        });

    // 等待对话框结果
    match rx.await {
        Ok(Some(path)) => {
            let path_str = path.to_string();
            let path_buf = std::path::PathBuf::from(&path_str);
            
            // 检查文件大小限制（10MB）
            const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024; // 10MB
            
            match tokio::fs::metadata(&path_buf).await {
                Ok(metadata) => {
                    if metadata.len() > MAX_FILE_SIZE {
                        return Err(format!("文件大小超过限制（最大 10MB）"));
                    }
                }
                Err(e) => return Err(format!("无法读取文件信息: {}", e)),
            }
            
            // 读取文件内容
            match tokio::fs::read_to_string(&path_buf).await {
                Ok(content) => Ok((path_str, content)),
                Err(e) => Err(format!("读取文件失败: {}", e)),
            }
        }
        Ok(None) => Err("用户取消了选择操作".to_string()),
        Err(_) => Err("对话框操作被取消".to_string()),
    }
}

/// 只选择文件并返回路径，不读取内容、不限制大小。
///
/// 供大 SQL 文件导入 / dump 还原使用：内容由后端直接流式写入 `mysql` 客户端，
/// 既不经过 IPC 也不占用内存，因此没有 10MB 上限。
#[command]
pub async fn pick_file_path(window: tauri::Window, format: String) -> Result<String, String> {
    let (filter_name, extensions) = dialog_filter(&format);

    let (tx, rx) = tokio::sync::oneshot::channel::<Option<FilePath>>();
    window.dialog()
        .file()
        .set_title("选择要导入的文件")
        .add_filter(filter_name, &extensions)
        .add_filter("所有文件", &["*"])
        .pick_file(move |path| {
            let _ = tx.send(path);
        });

    match rx.await {
        Ok(Some(path)) => Ok(path.to_string()),
        Ok(None) => Err("用户取消了选择操作".to_string()),
        Err(_) => Err("对话框操作被取消".to_string()),
    }
}

/// 对话框文件过滤器（名称 + 扩展名）
fn dialog_filter(format: &str) -> (&'static str, Vec<&'static str>) {
    match format {
        "csv" => ("CSV 文件", vec!["csv"]),
        "sql" => ("SQL 文件", vec!["sql"]),
        "json" => ("JSON 文件", vec!["json"]),
        _ => ("文本文件", vec!["txt", "csv", "sql", "json"]),
    }
}
