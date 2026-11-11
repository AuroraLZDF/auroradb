//! 表数据导出格式化模块
//! 将类型化的行值序列化为 CSV / SQL / JSON，并提供与导出对称的 CSV 解析。
//!
//! 设计要点：
//! - 值来自 `convert_value`，保留数字/日期/NULL 的真实类型，不再把所有非字符串列写成 "NULL"。
//! - CSV 遵循 RFC 4180：逗号/双引号/换行触发加引号，引号内部 `"` 双写；行尾 CRLF。
//! - CSV 以 UTF-8 BOM 开头，避免 Excel 打开中文乱码。
//! - NULL 输出为空字段；空字符串输出为 `""`，使导入侧能区分两者。
//! - `TableExportSink` 逐行写盘，整表导出不再把全部数据堆在内存里。

use super::driver::DriverKind;
use super::resultset::{
    classify_column, classify_pg_column, classify_sqlite_column, convert_pg_value,
    convert_sqlite_value, convert_value, format_sql_value, ColumnKind, PgColumnKind,
    SqliteColumnKind,
};
use super::types::CellValue;
use super::validation::{escape_string_pg, quote_pg_identifier, quote_pg_table_reference};
use anyhow::{anyhow, Result};
use sqlx::postgres::PgRow;
use sqlx::{Column, Row, ValueRef};
use std::io::Write;

/// 导出列定义（名称 + INFORMATION_SCHEMA 中的 DATA_TYPE）
#[derive(Debug, Clone)]
pub struct ExportColumn {
    pub name: String,
    pub data_type: String,
}

/// 一次表导出的完整数据：有序的列定义与类型化行值
///
/// 仅用于单测构造「整表数据」后跑流式写出；生产路径已改为边读边写（`TableExportSink`）。
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct TableExportData {
    pub columns: Vec<ExportColumn>,
    pub rows: Vec<Vec<CellValue>>,
}

const UTF8_BOM: char = '\u{feff}';
const LINE_END: &str = "\r\n";

/// 支持的文本导出格式（Excel 由独立命令直接写工作簿）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Sql,
    Json,
}

impl ExportFormat {
    pub fn parse(format: &str) -> Result<Self> {
        match format.trim().to_lowercase().as_str() {
            "csv" => Ok(Self::Csv),
            "sql" => Ok(Self::Sql),
            "json" => Ok(Self::Json),
            other => Err(anyhow!("不支持的导出格式: {}", other)),
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Sql => "sql",
            Self::Json => "json",
        }
    }

    pub fn filter_name(self) -> &'static str {
        match self {
            Self::Csv => "CSV 文件",
            Self::Sql => "SQL 文件",
            Self::Json => "JSON 文件",
        }
    }
}

/// 流式导出行写出器
///
/// 逐行从结果集读取并写入 `W`，内存占用与表大小无关：
/// - CSV：首次写入时补 BOM 与表头，值经 `convert_value` 保留数字/日期/NULL 语义；
/// - JSON：逐行追加对象（保持列顺序），结束时补 `]`；
/// - SQL：每行一条 `INSERT INTO ... SET ...;`。
pub struct TableExportSink<W: Write> {
    writer: W,
    format: ExportFormat,
    table: String,
    driver: DriverKind,
    columns: Vec<ExportColumn>,
    kinds: Vec<ColumnKind>,
    pg_kinds: Vec<PgColumnKind>,
    sqlite_kinds: Vec<SqliteColumnKind>,
    rows: u64,
    started: bool,
}

impl<W: Write> TableExportSink<W> {
    /// `columns` 来自元数据查询（MySQL 的 COLUMN_TYPE / PG 的 typname）；
    /// 为空时由首行结果集的列信息兜底。
    pub fn new(
        writer: W,
        format: ExportFormat,
        table: &str,
        columns: Vec<ExportColumn>,
        driver: DriverKind,
    ) -> Self {
        let kinds = columns
            .iter()
            .map(|col| classify_column(&col.data_type))
            .collect();
        let pg_kinds = columns
            .iter()
            .map(|col| classify_pg_column(&col.data_type))
            .collect();
        let sqlite_kinds = columns
            .iter()
            .map(|col| classify_sqlite_column(&col.data_type))
            .collect();
        Self {
            writer,
            format,
            table: table.to_string(),
            driver,
            columns,
            kinds,
            pg_kinds,
            sqlite_kinds,
            rows: 0,
            started: false,
        }
    }

    /// 写出一行 MySQL 结果集
    pub fn write_row(&mut self, row: &sqlx::mysql::MySqlRow) -> Result<()> {
        debug_assert_eq!(self.driver, DriverKind::MySql);
        if self.columns.is_empty() {
            self.columns = row
                .columns()
                .iter()
                .map(|col| ExportColumn {
                    name: col.name().to_string(),
                    data_type: col.type_info().to_string(),
                })
                .collect();
            self.kinds = self
                .columns
                .iter()
                .map(|col| classify_column(&col.data_type))
                .collect();
        }

        // SQL 导出按原始行的类型生成字面量，不能走类型化单元格（会丢失数值/日期字面量形式）
        if self.format == ExportFormat::Sql {
            self.start()?;
            let mut assignments = Vec::with_capacity(self.columns.len());
            for col in &self.columns {
                let value = format_sql_value(row, &col.name, &col.data_type)?;
                assignments.push(format!("`{}` = {}", col.name, value));
            }
            writeln!(
                self.writer,
                "INSERT INTO `{}` SET {};",
                self.table,
                assignments.join(", ")
            )?;
            self.rows += 1;
            return Ok(());
        }

        let mut cells = Vec::with_capacity(self.kinds.len());
        for (index, kind) in self.kinds.iter().enumerate() {
            cells.push(convert_value(row, index, *kind)?);
        }
        self.write_cells(&cells)
    }

    /// 写出一行 PostgreSQL 结果集
    ///
    /// 与 MySQL 版对称，但 SQL 导出使用 PG 的 `INSERT INTO … VALUES` 语法，
    /// 表名按 `schema.table` 展示名拆段加双引号，字面量按 PG 类型系统生成。
    pub fn write_pg_row(&mut self, row: &PgRow) -> Result<()> {
        debug_assert_eq!(self.driver, DriverKind::Postgres);
        if self.columns.is_empty() {
            self.columns = row
                .columns()
                .iter()
                .map(|col| ExportColumn {
                    name: col.name().to_string(),
                    data_type: col.type_info().to_string(),
                })
                .collect();
            self.pg_kinds = self
                .columns
                .iter()
                .map(|col| classify_pg_column(&col.data_type))
                .collect();
        }

        // SQL 导出按原始行的类型生成字面量，不能走类型化单元格（会丢失数值/日期字面量形式）
        if self.format == ExportFormat::Sql {
            self.start()?;
            let mut columns = Vec::with_capacity(self.columns.len());
            let mut values = Vec::with_capacity(self.columns.len());
            for col in &self.columns {
                columns.push(quote_pg_identifier(&col.name)?);
                values.push(format_pg_sql_value(row, &col.name, &col.data_type)?);
            }
            writeln!(
                self.writer,
                "INSERT INTO {} ({}) VALUES ({});",
                quote_pg_table_reference(&self.table)?,
                columns.join(", "),
                values.join(", ")
            )?;
            self.rows += 1;
            return Ok(());
        }

        let mut cells = Vec::with_capacity(self.pg_kinds.len());
        for (index, kind) in self.pg_kinds.iter().enumerate() {
            cells.push(convert_pg_value(row, index, *kind)?);
        }
        self.write_cells(&cells)
    }

    /// 写出一行 SQLite 结果集
    ///
    /// 与 PG 版对称：SQL 导出用 `INSERT INTO "table" ("c1") VALUES (…)` 语法，
    /// 字面量按 SQLite 存储类别生成（文本 '' 转义、BLOB x'…'、数值裸写）。
    pub fn write_sqlite_row(&mut self, row: &sqlx::sqlite::SqliteRow) -> Result<()> {
        debug_assert_eq!(self.driver, DriverKind::Sqlite);
        if self.columns.is_empty() {
            self.columns = row
                .columns()
                .iter()
                .map(|col| ExportColumn {
                    name: col.name().to_string(),
                    data_type: col.type_info().to_string(),
                })
                .collect();
            self.sqlite_kinds = self
                .columns
                .iter()
                .map(|col| classify_sqlite_column(&col.data_type))
                .collect();
        }

        // SQL 导出按原始行的类型生成字面量，不能走类型化单元格（会丢失数值/日期字面量形式）
        if self.format == ExportFormat::Sql {
            self.start()?;
            let mut columns = Vec::with_capacity(self.columns.len());
            let mut values = Vec::with_capacity(self.columns.len());
            for col in &self.columns {
                columns.push(quote_pg_identifier(&col.name)?);
                values.push(format_sqlite_sql_value(row, &col.name, &col.data_type)?);
            }
            writeln!(
                self.writer,
                "INSERT INTO {} ({}) VALUES ({});",
                quote_pg_identifier(&self.table)?,
                columns.join(", "),
                values.join(", ")
            )?;
            self.rows += 1;
            return Ok(());
        }

        let mut cells = Vec::with_capacity(self.sqlite_kinds.len());
        for (index, kind) in self.sqlite_kinds.iter().enumerate() {
            cells.push(convert_sqlite_value(row, index, *kind)?);
        }
        self.write_cells(&cells)
    }

    /// 写出一行已类型化的单元格值
    fn write_cells(&mut self, cells: &[CellValue]) -> Result<()> {
        self.start()?;
        match self.format {
            ExportFormat::Csv => {
                for index in 0..self.columns.len() {
                    if index > 0 {
                        self.writer.write_all(b",")?;
                    }
                    let text = cells.get(index).map(csv_cell).unwrap_or_default();
                    self.writer.write_all(text.as_bytes())?;
                }
                self.writer.write_all(LINE_END.as_bytes())?;
            }
            ExportFormat::Json => {
                if self.rows == 0 {
                    self.writer.write_all(b"\n")?;
                } else {
                    self.writer.write_all(b",\n")?;
                }
                self.writer.write_all(b"  {")?;
                if !self.columns.is_empty() {
                    self.writer.write_all(b"\n")?;
                }
                let col_count = self.columns.len();
                for (index, col) in self.columns.iter().enumerate() {
                    self.writer.write_all(b"    ")?;
                    self.writer
                        .write_all(serde_json::to_string(&col.name)?.as_bytes())?;
                    self.writer.write_all(b": ")?;
                    let cell = cells.get(index).unwrap_or(&CellValue::Null);
                    self.writer.write_all(json_cell(cell).as_bytes())?;
                    if index + 1 < col_count {
                        self.writer.write_all(b",")?;
                    }
                    self.writer.write_all(b"\n")?;
                }
                self.writer.write_all(b"  }")?;
            }
            ExportFormat::Sql => unreachable!("SQL 分行在 write_row 中处理"),
        }
        self.rows += 1;
        Ok(())
    }

    /// 收尾：补写 CSV 表头 / JSON 结束符并 flush
    pub fn finish(&mut self) -> Result<()> {
        self.start()?;
        if self.format == ExportFormat::Json {
            if self.rows == 0 {
                self.writer.write_all(b"]")?;
            } else {
                self.writer.write_all(b"\n]")?;
            }
        }
        self.writer.flush()?;
        Ok(())
    }

    /// 已写出的行数
    pub fn rows(&self) -> u64 {
        self.rows
    }

    /// 首次写入前的固定前缀（BOM/表头/JSON 起始符）
    fn start(&mut self) -> Result<()> {
        if self.started {
            return Ok(());
        }
        match self.format {
            ExportFormat::Csv => {
                self.writer.write_all("\u{feff}".as_bytes())?;
                let header = self
                    .columns
                    .iter()
                    .map(|col| csv_quote(&col.name, false))
                    .collect::<Vec<_>>()
                    .join(",");
                self.writer.write_all(header.as_bytes())?;
                self.writer.write_all(LINE_END.as_bytes())?;
            }
            ExportFormat::Json => self.writer.write_all(b"[")?,
            ExportFormat::Sql => {}
        }
        self.started = true;
        Ok(())
    }
}

/// 把 PG 行值格式化为 SQL 字面量（用于 SQL 格式导出）。
///
/// 与 MySQL 版语义一致：先判断原始值是否为 NULL，再解码；解码失败直接报错，
/// 绝不静默写成 NULL（否则重新导入会永久丢数据）。
fn format_pg_sql_value(row: &PgRow, column_name: &str, data_type: &str) -> Result<String> {
    let kind = classify_pg_column(data_type);
    let decode_error = |e: sqlx::Error| {
        anyhow!(
            "列 \"{}\"（{}）的值无法解码，已停止导出以避免写入错误的 NULL：{}",
            column_name,
            data_type,
            e
        )
    };

    match kind {
        PgColumnKind::Bool => match row.try_get::<Option<bool>, _>(column_name) {
            Ok(Some(v)) => Ok(if v { "TRUE" } else { "FALSE" }.to_string()),
            Ok(None) => Ok("NULL".to_string()),
            Err(e) => Err(decode_error(e)),
        },
        PgColumnKind::Int2 => match row.try_get::<Option<i16>, _>(column_name) {
            Ok(Some(v)) => Ok(v.to_string()),
            Ok(None) => Ok("NULL".to_string()),
            Err(e) => Err(decode_error(e)),
        },
        PgColumnKind::Int4 => match row.try_get::<Option<i32>, _>(column_name) {
            Ok(Some(v)) => Ok(v.to_string()),
            Ok(None) => Ok("NULL".to_string()),
            Err(e) => Err(decode_error(e)),
        },
        PgColumnKind::Int8 => match row.try_get::<Option<i64>, _>(column_name) {
            Ok(Some(v)) => Ok(v.to_string()),
            Ok(None) => Ok("NULL".to_string()),
            Err(e) => Err(decode_error(e)),
        },
        PgColumnKind::Float4 => match row.try_get::<Option<f32>, _>(column_name) {
            Ok(Some(v)) => Ok(float_literal(v as f64)),
            Ok(None) => Ok("NULL".to_string()),
            Err(e) => Err(decode_error(e)),
        },
        PgColumnKind::Float8 => match row.try_get::<Option<f64>, _>(column_name) {
            Ok(Some(v)) => Ok(float_literal(v)),
            Ok(None) => Ok("NULL".to_string()),
            Err(e) => Err(decode_error(e)),
        },
        PgColumnKind::Numeric => match row.try_get::<Option<rust_decimal::Decimal>, _>(column_name)
        {
            Ok(Some(v)) => Ok(v.to_string()),
            Ok(None) => Ok("NULL".to_string()),
            Err(e) => Err(decode_error(e)),
        },
        PgColumnKind::Timestamp => {
            match row.try_get::<Option<chrono::NaiveDateTime>, _>(column_name) {
                Ok(Some(v)) => Ok(format!("'{}'", v.format("%Y-%m-%d %H:%M:%S%.f"))),
                Ok(None) => Ok("NULL".to_string()),
                Err(e) => Err(decode_error(e)),
            }
        }
        PgColumnKind::TimestampTz => {
            match row.try_get::<Option<chrono::DateTime<chrono::Utc>>, _>(column_name) {
                Ok(Some(v)) => Ok(format!("'{}'", v.format("%Y-%m-%d %H:%M:%S%.f+00"))),
                Ok(None) => Ok("NULL".to_string()),
                Err(e) => Err(decode_error(e)),
            }
        }
        PgColumnKind::Date => match row.try_get::<Option<chrono::NaiveDate>, _>(column_name) {
            Ok(Some(v)) => Ok(format!("'{}'", v.format("%Y-%m-%d"))),
            Ok(None) => Ok("NULL".to_string()),
            Err(e) => Err(decode_error(e)),
        },
        PgColumnKind::Time => match row.try_get::<Option<chrono::NaiveTime>, _>(column_name) {
            Ok(Some(v)) => Ok(format!("'{}'", v.format("%H:%M:%S%.f"))),
            Ok(None) => Ok("NULL".to_string()),
            Err(e) => Err(decode_error(e)),
        },
        PgColumnKind::Json => match row.try_get::<Option<serde_json::Value>, _>(column_name) {
            Ok(Some(v)) => Ok(format!("'{}'", escape_string_pg(&v.to_string()))),
            Ok(None) => Ok("NULL".to_string()),
            Err(e) => Err(decode_error(e)),
        },
        PgColumnKind::Bytea => match row.try_get::<Option<Vec<u8>>, _>(column_name) {
            Ok(Some(v)) => {
                let hex: String = v.iter().map(|b| format!("{:02x}", b)).collect();
                Ok(format!("'\\x{}'", hex))
            }
            Ok(None) => Ok("NULL".to_string()),
            Err(e) => Err(decode_error(e)),
        },
        PgColumnKind::Uuid => match row.try_get::<Option<sqlx::types::Uuid>, _>(column_name) {
            Ok(Some(v)) => Ok(format!("'{}'", v)),
            Ok(None) => Ok("NULL".to_string()),
            Err(e) => Err(decode_error(e)),
        },
        PgColumnKind::Text => match row.try_get::<Option<String>, _>(column_name) {
            Ok(Some(v)) => Ok(format!("'{}'", escape_string_pg(&v))),
            Ok(None) => Ok("NULL".to_string()),
            Err(e) => {
                // 未识别类型（数组、自定义类型等）回退到原始字节，与单元格展示规则一致
                match row.try_get_raw(column_name) {
                    Ok(raw) if raw.is_null() => Ok("NULL".to_string()),
                    Ok(raw) => match raw.as_bytes() {
                        Ok(bytes) => {
                            let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
                            Ok(format!("'\\x{}'", hex))
                        }
                        Err(err) => Err(anyhow!(
                            "列 \"{}\"（{}）的值无法解码，已停止导出以避免写入错误的 NULL：{}",
                            column_name,
                            data_type,
                            err
                        )),
                    },
                    Err(_) => Err(decode_error(e)),
                }
            }
        },
    }
}

/// 按声明列类型生成 SQLite SQL 字面量。
///
/// SQLite 是动态类型：声明类型只决定亲和性，实际存储类别以值为准，
/// 因此每类先按首选类型解码，失败时逐级回退到 `try_get_raw` 的原始字节。
fn format_sqlite_sql_value(
    row: &sqlx::sqlite::SqliteRow,
    column_name: &str,
    data_type: &str,
) -> Result<String> {
    let kind = classify_sqlite_column(data_type);
    let decode_error = |e: sqlx::Error| {
        anyhow!(
            "列 \"{}\"（{}）的值无法解码，已停止导出以避免写入错误的 NULL：{}",
            column_name,
            data_type,
            e
        )
    };
    // 存储类别兜底：i64/f64/Vec<u8>/String 覆盖 SQLite 全部存储类别
    // （INTEGER/REAL/BLOB/TEXT/NULL）。先看声明的解码结果是不是 TEXT/BLOB
    // 类别，不是就用另一半；SqliteValueRef 无公开原始字节访问，靠类型化
    // try_get 的成功/失败区分存储类别。
    let raw_fallback = |row: &sqlx::sqlite::SqliteRow,
                        column_name: &str,
                        _data_type: &str,
                        e: sqlx::Error| -> Result<String> {
        if let Ok(raw) = row.try_get_raw(column_name) {
            if raw.is_null() {
                return Ok("NULL".to_string());
            }
        }
        if let Ok(Some(bytes)) = row.try_get::<Option<Vec<u8>>, _>(column_name) {
            let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
            return Ok(format!("x'{}'", hex));
        }
        if let Ok(Some(text)) = row.try_get::<Option<String>, _>(column_name) {
            return Ok(format!("'{}'", escape_string_pg(&text)));
        }
        Err(decode_error(e))
    };

    match kind {
        SqliteColumnKind::Integer => match row.try_get::<Option<i64>, _>(column_name) {
            Ok(Some(v)) => Ok(v.to_string()),
            Ok(None) => Ok("NULL".to_string()),
            Err(e) => raw_fallback(row, column_name, data_type, e),
        },
        SqliteColumnKind::Real => match row.try_get::<Option<f64>, _>(column_name) {
            Ok(Some(v)) => Ok(float_literal(v)),
            Ok(None) => Ok("NULL".to_string()),
            Err(e) => raw_fallback(row, column_name, data_type, e),
        },
        // NUMERIC 亲和性列按值的实际存储类别回存（INTEGER/REAL/TEXT 都可能出现），
        // 逐类尝试解码，保留原始字面量，避免 f64 往返精度丢失。
        SqliteColumnKind::Numeric => {
            if let Ok(v) = row.try_get::<Option<i64>, _>(column_name) {
                return Ok(v.map(|v| v.to_string()).unwrap_or_else(|| "NULL".into()));
            }
            if let Ok(Some(v)) = row.try_get::<Option<f64>, _>(column_name) {
                return Ok(float_literal(v));
            }
            match row.try_get::<Option<String>, _>(column_name) {
                Ok(Some(v)) => Ok(v),
                Ok(None) => Ok("NULL".to_string()),
                Err(e) => raw_fallback(row, column_name, data_type, e),
            }
        }
        SqliteColumnKind::Boolean => match row.try_get::<Option<bool>, _>(column_name) {
            Ok(Some(v)) => Ok(if v { "TRUE" } else { "FALSE" }.to_string()),
            Ok(None) => Ok("NULL".to_string()),
            Err(e) => raw_fallback(row, column_name, data_type, e),
        },
        SqliteColumnKind::Datetime => {
            match row.try_get::<Option<chrono::NaiveDateTime>, _>(column_name) {
                Ok(Some(v)) => Ok(format!("'{}'", v.format("%Y-%m-%d %H:%M:%S%.f"))),
                Ok(None) => Ok("NULL".to_string()),
                Err(e) => raw_fallback(row, column_name, data_type, e),
            }
        }
        SqliteColumnKind::Date => match row.try_get::<Option<chrono::NaiveDate>, _>(column_name) {
            Ok(Some(v)) => Ok(format!("'{}'", v.format("%Y-%m-%d"))),
            Ok(None) => Ok("NULL".to_string()),
            Err(e) => raw_fallback(row, column_name, data_type, e),
        },
        SqliteColumnKind::Time => match row.try_get::<Option<chrono::NaiveTime>, _>(column_name) {
            Ok(Some(v)) => Ok(format!("'{}'", v.format("%H:%M:%S%.f"))),
            Ok(None) => Ok("NULL".to_string()),
            Err(e) => raw_fallback(row, column_name, data_type, e),
        },
        SqliteColumnKind::Blob => match row.try_get::<Option<Vec<u8>>, _>(column_name) {
            Ok(Some(v)) => {
                let hex: String = v.iter().map(|b| format!("{:02x}", b)).collect();
                Ok(format!("x'{}'", hex))
            }
            Ok(None) => Ok("NULL".to_string()),
            Err(e) => raw_fallback(row, column_name, data_type, e),
        },
        SqliteColumnKind::Text => match row.try_get::<Option<String>, _>(column_name) {
            Ok(Some(v)) => Ok(format!("'{}'", escape_string_pg(&v))),
            Ok(None) => Ok("NULL".to_string()),
            Err(e) => raw_fallback(row, column_name, data_type, e),
        },
    }
}

/// 浮点字面量：NaN/Infinity 需要带引号才能被 PG 解析。
fn float_literal(v: f64) -> String {    if v.is_nan() {
        "'NaN'".to_string()
    } else if v.is_infinite() {
        if v.is_sign_positive() {
            "'Infinity'".to_string()
        } else {
            "'-Infinity'".to_string()
        }
    } else {
        v.to_string()
    }
}

fn csv_cell(cell: &CellValue) -> String {
    match cell {
        CellValue::Null => String::new(),
        CellValue::Integer(value) => value.to_string(),
        CellValue::Float(value) => format_float(*value),
        CellValue::Boolean(value) => (if *value { "1" } else { "0" }).to_string(),
        // 二进制在导出中为 Base64 文本，与表格展示保持一致
        CellValue::String(value) | CellValue::DateTime(value) | CellValue::Binary(value) => {
            csv_quote(&neutralize_formula(value), true)
        }
    }
}

/// 中和 CSV 公式注入：Excel/WPS 会把以 `= + - @` 开头的单元格当公式执行，
/// 因此文本类单元格前置单引号强制按字符串处理（OWASP CSV Injection 缓解）。
///
/// 代价是导出文本会多一个 `'`（导出后再导入不会自动去掉），
/// 属于安全优先于格式保真的取舍。
fn neutralize_formula(value: &str) -> String {
    match value.chars().next() {
        Some('=') | Some('+') | Some('-') | Some('@') | Some('\t') | Some('\r') => {
            format!("'{}", value)
        }
        _ => value.to_string(),
    }
}

fn json_cell(cell: &CellValue) -> String {
    match cell {
        CellValue::Null => "null".to_string(),
        CellValue::Integer(value) => value.to_string(),
        CellValue::Float(value) => {
            if value.is_finite() {
                value.to_string()
            } else {
                "null".to_string()
            }
        }
        CellValue::Boolean(value) => value.to_string(),
        CellValue::String(value) | CellValue::DateTime(value) | CellValue::Binary(value) => {
            serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
        }
    }
}

fn format_float(value: f64) -> String {
    if value.is_finite() {
        value.to_string()
    } else {
        String::new()
    }
}

/// 按 RFC 4180 决定是否加引号并转义
///
/// `protect_null` 为真时，文本 "NULL"（忽略大小写）也会加引号，
/// 以便导入侧把未加引号的 NULL 识别为 SQL NULL，而带引号的 "NULL" 仍是字符串。
fn csv_quote(value: &str, protect_null: bool) -> String {
    let needs_quote = value.is_empty()
        || value.contains(',')
        || value.contains('"')
        || value.contains('\n')
        || value.contains('\r')
        || value.starts_with(' ')
        || value.ends_with(' ')
        || (protect_null && value.eq_ignore_ascii_case("NULL"));

    if needs_quote {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// CSV 字段：`quoted` 用于区分 `NULL`（未加引号）与字符串 `"NULL"`/空串
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsvField {
    pub value: String,
    pub quoted: bool,
}

/// 解析 CSV 文本（支持引号包裹的逗号、换行与双写引号），忽略 UTF-8 BOM
pub fn parse_csv(text: &str) -> Vec<Vec<CsvField>> {
    let mut records: Vec<Vec<CsvField>> = Vec::new();
    let mut record: Vec<CsvField> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut in_quotes = false;

    let mut chars = text.chars().peekable();
    if text.starts_with(UTF8_BOM) {
        chars.next();
    }

    while let Some(ch) = chars.next() {
        if in_quotes {
            match ch {
                '"' => {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        field.push('"');
                    } else {
                        in_quotes = false;
                    }
                }
                _ => field.push(ch),
            }
        } else {
            match ch {
                '"' if field.is_empty() => {
                    in_quotes = true;
                    quoted = true;
                }
                // 非法位置的双引号按字面字符处理，避免整份文件解析失败
                '"' => field.push('"'),
                ',' => record.push(CsvField {
                    value: std::mem::take(&mut field),
                    quoted,
                }),
                '\r' => {}
                '\n' => {
                    record.push(CsvField {
                        value: std::mem::take(&mut field),
                        quoted,
                    });
                    quoted = false;
                    records.push(std::mem::take(&mut record));
                }
                _ => field.push(ch),
            }
        }
    }

    // 文件以换行结束时，残留记录为空，不追加多余行
    if !field.is_empty() || quoted || !record.is_empty() {
        record.push(CsvField {
            value: field,
            quoted,
        });
        records.push(record);
    }

    records
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用内存 writer 跑一遍流式写出，得到完整文本
    fn render(data: &TableExportData, format: ExportFormat) -> String {
        let mut sink = TableExportSink::new(
            Vec::new(),
            format,
            "t",
            data.columns.clone(),
            DriverKind::MySql,
        );
        for row in &data.rows {
            sink.write_cells(row).unwrap();
        }
        sink.finish().unwrap();
        String::from_utf8(sink.writer).unwrap()
    }

    fn render_csv(data: &TableExportData) -> String {
        render(data, ExportFormat::Csv)
    }

    fn render_json(data: &TableExportData) -> String {
        render(data, ExportFormat::Json)
    }

    fn sample() -> TableExportData {
        TableExportData {
            columns: vec![
                ExportColumn {
                    name: "id".into(),
                    data_type: "int".into(),
                },
                ExportColumn {
                    name: "name".into(),
                    data_type: "varchar".into(),
                },
                ExportColumn {
                    name: "note".into(),
                    data_type: "text".into(),
                },
                ExportColumn {
                    name: "created_at".into(),
                    data_type: "datetime".into(),
                },
                ExportColumn {
                    name: "score".into(),
                    data_type: "decimal".into(),
                },
            ],
            rows: vec![
                vec![
                    CellValue::Integer(1),
                    CellValue::String("张三".into()),
                    CellValue::String("含,逗号 与\"引号\"\n换行".into()),
                    CellValue::DateTime("2026-10-22 10:00:00".into()),
                    CellValue::String("12.50".into()),
                ],
                vec![
                    CellValue::Integer(2),
                    CellValue::Null,
                    CellValue::String(String::new()),
                    CellValue::Null,
                    CellValue::Null,
                ],
            ],
        }
    }

    #[test]
    fn csv_neutralizes_formula_injection() {
        let data = TableExportData {
            columns: vec![ExportColumn {
                name: "v".into(),
                data_type: "varchar".into(),
            }],
            rows: vec![
                vec![CellValue::String("=HYPERLINK(\"http://evil\")".into())],
                vec![CellValue::String("+1+1".into())],
                vec![CellValue::String("-2+3".into())],
                vec![CellValue::String("@SUM(A1:A2)".into())],
                vec![CellValue::String("normal".into())],
            ],
        };
        let csv = render_csv(&data);
        assert!(csv.contains("'=HYPERLINK"), "公式必须以单引号中和: {csv}");
        assert!(csv.contains("'+1+1"), "公式必须以单引号中和: {csv}");
        assert!(csv.contains("'-2+3"), "公式必须以单引号中和: {csv}");
        assert!(csv.contains("'@SUM"), "公式必须以单引号中和: {csv}");
        assert!(csv.contains("\r\nnormal\r\n"), "普通文本不应被改写: {csv}");
    }

    #[test]
    fn csv_has_bom_and_escapes() {
        let csv = render_csv(&sample());
        assert!(csv.starts_with('\u{feff}'), "CSV 必须以 BOM 开头");
        assert!(csv.contains("id,name,note,created_at,score\r\n"));
        // 逗号/引号/换行触发加引号，内部引号双写
        assert!(csv.contains("\"含,逗号 与\"\"引号\"\"\n换行\""));
        // NULL 为空字段，空字符串为 ""
        assert!(csv.contains("2,,\"\",,\r\n"));
    }

    #[test]
    fn csv_round_trip_preserves_quoted_values() {
        let csv = render_csv(&sample());
        let records = parse_csv(&csv);
        assert_eq!(records.len(), 3);
        assert_eq!(records[0][0].value, "id");
        assert_eq!(records[1][2].value, "含,逗号 与\"引号\"\n换行");
        assert!(records[1][2].quoted);
        // 第 2 行：NULL 字段未加引号，空字符串加引号
        assert_eq!(records[2][1].value, "");
        assert!(!records[2][1].quoted);
        assert_eq!(records[2][2].value, "");
        assert!(records[2][2].quoted);
    }

    #[test]
    fn null_text_is_quoted_to_avoid_ambiguity() {
        let data = TableExportData {
            columns: vec![ExportColumn {
                name: "v".into(),
                data_type: "varchar".into(),
            }],
            rows: vec![
                vec![CellValue::String("NULL".into())],
                vec![CellValue::Null],
            ],
        };
        let csv = render_csv(&data);
        assert!(csv.ends_with("v\r\n\"NULL\"\r\n\r\n"));
        let records = parse_csv(&csv);
        assert_eq!(records[1][0].value, "NULL");
        assert!(records[1][0].quoted);
        assert_eq!(records[2][0].value, "");
        assert!(!records[2][0].quoted);
    }

    #[test]
    fn json_keeps_column_order_and_types() {
        let json = render_json(&sample());
        let id_pos = json.find("\"id\"").unwrap();
        let name_pos = json.find("\"name\"").unwrap();
        let note_pos = json.find("\"note\"").unwrap();
        assert!(
            id_pos < name_pos && name_pos < note_pos,
            "JSON 字段必须保持列顺序"
        );
        assert!(json.contains("\"id\": 1"));
        assert!(json.contains("\"created_at\": \"2026-10-22 10:00:00\""));
        assert!(json.contains("\"score\": \"12.50\""));
        assert!(json.contains("\"name\": null"));
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.as_array().unwrap().len(), 2);
    }

    #[test]
    fn parse_csv_handles_bom_crlf_and_trailing_newline() {
        let records = parse_csv("\u{feff}a,b\r\n1,2\r\n");
        assert_eq!(records.len(), 2);
        assert_eq!(records[1][1].value, "2");
    }
}
