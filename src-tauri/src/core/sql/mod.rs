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
    use sqlparser::ast::{
        CreateTableLikeKind, Ident, ObjectName, ObjectNamePart, ObjectType, Statement, VisitMut,
        VisitorMut,
    };
    use sqlparser::dialect::{MySqlDialect, PostgreSqlDialect};
    use sqlparser::parser::Parser;
    use std::ops::ControlFlow;

    if !driver.supports_db_prefix_rewrite() {
        let parsed = Parser::parse_sql(&PostgreSqlDialect {}, sql).is_ok();
        return (sql.to_string(), parsed);
    }

    /// 仅当表名没有 schema 前缀（如 `db`.`table`）时才添加前缀，保留跨库引用
    fn prefix_relation(relation: &mut ObjectName, database: &str) {
        if relation.0.len() == 1 {
            if let Some(ObjectNamePart::Identifier(ident)) = relation.0.first() {
                let table = ident.value.clone();
                // 数据库名和表名都需要用反引号包裹，以支持特殊字符（如连字符）
                relation.0 = vec![
                    ObjectNamePart::Identifier(Ident::with_quote('`', database)),
                    ObjectNamePart::Identifier(Ident::with_quote('`', table)),
                ];
            }
        }
    }

    struct TablePrefixer {
        database: String,
    }

    impl VisitorMut for TablePrefixer {
        type Break = ();

        fn pre_visit_relation(&mut self, relation: &mut ObjectName) -> ControlFlow<Self::Break> {
            prefix_relation(relation, &self.database);
            ControlFlow::Continue(())
        }
    }

    match Parser::parse_sql(&MySqlDialect {}, sql) {
        Ok(mut statements) => {
            let _ = statements.visit(&mut TablePrefixer {
                database: database.to_string(),
            });
            // CREATE TABLE ... LIKE/CLONE 的源表、DROP TABLE/VIEW 的目标表在 AST 中
            // 不是 relation 位置，visitor 不会访问到，需单独补前缀，
            // 否则会报 3D000（未选择数据库）
            for statement in &mut statements {
                match statement {
                    Statement::CreateTable(create) => {
                        if let Some(like) = &mut create.like {
                            let source = match like {
                                CreateTableLikeKind::Plain(like)
                                | CreateTableLikeKind::Parenthesized(like) => &mut like.name,
                            };
                            prefix_relation(source, database);
                        }
                        if let Some(clone) = &mut create.clone {
                            prefix_relation(clone, database);
                        }
                    }
                    Statement::Drop {
                        object_type,
                        names,
                        table,
                        ..
                    } => {
                        if matches!(
                            object_type,
                            ObjectType::Table | ObjectType::View | ObjectType::MaterializedView
                        ) {
                            for name in names.iter_mut() {
                                prefix_relation(name, database);
                            }
                        }
                        // MySQL 的 DROP INDEX idx ON tbl 语法：tbl 才是表名，索引名不加前缀
                        if *object_type == ObjectType::Index {
                            if let Some(table) = table {
                                prefix_relation(table, database);
                            }
                        }
                    }
                    _ => {}
                }
            }
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

/// 判断 SQL 是否会改变库/表结构（DDL）。
///
/// 用于执行成功后失效元数据缓存：对象树与表结构有 30 秒 TTL，
/// 建表/删表后若不失效，界面刷新会拿到旧快照。
pub fn is_schema_changing(sql: &str) -> bool {
    let upper = sql.trim_start().to_uppercase();
    const PREFIXES: &[&str] = &[
        "CREATE TABLE",
        "CREATE TEMPORARY TABLE",
        "CREATE VIEW",
        "CREATE OR REPLACE VIEW",
        "CREATE INDEX",
        "CREATE UNIQUE INDEX",
        "CREATE DATABASE",
        "CREATE SCHEMA",
        "ALTER TABLE",
        "ALTER VIEW",
        "ALTER DATABASE",
        "ALTER SCHEMA",
        "DROP TABLE",
        "DROP VIEW",
        "DROP INDEX",
        "DROP DATABASE",
        "DROP SCHEMA",
        "RENAME TABLE",
    ];
    PREFIXES.iter().any(|prefix| upper.starts_with(prefix))
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
    use super::{is_schema_changing, rewrite_table_names};
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
    fn test_rewrite_create_table_like() {
        let (sql, parsed) = rewrite_table_names(
            "mydb",
            "CREATE TABLE `t_copy` LIKE `users`",
            DriverKind::MySql,
        );
        assert!(parsed);
        assert!(sql.contains("`mydb`.`t_copy`"), "got: {sql}");
        assert!(sql.contains("`mydb`.`users`"), "got: {sql}");
    }

    #[test]
    fn test_rewrite_create_table_like_preserves_cross_database() {
        let (sql, parsed) = rewrite_table_names(
            "mydb",
            "CREATE TABLE t_copy LIKE otherdb.users",
            DriverKind::MySql,
        );
        assert!(parsed);
        assert!(sql.contains("`mydb`.`t_copy`"), "got: {sql}");
        assert!(sql.contains("otherdb.users"), "got: {sql}");
    }

    #[test]
    fn test_rewrite_drop_table() {
        let (sql, parsed) = rewrite_table_names(
            "mydb",
            "DROP TABLE IF EXISTS `users`",
            DriverKind::MySql,
        );
        assert!(parsed);
        assert!(sql.contains("`mydb`.`users`"), "got: {sql}");
    }

    #[test]
    fn test_rewrite_drop_view_preserves_cross_database() {
        let (sql, parsed) = rewrite_table_names(
            "mydb",
            "DROP VIEW IF EXISTS v1, otherdb.v2",
            DriverKind::MySql,
        );
        assert!(parsed);
        assert!(sql.contains("`mydb`.`v1`"), "got: {sql}");
        assert!(sql.contains("otherdb.v2"), "got: {sql}");
    }

    #[test]
    fn test_rewrite_drop_index_keeps_index_name() {
        let (sql, parsed) = rewrite_table_names(
            "mydb",
            "DROP INDEX `idx_name` ON `users`",
            DriverKind::MySql,
        );
        assert!(parsed);
        assert!(sql.contains("`mydb`.`users`"), "got: {sql}");
        assert!(!sql.contains("`mydb`.`idx_name`"), "got: {sql}");
    }

    #[test]
    fn test_is_schema_changing() {
        assert!(is_schema_changing("CREATE TABLE `t` (id INT)"));
        assert!(is_schema_changing("  drop table `t`"));
        assert!(is_schema_changing("ALTER TABLE `t` ADD COLUMN c INT"));
        assert!(is_schema_changing("CREATE OR REPLACE VIEW v AS SELECT 1"));
        assert!(is_schema_changing("RENAME TABLE a TO b"));
        assert!(!is_schema_changing("SELECT * FROM t"));
        assert!(!is_schema_changing("INSERT INTO t VALUES (1)"));
        assert!(!is_schema_changing("TRUNCATE TABLE t"));
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
