//! 表数据导出格式化模块
//! 将类型化的行值序列化为 CSV / JSON，并提供与导出对称的 CSV 解析。
//!
//! 设计要点：
//! - 值来自 `convert_value`，保留数字/日期/NULL 的真实类型，不再把所有非字符串列写成 "NULL"。
//! - CSV 遵循 RFC 4180：逗号/双引号/换行触发加引号，引号内部 `"` 双写；行尾 CRLF。
//! - CSV 以 UTF-8 BOM 开头，避免 Excel 打开中文乱码。
//! - NULL 输出为空字段；空字符串输出为 `""`，使导入侧能区分两者。

use super::types::CellValue;
use anyhow::Result;

/// 导出列定义（名称 + INFORMATION_SCHEMA 中的 DATA_TYPE）
#[derive(Debug, Clone)]
pub struct ExportColumn {
    pub name: String,
    pub data_type: String,
}

/// 一次表导出的完整数据：有序的列定义与类型化行值
#[derive(Debug, Clone)]
pub struct TableExportData {
    pub columns: Vec<ExportColumn>,
    pub rows: Vec<Vec<CellValue>>,
}

const UTF8_BOM: char = '\u{feff}';
const LINE_END: &str = "\r\n";

/// 序列化为 CSV 文本
pub fn to_csv(data: &TableExportData) -> String {
    let mut out = String::new();
    out.push(UTF8_BOM);

    let header = data
        .columns
        .iter()
        .map(|col| csv_quote(&col.name, false))
        .collect::<Vec<_>>()
        .join(",");
    out.push_str(&header);
    out.push_str(LINE_END);

    for row in &data.rows {
        let line = data
            .columns
            .iter()
            .enumerate()
            .map(|(index, _)| {
                row.get(index)
                    .map(csv_cell)
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(",");
        out.push_str(&line);
        out.push_str(LINE_END);
    }

    out
}

/// 序列化为 JSON 数组文本（保持列顺序，数字/布尔/NULL 保留原始类型）
pub fn to_json(data: &TableExportData) -> Result<String> {
    let mut out = String::from("[");
    if !data.rows.is_empty() {
        out.push('\n');
    }

    let row_count = data.rows.len();
    for (row_index, row) in data.rows.iter().enumerate() {
        out.push_str("  {");
        if !data.columns.is_empty() {
            out.push('\n');
        }

        let col_count = data.columns.len();
        for (col_index, col) in data.columns.iter().enumerate() {
            out.push_str("    ");
            out.push_str(&serde_json::to_string(&col.name)?);
            out.push_str(": ");
            let cell = row.get(col_index).unwrap_or(&CellValue::Null);
            out.push_str(&json_cell(cell));
            if col_index + 1 < col_count {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  }");
        if row_index + 1 < row_count {
            out.push(',');
        }
        out.push('\n');
    }

    out.push(']');
    Ok(out)
}

fn csv_cell(cell: &CellValue) -> String {
    match cell {
        CellValue::Null => String::new(),
        CellValue::Integer(value) => value.to_string(),
        CellValue::Float(value) => format_float(*value),
        CellValue::Boolean(value) => (if *value { "1" } else { "0" }).to_string(),
        // 二进制在导出中为 Base64 文本，与表格展示保持一致
        CellValue::String(value) | CellValue::DateTime(value) | CellValue::Binary(value) => {
            csv_quote(value, true)
        }
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
        record.push(CsvField { value: field, quoted });
        records.push(record);
    }

    records
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> TableExportData {
        TableExportData {
            columns: vec![
                ExportColumn { name: "id".into(), data_type: "int".into() },
                ExportColumn { name: "name".into(), data_type: "varchar".into() },
                ExportColumn { name: "note".into(), data_type: "text".into() },
                ExportColumn { name: "created_at".into(), data_type: "datetime".into() },
                ExportColumn { name: "score".into(), data_type: "decimal".into() },
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
    fn csv_has_bom_and_escapes() {
        let csv = to_csv(&sample());
        assert!(csv.starts_with('\u{feff}'), "CSV 必须以 BOM 开头");
        assert!(csv.contains("id,name,note,created_at,score\r\n"));
        // 逗号/引号/换行触发加引号，内部引号双写
        assert!(csv.contains("\"含,逗号 与\"\"引号\"\"\n换行\""));
        // NULL 为空字段，空字符串为 ""
        assert!(csv.contains("2,,\"\",,\r\n"));
    }

    #[test]
    fn csv_round_trip_preserves_quoted_values() {
        let csv = to_csv(&sample());
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
            columns: vec![ExportColumn { name: "v".into(), data_type: "varchar".into() }],
            rows: vec![
                vec![CellValue::String("NULL".into())],
                vec![CellValue::Null],
            ],
        };
        let csv = to_csv(&data);
        assert!(csv.ends_with("v\r\n\"NULL\"\r\n\r\n"));
        let records = parse_csv(&csv);
        assert_eq!(records[1][0].value, "NULL");
        assert!(records[1][0].quoted);
        assert_eq!(records[2][0].value, "");
        assert!(!records[2][0].quoted);
    }

    #[test]
    fn json_keeps_column_order_and_types() {
        let json = to_json(&sample()).unwrap();
        let id_pos = json.find("\"id\"").unwrap();
        let name_pos = json.find("\"name\"").unwrap();
        let note_pos = json.find("\"note\"").unwrap();
        assert!(id_pos < name_pos && name_pos < note_pos, "JSON 字段必须保持列顺序");
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
