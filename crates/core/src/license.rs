//! 离线许可证（激活码）校验与状态管理。
//!
//! ## 激活码格式
//! `AURORADB1.<base64url(payload_json)>.<base64url(ed25519_signature)>`
//!
//! 签名对象为 payload 段（base64url 字符串）的 ASCII 字节，私钥只存在于发行方的
//! `tools/license-gen`，应用内置公钥做本地验签，因此激活与校验都无需联网。
//!
//! ## 一码一机
//! payload 中的 `fingerprint` 必须与当前机器指纹一致，把激活码复制到其他机器无效。
//! 指纹由 `/etc/machine-id`、DMI `product_uuid` 派生（Windows 用注册表 MachineGuid，
//! 极简系统回退到主机名 + 用户名）。
//!
//! ## 免费版限制
//! 未激活时 `edition = "free"`，`ensure()` 会拒绝专业版功能；基础查询不受影响。

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;

/// 激活码固定前缀，同时标识格式版本
pub const LICENSE_PREFIX: &str = "AURORADB1";
/// payload 结构版本
pub const LICENSE_VERSION: u8 = 1;
/// 产品标识，防止把其他产品的激活码拿来使用
pub const PRODUCT_ID: &str = "auroradb";

/// 专业版功能键（前端 `stores/license.ts` 使用同一组字符串）
pub const FEATURE_SCHEMA_DIFF: &str = "schema-diff";
pub const FEATURE_MONITOR: &str = "monitor";
pub const FEATURE_AUDIT: &str = "audit";
pub const FEATURE_SSH_TUNNEL: &str = "ssh-tunnel";
pub const FEATURE_QUERY_PLAN: &str = "query-plan";
pub const FEATURE_ER_DIAGRAM: &str = "er-diagram";
pub const FEATURE_BACKUP: &str = "backup";
pub const FEATURE_GLOBAL_SEARCH: &str = "global-search";

/// 命令层错误前缀：前端据此识别「需要激活」并弹出激活窗口
pub const LICENSE_REQUIRED_PREFIX: &str = "LICENSE_REQUIRED:";

/// 内置发行公钥（Ed25519，32 字节 hex）。由 `tools/license-gen keygen` 生成，
/// 对应的私钥仅保存在发行方机器上。
const PUBLIC_KEY_HEX: &str = "24849e40c32bab22cdf5a931fbc66fb883ee5aab157b07460ce9b9f66f774cdd";

/// 激活码解码后的负载
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicensePayload {
    pub v: u8,
    pub product: String,
    pub edition: String,
    pub license_id: String,
    pub fingerprint: String,
    pub issued_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub features: Vec<String>,
    /// 多机位授权中的机位序号（从 1 开始）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seat: Option<u32>,
    /// 多机位授权的机位总数
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seats: Option<u32>,
}

/// 返回给前端的授权状态
#[derive(Debug, Clone, Serialize)]
pub struct LicenseStatus {
    /// 是否已激活且校验通过
    pub activated: bool,
    /// `free` / `pro` / `trial`
    pub edition: String,
    pub fingerprint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issued_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    pub features: Vec<String>,
    /// 多机位授权的机位序号（单机授权为 None）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seat: Option<u32>,
    /// 多机位授权的机位总数（单机授权为 None）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seats: Option<u32>,
    /// 已保存激活码但校验失败时的原因（过期、换机、文件损坏等）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl LicenseStatus {
    fn free(fingerprint: String) -> Self {
        Self {
            activated: false,
            edition: "free".to_string(),
            fingerprint,
            license_id: None,
            issued_at: None,
            expires_at: None,
            features: Vec::new(),
            seat: None,
            seats: None,
            error: None,
        }
    }

    fn active(fingerprint: String, payload: &LicensePayload) -> Self {
        Self {
            activated: true,
            edition: payload.edition.clone(),
            fingerprint,
            license_id: Some(payload.license_id.clone()),
            issued_at: Some(payload.issued_at.clone()),
            expires_at: payload.expires_at.clone(),
            features: payload.features.clone(),
            seat: payload.seat,
            seats: payload.seats,
            error: None,
        }
    }

    fn invalid(fingerprint: String, error: String) -> Self {
        Self {
            error: Some(error),
            ..Self::free(fingerprint)
        }
    }
}

/// 许可证状态管理器，作为 Tauri `manage` 状态注入各命令
pub struct LicenseManager {
    path: PathBuf,
    public_key: [u8; 32],
    /// 已保存的激活码原文；内存中保留一份，便于运行时重新校验有效期
    code: RwLock<Option<String>>,
    status: RwLock<LicenseStatus>,
}

impl LicenseManager {
    /// 使用默认存储位置 `<应用数据目录>/mysql-client/license.key`
    pub fn new() -> Self {
        let path = dirs::data_dir()
            .map(|dir| dir.join("mysql-client").join("license.key"))
            .unwrap_or_else(|| PathBuf::from("license.key"));
        let public_key =
            decode_public_key(PUBLIC_KEY_HEX).expect("内置许可证公钥格式错误");
        Self::new_in(path, public_key)
    }

    pub fn new_in(path: impl Into<PathBuf>, public_key: [u8; 32]) -> Self {
        let path = path.into();
        let fingerprint = machine_fingerprint();
        let code = std::fs::read_to_string(&path)
            .ok()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty());

        let status = match code.as_deref() {
            Some(code) => match verify_code(code, &public_key, &fingerprint, Utc::now()) {
                Ok(payload) => LicenseStatus::active(fingerprint, &payload),
                Err(error) => LicenseStatus::invalid(fingerprint, error),
            },
            None => LicenseStatus::free(fingerprint),
        };

        Self {
            path,
            public_key,
            code: RwLock::new(code),
            status: RwLock::new(status),
        }
    }

    /// 当前状态（每次读取都会重新校验，运行中过期能立即生效）
    pub async fn status(&self) -> LicenseStatus {
        self.refresh().await;
        self.status.read().await.clone()
    }

    /// 当前是否具备指定专业版能力
    pub async fn allows(&self, feature: &str) -> bool {
        self.refresh().await;
        let status = self.status.read().await;
        // features 为空表示该授权包含全部专业版能力；trial 同样视为全功能，到期后由 expires_at 失效
        status.activated
            && (status.edition == "pro" || status.edition == "trial")
            && (status.features.is_empty()
                || status.features.iter().any(|item| item == "*" || item == feature))
    }

    /// 专业版功能门禁：未授权时返回可被前端识别的错误
    pub async fn ensure(&self, feature: &str, label: &str) -> Result<(), String> {
        if self.allows(feature).await {
            return Ok(());
        }
        Err(format!(
            "{}「{}」属于专业版功能，请先在「激活」中输入激活码（feature={}）",
            LICENSE_REQUIRED_PREFIX, label, feature
        ))
    }

    /// 激活：验签 + 绑定机器 + 落盘
    pub async fn activate(&self, code: &str) -> Result<LicenseStatus, String> {
        let code = code.trim();
        if code.is_empty() {
            return Err("请输入激活码".to_string());
        }
        let fingerprint = machine_fingerprint();
        let payload = verify_code(code, &self.public_key, &fingerprint, Utc::now())?;
        write_code(&self.path, code)?;

        let status = LicenseStatus::active(fingerprint, &payload);
        *self.code.write().await = Some(code.to_string());
        *self.status.write().await = status.clone();
        Ok(status)
    }

    /// 停用：删除本地激活码，回到免费版
    pub async fn deactivate(&self) -> Result<(), String> {
        match std::fs::remove_file(&self.path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("删除激活信息失败：{}", e)),
        }
        *self.code.write().await = None;
        *self.status.write().await = LicenseStatus::free(machine_fingerprint());
        Ok(())
    }

    async fn refresh(&self) {
        let fingerprint = machine_fingerprint();
        let code = self.code.read().await.clone();
        let next = match code.as_deref() {
            Some(code) => match verify_code(code, &self.public_key, &fingerprint, Utc::now()) {
                Ok(payload) => LicenseStatus::active(fingerprint, &payload),
                Err(error) => LicenseStatus::invalid(fingerprint, error),
            },
            None => LicenseStatus::free(fingerprint),
        };
        *self.status.write().await = next;
    }
}

impl Default for LicenseManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 机器指纹：`XXXX-XXXX-XXXX-XXXX`，由机器标识做 SHA-256 后取前 8 字节
pub fn machine_fingerprint() -> String {
    fingerprint_from_parts(&machine_identity_parts())
}

/// 采集当前平台的机器标识片段。
///
/// 优先使用不可伪造的唯一标识（Linux 的 machine-id/DMI UUID、Windows 的注册表 MachineGuid）；
/// 极简环境读不到时退回主机名/用户名等组合，避免所有机器得到同一指纹。
fn machine_identity_parts() -> Vec<String> {
    #[cfg(windows)]
    {
        windows_identity_parts()
    }
    #[cfg(not(windows))]
    {
        unix_identity_parts()
    }
}

#[cfg(not(windows))]
fn unix_identity_parts() -> Vec<String> {
    let mut parts = Vec::new();
    // 依次纳入可用的机器标识：Linux machine-id 与主板 UUID 组合后换机可感知
    for path in [
        "/etc/machine-id",
        "/var/lib/dbus/machine-id",
        "/sys/class/dmi/id/product_uuid",
    ] {
        if let Ok(raw) = std::fs::read_to_string(path) {
            let value = raw.trim();
            if !value.is_empty() {
                parts.push(value.to_string());
            }
        }
    }

    // 兜底：极简系统上没有任何机器标识时，用主机名 + 用户名避免所有机器得到同一指纹
    if parts.is_empty() {
        let host = std::env::var("HOSTNAME")
            .ok()
            .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
            .unwrap_or_default();
        let user = std::env::var("USER").unwrap_or_default();
        parts.push(format!("{}|{}|auroradb", host.trim(), user));
    }
    parts
}

/// Windows 机器标识：优先注册表 `MachineGuid`，读不到再退回计算机名/用户名/CPU 标识。
#[cfg(windows)]
fn windows_identity_parts() -> Vec<String> {
    // MachineGuid 在系统安装时生成，重装系统才会变化，是 Windows 上最稳定的机器标识
    if let Some(guid) = windows_machine_guid() {
        return vec![guid];
    }
    // Windows 没有 HOSTNAME/USER，改用系统自带的环境变量
    ["COMPUTERNAME", "USERNAME", "PROCESSOR_IDENTIFIER"]
        .iter()
        .map(|key| std::env::var(key).unwrap_or_default())
        .filter(|value| !value.trim().is_empty())
        .collect()
}

#[cfg(windows)]
fn windows_machine_guid() -> Option<String> {
    use std::os::windows::process::CommandExt;

    // CREATE_NO_WINDOW：reg.exe 是控制台程序，不加该标志会闪出黑窗口
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = std::process::Command::new("reg")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Cryptography",
            "/v",
            "MachineGuid",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_reg_sz(&String::from_utf8_lossy(&output.stdout))
}

/// 从 `reg query` 输出中解析 `REG_SZ` 值（单独拆出便于跨平台单测）。
#[cfg(any(windows, test))]
fn parse_reg_sz(output: &str) -> Option<String> {
    // 输出形如：`    MachineGuid    REG_SZ    1234abcd-...`
    output.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        if fields.next()? != "MachineGuid" {
            return None;
        }
        if fields.next()? != "REG_SZ" {
            return None;
        }
        fields.next().map(str::to_string)
    })
}

/// 由机器标识片段派生指纹：同一组片段总是得到同一指纹。
///
/// 空白片段会被忽略；空输入也有确定性兜底，保证指纹稳定且不会 panic。
fn fingerprint_from_parts(parts: &[String]) -> String {
    let mut hasher = Sha256::new();
    let mut hashed_any = false;
    for part in parts {
        let value = part.trim();
        if value.is_empty() {
            continue;
        }
        hasher.update(value.as_bytes());
        hasher.update(b"|");
        hashed_any = true;
    }
    if !hashed_any {
        // 兜底：极简系统连主机名/用户名都读不到时，仍然给出确定性指纹
        hasher.update(b"auroradb-empty");
    }

    let digest = hasher.finalize();
    let hex: String = digest[..8].iter().map(|byte| format!("{:02X}", byte)).collect();
    format!(
        "{}-{}-{}-{}",
        &hex[0..4],
        &hex[4..8],
        &hex[8..12],
        &hex[12..16]
    )
}

/// 校验激活码并返回负载；`now` 便于测试过期逻辑
fn verify_code(
    code: &str,
    public_key: &[u8; 32],
    fingerprint: &str,
    now: DateTime<Utc>,
) -> Result<LicensePayload, String> {
    let parts: Vec<&str> = code.trim().split('.').collect();
    if parts.len() != 3 {
        return Err("激活码格式不正确".to_string());
    }
    if parts[0] != LICENSE_PREFIX {
        return Err("激活码版本不受支持，请升级客户端后再试".to_string());
    }

    let payload_bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|_| "激活码内容损坏".to_string())?;
    let signature_bytes = URL_SAFE_NO_PAD
        .decode(parts[2])
        .map_err(|_| "激活码签名损坏".to_string())?;
    let signature =
        Signature::from_slice(&signature_bytes).map_err(|_| "激活码签名格式错误".to_string())?;
    let key = VerifyingKey::from_bytes(public_key).map_err(|_| "内置公钥无效".to_string())?;
    key.verify_strict(parts[1].as_bytes(), &signature)
        .map_err(|_| "激活码校验失败：签名无效".to_string())?;

    let payload: LicensePayload =
        serde_json::from_slice(&payload_bytes).map_err(|_| "激活码内容无法解析".to_string())?;

    if payload.v != LICENSE_VERSION {
        return Err("激活码版本与当前客户端不匹配".to_string());
    }
    if payload.product != PRODUCT_ID {
        return Err("该激活码不属于 AuroraDB".to_string());
    }
    // pro 为买断/年度授权，trial 为全功能试用；trial 同样依赖 expires_at 到期失效
    if payload.edition != "pro" && payload.edition != "trial" {
        return Err("激活码类型不受支持".to_string());
    }
    if normalize_fingerprint(&payload.fingerprint) != normalize_fingerprint(fingerprint) {
        return Err(format!(
            "激活码与当前机器不匹配，本机机器码为 {}",
            fingerprint
        ));
    }
    if let Some(expires_at) = payload.expires_at.as_deref() {
        let deadline = DateTime::parse_from_rfc3339(expires_at)
            .map_err(|_| "激活码有效期格式错误".to_string())?
            .with_timezone(&Utc);
        if now > deadline {
            return Err(format!("激活码已于 {} 过期", expires_at));
        }
    }

    Ok(payload)
}

fn normalize_fingerprint(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase()
}

fn decode_public_key(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }
    let mut bytes = [0u8; 32];
    for (index, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let text = std::str::from_utf8(chunk).ok()?;
        bytes[index] = u8::from_str_radix(text, 16).ok()?;
    }
    Some(bytes)
}

/// 写入激活码（目录 0700、文件 0600，避免其他用户读取）
fn write_code(path: &Path, code: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建许可证目录失败：{}", e))?;
    }

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    use std::io::Write;
    let mut file = options
        .open(path)
        .map_err(|e| format!("写入激活信息失败：{}", e))?;
    file.write_all(code.as_bytes())
        .map_err(|e| format!("写入激活信息失败：{}", e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use ed25519_dalek::{Signer, SigningKey};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn test_key() -> (SigningKey, [u8; 32]) {
        let signing = SigningKey::from_bytes(&[42u8; 32]);
        let public = signing.verifying_key().to_bytes();
        (signing, public)
    }

    fn payload(fingerprint: &str, expires_at: Option<String>) -> LicensePayload {
        LicensePayload {
            v: LICENSE_VERSION,
            product: PRODUCT_ID.to_string(),
            edition: "pro".to_string(),
            license_id: "AD-TEST".to_string(),
            fingerprint: fingerprint.to_string(),
            issued_at: "2026-01-01T00:00:00Z".to_string(),
            expires_at,
            features: vec![FEATURE_SCHEMA_DIFF.to_string()],
            seat: None,
            seats: None,
        }
    }

    fn encode(signing: &SigningKey, payload: &LicensePayload) -> String {
        let json = serde_json::to_vec(payload).unwrap();
        let payload_b64 = URL_SAFE_NO_PAD.encode(&json);
        let signature = signing.sign(payload_b64.as_bytes());
        format!(
            "{}.{}.{}",
            LICENSE_PREFIX,
            payload_b64,
            URL_SAFE_NO_PAD.encode(signature.to_bytes())
        )
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-06-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn accepts_valid_code() {
        let (signing, public) = test_key();
        let code = encode(
            &signing,
            &payload("ABCD-EF01-2345-6789", Some("2027-01-01T00:00:00Z".to_string())),
        );
        let verified = verify_code(&code, &public, "abcd-ef01-2345-6789", now()).unwrap();
        assert_eq!(verified.license_id, "AD-TEST");
        assert_eq!(verified.features, vec![FEATURE_SCHEMA_DIFF.to_string()]);
    }

    #[test]
    fn rejects_tampered_payload() {
        let (signing, public) = test_key();
        let code = encode(&signing, &payload("ABCD-EF01-2345-6789", None));
        let parts: Vec<&str> = code.split('.').collect();
        // 修改 payload 但保留原签名
        let tampered_json = serde_json::to_vec(&payload("FFFF-FFFF-FFFF-FFFF", None)).unwrap();
        let tampered = format!(
            "{}.{}.{}",
            parts[0],
            URL_SAFE_NO_PAD.encode(tampered_json),
            parts[2]
        );
        let error = verify_code(&tampered, &public, "FFFF-FFFF-FFFF-FFFF", now()).unwrap_err();
        assert!(error.contains("签名无效"), "got: {error}");
    }

    #[test]
    fn rejects_other_machine() {
        let (signing, public) = test_key();
        let code = encode(&signing, &payload("ABCD-EF01-2345-6789", None));
        let error = verify_code(&code, &public, "0000-0000-0000-0000", now()).unwrap_err();
        assert!(error.contains("机器不匹配"), "got: {error}");
        assert!(error.contains("0000-0000-0000-0000"), "got: {error}");
    }

    #[test]
    fn rejects_expired_code() {
        let (signing, public) = test_key();
        let code = encode(
            &signing,
            &payload("ABCD-EF01-2345-6789", Some("2026-05-31T23:59:59Z".to_string())),
        );
        let error = verify_code(&code, &public, "ABCD-EF01-2345-6789", now()).unwrap_err();
        assert!(error.contains("过期"), "got: {error}");
    }

    #[tokio::test]
    async fn trial_code_activates_and_allows_all_features() {
        let (signing, public) = test_key();
        let path = temp_path();
        // 试用版为全功能：features 留空表示全部能力，到期由 expires_at 控制
        let trial = LicensePayload {
            edition: "trial".to_string(),
            features: Vec::new(),
            ..payload(
                &machine_fingerprint(),
                Some("2999-01-01T00:00:00Z".to_string()),
            )
        };
        let code = encode(&signing, &trial);

        let manager = LicenseManager::new_in(&path, public);
        let status = manager.activate(&code).await.unwrap();
        assert!(status.activated);
        assert_eq!(status.edition, "trial");
        assert!(manager.allows(FEATURE_MONITOR).await);
        assert!(manager.allows(FEATURE_SCHEMA_DIFF).await);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_expired_trial_code() {
        let (signing, public) = test_key();
        let trial = LicensePayload {
            edition: "trial".to_string(),
            features: Vec::new(),
            ..payload(
                "ABCD-EF01-2345-6789",
                Some("2026-05-31T23:59:59Z".to_string()),
            )
        };
        let code = encode(&signing, &trial);
        let error = verify_code(&code, &public, "ABCD-EF01-2345-6789", now()).unwrap_err();
        assert!(error.contains("过期"), "got: {error}");
    }

    #[test]
    fn parses_seat_fields_and_omits_them_when_absent() {
        let (signing, public) = test_key();
        let seat_payload = LicensePayload {
            seat: Some(2),
            seats: Some(5),
            ..payload("ABCD-EF01-2345-6789", None)
        };
        let verified = verify_code(
            &encode(&signing, &seat_payload),
            &public,
            "ABCD-EF01-2345-6789",
            now(),
        )
        .unwrap();
        assert_eq!(verified.seat, Some(2));
        assert_eq!(verified.seats, Some(5));

        // 未设置时序列化不出现 seat/seats，旧码结构保持不变
        let old = payload("ABCD-EF01-2345-6789", None);
        let json = serde_json::to_value(&old).unwrap();
        assert!(json.get("seat").is_none());
        assert!(json.get("seats").is_none());
        let verified = verify_code(
            &encode(&signing, &old),
            &public,
            "ABCD-EF01-2345-6789",
            now(),
        )
        .unwrap();
        assert_eq!(verified.seat, None);
        assert_eq!(verified.seats, None);
    }

    #[test]
    fn old_pro_code_without_new_fields_still_parses() {
        let (signing, public) = test_key();
        // 旧版 license-gen 生成的 JSON：没有 seat/seats 字段，serde(default) 应解析为 None
        let json = r#"{"v":1,"product":"auroradb","edition":"pro","license_id":"AD-OLD","fingerprint":"ABCD-EF01-2345-6789","issued_at":"2026-01-01T00:00:00Z","features":["schema-diff"]}"#;
        let payload_b64 = URL_SAFE_NO_PAD.encode(json);
        let signature = signing.sign(payload_b64.as_bytes());
        let code = format!(
            "{}.{}.{}",
            LICENSE_PREFIX,
            payload_b64,
            URL_SAFE_NO_PAD.encode(signature.to_bytes())
        );

        let verified = verify_code(&code, &public, "ABCD-EF01-2345-6789", now()).unwrap();
        assert_eq!(verified.edition, "pro");
        assert_eq!(verified.license_id, "AD-OLD");
        assert_eq!(verified.seat, None);
        assert_eq!(verified.seats, None);
        assert_eq!(verified.features, vec![FEATURE_SCHEMA_DIFF.to_string()]);
    }

    #[test]
    fn rejects_malformed_code() {
        let (_, public) = test_key();
        assert!(verify_code("not-a-code", &public, "AAAA", now()).is_err());
        assert!(verify_code("AURORA2.abc.def", &public, "AAAA", now()).is_err());
    }

    #[test]
    fn fingerprint_shape_is_stable() {
        let fingerprint = machine_fingerprint();
        assert_eq!(fingerprint.len(), 19, "got: {fingerprint}");
        assert_eq!(fingerprint.matches('-').count(), 3, "got: {fingerprint}");
        assert_eq!(fingerprint, machine_fingerprint());
    }

    #[test]
    fn fingerprint_from_parts_is_stable_and_formatted() {
        let parts = vec!["machine-a".to_string(), "board-b".to_string()];
        let fingerprint = fingerprint_from_parts(&parts);
        assert_eq!(fingerprint, fingerprint_from_parts(&parts));
        assert_eq!(fingerprint.len(), 19, "got: {fingerprint}");
        assert_eq!(fingerprint.matches('-').count(), 3, "got: {fingerprint}");
        assert!(
            fingerprint
                .chars()
                .all(|c| c.is_ascii_hexdigit() || c == '-'),
            "got: {fingerprint}"
        );
    }

    #[test]
    fn fingerprint_from_parts_differs_by_identity() {
        let a = fingerprint_from_parts(&["machine-a".to_string()]);
        let b = fingerprint_from_parts(&["machine-b".to_string()]);
        assert_ne!(a, b);
    }

    #[test]
    fn fingerprint_from_parts_empty_is_deterministic() {
        let empty = fingerprint_from_parts(&[]);
        assert_eq!(empty, fingerprint_from_parts(&[]));
        assert_eq!(empty.len(), 19, "got: {empty}");
        // 空白片段与空输入等价，避免环境变量为空时指纹抖动
        assert_eq!(
            empty,
            fingerprint_from_parts(&[" ".to_string(), "\t".to_string()])
        );
        // 空输入兜底不能与真实机器标识得到同一个指纹
        assert_ne!(empty, fingerprint_from_parts(&["machine-a".to_string()]));
    }

    #[test]
    fn parse_reg_sz_reads_machine_guid() {
        let output = "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Cryptography\r\n    MachineGuid    REG_SZ    3f2a1b4c-5d6e-7f80-9a1b-2c3d4e5f6a7b\r\n\r\n";
        assert_eq!(
            parse_reg_sz(output).as_deref(),
            Some("3f2a1b4c-5d6e-7f80-9a1b-2c3d4e5f6a7b")
        );
        assert_eq!(parse_reg_sz("MachineGuid    REG_DWORD    0x1"), None);
        assert_eq!(parse_reg_sz(""), None);
    }

    fn temp_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "auroradb-license-{}-{}.key",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[tokio::test]
    async fn activate_persists_and_deactivate_clears() {
        let (signing, public) = test_key();
        let path = temp_path();
        let _ = std::fs::remove_file(&path);

        let manager = LicenseManager::new_in(&path, public);
        assert!(!manager.status().await.activated);

        let code = encode(
            &signing,
            &payload(&machine_fingerprint(), Some("2999-01-01T00:00:00Z".to_string())),
        );
        let status = manager.activate(&code).await.unwrap();
        assert!(status.activated);
        assert_eq!(status.edition, "pro");
        assert!(path.exists());
        assert!(manager.allows(FEATURE_SCHEMA_DIFF).await);
        assert!(!manager.allows(FEATURE_MONITOR).await);

        // 重新加载（模拟重启）后仍处于激活态
        let reloaded = LicenseManager::new_in(&path, public);
        assert!(reloaded.status().await.activated);

        reloaded.deactivate().await.unwrap();
        assert!(!reloaded.status().await.activated);
        assert!(!path.exists());

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn expired_saved_code_falls_back_to_free() {
        let (signing, public) = test_key();
        let path = temp_path();

        let code = encode(
            &signing,
            &payload(
                &machine_fingerprint(),
                Some((Utc::now() - Duration::days(1)).to_rfc3339()),
            ),
        );
        std::fs::write(&path, &code).unwrap();

        let manager = LicenseManager::new_in(&path, public);
        let status = manager.status().await;
        assert!(!status.activated);
        assert_eq!(status.edition, "free");
        assert!(status.error.as_deref().unwrap_or_default().contains("过期"));
        assert!(manager.ensure(FEATURE_MONITOR, "实例监控").await.is_err());

        let _ = std::fs::remove_file(&path);
    }
}
