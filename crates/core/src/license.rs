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
//! ## 本机自助试用
//! 未激活时用户可以开始一次 14 天全功能试用，记录写入 `trial.state`，并在配置目录
//! 另存一份 `trial.ledger` 防止通过删除记录重置试用。记录用 HMAC-SHA256 绑定机器指纹，
//! 并检测系统时间回拨：回拨一次即永久失效。
//!
//! ## 免费版限制
//! 未激活且没有有效试用时 `edition = "free"`，`ensure()` 会拒绝专业版功能；基础查询不受影响。

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
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

/// 自助试用天数
pub const TRIAL_DAYS: i64 = 14;
/// 与 license.key 同目录的试用主记录文件（锚点 A）
const TRIAL_STATE_FILE: &str = "trial.state";
/// 配置目录下的防删除台账（锚点 B）
const TRIAL_LEDGER_FILE: &str = "trial.ledger";
/// 缓存目录下的隐藏 marker（锚点 C）
const TRIAL_MARKER_FILE: &str = "trial.marker";
/// 系统级锚点（Unix）：全局临时目录，权限不足时静默跳过
#[cfg(not(windows))]
const TRIAL_SYSTEM_FILE: &str = "/var/tmp/.auroradb-trial";
/// 试用记录格式版本
const TRIAL_RECORD_VERSION: u8 = 1;
/// HMAC 密钥派生前缀；修改后旧记录一律失效，需随协议升级一起变更
const TRIAL_HMAC_PREFIX: &str = "auroradb-trial-v1|";
/// 允许的时钟误差（秒），超出则判定为时间回拨
const TRIAL_ROLLBACK_TOLERANCE_SECS: i64 = 300;
/// last_seen_at 落盘节流（秒），避免频繁写盘
const TRIAL_LAST_SEEN_THROTTLE_SECS: i64 = 60;

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
    /// 本机是否还可以开始自助试用（已激活或试用已用过时为 false）
    pub trial_available: bool,
    /// 是否正处于有效的自助试用期内
    pub trial_active: bool,
    /// 自助试用是否已到期
    pub trial_expired: bool,
    /// 自助试用开始时间
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trial_started_at: Option<String>,
    /// 自助试用到期时间
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trial_expires_at: Option<String>,
    /// 试用剩余天数（1..=14，向上取整；非试用状态为 None）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trial_days_left: Option<u32>,
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
            trial_available: false,
            trial_active: false,
            trial_expired: false,
            trial_started_at: None,
            trial_expires_at: None,
            trial_days_left: None,
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
            trial_available: false,
            trial_active: false,
            trial_expired: false,
            trial_started_at: None,
            trial_expires_at: None,
            trial_days_left: None,
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
    /// 与 license.key 同目录的试用主记录（锚点 A；删除后可从其它锚点恢复）
    trial_state_path: PathBuf,
    /// 多处试用记录锚点，顺序即恢复优先级
    trial_anchors: TrialAnchors,
    public_key: [u8; 32],
    /// 已保存的激活码原文；内存中保留一份，便于运行时重新校验有效期
    code: RwLock<Option<String>>,
    status: RwLock<LicenseStatus>,
}

impl LicenseManager {
    /// 使用默认存储位置 `<应用数据目录>/mysql-client/license.key`；
    /// 试用记录分散在数据/配置/缓存/状态目录与系统级位置共 5 处，任一处幸存即可恢复
    pub fn new() -> Self {
        let path = dirs::data_dir()
            .map(|dir| dir.join("mysql-client").join("license.key"))
            .unwrap_or_else(|| PathBuf::from("license.key"));
        let mut anchors = TrialAnchors::default();
        // A：与 license.key 同目录的主记录
        if let Some(parent) = path.parent() {
            anchors.push_file(parent.join(TRIAL_STATE_FILE));
        }
        // B：配置目录台账
        if let Some(dir) = dirs::config_dir() {
            anchors.push_file(dir.join("mysql-client").join(TRIAL_LEDGER_FILE));
        }
        // C：缓存目录 marker
        if let Some(dir) = dirs::cache_dir() {
            anchors.push_file(dir.join("mysql-client").join(TRIAL_MARKER_FILE));
        }
        // D：XDG 状态目录（无该目录的平台自动跳过）
        if let Some(dir) = dirs::state_dir() {
            anchors.push_file(dir.join("mysql-client").join(TRIAL_STATE_FILE));
        }
        // E：系统级锚点（Unix 固定文件；Windows 注册表）
        anchors.push_system_anchor();
        let public_key = decode_public_key(PUBLIC_KEY_HEX).expect("内置许可证公钥格式错误");
        Self::with_anchors(path, anchors, public_key)
    }

    /// 测试用构造：5 处锚点全部收拢到 license.key 所在目录附近，
    /// 系统级锚点也用测试目录模拟，避免污染真实 /var/tmp 或注册表
    pub fn new_in(path: impl Into<PathBuf>, public_key: [u8; 32]) -> Self {
        let path = path.into();
        let parent = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let mut anchors = TrialAnchors::default();
        anchors.push_file(parent.join(TRIAL_STATE_FILE));
        anchors.push_file(parent.join(TRIAL_LEDGER_FILE));
        anchors.push_file(parent.join(TRIAL_MARKER_FILE));
        anchors.push_file(parent.join("state").join(TRIAL_STATE_FILE));
        anchors.push_file(parent.join("trial.system"));
        Self::with_anchors(path, anchors, public_key)
    }

    fn with_anchors(path: PathBuf, trial_anchors: TrialAnchors, public_key: [u8; 32]) -> Self {
        let trial_state_path = trial_anchors
            .file_paths()
            .into_iter()
            .next()
            .unwrap_or_else(|| path.with_file_name(TRIAL_STATE_FILE));
        let fingerprint = machine_fingerprint();
        let code = std::fs::read_to_string(&path)
            .ok()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty());
        // 初始化时就纳入试用状态，避免首次启动漏报 trial_available
        let status = compute_status(
            code.as_deref(),
            &public_key,
            &fingerprint,
            &trial_anchors,
            Utc::now(),
        );
        Self {
            path,
            trial_state_path,
            trial_anchors,
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

    /// 指定时间点的状态，供测试注入时间
    #[cfg(test)]
    async fn status_at(&self, now: DateTime<Utc>) -> LicenseStatus {
        self.refresh_at(now).await;
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
        verify_code(code, &self.public_key, &fingerprint, Utc::now())?;
        write_code(&self.path, code)?;

        // 正式授权生效后删除自助试用记录，但保留台账，防止删除记录后重新开始试用
        match std::fs::remove_file(&self.trial_state_path) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {}
        }
        {
            let mut saved = self.code.write().await;
            *saved = Some(code.to_string());
        }
        self.refresh_at(Utc::now()).await;
        Ok(self.status.read().await.clone())
    }

    /// 开始本机 14 天自助试用
    pub async fn start_trial(&self) -> Result<LicenseStatus, String> {
        self.start_trial_at(Utc::now()).await
    }

    async fn start_trial_at(&self, now: DateTime<Utc>) -> Result<LicenseStatus, String> {
        let fingerprint = machine_fingerprint();
        let code = self.code.read().await.clone();

        // 已激活的正式授权（或有签名的试用码）不允许再开自助试用
        if let Some(code) = code.as_deref() {
            if let Ok(payload) = verify_code(code, &self.public_key, &fingerprint, now) {
                return Err(format!(
                    "当前已激活（{}），无需开始试用",
                    payload.license_id
                ));
            }
        }

        match evaluate_trial(&self.trial_anchors, &fingerprint, now) {
            // 幂等：已在试用中直接返回当前状态
            TrialEvaluation::Valid { .. } => {
                self.refresh_at(now).await;
                Ok(self.status.read().await.clone())
            }
            TrialEvaluation::Expired { record } => Err(format!(
                "试用已于 {} 结束，请购买正式授权",
                record.expires_at
            )),
            TrialEvaluation::Invalid { reason, .. } => Err(reason),
            TrialEvaluation::Absent => {
                if code.is_some() {
                    return Err("检测到已有激活码记录，试用不可用".to_string());
                }
                let record = new_trial_record(&fingerprint, now);
                write_trial_record(&self.trial_anchors, &record)?;
                self.refresh_at(now).await;
                Ok(self.status.read().await.clone())
            }
        }
    }

    /// 停用：删除本地激活码，回到免费版（已有的试用记录不会因此重置）
    pub async fn deactivate(&self) -> Result<(), String> {
        match std::fs::remove_file(&self.path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("删除激活信息失败：{}", e)),
        }
        {
            let mut saved = self.code.write().await;
            *saved = None;
        }
        self.refresh_at(Utc::now()).await;
        Ok(())
    }

    async fn refresh(&self) {
        self.refresh_at(Utc::now()).await;
    }

    async fn refresh_at(&self, now: DateTime<Utc>) {
        let fingerprint = machine_fingerprint();
        let code = self.code.read().await.clone();
        let next = compute_status(
            code.as_deref(),
            &self.public_key,
            &fingerprint,
            &self.trial_anchors,
            now,
        );
        *self.status.write().await = next;
    }
}

impl Default for LicenseManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 本机自助试用记录（trial.state 与 trial.ledger 内容一致）
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TrialRecord {
    v: u8,
    license_id: String,
    fingerprint: String,
    started_at: String,
    expires_at: String,
    last_seen_at: String,
    tampered: bool,
    /// 上述字段的 HMAC-SHA256（hex），用于防手工修改
    hmac: String,
}

/// 单份试用文件的读取结果
#[derive(Debug, Clone)]
enum TrialFile {
    Missing,
    Corrupt(String),
    Present(TrialRecord),
}

/// 试用记录校验结果
enum TrialEvaluation {
    /// 本机从未开始过试用
    Absent,
    /// 试用有效，`days_left` 为向上取整的剩余天数（1..=14）
    Valid {
        record: TrialRecord,
        days_left: u32,
    },
    /// 试用已到期
    Expired { record: TrialRecord },
    /// 记录损坏、HMAC/指纹不符或检测到时间回拨
    Invalid {
        reason: String,
        record: Option<TrialRecord>,
    },
}

/// 单份试用记录锚点：通常是文件，Windows 额外有一份注册表副本
#[derive(Debug, Clone)]
enum TrialAnchor {
    File(PathBuf),
    #[cfg(windows)]
    Registry,
}

impl TrialAnchor {
    fn read(&self) -> TrialFile {
        match self {
            TrialAnchor::File(path) => read_trial_file(path),
            #[cfg(windows)]
            TrialAnchor::Registry => read_registry_trial(),
        }
    }

    fn write(&self, content: &[u8]) -> Result<(), String> {
        match self {
            TrialAnchor::File(path) => write_private(path, content),
            #[cfg(windows)]
            TrialAnchor::Registry => write_registry_trial(content),
        }
    }
}

/// 多处试用记录锚点，顺序即恢复优先级（A 主记录 → B 台账 → C 缓存 → D 状态目录 → E 系统级）
#[derive(Debug, Clone, Default)]
struct TrialAnchors {
    anchors: Vec<TrialAnchor>,
}

impl TrialAnchors {
    fn push_file(&mut self, path: PathBuf) {
        if path.as_os_str().is_empty() {
            return;
        }
        // XDG 环境变量未设置时配置/缓存/状态目录可能重合，按路径去重
        let duplicated = self
            .anchors
            .iter()
            .any(|anchor| matches!(anchor, TrialAnchor::File(existing) if existing == &path));
        if !duplicated {
            self.anchors.push(TrialAnchor::File(path));
        }
    }

    /// 系统级锚点：Unix 用全局临时目录文件；Windows 用当前用户注册表
    #[cfg(not(windows))]
    fn push_system_anchor(&mut self) {
        self.push_file(PathBuf::from(TRIAL_SYSTEM_FILE));
    }

    #[cfg(windows)]
    fn push_system_anchor(&mut self) {
        self.anchors.push(TrialAnchor::Registry);
    }

    fn file_paths(&self) -> Vec<PathBuf> {
        // Unix 上没有注册表锚点，直接映射；Windows 需要过滤掉注册表变体
        #[cfg(not(windows))]
        {
            self.anchors
                .iter()
                .map(|anchor| match anchor {
                    TrialAnchor::File(path) => path.clone(),
                })
                .collect()
        }
        #[cfg(windows)]
        {
            self.anchors
                .iter()
                .filter_map(|anchor| match anchor {
                    TrialAnchor::File(path) => Some(path.clone()),
                    TrialAnchor::Registry => None,
                })
                .collect()
        }
    }

    fn read_all(&self) -> Vec<TrialFile> {
        self.anchors.iter().map(TrialAnchor::read).collect()
    }

    /// 尽力写入全部锚点：只要有一处成功就算成功，个别位置失败不阻断试用
    fn write_all(&self, content: &[u8]) -> bool {
        let mut any_written = false;
        for anchor in &self.anchors {
            if anchor.write(content).is_ok() {
                any_written = true;
            }
        }
        any_written
    }
}

/// 记录本身是否可信（HMAC、版本、指纹与时间字段均合法）；时间语义另行判断
fn trial_record_is_valid(record: &TrialRecord, fingerprint: &str) -> bool {
    record.v == TRIAL_RECORD_VERSION
        && trial_hmac_matches(record, fingerprint)
        && normalize_fingerprint(&record.fingerprint) == normalize_fingerprint(fingerprint)
        && parse_timestamp(&record.started_at).is_some()
        && parse_timestamp(&record.expires_at).is_some()
        && parse_timestamp(&record.last_seen_at).is_some()
}

/// 综合激活码与试用记录计算对外状态
fn compute_status(
    code: Option<&str>,
    public_key: &[u8; 32],
    fingerprint: &str,
    trial_anchors: &TrialAnchors,
    now: DateTime<Utc>,
) -> LicenseStatus {
    let code_status = code.map(|code| {
        match verify_code(code, public_key, fingerprint, now) {
            Ok(payload) => LicenseStatus::active(fingerprint.to_string(), &payload),
            Err(error) => LicenseStatus::invalid(fingerprint.to_string(), error),
        }
    });

    // 有效激活码优先；此时不再展示自助试用入口
    if let Some(status) = code_status.as_ref() {
        if status.activated {
            return status.clone();
        }
    }

    let has_code = code_status.is_some();
    let mut status = code_status.unwrap_or_else(|| LicenseStatus::free(fingerprint.to_string()));
    apply_trial_evaluation(
        &mut status,
        evaluate_trial(trial_anchors, fingerprint, now),
        has_code,
    );
    status
}

fn apply_trial_evaluation(
    status: &mut LicenseStatus,
    evaluation: TrialEvaluation,
    has_code: bool,
) {
    match evaluation {
        TrialEvaluation::Absent => {
            // 只有完全没有激活码记录时才允许开始试用
            status.trial_available = !has_code;
        }
        TrialEvaluation::Valid { record, days_left } => {
            status.activated = true;
            status.edition = "trial".to_string();
            // 空 features 表示全功能
            status.features = Vec::new();
            status.license_id = Some(record.license_id.clone());
            status.issued_at = Some(record.started_at.clone());
            status.expires_at = Some(record.expires_at.clone());
            status.seat = None;
            status.seats = None;
            status.trial_available = false;
            status.trial_active = true;
            status.trial_expired = false;
            status.trial_started_at = Some(record.started_at);
            status.trial_expires_at = Some(record.expires_at);
            status.trial_days_left = Some(days_left);
            status.error = None;
        }
        TrialEvaluation::Expired { record } => {
            status.activated = false;
            status.edition = "free".to_string();
            status.features.clear();
            status.license_id = None;
            status.issued_at = None;
            status.expires_at = None;
            status.trial_available = false;
            status.trial_active = false;
            status.trial_expired = true;
            status.trial_started_at = Some(record.started_at.clone());
            status.trial_expires_at = Some(record.expires_at.clone());
            status.trial_days_left = None;
            // 已有激活码错误时优先展示激活码问题，避免试用信息盖掉「激活码过期」
            if status.error.is_none() {
                status.error = Some(format!(
                    "试用已于 {} 结束，请购买正式授权",
                    record.expires_at
                ));
            }
        }
        TrialEvaluation::Invalid { reason, record } => {
            status.activated = false;
            status.edition = "free".to_string();
            status.features.clear();
            status.license_id = None;
            status.issued_at = None;
            status.expires_at = None;
            status.seat = None;
            status.seats = None;
            status.trial_available = false;
            status.trial_active = false;
            status.trial_expired = false;
            status.trial_days_left = None;
            if let Some(record) = record {
                status.trial_started_at = Some(record.started_at);
                status.trial_expires_at = Some(record.expires_at);
            }
            if status.error.is_none() {
                status.error = Some(reason);
            }
        }
    }
}

fn evaluate_trial(
    anchors: &TrialAnchors,
    fingerprint: &str,
    now: DateTime<Utc>,
) -> TrialEvaluation {
    let reads = anchors.read_all();

    // 任意一处锚点留有可信记录即视为试用存在；按优先级取第一份
    let mut chosen: Option<TrialRecord> = None;
    let mut first_problem: Option<String> = None;
    for (index, read) in reads.iter().enumerate() {
        match read {
            TrialFile::Present(record) if trial_record_is_valid(record, fingerprint) => {
                chosen = Some(record.clone());
                break;
            }
            TrialFile::Present(_) => {
                if first_problem.is_none() {
                    first_problem = Some(format!("第 {} 处锚点记录无效", index + 1));
                }
            }
            TrialFile::Corrupt(detail) => {
                if first_problem.is_none() {
                    first_problem = Some(detail.clone());
                }
            }
            TrialFile::Missing => {}
        }
    }

    let Some(mut record) = chosen else {
        return match first_problem {
            Some(detail) => TrialEvaluation::Invalid {
                reason: format!("试用记录校验失败：{detail}"),
                record: None,
            },
            // 所有锚点都不存在：本机从未开始过试用
            None => TrialEvaluation::Absent,
        };
    };

    // 任一处锚点被标记 tampered 都永久失效，避免部分锚点写入失败时被绕过
    let any_tampered = reads.iter().any(|read| {
        matches!(read, TrialFile::Present(existing) if existing.tampered && trial_record_is_valid(existing, fingerprint))
    });
    if record.tampered || any_tampered {
        // 强制同步到所有锚点，避免删掉某一处后从旧副本复活
        record.tampered = true;
        record.hmac = trial_hmac(&record, fingerprint);
        let _ = write_trial_record(anchors, &record);
        return TrialEvaluation::Invalid {
            reason: "试用记录已因时间回拨永久失效，请购买正式授权".to_string(),
            record: Some(record),
        };
    }

    // 除可信记录外的缺失/损坏锚点都要用可信记录修复
    let need_repair = reads.iter().any(|read| {
        !matches!(read, TrialFile::Present(existing) if trial_record_is_valid(existing, fingerprint))
    });

    let last_seen = parse_timestamp(&record.last_seen_at).expect("记录已验证");
    let expires = parse_timestamp(&record.expires_at).expect("记录已验证");

    // 时钟回拨超过容忍窗口：永久标记 tampered 并写入所有锚点
    if now.timestamp() + TRIAL_ROLLBACK_TOLERANCE_SECS < last_seen.timestamp() {
        record.tampered = true;
        record.hmac = trial_hmac(&record, fingerprint);
        let _ = write_trial_record(anchors, &record);
        return TrialEvaluation::Invalid {
            reason: "检测到系统时间回拨，试用已永久失效".to_string(),
            record: Some(record),
        };
    }

    if now > expires {
        // 到期时也把 last_seen_at 推进到当前时间：之后再把时钟调回试用期内
        // 会因 last_seen_at 领先而命中「时间回拨」，无法重置试用
        record.last_seen_at = format_timestamp(now);
        record.hmac = trial_hmac(&record, fingerprint);
        let _ = write_trial_record(anchors, &record);
        return TrialEvaluation::Expired { record };
    }

    let remaining = (expires - now).num_seconds().max(0);
    let days_left = ((remaining + 86_399) / 86_400).clamp(1, TRIAL_DAYS) as u32;
    // 节流更新 last_seen_at，减少写盘次数；缺失/损坏的锚点则借机修复
    let mut changed = need_repair;
    if now.timestamp() - last_seen.timestamp() > TRIAL_LAST_SEEN_THROTTLE_SECS {
        record.last_seen_at = format_timestamp(now);
        record.hmac = trial_hmac(&record, fingerprint);
        changed = true;
    }
    if changed {
        let _ = write_trial_record(anchors, &record);
    }
    TrialEvaluation::Valid { record, days_left }
}

fn read_trial_file(path: &Path) -> TrialFile {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<TrialRecord>(&text) {
            Ok(record) => TrialFile::Present(record),
            Err(error) => TrialFile::Corrupt(format!("无法解析：{error}")),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => TrialFile::Missing,
        Err(error) => TrialFile::Corrupt(format!("读取失败：{error}")),
    }
}

/// 创建一份新的试用记录（14 天，从 `now` 起算）
fn new_trial_record(fingerprint: &str, now: DateTime<Utc>) -> TrialRecord {
    let started_at = format_timestamp(now);
    let mut record = TrialRecord {
        v: TRIAL_RECORD_VERSION,
        license_id: trial_license_id(fingerprint),
        fingerprint: fingerprint.to_string(),
        started_at: started_at.clone(),
        expires_at: format_timestamp(now + Duration::days(TRIAL_DAYS)),
        last_seen_at: started_at,
        tampered: false,
        hmac: String::new(),
    };
    record.hmac = trial_hmac(&record, fingerprint);
    record
}

/// 试用授权号：`TRIAL-` + 机器指纹 SHA-256 前 4 字节（8 位大写 hex）
fn trial_license_id(fingerprint: &str) -> String {
    let digest = Sha256::digest(fingerprint.as_bytes());
    let hex: String = digest[..4]
        .iter()
        .map(|byte| format!("{:02X}", byte))
        .collect();
    format!("TRIAL-{hex}")
}

/// 试用记录的 HMAC：密钥由机器指纹派生，消息字段顺序固定
fn trial_hmac(record: &TrialRecord, fingerprint: &str) -> String {
    let key = sha256_bytes(format!("{}{}", TRIAL_HMAC_PREFIX, fingerprint).as_bytes());
    let message = format!(
        "{}|{}|{}|{}|{}|{}|{}",
        record.v,
        record.license_id,
        record.fingerprint,
        record.started_at,
        record.expires_at,
        record.last_seen_at,
        record.tampered
    );
    to_hex(&hmac_sha256(&key, message.as_bytes()))
}

fn trial_hmac_matches(record: &TrialRecord, fingerprint: &str) -> bool {
    record
        .hmac
        .eq_ignore_ascii_case(&trial_hmac(record, fingerprint))
}

/// 手写 HMAC-SHA256（RFC 2104），避免为试用功能新增依赖
fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK_SIZE: usize = 64;
    let mut key_block = [0u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        key_block[..32].copy_from_slice(&sha256_bytes(key));
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut inner_pad = [0u8; BLOCK_SIZE];
    for (slot, byte) in inner_pad.iter_mut().zip(key_block.iter()) {
        *slot = byte ^ 0x36;
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(message);
    let inner_hash: [u8; 32] = inner.finalize().into();

    let mut outer_pad = [0u8; BLOCK_SIZE];
    for (slot, byte) in outer_pad.iter_mut().zip(key_block.iter()) {
        *slot = byte ^ 0x5c;
    }
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner_hash);
    outer.finalize().into()
}

fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{:02x}", byte)).collect()
}

fn format_timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Utc))
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

/// Windows 注册表试用锚点位置（当前用户，无需管理员权限）
#[cfg(windows)]
const TRIAL_REGISTRY_KEY: &str = r"HKCU\Software\AuroraDB";
#[cfg(windows)]
const TRIAL_REGISTRY_VALUE: &str = "Trial";

/// 读取 Windows 注册表锚点；无 reg.exe / 无权限等失败一律静默视为缺失
#[cfg(windows)]
fn read_registry_trial() -> TrialFile {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = match std::process::Command::new("reg")
        .args([
            "query",
            TRIAL_REGISTRY_KEY,
            "/v",
            TRIAL_REGISTRY_VALUE,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return TrialFile::Missing,
    };
    let text = String::from_utf8_lossy(&output.stdout);
    match parse_reg_sz_named(&text, TRIAL_REGISTRY_VALUE) {
        Some(value) => match serde_json::from_str::<TrialRecord>(&value) {
            Ok(record) => TrialFile::Present(record),
            Err(error) => TrialFile::Corrupt(format!("无法解析注册表值：{error}")),
        },
        None => TrialFile::Missing,
    }
}

/// 写入 Windows 注册表锚点；失败只返回错误，由锚点集合忽略
#[cfg(windows)]
fn write_registry_trial(content: &[u8]) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let value = String::from_utf8_lossy(content).into_owned();
    let output = std::process::Command::new("reg")
        .args([
            "add",
            TRIAL_REGISTRY_KEY,
            "/v",
            TRIAL_REGISTRY_VALUE,
            "/t",
            "REG_SZ",
            "/d",
            value.as_str(),
            "/f",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("调用 reg.exe 失败：{error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err("写入注册表试用锚点失败".to_string())
    }
}

/// 从 `reg query` 输出中解析指定名称的 `REG_SZ` 值（单独拆出便于跨平台单测）。
#[cfg(any(windows, test))]
fn parse_reg_sz_named(output: &str, name: &str) -> Option<String> {
    // 输出形如：`    MachineGuid    REG_SZ    1234abcd-...`
    output.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        if fields.next()? != name {
            return None;
        }
        if fields.next()? != "REG_SZ" {
            return None;
        }
        fields.next().map(str::to_string)
    })
}

#[cfg(any(windows, test))]
fn parse_reg_sz(output: &str) -> Option<String> {
    parse_reg_sz_named(output, "MachineGuid")
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
    write_private(path, code.as_bytes())
}

/// 以 0600 权限写入私有文件；目录不存在时自动创建
fn write_private(path: &Path, content: &[u8]) -> Result<(), String> {
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
    file.write_all(content)
        .map_err(|e| format!("写入激活信息失败：{}", e))?;
    Ok(())
}

/// 把试用记录写入全部锚点：只要有一处成功就算成功，
/// 个别位置（如 /var/tmp 无权限、注册表不可用）失败不阻断试用
fn write_trial_record(anchors: &TrialAnchors, record: &TrialRecord) -> Result<(), String> {
    let content = serde_json::to_vec(record)
        .map_err(|e| format!("序列化试用记录失败：{}", e))?;
    if anchors.write_all(&content) {
        Ok(())
    } else {
        Err("写入试用记录失败：所有锚点均不可写".to_string())
    }
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

    /// 每个试用测试独占一个临时目录，避免 trial.state / trial.ledger 互相干扰
    fn trial_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "auroradb-trial-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn trial_manager(dir: &Path, public: [u8; 32]) -> LicenseManager {
        LicenseManager::new_in(dir.join("license.key"), public)
    }

    fn trial_anchor_paths(manager: &LicenseManager) -> Vec<PathBuf> {
        manager.trial_anchors.file_paths()
    }

    fn cleanup_trial(dir: &Path) {
        // 锚点包含 state 子目录与模拟的系统级文件，直接递归清理
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn hmac_sha256_matches_rfc4231_vector() {
        // RFC 4231 Test Case 2
        let key = [0x0bu8; 20];
        let digest = hmac_sha256(&key, b"Hi There");
        assert_eq!(
            to_hex(&digest),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[tokio::test]
    async fn start_trial_activates_full_features_and_counts_down() {
        let (_signing, public) = test_key();
        let dir = trial_dir();
        let manager = trial_manager(&dir, public);
        let t0 = Utc::now();

        let status = manager.start_trial_at(t0).await.unwrap();
        assert!(status.trial_active);
        assert!(status.activated);
        assert_eq!(status.edition, "trial");
        assert!(status.features.is_empty());
        assert_eq!(status.trial_days_left, Some(14));
        assert!(
            status.license_id.as_deref().unwrap_or_default().starts_with("TRIAL-"),
            "got: {:?}",
            status.license_id
        );
        assert!(status.trial_started_at.is_some());
        assert!(status.trial_expires_at.is_some());
        assert!(!status.trial_available);
        // 试用期全功能
        assert!(manager.allows(FEATURE_MONITOR).await);
        assert!(manager.allows(FEATURE_SCHEMA_DIFF).await);

        // 一天后剩余 13 天
        let next_day = manager.status_at(t0 + Duration::days(1)).await;
        assert!(next_day.trial_active);
        assert_eq!(next_day.trial_days_left, Some(13));

        cleanup_trial(&dir);
    }

    #[tokio::test]
    async fn trial_survives_restart() {
        let (_signing, public) = test_key();
        let dir = trial_dir();
        let t0 = Utc::now();
        let manager = trial_manager(&dir, public);
        let started = manager.start_trial_at(t0).await.unwrap();

        // 重新构造等价于应用重启
        let reloaded = trial_manager(&dir, public);
        let status = reloaded.status_at(t0 + Duration::hours(1)).await;
        assert!(status.trial_active);
        assert!(status.activated);
        assert_eq!(status.edition, "trial");
        assert_eq!(status.license_id, started.license_id);
        assert_eq!(status.trial_started_at, started.trial_started_at);

        cleanup_trial(&dir);
    }

    #[tokio::test]
    async fn expired_trial_falls_back_to_free() {
        let (_signing, public) = test_key();
        let dir = trial_dir();
        let t0 = Utc::now();
        let manager = trial_manager(&dir, public);
        manager.start_trial_at(t0).await.unwrap();

        let status = manager.status_at(t0 + Duration::days(15)).await;
        assert!(!status.activated);
        assert!(!status.trial_active);
        assert!(status.trial_expired);
        assert_eq!(status.edition, "free");
        assert!(
            status.error.as_deref().unwrap_or_default().contains("结束"),
            "got: {:?}",
            status.error
        );
        // 过期后不能重新开始试用
        let error = manager
            .start_trial_at(t0 + Duration::days(15))
            .await
            .unwrap_err();
        assert!(error.contains("结束"), "got: {error}");

        cleanup_trial(&dir);
    }

    #[tokio::test]
    async fn rollback_marks_tampered_and_stays_invalid() {
        let (_signing, public) = test_key();
        let dir = trial_dir();
        let t0 = Utc::now();
        let manager = trial_manager(&dir, public);
        manager.start_trial_at(t0).await.unwrap();

        // 把时钟往回拨 10 分钟，超过 300 秒容忍窗口
        let status = manager.status_at(t0 - Duration::minutes(10)).await;
        assert!(!status.activated);
        assert!(
            status.error.as_deref().unwrap_or_default().contains("时间回拨"),
            "got: {:?}",
            status.error
        );

        // tampered 已持久化：重启后即使时间恢复正常也永久失效
        let saved = std::fs::read_to_string(dir.join(TRIAL_STATE_FILE)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&saved).unwrap();
        assert_eq!(value["tampered"], serde_json::Value::Bool(true));

        let reloaded = trial_manager(&dir, public);
        let status = reloaded.status_at(t0).await;
        assert!(!status.activated);
        assert!(!status.trial_active);
        assert!(
            status.error.as_deref().unwrap_or_default().contains("时间回拨"),
            "got: {:?}",
            status.error
        );
        assert!(reloaded.start_trial_at(t0).await.is_err());

        cleanup_trial(&dir);
    }

    #[tokio::test]
    async fn tampered_record_fails_hmac_validation() {
        let (_signing, public) = test_key();
        let dir = trial_dir();
        let t0 = Utc::now();
        let manager = trial_manager(&dir, public);
        manager.start_trial_at(t0).await.unwrap();

        // 任意一处有效锚点都会让试用继续，因此必须把所有锚点都改成伪造内容
        for state_path in trial_anchor_paths(&manager) {
            let text = std::fs::read_to_string(&state_path).unwrap();
            let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
            value["license_id"] = serde_json::Value::String("TRIAL-FORGED".to_string());
            std::fs::write(&state_path, serde_json::to_vec(&value).unwrap()).unwrap();
        }

        let status = manager.status_at(t0 + Duration::minutes(1)).await;
        assert!(!status.activated);
        assert!(!status.trial_active);
        assert!(
            status.error.as_deref().unwrap_or_default().contains("校验失败"),
            "got: {:?}",
            status.error
        );

        cleanup_trial(&dir);
    }

    #[tokio::test]
    async fn repeated_start_trial_is_idempotent() {
        let (_signing, public) = test_key();
        let dir = trial_dir();
        let t0 = Utc::now();
        let manager = trial_manager(&dir, public);

        let first = manager.start_trial_at(t0).await.unwrap();
        let second = manager.start_trial_at(t0 + Duration::minutes(5)).await.unwrap();
        assert_eq!(first.license_id, second.license_id);
        assert_eq!(first.trial_started_at, second.trial_started_at);
        assert_eq!(second.trial_days_left, Some(14));

        cleanup_trial(&dir);
    }

    #[tokio::test]
    async fn activation_removes_trial_state_but_keeps_ledger_and_blocks_trial() {
        let (signing, public) = test_key();
        let dir = trial_dir();
        let t0 = Utc::now();
        let manager = trial_manager(&dir, public);
        manager.start_trial_at(t0).await.unwrap();
        assert!(dir.join(TRIAL_STATE_FILE).exists());
        assert!(dir.join(TRIAL_LEDGER_FILE).exists());

        let code = encode(
            &signing,
            &payload(&machine_fingerprint(), Some("2999-01-01T00:00:00Z".to_string())),
        );
        manager.activate(&code).await.unwrap();

        assert!(!dir.join(TRIAL_STATE_FILE).exists());
        assert!(dir.join(TRIAL_LEDGER_FILE).exists());
        // 激活后试用入口关闭
        let status = manager.status_at(t0 + Duration::minutes(1)).await;
        assert!(status.activated);
        assert_eq!(status.edition, "pro");
        assert!(!status.trial_available);
        let error = manager.start_trial_at(t0).await.unwrap_err();
        assert!(error.contains("已激活"), "got: {error}");

        cleanup_trial(&dir);
    }

    #[tokio::test]
    async fn deleted_trial_state_recovers_from_ledger() {
        let (_signing, public) = test_key();
        let dir = trial_dir();
        let t0 = Utc::now();
        let manager = trial_manager(&dir, public);
        let started = manager.start_trial_at(t0).await.unwrap();

        std::fs::remove_file(dir.join(TRIAL_STATE_FILE)).unwrap();
        let reloaded = trial_manager(&dir, public);
        let status = reloaded.status_at(t0 + Duration::hours(2)).await;
        assert!(status.trial_active, "got: {:?}", status.error);
        assert_eq!(status.license_id, started.license_id);
        // 恢复后主记录被重新写回
        assert!(dir.join(TRIAL_STATE_FILE).exists());

        cleanup_trial(&dir);
    }

    #[tokio::test]
    async fn fresh_machine_offers_trial() {
        let (_signing, public) = test_key();
        let dir = trial_dir();
        let manager = trial_manager(&dir, public);
        let status = manager.status_at(Utc::now()).await;
        assert!(status.trial_available);
        assert!(!status.trial_active);
        assert!(!status.trial_expired);
        assert!(!status.activated);
        assert_eq!(status.edition, "free");

        cleanup_trial(&dir);
    }

    #[tokio::test]
    async fn deleted_primary_recovers_from_remaining_anchors() {
        let (_signing, public) = test_key();
        let dir = trial_dir();
        let t0 = Utc::now();
        let manager = trial_manager(&dir, public);
        let started = manager.start_trial_at(t0).await.unwrap();

        let paths = trial_anchor_paths(&manager);
        std::fs::remove_file(&paths[0]).unwrap();

        let reloaded = trial_manager(&dir, public);
        let status = reloaded.status_at(t0 + Duration::days(1)).await;
        assert!(status.trial_active, "got: {:?}", status.error);
        assert_eq!(status.license_id, started.license_id);
        assert_eq!(status.trial_days_left, Some(13));
        // 主记录被回写
        assert!(paths[0].exists());

        cleanup_trial(&dir);
    }

    #[tokio::test]
    async fn system_anchor_alone_recovers_trial() {
        let (_signing, public) = test_key();
        let dir = trial_dir();
        let t0 = Utc::now();
        let manager = trial_manager(&dir, public);
        let started = manager.start_trial_at(t0).await.unwrap();

        let paths = trial_anchor_paths(&manager);
        for path in &paths[..4] {
            std::fs::remove_file(path).unwrap();
        }

        let reloaded = trial_manager(&dir, public);
        let status = reloaded.status_at(t0 + Duration::hours(1)).await;
        assert!(status.trial_active, "got: {:?}", status.error);
        assert_eq!(status.license_id, started.license_id);
        // 其余锚点被重新修复
        for path in &paths[..4] {
            assert!(path.exists(), "missing: {}", path.display());
        }

        cleanup_trial(&dir);
    }

    #[tokio::test]
    async fn all_anchors_deleted_offers_fresh_trial() {
        let (_signing, public) = test_key();
        let dir = trial_dir();
        let t0 = Utc::now();
        let manager = trial_manager(&dir, public);
        manager.start_trial_at(t0).await.unwrap();

        for path in trial_anchor_paths(&manager) {
            let _ = std::fs::remove_file(&path);
        }

        let reloaded = trial_manager(&dir, public);
        let status = reloaded.status_at(t0 + Duration::hours(1)).await;
        assert!(status.trial_available);
        assert!(!status.trial_active);

        let started = reloaded
            .start_trial_at(t0 + Duration::hours(1))
            .await
            .unwrap();
        assert!(started.trial_active);
        assert_eq!(started.trial_days_left, Some(14));

        cleanup_trial(&dir);
    }

    #[tokio::test]
    async fn corrupt_anchors_are_repaired_from_valid_copy() {
        let (_signing, public) = test_key();
        let dir = trial_dir();
        let t0 = Utc::now();
        let manager = trial_manager(&dir, public);
        let started = manager.start_trial_at(t0).await.unwrap();

        let paths = trial_anchor_paths(&manager);
        std::fs::write(&paths[0], b"not json").unwrap();
        std::fs::write(&paths[2], b"not json").unwrap();

        let reloaded = trial_manager(&dir, public);
        let status = reloaded.status_at(t0 + Duration::hours(1)).await;
        assert!(status.trial_active, "got: {:?}", status.error);
        assert_eq!(status.license_id, started.license_id);
        for path in [&paths[0], &paths[2]] {
            let text = std::fs::read_to_string(path).unwrap();
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(
                value["license_id"],
                serde_json::Value::String(started.license_id.clone().unwrap())
            );
        }

        cleanup_trial(&dir);
    }

    #[tokio::test]
    async fn all_anchors_corrupt_is_invalid() {
        let (_signing, public) = test_key();
        let dir = trial_dir();
        let t0 = Utc::now();
        let manager = trial_manager(&dir, public);
        manager.start_trial_at(t0).await.unwrap();

        for path in trial_anchor_paths(&manager) {
            std::fs::write(&path, b"broken").unwrap();
        }

        let status = manager.status_at(t0 + Duration::hours(1)).await;
        assert!(!status.activated);
        assert!(!status.trial_active);
        assert!(!status.trial_available);
        assert!(
            status.error.as_deref().unwrap_or_default().contains("校验失败"),
            "got: {:?}",
            status.error
        );

        cleanup_trial(&dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn trial_anchor_files_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let (_signing, public) = test_key();
        let dir = trial_dir();
        let t0 = Utc::now();
        let manager = trial_manager(&dir, public);
        manager.start_trial_at(t0).await.unwrap();

        for path in trial_anchor_paths(&manager) {
            assert!(path.exists(), "missing: {}", path.display());
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "unexpected mode {mode:o} for {}", path.display());
        }

        cleanup_trial(&dir);
    }

    #[tokio::test]
    async fn expired_then_clock_rollback_is_tampered() {
        let (_signing, public) = test_key();
        let dir = trial_dir();
        let t0 = Utc::now();
        let manager = trial_manager(&dir, public);
        manager.start_trial_at(t0).await.unwrap();

        // 先调到 15 天后判定过期：此时也会把 last_seen_at 推进到该时间
        let expired = manager.status_at(t0 + Duration::days(15)).await;
        assert!(expired.trial_expired);

        // 再调回试用期内应命中时间回拨而永久失效，而不是恢复试用
        let status = manager.status_at(t0 + Duration::days(3)).await;
        assert!(!status.activated);
        assert!(!status.trial_active);
        assert!(
            status.error.as_deref().unwrap_or_default().contains("时间回拨"),
            "got: {:?}",
            status.error
        );

        // tampered 已持久化，重启后仍失效
        let reloaded = trial_manager(&dir, public);
        let status = reloaded.status_at(t0 + Duration::days(3)).await;
        assert!(!status.activated);
        assert!(
            status.error.as_deref().unwrap_or_default().contains("时间回拨"),
            "got: {:?}",
            status.error
        );

        cleanup_trial(&dir);
    }
}
