//! 统一错误出口：把 MySQL 驱动错误翻译成中文可读提示。
//!
//! 规则：
//! - 驱动错误按 MySQL 错误码精确匹配（见 `KNOWN_ERRORS`），输出「中文提示（MySQL <code>：<原始信息>）」；
//! - 未收录的错误码回退为「数据库错误（<code>）：<原始信息>」，不吞错、不丢码；
//! - 非驱动错误（参数校验、业务逻辑等）保持原有信息，不翻译。

use sqlx::error::Error as SqlxError;

/// 已收录的 MySQL 错误码 → 中文提示
const KNOWN_ERRORS: &[(&str, &str)] = &[
    // 连接与认证
    ("1045", "认证失败：用户名或密码错误，请检查连接配置"),
    ("1049", "数据库不存在：请检查默认数据库名称"),
    ("2002", "无法连接到数据库服务器：请检查主机地址、端口及网络"),
    ("2003", "无法连接到数据库服务器：请检查主机地址、端口及网络"),
    ("2013", "连接已断开：查询执行期间与服务器失去连接，请重试"),
    // 权限
    ("1044", "权限不足：当前账号无法访问该数据库"),
    ("1142", "权限不足：当前账号没有执行该操作的权限"),
    ("1143", "权限不足：当前账号没有访问该列的权限"),
    // 数据库/表/列不存在或冲突
    ("1046", "未选择数据库：请先 USE 或指定数据库"),
    ("1050", "表已存在：无法创建同名表"),
    ("1051", "表不存在：请确认表名是否正确"),
    ("1054", "列不存在：请检查 SQL 中的列名"),
    ("1091", "操作对象不存在：请刷新表结构后重试"),
    ("1146", "表不存在：请确认表名是否正确，或刷新元数据后重试"),
    // 数据约束
    ("1048", "字段不能为空：存在 NOT NULL 字段未赋值"),
    ("1062", "重复键冲突：已存在相同的主键或唯一键值"),
    ("1216", "外键约束冲突：关联数据不存在"),
    ("1217", "外键约束冲突：数据仍被其他表引用"),
    ("1264", "数值超出字段范围"),
    ("1265", "数据被截断：值超出字段定义长度"),
    ("1366", "值不合法：数据类型或格式不正确"),
    ("1451", "外键约束冲突：该记录仍被其他表引用，无法删除或修改"),
    ("1452", "外键约束冲突：引用的记录不存在"),
    // 语法
    ("1064", "SQL 语法错误：请检查语句"),
    // 锁与超时
    ("1205", "锁等待超时：请稍后重试"),
    ("1213", "死锁：事务已被回滚，请重试"),
];

/// 将 sqlx 错误翻译为中文可读提示
pub fn translate(err: &SqlxError) -> String {
    match err {
        SqlxError::Database(db) => translate_database_error(db.as_ref()),
        SqlxError::Io(e) => translate_io_error(e),
        SqlxError::PoolClosed => "连接池已关闭，请重新连接".to_string(),
        SqlxError::PoolTimedOut => "从连接池获取连接超时，请稍后重试".to_string(),
        SqlxError::RowNotFound => "查询没有返回任何行".to_string(),
        SqlxError::ColumnNotFound(name) => format!("列不存在：{}", name),
        SqlxError::ColumnIndexOutOfBounds { index, len } => {
            format!("列索引越界：第 {} 列（共 {} 列）", index, len)
        }
        SqlxError::Decode(e) => format!("数据解码错误：{}", e),
        SqlxError::ColumnDecode { index, source } => {
            format!("第 {} 列数据解码错误：{}", index, source)
        }
        SqlxError::Tls(e) => format!("TLS 握手失败：{}", e),
        SqlxError::Protocol(msg) => format!("数据库协议错误：{}", msg),
        SqlxError::TypeNotFound { type_name } => format!("未知的数据类型：{}", type_name),
        SqlxError::Configuration(msg) => format!("连接配置错误：{}", msg),
        SqlxError::AnyDriverError(e) => format!("数据库驱动错误：{}", e),
        SqlxError::WorkerCrashed => "数据库驱动内部错误，请重试".to_string(),
        _ => format!("数据库错误：{}", err),
    }
}

/// rustls 的证书校验失败被 sqlx 包成 `Io`，直译成「网络 I/O 错误」会误导排查方向
fn translate_io_error(e: &std::io::Error) -> String {
    let msg = e.to_string();
    if msg.contains("invalid peer certificate") || msg.contains("invalid certificate") {
        format!("TLS 证书校验失败：{}", msg)
    } else if msg.contains("corrupt message") || msg.contains("UnknownProtocolVersion") {
        // TLS 协商阶段读到明文应答：服务端拒绝或未正确配置 TLS
        format!(
            "TLS 握手失败：服务器未正常响应 TLS 协商（{}）。请检查服务端 TLS 配置，或将 SSL 模式改为「禁用」",
            msg
        )
    } else {
        format!("网络 I/O 错误：{}", msg)
    }
}

fn translate_database_error(db: &dyn sqlx::error::DatabaseError) -> String {
    let code = db.code().map(|c| c.to_string()).unwrap_or_default();
    let message = db.message();

    match KNOWN_ERRORS.iter().find(|(c, _)| *c == code) {
        Some((_, hint)) => format!("{}（MySQL {}：{}）", hint, code, message),
        None if !code.is_empty() => format!("数据库错误（{}）：{}", code, message),
        None => format!("数据库错误：{}", message),
    }
}

/// 命令层统一错误出口：沿错误链找到第一个驱动错误并翻译，否则保留原信息
pub fn user_message(err: &anyhow::Error) -> String {
    for cause in err.chain() {
        if let Some(sqlx_err) = cause.downcast_ref::<SqlxError>() {
            return translate(sqlx_err);
        }
    }
    err.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::error::ErrorKind;
    use std::borrow::Cow;
    use std::fmt::{Debug, Display};

    /// 测试用最小 DatabaseError 实现
    struct MockDbError {
        code: &'static str,
        message: &'static str,
    }

    impl Display for MockDbError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "error {}: {}", self.code, self.message)
        }
    }

    impl Debug for MockDbError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "MockDbError({})", self.code)
        }
    }

    impl std::error::Error for MockDbError {}

    impl sqlx::error::DatabaseError for MockDbError {
        fn message(&self) -> &str {
            self.message
        }

        fn code(&self) -> Option<Cow<'_, str>> {
            Some(Cow::Borrowed(self.code))
        }

        fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
            self
        }

        fn kind(&self) -> ErrorKind {
            ErrorKind::Other
        }
    }

    fn db_error(code: &'static str, message: &'static str) -> SqlxError {
        SqlxError::Database(Box::new(MockDbError { code, message }))
    }

    #[test]
    fn test_translate_known_codes() {
        // 任务要求的四个核心错误码必须覆盖
        let dup = translate(&db_error("1062", "Duplicate entry '1' for key 'PRIMARY'"));
        assert!(dup.contains("重复键冲突"), "got: {dup}");
        assert!(dup.contains("1062"), "got: {dup}");

        let no_table = translate(&db_error("1146", "Table 'test.users' doesn't exist"));
        assert!(no_table.contains("表不存在"), "got: {no_table}");

        let denied = translate(&db_error("1045", "Access denied"));
        assert!(denied.contains("认证失败"), "got: {denied}");

        let lost = translate(&db_error("2013", "Lost connection"));
        assert!(lost.contains("连接已断开"), "got: {lost}");

        let timeout = translate(&db_error("1205", "Lock wait timeout exceeded"));
        assert!(timeout.contains("锁等待超时"), "got: {timeout}");

        let conn = translate(&db_error("2002", "Can't connect to server"));
        assert!(conn.contains("无法连接到数据库服务器"), "got: {conn}");
    }

    #[test]
    fn test_translate_known_codes_keep_raw_message() {
        // 中文化只替换「提示语」，MySQL 原始报文与错误码必须原样保留，
        // 否则用户无法按官方文档/搜索引擎排障
        let cases = [
            ("1062", "Duplicate entry '1' for key 'PRIMARY'"),
            ("1146", "Table 'test.users' doesn't exist"),
            ("1045", "Access denied for user 'root'@'localhost'"),
            ("2013", "Lost connection to MySQL server during query"),
            ("1205", "Lock wait timeout exceeded; try restarting transaction"),
            ("2002", "Can't connect to local MySQL server through socket"),
        ];

        for (code, raw) in cases {
            let msg = translate(&db_error(code, raw));
            assert!(msg.contains(code), "错误码 {code} 未保留：{msg}");
            assert!(msg.contains(raw), "错误码 {code} 的原始报文丢失：{msg}");
        }
    }

    #[test]
    fn test_translate_unknown_code_keeps_code_and_message() {
        let msg = translate(&db_error("99999", "some weird error"));
        assert!(msg.contains("99999"), "got: {msg}");
        assert!(msg.contains("some weird error"), "got: {msg}");
    }

    #[test]
    fn test_translate_non_database_errors() {
        assert_eq!(translate(&SqlxError::RowNotFound), "查询没有返回任何行");
        assert_eq!(
            translate(&SqlxError::ColumnNotFound("name".to_string())),
            "列不存在：name"
        );
        assert_eq!(translate(&SqlxError::PoolClosed), "连接池已关闭，请重新连接");
    }

    #[test]
    fn test_translate_certificate_io_errors() {
        // rustls 的校验失败：不能显示成网络错误
        let cert = SqlxError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid peer certificate: NotValidForName",
        ));
        let msg = translate(&cert);
        assert!(msg.starts_with("TLS 证书校验失败："), "got: {msg}");
        assert!(msg.contains("NotValidForName"), "got: {msg}");

        // 普通网络错误保持原文案
        let net = SqlxError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "Connection refused",
        ));
        let msg = translate(&net);
        assert!(msg.starts_with("网络 I/O 错误："), "got: {msg}");
    }

    #[test]
    fn test_translate_tls_handshake_corruption() {
        // 服务端 TLS 协商失败返回明文，rustls 报损坏记录：应提示 TLS 而非网络
        let corrupt = SqlxError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "received corrupt message of type UnknownProtocolVersion",
        ));
        let msg = translate(&corrupt);
        assert!(msg.starts_with("TLS 握手失败："), "got: {msg}");
        assert!(msg.contains("服务端 TLS 配置"), "got: {msg}");
    }

    #[test]
    fn test_user_message() {
        // 根部是驱动错误 → 翻译（生产路径为 `?` 的 From 转换，此处用 into 等价模拟）
        let err: anyhow::Error = db_error("1062", "Duplicate entry").into();
        assert!(user_message(&err).contains("重复键冲突"));

        // 包装了上下文的驱动错误 → 仍能沿链翻译
        let wrapped: anyhow::Error = db_error("1146", "Table 'test.t' doesn't exist")
            .into();
        let wrapped = wrapped.context("执行查询失败");
        let msg = user_message(&wrapped);
        assert!(msg.contains("表不存在"), "got: {msg}");
        assert!(msg.contains("1146"), "got: {msg}");

        // 普通业务错误 → 原样保留
        let plain = anyhow::anyhow!("SQL 长度不能超过 10MB");
        assert_eq!(user_message(&plain), "SQL 长度不能超过 10MB");
    }
}
