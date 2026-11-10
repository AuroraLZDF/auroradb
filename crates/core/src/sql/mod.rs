//! SQL 文本处理模块
//! 纯文本函数：表名重写与语句分割，无连接/状态依赖。

pub mod safety;

use crate::connection::driver::DriverKind;

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

    // sqlparser 重新序列化 AST 时会丢弃注释，而 MySQL 的可执行注释 `/*! ... */`
    // 是会被服务端执行的真实语句（如 `/*!40101 SET NAMES utf8 */`），
    // 重写后丢失会改变语义，因此遇到可执行注释时原样返回。
    if sql.contains("/*!") {
        let parsed = Parser::parse_sql(&MySqlDialect {}, sql).is_ok();
        return (sql.to_string(), parsed);
    }

    // 普通注释不参与语义，先剥离再解析，避免序列化时把注释丢掉后再拼接出错误语句
    // （例如 `-- 注释` 后跟关键字时被合并成非法 token）。
    let sql_without_comments = strip_sql_comments(sql);
    let sql_for_parse = if sql_without_comments.trim().is_empty() {
        sql
    } else {
        sql_without_comments.as_str()
    };

    match Parser::parse_sql(&MySqlDialect {}, sql_for_parse) {
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

/// 判断当前 `-` 是否开始一段 `--` 行注释。
///
/// MySQL 规定 `--` 后必须跟至少一个空白或控制字符才是注释，
/// 否则是两个连续的减号，如 `SELECT 1--1` 表示 `1 - (-1)`。
/// 只比较两个 `-` 会把后者误判为注释，导致语句被静默截断。
/// 行尾（`--` 后没有字符）同样不算注释：截断成合法语句比报错更危险。
///
/// 调用方传入的迭代器位置在第一个 `-` 之后。
fn is_line_comment_start(chars: &std::iter::Peekable<std::str::Chars<'_>>) -> bool {
    let mut lookahead = chars.clone();
    if lookahead.next() != Some('-') {
        return false;
    }
    matches!(lookahead.next(), Some(next) if next.is_whitespace())
}

/// 剥离 SQL 中的普通注释（`--` 行注释、`#` 行注释、`/* */` 块注释），
/// 字符串字面量与反引号标识符内的内容原样保留。
/// 注释被替换为一个空格，避免前后 token 直接粘连。
fn strip_sql_comments(sql: &str) -> String {
    strip_comments(sql, false)
}

/// 剥离普通注释，但保留 MySQL 可执行注释（`/*! ... */`，版本号可选）里的语句体。
///
/// 可执行注释会被服务端当作真实 SQL 执行，分类与结构变更判断不能像普通注释一样整段丢弃；
/// 这里只去掉注释外壳（`/*!`、版本号与结尾 `*/`），保留中间文本参与后续判断。
fn strip_comments_keep_executable(sql: &str) -> String {
    strip_comments(sql, true)
}

/// 剥离注释的公共实现；`keep_executable` 为 true 时保留可执行注释体。
fn strip_comments(sql: &str, keep_executable: bool) -> String {
    let mut result = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();

    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut in_backtick = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut in_executable_comment = false;

    while let Some(c) = chars.next() {
        // 可执行注释体原样保留（去掉结尾的 `*/`），供注释感知的分类逻辑使用
        if in_executable_comment {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_executable_comment = false;
                result.push(' ');
            } else {
                result.push(c);
            }
            continue;
        }

        if in_line_comment {
            if c == '\n' {
                in_line_comment = false;
                result.push('\n');
            }
            continue;
        }

        if in_block_comment {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block_comment = false;
                result.push(' ');
            }
            continue;
        }

        if (in_single_quote || in_double_quote) && c == '\\' {
            result.push(c);
            if let Some(next) = chars.next() {
                result.push(next);
            }
            continue;
        }

        if c == '\'' && !in_double_quote && !in_backtick {
            if in_single_quote && chars.peek() == Some(&'\'') {
                result.push(c);
                result.push(chars.next().unwrap());
                continue;
            }
            in_single_quote = !in_single_quote;
            result.push(c);
            continue;
        }

        if c == '"' && !in_single_quote && !in_backtick {
            in_double_quote = !in_double_quote;
            result.push(c);
            continue;
        }

        if c == '`' && !in_single_quote && !in_double_quote {
            in_backtick = !in_backtick;
            result.push(c);
            continue;
        }

        if !in_single_quote && !in_double_quote && !in_backtick {
            if c == '-' && is_line_comment_start(&chars) {
                chars.next();
                in_line_comment = true;
                continue;
            }
            if c == '#' {
                in_line_comment = true;
                continue;
            }
            if c == '/' && chars.peek() == Some(&'*') {
                chars.next();
                if keep_executable && chars.peek() == Some(&'!') {
                    // 可执行注释：跳过 `!` 与可选版本号，保留语句体
                    chars.next();
                    while chars.peek().is_some_and(|next| next.is_ascii_digit()) {
                        chars.next();
                    }
                    in_executable_comment = true;
                    result.push(' ');
                } else {
                    in_block_comment = true;
                }
                continue;
            }
        }

        result.push(c);
    }

    result
}

/// SQL 语句的执行路径粗分类
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlKind {
    /// 会话级语句：`USE` / `SET` / `RESET`，必须固定在一条物理连接上执行
    Session,
    /// 结构变更语句：`CREATE` / `ALTER` / `DROP` / `RENAME` / `TRUNCATE` 等
    Ddl,
    /// 数据变更语句：`INSERT` / `UPDATE` / `DELETE` / `REPLACE`，含 `WITH` 前缀
    Dml,
    /// 需要数据库上下文的元数据语句：`SHOW` / `DESC` / `DESCRIBE`
    NeedsDbContext,
    /// 其他语句（普通查询等）
    Query,
}

/// 对 SQL 做执行路径分类：先剥离注释取首关键字，`WITH` 前缀交给 sqlparser 判定。
///
/// 直接 `starts_with` 会被前缀注释（如 `/* c */ DELETE ...`）和 MySQL 可执行注释
/// （如 `/*!40101 SET NAMES utf8 */`）误判；`WITH ... INSERT/UPDATE/DELETE` 的首关键字
/// 也不是真实动作，需要用 sqlparser 检查查询体。多语句按第一条语句分类。
pub fn classify_sql(sql: &str) -> SqlKind {
    let cleaned = strip_comments_keep_executable(sql);
    let keyword = first_keyword(&cleaned);
    if keyword == "WITH" {
        return classify_with_statement(sql);
    }
    classify_keyword(&keyword)
}

/// 按首个关键字分类（`WITH` 前缀不适用）
fn classify_keyword(keyword: &str) -> SqlKind {
    match keyword {
        "USE" | "SET" | "RESET" => SqlKind::Session,
        "SHOW" | "DESC" | "DESCRIBE" => SqlKind::NeedsDbContext,
        "INSERT" | "UPDATE" | "DELETE" | "REPLACE" => SqlKind::Dml,
        "CREATE" | "ALTER" | "DROP" | "RENAME" | "TRUNCATE" | "GRANT" | "REVOKE" => SqlKind::Ddl,
        _ => SqlKind::Query,
    }
}

/// `WITH` 前缀的真实动作在 CTE 之后：用 sqlparser 解析查询体判定
fn classify_with_statement(sql: &str) -> SqlKind {
    use sqlparser::ast::{SetExpr, Statement};
    use sqlparser::dialect::MySqlDialect;
    use sqlparser::parser::Parser;

    let Ok(statements) = Parser::parse_sql(&MySqlDialect {}, sql) else {
        // 解析失败时无法判断 CTE 之后的动作，按普通查询处理（安全校验另有 fail-closed 逻辑）
        return SqlKind::Query;
    };
    for statement in &statements {
        match statement {
            Statement::Query(query) => {
                if matches!(
                    query.body.as_ref(),
                    SetExpr::Insert(_) | SetExpr::Update(_) | SetExpr::Delete(_)
                ) {
                    return SqlKind::Dml;
                }
            }
            Statement::Insert(_) | Statement::Update(_) | Statement::Delete(_) => {
                return SqlKind::Dml;
            }
            _ => {}
        }
    }
    SqlKind::Query
}

/// 取文本中的首个关键字（大写）；调用前应已剥离注释
fn first_keyword(sql: &str) -> String {
    sql.trim_start()
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect::<String>()
        .to_uppercase()
}

/// 判断 SQL 是否会改变库/表结构（DDL）。
///
/// 用于执行成功后失效元数据缓存：对象树与表结构有 30 秒 TTL，
/// 建表/删表后若不失效，界面刷新会拿到旧快照。
pub fn is_schema_changing(sql: &str) -> bool {
    // 注释可能出现在 DDL 之前，直接 trim_start 会漏判；先剥离注释（保留可执行注释体）
    let upper = strip_comments_keep_executable(sql)
        .trim_start()
        .to_uppercase();
    const PREFIXES: &[&str] = &[
        "CREATE TABLE",
        "CREATE TEMPORARY TABLE",
        "CREATE VIEW",
        "CREATE OR REPLACE VIEW",
        "CREATE MATERIALIZED VIEW",
        "CREATE INDEX",
        "CREATE UNIQUE INDEX",
        "CREATE DATABASE",
        "CREATE SCHEMA",
        "CREATE TRIGGER",
        "CREATE FUNCTION",
        "CREATE PROCEDURE",
        "ALTER TABLE",
        "ALTER VIEW",
        "ALTER MATERIALIZED VIEW",
        "ALTER DATABASE",
        "ALTER SCHEMA",
        "DROP TABLE",
        "DROP VIEW",
        "DROP MATERIALIZED VIEW",
        "DROP INDEX",
        "DROP DATABASE",
        "DROP SCHEMA",
        "DROP TRIGGER",
        "DROP FUNCTION",
        "DROP PROCEDURE",
        "REFRESH MATERIALIZED VIEW",
        "COMMENT ON",
        "RENAME TABLE",
    ];
    PREFIXES.iter().any(|prefix| upper.starts_with(prefix))
}

/// 提取 SQL 文本开头连续的 `USE <db>` / `SET ...` 预置语句中最后切换到的库名。
///
/// 用于「同一次提交内先切库再查询」（如 `USE db2; SELECT * FROM t`）：
/// 未限定表名必须以最后切换到的库为准补前缀，否则会被补上侧栏选中的库，
/// 静默查询到错误的库。返回 `None` 表示文本开头没有 USE 语句。
pub fn leading_use_database(sql: &str) -> Option<String> {
    // 只在开头一段文本里查找，避免超长脚本全量扫描
    const MAX_SCAN_LEN: usize = 64 * 1024;
    let head: String = sql.chars().take(MAX_SCAN_LEN).collect();
    let cleaned = strip_sql_comments(&head);

    let mut database = None;
    for statement in split_sql_statements(&cleaned) {
        let trimmed = statement.trim();
        let upper = trimmed.to_uppercase();
        // SET 只是连接级预置（如 `SET NAMES utf8mb4`），不改变目标库，允许跳过
        if upper.starts_with("SET ") || upper.starts_with("SET\t") {
            continue;
        }
        if !upper.starts_with("USE ") && !upper.starts_with("USE\t") {
            break;
        }
        let raw = trimmed.get(3..).unwrap_or("").trim();
        let name = raw
            .strip_prefix('`')
            .and_then(|s| s.strip_suffix('`'))
            .or_else(|| raw.strip_prefix('"').and_then(|s| s.strip_suffix('"')))
            .unwrap_or(raw)
            .trim();
        if !name.is_empty() {
            database = Some(name.to_string());
        }
    }
    database
}

/// 解析 `DELIMITER <token>` 指令行，返回新的语句分隔符。
///
/// 该指令是 mysql 客户端命令，不会发给服务端；导入含存储过程/触发器的脚本时，
/// 必须按它切换分隔符，否则过程体内部的 `;` 会被当作语句结尾切碎。
fn parse_delimiter_directive(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let keyword = parts.next()?;
    if !keyword.eq_ignore_ascii_case("DELIMITER") {
        return None;
    }
    let token = parts.next()?.split_whitespace().next()?;
    Some(token.to_string())
}

/// 判断当前位置是否命中语句分隔符；命中时消费分隔符的剩余字符。
fn consume_delimiter(
    c: char,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    delimiter: &str,
) -> bool {
    let mut expected = delimiter.chars();
    if expected.next() != Some(c) {
        return false;
    }
    for want in expected {
        if chars.peek() != Some(&want) {
            return false;
        }
        chars.next();
    }
    true
}

/// 安全分割 SQL 语句，忽略字符串字面量、反引号标识符和注释中的分号，
/// 并支持 `DELIMITER` 指令切换分隔符（存储过程/触发器脚本）。
pub fn split_sql_statements(sql: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let mut delimiter = String::from(";");
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
            if c == '-' && is_line_comment_start(&chars) {
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

        if !in_single_quote && !in_double_quote && !in_backtick {
            // DELIMITER 指令只出现在语句开头，且不影响服务端语义
            if !c.is_whitespace() && current.trim().is_empty() {
                let mut line = String::with_capacity(128);
                line.push(c);
                for next in chars.clone().take(128) {
                    if next == '\n' {
                        break;
                    }
                    line.push(next);
                }
                if let Some(new_delimiter) = parse_delimiter_directive(&line) {
                    for next in chars.by_ref() {
                        if next == '\n' {
                            break;
                        }
                    }
                    delimiter = new_delimiter;
                    current.clear();
                    continue;
                }
            }

            // 处理语句分隔符（默认 `;`，可由 DELIMITER 切换为 `$$` 等）
            if consume_delimiter(c, &mut chars, &delimiter) {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    statements.push(trimmed.to_string());
                }
                current.clear();
                continue;
            }
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
    use super::{
        classify_sql, is_schema_changing, leading_use_database, rewrite_table_names,
        split_sql_statements, strip_sql_comments, SqlKind,
    };
    use crate::connection::driver::DriverKind;

    #[test]
    fn test_rewrite_simple_select() {
        let (sql, parsed) = rewrite_table_names("mydb", "SELECT * FROM users", DriverKind::MySql);
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
        let (sql, parsed) = rewrite_table_names("mydb", "TRUNCATE TABLE users", DriverKind::MySql);
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
        let (sql, parsed) =
            rewrite_table_names("mydb", "DROP TABLE IF EXISTS `users`", DriverKind::MySql);
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
    fn test_rewrite_with_plain_comments_keeps_semantics() {
        let (sql, parsed) = rewrite_table_names(
            "mydb",
            "-- 统计用户数\nSELECT COUNT(*) FROM users /* 注释 */ WHERE id > 0",
            DriverKind::MySql,
        );
        assert!(parsed);
        assert!(sql.contains("`mydb`.`users`"), "got: {sql}");
        assert!(sql.contains("WHERE id > 0"), "got: {sql}");
    }

    #[test]
    fn test_rewrite_keeps_mysql_executable_comments() {
        let input = "/*!40101 SET NAMES utf8 */";
        let (sql, _) = rewrite_table_names("mydb", input, DriverKind::MySql);
        assert_eq!(sql, input);
    }

    #[test]
    fn test_strip_comments_ignores_quoted_content() {
        let stripped = strip_sql_comments("SELECT '-- not comment', `a#b` FROM t -- real");
        assert!(stripped.contains("'-- not comment'"), "got: {stripped}");
        assert!(stripped.contains("`a#b`"), "got: {stripped}");
        assert!(!stripped.contains("real"), "got: {stripped}");
    }

    #[test]
    fn test_double_dash_requires_whitespace() {
        // `5--3` 是两个减号（5 - (-3)），不能被当成注释截断
        assert_eq!(strip_sql_comments("SELECT 5--3"), "SELECT 5--3");
        // 行尾的 `--` 也不算注释，避免被截断成看似合法的语句
        assert_eq!(strip_sql_comments("SELECT 5--"), "SELECT 5--");
        // 后随空白或换行才是注释
        assert_eq!(strip_sql_comments("SELECT 1 -- note"), "SELECT 1 ");
        assert_eq!(
            strip_sql_comments("SELECT 1 --\nFROM t"),
            "SELECT 1 \nFROM t"
        );
        // `--` 后跟制表符同样算注释
        assert_eq!(strip_sql_comments("SELECT 1 --\tnote"), "SELECT 1 ");
    }

    #[test]
    fn test_split_statements_keeps_double_dash_expression() {
        let statements = split_sql_statements("SELECT 5--3; SELECT 1 -- 注释\nFROM t;");
        assert_eq!(statements.len(), 2, "got: {statements:?}");
        assert_eq!(statements[0], "SELECT 5--3");
        assert!(statements[1].contains("FROM t"), "got: {statements:?}");
    }

    #[test]
    fn test_split_statements_respects_delimiter() {
        let sql = "DELIMITER $$\n\
                   CREATE PROCEDURE p()\n\
                   BEGIN\n  SELECT 1;\n  SELECT 2;\nEND$$\n\
                   DELIMITER ;\n\
                   SELECT 3;\n";
        let statements = split_sql_statements(sql);
        assert_eq!(statements.len(), 2, "got: {statements:?}");
        assert!(
            statements[0].starts_with("CREATE PROCEDURE p()"),
            "got: {statements:?}"
        );
        assert!(statements[0].contains("SELECT 2;"), "got: {statements:?}");
        assert!(!statements[0].contains("DELIMITER"), "got: {statements:?}");
        assert_eq!(statements[1], "SELECT 3");
    }

    #[test]
    fn test_split_statements_respects_custom_delimiter() {
        let sql = "delimiter //\n\
                   CREATE TRIGGER tr BEFORE INSERT ON t FOR EACH ROW\n\
                   BEGIN\n  SET NEW.a = 1;\nEND//\n\
                   DELIMITER ;\n\
                   INSERT INTO t VALUES (1);";
        let statements = split_sql_statements(sql);
        assert_eq!(statements.len(), 2, "got: {statements:?}");
        assert!(
            statements[0].starts_with("CREATE TRIGGER"),
            "got: {statements:?}"
        );
        assert!(
            statements[0].contains("SET NEW.a = 1;"),
            "got: {statements:?}"
        );
        assert_eq!(statements[1], "INSERT INTO t VALUES (1)");
    }

    #[test]
    fn test_is_schema_changing() {
        assert!(is_schema_changing("CREATE TABLE `t` (id INT)"));
        assert!(is_schema_changing("  drop table `t`"));
        assert!(is_schema_changing("ALTER TABLE `t` ADD COLUMN c INT"));
        assert!(is_schema_changing("CREATE OR REPLACE VIEW v AS SELECT 1"));
        assert!(is_schema_changing("RENAME TABLE a TO b"));
        // 前缀注释（普通注释/可执行注释）不影响判断
        assert!(is_schema_changing("-- 建表\nCREATE TABLE `t` (id INT)"));
        assert!(is_schema_changing("/* c */ DROP TABLE `t`"));
        assert!(is_schema_changing("/*!50000 CREATE TABLE `t` (id INT)*/"));
        assert!(!is_schema_changing("SELECT * FROM t"));
        assert!(!is_schema_changing("INSERT INTO t VALUES (1)"));
        assert!(!is_schema_changing("TRUNCATE TABLE t"));
        // PostgreSQL 专有 DDL 同样要失效元数据缓存（对象树/表结构 30s TTL）
        assert!(is_schema_changing(
            "CREATE MATERIALIZED VIEW mv AS SELECT 1"
        ));
        assert!(is_schema_changing(
            "DROP MATERIALIZED VIEW IF EXISTS mv"
        ));
        assert!(is_schema_changing("REFRESH MATERIALIZED VIEW mv"));
        assert!(is_schema_changing("COMMENT ON TABLE t IS 'x'"));
        assert!(!is_schema_changing("/*!40101 SET NAMES utf8 */"));
    }

    #[test]
    fn test_classify_sql() {
        // 注释前缀不能影响首关键字判定
        assert_eq!(
            classify_sql("/* c */ CREATE TABLE t (id INT)"),
            SqlKind::Ddl
        );
        assert_eq!(
            classify_sql("-- 注释\nDELETE FROM t WHERE id = 1"),
            SqlKind::Dml
        );
        // WITH 前缀看查询体真实动作
        assert_eq!(
            classify_sql("WITH x AS (SELECT 1) INSERT INTO t SELECT * FROM x"),
            SqlKind::Dml
        );
        assert_eq!(
            classify_sql("WITH x AS (SELECT 1) SELECT * FROM x"),
            SqlKind::Query
        );
        // 元数据语句需要数据库上下文
        assert_eq!(classify_sql("SHOW TABLES"), SqlKind::NeedsDbContext);
        assert_eq!(classify_sql("DESC t"), SqlKind::NeedsDbContext);
        // MySQL 可执行注释按真实语句分类
        assert_eq!(classify_sql("/*!40101 SET NAMES utf8*/"), SqlKind::Session);
        assert_eq!(classify_sql("/*!50000 DROP TABLE t*/"), SqlKind::Ddl);
        // 常见语句分类
        assert_eq!(classify_sql("USE demo"), SqlKind::Session);
        assert_eq!(classify_sql("REPLACE INTO t VALUES (1)"), SqlKind::Dml);
        assert_eq!(classify_sql("TRUNCATE TABLE t"), SqlKind::Ddl);
        assert_eq!(classify_sql("GRANT SELECT ON *.* TO u"), SqlKind::Ddl);
        assert_eq!(classify_sql("SELECT * FROM t"), SqlKind::Query);
    }

    #[test]
    fn test_leading_use_database() {
        assert_eq!(
            leading_use_database("USE db2; SELECT * FROM t").as_deref(),
            Some("db2")
        );
        assert_eq!(
            leading_use_database("use `db-2`;\nselect 1").as_deref(),
            Some("db-2")
        );
        // 多条 USE 以最后一条为准
        assert_eq!(
            leading_use_database("USE a; USE b; SELECT 1").as_deref(),
            Some("b")
        );
        // 前置 SET 预置语句可跳过
        assert_eq!(
            leading_use_database("SET NAMES utf8mb4; USE db3; SELECT 1").as_deref(),
            Some("db3")
        );
        // 注释与可执行注释不影响识别
        assert_eq!(
            leading_use_database("-- 切库\n/*!40101 SET NAMES utf8 */ USE db4; SELECT 1")
                .as_deref(),
            Some("db4")
        );
        // 没有前置 USE，或 USE 出现在普通语句之后，都不改变目标库
        assert_eq!(leading_use_database("SELECT * FROM t"), None);
        assert_eq!(
            leading_use_database("SELECT 1; USE db2; SELECT * FROM t"),
            None
        );
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
