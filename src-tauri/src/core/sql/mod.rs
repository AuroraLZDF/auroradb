//! SQL 文本处理模块
//! 纯文本函数：表名重写与语句分割，无连接/状态依赖。

pub mod safety;

use crate::core::connection::driver::DriverKind;

/// 使用 sqlparser 解析 SQL，为所有未带 schema 前缀的表名添加数据库前缀。
/// 解析失败时回退返回原始 SQL，避免破坏 MySQL 特有语法。
/// 返回 (重写后的 SQL, 是否解析成功)。
///
/// 仅 MySQL 做库名前缀重写：PostgreSQL 的 database 与 schema 是两层结构，
/// 自动补 database 会改变语义，因此只解析取得「可解析」标志，SQL 原样返回。
pub fn rewrite_table_names(database: &str, sql: &str, driver: DriverKind) -> (String, bool) {
    use sqlparser::ast::{Ident, ObjectName, ObjectNamePart, VisitMut, VisitorMut};
    use sqlparser::dialect::{MySqlDialect, PostgreSqlDialect};
    use sqlparser::parser::Parser;
    use std::ops::ControlFlow;

    if !driver.supports_db_prefix_rewrite() {
        let parsed = Parser::parse_sql(&PostgreSqlDialect {}, sql).is_ok();
        return (sql.to_string(), parsed);
    }

    struct TablePrefixer {
        database: String,
    }

    impl VisitorMut for TablePrefixer {
        type Break = ();

        fn pre_visit_relation(&mut self, relation: &mut ObjectName) -> ControlFlow<Self::Break> {
            // 仅当表名没有 schema 前缀（如 `db`.`table`）时才添加前缀，保留跨库引用
            if relation.0.len() == 1 {
                if let Some(ObjectNamePart::Identifier(ident)) = relation.0.first() {
                    let table = ident.value.clone();
                    // 数据库名和表名都需要用反引号包裹，以支持特殊字符（如连字符）
                    relation.0 = vec![
                        ObjectNamePart::Identifier(Ident::with_quote('`', self.database.clone())),
                        ObjectNamePart::Identifier(Ident::with_quote('`', table)),
                    ];
                }
            }
            ControlFlow::Continue(())
        }
    }

    match Parser::parse_sql(&MySqlDialect {}, sql) {
        Ok(mut statements) => {
            let _ = statements.visit(&mut TablePrefixer {
                database: database.to_string(),
            });
            let rewritten = statements
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            (rewritten, true)
        }
        Err(_) => (sql.to_string(), false),
    }
}

/// 安全分割 SQL 语句，忽略字符串字面量、反引号标识符和注释中的分号
pub fn split_sql_statements(sql: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut chars = sql.chars().peekable();

    // 状态标记
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut in_backtick = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;

    while let Some(c) = chars.next() {
        // 处理行注释结束
        if in_line_comment {
            if c == '\n' {
                in_line_comment = false;
                current.push(c);
            }
            continue;
        }

        // 处理块注释结束
        if in_block_comment {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block_comment = false;
            }
            continue;
        }

        // 处理转义字符（单引号/双引号内）
        if (in_single_quote || in_double_quote) && c == '\\' {
            current.push(c);
            if let Some(next) = chars.next() {
                current.push(next);
            }
            continue;
        }

        // 处理单引号
        if c == '\'' && !in_double_quote && !in_backtick {
            // 检查是否为转义的单引号 ''
            if in_single_quote && chars.peek() == Some(&'\'') {
                current.push(c);
                current.push(chars.next().unwrap());
                continue;
            }
            in_single_quote = !in_single_quote;
            current.push(c);
            continue;
        }

        // 处理双引号
        if c == '"' && !in_single_quote && !in_backtick {
            in_double_quote = !in_double_quote;
            current.push(c);
            continue;
        }

        // 处理反引号
        if c == '`' && !in_single_quote && !in_double_quote {
            in_backtick = !in_backtick;
            current.push(c);
            continue;
        }

        // 处理注释开始（仅在非字符串状态）
        if !in_single_quote && !in_double_quote && !in_backtick {
            // 行注释 --
            if c == '-' && chars.peek() == Some(&'-') {
                in_line_comment = true;
                chars.next();
                continue;
            }
            // 块注释 /*
            if c == '/' && chars.peek() == Some(&'*') {
                in_block_comment = true;
                chars.next();
                continue;
            }
        }

        // 处理分号（语句分隔符）
        if c == ';' && !in_single_quote && !in_double_quote && !in_backtick {
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                statements.push(trimmed.to_string());
            }
            current.clear();
            continue;
        }

        current.push(c);
    }

    // 处理最后一条没有分号结尾的语句
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        statements.push(trimmed.to_string());
    }

    statements
}

#[cfg(test)]
mod tests {
    use super::rewrite_table_names;
    use crate::core::connection::driver::DriverKind;

    #[test]
    fn test_rewrite_simple_select() {
        let (sql, parsed) =
            rewrite_table_names("mydb", "SELECT * FROM users", DriverKind::MySql);
        assert!(parsed);
        assert!(sql.contains("`mydb`.`users`"), "got: {sql}");
        assert!(!sql.contains("FROM users"), "got: {sql}");
    }

    #[test]
    fn test_rewrite_join() {
        let (sql, parsed) = rewrite_table_names(
            "mydb",
            "SELECT * FROM users JOIN orders ON users.id = orders.user_id",
            DriverKind::MySql,
        );
        assert!(parsed);
        assert!(sql.contains("`mydb`.`users`"), "got: {sql}");
        assert!(sql.contains("`mydb`.`orders`"), "got: {sql}");
    }

    #[test]
    fn test_rewrite_subquery() {
        let (sql, parsed) = rewrite_table_names(
            "mydb",
            "SELECT * FROM (SELECT * FROM users) AS u",
            DriverKind::MySql,
        );
        assert!(parsed);
        assert!(sql.contains("`mydb`.`users`"), "got: {sql}");
    }

    #[test]
    fn test_rewrite_insert_select() {
        let (sql, parsed) = rewrite_table_names(
            "mydb",
            "INSERT INTO logs SELECT * FROM events",
            DriverKind::MySql,
        );
        assert!(parsed);
        assert!(sql.contains("`mydb`.`logs`"), "got: {sql}");
        assert!(sql.contains("`mydb`.`events`"), "got: {sql}");
    }

    #[test]
    fn test_rewrite_update_join() {
        let (sql, parsed) = rewrite_table_names(
            "mydb",
            "UPDATE users JOIN orders ON users.id = orders.user_id SET users.name = 'x'",
            DriverKind::MySql,
        );
        assert!(parsed);
        assert!(sql.contains("`mydb`.`users`"), "got: {sql}");
        assert!(sql.contains("`mydb`.`orders`"), "got: {sql}");
    }

    #[test]
    fn test_rewrite_preserves_cross_database() {
        let (sql, parsed) =
            rewrite_table_names("mydb", "SELECT * FROM otherdb.users", DriverKind::MySql);
        assert!(parsed);
        assert!(sql.contains("otherdb.users"), "got: {sql}");
        assert!(!sql.contains("mydb.otherdb"), "got: {sql}");
    }

    #[test]
    fn test_rewrite_truncate() {
        let (sql, parsed) =
            rewrite_table_names("mydb", "TRUNCATE TABLE users", DriverKind::MySql);
        assert!(parsed);
        assert!(sql.contains("`mydb`.`users`"), "got: {sql}");
    }

    #[test]
    fn test_postgres_keeps_sql_unmodified() {
        let input = "SELECT * FROM users WHERE id = 1";
        let (sql, parsed) = rewrite_table_names("public", input, DriverKind::Postgres);
        assert!(parsed);
        assert_eq!(sql, input);
    }

    #[test]
    fn test_postgres_reports_unparsable_sql() {
        // PG 方言下无法解析的语句原样返回并标记 parsed=false
        let input = "SELECT * FROM users WHERE id =";
        let (sql, parsed) = rewrite_table_names("public", input, DriverKind::Postgres);
        assert!(!parsed);
        assert_eq!(sql, input);
    }
}
