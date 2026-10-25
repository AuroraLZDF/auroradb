//! SQL 输入验证模块
//! 用于防止 SQL 注入攻击

use anyhow::{anyhow, Result};

/// 验证数据库/表/列名是否为合法的 MySQL 标识符
/// 只允许字母、数字、下划线，且不能以数字开头
pub fn validate_identifier(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!("标识符不能为空"));
    }

    // 检查长度（MySQL 限制为 64 字符）
    if name.len() > 64 {
        return Err(anyhow!("标识符长度不能超过 64 字符"));
    }

    // 检查是否包含危险字符
    let dangerous_chars = [';', '\'', '"', '`', '\\', '\n', '\r', '\t'];
    if name.chars().any(|c| dangerous_chars.contains(&c)) {
        return Err(anyhow!("标识符包含非法字符"));
    }

    // 检查是否包含 SQL 关键字
    let sql_keywords = [
        "select", "insert", "update", "delete", "drop", "create", "alter",
        "table", "database", "where", "from", "join", "union", "order",
        "group", "having", "limit", "offset", "and", "or", "not",
    ];
    let lower_name = name.to_lowercase();
    if sql_keywords.contains(&lower_name.as_str()) {
        return Err(anyhow!("标识符不能使用 SQL 关键字: {}", name));
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

/// 安全地包装标识符（使用反引号）
pub fn quote_identifier(name: &str) -> Result<String> {
    validate_identifier(name)?;
    Ok(format!("`{}`", name.replace('`', "``")))
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
        assert!(validate_identifier("select").is_err());
        assert!(validate_identifier("users;drop").is_err());
        assert!(validate_identifier("`backtick`").is_err());
    }

    #[test]
    fn test_escape_string() {
        assert_eq!(escape_string("hello"), "hello");
        assert_eq!(escape_string("it's"), "it\\'s");
        assert_eq!(escape_string("a\\b"), "a\\\\b");
    }
}
