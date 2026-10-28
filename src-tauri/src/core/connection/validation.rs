//! SQL 输入验证模块
//! 用于防止 SQL 注入攻击

use anyhow::{anyhow, Result};

/// 验证数据库/表/列名是否可以安全地拼进 SQL
///
/// 标识符最终都会被反引号/双引号包裹，因此这里只拒绝无法被引用包裹或会破坏
/// 语句结构的字符（引号、反斜杠、分号、控制字符、空字节），不再维护 SQL 关键字
/// 黑名单——`order`、`group`、`user` 等是合法且常见的表名/列名，之前会被误拒。
pub fn validate_identifier(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!("标识符不能为空"));
    }

    // 检查长度（MySQL 限制为 64 字符，PG 为 63，取更宽松的 64 并允许 schema.table）
    if name.chars().count() > 128 {
        return Err(anyhow!("标识符长度不能超过 128 字符"));
    }

    // 检查是否包含无法安全引用的字符
    let dangerous_chars = [';', '\'', '"', '`', '\\', '\n', '\r', '\t', '\0'];
    if name.chars().any(|c| dangerous_chars.contains(&c)) {
        return Err(anyhow!("标识符包含非法字符"));
    }

    Ok(())
}

/// 转义 MySQL 字符串字面量
pub fn escape_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
        .replace('\0', "\\0")
}

/// 转义 PostgreSQL 字符串字面量
///
/// PG 默认 `standard_conforming_strings = on`：反斜杠不是转义符，只有单引号需要翻倍。
/// 若沿用 MySQL 的反斜杠转义，`'a\b'` 会变成 `'a\\b'`（字面量里多一个反斜杠），
/// 且在 `E''` 之外的字符串里反斜杠转义并不生效，反而会制造语法错误。
pub fn escape_string_pg(value: &str) -> String {
    value.replace('\0', "").replace('\'', "''")
}

/// 安全地包装标识符（使用反引号）
pub fn quote_identifier(name: &str) -> Result<String> {
    validate_identifier(name)?;
    Ok(format!("`{}`", name.replace('`', "``")))
}

/// 按 PostgreSQL 规则引用标识符（双引号包裹，内部双引号翻倍）
pub fn quote_pg_identifier(name: &str) -> Result<String> {
    validate_identifier(name)?;
    Ok(format!("\"{}\"", name.replace('"', "\"\"")))
}

/// 验证分页参数
pub fn validate_pagination(page: usize, page_size: usize) -> Result<()> {
    if page == 0 {
        return Err(anyhow!("页码必须从 1 开始"));
    }

    if page_size == 0 {
        return Err(anyhow!("每页大小不能为 0"));
    }

    // 限制最大页大小，防止内存溢出
    const MAX_PAGE_SIZE: usize = 10000;
    if page_size > MAX_PAGE_SIZE {
        return Err(anyhow!("每页大小不能超过 {}", MAX_PAGE_SIZE));
    }

    Ok(())
}

/// 验证数据值类型
pub fn validate_value(value: &serde_json::Value) -> Result<()> {
    match value {
        serde_json::Value::Null => Ok(()),
        serde_json::Value::Bool(_) => Ok(()),
        serde_json::Value::Number(n) => {
            // 检查数值是否在合理范围内
            if n.is_f64() {
                let f = n.as_f64().unwrap();
                if f.is_infinite() || f.is_nan() {
                    return Err(anyhow!("数值不能为无穷大或 NaN"));
                }
            }
            Ok(())
        }
        serde_json::Value::String(s) => {
            // 检查字符串长度
            const MAX_STRING_LENGTH: usize = 10_000_000; // 10MB
            if s.len() > MAX_STRING_LENGTH {
                return Err(anyhow!("字符串长度不能超过 10MB"));
            }
            // 检查是否包含空字节
            if s.contains('\0') {
                return Err(anyhow!("字符串不能包含空字节"));
            }
            Ok(())
        }
        serde_json::Value::Array(arr) => {
            const MAX_ARRAY_LENGTH: usize = 1000;
            if arr.len() > MAX_ARRAY_LENGTH {
                return Err(anyhow!("数组长度不能超过 {}", MAX_ARRAY_LENGTH));
            }
            for item in arr {
                validate_value(item)?;
            }
            Ok(())
        }
        serde_json::Value::Object(_) => Err(anyhow!("不支持对象类型的值")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_identifier() {
        assert!(validate_identifier("users").is_ok());
        assert!(validate_identifier("user_data").is_ok());
        assert!(validate_identifier("user123").is_ok());
        assert!(validate_identifier("").is_err());
        assert!(validate_identifier("users;drop").is_err());
        assert!(validate_identifier("`backtick`").is_err());
        // 关键字是合法标识符（引用后即可使用），不应被误拒
        assert!(validate_identifier("select").is_ok());
        assert!(validate_identifier("order").is_ok());
        assert!(validate_identifier("group").is_ok());
    }

    #[test]
    fn test_escape_string() {
        assert_eq!(escape_string("hello"), "hello");
        assert_eq!(escape_string("it's"), "it\\'s");
        assert_eq!(escape_string("a\\b"), "a\\\\b");
    }

    #[test]
    fn test_escape_string_pg() {
        assert_eq!(escape_string_pg("hello"), "hello");
        assert_eq!(escape_string_pg("it's"), "it''s");
        // PG 标准字符串里反斜杠不是转义符，保持原样
        assert_eq!(escape_string_pg("a\\b"), "a\\b");
    }
}
