//! 许可证激活命令：状态读取、激活与停用

use auroradb_core::license::{LicenseManager, LicenseStatus};
use tauri::{command, State};

/// 读取授权状态；已保存的激活码会重新验签与检查有效期
#[command]
pub async fn get_license_status(license: State<'_, LicenseManager>) -> Result<LicenseStatus, String> {
    Ok(license.status().await)
}

/// 输入激活码完成激活（一码一机，验签在本机离线完成）
#[command]
pub async fn activate_license(
    license: State<'_, LicenseManager>,
    code: String,
) -> Result<LicenseStatus, String> {
    license.activate(&code).await
}

/// 停用当前激活码，回到免费版
#[command]
pub async fn deactivate_license(license: State<'_, LicenseManager>) -> Result<(), String> {
    license.deactivate().await
}
