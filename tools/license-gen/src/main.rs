//! AuroraDB 离线激活码签发工具（仅发行方使用，不随应用分发）
//!
//! 用法：
//!   license-gen keygen [--out <私钥路径>] [--force]
//!   license-gen sign --fingerprint <机器码> [--key <私钥路径>] [--license-id <编号>]
//!                    [--days <有效天数，0=永久>] [--edition pro]
//!                    [--features schema-diff,monitor,audit,import-export,ssh-tunnel]
//!
//! 激活码格式：`AURORADB1.<base64url(payload_json)>.<base64url(ed25519_signature)>`
//! 签名对象为 payload 段（base64url 字符串）的 ASCII 字节。

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{Duration, SecondsFormat, Utc};
use ed25519_dalek::{Signer, SigningKey};
use serde::Serialize;
use std::path::PathBuf;

const LICENSE_PREFIX: &str = "AURORADB1";
const PRODUCT_ID: &str = "auroradb";
const DEFAULT_FEATURES: &str = "schema-diff,monitor,audit,import-export,ssh-tunnel";

#[derive(Serialize)]
struct LicensePayload {
    v: u8,
    product: String,
    edition: String,
    license_id: String,
    fingerprint: String,
    issued_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at: Option<String>,
    features: Vec<String>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(command) = args.get(1) else {
        print_usage();
        std::process::exit(2);
    };

    let result = match command.as_str() {
        "keygen" => keygen(&args[2..]),
        "sign" => sign(&args[2..]),
        "-h" | "--help" | "help" => {
            print_usage();
            Ok(())
        }
        other => Err(format!("未知子命令：{}", other)),
    };

    if let Err(message) = result {
        eprintln!("错误：{}", message);
        std::process::exit(1);
    }
}

fn print_usage() {
    eprintln!(
        "AuroraDB 激活码签发工具\n\n\
         生成密钥对（私钥务必离线保管，切勿提交到代码仓库）：\n  \
         license-gen keygen [--out <路径>] [--force]\n\n\
         签发激活码：\n  \
         license-gen sign --fingerprint <机器码> [--key <私钥路径>] [--license-id <编号>]\n                   \
         [--days <有效天数，0=永久>] [--edition pro] [--features <逗号分隔的功能键>]"
    );
}

fn default_key_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".config")
        .join("auroradb")
        .join("license.key")
}

fn arg_value(args: &[String], key: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == key)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

fn keygen(args: &[String]) -> Result<(), String> {
    let path = arg_value(args, "--out")
        .map(PathBuf::from)
        .unwrap_or_else(default_key_path);

    if path.exists() && !has_flag(args, "--force") {
        return Err(format!(
            "私钥已存在：{}。如确需覆盖请加 --force（覆盖后已签发的激活码仍可用，但新码必须用新私钥签）",
            path.display()
        ));
    }

    let mut rng = rand::rngs::OsRng;
    let signing = SigningKey::generate(&mut rng);
    let secret_hex = hex_encode(&signing.to_bytes());
    let public_hex = hex_encode(&signing.verifying_key().to_bytes());

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败：{}", e))?;
    }
    std::fs::write(&path, format!("{}\n", secret_hex))
        .map_err(|e| format!("写入私钥失败：{}", e))?;
    restrict_permissions(&path)?;

    println!("私钥已写入：{}", path.display());
    println!("公钥（填入 src-tauri/src/core/license.rs 的 PUBLIC_KEY_HEX）：");
    println!("{}", public_hex);
    Ok(())
}

fn sign(args: &[String]) -> Result<(), String> {
    let fingerprint = arg_value(args, "--fingerprint")
        .ok_or_else(|| "缺少 --fingerprint（客户端「激活」窗口中显示的机器码）".to_string())?;
    let key_path = arg_value(args, "--key")
        .map(PathBuf::from)
        .unwrap_or_else(default_key_path);

    let secret_hex = std::fs::read_to_string(&key_path)
        .map_err(|e| format!("读取私钥失败（{}）：{}", key_path.display(), e))?;
    let secret = hex_decode(secret_hex.trim())?;
    let secret: [u8; 32] = secret
        .try_into()
        .map_err(|_| "私钥长度错误，应为 32 字节".to_string())?;
    let signing = SigningKey::from_bytes(&secret);

    let days: i64 = arg_value(args, "--days")
        .unwrap_or_else(|| "365".to_string())
        .parse()
        .map_err(|_| "--days 必须是整数".to_string())?;
    let issued_at = Utc::now();
    let expires_at = if days <= 0 {
        None
    } else {
        Some(
            (issued_at + Duration::days(days))
                .to_rfc3339_opts(SecondsFormat::Secs, true),
        )
    };

    let license_id = arg_value(args, "--license-id")
        .unwrap_or_else(|| format!("AD-{}", issued_at.format("%Y%m%d%H%M%S")));
    let features: Vec<String> = arg_value(args, "--features")
        .unwrap_or_else(|| DEFAULT_FEATURES.to_string())
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect();

    let payload = LicensePayload {
        v: 1,
        product: PRODUCT_ID.to_string(),
        edition: arg_value(args, "--edition").unwrap_or_else(|| "pro".to_string()),
        license_id,
        fingerprint: normalize_fingerprint(&fingerprint),
        issued_at: issued_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        expires_at,
        features,
    };

    let json = serde_json::to_vec(&payload).map_err(|e| format!("序列化失败：{}", e))?;
    let payload_b64 = URL_SAFE_NO_PAD.encode(&json);
    let signature = signing.sign(payload_b64.as_bytes());
    let signature_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());

    println!("{}.{}.{}", LICENSE_PREFIX, payload_b64, signature_b64);
    Ok(())
}

fn normalize_fingerprint(value: &str) -> String {
    let compact: String = value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase();
    if compact.len() == 16 {
        format!(
            "{}-{}-{}-{}",
            &compact[0..4],
            &compact[4..8],
            &compact[8..12],
            &compact[12..16]
        )
    } else {
        compact
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn hex_decode(text: &str) -> Result<Vec<u8>, String> {
    if text.len() % 2 != 0 {
        return Err("私钥 hex 长度必须是偶数".to_string());
    }
    (0..text.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&text[index..index + 2], 16)
                .map_err(|_| "私钥不是合法的 hex".to_string())
        })
        .collect()
}

#[cfg(unix)]
fn restrict_permissions(path: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("设置私钥权限失败：{}", e))
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &std::path::Path) -> Result<(), String> {
    Ok(())
}
