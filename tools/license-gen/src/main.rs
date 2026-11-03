//! AuroraDB 离线激活码签发工具（仅发行方使用，不随应用分发）
//!
//! 用法：
//!   license-gen keygen [--out <私钥路径>] [--force]
//!   license-gen sign --fingerprint <机器码> [--plan <预设> | --days <天数>]
//!                    [--key <私钥路径>] [--license-id <编号>]
//!                    [--edition pro|trial] [--features <逗号分隔的功能键>]
//!                    [--seat N --seats M]
//!   license-gen batch --plan <预设> --input <文件> [--out codes.csv]
//!                     [--key <私钥路径>] [--license-id <编号>]
//!
//! `--plan` 预设：personal/team/enterprise/education（买断，永久）、annual（365 天）、
//! trial（14 天，edition=trial）。显式 `--days` 覆盖预设期限，0 表示永久。
//!
//! 激活码格式：`AURORADB1.<base64url(payload_json)>.<base64url(ed25519_signature)>`
//! 签名对象为 payload 段（base64url 字符串）的 ASCII 字节。

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use ed25519_dalek::{Signer, SigningKey};
use serde::Serialize;
use std::collections::HashSet;
use std::path::PathBuf;

const LICENSE_PREFIX: &str = "AURORADB1";
const PRODUCT_ID: &str = "auroradb";
/// 专业版默认包含全部能力；`*` 与空列表等价，新增能力时无需再改签发工具
const DEFAULT_FEATURES: &str = "*";
/// 买断档的有效天数（0 表示永久）
const BUYOUT_DAYS: i64 = 0;

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
    /// 多机位授权中的机位序号（从 1 开始）
    #[serde(skip_serializing_if = "Option::is_none")]
    seat: Option<u32>,
    /// 多机位授权的机位总数
    #[serde(skip_serializing_if = "Option::is_none")]
    seats: Option<u32>,
}

/// 套餐预设：决定期限与 edition（payload v1 结构不变）
struct Plan {
    name: &'static str,
    label: &'static str,
    days: i64,
    edition: &'static str,
}

const PLANS: &[Plan] = &[
    Plan {
        name: "personal",
        label: "个人买断",
        days: BUYOUT_DAYS,
        edition: "pro",
    },
    Plan {
        name: "team",
        label: "团队买断",
        days: BUYOUT_DAYS,
        edition: "pro",
    },
    Plan {
        name: "enterprise",
        label: "企业买断",
        days: BUYOUT_DAYS,
        edition: "pro",
    },
    Plan {
        name: "education",
        label: "教育买断",
        days: BUYOUT_DAYS,
        edition: "pro",
    },
    Plan {
        name: "annual",
        label: "年度订阅",
        days: 365,
        edition: "pro",
    },
    Plan {
        name: "trial",
        label: "全功能试用",
        days: 14,
        edition: "trial",
    },
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(command) = args.get(1) else {
        print_usage();
        std::process::exit(2);
    };

    let result = match command.as_str() {
        "keygen" => keygen(&args[2..]),
        "sign" => sign(&args[2..]),
        "batch" => batch(&args[2..]),
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
         签发单个激活码：\n  \
         license-gen sign --fingerprint <机器码> [--plan <预设> | --days <有效天数，0=永久>]\n                   \
         [--key <私钥路径>] [--license-id <编号>] [--edition pro|trial]\n                   \
         [--features <逗号分隔的功能键>] [--seat N --seats M]\n\n\
         批量签发（同一 license_id，按行生成 1..N 机位）：\n  \
         license-gen batch --plan <预设> --input <文件> [--out codes.csv]\n                   \
         [--key <私钥路径>] [--license-id <编号>]\n\n\
         {}\n\n\
         --days 显式指定时覆盖 plan 预设期限（0=永久）；--plan 与 --days 都没给会报错。",
        plan_table()
    );
}

fn plan_spec(name: &str) -> Option<&'static Plan> {
    PLANS.iter().find(|plan| plan.name == name)
}

/// 可打印的 plan 表（缺参数报错时一并输出，避免误用默认 365 天）
fn plan_table() -> String {
    let mut table = String::from("可用 --plan 预设：\n");
    for plan in PLANS {
        let days = if plan.days == 0 {
            "永久".to_string()
        } else {
            format!("{} 天", plan.days)
        };
        table.push_str(&format!(
            "  {:<11} {:<10} {:<6} edition={}\n",
            plan.name, plan.label, days, plan.edition
        ));
    }
    table.trim_end().to_string()
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

/// sign/batch 共用的期限与版本解析：`--plan` 预设、`--days` 显式覆盖。
struct ResolvedPlan {
    days: i64,
    edition: String,
}

fn resolve_plan(args: &[String], require_plan: bool) -> Result<ResolvedPlan, String> {
    let plan = match arg_value(args, "--plan") {
        Some(name) => Some(
            plan_spec(&name)
                .ok_or_else(|| format!("未知 --plan「{}」。\n{}", name, plan_table()))?,
        ),
        None => None,
    };
    if require_plan && plan.is_none() {
        return Err(format!("必须指定 --plan。\n{}", plan_table()));
    }

    let explicit_days = match arg_value(args, "--days") {
        Some(text) => Some(
            text.trim()
                .parse::<i64>()
                .map_err(|_| "--days 必须是整数（0 表示永久）".to_string())?,
        ),
        None => None,
    };
    let days = match (plan, explicit_days) {
        (Some(plan), None) => plan.days,
        (_, Some(days)) => days,
        (None, None) => {
            return Err(format!(
                "缺少 --plan 或 --days，不再默认 365 天。\n{}",
                plan_table()
            ))
        }
    };
    if days < 0 {
        return Err("--days 不能为负数（0 表示永久）".to_string());
    }

    // 显式 --edition 可覆盖 plan 预设；两者都没有时沿用旧行为 pro
    let edition = arg_value(args, "--edition")
        .or_else(|| plan.map(|plan| plan.edition.to_string()))
        .unwrap_or_else(|| "pro".to_string());
    if edition != "pro" && edition != "trial" {
        return Err(format!("--edition 只支持 pro 或 trial，收到「{}」", edition));
    }

    Ok(ResolvedPlan { days, edition })
}

fn expires_at(issued_at: DateTime<Utc>, days: i64) -> Option<String> {
    if days <= 0 {
        None
    } else {
        Some((issued_at + Duration::days(days)).to_rfc3339_opts(SecondsFormat::Secs, true))
    }
}

#[allow(clippy::too_many_arguments)]
fn build_payload(
    fingerprint: &str,
    license_id: &str,
    issued_at: DateTime<Utc>,
    resolved: &ResolvedPlan,
    features: Vec<String>,
    seat: Option<u32>,
    seats: Option<u32>,
) -> LicensePayload {
    LicensePayload {
        v: 1,
        product: PRODUCT_ID.to_string(),
        edition: resolved.edition.clone(),
        license_id: license_id.to_string(),
        fingerprint: normalize_fingerprint(fingerprint),
        issued_at: issued_at.to_rfc3339_opts(SecondsFormat::Secs, true),
        expires_at: expires_at(issued_at, resolved.days),
        features,
        seat,
        seats,
    }
}

fn sign_code(signing: &SigningKey, payload: &LicensePayload) -> Result<String, String> {
    let json = serde_json::to_vec(payload).map_err(|e| format!("序列化失败：{}", e))?;
    let payload_b64 = URL_SAFE_NO_PAD.encode(&json);
    let signature = signing.sign(payload_b64.as_bytes());
    Ok(format!(
        "{}.{}.{}",
        LICENSE_PREFIX,
        payload_b64,
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    ))
}

fn load_signing_key(args: &[String]) -> Result<SigningKey, String> {
    let key_path = arg_value(args, "--key")
        .map(PathBuf::from)
        .unwrap_or_else(default_key_path);
    let secret_hex = std::fs::read_to_string(&key_path)
        .map_err(|e| format!("读取私钥失败（{}）：{}", key_path.display(), e))?;
    let secret = hex_decode(secret_hex.trim())?;
    let secret: [u8; 32] = secret
        .try_into()
        .map_err(|_| "私钥长度错误，应为 32 字节".to_string())?;
    Ok(SigningKey::from_bytes(&secret))
}

fn features_from_args(args: &[String]) -> Vec<String> {
    arg_value(args, "--features")
        .unwrap_or_else(|| DEFAULT_FEATURES.to_string())
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
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
    println!("公钥（填入 crates/core/src/license.rs 的 PUBLIC_KEY_HEX）：");
    println!("{}", public_hex);
    Ok(())
}

fn sign(args: &[String]) -> Result<(), String> {
    let fingerprint = arg_value(args, "--fingerprint")
        .ok_or_else(|| "缺少 --fingerprint（客户端「激活」窗口中显示的机器码）".to_string())?;
    if normalize_fingerprint(&fingerprint).is_empty() {
        return Err("--fingerprint 不能为空".to_string());
    }

    let resolved = resolve_plan(args, false)?;
    let (seat, seats) = parse_seat_pair(args)?;
    let signing = load_signing_key(args)?;

    let issued_at = Utc::now();
    let license_id = arg_value(args, "--license-id")
        .unwrap_or_else(|| format!("AD-{}", issued_at.format("%Y%m%d%H%M%S")));
    let payload = build_payload(
        &fingerprint,
        &license_id,
        issued_at,
        &resolved,
        features_from_args(args),
        seat,
        seats,
    );

    println!("{}", sign_code(&signing, &payload)?);
    Ok(())
}

fn parse_seat_pair(args: &[String]) -> Result<(Option<u32>, Option<u32>), String> {
    let seat = parse_u32_arg(args, "--seat")?;
    let seats = parse_u32_arg(args, "--seats")?;
    match (seat, seats) {
        (None, None) => Ok((None, None)),
        (Some(seat), Some(seats)) if seat >= 1 && seat <= seats => Ok((Some(seat), Some(seats))),
        (Some(_), Some(_)) => Err("--seat 与 --seats 需满足 1 <= seat <= seats".to_string()),
        _ => Err("--seat 与 --seats 必须成对使用".to_string()),
    }
}

fn parse_u32_arg(args: &[String], key: &str) -> Result<Option<u32>, String> {
    match arg_value(args, key) {
        Some(text) => text
            .trim()
            .parse::<u32>()
            .map(Some)
            .map_err(|_| format!("{} 必须是正整数", key)),
        None => Ok(None),
    }
}

struct CsvRow {
    license_id: String,
    seat: u32,
    fingerprint: String,
    edition: String,
    issued_at: String,
    expires_at: String,
    code: String,
}

fn batch(args: &[String]) -> Result<(), String> {
    if has_flag(args, "--seat") || has_flag(args, "--seats") {
        return Err("batch 的席位由输入行数决定，不支持 --seat/--seats".to_string());
    }
    let input_path =
        arg_value(args, "--input").ok_or_else(|| "batch 缺少 --input <文件>".to_string())?;
    let out_path = arg_value(args, "--out").unwrap_or_else(|| "codes.csv".to_string());
    let resolved = resolve_plan(args, true)?;
    let signing = load_signing_key(args)?;

    let text = std::fs::read_to_string(&input_path)
        .map_err(|e| format!("读取输入文件失败（{}）：{}", input_path, e))?;
    let rows = parse_batch_rows(&text)?;
    let total = rows.len();

    let issued_at = Utc::now();
    let license_id = arg_value(args, "--license-id")
        .unwrap_or_else(|| format!("AD-{}", issued_at.timestamp()));
    if license_id.contains(',') || license_id.contains('\n') || license_id.contains('\r') {
        return Err("--license-id 不能包含逗号或换行".to_string());
    }

    // 先整体生成并校验，任何一行失败都不写出半成品 CSV
    let features = features_from_args(args);
    let mut issued = Vec::with_capacity(total);
    for (index, (fingerprint, seat)) in rows.iter().enumerate() {
        let seat = seat.unwrap_or((index + 1) as u32);
        let payload = build_payload(
            fingerprint,
            &license_id,
            issued_at,
            &resolved,
            features.clone(),
            Some(seat),
            Some(total as u32),
        );
        issued.push(CsvRow {
            license_id: license_id.clone(),
            seat,
            fingerprint: payload.fingerprint.clone(),
            edition: payload.edition.clone(),
            issued_at: payload.issued_at.clone(),
            expires_at: payload.expires_at.clone().unwrap_or_default(),
            code: sign_code(&signing, &payload)?,
        });
    }

    let mut csv = String::from("license_id,seat,fingerprint,edition,issued_at,expires_at,code\n");
    for row in &issued {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            row.license_id,
            row.seat,
            row.fingerprint,
            row.edition,
            row.issued_at,
            row.expires_at,
            row.code
        ));
    }
    std::fs::write(&out_path, csv)
        .map_err(|e| format!("写入输出文件失败（{}）：{}", out_path, e))?;

    println!(
        "已生成 {} 个激活码：{}（license_id={}，edition={}）",
        total, out_path, license_id, resolved.edition
    );
    Ok(())
}

/// 解析 batch 输入：每行 `fingerprint` 或 `fingerprint,seat`，允许首行表头。
/// 返回 (fingerprint, 指定席位) 列表；行数即总机位数。
fn parse_batch_rows(text: &str) -> Result<Vec<(String, Option<u32>)>, String> {
    let mut rows = Vec::new();
    let mut first_line_checked = false;
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let line_no = index + 1;
        let fields: Vec<&str> = line.split(',').map(str::trim).collect();

        // 首行允许表头（fingerprint/fp 均可），避免把表头当机器码
        if !first_line_checked {
            first_line_checked = true;
            let header = fields[0].eq_ignore_ascii_case("fingerprint");
            if header {
                continue;
            }
        }

        if fields.len() > 2 && fields[2..].iter().any(|field| !field.is_empty()) {
            return Err(format!(
                "第 {} 行格式不正确：仅支持 fingerprint 或 fingerprint,seat 两列",
                line_no
            ));
        }
        let fingerprint = fields[0];
        if fingerprint.is_empty() {
            return Err(format!("第 {} 行缺少 fingerprint", line_no));
        }
        let seat = if fields.len() >= 2 && !fields[1].is_empty() {
            let seat: u32 = fields[1]
                .parse()
                .map_err(|_| format!("第 {} 行 seat 必须是正整数", line_no))?;
            if seat == 0 {
                return Err(format!("第 {} 行 seat 必须从 1 开始", line_no));
            }
            Some(seat)
        } else {
            None
        };
        rows.push((fingerprint.to_string(), seat));
    }

    if rows.is_empty() {
        return Err("输入文件中没有机器码".to_string());
    }
    validate_batch_rows(&rows)?;
    Ok(rows)
}

/// 全量校验：指纹去重、指定 seat 时必须唯一且恰好覆盖 1..N。
fn validate_batch_rows(rows: &[(String, Option<u32>)]) -> Result<(), String> {
    let total = rows.len();
    let mut seen = HashSet::new();
    for (fingerprint, _) in rows.iter() {
        let normalized = normalize_fingerprint(fingerprint);
        if !seen.insert(normalized.clone()) {
            return Err(format!("输入中存在重复指纹：{}", normalized));
        }
    }

    let specified: Vec<Option<u32>> = rows.iter().map(|(_, seat)| *seat).collect();
    if specified.iter().any(Option::is_some) {
        if specified.iter().any(Option::is_none) {
            return Err(
                "seat 需在每行全部指定，或全部留空由工具按行号自动分配".to_string(),
            );
        }
        let mut seats: Vec<u32> = specified.into_iter().flatten().collect();
        seats.sort_unstable();
        let expected: Vec<u32> = (1..=total as u32).collect();
        if seats != expected {
            return Err(format!(
                "文件指定的 seat 必须唯一且覆盖 1..{}，实际为 {:?}",
                total, seats
            ));
        }
    }
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
