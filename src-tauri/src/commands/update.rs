//! 更新包下载
//!
//! Tauri 内置更新器能在 AppImage 上原地替换、在 Windows 上执行 NSIS 静默升级；
//! deb 安装的版本无法走这条路，改为把更新包下载到系统下载目录，由用户手动安装。

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
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

/// 把更新包下载到系统下载目录，校验 SHA-256 后返回落盘路径
///
/// `sha256` 来自更新清单（由 tools/merge-latest.py 写入 `deb.sha256`）：
/// - 提供了校验值：下载完成后计算文件摘要并比对，不一致删除文件并报错；
/// - 未提供校验值：记录告警但不阻断，兼容尚未升级的旧清单。
#[tauri::command]
pub async fn download_update_package(
    app: AppHandle,
    url: String,
    file_name: String,
    sha256: Option<String>,
) -> Result<String, String> {
    // 清单由发布脚本或人工维护，可能混入反引号/空白，直接请求会失败
    let url = url.trim().trim_matches('`').trim().to_string();
    // 只允许 HTTPS：http 可被中间人替换成带毒安装包，落盘后用户会直接双击安装
    if !url.to_ascii_lowercase().starts_with("https://") {
        return Err(format!("不支持的下载地址（仅允许 https://）: {url}"));
    }

    let expected = match sha256.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        Some(raw) => {
            let digest = normalize_sha256(raw);
            if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(format!("更新清单提供的 SHA-256 格式非法: {raw}"));
            }
            Some(digest)
        }
        None => {
            tracing::warn!(
                %url,
                "更新清单未提供 sha256，本次下载无法校验安装包完整性（为兼容旧清单暂不阻断）"
            );
            None
        }
    };

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

    let actual = match write_package(&app, &url, &target).await {
        Ok(digest) => digest,
        Err(e) => {
            // 半截文件会被当成可用安装包，失败时直接清掉
            let _ = tokio::fs::remove_file(&target).await;
            return Err(e);
        }
    };

    if let Some(expected) = &expected {
        if !actual.eq_ignore_ascii_case(expected) {
            // 哈希不一致说明文件被篡改或下载不完整，删除后拒绝继续，绝不引导用户安装
            let _ = tokio::fs::remove_file(&target).await;
            return Err(format!(
                "更新包 SHA-256 校验失败（期望 {expected}，实际 {actual}），已删除下载文件"
            ));
        }
        tracing::info!(sha256 = %actual, "更新包 SHA-256 校验通过");
    }

    // 打开下载目录，方便用户直接双击安装
    open_directory(&dir);

    Ok(target.to_string_lossy().into_owned())
}

/// 规范化清单中的 SHA-256：去掉 `sha256:` 前缀与所有空白（含换行），统一小写
fn normalize_sha256(raw: &str) -> String {
    let compact: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    let lower = compact.to_ascii_lowercase();
    lower.strip_prefix("sha256:").unwrap_or(&lower).to_string()
}

/// 在系统文件管理器中打开目录（各平台命令不同，失败不影响下载结果）
fn open_directory(dir: &Path) {
    #[cfg(target_os = "linux")]
    let program = "xdg-open";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(target_os = "windows")]
    let program = "explorer";

    let _ = std::process::Command::new(program).arg(dir).spawn();
}

/// 流式写入更新包，同时计算 SHA-256，返回小写十六进制摘要
async fn write_package(app: &AppHandle, url: &str, target: &Path) -> Result<String, String> {
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
    // 边写边算摘要，避免下载完成后把整个安装包再读一遍
    let mut hasher = Sha256::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("下载中断: {e}"))?;
        hasher.update(&chunk);
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
    Ok(format!("{:x}", hasher.finalize()))
}
