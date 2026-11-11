//! 方言差异
//!
//! 只收敛驱动之间必须区分的 SQL 片段：标识符引号、切库语句，
//! 以及是否允许自动补库名前缀。元数据/DDL/监控等方言留到后续批次再扩展。

use super::driver::DriverKind;

impl DriverKind {
    /// 包装标识符（内部转义引号，不做合法性校验）
    pub fn quote_ident(self, name: &str) -> String {
        match self {
            Self::MySql => format!("`{}`", name.replace('`', "``")),
            Self::Postgres | Self::Sqlite => format!("\"{}\"", name.replace('"', "\"\"")),
        }
    }

    /// 切换到指定库/模式的语句
    ///
    /// SQLite 单文件库没有切库概念：调用方应先按驱动分流，不要对 SQLite 调用本方法；
    /// 这里返回空串以保持签名统一，避免每个调用点为语句本身特判。
    pub fn switch_database_sql(self, database: &str) -> String {
        match self {
            Self::MySql => format!("USE {}", self.quote_ident(database)),
            Self::Postgres => format!("SET search_path TO {}", self.quote_ident(database)),
            Self::Sqlite => String::new(),
        }
    }

    /// 是否支持把未带前缀的表名自动补成「库.表」
    ///
    /// MySQL 的库即命名空间，可以安全补全；PostgreSQL 的 database 与 schema 是两层，
    /// 自动补 database 会改变语义，因此不启用；SQLite 是单文件库，没有库前缀概念。
    pub fn supports_db_prefix_rewrite(self) -> bool {
        matches!(self, Self::MySql)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quote_ident() {
        assert_eq!(DriverKind::MySql.quote_ident("users"), "`users`");
        assert_eq!(DriverKind::MySql.quote_ident("a`b"), "`a``b`");
        assert_eq!(DriverKind::Postgres.quote_ident("users"), "\"users\"");
        assert_eq!(DriverKind::Postgres.quote_ident("a\"b"), "\"a\"\"b\"");
        assert_eq!(DriverKind::Sqlite.quote_ident("users"), "\"users\"");
        assert_eq!(DriverKind::Sqlite.quote_ident("a\"b"), "\"a\"\"b\"");
    }

    #[test]
    fn test_switch_database() {
        assert_eq!(DriverKind::MySql.switch_database_sql("app"), "USE `app`");
        assert_eq!(
            DriverKind::Postgres.switch_database_sql("public"),
            "SET search_path TO \"public\""
        );
        // SQLite 单文件库无切库语句，返回空串（调用方按驱动分流）
        assert_eq!(DriverKind::Sqlite.switch_database_sql("main"), "");
    }

    #[test]
    fn test_db_prefix_rewrite_support() {
        assert!(DriverKind::MySql.supports_db_prefix_rewrite());
        assert!(!DriverKind::Postgres.supports_db_prefix_rewrite());
        assert!(!DriverKind::Sqlite.supports_db_prefix_rewrite());
    }
}
