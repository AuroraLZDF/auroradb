//! Linux 更新包下载
//!
//! Tauri 内置更新器只会用 AppImage 原地替换应用，deb 安装的版本无法走这条路；
//! deb 安装改为把更新包下载到系统下载目录，由用户手动安装。

use futures_util::StreamExt;
use serde::Serialize;
use std::path::Path;
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;

/// 下载进度事件名，前端据此刷新「下载 xx%」文案
const PROGRESS_EVENT: &str = "updater://package-progress";

#[derive(Clone, Serialize)]
struct PackageProgress {
    downloaded: u64,
    total: Option<u64>,
}

/// 当前安装形态：
/// - `auto`：内置更新器可原地安装（AppImage，以及非 Linux 平台）
/// - `deb`：deb 安装，只能下载安装包后由用户手动安装
#[tauri::command]
pub fn get_update_install_kind() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        // AppImage 运行时会把自身路径写入 APPIMAGE 环境变量
        let is_appimage = std::env::var_os("APPIMAGE").is_some()
            || std::env::current_exe()
                .map(|exe| exe.to_string_lossy().ends_with(".AppImage"))
                .unwrap_or(false);
        if is_appimage {
            "auto"
        } else {
            "deb"
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        "auto"
    }
}

/// 把更新包下载到系统下载目录并返回落盘路径
#[tauri::command]
pub async fn download_update_package(
    app: AppHandle,
    url: String,
    file_name: String,
) -> Result<String, String> {
    // 清单由发布脚本或人工维护，可能混入反引号/空白，直接请求会失败
    let url = url.trim().trim_matches('`').trim().to_string();
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err(format!("不支持的下载地址: {url}"));
    }

    let dir = dirs::download_dir()
        .or_else(dirs::home_dir)
        .ok_or_else(|| "无法定位下载目录".to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建下载目录失败: {e}"))?;

    // 只取文件名，避免远端传来的名字带路径穿越
    let file_name = Path::new(&file_name)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("非法的文件名: {file_name}"))?
        .to_string();
    let target = dir.join(file_name);

    if let Err(e) = write_package(&app, &url, &target).await {
        // 半截文件会被当成可用安装包，失败时直接清掉
        let _ = tokio::fs::remove_file(&target).await;
        return Err(e);
    }

    // 打开下载目录，方便用户直接双击安装
    let _ = std::process::Command::new("xdg-open").arg(&dir).spawn();

    Ok(target.to_string_lossy().into_owned())
}

async fn write_package(app: &AppHandle, url: &str, target: &Path) -> Result<(), String> {
    // reqwest 用 rustls 但不自带加密后端，需在首次使用前装入默认 provider
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    let response = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map_err(|e| format!("请求更新包失败: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("下载更新包失败: HTTP {}", response.status()));
    }

    let total = response.content_length();
    let mut file = tokio::fs::File::create(target)
        .await
        .map_err(|e| format!("创建文件 {} 失败: {e}", target.display()))?;

    let mut stream = response.bytes_stream();
    let mut downloaded = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("下载中断: {e}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("写入更新包失败: {e}"))?;
        downloaded += chunk.len() as u64;
        let _ = app.emit(
            PROGRESS_EVENT,
            PackageProgress {
                downloaded,
                total,
            },
        );
    }

    file.flush()
        .await
        .map_err(|e| format!("写入更新包失败: {e}"))?;
    Ok(())
}
