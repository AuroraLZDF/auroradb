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
            Self::Postgres => format!("\"{}\"", name.replace('"', "\"\"")),
        }
    }

    /// 切换到指定库/模式的语句
    pub fn switch_database_sql(self, database: &str) -> String {
        match self {
            Self::MySql => format!("USE {}", self.quote_ident(database)),
            Self::Postgres => format!("SET search_path TO {}", self.quote_ident(database)),
        }
    }

    /// 是否支持把未带前缀的表名自动补成「库.表」
    ///
    /// MySQL 的库即命名空间，可以安全补全；PostgreSQL 的 database 与 schema 是两层，
    /// 自动补 database 会改变语义，因此不启用。
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
    }

    #[test]
    fn test_switch_database() {
        assert_eq!(DriverKind::MySql.switch_database_sql("app"), "USE `app`");
        assert_eq!(
            DriverKind::Postgres.switch_database_sql("public"),
            "SET search_path TO \"public\""
        );
    }

    #[test]
    fn test_db_prefix_rewrite_support() {
        assert!(DriverKind::MySql.supports_db_prefix_rewrite());
        assert!(!DriverKind::Postgres.supports_db_prefix_rewrite());
    }
}
