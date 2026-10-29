//! 连接生命周期模块
//! 连接建立/断开、状态查询、心跳、事务的开始/提交/回滚。

use super::driver::{AnyPool, DriverKind};
use super::manager::{ConnectionManager, ManagedConnection};
use super::types::*;
use anyhow::{anyhow, Context, Result};
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions, MySqlSslMode};
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions, PgSslMode};
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

/// 构造 PostgreSQL 连接参数
///
/// `database` 为 None 时沿用服务端默认库（PG 协议中空库名等同未指定）。
fn build_pg_options(
    config: &ConnectionConfig,
    host: &str,
    port: u16,
    database: Option<&str>,
) -> PgConnectOptions {
    let ssl = &config.ssl_config;
    let mut opts = PgConnectOptions::new()
        .host(host)
        .port(port)
        .username(&config.username)
        .password(&config.password)
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
    if let Some(db) = database {
        opts = opts.database(db);
    }
    opts
}

impl ConnectionManager {
    /// 建立连接
    pub async fn connect(&self, request: ConnectionRequest) -> Result<String> {
        let id = request.config.id.clone();

        // 检查是否已存在
        {
            let connections = self.connections.read().await;
            if connections.contains_key(&id) {
                return Err(anyhow!("连接 {} 已存在", id));
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

        // 记录 MySQL 连接是否因 TLS 握手失败回退为明文，供备份/还原 CLI 保持一致
        let mut tls_disabled = false;

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
                        tls_disabled = true;
                        connect(MySqlSslMode::Disabled).await.map(AnyPool::MySql)?
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            DriverKind::Postgres => {
                // PG 的 db 名与 MySQL 的库等价，连接时直接指定；切换 schema 走 search_path。
                // 用户未填库时（空串）交给服务端决定，避免连到名为空串的库
                let database = request
                    .config
                    .database
                    .as_deref()
                    .map(str::trim)
                    .filter(|db| !db.is_empty());
                let opts = build_pg_options(&request.config, &host, port, database);

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

        // 回查实际落地的数据库名：配置未填库时 PG 由服务端决定（默认与用户名同名），
        // 前端在连接节点上标注该值，避免把 schema 误当成数据库
        let current_database = match &pool {
            AnyPool::Postgres(pg) => {
                sqlx::query_scalar::<_, String>("SELECT current_database()")
                    .fetch_one(pg)
                    .await
                    .ok()
            }
            AnyPool::MySql(mysql) => {
                sqlx::query_scalar::<_, Option<String>>("SELECT DATABASE()")
                    .fetch_one(mysql)
                    .await
                    .ok()
                    .flatten()
            }
        };

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
            current_database,
            state: ConnectionState::Connected,
            connected_at: Some(Instant::now()),
            last_error: None,
            heartbeat_handle: Some(heartbeat_handle),
            transaction: Arc::new(Mutex::new(None)),
            query_cancel: Arc::new(AtomicBool::new(false)),
            query_notify: Arc::new(Notify::new()),
            query_running: Arc::new(AtomicBool::new(false)),
            session: Arc::new(Mutex::new(None)),
            metadata_cache: Arc::new(Mutex::new(HashMap::new())),
            tunnel,
            tls_disabled,
        };

        self.connections.write().await.insert(id.clone(), managed);

        Ok(id)
    }

    /// 断开连接
    ///
    /// 连接表写锁只用于摘除条目，`pool.close().await` 与隧道关闭都放在锁外执行：
    /// 关池可能等待在途查询返回，持锁关闭会让其他连接的状态查询/执行全部阻塞。
    pub async fn disconnect(&self, connection_id: &str) -> Result<()> {
        let removed = {
            let mut connections = self.connections.write().await;
            connections.remove(connection_id)
        };

        if let Some(mut conn) = removed {
            // 停止心跳
            if let Some(handle) = conn.heartbeat_handle.take() {
                handle.abort();
            }

            // 先丢弃事务（Drop 自动回滚），再关闭池，避免池关闭等待被事务占用的物理连接
            conn.transaction = Arc::new(Mutex::new(None));
            let session = conn.session.clone();
            let pool = conn.pool.take();

            // 关闭 SSH 隧道：Drop 中终止本地监听任务并释放 SSH 会话
            conn.tunnel = None;

            conn.state = ConnectionState::Disconnected;
            drop(conn);

            // 释放持久会话连接（归还池后随池关闭一并销毁）
            session.lock().await.take();

            if let Some(pool) = pool {
                pool.close().await;
            }
        }

        // 关闭该连接按需建立的按库附加池
        let extra_pools: Vec<PgPool> = {
            let mut pools = self.pg_pools.lock().await;
            let keys: Vec<(String, String)> = pools
                .keys()
                .filter(|(id, _)| id == connection_id)
                .cloned()
                .collect();
            keys.into_iter().filter_map(|k| pools.remove(&k)).collect()
        };
        for pool in extra_pools {
            pool.close().await;
        }

        Ok(())
    }

    /// 获取连接状态
    pub async fn get_status(&self, connection_id: &str) -> Result<ConnectionStatus> {
        let connections = self.connections.read().await;

        let conn = connections
            .get(connection_id)
            .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;

        Ok(ConnectionStatus {
            id: connection_id.to_string(),
            driver: conn.config.driver,
            database: conn.current_database.clone(),
            state: state_key(&conn.state).to_string(),
            message: state_message(&conn.state),
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

    /// 连接的驱动类型
    pub(super) async fn driver_of(&self, connection_id: &str) -> Result<DriverKind> {
        let connections = self.connections.read().await;
        connections
            .get(connection_id)
            .map(|conn| conn.config.driver)
            .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))
    }

    /// PG 主池（连接时指定的库；未指定时为服务端默认库）与实际落地的库名
    pub(super) async fn pg_main_pool(&self, connection_id: &str) -> Result<(PgPool, Option<String>)> {
        let connections = self.connections.read().await;
        let conn = connections
            .get(connection_id)
            .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;
        let pool = conn
            .pool
            .as_ref()
            .ok_or_else(|| anyhow!("连接不可用: {}", connection_id))?;
        match pool {
            AnyPool::Postgres(pg) => Ok((pg.clone(), conn.current_database.clone())),
            AnyPool::MySql(_) => Err(anyhow!(
                "连接 {} 不是 PostgreSQL 连接",
                connection_id
            )),
        }
    }

    /// 取指定数据库的 PG 池
    ///
    /// PG 的数据库无法在同一连接内切换：与主库相同时复用主池，否则按需建立附加池并缓存，
    /// 断开连接时统一关闭。SSH 隧道场景下附加池同样连本地转发端口。
    pub(super) async fn pg_pool_for(&self, connection_id: &str, database: &str) -> Result<PgPool> {
        let (main_pool, current_database, config, host, port) = {
            let connections = self.connections.read().await;
            let conn = connections
                .get(connection_id)
                .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;
            let pool = conn
                .pool
                .as_ref()
                .ok_or_else(|| anyhow!("连接不可用: {}", connection_id))?;
            let main_pool = match pool {
                AnyPool::Postgres(pg) => pg.clone(),
                AnyPool::MySql(_) => {
                    return Err(anyhow!(
                        "连接 {} 不是 PostgreSQL 连接",
                        connection_id
                    ))
                }
            };
            // 隧道存在时数据库只监听在本地转发端口
            let (host, port) = match &conn.tunnel {
                Some(tunnel) => ("127.0.0.1".to_string(), tunnel.local_port()),
                None => (conn.config.host.clone(), conn.config.port),
            };
            (
                main_pool,
                conn.current_database.clone(),
                conn.config.clone(),
                host,
                port,
            )
        };

        if current_database.as_deref() == Some(database) {
            return Ok(main_pool);
        }

        let key = (connection_id.to_string(), database.to_string());
        if let Some(pool) = self.pg_pools.lock().await.get(&key) {
            return Ok(pool.clone());
        }

        // 附加池只为浏览数据/查询元数据服务，连接数按小池配置即可
        let opts = build_pg_options(&config, &host, port, Some(database));
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .min_connections(0)
            .acquire_timeout(Duration::from_secs(config.pool_config.connect_timeout))
            .idle_timeout(Duration::from_secs(config.pool_config.idle_timeout))
            .max_lifetime(Duration::from_secs(config.pool_config.max_lifetime))
            .connect_with(opts)
            .await
            .map_err(|e| anyhow!("连接数据库 {} 失败：{}", database, e))?;

        // 并发下可能已有同库的池，保留先插入者，关闭后建的
        let mut pools = self.pg_pools.lock().await;
        let existing = pools.get(&key).cloned();
        match existing {
            Some(existing) => {
                drop(pools);
                pool.close().await;
                Ok(existing)
            }
            None => {
                pools.insert(key, pool.clone());
                Ok(pool)
            }
        }
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
            // 单次心跳失败可能只是瞬时网络抖动，连续多次失败才判定连接失效
            const MAX_CONSECUTIVE_FAILURES: u32 = 3;
            let mut consecutive_failures: u32 = 0;

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
                    match result {
                        Ok(()) => {
                            consecutive_failures = 0;
                            conn.state = ConnectionState::Connected;
                            conn.last_error = None;
                        }
                        Err(e) => {
                            consecutive_failures += 1;
                            tracing::warn!(
                                "Heartbeat failed for connection {} ({}/{}): {}",
                                connection_id,
                                consecutive_failures,
                                MAX_CONSECUTIVE_FAILURES,
                                e
                            );
                            if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                                let message = format!("心跳连续失败 {} 次：{}", consecutive_failures, e);
                                conn.last_error = Some(message.clone());
                                conn.state = ConnectionState::Failed(message);
                            } else {
                                conn.last_error = Some(format!("心跳失败：{}", e));
                                conn.state = ConnectionState::Reconnecting {
                                    attempt: consecutive_failures,
                                };
                            }
                        }
                    }
                }
            }
        })
    }

    /// 开始事务：从池中钉扎一条物理连接，COMMIT/ROLLBACK 前所有语句都在该连接上执行
    pub async fn begin_transaction(&self, connection_id: &str) -> Result<()> {
        let connections = self.connections.read().await;
        let conn = connections.get(connection_id)
            .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;

        let pool = conn.pool.as_ref()
            .ok_or_else(|| anyhow!("连接尚未建立"))?;

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
            .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;

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
            .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;

        let mut txn_guard = conn.transaction.lock().await;
        let txn = txn_guard.take()
            .ok_or_else(|| anyhow!("没有正在进行的事务"))?;
        txn.rollback().await.context("回滚事务失败")?;

        Ok(())
    }
}

/// `ConnectionStatus.state` 的取值：前端按它做分支判断（InfoPanel / ConnectionTree），
/// 必须是稳定的小写单词，不能直接格式化枚举的 Debug 输出。
fn state_key(state: &ConnectionState) -> &'static str {
    match state {
        ConnectionState::Disconnected => "disconnected",
        ConnectionState::Connecting => "connecting",
        ConnectionState::Connected => "connected",
        ConnectionState::Reconnecting { .. } => "reconnecting",
        ConnectionState::Failed(_) => "failed",
    }
}

/// `ConnectionStatus.message` 的展示文案；`Failed` 透传底层错误信息。
fn state_message(state: &ConnectionState) -> String {
    match state {
        ConnectionState::Connected => "Connected".to_string(),
        ConnectionState::Connecting => "Connecting...".to_string(),
        ConnectionState::Reconnecting { attempt } => {
            format!("Reconnecting (attempt {})", attempt)
        }
        ConnectionState::Failed(msg) => msg.clone(),
        ConnectionState::Disconnected => "Disconnected".to_string(),
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

    #[test]
    fn test_status_state_keys_are_stable_words() {
        // 前端（InfoPanel / ConnectionTree）按 state 做分支，取值变更需同步前端映射
        assert_eq!(state_key(&ConnectionState::Disconnected), "disconnected");
        assert_eq!(state_key(&ConnectionState::Connecting), "connecting");
        assert_eq!(state_key(&ConnectionState::Connected), "connected");
        assert_eq!(
            state_key(&ConnectionState::Reconnecting { attempt: 3 }),
            "reconnecting"
        );
        assert_eq!(state_key(&ConnectionState::Failed("x".into())), "failed");
    }

    #[test]
    fn test_status_message() {
        assert_eq!(state_message(&ConnectionState::Connected), "Connected");
        assert_eq!(state_message(&ConnectionState::Connecting), "Connecting...");
        assert_eq!(
            state_message(&ConnectionState::Reconnecting { attempt: 2 }),
            "Reconnecting (attempt 2)"
        );
        assert_eq!(state_message(&ConnectionState::Disconnected), "Disconnected");

        // Failed 透传底层错误原文（已由 error.rs 统一中文化），不能二次翻译丢失细节
        let failed = ConnectionState::Failed("认证失败（MySQL 1045：Access denied）".into());
        assert_eq!(
            state_message(&failed),
            "认证失败（MySQL 1045：Access denied）"
        );
    }
}
