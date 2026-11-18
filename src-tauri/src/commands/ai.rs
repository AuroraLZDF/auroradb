//! AI 助手 BYOK（V3-2.1）：OpenAI 兼容端点配置。
//! base_url/model 存应用数据目录的 ai.json；api_key 沿用连接密码的系统密钥环方案，
//! 请求由前端配置的端点直连，不经过任何中间服务。

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use tauri::Manager;

const KEYRING_SERVICE: &str = "com.auroradb.app";
const KEYRING_ENTRY: &str = "ai-api-key";
const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_MODEL: &str = "gpt-4o-mini";

#[derive(Serialize, Deserialize, Clone)]
pub struct AiConfig {
    pub base_url: String,
    pub model: String,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            model: DEFAULT_MODEL.to_string(),
        }
    }
}

#[derive(Serialize)]
pub struct AiConfigView {
    pub base_url: String,
    pub model: String,
    /// 前端只显示「已保存」，不回传明文 Key
    pub has_key: bool,
}

fn config_path(app: &tauri::AppHandle) -> anyhow::Result<PathBuf> {
    let dir = app.path().app_data_dir()?;
    Ok(dir.join("ai.json"))
}

fn load_config(app: &tauri::AppHandle) -> AiConfig {
    let path = match config_path(app) {
        Ok(p) => p,
        Err(_) => return AiConfig::default(),
    };
    fs::read(&path)
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

fn keyring_entry() -> anyhow::Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_ENTRY)
        .context("无法打开系统密钥环条目")
}

fn read_api_key() -> anyhow::Result<Option<String>> {
    match keyring_entry()?.get_password() {
        Ok(k) => Ok(Some(k)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(anyhow::Error::new(e).context("无法从系统密钥环读取 API Key")),
    }
}

/// 读取 AI 配置（Key 不回传明文，只给 has_key 标记）
#[tauri::command]
pub fn ai_get_config(app: tauri::AppHandle) -> Result<AiConfigView, String> {
    let config = load_config(&app);
    let has_key = read_api_key().map_err(|e| e.to_string())?.is_some();
    Ok(AiConfigView {
        base_url: config.base_url,
        model: config.model,
        has_key,
    })
}

/// 保存 AI 配置；api_key 为 None/空串表示不修改已存 Key
#[tauri::command]
pub fn ai_save_config(
    app: tauri::AppHandle,
    base_url: String,
    model: String,
    api_key: Option<String>,
) -> Result<(), String> {
    let base_url = base_url.trim().to_string();
    if !base_url.is_empty() && !base_url.starts_with("http://") && !base_url.starts_with("https://") {
        return Err("Base URL 必须以 http:// 或 https:// 开头".to_string());
    }
    let config = AiConfig {
        base_url: if base_url.is_empty() { DEFAULT_BASE_URL.to_string() } else { base_url },
        model: if model.trim().is_empty() { DEFAULT_MODEL.to_string() } else { model.trim().to_string() },
    };
    let path = config_path(&app).map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(&path, serde_json::to_vec(&config).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;

    if let Some(key) = api_key {
        let key = key.trim().to_string();
        if !key.is_empty() {
            keyring_entry()
                .map_err(|e| e.to_string())?
                .set_password(&key)
                .map_err(|e| format!("无法将 API Key 写入系统密钥环：{e}"))?;
        }
    }
    Ok(())
}

/// 连通性测试：GET {base_url}/models，返回模型 id 列表。
/// api_key 传空则使用已保存的 Key。
#[tauri::command]
pub async fn ai_test_connection(
    base_url: String,
    api_key: Option<String>,
) -> Result<Vec<String>, String> {
    let key = match api_key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()) {
        Some(k) => k,
        None => read_api_key()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "尚未配置 API Key".to_string())?,
    };
    let url = format!("{}/models", base_url.trim_end().trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(&url)
        .bearer_auth(&key)
        .send()
        .await
        .map_err(|e| format!("请求失败：{e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        let brief = body.chars().take(200).collect::<String>();
        return Err(format!("端点返回 {status}：{brief}"));
    }
    #[derive(Deserialize)]
    struct ModelsResp {
        data: Vec<ModelItem>,
    }
    #[derive(Deserialize)]
    struct ModelItem {
        id: String,
    }
    let parsed: ModelsResp = resp
        .json()
        .await
        .map_err(|e| format!("响应解析失败（确认是 OpenAI 兼容端点）：{e}"))?;
    let mut ids: Vec<String> = parsed.data.into_iter().map(|m| m.id).collect();
    ids.sort();
    Ok(ids)
}
