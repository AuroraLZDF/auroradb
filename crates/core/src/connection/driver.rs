//! 驱动抽象
//!
//! 用枚举包装各驱动自己的池/连接/事务/行类型，避免 `sqlx::Any` 在类型推断、
//! 参数绑定与扩展类型解码上的限制。方言相关的 SQL 片段见 `dialect.rs`。

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sqlx::mysql::{MySqlConnection, MySqlPool};
use sqlx::postgres::{PgConnection, PgPool};
use sqlx::sqlite::{SqliteConnection, SqlitePool};
use sqlx::{MySql, Postgres, Sqlite};

/// 数据库驱动类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DriverKind {
    /// MySQL / MariaDB
    #[default]
    MySql,
    /// PostgreSQL
    Postgres,
    /// SQLite（本地单文件库）
    Sqlite,
}

impl DriverKind {
    /// 用于提示文案的展示名
    pub fn display_name(self) -> &'static str {
        match self {
            Self::MySql => "MySQL",
            Self::Postgres => "PostgreSQL",
            Self::Sqlite => "SQLite",
        }
    }
}

/// 连接池的驱动无关包装
#[derive(Debug, Clone)]
pub enum AnyPool {
    MySql(MySqlPool),
    Postgres(PgPool),
    Sqlite(SqlitePool),
}

impl AnyPool {
    pub fn driver(&self) -> DriverKind {
        match self {
            Self::MySql(_) => DriverKind::MySql,
            Self::Postgres(_) => DriverKind::Postgres,
            Self::Sqlite(_) => DriverKind::Sqlite,
        }
    }

    /// 关闭连接池（等待所有连接归还后释放）
    pub async fn close(&self) {
        match self {
            Self::MySql(pool) => pool.close().await,
            Self::Postgres(pool) => pool.close().await,
            Self::Sqlite(pool) => pool.close().await,
        }
    }

    /// 连通性检测：执行 `SELECT 1`，失败即视为连接不可用
    pub async fn ping(&self) -> std::result::Result<(), sqlx::Error> {
        match self {
            Self::MySql(pool) => {
                let _: i32 = sqlx::query_scalar("SELECT 1").fetch_one(pool).await?;
            }
            Self::Postgres(pool) => {
                let _: i32 = sqlx::query_scalar("SELECT 1").fetch_one(pool).await?;
            }
            Self::Sqlite(pool) => {
                let _: i32 = sqlx::query_scalar("SELECT 1").fetch_one(pool).await?;
            }
        }
        Ok(())
    }

    /// 从池中钉扎一条物理连接并开启事务
    pub async fn begin(&self) -> std::result::Result<AnyTransaction, sqlx::Error> {
        match self {
            Self::MySql(pool) => pool.begin().await.map(AnyTransaction::MySql),
            Self::Postgres(pool) => pool.begin().await.map(AnyTransaction::Postgres),
            Self::Sqlite(pool) => pool.begin().await.map(AnyTransaction::Sqlite),
        }
    }

    /// 取 MySQL 池；仅未迁移方言的模块使用，PG 连接返回带功能名的明确错误
    pub fn as_mysql(&self, feature: &str) -> Result<&MySqlPool> {
        match self {
            Self::MySql(pool) => Ok(pool),
            Self::Postgres(_) | Self::Sqlite(_) => Err(anyhow!(
                "「{}」尚未支持 {}，将在后续批次提供",
                feature,
                self.driver().display_name()
            )),
        }
    }

    /// 取 SQLite 池
    pub fn as_sqlite(&self, feature: &str) -> Result<&sqlx::SqlitePool> {
        match self {
            Self::Sqlite(pool) => Ok(pool),
            Self::MySql(_) | Self::Postgres(_) => Err(anyhow!(
                "「{}」仅支持 SQLite 连接，当前驱动为 {}",
                feature,
                self.driver().display_name()
            )),
        }
    }
}

/// 事务的驱动无关包装
pub enum AnyTransaction {
    MySql(sqlx::Transaction<'static, MySql>),
    Postgres(sqlx::Transaction<'static, Postgres>),
    Sqlite(sqlx::Transaction<'static, Sqlite>),
}

impl AnyTransaction {
    pub fn driver(&self) -> DriverKind {
        match self {
            Self::MySql(_) => DriverKind::MySql,
            Self::Postgres(_) => DriverKind::Postgres,
            Self::Sqlite(_) => DriverKind::Sqlite,
        }
    }

    /// 取 MySQL 事务连接；仅未迁移方言的模块使用，PG 事务返回明确错误
    pub fn as_mysql_connection(&mut self) -> Result<&mut MySqlConnection> {
        match self {
            Self::MySql(txn) => Ok(&mut **txn),
            Self::Postgres(_) | Self::Sqlite(_) => Err(anyhow!(
                "该功能尚未支持 {}，将在后续批次提供",
                self.driver().display_name()
            )),
        }
    }

    /// 取 PostgreSQL 事务连接；MySQL 事务返回明确错误
    pub fn as_pg_connection(&mut self) -> Result<&mut PgConnection> {
        match self {
            Self::Postgres(txn) => Ok(&mut **txn),
            Self::MySql(_) | Self::Sqlite(_) => Err(anyhow!(
                "当前连接不是 PostgreSQL，无法在事务中执行该语句"
            )),
        }
    }

    /// 取 SQLite 事务连接；其它驱动返回明确错误
    pub fn as_sqlite_connection(&mut self) -> Result<&mut SqliteConnection> {
        match self {
            Self::Sqlite(txn) => Ok(&mut **txn),
            Self::MySql(_) | Self::Postgres(_) => Err(anyhow!(
                "当前连接不是 SQLite，无法在事务中执行该语句"
            )),
        }
    }

    pub async fn commit(self) -> Result<()> {
        match self {
            Self::MySql(txn) => txn.commit().await,
            Self::Postgres(txn) => txn.commit().await,
            Self::Sqlite(txn) => txn.commit().await,
        }
        .map_err(Into::into)
    }

    pub async fn rollback(self) -> Result<()> {
        match self {
            Self::MySql(txn) => txn.rollback().await,
            Self::Postgres(txn) => txn.rollback().await,
            Self::Sqlite(txn) => txn.rollback().await,
        }
        .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn postgres_reports_feature_name_for_unmigrated_pool() {
        // 未迁移模块在 PG 下必须给出可读提示，而不是让 MySQL 语法错误暴露给用户
        let pool = PgPool::connect_lazy("postgres://user@localhost/db").unwrap();
        let err = AnyPool::Postgres(pool).as_mysql("浏览表数据").unwrap_err();
        assert_eq!(
            err.to_string(),
            "「浏览表数据」尚未支持 PostgreSQL，将在后续批次提供"
        );
    }
}
