//! 结果集处理模块
//! 列类型分类、单元格值转换与 SQL 字面量格式化等纯函数。

use super::driver::DriverKind;
use super::types::*;
use super::validation::{escape_string, escape_string_pg};
use anyhow::{anyhow, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use sqlx::{Column, Row, ValueRef};

/// 列值类型分类。在列级别一次性计算，避免对每个单元格重复做字符串匹配和小写化。
#[derive(Clone, Copy)]
pub enum ColumnKind {
    BitBool,
    BitBytes,
    Boolean,
    UnsignedInt,
    SignedInt,
    Float,
    Decimal,
    Double,
    Timestamp,
    DateTime,
    Date,
    Time,
    Year,
    Json,
    Binary,
    Text,
}

/// 根据列类型字符串分类，供后续逐单元格读取使用。
pub fn classify_column(data_type: &str) -> ColumnKind {
    let lower = data_type.to_lowercase();
    if lower.starts_with("bit") {
        if lower == "bit(1)" || lower == "bit" {
            ColumnKind::BitBool
        } else {
            ColumnKind::BitBytes
        }
    } else if lower == "boolean" || lower == "bool" {
        ColumnKind::Boolean
    } else if (lower.contains("int")
        || lower.contains("tiny")
        || lower.contains("small")
        || lower.contains("medium")
        || lower.contains("bigint"))
        && !lower.contains("bit")
    {
        if lower.contains("unsigned") {
            ColumnKind::UnsignedInt
        } else {
            ColumnKind::SignedInt
        }
    } else if lower.contains("float") && !lower.contains("double") {
        ColumnKind::Float
    } else if lower.contains("decimal") || lower.contains("numeric") {
        ColumnKind::Decimal
    } else if lower.contains("double") || lower.contains("real") {
        ColumnKind::Double
    } else if lower.contains("timestamp") {
        ColumnKind::Timestamp
    } else if lower.contains("datetime") {
        ColumnKind::DateTime
    } else if lower.contains("date") {
        ColumnKind::Date
    } else if lower.contains("time") {
        ColumnKind::Time
    } else if lower.contains("year") {
        ColumnKind::Year
    } else if lower.contains("json") {
        ColumnKind::Json
    } else if lower.contains("binary") || lower.contains("blob") || lower.contains("varbinary") {
        ColumnKind::Binary
    } else {
        ColumnKind::Text
    }
}

/// 转换值（根据预分类的列类型读取）
///
/// 规则：NULL 值返回 `CellValue::Null`；驱动类型映射差异导致的读取失败
/// 按预置降级路径重试（如 BIT(1) 按布尔读失败则按字节读）；
/// 所有降级路径都失败时返回带列名/类型的错误，禁止静默吞错返回假 NULL。
pub fn convert_value(
    row: &sqlx::mysql::MySqlRow,
    index: usize,
    kind: ColumnKind,
) -> Result<CellValue> {
    let (col_name, col_type) = row
        .columns()
        .get(index)
        .map(|c| (c.name().to_string(), c.type_info().to_string()))
        .unwrap_or_else(|| ("?".to_string(), String::new()));

    // 所有降级路径都失败后的统一报错，带列名与类型
    let fail = |last_err: &dyn std::fmt::Display| -> anyhow::Error {
        anyhow!(
            "读取列 '{}'（{}）的值失败: {}",
            col_name,
            col_type,
            last_err
        )
    };

    match kind {
        // BIT(1) 作为布尔值处理
        ColumnKind::BitBool => match row.try_get::<Option<bool>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::Boolean(v)),
            Ok(None) => Ok(CellValue::Null),
            Err(e0) => {
                tracing::debug!("BIT(1) 按布尔读取失败，尝试按字节读取: {}", e0);
                match row.try_get::<Option<Vec<u8>>, _>(index) {
                    Ok(Some(v)) => Ok(CellValue::Boolean(!v.is_empty() && v[0] != 0)),
                    Ok(None) => Ok(CellValue::Null),
                    Err(e1) => Err(fail(&e1)),
                }
            }
        },
        // BIT(n) n>1 使用 Vec<u8> 读取
        ColumnKind::BitBytes => match row.try_get::<Option<Vec<u8>>, _>(index) {
            Ok(Some(v)) => {
                let mut value: u64 = 0;
                for (i, &byte) in v.iter().enumerate() {
                    value |= (byte as u64) << ((v.len() - 1 - i) * 8);
                }
                Ok(CellValue::Integer(value as i64))
            }
            Ok(None) => Ok(CellValue::Null),
            Err(e) => Err(fail(&e)),
        },
        // 布尔类型（MySQL 中 TINYINT(1) 有时被映射为 BOOLEAN）
        ColumnKind::Boolean => match row.try_get::<Option<bool>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::Integer(if v { 1 } else { 0 })),
            Ok(None) => Ok(CellValue::Null),
            Err(e) => Err(fail(&e)),
        },
        // 无符号整数（如 BIGINT UNSIGNED），范围可能超过 i64，使用字符串存储避免溢出
        ColumnKind::UnsignedInt => match row.try_get::<Option<u64>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::String(v.to_string())),
            Ok(None) => Ok(CellValue::Null),
            Err(e0) => {
                tracing::debug!("u64 读取失败，尝试按字节读取: {}", e0);
                match row.try_get::<Option<Vec<u8>>, _>(index) {
                    Ok(Some(v)) => {
                        let mut value: u64 = 0;
                        for (i, &byte) in v.iter().enumerate() {
                            value |= (byte as u64) << ((v.len() - 1 - i) * 8);
                        }
                        Ok(CellValue::String(value.to_string()))
                    }
                    Ok(None) => Ok(CellValue::Null),
                    Err(e1) => Err(fail(&e1)),
                }
            }
        },
        // 有符号整数
        ColumnKind::SignedInt => match row.try_get::<Option<i64>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::Integer(v)),
            Ok(None) => Ok(CellValue::Null),
            Err(e) => Err(fail(&e)),
        },
        // FLOAT（4字节）用 f32 读取
        ColumnKind::Float => match row.try_get::<Option<f32>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::Float(v as f64)),
            Ok(None) => Ok(CellValue::Null),
            Err(e) => Err(fail(&e)),
        },
        // DECIMAL/NUMERIC 通过 rust_decimal 读取以保留精度
        ColumnKind::Decimal => match row.try_get::<Option<rust_decimal::Decimal>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::String(v.to_string())),
            Ok(None) => Ok(CellValue::Null),
            Err(e) => Err(fail(&e)),
        },
        // DOUBLE 等 8 字节浮点
        ColumnKind::Double => match row.try_get::<Option<f64>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::Float(v)),
            Ok(None) => Ok(CellValue::Null),
            Err(e) => Err(fail(&e)),
        },
        ColumnKind::Timestamp => {
            match row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(index) {
                Ok(Some(v)) => Ok(CellValue::DateTime(
                    v.format("%Y-%m-%d %H:%M:%S%.f").to_string(),
                )),
                Ok(None) => Ok(CellValue::Null),
                Err(e0) => {
                    tracing::debug!("Timestamp 按类型读取失败，尝试按字符串读取: {}", e0);
                    match row.try_get::<Option<String>, _>(index) {
                        Ok(Some(v)) => Ok(CellValue::DateTime(v)),
                        Ok(None) => Ok(CellValue::Null),
                        Err(e1) => Err(fail(&e1)),
                    }
                }
            }
        }
        ColumnKind::DateTime => match row.try_get::<Option<chrono::NaiveDateTime>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::DateTime(
                v.format("%Y-%m-%d %H:%M:%S%.f").to_string(),
            )),
            Ok(None) => Ok(CellValue::Null),
            Err(e0) => {
                tracing::debug!("DateTime 按类型读取失败，尝试按字符串读取: {}", e0);
                match row.try_get::<Option<String>, _>(index) {
                    Ok(Some(v)) => Ok(CellValue::DateTime(v)),
                    Ok(None) => Ok(CellValue::Null),
                    Err(e1) => Err(fail(&e1)),
                }
            }
        },
        ColumnKind::Date => match row.try_get::<Option<chrono::NaiveDate>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::DateTime(v.to_string())),
            Ok(None) => Ok(CellValue::Null),
            Err(e0) => {
                tracing::debug!("Date 按类型读取失败，尝试按字符串读取: {}", e0);
                match row.try_get::<Option<String>, _>(index) {
                    Ok(Some(v)) => Ok(CellValue::DateTime(v)),
                    Ok(None) => Ok(CellValue::Null),
                    Err(e1) => Err(fail(&e1)),
                }
            }
        },
        ColumnKind::Time => match row.try_get::<Option<chrono::NaiveTime>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::DateTime(v.to_string())),
            Ok(None) => Ok(CellValue::Null),
            Err(e0) => {
                tracing::debug!("Time 按类型读取失败，尝试按字符串读取: {}", e0);
                match row.try_get::<Option<String>, _>(index) {
                    Ok(Some(v)) => Ok(CellValue::DateTime(v)),
                    Ok(None) => Ok(CellValue::Null),
                    Err(e1) => Err(fail(&e1)),
                }
            }
        },
        ColumnKind::Year => match row.try_get::<Option<i32>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::Integer(v as i64)),
            Ok(None) => Ok(CellValue::Null),
            Err(e) => Err(fail(&e)),
        },
        ColumnKind::Json => match row.try_get::<Option<serde_json::Value>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::String(v.to_string())),
            Ok(None) => Ok(CellValue::Null),
            Err(e) => Err(fail(&e)),
        },
        // 二进制数据作为字符串处理（Base64 编码）
        ColumnKind::Binary => match row.try_get::<Option<Vec<u8>>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::Binary(STANDARD.encode(&v))),
            Ok(None) => Ok(CellValue::Null),
            Err(e) => Err(fail(&e)),
        },
        // 默认字符串（varchar/char/text/enum/set 等）
        ColumnKind::Text => match row.try_get::<Option<String>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::String(v)),
            Ok(None) => Ok(CellValue::Null),
            Err(e) => Err(fail(&e)),
        },
    }
}

/// PostgreSQL 列值类型分类
///
/// 与 MySQL 的 `ColumnKind` 分开维护：PG 的类型名（`INT4`/`FLOAT8`/`TIMESTAMPTZ`…）
/// 与 MySQL 的 DDL 类型串差异较大，合并会让两边的分支互相污染。
#[derive(Clone, Copy)]
pub enum PgColumnKind {
    Bool,
    Int2,
    Int4,
    Int8,
    Float4,
    Float8,
    Numeric,
    Timestamp,
    TimestampTz,
    Date,
    Time,
    Json,
    Bytea,
    Uuid,
    Text,
}

/// 根据 PostgreSQL 类型名分类。类型名取自 `PgTypeInfo::to_string()`（大写，如 `INT4`）。
pub fn classify_pg_column(data_type: &str) -> PgColumnKind {
    match data_type.to_uppercase().as_str() {
        "BOOL" | "BOOLEAN" => PgColumnKind::Bool,
        "INT2" | "SMALLINT" | "SMALLSERIAL" => PgColumnKind::Int2,
        "INT4" | "INT" | "INTEGER" | "SERIAL" => PgColumnKind::Int4,
        "INT8" | "BIGINT" | "BIGSERIAL" => PgColumnKind::Int8,
        "FLOAT4" | "REAL" => PgColumnKind::Float4,
        "FLOAT8" | "DOUBLE PRECISION" => PgColumnKind::Float8,
        "NUMERIC" | "DECIMAL" => PgColumnKind::Numeric,
        "TIMESTAMP" => PgColumnKind::Timestamp,
        "TIMESTAMPTZ" => PgColumnKind::TimestampTz,
        "DATE" => PgColumnKind::Date,
        "TIME" => PgColumnKind::Time,
        "JSON" | "JSONB" => PgColumnKind::Json,
        "BYTEA" => PgColumnKind::Bytea,
        "UUID" => PgColumnKind::Uuid,
        _ => PgColumnKind::Text,
    }
}

/// 转换 PostgreSQL 单元格值。
///
/// 规则与 MySQL 版本一致：NULL 返回 `CellValue::Null`；日期时间类解码失败时
/// 降级为按字符串读取；所有路径都失败时返回带列名/类型的错误，禁止静默返回假 NULL。
pub fn convert_pg_value(
    row: &sqlx::postgres::PgRow,
    index: usize,
    kind: PgColumnKind,
) -> Result<CellValue> {
    let (col_name, col_type) = row
        .columns()
        .get(index)
        .map(|c| (c.name().to_string(), c.type_info().to_string()))
        .unwrap_or_else(|| ("?".to_string(), String::new()));

    let fail = |last_err: &dyn std::fmt::Display| -> anyhow::Error {
        anyhow!(
            "读取列 '{}'（{}）的值失败: {}",
            col_name,
            col_type,
            last_err
        )
    };

    match kind {
        PgColumnKind::Bool => match row.try_get::<Option<bool>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::Integer(if v { 1 } else { 0 })),
            Ok(None) => Ok(CellValue::Null),
            Err(e) => Err(fail(&e)),
        },
        PgColumnKind::Int2 => match row.try_get::<Option<i16>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::Integer(v as i64)),
            Ok(None) => Ok(CellValue::Null),
            Err(e) => Err(fail(&e)),
        },
        PgColumnKind::Int4 => match row.try_get::<Option<i32>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::Integer(v as i64)),
            Ok(None) => Ok(CellValue::Null),
            Err(e) => Err(fail(&e)),
        },
        PgColumnKind::Int8 => match row.try_get::<Option<i64>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::Integer(v)),
            Ok(None) => Ok(CellValue::Null),
            Err(e) => Err(fail(&e)),
        },
        PgColumnKind::Float4 => match row.try_get::<Option<f32>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::Float(v as f64)),
            Ok(None) => Ok(CellValue::Null),
            Err(e) => Err(fail(&e)),
        },
        PgColumnKind::Float8 => match row.try_get::<Option<f64>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::Float(v)),
            Ok(None) => Ok(CellValue::Null),
            Err(e) => Err(fail(&e)),
        },
        // NUMERIC 通过 rust_decimal 读取以保留精度
        PgColumnKind::Numeric => match row.try_get::<Option<rust_decimal::Decimal>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::String(v.to_string())),
            Ok(None) => Ok(CellValue::Null),
            Err(e) => Err(fail(&e)),
        },
        // TIMESTAMP（无时区）
        PgColumnKind::Timestamp => match row.try_get::<Option<chrono::NaiveDateTime>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::DateTime(
                v.format("%Y-%m-%d %H:%M:%S%.f").to_string(),
            )),
            Ok(None) => Ok(CellValue::Null),
            Err(e0) => {
                tracing::debug!("Timestamp 按类型读取失败，尝试按字符串读取: {}", e0);
                match row.try_get::<Option<String>, _>(index) {
                    Ok(Some(v)) => Ok(CellValue::DateTime(v)),
                    Ok(None) => Ok(CellValue::Null),
                    Err(e1) => Err(fail(&e1)),
                }
            }
        },
        // TIMESTAMPTZ：按 UTC 展示，与 MySQL TIMESTAMP 保持一致
        PgColumnKind::TimestampTz => {
            match row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(index) {
                Ok(Some(v)) => Ok(CellValue::DateTime(
                    v.format("%Y-%m-%d %H:%M:%S%.f").to_string(),
                )),
                Ok(None) => Ok(CellValue::Null),
                Err(e0) => {
                    tracing::debug!("TimestampTz 按类型读取失败，尝试按字符串读取: {}", e0);
                    match row.try_get::<Option<String>, _>(index) {
                        Ok(Some(v)) => Ok(CellValue::DateTime(v)),
                        Ok(None) => Ok(CellValue::Null),
                        Err(e1) => Err(fail(&e1)),
                    }
                }
            }
        }
        PgColumnKind::Date => match row.try_get::<Option<chrono::NaiveDate>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::DateTime(v.to_string())),
            Ok(None) => Ok(CellValue::Null),
            Err(e0) => {
                tracing::debug!("Date 按类型读取失败，尝试按字符串读取: {}", e0);
                match row.try_get::<Option<String>, _>(index) {
                    Ok(Some(v)) => Ok(CellValue::DateTime(v)),
                    Ok(None) => Ok(CellValue::Null),
                    Err(e1) => Err(fail(&e1)),
                }
            }
        },
        PgColumnKind::Time => match row.try_get::<Option<chrono::NaiveTime>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::DateTime(v.to_string())),
            Ok(None) => Ok(CellValue::Null),
            Err(e0) => {
                tracing::debug!("Time 按类型读取失败，尝试按字符串读取: {}", e0);
                match row.try_get::<Option<String>, _>(index) {
                    Ok(Some(v)) => Ok(CellValue::DateTime(v)),
                    Ok(None) => Ok(CellValue::Null),
                    Err(e1) => Err(fail(&e1)),
                }
            }
        },
        PgColumnKind::Json => match row.try_get::<Option<serde_json::Value>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::String(v.to_string())),
            Ok(None) => Ok(CellValue::Null),
            Err(e) => Err(fail(&e)),
        },
        // BYTEA 以 Base64 字符串返回，与 MySQL BLOB 保持一致
        PgColumnKind::Bytea => match row.try_get::<Option<Vec<u8>>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::Binary(STANDARD.encode(&v))),
            Ok(None) => Ok(CellValue::Null),
            Err(e) => Err(fail(&e)),
        },
        PgColumnKind::Uuid => match row.try_get::<Option<sqlx::types::Uuid>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::String(v.to_string())),
            Ok(None) => Ok(CellValue::Null),
            Err(e) => Err(fail(&e)),
        },
        // 默认按字符串处理（text/varchar/char/name/enum 等）；
        // sqlx 没有对应解码器的自定义类型（vector 等）无法按 String 读取，退回原始字节，
        // 避免一行里的一列就让整个结果集读取失败
        PgColumnKind::Text => match row.try_get::<Option<String>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::String(v)),
            Ok(None) => Ok(CellValue::Null),
            Err(e) => match row.try_get_raw(index) {
                Ok(raw) if raw.is_null() => Ok(CellValue::Null),
                Ok(raw) => {
                    let bytes = raw.as_bytes().map_err(|err| fail(&err))?;
                    Ok(CellValue::String(match std::str::from_utf8(bytes) {
                        Ok(text) => text.to_string(),
                        Err(_) => format!(
                            "\\x{}",
                            bytes
                                .iter()
                                .map(|b| format!("{:02x}", b))
                                .collect::<String>()
                        ),
                    }))
                }
                Err(_) => Err(fail(&e)),
            },
        },
    }
}

/// SQLite 列值类型分类
///
/// SQLite 的列类型是「声明亲和性」而非强类型：同一个 INTEGER 列里可能存 TEXT/BLOB。
/// 因此这里只作为「格式提示」（如 DATETIME 提示按日期时间解析），实际读取按
/// 多类型回退链进行，保证亲和性与存储类型不一致时仍能读出原始值。
#[derive(Clone, Copy)]
pub enum SqliteColumnKind {
    Integer,
    Real,
    Numeric,
    Boolean,
    Datetime,
    Date,
    Time,
    Blob,
    Text,
}

/// 根据 SQLite 声明列类型分类。类型名取自 `SqliteTypeInfo::to_string()`
/// （声明类型原文或运行时类型码：NULL/INTEGER/REAL/TEXT/BLOB）。
pub fn classify_sqlite_column(data_type: &str) -> SqliteColumnKind {
    let upper = data_type.to_uppercase();
    if upper.contains("DATETIME") || upper.contains("TIMESTAMP") {
        SqliteColumnKind::Datetime
    } else if upper.contains("DATE") {
        SqliteColumnKind::Date
    } else if upper == "TIME" || upper.ends_with(" TIME") {
        SqliteColumnKind::Time
    } else if upper.contains("BOOL") {
        SqliteColumnKind::Boolean
    } else if upper.contains("INT") {
        SqliteColumnKind::Integer
    } else if upper.contains("REAL") || upper.contains("FLOA") || upper.contains("DOUB") {
        SqliteColumnKind::Real
    } else if upper.contains("NUMERIC") || upper.contains("DEC") {
        SqliteColumnKind::Numeric
    } else if upper.contains("BLOB") {
        SqliteColumnKind::Blob
    } else {
        // TEXT 与未声明类型（表达式列的 NULL/空）都走文本+回退链
        SqliteColumnKind::Text
    }
}

/// 转换 SQLite 单元格值。
///
/// 与 MySQL/PG 版本一致：NULL 返回 `CellValue::Null`；按 kind 尝试类型化读取，
/// 失败时沿「同语义类型 → 文本 → 原始字节」回退，禁止静默返回假 NULL。
pub fn convert_sqlite_value(
    row: &sqlx::sqlite::SqliteRow,
    index: usize,
    kind: SqliteColumnKind,
) -> Result<CellValue> {
    let (col_name, col_type) = row
        .columns()
        .get(index)
        .map(|c| (c.name().to_string(), c.type_info().to_string()))
        .unwrap_or_else(|| ("?".to_string(), String::new()));

    let fail = |last_err: &dyn std::fmt::Display| -> anyhow::Error {
        anyhow!(
            "读取列 '{}'（{}）的值失败: {}",
            col_name,
            col_type,
            last_err
        )
    };

    // 空值快速路径：任何存储类型都先看 NULL
    match row.try_get_raw(index) {
        Ok(raw) if raw.is_null() => return Ok(CellValue::Null),
        _ => {}
    }

    /// 读到原始字节时的统一出口：文本按 UTF-8，二进制转 Base64（与 BLOB 一致）
    fn bytes_to_value(bytes: Vec<u8>) -> CellValue {
        match std::str::from_utf8(&bytes) {
            Ok(text) => CellValue::String(text.to_string()),
            Err(_) => CellValue::Binary(STANDARD.encode(bytes)),
        }
    }

    match kind {
        SqliteColumnKind::Integer => {
            if let Ok(Some(v)) = row.try_get::<Option<i64>, _>(index) {
                return Ok(CellValue::Integer(v));
            }
            if let Ok(Some(v)) = row.try_get::<Option<f64>, _>(index) {
                return Ok(CellValue::Float(v));
            }
            match row.try_get::<Option<String>, _>(index) {
                Ok(Some(v)) => Ok(CellValue::String(v)),
                Ok(None) => Ok(CellValue::Null),
                Err(e) => match row.try_get::<Option<Vec<u8>>, _>(index) {
                    Ok(Some(bytes)) => Ok(bytes_to_value(bytes)),
                    Ok(None) => Ok(CellValue::Null),
                    Err(_) => Err(fail(&e)),
                },
            }
        }
        SqliteColumnKind::Real => {
            if let Ok(Some(v)) = row.try_get::<Option<f64>, _>(index) {
                return Ok(CellValue::Float(v));
            }
            if let Ok(Some(v)) = row.try_get::<Option<i64>, _>(index) {
                return Ok(CellValue::Integer(v));
            }
            match row.try_get::<Option<String>, _>(index) {
                Ok(Some(v)) => Ok(CellValue::String(v)),
                Ok(None) => Ok(CellValue::Null),
                Err(e) => match row.try_get::<Option<Vec<u8>>, _>(index) {
                    Ok(Some(bytes)) => Ok(bytes_to_value(bytes)),
                    Ok(None) => Ok(CellValue::Null),
                    Err(_) => Err(fail(&e)),
                },
            }
        }
        // NUMERIC 亲和性：存储可能是 INTEGER/REAL/TEXT，保留原文本避免精度丢失
        SqliteColumnKind::Numeric => {
            if let Ok(Some(v)) = row.try_get::<Option<i64>, _>(index) {
                return Ok(CellValue::Integer(v));
            }
            if let Ok(Some(v)) = row.try_get::<Option<f64>, _>(index) {
                return Ok(CellValue::Float(v));
            }
            match row.try_get::<Option<String>, _>(index) {
                Ok(Some(v)) => Ok(CellValue::String(v)),
                Ok(None) => Ok(CellValue::Null),
                Err(e) => match row.try_get::<Option<Vec<u8>>, _>(index) {
                    Ok(Some(bytes)) => Ok(bytes_to_value(bytes)),
                    Ok(None) => Ok(CellValue::Null),
                    Err(_) => Err(fail(&e)),
                },
            }
        }
        // SQLite 无独立布尔存储，BOOLEAN 列通常存 0/1
        SqliteColumnKind::Boolean => {
            if let Ok(Some(v)) = row.try_get::<Option<bool>, _>(index) {
                return Ok(CellValue::Integer(if v { 1 } else { 0 }));
            }
            match row.try_get::<Option<i64>, _>(index) {
                Ok(Some(v)) => Ok(CellValue::Integer(v)),
                Ok(None) => Ok(CellValue::Null),
                Err(e0) => match row.try_get::<Option<String>, _>(index) {
                    Ok(Some(v)) => Ok(CellValue::String(v)),
                    Ok(None) => Ok(CellValue::Null),
                    Err(_) => Err(fail(&e0)),
                },
            }
        }
        // DATETIME/TIMESTAMP：TEXT（SQLite 格式表）/INTEGER（unix 秒）/REAL（儒略日）
        // 都能被 chrono 解码；失败则原样返回文本
        SqliteColumnKind::Datetime => {
            match row.try_get::<Option<chrono::NaiveDateTime>, _>(index) {
                Ok(Some(v)) => Ok(CellValue::DateTime(
                    v.format("%Y-%m-%d %H:%M:%S%.f").to_string(),
                )),
                Ok(None) => Ok(CellValue::Null),
                Err(e0) => match row.try_get::<Option<String>, _>(index) {
                    Ok(Some(v)) => Ok(CellValue::DateTime(v)),
                    Ok(None) => Ok(CellValue::Null),
                    Err(_) => Err(fail(&e0)),
                },
            }
        }
        SqliteColumnKind::Date => match row.try_get::<Option<chrono::NaiveDate>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::DateTime(v.to_string())),
            Ok(None) => Ok(CellValue::Null),
            Err(e0) => match row.try_get::<Option<String>, _>(index) {
                Ok(Some(v)) => Ok(CellValue::DateTime(v)),
                Ok(None) => Ok(CellValue::Null),
                Err(_) => Err(fail(&e0)),
            },
        },
        SqliteColumnKind::Time => match row.try_get::<Option<chrono::NaiveTime>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::DateTime(v.to_string())),
            Ok(None) => Ok(CellValue::Null),
            Err(e0) => match row.try_get::<Option<String>, _>(index) {
                Ok(Some(v)) => Ok(CellValue::DateTime(v)),
                Ok(None) => Ok(CellValue::Null),
                Err(_) => Err(fail(&e0)),
            },
        },
        // BLOB 以 Base64 字符串返回，与 MySQL/PG 保持一致；TEXT 存储时读原文
        SqliteColumnKind::Blob => match row.try_get::<Option<Vec<u8>>, _>(index) {
            Ok(Some(bytes)) => Ok(bytes_to_value(bytes)),
            Ok(None) => Ok(CellValue::Null),
            Err(e0) => match row.try_get::<Option<String>, _>(index) {
                Ok(Some(v)) => Ok(CellValue::String(v)),
                Ok(None) => Ok(CellValue::Null),
                Err(_) => Err(fail(&e0)),
            },
        },
        // 默认（TEXT/未声明类型）：依次尝试 文本 → 整数 → 浮点 → 字节
        SqliteColumnKind::Text => match row.try_get::<Option<String>, _>(index) {
            Ok(Some(v)) => Ok(CellValue::String(v)),
            Ok(None) => Ok(CellValue::Null),
            Err(e0) => {
                if let Ok(Some(v)) = row.try_get::<Option<i64>, _>(index) {
                    return Ok(CellValue::Integer(v));
                }
                if let Ok(Some(v)) = row.try_get::<Option<f64>, _>(index) {
                    return Ok(CellValue::Float(v));
                }
                match row.try_get::<Option<Vec<u8>>, _>(index) {
                    Ok(Some(bytes)) => Ok(bytes_to_value(bytes)),
                    Ok(None) => Ok(CellValue::Null),
                    Err(_) => Err(fail(&e0)),
                }
            }
        },
    }
}

/// 引用值（按驱动选择字符串转义规则）
///
/// MySQL 与 PG 的字符串字面量规则不同：MySQL 用反斜杠转义，PG 标准字符串只翻倍单引号。
/// 之前 PG 过滤条件复用了 MySQL 的转义，含反斜杠的值会多出一个反斜杠、含 `\'` 的值会直接语法错误。
pub(super) fn quote_value(value: &serde_json::Value, driver: DriverKind) -> Result<String> {
    match value {
        serde_json::Value::String(s) => {
            // SQLite 标准字符串字面量同样只翻倍单引号（反斜杠不是转义符）
            let escaped = match driver {
                DriverKind::MySql => escape_string(s),
                DriverKind::Postgres | DriverKind::Sqlite => escape_string_pg(s),
            };
            Ok(format!("'{}'", escaped))
        }
        serde_json::Value::Number(n) => Ok(n.to_string()),
        serde_json::Value::Bool(b) => Ok(b.to_string()),
        serde_json::Value::Null => Ok("NULL".to_string()),
        _ => Err(anyhow!("不支持的值类型")),
    }
}

/// 将数据库行中的值格式化为 SQL 字面量（用于导出）
///
/// 解码失败时返回错误，而不是静默写成 `NULL`：导出 SQL 若把真实值写成 NULL，
/// 重新导入后会永久丢失数据。这里先判断原始值是否真为 NULL，再核对格式化结果。
pub(super) fn format_sql_value(
    row: &sqlx::mysql::MySqlRow,
    column_name: &str,
    data_type: &str,
) -> Result<String> {
    let raw = row
        .try_get_raw(column_name)
        .map_err(|e| anyhow!("读取列 `{}`（{}）失败：{}", column_name, data_type, e))?;
    let is_null = raw.is_null();

    let value = format_sql_value_legacy(row, column_name, data_type);
    if value == "NULL" && !is_null {
        return Err(anyhow!(
            "列 `{}`（{}）的值无法解码，已停止导出以避免写入错误的 NULL",
            column_name,
            data_type
        ));
    }
    Ok(value)
}

/// 旧版逐类型解码实现：失败时回退为 "NULL"，由上层 `format_sql_value` 结合原始值判定
fn format_sql_value_legacy(
    row: &sqlx::mysql::MySqlRow,
    column_name: &str,
    data_type: &str,
) -> String {
    let lower_type = data_type.to_lowercase();
    tracing::debug!(
        "format_sql_value: column='{}', type='{}'",
        column_name,
        lower_type
    );

    // BIT 类型必须在整数类型之前检查，避免 BIT(8) 被错误识别
    // BIT 类型在 sqlx 中显示为 "BIT" 或 "BIT(8)" 等
    if lower_type.starts_with("bit") {
        // BIT(1) 作为布尔值处理
        if lower_type == "bit(1)" || lower_type == "bit" {
            return match row.try_get::<Option<bool>, _>(column_name) {
                Ok(Some(v)) => {
                    if v {
                        "1".to_string()
                    } else {
                        "0".to_string()
                    }
                }
                Ok(None) => "NULL".to_string(),
                Err(e) => {
                    tracing::warn!(
                        "Failed to read BIT(1) column '{}' as bool, trying Vec<u8>: {}",
                        column_name,
                        e
                    );
                    match row.try_get::<Option<Vec<u8>>, _>(column_name) {
                        Ok(Some(v)) => {
                            let val = !v.is_empty() && v[0] != 0;
                            if val {
                                "1".to_string()
                            } else {
                                "0".to_string()
                            }
                        }
                        _ => "NULL".to_string(),
                    }
                }
            };
        }
        // BIT(n) n>1 使用 Vec<u8> 读取
        return match row.try_get::<Option<Vec<u8>>, _>(column_name) {
            Ok(Some(v)) => {
                let mut value: u64 = 0;
                for (i, &byte) in v.iter().enumerate() {
                    value |= (byte as u64) << ((v.len() - 1 - i) * 8);
                }
                value.to_string()
            }
            Ok(None) => "NULL".to_string(),
            Err(e) => {
                tracing::error!(
                    "Failed to read BIT column '{}' as Vec<u8>: {}",
                    column_name,
                    e
                );
                "NULL".to_string()
            }
        };
    }

    // 布尔类型（MySQL 中 TINYINT(1) 有时被映射为 BOOLEAN）
    if lower_type == "boolean" || lower_type == "bool" {
        return match row.try_get::<Option<bool>, _>(column_name) {
            Ok(Some(v)) => (v as i8).to_string(),
            Ok(None) => "NULL".to_string(),
            Err(e) => {
                tracing::warn!(
                    "Failed to read boolean column '{}' type '{}': {}",
                    column_name,
                    data_type,
                    e
                );
                "NULL".to_string()
            }
        };
    }

    // 整数类型（排除 BIT，避免字节长度不匹配）
    if (lower_type.contains("int")
        || lower_type.contains("tiny")
        || lower_type.contains("small")
        || lower_type.contains("medium")
        || lower_type.contains("bigint")
        || lower_type.contains("year"))
        && !lower_type.contains("bit")
    {
        // 根据类型选择合适的 Rust 类型
        // TINYINT(1) 可能被映射为 BOOLEAN
        if lower_type.contains("tinyint") {
            return match row.try_get::<Option<i8>, _>(column_name) {
                Ok(Some(v)) => v.to_string(),
                Ok(None) => "NULL".to_string(),
                Err(_) => {
                    // 尝试作为 bool 读取 (TINYINT(1) 有时会被映射为 BOOLEAN)
                    match row.try_get::<Option<bool>, _>(column_name) {
                        Ok(Some(v)) => (v as i8).to_string(),
                        Ok(None) => "NULL".to_string(),
                        Err(e) => {
                            tracing::warn!(
                                "Failed to read tinyint column '{}' type '{}': {}",
                                column_name,
                                data_type,
                                e
                            );
                            "NULL".to_string()
                        }
                    }
                }
            };
        }
        // SMALLINT
        if lower_type.contains("smallint") {
            return match row.try_get::<Option<i16>, _>(column_name) {
                Ok(Some(v)) => v.to_string(),
                Ok(None) => "NULL".to_string(),
                Err(e) => {
                    tracing::warn!(
                        "Failed to read smallint column '{}' type '{}': {}",
                        column_name,
                        data_type,
                        e
                    );
                    "NULL".to_string()
                }
            };
        }
        // INT / INTEGER - 尝试 i32，失败则尝试 u32 (for unsigned)
        if lower_type.contains("int") && !lower_type.contains("bigint") {
            return match row.try_get::<Option<i32>, _>(column_name) {
                Ok(Some(v)) => v.to_string(),
                Ok(None) => "NULL".to_string(),
                Err(_) => {
                    // 可能是 UNSIGNED，尝试 u32
                    match row.try_get::<Option<u32>, _>(column_name) {
                        Ok(Some(v)) => v.to_string(),
                        Ok(None) => "NULL".to_string(),
                        Err(e) => {
                            tracing::warn!(
                                "Failed to read int column '{}' type '{}': {}",
                                column_name,
                                data_type,
                                e
                            );
                            "NULL".to_string()
                        }
                    }
                }
            };
        }
        // BIGINT
        return match row.try_get::<Option<i64>, _>(column_name) {
            Ok(Some(v)) => v.to_string(),
            Ok(None) => "NULL".to_string(),
            Err(_) => {
                // 可能是 UNSIGNED，尝试 u64
                match row.try_get::<Option<u64>, _>(column_name) {
                    Ok(Some(v)) => v.to_string(),
                    Ok(None) => "NULL".to_string(),
                    Err(e) => {
                        tracing::warn!(
                            "Failed to read bigint column '{}' type '{}': {}",
                            column_name,
                            data_type,
                            e
                        );
                        "NULL".to_string()
                    }
                }
            }
        };
    }

    // 浮点/定点类型
    // FLOAT 是 4 字节，DOUBLE 是 8 字节
    // 注意：FLOAT 必须用 f32 读取，DOUBLE 用 f64 读取，否则 byteorder 会 panic
    if lower_type.contains("float") && !lower_type.contains("double") {
        // FLOAT 类型（4字节）
        return match row.try_get::<Option<f32>, _>(column_name) {
            Ok(Some(v)) => v.to_string(),
            Ok(None) => "NULL".to_string(),
            Err(_) => "NULL".to_string(),
        };
    }

    if lower_type.contains("decimal") || lower_type.contains("numeric") {
        // DECIMAL/NUMERIC 类型 - 尝试多种方式读取
        // 方式1: 作为字符串直接读取
        if let Ok(Some(v)) = row.try_get::<Option<&str>, _>(column_name) {
            return v.to_string();
        }
        // 方式2: 作为 String 读取
        if let Ok(Some(v)) = row.try_get::<Option<String>, _>(column_name) {
            return v;
        }
        // 方式3: 作为 f64 读取并格式化
        return match row.try_get::<Option<f64>, _>(column_name) {
            Ok(Some(v)) => format!("{:.10}", v)
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string(),
            Ok(None) => "NULL".to_string(),
            Err(_) => "NULL".to_string(),
        };
    }

    if lower_type.contains("double") || lower_type.contains("real") {
        // DOUBLE 等 8字节浮点类型
        return match row.try_get::<Option<f64>, _>(column_name) {
            Ok(Some(v)) => v.to_string(),
            Ok(None) => "NULL".to_string(),
            Err(_) => "NULL".to_string(),
        };
    }

    // 二进制类型
    if lower_type.contains("binary") || lower_type.contains("blob") {
        return match row.try_get::<Option<Vec<u8>>, _>(column_name) {
            Ok(Some(v)) => format!(
                "X'{}'",
                v.iter().map(|b| format!("{:02X}", b)).collect::<String>()
            ),
            Ok(None) => "NULL".to_string(),
            Err(_) => "NULL".to_string(),
        };
    }

    // 日期时间类型
    if lower_type.contains("timestamp") {
        // TIMESTAMP 类型需要用 DateTime<Utc> 读取
        return match row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(column_name) {
            Ok(Some(v)) => format!("'{}'", v.format("%Y-%m-%d %H:%M:%S%.f")),
            Ok(None) => "NULL".to_string(),
            Err(_) => {
                // 尝试作为字符串读取
                match row.try_get::<Option<String>, _>(column_name) {
                    Ok(Some(v)) => format!("'{}'", v),
                    Ok(None) => "NULL".to_string(),
                    Err(_) => "NULL".to_string(),
                }
            }
        };
    }
    if lower_type.contains("datetime") {
        // DATETIME 类型用 NaiveDateTime 读取
        return match row.try_get::<Option<chrono::NaiveDateTime>, _>(column_name) {
            Ok(Some(v)) => format!("'{}'", v.format("%Y-%m-%d %H:%M:%S%.f")),
            Ok(None) => "NULL".to_string(),
            Err(_) => {
                // 尝试作为字符串读取
                match row.try_get::<Option<String>, _>(column_name) {
                    Ok(Some(v)) => format!("'{}'", v),
                    Ok(None) => "NULL".to_string(),
                    Err(_) => "NULL".to_string(),
                }
            }
        };
    }
    if lower_type.contains("date") {
        return match row.try_get::<Option<chrono::NaiveDate>, _>(column_name) {
            Ok(Some(v)) => format!("'{}'", v.format("%Y-%m-%d")),
            Ok(None) => "NULL".to_string(),
            Err(_) => {
                // 尝试作为字符串读取
                match row.try_get::<Option<String>, _>(column_name) {
                    Ok(Some(v)) => format!("'{}'", v),
                    Ok(None) => "NULL".to_string(),
                    Err(e) => {
                        tracing::warn!(
                            "Failed to read date column '{}' type '{}': {}",
                            column_name,
                            data_type,
                            e
                        );
                        "NULL".to_string()
                    }
                }
            }
        };
    }
    if lower_type.contains("time") {
        return match row.try_get::<Option<chrono::NaiveTime>, _>(column_name) {
            Ok(Some(v)) => format!("'{}'", v.format("%H:%M:%S%.f")),
            Ok(None) => "NULL".to_string(),
            Err(_) => {
                // 尝试作为字符串读取
                match row.try_get::<Option<String>, _>(column_name) {
                    Ok(Some(v)) => format!("'{}'", v),
                    Ok(None) => "NULL".to_string(),
                    Err(e) => {
                        tracing::warn!(
                            "Failed to read time column '{}' type '{}': {}",
                            column_name,
                            data_type,
                            e
                        );
                        "NULL".to_string()
                    }
                }
            }
        };
    }

    // 默认按字符串处理（varchar, char, text, enum, set, json 等）
    match row.try_get::<Option<String>, _>(column_name) {
        Ok(Some(v)) => format!("'{}'", v.replace('\\', "\\\\").replace('\'', "''")),
        Ok(None) => "NULL".to_string(),
        Err(e) => {
            tracing::warn!(
                "Failed to read string column '{}' type '{}': {}",
                column_name,
                data_type,
                e
            );
            "NULL".to_string()
        }
    }
}

/// 从 COLUMN_TYPE 字符串解析数值精度与小数位，如 "decimal(10,2)" -> (Some(10), Some(2))。
pub(super) fn parse_column_type_precision(column_type: &str) -> (Option<u32>, Option<u32>) {
    let Some(open) = column_type.find('(') else {
        return (None, None);
    };
    let close = column_type.rfind(')').unwrap_or(column_type.len());
    if close <= open {
        return (None, None);
    }
    let inner = &column_type[open + 1..close];
    let mut parts = inner.split(',');
    let length = parts.next().and_then(|s| s.trim().parse::<u32>().ok());
    let decimals = parts.next().and_then(|s| s.trim().parse::<u32>().ok());
    (length, decimals)
}
