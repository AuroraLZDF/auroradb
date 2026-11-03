//! SSH 隧道抽象
//!
//! 免费核心只定义隧道接口与工厂，具体实现位于专业版 crate。
//! 连接建立时通过工厂创建隧道，数据库实际连接本地转发端口。

use super::types::SshConfig;
use anyhow::Result;
use futures_util::future::BoxFuture;
use std::sync::Arc;

/// 一条已建立的本地转发隧道，随 `ManagedConnection` 存活，Drop 时关闭。
pub trait Tunnel: Send + Sync {
    /// 本地转发端口，数据库应连接 `127.0.0.1:local_port`
    fn local_port(&self) -> u16;
}

/// 隧道工厂：参数依次为 SSH 配置、SSH 服务器侧数据库主机、端口。
pub type TunnelFactory = Arc<
    dyn Fn(SshConfig, String, u16) -> BoxFuture<'static, Result<Box<dyn Tunnel>>> + Send + Sync,
>;
