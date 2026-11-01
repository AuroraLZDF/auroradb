//! `.sql` 文件打开入口：命令行 / 文件关联 / 拖拽三条路径共用这里的读取与排队逻辑。
//!
//! 启动参数与「第二个实例转发过来的参数」都可能早于前端就绪，因此统一先入队，
//! 前端挂载后通过 `take_pending_sql_files` 主动取走；事件只作为唤醒信号，
//! 避免事件载荷与队列两份数据重复打开同一个文件。

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};

/// 前端直接打开的 SQL 文件载荷
#[derive(Debug, Clone, Serialize)]
pub struct OpenSqlFile {
    pub path: String,
    pub name: String,
    pub content: String,
}

/// 在编辑器中打开的文件大小上限：超过只提示，不把整份文件塞进 IPC 与编辑器
const MAX_SQL_FILE_BYTES: u64 = 20 * 1024 * 1024;

/// 启动阶段 / 单实例转发时前端尚未就绪，先把文件暂存在这里
#[derive(Default)]
pub struct PendingSqlFiles(pub Mutex<Vec<OpenSqlFile>>);

/// 从命令行参数中挑出真实存在的 `.sql` 文件。
///
/// 双击关联文件时系统会把路径作为第一个参数传给可执行文件，
/// 首个参数是程序自身路径，因此统一跳过 `skip(1)`。
pub fn sql_paths_from_args<I: IntoIterator<Item = String>>(args: I) -> Vec<PathBuf> {
    args.into_iter()
        .skip(1)
        .map(PathBuf::from)
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("sql"))
                && path.is_file()
        })
        .collect()
}

/// 读取单个 SQL 文件；非 UTF-8 内容用替换字符兜底，保证文件仍能打开
async fn load_sql_file(path: &Path) -> Result<OpenSqlFile, String> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|e| format!("无法读取文件信息：{e}"))?;
    if metadata.len() > MAX_SQL_FILE_BYTES {
        return Err(format!(
            "文件超过 {} MB，暂不支持在编辑器中打开",
            MAX_SQL_FILE_BYTES / 1024 / 1024
        ));
    }

    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| format!("读取文件失败：{e}"))?;

    Ok(OpenSqlFile {
        path: path.to_string_lossy().into_owned(),
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned()),
        content: String::from_utf8_lossy(&bytes).into_owned(),
    })
}

/// 读取并排队一批 SQL 文件，再发一个事件唤醒前端来取。
///
/// 读取失败的文件只记日志，不影响其它文件打开。
pub async fn enqueue_sql_paths(app: &AppHandle, paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }

    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        match load_sql_file(&path).await {
            Ok(file) => files.push(file),
            Err(e) => tracing::warn!("打开 SQL 文件 {:?} 失败：{}", path, e),
        }
    }
    if files.is_empty() {
        return;
    }

    match app.state::<PendingSqlFiles>().0.lock() {
        Ok(mut pending) => pending.extend(files),
        Err(_) => return,
    }

    if let Err(e) = app.emit("open-sql-file", ()) {
        tracing::warn!("通知前端打开 SQL 文件失败：{}", e);
    }
}

/// 取走所有待打开的 SQL 文件（前端挂载后调用，同时处理事件唤醒）
#[tauri::command]
pub fn take_pending_sql_files(state: State<'_, PendingSqlFiles>) -> Vec<OpenSqlFile> {
    match state.0.lock() {
        Ok(mut pending) => std::mem::take(&mut *pending),
        Err(_) => Vec::new(),
    }
}

/// 拖拽打开：路径由系统拖放事件给出，直接读取内容
#[tauri::command]
pub async fn read_sql_file(path: String) -> Result<OpenSqlFile, String> {
    let path = PathBuf::from(path);
    if !path.is_file() {
        return Err("文件不存在或不可访问".to_string());
    }
    load_sql_file(&path).await
}
