//! 连接生命周期模块
//! 连接建立/断开、状态查询、心跳、事务的开始/提交/回滚。

use super::driver::{AnyPool, DriverKind};
use super::manager::{ConnectionManager, ManagedConnection};
use super::types::*;
use anyhow::{anyhow, Context, Result};
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlSslMode};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions, PgSslMode};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Notify};
use tokio::time::interval;

/// 把配置里的 SSL 模式映射为 MySQL 驱动的枚举
fn to_mysql_ssl_mode(mode: SslMode) -> MySqlSslMode {
    match mode {
        SslMode::Disabled => MySqlSslMode::Disabled,
        SslMode::Preferred => MySqlSslMode::Preferred,
        SslMode::Required => MySqlSslMode::Required,
        SslMode::VerifyCa => MySqlSslMode::VerifyCa,
        SslMode::VerifyIdentity => MySqlSslMode::VerifyIdentity,
    }
}

/// 把配置里的 SSL 模式映射为 PostgreSQL 驱动的枚举
///
/// PG 没有独立的「验证身份」档位，最严格一档 VerifyFull 同时校验证书链与主机名。
fn to_pg_ssl_mode(mode: SslMode) -> PgSslMode {
    match mode {
        SslMode::Disabled => PgSslMode::Disable,
        SslMode::Preferred => PgSslMode::Prefer,
        SslMode::Required => PgSslMode::Require,
        SslMode::VerifyCa => PgSslMode::VerifyCa,
        SslMode::VerifyIdentity => PgSslMode::VerifyFull,
    }
}

/// 前端未填写的路径会以空串传来，这里统一视为未配置
fn non_empty_path(path: &Option<String>) -> Option<&str> {
    path.as_deref().map(str::trim).filter(|p| !p.is_empty())
}

/// 判定错误是否为 TLS 握手阶段读到了明文数据
///
/// MySQL 服务端 TLS 协商失败时常用明文 ERR 包回应，rustls 会把它当成损坏的 TLS 记录，
/// 报 `received corrupt message of type UnknownProtocolVersion`，并被 sqlx 包成 `Io`。
fn is_tls_handshake_failure(err: &sqlx::Error) -> bool {
    let msg = match err {
        sqlx::Error::Io(e) => e.to_string(),
        other => other.to_string(),
    };
    msg.contains("corrupt message") || msg.contains("UnknownProtocolVersion")
}

/// 提前校验 TLS 文件，避免驱动只抛出底层的 “No such file or directory”
fn validate_ssl_files(ssl: &SslConfig) -> Result<()> {
    if ssl.mode == SslMode::Disabled {
        return Ok(());
    }

    let ca = non_empty_path(&ssl.ca_path);
    let cert = non_empty_path(&ssl.cert_path);
    let key = non_empty_path(&ssl.key_path);

    if cert.is_some() != key.is_some() {
        return Err(anyhow!("TLS 客户端证书与私钥必须同时配置"));
    }

    for (label, path) in [("CA 证书", ca), ("客户端证书", cert), ("客户端私钥", key)] {
        if let Some(path) = path {
            if !std::path::Path::new(path).is_file() {
                return Err(anyhow!("TLS {}文件不存在或不可读：{}", label, path));
            }
        }
    }

    Ok(())
}

impl ConnectionManager {
    /// 建立连接
    pub async fn connect(&self, request: ConnectionRequest) -> Result<String> {
        let id = request.config.id.clone();

        // 检查是否已存在
        {
            let connections = self.connections.read().await;
            if connections.contains_key(&id) {
                return Err(anyhow!("Connection {} already exists", id));
            }
        }

        validate_ssl_files(&request.config.ssl_config)?;

        // 启用 SSH 时先建隧道，数据库实际连本地转发端口；
        // request.config.host/port 作为 SSH 服务器侧的目标地址。
        // 隧道对象在此处持有，若后续建池失败会随作用域 Drop 一并关闭。
        let (tunnel, host, port) = match &request.ssh_config {
            Some(ssh) => {
                let tunnel = super::tunnel::SshTunnel::open(ssh, &request.config.host, request.config.port)
                    .await?;
                let local_port = tunnel.local_port();
                (Some(tunnel), "127.0.0.1".to_string(), local_port)
            }
            None => (None, request.config.host.clone(), request.config.port),
        };

        // 证书路径为空串时视为未配置，避免把空路径交给驱动
        let ssl = &request.config.ssl_config;
        let pool_config = &request.config.pool_config;

        // 按驱动创建连接池
        let pool = match request.config.driver {
            DriverKind::MySql => {
                let mode = ssl.mode;
                let connect = |ssl_mode: MySqlSslMode| {
                    let mut opts = MySqlConnectOptions::new()
                        .host(&host)
                        .port(port)
                        .username(&request.config.username)
                        .password(&request.config.password)
                        .ssl_mode(ssl_mode);

                    if let Some(path) = non_empty_path(&ssl.ca_path) {
                        opts = opts.ssl_ca(path);
                    }
                    if let Some(path) = non_empty_path(&ssl.cert_path) {
                        opts = opts.ssl_client_cert(path);
                    }
                    if let Some(path) = non_empty_path(&ssl.key_path) {
                        opts = opts.ssl_client_key(path);
                    }
                    if let Some(db) = &request.config.database {
                        opts = opts.database(db);
                    }

                    async move {
                        MySqlPoolOptions::new()
                            .max_connections(pool_config.max_connections)
                            .min_connections(pool_config.min_connections)
                            .acquire_timeout(Duration::from_secs(pool_config.connect_timeout))
                            .idle_timeout(Duration::from_secs(pool_config.idle_timeout))
                            .max_lifetime(Duration::from_secs(pool_config.max_lifetime))
                            .connect_with(opts)
                            .await
                    }
                };

                match connect(to_mysql_ssl_mode(mode)).await {
                    Ok(pool) => AnyPool::MySql(pool),
                    // 「首选」的语义是能加密就加密：服务端 TLS 协商失败时退回明文重连一次，
                    // 必需/校验类模式不满足条件，仍然硬失败。
                    Err(e) if mode == SslMode::Preferred && is_tls_handshake_failure(&e) => {
                        tracing::warn!("TLS 握手失败，按「首选」语义回退为明文连接：{}", e);
                        connect(MySqlSslMode::Disabled).await.map(AnyPool::MySql)?
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            DriverKind::Postgres => {
                // PG 的 db 名与 MySQL 的库等价，连接时直接指定；切换 schema 走 search_path
                let mut opts = PgConnectOptions::new()
                    .host(&host)
                    .port(port)
                    .username(&request.config.username)
                    .password(&request.config.password)
                    .ssl_mode(to_pg_ssl_mode(ssl.mode));

                // PG 的 CA 参数名为 root_cert
                if let Some(path) = non_empty_path(&ssl.ca_path) {
                    opts = opts.ssl_root_cert(path);
                }
                if let Some(path) = non_empty_path(&ssl.cert_path) {
                    opts = opts.ssl_client_cert(path);
                }
                if let Some(path) = non_empty_path(&ssl.key_path) {
                    opts = opts.ssl_client_key(path);
                }
                if let Some(db) = &request.config.database {
                    opts = opts.database(db);
                }

                PgPoolOptions::new()
                    .max_connections(pool_config.max_connections)
                    .min_connections(pool_config.min_connections)
                    .acquire_timeout(Duration::from_secs(pool_config.connect_timeout))
                    .idle_timeout(Duration::from_secs(pool_config.idle_timeout))
                    .max_lifetime(Duration::from_secs(pool_config.max_lifetime))
                    .connect_with(opts)
                    .await
                    .map(AnyPool::Postgres)?
            }
        };

        // 测试连接
        pool.ping().await?;

        // 启动心跳
        let heartbeat_handle = self.spawn_heartbeat(
            id.clone(),
            pool.clone(),
            request.config.pool_config.heartbeat_interval,
        );

        // 保存连接
        let managed = ManagedConnection {
            config: request.config,
            pool: Some(pool),
            state: ConnectionState::Connected,
            connected_at: Some(Instant::now()),
            last_error: None,
            heartbeat_handle: Some(heartbeat_handle),
            transaction: Arc::new(Mutex::new(None)),
            query_cancel: Arc::new(AtomicBool::new(false)),
            query_notify: Arc::new(Notify::new()),
            metadata_cache: Arc::new(Mutex::new(HashMap::new())),
            tunnel,
        };

        self.connections.write().await.insert(id.clone(), managed);

        Ok(id)
    }

    /// 断开连接
    pub async fn disconnect(&self, connection_id: &str) -> Result<()> {
        let mut connections = self.connections.write().await;

        if let Some(mut conn) = connections.remove(connection_id) {
            // 停止心跳
            if let Some(handle) = conn.heartbeat_handle {
                handle.abort();
            }

            // 未提交的事务随 ManagedConnection 丢弃，sqlx Transaction Drop 自动回滚

            // 关闭连接池
            if let Some(pool) = conn.pool {
                pool.close().await;
            }

            // 关闭 SSH 隧道：Drop 中终止本地监听任务并释放 SSH 会话
            conn.tunnel = None;

            conn.state = ConnectionState::Disconnected;
        }

        Ok(())
    }

    /// 获取连接状态
    pub async fn get_status(&self, connection_id: &str) -> Result<ConnectionStatus> {
        let connections = self.connections.read().await;

        let conn = connections
            .get(connection_id)
            .ok_or_else(|| anyhow!("Connection {} not found", connection_id))?;

        Ok(ConnectionStatus {
            id: connection_id.to_string(),
            driver: conn.config.driver,
            state: format!("{:?}", conn.state).to_lowercase(),
            message: match conn.state {
                ConnectionState::Connected => "Connected".to_string(),
                ConnectionState::Connecting => "Connecting...".to_string(),
                ConnectionState::Reconnecting { attempt } => format!("Reconnecting (attempt {})", attempt),
                ConnectionState::Failed(ref msg) => msg.clone(),
                ConnectionState::Disconnected => "Disconnected".to_string(),
            },
            connected_at: conn.connected_at.map(|t| format!("{:?}", t)),
            last_error: conn.last_error.clone(),
            stability_score: 100,
            health_status: if matches!(conn.state, ConnectionState::Connected) {
                "good"
            } else {
                "poor"
            }.to_string(),
        })
    }

    /// 启动心跳
    fn spawn_heartbeat(
        &self,
        connection_id: String,
        pool: AnyPool,
        interval_secs: u64,
    ) -> tokio::task::JoinHandle<()> {
        let connections = self.connections.clone();
        // 防止心跳间隔为 0 导致 interval panic
        let interval_secs = interval_secs.max(1);

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(interval_secs));

            loop {
                ticker.tick().await;

                // 执行心跳检测
                let result = pool.ping().await;

                // 查询执行期间会长时间持有读锁；这里用 try_write 避免排队写锁阻塞后续读请求
                let mut connections = match connections.try_write() {
                    Ok(guard) => guard,
                    Err(_) => {
                        tracing::debug!("Heartbeat state update skipped for {}", connection_id);
                        continue;
                    }
                };
                if let Some(conn) = connections.get_mut(&connection_id) {
                    if result.is_err() {
                        tracing::warn!("Heartbeat failed for connection {}", connection_id);
                        conn.last_error = Some("Heartbeat failed".to_string());
                        conn.state = ConnectionState::Failed("Heartbeat failed".to_string());
                    } else {
                        conn.state = ConnectionState::Connected;
                        conn.last_error = None;
                    }
                }
            }
        })
    }

    /// 开始事务：从池中钉扎一条物理连接，COMMIT/ROLLBACK 前所有语句都在该连接上执行
    pub async fn begin_transaction(&self, connection_id: &str) -> Result<()> {
        let connections = self.connections.read().await;
        let conn = connections.get(connection_id)
            .ok_or_else(|| anyhow!("Connection not found: {}", connection_id))?;

        let pool = conn.pool.as_ref()
            .ok_or_else(|| anyhow!("Connection not established"))?;

        let mut txn_guard = conn.transaction.lock().await;
        if txn_guard.is_some() {
            return Err(anyhow!("事务已在进行中，请先提交或回滚"));
        }
        let txn = pool.begin().await.context("开始事务失败")?;
        *txn_guard = Some(txn);

        Ok(())
    }

    /// 提交事务
    pub async fn commit_transaction(&self, connection_id: &str) -> Result<()> {
        let connections = self.connections.read().await;
        let conn = connections.get(connection_id)
            .ok_or_else(|| anyhow!("Connection not found: {}", connection_id))?;

        let mut txn_guard = conn.transaction.lock().await;
        let txn = txn_guard.take()
            .ok_or_else(|| anyhow!("没有正在进行的事务"))?;
        txn.commit().await.context("提交事务失败")?;

        Ok(())
    }

    /// 回滚事务
    pub async fn rollback_transaction(&self, connection_id: &str) -> Result<()> {
        let connections = self.connections.read().await;
        let conn = connections.get(connection_id)
            .ok_or_else(|| anyhow!("Connection not found: {}", connection_id))?;

        let mut txn_guard = conn.transaction.lock().await;
        let txn = txn_guard.take()
            .ok_or_else(|| anyhow!("没有正在进行的事务"))?;
        txn.rollback().await.context("回滚事务失败")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ssl(mode: SslMode, ca: Option<&str>, cert: Option<&str>, key: Option<&str>) -> SslConfig {
        SslConfig {
            mode,
            ca_path: ca.map(str::to_string),
            cert_path: cert.map(str::to_string),
            key_path: key.map(str::to_string),
        }
    }

    #[test]
    fn test_validate_ssl_files() {
        let missing = "/tmp/auroradb-not-exists-ca.pem";

        // 路径不存在时给出明确提示
        let err = validate_ssl_files(&ssl(SslMode::VerifyCa, Some(missing), None, None))
            .expect_err("缺少 CA 文件应报错");
        assert!(err.to_string().contains("CA 证书文件不存在"), "{err}");

        // disabled 时忽略遗留路径，不阻断连接
        assert!(validate_ssl_files(&ssl(SslMode::Disabled, Some(missing), None, None)).is_ok());

        // 证书与私钥必须成对出现
        let err = validate_ssl_files(&ssl(SslMode::VerifyCa, None, Some(missing), None))
            .expect_err("只配置证书应报错");
        assert!(err.to_string().contains("必须同时配置"), "{err}");

        // 文件存在时通过
        let dir = std::env::temp_dir().join("auroradb-ssl-validate-test");
        std::fs::create_dir_all(&dir).unwrap();
        let ca = dir.join("ca.pem");
        std::fs::write(&ca, b"dummy").unwrap();
        let ca_str = ca.to_str().unwrap().to_string();
        assert!(validate_ssl_files(&ssl(SslMode::VerifyCa, Some(&ca_str), None, None)).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_detect_tls_handshake_failure() {
        let corrupt = sqlx::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "received corrupt message of type UnknownProtocolVersion",
        ));
        assert!(is_tls_handshake_failure(&corrupt));

        // 普通网络错误不应触发明文回退
        let refused = sqlx::Error::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "Connection refused",
        ));
        assert!(!is_tls_handshake_failure(&refused));
    }
}
