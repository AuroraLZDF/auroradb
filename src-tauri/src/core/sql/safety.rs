//! SQL 执行安全守卫：按连接访问模式拦截危险语句。
//!
//! - `ReadOnly`：只放行查询类语句（fail-closed：无法识别的语句一律拒绝）；
//! - `Safe`：拦截不带 WHERE 的 UPDATE/DELETE。
//!
//! 优先用 sqlparser 解析；遇到 MySQL 特有语法解析失败时，退化为首关键字判断。

use anyhow::{anyhow, Result};
use sqlparser::ast::{Query, SetExpr, Statement};
use sqlparser::dialect::MySqlDialect;
use sqlparser::parser::Parser;

use crate::core::connection::types::SafetyMode;

pub const READ_ONLY_MESSAGE: &str = "当前连接为只读模式，已阻止执行非查询语句";
pub const SAFE_MODE_MESSAGE: &str = "当前连接已开启安全模式，已阻止执行不带 WHERE 的 UPDATE/DELETE";

/// 只读模式下解析失败时的兜底放行关键字
///
/// 不含 `WITH`：CTE 可以前缀 DELETE/UPDATE，兜底模式下无法判定其后真实动作，按 fail-closed 拒绝。
const READ_ONLY_KEYWORDS: &[&str] = &[
    "SELECT", "SHOW", "DESC", "DESCRIBE", "EXPLAIN", "USE", "SET", "TABLE", "VALUES", "BEGIN",
    "START", "COMMIT", "ROLLBACK", "SAVEPOINT", "RELEASE",
];

/// 校验 SQL 是否允许在当前访问模式下执行。
pub fn enforce(mode: SafetyMode, sql: &str) -> Result<()> {
    if mode == SafetyMode::Off {
        return Ok(());
    }

    match Parser::parse_sql(&MySqlDialect {}, sql) {
        Ok(statements) => check_statements(mode, &statements),
        Err(_) => check_by_keyword(mode, sql),
    }
}

fn check_statements(mode: SafetyMode, statements: &[Statement]) -> Result<()> {
    for statement in statements {
        match mode {
            SafetyMode::ReadOnly => {
                if !is_read_only(statement) {
                    return Err(anyhow!(READ_ONLY_MESSAGE));
                }
            }
            SafetyMode::Safe => {
                if is_unconditional_write(statement) {
                    return Err(anyhow!(SAFE_MODE_MESSAGE));
                }
            }
            SafetyMode::Off => {}
        }
    }
    Ok(())
}

/// 只读模式允许的语句：查询、元数据查询、会话/事务控制
fn is_read_only(statement: &Statement) -> bool {
    match statement {
        // sqlparser 会把 `WITH ... DELETE/UPDATE` 解析成 Query { body: Delete/Update }，须递归检查查询体
        Statement::Query(query) => is_read_only_query(query),
        Statement::Explain { .. }
        | Statement::ExplainTable { .. }
        | Statement::ShowFunctions { .. }
        | Statement::ShowVariable { .. }
        | Statement::ShowStatus { .. }
        | Statement::ShowVariables { .. }
        | Statement::ShowCreate { .. }
        | Statement::ShowColumns { .. }
        | Statement::ShowCatalogs { .. }
        | Statement::ShowDatabases { .. }
        | Statement::ShowProcessList { .. }
        | Statement::ShowSchemas { .. }
        | Statement::ShowCharset(_)
        | Statement::ShowObjects(_)
        | Statement::ShowTables { .. }
        | Statement::ShowViews { .. }
        | Statement::ShowCollation { .. }
        | Statement::Use(_)
        | Statement::Set(_)
        | Statement::StartTransaction { .. }
        | Statement::Commit { .. }
        | Statement::Rollback { .. }
        | Statement::Savepoint { .. }
        | Statement::ReleaseSavepoint { .. } => true,
        _ => false,
    }
}

fn is_read_only_query(query: &Query) -> bool {
    is_read_only_set_expr(&query.body)
}

fn is_read_only_set_expr(expr: &SetExpr) -> bool {
    match expr {
        SetExpr::Select(_) | SetExpr::Values(_) | SetExpr::Table(_) => true,
        SetExpr::Query(query) => is_read_only_query(query),
        SetExpr::SetOperation { left, right, .. } => {
            is_read_only_set_expr(left) && is_read_only_set_expr(right)
        }
        SetExpr::Insert(_) | SetExpr::Update(_) | SetExpr::Delete(_) | SetExpr::Merge(_) => false,
    }
}

/// 安全模式下需拦截的语句：不带 WHERE 的 UPDATE/DELETE
fn is_unconditional_write(statement: &Statement) -> bool {
    match statement {
        Statement::Update(update) => update.selection.is_none(),
        Statement::Delete(delete) => delete.selection.is_none(),
        Statement::Query(query) => set_expr_has_unconditional_write(&query.body),
        _ => false,
    }
}

fn set_expr_has_unconditional_write(expr: &SetExpr) -> bool {
    match expr {
        SetExpr::Update(statement) => {
            matches!(statement, Statement::Update(update) if update.selection.is_none())
        }
        SetExpr::Delete(statement) => {
            matches!(statement, Statement::Delete(delete) if delete.selection.is_none())
        }
        SetExpr::Query(query) => set_expr_has_unconditional_write(&query.body),
        SetExpr::SetOperation { left, right, .. } => {
            set_expr_has_unconditional_write(left) || set_expr_has_unconditional_write(right)
        }
        _ => false,
    }
}

/// 解析失败时的兜底判断：逐条按首关键字与 WHERE 关键字判断
fn check_by_keyword(mode: SafetyMode, sql: &str) -> Result<()> {
    for statement in crate::core::sql::split_sql_statements(sql) {
        let statement = statement.trim();
        if statement.is_empty() {
            continue;
        }

        let keyword = leading_keyword(statement);
        match mode {
            SafetyMode::ReadOnly => {
                if !READ_ONLY_KEYWORDS.contains(&keyword.as_str()) {
                    return Err(anyhow!(READ_ONLY_MESSAGE));
                }
            }
            SafetyMode::Safe => {
                let is_write = keyword == "UPDATE" || keyword == "DELETE";
                if is_write && !contains_keyword(statement, "WHERE") {
                    return Err(anyhow!(SAFE_MODE_MESSAGE));
                }
                // WITH 开头时无法在兜底模式下判定 CTE 之后的真实动作，含写操作即拒绝
                if keyword == "WITH"
                    && (contains_keyword(statement, "UPDATE")
                        || contains_keyword(statement, "DELETE"))
                {
                    return Err(anyhow!(SAFE_MODE_MESSAGE));
                }
            }
            SafetyMode::Off => {}
        }
    }
    Ok(())
}

/// 取语句的首个关键字（跳过空白与注释），返回大写形式
fn leading_keyword(sql: &str) -> String {
    let mut word = String::new();
    let mut chars = sql.chars().peekable();

    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            if word.is_empty() {
                continue;
            }
            break;
        }
        if c == '-' && chars.peek() == Some(&'-') {
            chars.next();
            for c in chars.by_ref() {
                if c == '\n' {
                    break;
                }
            }
        } else if c == '#' {
            for c in chars.by_ref() {
                if c == '\n' {
                    break;
                }
            }
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut prev = '\0';
            for c in chars.by_ref() {
                if prev == '*' && c == '/' {
                    break;
                }
                prev = c;
            }
        } else if c.is_alphanumeric() || c == '_' {
            word.push(c);
        } else if !word.is_empty() {
            break;
        }
    }

    word.to_uppercase()
}

/// 判断语句中是否存在指定关键字（忽略字符串字面量、反引号标识符和注释中的内容）
fn contains_keyword(sql: &str, keyword: &str) -> bool {
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut chars = sql.chars().peekable();

    while let Some(c) = chars.next() {
        if let Some(q) = quote {
            if c == '\\' {
                chars.next();
            } else if c == q {
                if chars.peek() == Some(&q) {
                    chars.next();
                } else {
                    quote = None;
                }
            }
            continue;
        }

        match c {
            '\'' | '"' | '`' => {
                if word.eq_ignore_ascii_case(keyword) {
                    return true;
                }
                word.clear();
                quote = Some(c);
            }
            '-' if chars.peek() == Some(&'-') => {
                if word.eq_ignore_ascii_case(keyword) {
                    return true;
                }
                word.clear();
                chars.next();
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '#' => {
                if word.eq_ignore_ascii_case(keyword) {
                    return true;
                }
                word.clear();
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                if word.eq_ignore_ascii_case(keyword) {
                    return true;
                }
                word.clear();
                chars.next();
                let mut prev = '\0';
                for c in chars.by_ref() {
                    if prev == '*' && c == '/' {
                        break;
                    }
                    prev = c;
                }
            }
            c if c.is_alphanumeric() || c == '_' || c == '$' => word.push(c),
            _ => {
                if word.eq_ignore_ascii_case(keyword) {
                    return true;
                }
                word.clear();
            }
        }
    }

    word.eq_ignore_ascii_case(keyword)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_allows_everything() {
        assert!(enforce(SafetyMode::Off, "DELETE FROM t").is_ok());
        assert!(enforce(SafetyMode::Off, "DROP TABLE t").is_ok());
    }

    #[test]
    fn read_only_allows_queries() {
        for sql in [
            "SELECT * FROM t",
            "SELECT 1; SELECT 2",
            "SHOW TABLES",
            "SHOW VARIABLES LIKE 'x'",
            "DESC t",
            "EXPLAIN SELECT * FROM t",
            "USE demo",
            "SET NAMES utf8mb4",
            "BEGIN",
            "COMMIT",
            "WITH x AS (SELECT 1) SELECT * FROM x",
        ] {
            assert!(enforce(SafetyMode::ReadOnly, sql).is_ok(), "should allow: {sql}");
        }
    }

    #[test]
    fn read_only_blocks_writes() {
        for sql in [
            "INSERT INTO t VALUES (1)",
            "UPDATE t SET a = 1 WHERE id = 1",
            "DELETE FROM t WHERE id = 1",
            "DROP TABLE t",
            "ALTER TABLE t ADD COLUMN c INT",
            "TRUNCATE TABLE t",
            "CALL p()",
            "CREATE TABLE t (id INT)",
        ] {
            let err = enforce(SafetyMode::ReadOnly, sql).unwrap_err().to_string();
            assert_eq!(err, READ_ONLY_MESSAGE, "should block: {sql}");
        }
    }

    #[test]
    fn read_only_blocks_any_unsafe_statement_in_batch() {
        assert!(enforce(SafetyMode::ReadOnly, "SELECT 1; DELETE FROM t").is_err());
    }

    #[test]
    fn read_only_blocks_replace() {
        assert!(enforce(SafetyMode::ReadOnly, "REPLACE INTO t VALUES (1)").is_err());
    }

    #[test]
    fn safe_blocks_write_without_where() {
        for sql in ["UPDATE t SET a = 1", "DELETE FROM t"] {
            let err = enforce(SafetyMode::Safe, sql).unwrap_err().to_string();
            assert_eq!(err, SAFE_MODE_MESSAGE, "should block: {sql}");
        }
    }

    #[test]
    fn safe_blocks_delete_without_where_in_batch() {
        let err = enforce(SafetyMode::Safe, "UPDATE t SET a = 1 WHERE id = 1; DELETE FROM t")
            .unwrap_err()
            .to_string();
        assert_eq!(err, SAFE_MODE_MESSAGE);
    }

    #[test]
    fn safe_allows_write_with_where() {
        for sql in [
            "UPDATE t SET a = 1 WHERE id = 1",
            "DELETE FROM t WHERE id = 1",
            "INSERT INTO t VALUES (1)",
            "SELECT * FROM t",
        ] {
            assert!(enforce(SafetyMode::Safe, sql).is_ok(), "should allow: {sql}");
        }
    }

    #[test]
    fn safe_keyword_fallback_ignores_where_in_literals() {
        // 值里出现 where 不能当成 WHERE 子句
        assert!(enforce(SafetyMode::Safe, "UPDATE t SET a = ' where '").is_err());
    }

    #[test]
    fn cte_prefixed_write_is_blocked() {
        // MySQL 支持 WITH 前缀 DELETE/UPDATE，sqlparser 会解析为 Query { body: Delete/Update }
        for sql in [
            "WITH c AS (SELECT 1) DELETE FROM t",
            "WITH c AS (SELECT 1) UPDATE t SET a = 1",
        ] {
            assert!(enforce(SafetyMode::ReadOnly, sql).is_err(), "read-only should block: {sql}");
            assert!(enforce(SafetyMode::Safe, sql).is_err(), "safe should block: {sql}");
        }
        // 带 WHERE 的 CTE 写操作在安全模式下放行，只读模式仍拦截
        let sql = "WITH c AS (SELECT 1) DELETE FROM t WHERE id = 1";
        assert!(enforce(SafetyMode::Safe, sql).is_ok());
        assert!(enforce(SafetyMode::ReadOnly, sql).is_err());
        // 纯 CTE 查询放行
        let sql = "WITH c AS (SELECT 1) SELECT * FROM c";
        assert!(enforce(SafetyMode::ReadOnly, sql).is_ok());
        assert!(enforce(SafetyMode::Safe, sql).is_ok());
    }

    #[test]
    fn leading_keyword_skips_comments() {
        assert_eq!(leading_keyword("  -- note\n DELETE FROM t"), "DELETE");
        assert_eq!(leading_keyword("/* c */ update t set a = 1"), "UPDATE");
    }
}
