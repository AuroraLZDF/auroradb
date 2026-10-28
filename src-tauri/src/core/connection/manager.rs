//! 连接管理器核心类型定义
//! 仅持有连接注册表与连接状态的类型定义；各方法实现拆分到 session/executor/metadata 兄弟模块。

use super::driver::{AnyPool, AnyTransaction};
use super::tunnel::SshTunnel;
use super::types::*;
use anyhow::{anyhow, Result};
use sqlx::pool::PoolConnection;
use sqlx::{MySql, MySqlPool, PgPool};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, Notify, RwLock};

/// 连接管理器
pub struct ConnectionManager {
    pub(super) connections: Arc<RwLock<HashMap<String, ManagedConnection>>>,
    /// PG 按库附加的连接池：key 为 (连接 ID, 数据库名)
    ///
    /// PG 的数据库在建立连接时固定、无法在同一连接内切换，浏览非默认库只能另开连接池；
    /// 按需建立并缓存，断开连接时统一关闭。
    pub(super) pg_pools: Arc<Mutex<HashMap<(String, String), PgPool>>>,
}

/// 管理的连接
pub struct ManagedConnection {
    pub config: ConnectionConfig,
    pub pool: Option<AnyPool>,
    /// 建立连接后实际落在的数据库名：配置未填库时 PG 由服务端决定（默认与用户名同名），
    /// 前端据此在连接节点上标注当前库，避免把 schema 误认为数据库
    pub current_database: Option<String>,
    pub state: ConnectionState,
    pub connected_at: Option<Instant>,
    pub last_error: Option<String>,
    pub heartbeat_handle: Option<tokio::task::JoinHandle<()>>,
    /// 进行中事务：BEGIN 时钉扎一条物理连接，COMMIT/ROLLBACK 前所有语句都在该连接上执行
    pub transaction: Arc<Mutex<Option<AnyTransaction>>>,
    /// 查询取消信号：cancel_query 置位并唤醒，执行中的查询据此终止（配合 query_notify 双保险）
    pub query_cancel: Arc<AtomicBool>,
    pub query_notify: Arc<Notify>,
    /// 是否有查询正在执行：取消时据此判断是否真的需要丢弃事务（无在途查询时不丢用户事务）
    pub query_running: Arc<AtomicBool>,
    /// MySQL 持久会话连接：`USE` / `SET` 这类会话级语句必须与后续查询落在同一条物理连接上，
    /// 且不能随语句结束归还连接池（否则会话语义丢失并污染池中连接的默认库）。
    /// 懒创建：只有在用户执行过会话语句后才占用一条物理连接。
    pub session: Arc<Mutex<Option<PoolConnection<MySql>>>>,
    /// 元数据缓存：对象树/表结构带 TTL，避免每次展开都打 INFORMATION_SCHEMA
    pub metadata_cache: Arc<Mutex<HashMap<String, (Instant, MetadataCacheValue)>>>,
    /// SSH 隧道：连接期间必须存活，断开时置 None 触发 Drop 关闭
    pub tunnel: Option<SshTunnel>,
}

impl ManagedConnection {
    /// 取 MySQL 池的克隆；仅供尚未迁移方言的模块使用，PG 连接返回带功能名的明确错误
    pub fn mysql_pool(&self, feature: &str) -> Result<MySqlPool> {
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| anyhow!("连接尚未建立"))?;
        Ok(pool.as_mysql(feature)?.clone())
    }
}

/// 在固定物理连接上执行语句须走 `Executor` trait 方法（如 `Executor::execute(&mut conn, sql)`），
/// 而非 `RawSql::execute/fetch_all`——后者是 async fn，返回的 opaque future 会触发
/// "implementation of Executor is not general enough" 推断冲突；前者直接返回 BoxFuture。
/// `&str` 的 Execute 实现不携带参数（简单文本协议），与 sqlx::raw_sql 行为一致。
const _: () = ();

/// 元数据缓存条目：对象树、表结构或 schema 快照
#[derive(Clone)]
pub(crate) enum MetadataCacheValue {
    ObjectTree(Vec<DatabaseObject>),
    TableStructure(TableStructure),
    SchemaSnapshot(SchemaSnapshot),
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            pg_pools: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 连接的展示信息（名称、主机、端口），供审计日志记录操作来源
    pub async fn connection_meta(&self, connection_id: &str) -> Option<(String, String, u16)> {
        let connections = self.connections.read().await;
        connections
            .get(connection_id)
            .map(|conn| (conn.config.name.clone(), conn.config.host.clone(), conn.config.port))
    }
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}
