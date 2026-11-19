//! SQL 文本工具类命令（无连接参与，纯前端辅助能力）

use sqlformat::{FormatOptions, Indent, QueryParams};

/// 前端传来的格式化参数
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatSqlArgs {
    /// 缩进宽度，默认 2
    pub indent_size: Option<u8>,
}

/// 用 sqlformat 格式化 SQL：正确处理嵌套子查询、窗口函数、字符串字面量与注释，
/// 替代前端正则实现。
#[tauri::command]
pub fn format_sql(sql: String, args: Option<FormatSqlArgs>) -> Result<String, String> {
    let indent_size = args.and_then(|a| a.indent_size).unwrap_or(2);
    let indent = if indent_size >= 4 {
        Indent::Spaces(4)
    } else {
        Indent::Spaces(2)
    };

    // 关键字统一大写，与前端正则版行为一致
    let options = FormatOptions {
        indent,
        uppercase: Some(true),
        ..FormatOptions::default()
    };

    Ok(sqlformat::format(&sql, &QueryParams::default(), &options))
}
