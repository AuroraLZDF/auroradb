//! SSH 隧道模块
//!
//! 通过 SSH 的 `direct-tcpip` 通道转发 MySQL 流量：本地监听 `127.0.0.1` 随机端口，
//! 每条入站 TCP 连接对应一条 SSH 通道，数据库主机/端口由 SSH 服务器侧解析。
//!
//! 隧道随 `ManagedConnection` 存活，连接断开时 Drop 终止本地监听任务并关闭 SSH 会话。
//!
//! 第一版限制：
//! - 认证方式仅支持密码与私钥（不含 SSH Agent）。
//!
//! 主机密钥按 `~/.ssh/known_hosts` 校验：已记录但不一致时拒绝连接（防中间人），
//! 未记录时按 TOFU 记录后再放行（等价 OpenSSH 的 `StrictHostKeyChecking=accept-new`）。

use auroradb_core::connection::tunnel::{Tunnel, TunnelFactory};
use auroradb_core::connection::types::{SshAuthType, SshConfig};
use anyhow::{anyhow, Context, Result};
use futures_util::future::BoxFuture;
use russh::client::{self, AuthResult, Handle, Msg};
use russh::keys::known_hosts::{check_known_hosts_path, learn_known_hosts_path};
use russh::keys::{load_secret_key, HashAlg, PrivateKeyWithHashAlg, PublicKey};
use russh::Channel;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// SSH 建连超时
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// 单条转发通道打开超时
const CHANNEL_OPEN_TIMEOUT: Duration = Duration::from_secs(10);

/// 本地转发的 SSH 隧道
pub struct SshTunnel {
    local_port: u16,
    accept_task: Option<JoinHandle<()>>,
}

impl SshTunnel {
    /// 本地转发端口，数据库应连接 `127.0.0.1:local_port`
    pub fn local_port(&self) -> u16 {
        self.local_port
    }

    /// 建立 SSH 连接并在本地随机端口开启转发。
    ///
    /// `target_host` / `target_port` 是数据库在 SSH 服务器视角下的地址。
    pub async fn open(config: &SshConfig, target_host: &str, target_port: u16) -> Result<Self> {
        validate(config, target_host, target_port)?;

        let ssh_config = Arc::new(client::Config {
            inactivity_timeout: Some(Duration::from_secs(300)),
            keepalive_interval: Some(Duration::from_secs(30)),
            ..Default::default()
        });

        // russh 拒绝主机密钥时只报「key rejected」，具体原因由 handler 记在这里
        let host_key_failure: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let handler = ClientHandler {
            host: config.host.trim().to_string(),
            port: config.port,
            known_hosts: known_hosts_file(),
            failure: Arc::clone(&host_key_failure),
        };

        let mut handle = tokio::time::timeout(
            CONNECT_TIMEOUT,
            client::connect(
                ssh_config,
                (config.host.trim(), config.port),
                handler,
            ),
        )
        .await
        .map_err(|_| anyhow!("SSH 连接超时（{}:{}）", config.host, config.port))?
        .map_err(|e| {
            let reason = host_key_failure.lock().ok().and_then(|slot| slot.clone());
            match reason {
                Some(reason) => anyhow!("SSH 连接失败（{}:{}）：{}", config.host, config.port, reason),
                None => anyhow!("SSH 连接失败（{}:{}）：{}", config.host, config.port, e),
            }
        })?;

        authenticate(&mut handle, config).await?;

        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .context("SSH 隧道本地端口监听失败")?;
        let local_port = listener
            .local_addr()
            .context("读取 SSH 隧道本地端口失败")?
            .port();

        tracing::info!(
            "SSH 隧道已建立：127.0.0.1:{} -> {}:{}（经 {}:{}）",
            local_port,
            target_host,
            target_port,
            config.host,
            config.port
        );

        let accept_task = tokio::spawn(accept_loop(
            listener,
            handle,
            target_host.to_string(),
            target_port,
        ));

        Ok(Self {
            local_port,
            accept_task: Some(accept_task),
        })
    }
}

impl Drop for SshTunnel {
    fn drop(&mut self) {
        // 终止本地监听；SSH 会话随 Handle 一起释放
        if let Some(task) = self.accept_task.take() {
            task.abort();
        }
    }
}

impl Tunnel for SshTunnel {
    fn local_port(&self) -> u16 {
        self.local_port
    }
}

/// 创建专业版隧道工厂，应用启动时注入连接管理器。
pub fn factory() -> TunnelFactory {
    Arc::new(|config: SshConfig, target_host: String, target_port: u16| {
        Box::pin(async move {
            let tunnel = SshTunnel::open(&config, &target_host, target_port).await?;
            Ok(Box::new(tunnel) as Box<dyn Tunnel>)
        }) as BoxFuture<'static, Result<Box<dyn Tunnel>>>
    })
}

/// 参数校验：尽量在连网前给出可读提示
fn validate(config: &SshConfig, target_host: &str, target_port: u16) -> Result<()> {
    if config.host.trim().is_empty() {
        return Err(anyhow!("SSH 主机不能为空"));
    }
    if config.port == 0 {
        return Err(anyhow!("SSH 端口必须在 1-65535 之间"));
    }
    if config.username.trim().is_empty() {
        return Err(anyhow!("SSH 用户名不能为空"));
    }
    if target_host.trim().is_empty() {
        return Err(anyhow!("数据库主机不能为空"));
    }
    if target_port == 0 {
        return Err(anyhow!("数据库端口必须在 1-65535 之间"));
    }

    match config.auth_type {
        SshAuthType::Password => {
            if config.password.as_deref().unwrap_or("").is_empty() {
                return Err(anyhow!("SSH 密码认证需要填写密码"));
            }
        }
        SshAuthType::PrivateKey => {
            let path = config.private_key_path.as_deref().map(str::trim).unwrap_or("");
            if path.is_empty() {
                return Err(anyhow!("SSH 私钥认证需要指定私钥文件"));
            }
            if !std::path::Path::new(path).is_file() {
                return Err(anyhow!("SSH 私钥文件不存在或不可读：{}", path));
            }
        }
        SshAuthType::Agent => {
            return Err(anyhow!("暂不支持 SSH Agent 认证，请改用密码或私钥"));
        }
    }

    Ok(())
}

/// 认证：密码 / 私钥
async fn authenticate(handle: &mut Handle<ClientHandler>, config: &SshConfig) -> Result<()> {
    let user = config.username.trim();

    match config.auth_type {
        SshAuthType::Password => {
            let password = config
                .password
                .as_deref()
                .filter(|p| !p.is_empty())
                .ok_or_else(|| anyhow!("SSH 密码认证需要填写密码"))?;

            let result = handle
                .authenticate_password(user, password)
                .await
                .map_err(|e| anyhow!("SSH 密码认证请求失败：{}", e))?;
            if !matches!(result, AuthResult::Success) {
                return Err(anyhow!("SSH 认证失败：用户名或密码错误"));
            }
        }
        SshAuthType::PrivateKey => {
            let path = config
                .private_key_path
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .ok_or_else(|| anyhow!("SSH 私钥认证需要指定私钥文件"))?;
            // 空串视为未设置口令
            let passphrase = config.passphrase.as_deref().filter(|p| !p.is_empty());

            let key = load_secret_key(path, passphrase)
                .map_err(|e| anyhow!("SSH 私钥加载失败（{}）：{}", path, e))?;

            // RSA 需要与服务器协商签名算法，其余算法忽略该参数
            let hash_alg = if key.algorithm().is_rsa() {
                handle
                    .best_supported_rsa_hash()
                    .await
                    .map_err(|e| anyhow!("SSH 协商 RSA 签名算法失败：{}", e))?
                    .flatten()
            } else {
                None
            };

            let key = PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg);
            let result = handle
                .authenticate_publickey(user, key)
                .await
                .map_err(|e| anyhow!("SSH 私钥认证请求失败：{}", e))?;
            if !matches!(result, AuthResult::Success) {
                return Err(anyhow!("SSH 认证失败：私钥未被服务器接受"));
            }
        }
        SshAuthType::Agent => {
            return Err(anyhow!("暂不支持 SSH Agent 认证，请改用密码或私钥"));
        }
    }

    Ok(())
}

/// `~/.ssh/known_hosts` 路径；取不到主目录时返回 None（无法校验）
fn known_hosts_file() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".ssh").join("known_hosts"))
}

/// 校验服务端主机密钥。
///
/// - known_hosts 已有该主机且密钥一致 → 通过
/// - 已有记录但密钥不一致 → 拒绝（可能是中间人攻击）
/// - 无记录 → 记录后放行（TOFU；写入失败只告警，不阻断首次连接）
fn verify_host_key(path: &Path, host: &str, port: u16, key: &PublicKey) -> Result<(), String> {
    match check_known_hosts_path(host, port, key, path) {
        Ok(true) => Ok(()),
        Ok(false) => {
            let fingerprint = key.fingerprint(HashAlg::Sha256);
            match learn_known_hosts_path(host, port, key, path) {
                Ok(()) => tracing::warn!(
                    "首次连接 SSH 主机 {}:{}，已记录主机密钥指纹 {}（写入 {}）",
                    host,
                    port,
                    fingerprint,
                    path.display()
                ),
                Err(e) => tracing::warn!(
                    "首次连接 SSH 主机 {}:{}，主机密钥指纹 {} 写入 {} 失败：{}",
                    host,
                    port,
                    fingerprint,
                    path.display(),
                    e
                ),
            }
            Ok(())
        }
        Err(russh::keys::Error::KeyChanged { line }) => Err(format!(
            "SSH 主机 {}:{} 的主机密钥与 known_hosts 第 {} 行记录不一致，可能存在中间人攻击；\
             确认无误后请删除该条目再重试（{}）",
            host,
            port,
            line,
            path.display()
        )),
        Err(e) => Err(format!("SSH 主机 {}:{} 的主机密钥校验失败：{}", host, port, e)),
    }
}

/// SSH 客户端回调：按 known_hosts 校验服务端主机密钥
struct ClientHandler {
    host: String,
    port: u16,
    known_hosts: Option<PathBuf>,
    /// 校验失败原因（russh 只会返回「主机密钥被拒绝」）
    failure: Arc<Mutex<Option<String>>>,
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        let Some(path) = self.known_hosts.clone() else {
            tracing::warn!(
                "未找到用户主目录，跳过 SSH 主机密钥校验；服务端指纹：{}",
                server_public_key.fingerprint(HashAlg::Sha256)
            );
            return Ok(true);
        };

        let host = self.host.clone();
        let port = self.port;
        let key = server_public_key.clone();
        // known_hosts 是同步文件读写，放到阻塞线程池，避免卡住 async 运行时
        let outcome = tokio::task::spawn_blocking(move || verify_host_key(&path, &host, port, &key))
            .await;

        let rejection = match outcome {
            Ok(Ok(())) => return Ok(true),
            Ok(Err(reason)) => reason,
            Err(e) => format!("SSH 主机密钥校验任务异常：{}", e),
        };

        tracing::error!("{}", rejection);
        if let Ok(mut slot) = self.failure.lock() {
            *slot = Some(rejection);
        }
        Ok(false)
    }
}

/// 接受本地连接并为每条连接打开一条 SSH 转发通道
async fn accept_loop(
    listener: TcpListener,
    handle: Handle<ClientHandler>,
    target_host: String,
    target_port: u16,
) {
    loop {
        let (socket, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(e) => {
                tracing::warn!("SSH 隧道接受本地连接失败：{}", e);
                continue;
            }
        };

        if handle.is_closed() {
            tracing::warn!("SSH 会话已关闭，停止接受本地连接");
            break;
        }

        let channel = tokio::time::timeout(
            CHANNEL_OPEN_TIMEOUT,
            handle.channel_open_direct_tcpip(
                target_host.clone(),
                u32::from(target_port),
                "127.0.0.1",
                0,
            ),
        )
        .await;

        match channel {
            Ok(Ok(channel)) => {
                tokio::spawn(forward(socket, channel));
            }
            Ok(Err(e)) => {
                tracing::warn!("SSH 隧道打开转发通道失败：{}（来自 {}）", e, peer);
            }
            Err(_) => {
                tracing::warn!("SSH 隧道打开转发通道超时（来自 {}）", peer);
            }
        }
    }
}

/// 双向转发：本地 TCP <-> SSH 通道
async fn forward(mut socket: tokio::net::TcpStream, channel: Channel<Msg>) {
    let mut stream = channel.into_stream();
    match tokio::io::copy_bidirectional(&mut socket, &mut stream).await {
        Ok((upload, download)) => {
            tracing::debug!("SSH 隧道转发结束：上行 {} 字节，下行 {} 字节", upload, download);
        }
        Err(e) => tracing::warn!("SSH 隧道转发中断：{}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ssh(auth_type: SshAuthType) -> SshConfig {
        SshConfig {
            host: "jump.example.com".to_string(),
            port: 22,
            username: "ops".to_string(),
            auth_type,
            password: Some("secret".to_string()),
            private_key_path: None,
            passphrase: None,
        }
    }

    #[test]
    fn validate_requires_password_for_password_auth() {
        let mut config = ssh(SshAuthType::Password);
        config.password = None;
        let err = validate(&config, "127.0.0.1", 3306).expect_err("缺少密码应报错");
        assert!(err.to_string().contains("需要填写密码"), "{err}");
    }

    #[test]
    fn validate_reports_missing_private_key() {
        let mut config = ssh(SshAuthType::PrivateKey);
        config.private_key_path = Some("/tmp/auroradb-not-exists-key".to_string());
        let err = validate(&config, "127.0.0.1", 3306).expect_err("私钥不存在应报错");
        assert!(err.to_string().contains("私钥文件不存在"), "{err}");

        config.private_key_path = None;
        let err = validate(&config, "127.0.0.1", 3306).expect_err("未指定私钥应报错");
        assert!(err.to_string().contains("需要指定私钥文件"), "{err}");
    }

    #[test]
    fn validate_rejects_agent_and_bad_fields() {
        let err = validate(&ssh(SshAuthType::Agent), "127.0.0.1", 3306)
            .expect_err("Agent 认证第一版不支持");
        assert!(err.to_string().contains("暂不支持 SSH Agent"), "{err}");

        let mut config = ssh(SshAuthType::Password);
        config.host = "  ".to_string();
        assert!(validate(&config, "127.0.0.1", 3306).is_err());

        let mut config = ssh(SshAuthType::Password);
        config.port = 0;
        assert!(validate(&config, "127.0.0.1", 3306).is_err());
        assert!(validate(&config, "  ", 3306).is_err());
        assert!(validate(&config, "127.0.0.1", 0).is_err());
    }

    #[test]
    fn validate_accepts_password_auth() {
        assert!(validate(&ssh(SshAuthType::Password), "db.internal", 3306).is_ok());
    }

    const TEST_KEY: &str =
        "AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ";
    const OTHER_KEY: &str =
        "AAAAC3NzaC1lZDI1NTE5AAAAIA6rWI3G2sz07DnfFlrouTcysQlj2P+jpNSOEWD9OJ3X";

    /// 独立临时文件，避免测试并行时互相干扰，也不碰用户真实的 known_hosts
    fn temp_known_hosts(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "auroradb-known-hosts-{}-{}.txt",
            std::process::id(),
            tag
        ))
    }

    fn test_key(base64: &str) -> PublicKey {
        russh::keys::parse_public_key_base64(base64).expect("测试公钥应可解析")
    }

    #[test]
    fn verify_host_key_records_unknown_host_then_accepts() {
        let path = temp_known_hosts("tofu");
        let _ = std::fs::remove_file(&path);
        let key = test_key(TEST_KEY);

        verify_host_key(&path, "jump.example.com", 2222, &key).expect("首次连接应放行并记录");
        assert!(
            check_known_hosts_path("jump.example.com", 2222, &key, &path).unwrap(),
            "首次连接后应写入 known_hosts"
        );
        verify_host_key(&path, "jump.example.com", 2222, &key).expect("同密钥再次连接应通过");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn verify_host_key_rejects_changed_key() {
        let path = temp_known_hosts("mismatch");
        let _ = std::fs::remove_file(&path);

        verify_host_key(&path, "jump.example.com", 22, &test_key(TEST_KEY)).expect("首次记录应放行");
        let err = verify_host_key(&path, "jump.example.com", 22, &test_key(OTHER_KEY))
            .expect_err("主机密钥变化必须拒绝连接");
        assert!(err.contains("不一致"), "{err}");
        assert!(err.contains("中间人"), "{err}");

        let _ = std::fs::remove_file(&path);
    }
}
