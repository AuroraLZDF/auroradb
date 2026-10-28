use super::driver::DriverKind;
use serde::{Deserialize, Serialize};

/// 连接配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub id: String,
    pub name: String,
    /// 数据库驱动，缺省 MySQL（兼容旧配置）
    #[serde(default)]
    pub driver: DriverKind,
    pub host: String,
    pub port: u16,
    pub database: Option<String>,
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub pool_config: PoolConfig,
    /// SSL/TLS 配置，缺省为 Preferred（与 sqlx 默认一致，兼容旧配置）
    #[serde(default)]
    pub ssl_config: SslConfig,
    /// 访问模式，缺省为 Off（兼容旧配置）
    #[serde(default)]
    pub safety_mode: SafetyMode,
}

/// 连接访问模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum SafetyMode {
    /// 不限制
    #[default]
    Off,
    /// 安全模式：拦截不带 WHERE 的 UPDATE/DELETE
    Safe,
    /// 只读模式：仅允许查询类语句
    ReadOnly,
}

/// SSL/TLS 模式，对应各驱动的 sqlx SSL 模式（MySQL / PostgreSQL）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum SslMode {
    /// 不加密
    Disabled,
    /// 服务器支持则加密（默认，不校验证书）
    #[default]
    Preferred,
    /// 必须加密，但不校验证书
    Required,
    /// 必须加密并校验服务端证书链（需 CA）
    VerifyCa,
    /// 必须加密、校验证书链与主机名（需 CA）
    VerifyIdentity,
}

/// SSL/TLS 配置
///
/// 证书相关字段只保存本地文件路径，不保存证书内容。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SslConfig {
    #[serde(default)]
    pub mode: SslMode,
    /// CA 证书路径（verify-ca / verify-identity 时用于校验服务端）
    #[serde(default)]
    pub ca_path: Option<String>,
    /// 客户端证书路径（双向 TLS 时使用）
    #[serde(default)]
    pub cert_path: Option<String>,
    /// 客户端私钥路径（双向 TLS 时使用）
    #[serde(default)]
    pub key_path: Option<String>,
}

/// 连接池配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    #[serde(default = "default_min_connections")]
    pub min_connections: u32,
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout: u64,
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout: u64,
    #[serde(default = "default_max_lifetime")]
    pub max_lifetime: u64,
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval: u64,
}

fn default_max_connections() -> u32 { 10 }
fn default_min_connections() -> u32 { 2 }
fn default_connect_timeout() -> u64 { 10 }
fn default_idle_timeout() -> u64 { 300 }
fn default_max_lifetime() -> u64 { 1800 }
fn default_heartbeat_interval() -> u64 { 30 }

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            min_connections: 2,
            connect_timeout: 10,
            idle_timeout: 300,
            max_lifetime: 1800,
            heartbeat_interval: 30,
        }
    }
}

/// 连接状态
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Reconnecting { attempt: u32 },
    Failed(String),
}

/// SSH配置
///
/// 数据库主机/端口（`ConnectionConfig.host/port`）在启用 SSH 时是「SSH 服务器视角」的目标地址，
/// 由远端解析；本地实际连接 `127.0.0.1:<隧道端口>`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshConfig {
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    pub username: String,
    pub auth_type: SshAuthType,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub private_key_path: Option<String>,
    #[serde(default)]
    pub passphrase: Option<String>,
}

fn default_ssh_port() -> u16 { 22 }

/// SSH 认证方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SshAuthType {
    Password,
    PrivateKey,
    Agent,
}

/// 连接状态DTO
#[derive(Debug, Clone, Serialize)]
pub struct ConnectionStatus {
    pub id: String,
    /// 连接使用的驱动，供前端做方言差异化展示
    pub driver: DriverKind,
    /// 实际连接的数据库名（PG 配置未填库时为服务端默认库）
    pub database: Option<String>,
    pub state: String,
    pub message: String,
    pub connected_at: Option<String>,
    pub last_error: Option<String>,
    pub stability_score: u8,
    pub health_status: String,
}

/// 连接请求
#[derive(Debug, Clone, Deserialize)]
pub struct ConnectionRequest {
    pub config: ConnectionConfig,
    pub ssh_config: Option<SshConfig>,
}

/// 单元格值类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CellValue {
    Null,
    Integer(i64),
    Float(f64),
    String(String),
    Boolean(bool),
    DateTime(String),
    Binary(String),
}

/// 查询结果
#[derive(Debug, Clone, Serialize)]
pub struct QueryResult {
    pub columns: Vec<ColumnDefinition>,
    pub rows: Vec<Vec<CellValue>>,
    pub execution_time_ms: u64,
    pub rows_affected: Option<u64>,
    pub rows_returned: usize,
    /// 结果集是否因超出行数上限被截断（仅 SELECT 类查询可能出现）
    #[serde(default)]
    pub truncated: bool,
    /// 截断等需要提示用户的信息（前端展示用）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// 列定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnDefinition {
    pub name: String,
    pub data_type: String,
    pub is_nullable: bool,
    pub is_primary: bool,
    pub is_auto_increment: bool,
}

/// 数据库对象类型
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ObjectType {
    Connection,
    Database,
    Table,
    View,
    Procedure,
    Function,
}

/// 数据库对象
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseObject {
    pub id: String,
    pub name: String,
    pub object_type: ObjectType,
    pub schema: Option<String>,
    pub children: Option<Vec<DatabaseObject>>,
    /// 自增值（仅 MySQL 表，视图或未定义自增列时为 None）
    #[serde(default)]
    pub auto_increment: Option<i64>,
    /// 最后修改时间（仅 MySQL 表，字符串由数据库格式化）
    #[serde(default)]
    pub updated_at: Option<String>,
    /// 表占用空间字节数（数据 + 索引，仅 MySQL 表）
    #[serde(default)]
    pub size_bytes: Option<i64>,
}

/// 过滤条件
#[derive(Debug, Clone, Deserialize)]
pub struct FilterCondition {
    pub column: String,
    pub operator: FilterOperator,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub enum FilterOperator {
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
    Like,
    IsNull,
    IsNotNull,
}

/// 排序方向
#[derive(Debug, Clone, Copy, Deserialize)]
pub enum SortDirection {
    Asc,
    Desc,
}

/// 数据查询请求
#[derive(Debug, Clone, Deserialize)]
pub struct DataQueryRequest {
    pub connection_id: String,
    pub database: String,
    pub table: String,
    pub columns: Option<Vec<String>>,
    pub filter: Option<FilterCondition>,
    /// 关键字搜索：在所有可比较列上做 LIKE '%keyword%' 的包含匹配
    pub search: Option<String>,
    pub sort_column: Option<String>,
    pub sort_direction: Option<SortDirection>,
    pub page: usize,
    pub page_size: usize,
}

/// 数据查询响应
#[derive(Debug, Clone, Serialize)]
pub struct DataQueryResponse {
    pub columns: Vec<ColumnDefinition>,
    pub rows: Vec<Vec<CellValue>>,
    pub total_count: u64,
    pub page: usize,
    pub page_size: usize,
    pub execution_time_ms: u64,
}

/// 表数据变更类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TableChangeKind {
    Update,
    Insert,
    Delete,
}

/// 单条表数据变更
///
/// SQL 由前端按当前表结构生成（标识符与值已转义），后端负责在同一个事务中执行，
/// 并逐条校验影响行数，避免「改到一半失败」的半写状态。
#[derive(Debug, Clone, Deserialize)]
pub struct TableChange {
    pub kind: TableChangeKind,
    /// 已拼好的单条 DML（不带结尾分号）
    pub sql: String,
    /// 目标表名（与 `sql` 中的表名一致），用于影响 0 行时二次确认目标行是否存在
    pub table: String,
    /// UPDATE/DELETE 的行定位条件（不含 WHERE 关键字）
    #[serde(default)]
    pub where_clause: Option<String>,
}

/// 表数据变更执行结果
#[derive(Debug, Clone, Serialize)]
pub struct ApplyChangesResult {
    /// 实际执行并提交的语句条数
    pub applied: usize,
    /// UPDATE 值未变化（影响 0 行但目标行仍存在）的条数
    pub unchanged: usize,
}

/// 数据库信息
#[derive(Debug, Clone, Serialize)]
pub struct DatabaseInfo {
    pub name: String,
    pub charset: Option<String>,
    pub collation: Option<String>,
    pub table_count: u64,
    pub data_size: Option<u64>,
    pub index_size: Option<u64>,
    pub total_size: Option<u64>,
    pub create_time: Option<String>,
}

/// 表结构信息
#[derive(Debug, Clone, Serialize)]
pub struct TableStructure {
    pub columns: Vec<TableColumnInfo>,
    pub indexes: Vec<TableIndexInfo>,
    pub foreign_keys: Vec<TableForeignKeyInfo>,
    #[serde(rename = "createStatement")]
    pub create_statement: String,
    pub engine: Option<String>,
    pub charset: Option<String>,
    pub collation: Option<String>,
    pub row_count: Option<u64>,
    pub data_size: Option<u64>,
    pub index_size: Option<u64>,
    pub create_time: Option<String>,
    pub update_time: Option<String>,
}

/// 表列信息
#[derive(Debug, Clone, Serialize)]
pub struct TableColumnInfo {
    pub name: String,
    pub data_type: String,
    pub length: Option<u32>,
    pub decimals: Option<u32>,
    pub is_nullable: bool,
    pub default_value: Option<String>,
    pub is_auto_increment: bool,
    pub is_primary: bool,
    pub unsigned: bool,
    pub on_update_current_timestamp: bool,
    pub comment: Option<String>,
}

/// 表索引信息
#[derive(Debug, Clone, Serialize)]
pub struct TableIndexInfo {
    pub name: String,
    pub r#type: String, // type is reserved keyword
    pub columns: Vec<String>,
    pub is_unique: bool,
}

/// 表外键信息
#[derive(Debug, Clone, Serialize)]
pub struct TableForeignKeyInfo {
    pub name: String,
    pub column: String,
    pub ref_table: String,
    pub ref_column: String,
    pub on_delete: String,
    pub on_update: String,
}

/// schema 快照中的列（仅保留补全所需字段）
#[derive(Debug, Clone, Serialize)]
pub struct SchemaColumnInfo {
    pub name: String,
    pub data_type: String,
}

/// schema 快照中的表/视图
#[derive(Debug, Clone, Serialize)]
pub struct SchemaTableInfo {
    pub name: String,
    pub object_type: ObjectType,
    pub columns: Vec<SchemaColumnInfo>,
}

/// 数据库 schema 快照：一次查询拿到所有表与列，供编辑器补全使用
#[derive(Debug, Clone, Serialize)]
pub struct SchemaSnapshot {
    pub database: String,
    pub tables: Vec<SchemaTableInfo>,
}

/// Schema 对比：列定义（来自 INFORMATION_SCHEMA.COLUMNS）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SchemaCompareColumn {
    pub name: String,
    pub column_type: String,
    pub is_nullable: bool,
    pub default_value: Option<String>,
    pub extra: String,
    pub comment: String,
    pub ordinal: u32,
    pub generation_expression: Option<String>,
}

/// Schema 对比：索引定义（来自 INFORMATION_SCHEMA.STATISTICS）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SchemaCompareIndex {
    pub name: String,
    pub columns: Vec<String>,
    pub is_unique: bool,
    pub index_type: String,
}

/// Schema 对比：表结构快照（仅基表）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SchemaCompareTable {
    pub name: String,
    pub columns: Vec<SchemaCompareColumn>,
    pub indexes: Vec<SchemaCompareIndex>,
}

/// 差异类型：Added=仅源库存在，Removed=仅目标库存在，Changed=两边都有但不一致
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffChange {
    Added,
    Removed,
    Changed,
}

/// 列级差异
#[derive(Debug, Clone, Serialize)]
pub struct SchemaColumnDiff {
    pub name: String,
    pub change: DiffChange,
    pub source: Option<String>,
    pub target: Option<String>,
    pub details: Vec<String>,
}

/// 索引级差异
#[derive(Debug, Clone, Serialize)]
pub struct SchemaIndexDiff {
    pub name: String,
    pub change: DiffChange,
    pub source: Option<String>,
    pub target: Option<String>,
}

/// 表级差异
#[derive(Debug, Clone, Serialize)]
pub struct SchemaTableDiff {
    pub name: String,
    pub change: DiffChange,
    pub columns: Vec<SchemaColumnDiff>,
    pub indexes: Vec<SchemaIndexDiff>,
    pub statements: Vec<String>,
}

/// 差异汇总
#[derive(Debug, Clone, Default, Serialize)]
pub struct SchemaDiffSummary {
    pub added_tables: usize,
    pub removed_tables: usize,
    pub changed_tables: usize,
    pub added_columns: usize,
    pub removed_columns: usize,
    pub changed_columns: usize,
    pub added_indexes: usize,
    pub removed_indexes: usize,
    pub changed_indexes: usize,
}

/// Schema 对比结果：source 为基准，target 为待同步；statements 为可直接执行的迁移脚本
#[derive(Debug, Clone, Serialize)]
pub struct SchemaDiff {
    pub source_database: String,
    pub target_database: String,
    pub tables: Vec<SchemaTableDiff>,
    pub statements: Vec<String>,
    pub summary: SchemaDiffSummary,
}

/// 服务端会话/进程（对应 `SHOW FULL PROCESSLIST` 的一行）
#[derive(Debug, Clone, Serialize)]
pub struct ProcessInfo {
    pub id: u64,
    pub user: String,
    pub host: String,
    pub database: Option<String>,
    pub command: String,
    /// 当前状态已持续的秒数
    pub time: u64,
    pub state: Option<String>,
    pub info: Option<String>,
}

/// 服务端运行指标（来自 `SHOW GLOBAL STATUS` / `SHOW GLOBAL VARIABLES`）
#[derive(Debug, Clone, Serialize)]
pub struct ServerMetrics {
    pub version: String,
    pub uptime_seconds: u64,
    /// 自启动以来的平均每秒查询数（Questions / Uptime）
    pub qps: f64,
    pub threads_connected: u64,
    pub threads_running: u64,
    pub max_connections: u64,
    pub slow_queries: u64,
    pub slow_query_log: bool,
    pub long_query_time: f64,
    pub aborted_connects: u64,
    /// InnoDB 缓冲池命中率（百分比，0-100）；缺少相关状态项时为 None
    pub buffer_pool_hit_rate: Option<f64>,
    pub bytes_received: u64,
    pub bytes_sent: u64,
    pub com_select: u64,
    pub com_insert: u64,
    pub com_update: u64,
    pub com_delete: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_config_json() -> &'static str {
        r#"{"id":"c1","name":"local","host":"127.0.0.1","port":3306,
            "database":null,"username":"root","password":"pwd"}"#
    }

    #[test]
    fn ssl_config_defaults_to_preferred_when_absent() {
        // 旧版本前端/旧配置不会带 ssl_config，必须默认 Preferred 且不报错
        let config: ConnectionConfig = serde_json::from_str(minimal_config_json()).unwrap();
        assert_eq!(config.ssl_config.mode, SslMode::Preferred);
        assert!(config.ssl_config.ca_path.is_none());
        assert!(config.ssl_config.cert_path.is_none());
        assert!(config.ssl_config.key_path.is_none());
    }

    #[test]
    fn ssl_mode_uses_kebab_case() {
        let cases = [
            ("disabled", SslMode::Disabled),
            ("preferred", SslMode::Preferred),
            ("required", SslMode::Required),
            ("verify-ca", SslMode::VerifyCa),
            ("verify-identity", SslMode::VerifyIdentity),
        ];
        for (text, expected) in cases {
            let mode: SslMode = serde_json::from_str(&format!("\"{}\"", text)).unwrap();
            assert_eq!(mode, expected);
            assert_eq!(serde_json::to_string(&mode).unwrap(), format!("\"{}\"", text));
        }
    }

    #[test]
    fn ssl_config_accepts_certificate_paths() {
        let json = r#"{"id":"c1","name":"local","host":"db.example.com","port":3306,
            "database":"app","username":"root","password":"pwd",
            "ssl_config":{"mode":"verify-ca","ca_path":"/etc/mysql/ca.pem",
                          "cert_path":"/etc/mysql/client.pem","key_path":"/etc/mysql/client.key"}}"#;
        let config: ConnectionConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.ssl_config.mode, SslMode::VerifyCa);
        assert_eq!(config.ssl_config.ca_path.as_deref(), Some("/etc/mysql/ca.pem"));
        assert_eq!(config.ssl_config.cert_path.as_deref(), Some("/etc/mysql/client.pem"));
        assert_eq!(config.ssl_config.key_path.as_deref(), Some("/etc/mysql/client.key"));
    }

    #[test]
    fn ssh_auth_type_uses_kebab_case() {
        let cases = [
            ("password", SshAuthType::Password),
            ("private-key", SshAuthType::PrivateKey),
            ("agent", SshAuthType::Agent),
        ];
        for (text, expected) in cases {
            let parsed: SshAuthType = serde_json::from_str(&format!("\"{}\"", text)).unwrap();
            assert_eq!(parsed, expected);
            assert_eq!(serde_json::to_string(&parsed).unwrap(), format!("\"{}\"", text));
        }
    }

    #[test]
    fn ssh_config_defaults_port_and_optional_fields() {
        // 前端未填端口/未用到的凭据字段缺省时不报错
        let json = r#"{"host":"jump.example.com","username":"ops","auth_type":"password"}"#;
        let config: SshConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.port, 22);
        assert!(config.password.is_none());
        assert!(config.private_key_path.is_none());
        assert!(config.passphrase.is_none());
    }
}
