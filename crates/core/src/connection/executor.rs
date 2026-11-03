//! SQL 执行模块
//! SQL 执行、表数据查询、取消、导入导出等。

use super::driver::{AnyPool, AnyTransaction, DriverKind};
use super::export::{self, ExportColumn, ExportFormat, TableExportSink};
use super::manager::{ConnectionManager, MetadataCacheValue};
use super::resultset::{
    classify_column, classify_pg_column, convert_pg_value, convert_value, quote_value,
    ColumnKind, PgColumnKind,
};
use super::types::*;
use super::validation::*;
use anyhow::{anyhow, Context, Result};
use crate::sql::{rewrite_table_names, split_sql_statements};
use futures_util::TryStreamExt;
use sqlx::mysql::MySqlPool;
use sqlx::postgres::{PgConnection, PgPool};
use sqlx::{Acquire, Column, Executor, MySql, MySqlConnection, Postgres, Row, Statement};
use sqlparser::ast as sql_ast;
use sqlparser::dialect::MySqlDialect;
use sqlparser::parser::Parser;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Notify};

/// 一次执行所需的连接级共享状态快照。
///
/// 之前 `execute` 在查询全程持有连接表的读锁，慢查询会阻塞 `connect`/`disconnect` 等写操作；
/// 改为在锁内取出 Arc 克隆后立即释放锁，执行期间只持有这些共享句柄。
#[derive(Clone)]
struct ExecState {
    transaction: Arc<Mutex<Option<AnyTransaction>>>,
    /// MySQL 持久会话连接（USE/SET 的落点），懒创建
    session: Arc<Mutex<Option<sqlx::pool::PoolConnection<MySql>>>>,
    query_cancel: Arc<AtomicBool>,
    query_notify: Arc<Notify>,
    /// 是否有查询正在执行（取消时判断是否需要丢弃事务）
    query_running: Arc<AtomicBool>,
    metadata_cache: Arc<Mutex<HashMap<String, (Instant, MetadataCacheValue)>>>,
    /// 建立连接时配置的默认库（PG 用来判断是否需要 SET search_path）
    config: ExecConfigSnapshot,
}

#[derive(Clone)]
struct ExecConfigSnapshot {
    database: Option<String>,
}

/// 流式读取结果集的产物：只保留第一个结果集的前 `MAX_RESULT_ROWS` 行。
struct StreamedRows<R> {
    columns: Vec<ColumnDefinition>,
    rows: Vec<R>,
    total_rows: usize,
    multiple_result_sets: bool,
}

/// 独立查询（表浏览/导出）的运行标志守卫：与 `execute` 共用同一套取消标志，
/// 使前端「取消查询」能中断它；drop 时复位，避免取消请求后续误杀新查询。
struct QueryRunGuard(Arc<AtomicBool>);

impl QueryRunGuard {
    fn start(cancel: &Arc<AtomicBool>, running: &Arc<AtomicBool>) -> Self {
        cancel.store(false, Ordering::SeqCst);
        running.store(true, Ordering::SeqCst);
        Self(running.clone())
    }
}

impl Drop for QueryRunGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// 单条 SQL 执行超时：交互式查询的宽松上限，超时即丢弃查询 future（连接关闭，服务端随之终止查询）
const MAX_QUERY_DURATION: Duration = Duration::from_secs(300);

/// SELECT 结果集行数上限：超出截断，防止全量进内存与全量 JSON 过 IPC 撑爆前端
const MAX_RESULT_ROWS: usize = 10_000;

/// 整表导出的超时上限：比交互式查询宽松得多，但仍避免无限期挂住
const MAX_EXPORT_DURATION: Duration = Duration::from_secs(1800);

/// 执行带超时与取消语义的查询。
///
/// 取消/超时即丢弃查询 future：该物理连接协议流未完成，sqlx 将其关闭而非归还连接池，
/// MySQL 服务端检测到客户端断开后会终止该查询，因此无需额外发起 KILL QUERY。
async fn run_query<R, F>(fut: F, cancel: &AtomicBool, notify: &Notify) -> Result<R>
where
    F: std::future::Future<Output = std::result::Result<R, sqlx::Error>>,
{
    run_query_within(fut, cancel, notify, MAX_QUERY_DURATION).await
}

/// 同 `run_query`，但可指定超时时长（整表导出等长任务用）。
async fn run_query_within<R, F>(
    fut: F,
    cancel: &AtomicBool,
    notify: &Notify,
    limit: Duration,
) -> Result<R>
where
    F: std::future::Future<Output = std::result::Result<R, sqlx::Error>>,
{
    // 本次执行开始前已有取消请求（先于执行到达）时立即拒绝
    if cancel.load(Ordering::SeqCst) {
        return Err(anyhow!("查询已被用户取消"));
    }
    tokio::pin!(fut);
    let outcome = tokio::time::timeout(limit, async {
        loop {
            if cancel.load(Ordering::SeqCst) {
                return None;
            }
            let notified = notify.notified();
            tokio::select! {
                r = fut.as_mut() => return Some(r),
                // notified 可能错过通知，下一轮循环由 cancel 标志兜住
                _ = notified => {}
            }
        }
    })
    .await;

    match outcome {
        Err(_) => Err(anyhow!(
            "查询执行超过 {} 秒，已终止；请优化 SQL 或缩小查询范围",
            limit.as_secs()
        )),
        Ok(None) => Err(anyhow!("查询已被用户取消")),
        Ok(Some(Ok(r))) => Ok(r),
        Ok(Some(Err(e))) => Err(e.into()),
    }
}

/// 由结果集首行构造列定义（列名 + 类型）。
fn columns_from_mysql_row(row: &sqlx::mysql::MySqlRow) -> Vec<ColumnDefinition> {
    row.columns()
        .iter()
        .map(|col| ColumnDefinition {
            name: col.name().to_string(),
            data_type: col.type_info().to_string(),
            is_nullable: true,
            is_primary: false,
            is_auto_increment: false,
        })
        .collect()
}

/// 判断一行是否属于第一个结果集（列数与列名都一致）。
fn mysql_row_matches_columns(row: &sqlx::mysql::MySqlRow, columns: &[ColumnDefinition]) -> bool {
    let cols = row.columns();
    cols.len() == columns.len()
        && cols
            .iter()
            .zip(columns.iter())
            .all(|(c, def)| c.name() == def.name)
}

/// 流式收集 MySQL 结果集。
///
/// 只保留第一个结果集的前 `MAX_RESULT_ROWS` 行，其余行继续消费但不再入内存：
/// 提前 drop 流会让 sqlx 关闭该物理连接（可能连带终止用户事务），因此必须读完。
async fn collect_mysql_rows<S>(
    mut stream: S,
) -> std::result::Result<StreamedRows<sqlx::mysql::MySqlRow>, sqlx::Error>
where
    S: futures_util::Stream<
            Item = std::result::Result<
                sqlx::Either<sqlx::mysql::MySqlQueryResult, sqlx::mysql::MySqlRow>,
                sqlx::Error,
            >,
        > + Unpin,
{
    let mut out = StreamedRows {
        columns: Vec::new(),
        rows: Vec::new(),
        total_rows: 0,
        multiple_result_sets: false,
    };

    while let Some(item) = stream.try_next().await? {
        let row = match item {
            sqlx::Either::Right(row) => row,
            // 非结果集语句（DML 的 QueryResult）与 SELECT 混发时忽略其影响行数
            sqlx::Either::Left(_) => continue,
        };

        if out.columns.is_empty() {
            out.columns = columns_from_mysql_row(&row);
        }
        if !mysql_row_matches_columns(&row, &out.columns) {
            out.multiple_result_sets = true;
            continue;
        }
        out.total_rows += 1;
        if out.rows.len() < MAX_RESULT_ROWS {
            out.rows.push(row);
        }
    }

    Ok(out)
}

/// 由结果集首行构造 PG 列定义（列名 + 类型）。
fn columns_from_pg_row(row: &sqlx::postgres::PgRow) -> Vec<ColumnDefinition> {
    row.columns()
        .iter()
        .map(|col| ColumnDefinition {
            name: col.name().to_string(),
            data_type: col.type_info().to_string(),
            is_nullable: true,
            is_primary: false,
            is_auto_increment: false,
        })
        .collect()
}

/// 判断一行是否属于第一个结果集（列数与列名都一致）。
fn pg_row_matches_columns(row: &sqlx::postgres::PgRow, columns: &[ColumnDefinition]) -> bool {
    let cols = row.columns();
    cols.len() == columns.len()
        && cols
            .iter()
            .zip(columns.iter())
            .all(|(c, def)| c.name() == def.name)
}

/// 流式收集 PostgreSQL 结果集，语义同 `collect_mysql_rows`。
async fn collect_pg_rows<S>(
    mut stream: S,
) -> std::result::Result<StreamedRows<sqlx::postgres::PgRow>, sqlx::Error>
where
    S: futures_util::Stream<
            Item = std::result::Result<
                sqlx::Either<sqlx::postgres::PgQueryResult, sqlx::postgres::PgRow>,
                sqlx::Error,
            >,
        > + Unpin,
{
    let mut out = StreamedRows {
        columns: Vec::new(),
        rows: Vec::new(),
        total_rows: 0,
        multiple_result_sets: false,
    };

    while let Some(item) = stream.try_next().await? {
        let row = match item {
            sqlx::Either::Right(row) => row,
            sqlx::Either::Left(_) => continue,
        };

        if out.columns.is_empty() {
            out.columns = columns_from_pg_row(&row);
        }
        if !pg_row_matches_columns(&row, &out.columns) {
            out.multiple_result_sets = true;
            continue;
        }
        out.total_rows += 1;
        if out.rows.len() < MAX_RESULT_ROWS {
            out.rows.push(row);
        }
    }

    Ok(out)
}

/// 校验目标表存在主键。
///
/// 无主键表上 UPDATE / DELETE 只能靠列值定位行，可能命中多行；
/// 前端的只读限制属于 UI 约束，直接调用命令即可绕过，因此在这里做服务端兜底。
async fn ensure_table_has_primary_key(
    txn: &mut MySqlConnection,
    database: &str,
    table: &str,
    position: usize,
) -> Result<()> {
    let has_primary_key = sqlx::query_scalar::<_, String>(
        "SELECT COLUMN_NAME FROM INFORMATION_SCHEMA.STATISTICS \
         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND INDEX_NAME = 'PRIMARY' LIMIT 1",
    )
    .bind(database)
    .bind(table)
    .fetch_optional(&mut *txn)
    .await
    .map_err(|e| {
        anyhow!(
            "第 {} 条变更失败：检查表主键时出错：{}",
            position,
            crate::error::translate(&e)
        )
    })?
    .is_some();

    if !has_primary_key {
        return Err(anyhow!(
            "第 {} 条变更失败：表 `{}` 没有主键，无法安全定位行，已拒绝修改",
            position,
            table
        ));
    }

    Ok(())
}

/// 在事务连接上依次执行表数据变更，返回「UPDATE 值未变化」的条数。
///
/// 统一用 `Executor::execute(conn, &str)`（文本协议，与 `raw_sql` 等价）：
/// `sqlx::raw_sql` 的 `RawSql` future 在经由命令的 async 链转交时，
/// 会触发 rustc 的「`Send`/`Executor` 实现不够通用」错误。
async fn apply_changes_in_txn(
    txn: &mut MySqlConnection,
    database: &str,
    changes: &[TableChange],
    safety_mode: SafetyMode,
) -> Result<usize> {
    // 前端生成的语句使用不带库名的表名，统一在事务连接上切换库
    let use_sql = format!("USE `{}`", database);
    Executor::execute(&mut *txn, use_sql.as_str())
        .await
        .map_err(|e| anyhow!("切换数据库失败：{}", crate::error::translate(&e)))?;

    let mut unchanged = 0usize;
    // 同一次保存中的表名去重，避免每行变更都查一次元数据
    let mut checked_tables: HashSet<String> = HashSet::new();
    for (index, change) in changes.iter().enumerate() {
        let position = index + 1;
        validate_identifier(&change.table)
            .map_err(|e| anyhow!("第 {} 条变更的表名不合法：{}", position, e))?;

        // 变更 SQL 来自前端，必须逐条校验后才能执行
        validate_table_change(change, position, safety_mode)?;

        // 在任何变更执行前完成主键校验，无主键则整体失败并回滚
        if checked_tables.insert(change.table.clone()) {
            ensure_table_has_primary_key(txn, database, &change.table, position).await?;
        }

        let affected = Executor::execute(&mut *txn, change.sql.as_str())
            .await
            .map_err(|e| {
                anyhow!(
                    "第 {} 条变更失败：{}",
                    position,
                    crate::error::translate(&e)
                )
            })?
            .rows_affected();

        let problem = match change.kind {
            TableChangeKind::Insert => (affected != 1)
                .then(|| format!("新增未影响任何行（影响 {} 行）", affected)),
            TableChangeKind::Delete => match affected {
                1 => None,
                0 => Some("目标行已不存在，可能已被其他会话删除".to_string()),
                n => Some(format!("删除条件命中 {} 行，已中止（定位条件不安全）", n)),
            },
            TableChangeKind::Update => {
                if affected == 1 {
                    None
                } else if affected > 1 {
                    Some(format!("更新条件命中 {} 行，已中止（定位条件不安全）", affected))
                } else {
                    match &change.where_clause {
                        None => Some("更新未影响任何行".to_string()),
                        Some(where_clause) => {
                            let exists_sql = format!(
                                "SELECT 1 FROM {} WHERE {} LIMIT 1",
                                quote_identifier(&change.table)?,
                                where_clause
                            );
                            match sqlx::query_scalar::<_, i32>(&exists_sql)
                                .fetch_optional(&mut *txn)
                                .await
                            {
                                // 行仍在：值与原值相同，MySQL 按 changed rows 返回 0
                                Ok(Some(_)) => {
                                    unchanged += 1;
                                    None
                                }
                                Ok(None) => Some("目标行已不存在，可能已被其他会话删除".to_string()),
                                Err(e) => Some(format!(
                                    "确认目标行是否仍存在时失败：{}",
                                    crate::error::translate(&e)
                                )),
                            }
                        }
                    }
                }
            }
        };

        if let Some(message) = problem {
            return Err(anyhow!("第 {} 条变更失败：{}", position, message));
        }
    }

    Ok(unchanged)
}

/// 校验前端提交的单条表数据变更。
///
/// 变更 SQL 由前端拼接后直接执行，属于信任边界，必须保证：
/// 1. 只有一条语句（多语句会借助 `CLIENT_MULTI_STATEMENTS` 执行未声明的 SQL）；
/// 2. 语句类型与声明的操作一致；
/// 3. 通过连接访问模式的安全校验（安全模式拦截无 WHERE 的 UPDATE/DELETE）；
/// 4. 语句只引用声明的表；
/// 5. `where_clause` 是合法的条件表达式，不能借 `;` / `UNION` 改变语句结构。
fn validate_table_change(
    change: &TableChange,
    position: usize,
    safety_mode: SafetyMode,
) -> Result<()> {
    let statements = split_sql_statements(&change.sql);
    if statements.len() != 1 {
        return Err(anyhow!(
            "第 {} 条变更必须且只能包含一条 SQL 语句",
            position
        ));
    }
    let statement = statements[0].trim();
    if statement.is_empty() {
        return Err(anyhow!("第 {} 条变更的 SQL 为空", position));
    }

    let expected = match change.kind {
        TableChangeKind::Insert => "INSERT",
        TableChangeKind::Update => "UPDATE",
        TableChangeKind::Delete => "DELETE",
    };

    crate::sql::safety::enforce(safety_mode, statement)
        .map_err(|e| anyhow!("第 {} 条变更未通过安全校验：{}", position, e))?;

    // 目标表必须从 AST 取：文本匹配会被注释、字符串字面量、JOIN 的其它表绕过
    let target = change_target_table(statement, change.kind)
        .map_err(|e| anyhow!("第 {} 条变更不合法（期望 {}）：{}", position, expected, e))?;
    if !target.eq_ignore_ascii_case(&change.table) {
        return Err(anyhow!(
            "第 {} 条变更的 SQL 与目标表 {} 不一致（实际写入 {}）",
            position,
            change.table,
            target
        ));
    }

    if let Some(where_clause) = &change.where_clause {
        let quoted = quote_identifier(&change.table)?;
        crate::sql::safety::validate_where_clause(&quoted, where_clause)
            .map_err(|e| anyhow!("第 {} 条变更的定位条件不合法：{}", position, e))?;
    }

    Ok(())
}

/// 解析变更语句并取出实际写入的表名（末段），同时校验语句形态与操作类型一致。
///
/// 只接受单表、无 JOIN 的简单 DML：多表 UPDATE/DELETE 无法确认影响范围，
/// 与「一次变更只改一行」的模型不符，直接拒绝。
fn change_target_table(statement: &str, kind: TableChangeKind) -> Result<String> {
    let mut statements = Parser::parse_sql(&MySqlDialect {}, statement)
        .map_err(|e| anyhow!("SQL 无法解析：{}", e))?;
    if statements.len() != 1 {
        return Err(anyhow!("必须且只能包含一条 SQL 语句"));
    }
    let parsed = statements.remove(0);

    match (kind, parsed) {
        (TableChangeKind::Insert, sql_ast::Statement::Insert(insert)) => match insert.table {
            sql_ast::TableObject::TableName(name) => object_name_tail(&name),
            _ => Err(anyhow!("INSERT 的目标不是普通表")),
        },
        (TableChangeKind::Update, sql_ast::Statement::Update(update)) => {
            if update.from.is_some() || !update.table.joins.is_empty() {
                return Err(anyhow!("UPDATE 不能包含 JOIN 或 FROM 子句"));
            }
            table_factor_name(&update.table.relation)
        }
        (TableChangeKind::Delete, sql_ast::Statement::Delete(delete)) => {
            if !delete.tables.is_empty() || delete.using.is_some() {
                return Err(anyhow!("DELETE 不能作用于多表或使用 USING"));
            }
            let tables = match &delete.from {
                sql_ast::FromTable::WithFromKeyword(tables)
                | sql_ast::FromTable::WithoutKeyword(tables) => tables,
            };
            if tables.len() != 1 || !tables[0].joins.is_empty() {
                return Err(anyhow!("DELETE 只能作用于单表且不能包含 JOIN"));
            }
            table_factor_name(&tables[0].relation)
        }
        (TableChangeKind::Insert, _) => Err(anyhow!("SQL 与操作类型不匹配")),
        (TableChangeKind::Update, _) => Err(anyhow!("SQL 与操作类型不匹配")),
        (TableChangeKind::Delete, _) => Err(anyhow!("SQL 与操作类型不匹配")),
    }
}

/// 取 `schema.table` 形式的对象名末段（表名本身）
fn object_name_tail(name: &sql_ast::ObjectName) -> Result<String> {
    match name.0.last() {
        Some(sql_ast::ObjectNamePart::Identifier(ident)) => Ok(ident.value.clone()),
        _ => Err(anyhow!("无法识别表名")),
    }
}

/// 从表因子中取出表名；表函数（`users()`）等非常规目标一律拒绝
fn table_factor_name(factor: &sql_ast::TableFactor) -> Result<String> {
    match factor {
        sql_ast::TableFactor::Table { name, args, .. } if args.is_none() => object_name_tail(name),
        _ => Err(anyhow!("变更的目标不是普通表")),
    }
}

impl ConnectionManager {
    /// 执行SQL
    pub async fn execute(&self, connection_id: &str, database: &str, sql: &str) -> Result<QueryResult> {
        // 验证 SQL 长度限制（防止内存攻击）
        const MAX_SQL_LENGTH: usize = 10_000_000; // 10MB
        if sql.len() > MAX_SQL_LENGTH {
            return Err(anyhow!("SQL 长度不能超过 10MB"));
        }

        // 只在取连接信息期间持有读锁：慢查询不能阻塞 connect/disconnect 等写操作
        let (pool, state) = {
            let connections = self.connections.read().await;

            let conn = connections
                .get(connection_id)
                .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;

            // 输入验证：PostgreSQL 下空库名表示「沿用服务端 search_path」，需按驱动放行；
            // MySQL 的库名必填（空串会让后续限定名重写失去上下文）
            let allow_empty_database =
                conn.config.driver == DriverKind::Postgres && database.is_empty();
            if !allow_empty_database {
                validate_identifier(database)
                    .map_err(|e| anyhow!("数据库名验证失败: {}", e))?;
            }

            // 访问模式守卫：只读模式拒绝非查询语句，安全模式拒绝不带 WHERE 的 UPDATE/DELETE
            crate::sql::safety::enforce(conn.config.safety_mode, sql)?;

            // 池的克隆只是 Arc 计数增加，用于把具体驱动类型交给对应执行路径
            let pool = conn
                .pool
                .as_ref()
                .ok_or_else(|| anyhow!("连接不可用: {}", connection_id))?
                .clone();

            // 复位上一次的取消标志，确保新查询不被旧取消信号误杀
            conn.query_cancel.store(false, Ordering::SeqCst);
            conn.query_running.store(true, Ordering::SeqCst);

            let state = ExecState {
                transaction: conn.transaction.clone(),
                session: conn.session.clone(),
                query_cancel: conn.query_cancel.clone(),
                query_notify: conn.query_notify.clone(),
                query_running: conn.query_running.clone(),
                metadata_cache: conn.metadata_cache.clone(),
                config: ExecConfigSnapshot {
                    database: conn.config.database.clone(),
                },
            };
            (pool, state)
        };

        let outcome = match pool {
            AnyPool::MySql(pool) => self.execute_mysql(&state, &pool, connection_id, database, sql).await,
            AnyPool::Postgres(pool) => self.execute_postgres(&state, &pool, connection_id, database, sql).await,
        };
        // 无论成功/失败/取消都要复位，否则取消请求会一直认为有在途查询
        state.query_running.store(false, Ordering::SeqCst);
        let result = outcome?;

        // 结构变更成功后立刻失效元数据缓存，否则对象树/表结构会在 TTL（30s）内返回旧快照，
        // 表现为建表后刷新列表看不到新表
        if crate::sql::is_schema_changing(sql) {
            state.metadata_cache.lock().await.clear();
        }

        Ok(result)
    }

    /// MySQL 执行路径（原有逻辑）
    async fn execute_mysql(
        &self,
        conn: &ExecState,
        pool: &MySqlPool,
        connection_id: &str,
        database: &str,
        sql: &str,
    ) -> Result<QueryResult> {
        let t_start = Instant::now();

        // 同一次提交内可能先切库再查询（`USE db2; SELECT * FROM t`）。
        // 后续未限定表名必须以「最后切换到的库」为准补前缀，否则会被补上侧栏选中的库，静默查错库。
        let effective_db = match crate::sql::leading_use_database(sql) {
            Some(db) => {
                validate_identifier(&db).map_err(|e| anyhow!("USE 语句中的数据库名不合法: {}", e))?;
                db
            }
            None => database.to_string(),
        };

        // 在 SQL 中为表名添加数据库前缀（替换表名引用为 db.table 格式）。
        // 返回的 bool 表示 sqlparser 是否成功解析：标准 DML/DDL 语句会成功解析，
        // 表名已带数据库前缀，无需再执行 USE；SHOW/DESC/CALL 等 MySQL 特有语法解析失败，需先 USE。
        let (modified_sql, parsed) = rewrite_table_names(&effective_db, sql, DriverKind::MySql);
        let t_rewrite = Instant::now();

        // 按去注释后的真实语句分类：避免 `/* c */ DELETE` 这类前缀注释与
        // `WITH ... INSERT` 的 CTE 前缀被 starts_with 误判。
        let kind = crate::sql::classify_sql(&modified_sql);
        // 会话级语句：必须固定在一条物理连接上执行并保持到连接关闭，否则会话语义丢失
        let is_session_stmt = kind == crate::sql::SqlKind::Session;
        // 判断是否为不支持预处理协议的语句（DDL 类）；会话语句同样走文本协议执行
        let is_ddl = matches!(kind, crate::sql::SqlKind::Ddl | crate::sql::SqlKind::Session);

        // 判断是否为 SHOW/DESC 语句（需要数据库上下文）
        let needs_db_context = kind == crate::sql::SqlKind::NeedsDbContext;

        // 判断是否为 DML 语句：走 execute 拿真实影响行数（此前走 fetch_all 恒返回 0）
        let is_dml = kind == crate::sql::SqlKind::Dml;

        // 选择执行目标，保证「USE 与查询在同一物理连接」：
        // - 事务进行中：钉扎在事务连接上执行；
        // - 需要会话上下文（会话语句、解析失败或 SHOW/DESC）：使用持久会话连接，会话状态跨执行保留；
        // - 其余无状态语句：直接在池上执行（但会话连接一旦建立，也优先复用，保证 SET/USE 生效）。
        let need_session = !parsed || needs_db_context || is_session_stmt;
        let mut txn_guard = conn.transaction.lock().await;
        let mut session_guard = conn.session.lock().await;
        if txn_guard.is_none() && need_session && session_guard.is_none() {
            *session_guard = Some(pool.acquire().await.context("获取数据库连接失败")?);
        }
        // 统一「固定单条物理连接」：事务进行中优先钉扎事务连接，否则为会话连接
        let mut session_conn: Option<&mut MySqlConnection> = match txn_guard.as_mut() {
            Some(txn) => Some(txn.as_mysql_connection()?),
            None => session_guard.as_mut().map(|c| &mut **c),
        };
        // 会话语句自身会切换库，无需预置 USE
        if need_session && !is_session_stmt {
            let use_sql = format!("USE `{}`", effective_db);
            match session_conn.as_mut() {
                Some(c) => { Executor::execute(&mut **c, use_sql.as_str()).await?; }
                None => { sqlx::raw_sql(&use_sql).execute(pool).await?; }
            }
        }
        let t_use = Instant::now();

        // 执行查询（带超时与取消语义）
        if is_ddl {
            // DDL 语句使用 raw SQL 执行（不支持预处理协议）
            let affected = match session_conn.as_mut() {
                Some(c) => {
                    let fut = Executor::execute(&mut **c, modified_sql.as_str());
                    run_query(fut, &conn.query_cancel, &conn.query_notify).await?.rows_affected()
                }
                None => {
                    let fut = sqlx::raw_sql(&modified_sql).execute(pool);
                    run_query(fut, &conn.query_cancel, &conn.query_notify).await?.rows_affected()
                }
            };
            let execution_time_ms = t_start.elapsed().as_millis() as u64;
            tracing::info!(
                connection_id, database, sql_len = sql.len(),
                rewrite_ms = ms(t_rewrite, t_start),
                use_ms = ms(t_use, t_rewrite),
                exec_ms = ms(Instant::now(), t_use),
                total_ms = execution_time_ms,
                "query timing (DDL)"
            );
            return Ok(QueryResult {
                columns: vec![],
                rows: vec![],
                execution_time_ms,
                rows_affected: Some(affected),
                rows_returned: 0,
                truncated: false,
                warning: None,
            });
        }

        // DML 语句：文本协议 execute 返回真实影响行数
        if is_dml {
            let affected = match session_conn.as_mut() {
                Some(c) => {
                    let fut = Executor::execute(&mut **c, modified_sql.as_str());
                    run_query(fut, &conn.query_cancel, &conn.query_notify).await?.rows_affected()
                }
                None => {
                    let fut = sqlx::raw_sql(&modified_sql).execute(pool);
                    run_query(fut, &conn.query_cancel, &conn.query_notify).await?.rows_affected()
                }
            };
            let execution_time_ms = t_start.elapsed().as_millis() as u64;
            tracing::info!(
                connection_id, database, sql_len = sql.len(),
                rewrite_ms = ms(t_rewrite, t_start),
                use_ms = ms(t_use, t_rewrite),
                exec_ms = ms(Instant::now(), t_use),
                total_ms = execution_time_ms,
                "query timing (DML)"
            );
            return Ok(QueryResult {
                columns: vec![],
                rows: vec![],
                execution_time_ms,
                rows_affected: Some(affected),
                rows_returned: 0,
                truncated: false,
                warning: None,
            });
        }

        // SELECT 等需要结果集的语句：用 fetch_many 流式读取（文本协议，避免额外的 COM_STMT_PREPARE 往返），
        // 只保留前 MAX_RESULT_ROWS 行，避免大结果集先全量入内存再截断。
        let streamed = match session_conn.as_mut() {
            Some(c) => {
                let stream = Executor::fetch_many(&mut **c, modified_sql.as_str());
                run_query(collect_mysql_rows(stream), &conn.query_cancel, &conn.query_notify).await?
            }
            None => {
                let stream = Executor::fetch_many(pool, modified_sql.as_str());
                run_query(collect_mysql_rows(stream), &conn.query_cancel, &conn.query_notify).await?
            }
        };
        let t_fetch = Instant::now();

        let execution_time_ms = t_start.elapsed().as_millis() as u64;

        if streamed.total_rows == 0 {
            // 0 行时文本协议无列元数据；解析成功的 SELECT 类语句通过 describe（仅 prepare、不执行）补列头。
            // SHOW/DESC 无法 prepare，保持空列头（登记为遗留）
            let columns: Vec<ColumnDefinition> = if parsed {
                let describe_result = match session_conn.as_mut() {
                    Some(c) => {
                        let fut = Executor::describe(&mut **c, modified_sql.as_str());
                        run_query(fut, &conn.query_cancel, &conn.query_notify).await
                    }
                    None => {
                        let fut = Executor::describe(pool, modified_sql.as_str());
                        run_query(fut, &conn.query_cancel, &conn.query_notify).await
                    }
                };
                describe_result
                    .map(|d| {
                        d.columns
                            .iter()
                            .map(|col| ColumnDefinition {
                                name: col.name().to_string(),
                                data_type: col.type_info().to_string(),
                                is_nullable: true,
                                is_primary: false,
                                is_auto_increment: false,
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                vec![]
            };
            tracing::info!(
                connection_id, database, sql_len = sql.len(),
                rewrite_ms = ms(t_rewrite, t_start),
                use_ms = ms(t_use, t_rewrite),
                fetch_ms = ms(t_fetch, t_use),
                total_ms = execution_time_ms,
                "query timing (empty result)"
            );
            return Ok(QueryResult {
                columns,
                rows: vec![],
                execution_time_ms,
                rows_affected: None,
                rows_returned: 0,
                truncated: false,
                warning: None,
            });
        }

        // 获取列信息
        let columns: Vec<ColumnDefinition> = streamed.columns;
        let t_columns = Instant::now();

        // 列级预分类：每列只做一次类型匹配，避免在逐单元格转换时重复小写化与字符串比较
        let kinds: Vec<ColumnKind> = columns.iter().map(|c| classify_column(&c.data_type)).collect();

        // 多语句（`SELECT 1; SELECT 2;`）会返回多个结果集，
        // `collect_mysql_rows` 已只保留与第一个结果集列结构一致的行，其余仅标记。
        let multiple_result_sets = streamed.multiple_result_sets;

        // 转换数据（流式收集时已按上限截断，避免全量进内存/全量 JSON 过 IPC）
        let total_rows = streamed.total_rows;
        let truncated = total_rows > MAX_RESULT_ROWS;
        let mut rows = vec![];
        for row in streamed.rows.iter() {
            let mut row_data = vec![];
            for (i, _col) in columns.iter().enumerate() {
                let value = convert_value(row, i, kinds[i])?;
                row_data.push(value);
            }
            rows.push(row_data);
        }
        let t_convert = Instant::now();

        tracing::info!(
            connection_id, database, sql_len = sql.len(),
            rows = rows.len(), total_rows, cols = columns.len(),
            multiple_result_sets,
            rewrite_ms = ms(t_rewrite, t_start),
            use_ms = ms(t_use, t_rewrite),
            fetch_ms = ms(t_fetch, t_use),
            columns_ms = ms(t_columns, t_fetch),
            convert_ms = ms(t_convert, t_columns),
            total_ms = execution_time_ms,
            "query timing"
        );

        let warning = if multiple_result_sets {
            Some("检测到多个结果集，仅显示第一个；请分批执行各条语句".to_string())
        } else {
            truncated.then(|| {
                format!(
                    "结果集共 {} 行，仅显示前 {} 行；请用 LIMIT 或过滤条件缩小范围",
                    total_rows, MAX_RESULT_ROWS
                )
            })
        };

        Ok(QueryResult {
            columns,
            rows_returned: rows.len(),
            rows,
            execution_time_ms,
            rows_affected: None,
            truncated,
            warning,
        })
    }

    /// PostgreSQL 执行路径
    ///
    /// 与 MySQL 路径的差异：
    /// - 不做库名前缀重写（PG 的 database 与 schema 是两层）；
    /// - `SET search_path` 是会话级设置，必须与查询跑在同一条物理连接上，因此始终钉扎一条连接；
    /// - 空结果集没有列元数据，而 sqlx-postgres 未实现 `Describe for &str`，改用 `prepare`
    ///   （只解析、不执行）取列头。
    async fn execute_postgres(
        &self,
        conn: &ExecState,
        pool: &PgPool,
        connection_id: &str,
        database: &str,
        sql: &str,
    ) -> Result<QueryResult> {
        let t_start = Instant::now();
        let (modified_sql, parsed) = rewrite_table_names(database, sql, DriverKind::Postgres);
        let t_rewrite = Instant::now();

        let upper_sql = modified_sql.trim().to_uppercase();
        let is_ddl = upper_sql.starts_with("ALTER ")
            || upper_sql.starts_with("CREATE ")
            || upper_sql.starts_with("DROP ")
            || upper_sql.starts_with("TRUNCATE")
            || upper_sql.starts_with("GRANT")
            || upper_sql.starts_with("REVOKE")
            || upper_sql.starts_with("COMMENT ")
            || upper_sql.starts_with("VACUUM")
            || upper_sql.starts_with("ANALYZE")
            || upper_sql.starts_with("SET ")
            || upper_sql.starts_with("RESET ");

        // INSERT/UPDATE/DELETE 走 execute 取真实影响行数；带 RETURNING 的语句仍要取结果集
        let is_dml = upper_sql.starts_with("INSERT")
            || upper_sql.starts_with("UPDATE")
            || upper_sql.starts_with("DELETE");
        let returns_rows = is_dml && upper_sql.contains("RETURNING");

        // search_path 是会话级设置：事务进行中复用事务连接，否则从池中钉扎一条连接承载 SET + 查询
        let mut txn_guard = conn.transaction.lock().await;
        let mut acquired: Option<sqlx::pool::PoolConnection<Postgres>> = None;
        if txn_guard.is_none() {
            acquired = Some(pool.acquire().await.context("获取数据库连接失败")?);
        }
        let pg_conn: &mut PgConnection = match txn_guard.as_mut() {
            Some(txn) => txn.as_pg_connection()?,
            None => {
                // 上面在 txn_guard 为空时一定 acquire 成功，这里不用 expect 触发 panic
                &mut **acquired
                    .as_mut()
                    .ok_or_else(|| anyhow!("获取数据库连接失败"))?
            }
        };

        // PG 的 database 在连接时已固定、无法切换；`database` 参数取自侧栏选择，
        // 只有它与连接库不同时才按 schema 处理并设置 search_path，
        // 否则会把库名当成 schema 覆盖掉服务端的默认 search_path。
        // 空串表示「未选择库」（SQL 编辑器在 PG 下的默认值），保持服务端默认 search_path。
        if !database.is_empty() && conn.config.database.as_deref() != Some(database) {
            let set_sql = DriverKind::Postgres.switch_database_sql(database);
            Executor::execute(&mut *pg_conn, set_sql.as_str())
                .await
                .map_err(|e| anyhow!("切换模式失败：{}", crate::error::translate(&e)))?;
        }
        let t_use = Instant::now();

        // DDL 与不带 RETURNING 的 DML：不需要结果集，直接取影响行数
        if is_ddl || (is_dml && !returns_rows) {
            let fut = Executor::execute(&mut *pg_conn, modified_sql.as_str());
            let affected = run_query(fut, &conn.query_cancel, &conn.query_notify)
                .await?
                .rows_affected();
            let execution_time_ms = t_start.elapsed().as_millis() as u64;
            tracing::info!(
                connection_id, database, sql_len = sql.len(),
                kind = if is_ddl { "DDL" } else { "DML" },
                rewrite_ms = ms(t_rewrite, t_start),
                use_ms = ms(t_use, t_rewrite),
                exec_ms = ms(Instant::now(), t_use),
                total_ms = execution_time_ms,
                "query timing"
            );
            return Ok(QueryResult {
                columns: vec![],
                rows: vec![],
                execution_time_ms,
                rows_affected: Some(affected),
                rows_returned: 0,
                truncated: false,
                warning: None,
            });
        }

        // 流式读取结果集，只保留前 MAX_RESULT_ROWS 行（语义同 MySQL 路径）
        let streamed = {
            let stream = Executor::fetch_many(&mut *pg_conn, modified_sql.as_str());
            run_query(collect_pg_rows(stream), &conn.query_cancel, &conn.query_notify).await?
        };
        let t_fetch = Instant::now();

        let execution_time_ms = t_start.elapsed().as_millis() as u64;

        if streamed.total_rows == 0 {
            // 0 行时文本协议无列元数据，用 prepare 补列头；无法解析/无法 prepare 的语句保持空列头
            let columns: Vec<ColumnDefinition> = if parsed {
                let prepared = {
                    let fut = Executor::prepare(&mut *pg_conn, modified_sql.as_str());
                    run_query(fut, &conn.query_cancel, &conn.query_notify).await
                };
                prepared
                    .map(|stmt| {
                        stmt.columns()
                            .iter()
                            .map(|col| ColumnDefinition {
                                name: col.name().to_string(),
                                data_type: col.type_info().to_string(),
                                is_nullable: true,
                                is_primary: false,
                                is_auto_increment: false,
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                vec![]
            };
            tracing::info!(
                connection_id, database, sql_len = sql.len(),
                rewrite_ms = ms(t_rewrite, t_start),
                use_ms = ms(t_use, t_rewrite),
                fetch_ms = ms(t_fetch, t_use),
                total_ms = execution_time_ms,
                "query timing (empty result)"
            );
            return Ok(QueryResult {
                columns,
                rows: vec![],
                execution_time_ms,
                rows_affected: None,
                rows_returned: 0,
                truncated: false,
                warning: None,
            });
        }

        let columns: Vec<ColumnDefinition> = streamed.columns;
        let t_columns = Instant::now();

        let kinds: Vec<PgColumnKind> = columns.iter().map(|c| classify_pg_column(&c.data_type)).collect();

        let total_rows = streamed.total_rows;
        let truncated = total_rows > MAX_RESULT_ROWS;
        let mut rows = vec![];
        for row in streamed.rows.iter() {
            let mut row_data = vec![];
            for (i, _col) in columns.iter().enumerate() {
                row_data.push(convert_pg_value(row, i, kinds[i])?);
            }
            rows.push(row_data);
        }
        let t_convert = Instant::now();

        tracing::info!(
            connection_id, database, sql_len = sql.len(),
            rows = rows.len(), total_rows, cols = columns.len(),
            rewrite_ms = ms(t_rewrite, t_start),
            use_ms = ms(t_use, t_rewrite),
            fetch_ms = ms(t_fetch, t_use),
            columns_ms = ms(t_columns, t_fetch),
            convert_ms = ms(t_convert, t_columns),
            total_ms = execution_time_ms,
            "query timing"
        );

        Ok(QueryResult {
            columns,
            rows_returned: rows.len(),
            rows,
            execution_time_ms,
            rows_affected: None,
            truncated,
            warning: truncated.then(|| {
                format!(
                    "结果集共 {} 行，仅显示前 {} 行；请用 LIMIT 或过滤条件缩小范围",
                    total_rows, MAX_RESULT_ROWS
                )
            }),
        })
    }

    /// 在一个事务中批量执行表数据变更（UPDATE/INSERT/DELETE）。
    ///
    /// 任一条失败即整体回滚，错误信息带「第 N 条」，避免半写。
    /// 逐条校验影响行数：
    /// - INSERT 必须影响 1 行；
    /// - DELETE 必须影响 1 行（0 行说明目标行已不存在，>1 行说明定位条件不安全）；
    /// - UPDATE 影响 0 行时用 `SELECT 1 … LIMIT 1` 二次确认：行仍在则视为「值未变化」记录到
    ///   `unchanged`，行已不在则报错。sqlx 0.7 会启用 CLIENT_FOUND_ROWS（`rows_affected` 为
    ///   matched rows），值未变化的 UPDATE 通常返回 1；此分支用于兼容按 changed rows 计数的驱动行为。
    pub async fn apply_changes(
        &self,
        connection_id: &str,
        database: &str,
        changes: Vec<TableChange>,
    ) -> Result<ApplyChangesResult> {
        validate_identifier(database)
            .map_err(|e| anyhow!("数据库名验证失败: {}", e))?;

        // 每次保存的语句条数上限：与结果集行数上限一致，防止一次 IPC 提交过大批次
        const MAX_CHANGES: usize = 10_000;
        if changes.len() > MAX_CHANGES {
            return Err(anyhow!("单次保存的变更不能超过 {} 条", MAX_CHANGES));
        }

        // 只在读取连接信息时持锁，事务执行期间不再阻塞连接表的写操作
        let (pool, transaction, safety_mode) = {
            let connections = self.connections.read().await;
            let conn = connections
                .get(connection_id)
                .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;

            // 只读连接不允许通过表数据编辑面板写入
            if conn.config.safety_mode == SafetyMode::ReadOnly {
                return Err(anyhow!("当前连接为只读模式，无法保存表数据变更"));
            }

            let pool = conn
                .pool
                .as_ref()
                .ok_or_else(|| anyhow!("连接不可用: {}", connection_id))?
                .as_mysql("保存表数据变更")?
                .clone();
            (
                pool,
                conn.transaction.clone(),
                conn.config.safety_mode,
            )
        };

        if changes.is_empty() {
            return Ok(ApplyChangesResult { applied: 0, unchanged: 0 });
        }

        // 先取事务连接再拿事务锁：避免持锁等待池连接，与并发查询互相阻塞
        let mut txn = pool.begin().await.context("开始保存事务失败")?;
        let txn_guard = transaction.lock().await;
        if txn_guard.is_some() {
            // 用户已手动开启事务：两套事务并发修改同一批行会互相锁住，直接拒绝
            let _ = txn.rollback().await;
            return Err(anyhow!("当前连接有未提交的事务，请先 COMMIT 或 ROLLBACK 再保存"));
        }

        let unchanged = match apply_changes_in_txn(&mut txn, database, &changes, safety_mode).await {
            Ok(count) => count,
            Err(e) => {
                let _ = txn.rollback().await;
                return Err(e);
            }
        };

        if let Err(e) = txn.commit().await {
            return Err(anyhow!("提交保存事务失败：{}", crate::error::translate(&e)));
        }
        drop(txn_guard);

        Ok(ApplyChangesResult {
            applied: changes.len(),
            unchanged,
        })
    }

    /// 取出连接级的取消/超时句柄，供 `execute` 之外的独立查询（表浏览、导出）共用，
    /// 让前端「取消查询」与统一的超时兜底对这些查询同样生效。
    async fn exec_handles(
        &self,
        connection_id: &str,
    ) -> Result<(Arc<AtomicBool>, Arc<Notify>, Arc<AtomicBool>)> {
        let connections = self.connections.read().await;
        let conn = connections
            .get(connection_id)
            .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;
        Ok((
            conn.query_cancel.clone(),
            conn.query_notify.clone(),
            conn.query_running.clone(),
        ))
    }

    /// 查询表数据
    pub async fn query_table(
        &self,
        connection_id: &str,
        database: &str,
        table: &str,
        request: DataQueryRequest,
    ) -> Result<DataQueryResponse> {
        // 输入验证
        validate_identifier(database)
            .map_err(|e| anyhow!("数据库名验证失败: {}", e))?;
        validate_identifier(table)
            .map_err(|e| anyhow!("表名验证失败: {}", e))?;
        validate_pagination(request.page, request.page_size)
            .map_err(|e| anyhow!("分页参数验证失败: {}", e))?;

        // 验证排序列
        if let Some(ref col) = request.sort_column {
            validate_identifier(col)
                .map_err(|e| anyhow!("排序列验证失败: {}", e))?;
        }

        // 验证过滤条件
        if let Some(ref filter) = request.filter {
            validate_identifier(&filter.column)
                .map_err(|e| anyhow!("过滤列验证失败: {}", e))?;
            validate_value(&filter.value)
                .map_err(|e| anyhow!("过滤值验证失败: {}", e))?;
        }

        // 表浏览的分页查询与 COUNT 同样受超时/取消约束（大表 COUNT 可能很慢）
        let (query_cancel, query_notify, query_running) = self.exec_handles(connection_id).await?;
        let _run_guard = QueryRunGuard::start(&query_cancel, &query_running);

        // PG 的数据库在连接时固定，浏览非默认库需切到该库的连接池；
        // 树中非 public schema 的对象以 schema.table 展示，这里拆分出真实 schema 与表名
        if self.driver_of(connection_id).await? == DriverKind::Postgres {
            let pool = self.pg_pool_for(connection_id, database).await?;
            let (schema, table_name) = Self::split_pg_table_ref(table);
            return Self::query_table_postgres(
                &pool,
                &schema,
                &table_name,
                &request,
                &query_cancel,
                &query_notify,
            )
            .await;
        }

        // 只取池的克隆后立刻释放连接表读锁，避免分页查询期间阻塞连接管理
        let pool = {
            let connections = self.connections.read().await;

            let conn = connections
                .get(connection_id)
                .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;

            conn.pool
                .as_ref()
                .ok_or_else(|| anyhow!("连接不可用: {}", connection_id))?
                .as_mysql("浏览表数据")?
                .clone()
        };

        // 验证并安全包裹请求的列名（此前直接 join 拼接，未逐个校验，是注入面）
        let selected_columns: Option<Vec<String>> = match &request.columns {
            Some(cols) => {
                let mut quoted = Vec::with_capacity(cols.len());
                for col in cols {
                    quoted.push(quote_identifier(col)
                        .map_err(|e| anyhow!("列名验证失败: {}", e))?);
                }
                Some(quoted)
            }
            None => None,
        };

        let start = Instant::now();

        // 获取主键列名
        let pk_columns: Vec<String> = run_query(
            sqlx::query_as::<_, (String,)>(
                "SELECT COLUMN_NAME FROM INFORMATION_SCHEMA.STATISTICS
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND INDEX_NAME = 'PRIMARY'
                 ORDER BY SEQ_IN_INDEX",
            )
            .bind(database)
            .bind(table)
            .fetch_all(&pool),
            &query_cancel,
            &query_notify,
        )
        .await
        .map(|v| v.into_iter().map(|(name,)| name).collect())
        .unwrap_or_default();

        // 获取列类型（含精度信息，如 varchar(255)、decimal(10,2)、int(11)）与表内列顺序
        let column_meta: Vec<(String, String)> = run_query(
            sqlx::query_as::<_, (String, String)>(
                "SELECT COLUMN_NAME, COLUMN_TYPE FROM INFORMATION_SCHEMA.COLUMNS
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?
                 ORDER BY ORDINAL_POSITION",
            )
            .bind(database)
            .bind(table)
            .fetch_all(&pool),
            &query_cancel,
            &query_notify,
        )
        .await
        .unwrap_or_default();
        let column_types: HashMap<String, String> = column_meta.iter().cloned().collect();

        // 构建查询SQL
        let mut sql = String::from("SELECT ");

        match &selected_columns {
            Some(cols) => sql.push_str(&cols.join(", ")),
            None => sql.push('*'),
        }

        sql.push_str(&format!(" FROM `{}`.`{}`", database, table));

        // WHERE 子句单独构建，SELECT 与 COUNT 复用，保证带过滤/搜索时分页总数正确
        let mut conditions: Vec<String> = Vec::new();
        if let Some(filter) = &request.filter {
            let mut cond = format!("`{}` ", filter.column);
            match filter.operator {
                FilterOperator::Eq => {
                    let val = quote_value(&filter.value, DriverKind::MySql)?;
                    cond.push_str(&format!("= {}", val));
                }
                FilterOperator::Ne => {
                    let val = quote_value(&filter.value, DriverKind::MySql)?;
                    cond.push_str(&format!("!= {}", val));
                }
                FilterOperator::Gt => {
                    let val = quote_value(&filter.value, DriverKind::MySql)?;
                    cond.push_str(&format!("> {}", val));
                }
                FilterOperator::Gte => {
                    let val = quote_value(&filter.value, DriverKind::MySql)?;
                    cond.push_str(&format!(">= {}", val));
                }
                FilterOperator::Lt => {
                    let val = quote_value(&filter.value, DriverKind::MySql)?;
                    cond.push_str(&format!("< {}", val));
                }
                FilterOperator::Lte => {
                    let val = quote_value(&filter.value, DriverKind::MySql)?;
                    cond.push_str(&format!("<= {}", val));
                }
                FilterOperator::Like => {
                    let val = quote_value(&filter.value, DriverKind::MySql)?;
                    cond.push_str(&format!("LIKE {}", val));
                }
                FilterOperator::IsNull => {
                    cond.push_str("IS NULL");
                }
                FilterOperator::IsNotNull => {
                    cond.push_str("IS NOT NULL");
                }
            }
            conditions.push(cond);
        }

        // 关键字搜索：对所有可比较列做包含匹配（BLOB/二进制/空间类型跳过）
        if let Some(keyword) = request.search.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
            let searchable: Vec<&str> = column_meta
                .iter()
                .filter(|(_, col_type)| is_searchable_type(col_type))
                .map(|(name, _)| name.as_str())
                .collect();
            if !searchable.is_empty() {
                let pattern = escape_like_keyword(keyword);
                let likes: Vec<String> = searchable
                    .iter()
                    .map(|col| format!("`{}` LIKE '%{}%'", col, pattern))
                    .collect();
                conditions.push(format!("({})", likes.join(" OR ")));
            }
        }

        // 游标翻页能力：单列主键 + 未指定排序 + 主键类型可比较。
        // 主键是二进制/空间类型时无法安全回传比较，退回 OFFSET 翻页。
        let cursor_supported = request
            .sort_column
            .as_deref()
            .map_or(true, |col| col.trim().is_empty())
            && pk_columns.len() == 1
            && column_types
                .get(&pk_columns[0])
                .map_or(true, |data_type| is_searchable_type(data_type));
        let cursor_column: Option<String> = if cursor_supported {
            Some(pk_columns[0].clone())
        } else {
            None
        };
        // 只有续页请求（带游标）才走 keyset 条件；首页仍按 OFFSET 取，便于前端同时拿到总数
        let use_cursor = cursor_column.is_some() && request.after.is_some();
        if use_cursor {
            if let (Some(pk), Some(cursor)) = (&cursor_column, &request.after) {
                conditions.push(format!(
                    "`{}` > {}",
                    pk,
                    quote_value(cursor, DriverKind::MySql)?
                ));
            }
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };
        sql.push_str(&where_clause);

        // 游标翻页必须按主键升序，保证跨批次连续且不重复
        if let Some(pk) = &cursor_column {
            sql.push_str(&format!(" ORDER BY `{}` ASC", pk));
        }

        // ORDER BY - 只在排序列有效时添加
        if cursor_column.is_none() {
            if let Some(col) = &request.sort_column {
                // 验证排序列是否存在（非空且不是纯空格）
                if !col.trim().is_empty() {
                    let direction = request.sort_direction.unwrap_or(SortDirection::Asc);
                    sql.push_str(&format!(
                        " ORDER BY `{}` {}",
                        col,
                        match direction {
                            SortDirection::Asc => "ASC",
                            SortDirection::Desc => "DESC",
                        }
                    ));
                }
            }
        }

        // LIMIT：游标翻页不带 OFFSET，页间靠主键游标衔接
        if use_cursor {
            sql.push_str(&format!(" LIMIT {}", request.page_size));
        } else {
            let offset = (request.page - 1) * request.page_size;
            sql.push_str(&format!(" LIMIT {} OFFSET {}", request.page_size, offset));
        }

        // 数据与总数放在同一只读事务里：MySQL 默认可重复读，快照在首个读语句建立，
        // 避免两次查询之间数据变化导致「总数」与当前页不一致
        let mut tx = pool.begin().await?;
        let result = run_query(
            sqlx::query(&sql).fetch_all(&mut *tx),
            &query_cancel,
            &query_notify,
        )
        .await?;

        // 游标翻页不重复执行 COUNT：千万行表的 COUNT 同样昂贵，总数由前端沿用首页结果。
        // OFFSET 翻页时复用 WHERE 子句统计，保证带过滤时分页总数正确。
        let total_count: i64 = if use_cursor {
            0
        } else {
            let count_sql = format!(
                "SELECT COUNT(*) FROM `{}`.`{}`{}",
                database, table, where_clause
            );
            run_query(
                sqlx::query_scalar(&count_sql).fetch_one(&mut *tx),
                &query_cancel,
                &query_notify,
            )
            .await?
        };
        tx.commit().await?;

        let execution_time_ms = start.elapsed().as_millis() as u64;

        // 处理结果
        let columns: Vec<ColumnDefinition> = if result.is_empty() {
            // 文本协议在 0 行时不返回列元数据，回落到 INFORMATION_SCHEMA 的列顺序与类型，
            // 保证空表/筛选无结果/分页越界时表头与主键判定都不丢失
            let ordered: Vec<(String, String)> = match &request.columns {
                Some(cols) => cols.iter().filter_map(|name| column_meta.iter().find(|(n, _)| n == name).cloned()).collect(),
                None => column_meta.clone(),
            };
            ordered
                .into_iter()
                .map(|(name, data_type)| ColumnDefinition {
                    is_primary: pk_columns.contains(&name),
                    name,
                    data_type,
                    is_nullable: true,
                    is_auto_increment: false,
                })
                .collect()
        } else {
            result[0]
                .columns()
                .iter()
                .map(|col| {
                    let col_name = col.name().to_string();
                    let is_primary = pk_columns.contains(&col_name);
                    ColumnDefinition {
                        name: col_name.clone(),
                        data_type: column_types
                            .get(&col_name)
                            .cloned()
                            .unwrap_or_else(|| col.type_info().to_string()),
                        is_nullable: true,
                        is_primary,
                        is_auto_increment: false,
                    }
                })
                .collect()
        };

        let kinds: Vec<ColumnKind> = columns.iter().map(|c| classify_column(&c.data_type)).collect();

        let mut rows = vec![];
        for row in &result {
            let mut row_data = vec![];
            for (i, _col) in columns.iter().enumerate() {
                let value = convert_value(row, i, kinds[i])?;
                row_data.push(value);
            }
            rows.push(row_data);
        }

        // 下一次游标：取本批最后一行的主键值。只有整批取满（可能还有下一批）才给出游标，
        // 小表最后一批不足一页时返回 None，前端据此回到普通分页/停止加载。
        let next_cursor = match (&cursor_column, result.last()) {
            (Some(pk), Some(last_row)) if result.len() == request.page_size => columns
                .iter()
                .position(|c| &c.name == pk)
                .and_then(|index| convert_value(last_row, index, kinds[index]).ok())
                .and_then(|value| serde_json::to_value(value).ok()),
            _ => None,
        };

        Ok(DataQueryResponse {
            columns,
            rows,
            total_count: total_count as u64,
            page: request.page,
            page_size: request.page_size,
            execution_time_ms,
            next_cursor,
        })
    }

    /// 拆分 PG 表引用：树中非 public schema 的对象以 `schema.table` 展示，其余落到 public
    fn split_pg_table_ref(table: &str) -> (String, String) {
        match table.split_once('.') {
            Some((schema, name)) => (schema.to_string(), name.to_string()),
            None => ("public".to_string(), table.to_string()),
        }
    }

    /// PostgreSQL 表数据浏览
    ///
    /// 与 MySQL 的差异：schema/table 分开传入；限定名用双引号；
    /// 主键与列类型取自 information_schema 的标准视图（$n 绑定，不拼字符串）。
    async fn query_table_postgres(
        pool: &PgPool,
        schema: &str,
        table: &str,
        request: &DataQueryRequest,
        query_cancel: &AtomicBool,
        query_notify: &Notify,
    ) -> Result<DataQueryResponse> {
        let start = Instant::now();

        // 获取主键列（按约束内列顺序）
        let pk_columns: Vec<String> = run_query(
            sqlx::query_as::<_, (String,)>(
                "SELECT kcu.column_name
                 FROM information_schema.table_constraints tc
                 JOIN information_schema.key_column_usage kcu
                   ON kcu.constraint_name = tc.constraint_name
                  AND kcu.constraint_schema = tc.constraint_schema
                 WHERE tc.table_schema = $1 AND tc.table_name = $2
                   AND tc.constraint_type = 'PRIMARY KEY'
                 ORDER BY kcu.ordinal_position",
            )
            .bind(schema)
            .bind(table)
            .fetch_all(pool),
            query_cancel,
            query_notify,
        )
        .await
        .map(|v| v.into_iter().map(|(name,)| name).collect())
        .unwrap_or_default();

        // 展示类型带精度；udt_name 作为读取时的类型分类依据（int4/timestamptz/bytea 等）
        let column_meta: Vec<(String, String, String)> = run_query(
            sqlx::query_as::<_, (String, String, String)>(
                "SELECT column_name,
                        CASE
                            WHEN data_type IN ('character varying', 'character')
                                 AND character_maximum_length IS NOT NULL
                                THEN data_type || '(' || character_maximum_length || ')'
                            WHEN data_type = 'numeric' AND numeric_precision IS NOT NULL
                                THEN data_type || '(' || numeric_precision || ',' || COALESCE(numeric_scale, 0) || ')'
                            ELSE data_type
                        END,
                        udt_name
                 FROM information_schema.columns
                 WHERE table_schema = $1 AND table_name = $2
                 ORDER BY ordinal_position",
            )
            .bind(schema)
            .bind(table)
            .fetch_all(pool),
            query_cancel,
            query_notify,
        )
        .await
        .unwrap_or_default();

        let qualified = format!(
            "{}.{}",
            quote_pg_identifier(schema)?,
            quote_pg_identifier(table)?
        );

        // 显式列出列：无法由 sqlx 二进制解码的类型（vector、枚举、inet、数组等）先转 text，
        // 否则整页数据会因单列类型不兼容而读取失败
        let selected: Vec<String> = match &request.columns {
            Some(cols) => cols.clone(),
            None => column_meta.iter().map(|(name, _, _)| name.clone()).collect(),
        };

        let mut sql = String::from("SELECT ");
        if selected.is_empty() {
            // 元数据不可用时退回 *，由结果集的列类型兜底
            sql.push('*');
        } else {
            let mut expressions = Vec::with_capacity(selected.len());
            for name in &selected {
                let udt_name = column_meta
                    .iter()
                    .find(|(column, _, _)| column == name)
                    .map(|(_, _, udt)| udt.as_str())
                    .unwrap_or("");
                expressions.push(pg_select_expression(name, udt_name)?);
            }
            sql.push_str(&expressions.join(", "));
        }
        sql.push_str(" FROM ");
        sql.push_str(&qualified);

        // WHERE 子句单独构建，SELECT 与 COUNT 复用，保证带过滤/搜索时分页总数正确
        let mut conditions: Vec<String> = Vec::new();
        if let Some(filter) = &request.filter {
            let mut cond = format!("{} ", quote_pg_identifier(&filter.column)?);
            match filter.operator {
                FilterOperator::Eq => cond.push_str(&format!("= {}", quote_value(&filter.value, DriverKind::Postgres)?)),
                FilterOperator::Ne => cond.push_str(&format!("!= {}", quote_value(&filter.value, DriverKind::Postgres)?)),
                FilterOperator::Gt => cond.push_str(&format!("> {}", quote_value(&filter.value, DriverKind::Postgres)?)),
                FilterOperator::Gte => cond.push_str(&format!(">= {}", quote_value(&filter.value, DriverKind::Postgres)?)),
                FilterOperator::Lt => cond.push_str(&format!("< {}", quote_value(&filter.value, DriverKind::Postgres)?)),
                FilterOperator::Lte => cond.push_str(&format!("<= {}", quote_value(&filter.value, DriverKind::Postgres)?)),
                FilterOperator::Like => {
                    cond.push_str(&format!("LIKE {}", quote_value(&filter.value, DriverKind::Postgres)?))
                }
                FilterOperator::IsNull => cond.push_str("IS NULL"),
                FilterOperator::IsNotNull => cond.push_str("IS NOT NULL"),
            }
            conditions.push(cond);
        }

        // 关键字搜索：参与搜索的列统一转 text 后做包含匹配，避免非文本类型无 LIKE 操作符
        if let Some(keyword) = request
            .search
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
        {
            let searchable: Vec<String> = column_meta
                .iter()
                .filter(|(_, _, udt)| is_searchable_pg_type(udt))
                .filter_map(|(name, _, _)| quote_pg_identifier(name).ok())
                .collect();
            if !searchable.is_empty() {
                let pattern = escape_like_keyword(keyword);
                let likes: Vec<String> = searchable
                    .iter()
                    .map(|col| format!("{}::text LIKE '%{}%'", col, pattern))
                    .collect();
                conditions.push(format!("({})", likes.join(" OR ")));
            }
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };
        sql.push_str(&where_clause);

        if let Some(col) = &request.sort_column {
            if !col.trim().is_empty() {
                let direction = request.sort_direction.unwrap_or(SortDirection::Asc);
                sql.push_str(&format!(
                    " ORDER BY {} {}",
                    quote_pg_identifier(col)?,
                    match direction {
                        SortDirection::Asc => "ASC",
                        SortDirection::Desc => "DESC",
                    }
                ));
            }
        }

        let offset = (request.page - 1) * request.page_size;
        sql.push_str(&format!(" LIMIT {} OFFSET {}", request.page_size, offset));

        // 数据与总数放在同一只读事务里：PG 默认读已提交，需显式提升到可重复读，
        // 否则两条语句各取一次快照，会出现「总数」与当前页不一致
        let mut tx = pool.begin().await?;
        run_query(
            sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ").execute(&mut *tx),
            query_cancel,
            query_notify,
        )
        .await?;
        let result = run_query(
            sqlx::query(&sql).fetch_all(&mut *tx),
            query_cancel,
            query_notify,
        )
        .await?;

        let count_sql = format!("SELECT COUNT(*) FROM {}{}", qualified, where_clause);
        let total_count: i64 = run_query(
            sqlx::query_scalar(&count_sql).fetch_one(&mut *tx),
            query_cancel,
            query_notify,
        )
        .await?;
        tx.commit().await?;

        let execution_time_ms = start.elapsed().as_millis() as u64;

        // 列顺序与 SELECT 一致，空结果集也能保留表头
        let ordered: Vec<(String, String, String)> = match &request.columns {
            Some(cols) => cols
                .iter()
                .map(|name| {
                    column_meta
                        .iter()
                        .find(|(n, _, _)| n == name)
                        .cloned()
                        .unwrap_or_else(|| (name.clone(), String::new(), String::new()))
                })
                .collect(),
            None => column_meta.clone(),
        };

        let (columns, kinds): (Vec<ColumnDefinition>, Vec<PgColumnKind>) = if ordered.is_empty() {
            // 元数据不可用时退回结果集自身的列信息
            match result.first() {
                Some(first) => {
                    let columns = first
                        .columns()
                        .iter()
                        .map(|c| {
                            let name = c.name().to_string();
                            ColumnDefinition {
                                is_primary: pk_columns.contains(&name),
                                name,
                                data_type: c.type_info().to_string(),
                                is_nullable: true,
                                is_auto_increment: false,
                            }
                        })
                        .collect::<Vec<_>>();
                    let kinds = first
                        .columns()
                        .iter()
                        .map(|c| classify_pg_column(&c.type_info().to_string()))
                        .collect::<Vec<_>>();
                    (columns, kinds)
                }
                None => (Vec::new(), Vec::new()),
            }
        } else {
            let columns = ordered
                .iter()
                .map(|(name, data_type, _)| ColumnDefinition {
                    is_primary: pk_columns.contains(name),
                    name: name.clone(),
                    data_type: data_type.clone(),
                    is_nullable: true,
                    is_auto_increment: false,
                })
                .collect::<Vec<_>>();
            let kinds = ordered
                .iter()
                .map(|(_, _, udt)| classify_pg_column(udt))
                .collect::<Vec<_>>();
            (columns, kinds)
        };

        let mut rows = Vec::with_capacity(result.len());
        for row in &result {
            let mut row_data = Vec::with_capacity(kinds.len());
            for (i, kind) in kinds.iter().enumerate() {
                row_data.push(convert_pg_value(row, i, *kind)?);
            }
            rows.push(row_data);
        }

        Ok(DataQueryResponse {
            columns,
            rows,
            total_count: total_count as u64,
            page: request.page,
            page_size: request.page_size,
            execution_time_ms,
            // PG 表浏览暂未接入 keyset 翻页，前端据此走 OFFSET 分页
            next_cursor: None,
        })
    }

    /// 取消当前连接上正在执行的查询，返回是否因此丢弃了进行中的事务（已回滚）。
    ///
    /// 置位取消标志并唤醒等待中的执行：执行中的查询丢弃其 future，物理连接关闭、
    /// 服务端查询随客户端断开而终止。仅在确有在途查询时才会丢弃进行中的事务
    /// （Drop 自动回滚），避免留下一条已不可用的事务连接；无在途查询时不碰事务。
    pub async fn cancel_query(&self, connection_id: &str) -> Result<bool> {
        let connections = self.connections.read().await;
        let conn = connections.get(connection_id)
            .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;

        // 没有在途查询时不置位、也不碰事务：否则会误杀下一次查询或静默回滚用户事务
        if !conn.query_running.load(Ordering::SeqCst) {
            return Ok(false);
        }

        // 必须先置位并唤醒，再去抢事务锁：查询执行期间持有同一把锁，
        // 先抢锁会让取消请求一直阻塞到查询自然结束，取消形同失效。
        conn.query_cancel.store(true, Ordering::SeqCst);
        conn.query_notify.notify_waiters();

        // 置位后执行方会立即丢弃查询 future 并释放锁，此处的等待是毫秒级的。
        // 事务连接协议流已被破坏，只能丢弃事务（自动回滚）；通过返回值告知前端，
        // 避免界面仍显示「事务活跃」却已无事务可提交。
        let mut txn_guard = conn.transaction.lock().await;
        let rolled_back = if txn_guard.is_some() {
            *txn_guard = None;
            true
        } else {
            false
        };
        Ok(rolled_back)
    }

    /// 流式导出表数据到文件（CSV / SQL / JSON）
    ///
    /// 逐行读取结果集并追加写入 `BufWriter`，内存占用与表大小无关；
    /// 全程受取消标志与 `MAX_EXPORT_DURATION` 约束，失败时由调用方清理半成品文件。
    pub async fn export_table_to_file(
        &self,
        connection_id: &str,
        database: &str,
        table: &str,
        format: &str,
        path: &std::path::Path,
    ) -> Result<u64> {
        let format = ExportFormat::parse(format)?;
        // 输入验证
        validate_identifier(database).map_err(|e| anyhow!("数据库名验证失败: {}", e))?;
        validate_identifier(table).map_err(|e| anyhow!("表名验证失败: {}", e))?;

        // 只取池的克隆后释放连接表读锁，导出期间不阻塞其他连接操作
        let pool = {
            let connections = self.connections.read().await;
            let conn = connections
                .get(connection_id)
                .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;

            conn.pool
                .as_ref()
                .ok_or_else(|| anyhow!("连接尚未建立"))?
                .as_mysql("导出表数据")?
                .clone()
        };

        // 从连接池获取单个连接，确保 USE 语句和查询在同一个连接上执行
        let mut conn = pool
            .acquire()
            .await
            .map_err(|e| anyhow!("获取数据库连接失败: {}", e))?;

        // 整表导出可能很久：给更宽松的超时上限，同时允许前端取消
        let (query_cancel, query_notify, query_running) =
            self.exec_handles(connection_id).await?;
        let _run_guard = QueryRunGuard::start(&query_cancel, &query_running);

        // 使用当前数据库 - USE 语句不支持预处理协议
        let use_db = format!("USE `{}`", database);
        conn.execute(use_db.as_str())
            .await
            .map_err(|e| anyhow!("切换数据库失败: {}", e))?;

        // 创建目标文件：数据逐行写入，不在内存里保留整表
        let file =
            std::fs::File::create(path).map_err(|e| anyhow!("创建导出文件失败: {}", e))?;
        // 获取列类型信息，失败时回退到结果集自带的列信息
        let columns = export_columns(&mut conn, database, table).await;
        let mut sink = TableExportSink::new(
            std::io::BufWriter::with_capacity(256 * 1024, file),
            format,
            table,
            columns,
        );

        stream_table_rows(&mut conn, &query_cancel, &query_notify, table, |row| {
            sink.write_row(row)
        })
        .await?;

        sink.finish()?;
        Ok(sink.rows())
    }

    /// 读取整表的列定义（名称 + DATA_TYPE），顺序与 `SELECT *` 一致。
    ///
    /// Excel 导出需要先写表头，因此把列元数据单独暴露出来；
    /// INFORMATION_SCHEMA 查询失败时返回空列表，由调用方退回结果集列信息。
    pub async fn table_export_columns(
        &self,
        connection_id: &str,
        database: &str,
        table: &str,
    ) -> Result<Vec<ExportColumn>> {
        validate_identifier(database).map_err(|e| anyhow!("数据库名验证失败: {}", e))?;
        validate_identifier(table).map_err(|e| anyhow!("表名验证失败: {}", e))?;

        let pool = {
            let connections = self.connections.read().await;
            let conn = connections
                .get(connection_id)
                .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;
            conn.pool
                .as_ref()
                .ok_or_else(|| anyhow!("连接尚未建立"))?
                .as_mysql("导出表数据")?
                .clone()
        };

        let mut conn = pool
            .acquire()
            .await
            .map_err(|e| anyhow!("获取数据库连接失败: {}", e))?;
        Ok(export_columns(&mut conn, database, table).await)
    }

    /// 流式遍历整表并逐行回调，内存占用与表大小无关。
    ///
    /// 供 Excel 导出使用：调用方在回调里直接写工作簿，不再把整表读进内存；
    /// 全程受取消标志与 `MAX_EXPORT_DURATION` 约束。
    pub async fn export_table_stream<F>(
        &self,
        connection_id: &str,
        database: &str,
        table: &str,
        mut on_row: F,
    ) -> Result<u64>
    where
        F: FnMut(&sqlx::mysql::MySqlRow) -> Result<()>,
    {
        validate_identifier(database).map_err(|e| anyhow!("数据库名验证失败: {}", e))?;
        validate_identifier(table).map_err(|e| anyhow!("表名验证失败: {}", e))?;

        // 只取池的克隆后释放连接表读锁，导出期间不阻塞其他连接操作
        let pool = {
            let connections = self.connections.read().await;
            let conn = connections
                .get(connection_id)
                .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;
            conn.pool
                .as_ref()
                .ok_or_else(|| anyhow!("连接尚未建立"))?
                .as_mysql("导出表数据")?
                .clone()
        };

        let mut conn = pool
            .acquire()
            .await
            .map_err(|e| anyhow!("获取数据库连接失败: {}", e))?;

        let use_db = format!("USE `{}`", database);
        conn.execute(use_db.as_str())
            .await
            .map_err(|e| anyhow!("切换数据库失败: {}", e))?;

        let (query_cancel, query_notify, query_running) =
            self.exec_handles(connection_id).await?;
        let _run_guard = QueryRunGuard::start(&query_cancel, &query_running);

        stream_table_rows(&mut conn, &query_cancel, &query_notify, table, &mut on_row).await
    }

    /// 导入数据
    pub async fn import_data(
        &self,
        connection_id: &str,
        database: &str,
        table: &str,
        format: &str,
        data: &str,
    ) -> Result<u64> {
        // 输入验证
        validate_identifier(database)
            .map_err(|e| anyhow!("数据库名验证失败: {}", e))?;
        validate_identifier(table)
            .map_err(|e| anyhow!("表名验证失败: {}", e))?;

        // 验证导入格式
        let allowed_formats = ["csv", "sql"];
        if !allowed_formats.contains(&format.to_lowercase().as_str()) {
            return Err(anyhow!("不支持的导入格式: {}", format));
        }

        // 验证数据大小
        const MAX_IMPORT_SIZE: usize = 100_000_000; // 100MB
        if data.len() > MAX_IMPORT_SIZE {
            return Err(anyhow!("导入数据大小不能超过 100MB"));
        }

        // 只取池的克隆后释放连接表读锁：导入可能持续很久，不能在此期间阻塞其他连接操作
        let (pool, safety_mode) = {
            let connections = self.connections.read().await;
            let conn = connections.get(connection_id)
                .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;

            // 只读连接不允许导入数据
            if conn.config.safety_mode == SafetyMode::ReadOnly {
                return Err(anyhow!("当前连接为只读模式，无法导入数据"));
            }

            let pool = conn.pool.as_ref()
                .ok_or_else(|| anyhow!("连接尚未建立"))?
                .as_mysql("导入数据")?
                .clone();
            (pool, conn.config.safety_mode)
        };

        // 从连接池获取单个连接，确保 USE 语句和所有导入操作在同一个连接上执行
        let mut conn = pool.acquire().await
            .map_err(|e| anyhow!("获取数据库连接失败: {}", e))?;

        // 使用当前数据库 - USE 语句不支持预处理协议
        let use_db = format!("USE `{}`", database);
        conn.execute(use_db.as_str()).await
            .map_err(|e| anyhow!("切换数据库失败: {}", e))?;

        let mut count = 0u64;

        match format.to_lowercase().as_str() {
            "csv" => {
                // 与导出对称的 RFC 4180 解析：支持引号包裹的逗号、换行与双写引号
                let records = export::parse_csv(data);
                if records.is_empty() {
                    return Ok(0);
                }

                let headers: Vec<String> = records[0]
                    .iter()
                    .map(|field| field.value.trim().to_string())
                    .collect();

                // 验证列名
                for header in &headers {
                    validate_identifier(header)
                        .map_err(|e| anyhow!("CSV 列名验证失败 '{}': {}", header, e))?;
                }

                // 限制最大导入行数
                const MAX_IMPORT_ROWS: usize = 10000;
                if records.len() - 1 > MAX_IMPORT_ROWS {
                    return Err(anyhow!("CSV 导入行数不能超过 {}", MAX_IMPORT_ROWS));
                }

                // 二进制列导出时是 Base64 文本，导入必须反向解码，
                // 否则 BLOB/BINARY 会被当成普通字符串写回，内容不可还原
                let binary_columns = table_binary_columns(&mut conn, table).await;

                let columns_str: String = headers.iter()
                    .map(|h| format!("`{}`", escape_string(h)))
                    .collect::<Vec<_>>()
                    .join(", ");

                // CSV 导入整体放在一个事务里：中途失败时回滚，避免只导入一半数据
                let mut tx = conn.begin().await
                    .map_err(|e| anyhow!("开始导入事务失败：{}", e))?;
                for (index, record) in records.iter().enumerate().skip(1) {
                    // 跳过空行
                    if record.len() == 1 && record[0].value.trim().is_empty() && !record[0].quoted {
                        continue;
                    }
                    if record.len() != headers.len() {
                        return Err(anyhow!(
                            "CSV 第 {} 行列数为 {}，与表头列数 {} 不一致",
                            index + 1,
                            record.len(),
                            headers.len()
                        ));
                    }

                    let mut values: Vec<String> = Vec::with_capacity(record.len());
                    for (col_index, field) in record.iter().enumerate() {
                        // 未加引号的空字段与 NULL 视为 SQL NULL；带引号的一律按文本处理
                        if !field.quoted && (field.value.is_empty() || field.value == "NULL") {
                            values.push("NULL".to_string());
                        } else if headers
                            .get(col_index)
                            .is_some_and(|name| binary_columns.contains(name))
                        {
                            let column = headers.get(col_index).map(String::as_str).unwrap_or("");
                            let literal = decode_base64_to_hex(&field.value).with_context(|| {
                                format!(
                                    "CSV 第 {} 行列 '{}' 不是合法的 Base64（二进制列需按导出的 Base64 文本导入）",
                                    index + 1,
                                    column
                                )
                            })?;
                            values.push(literal);
                        } else {
                            values.push(format!("'{}'", escape_string(&field.value)));
                        }
                    }
                    let values_str = values.join(", ");

                    let query = format!("INSERT INTO `{}` ({}) VALUES ({})", table, columns_str, values_str);
                    sqlx::query(&query).execute(&mut *tx).await
                        .map_err(|e| anyhow!("CSV 第 {} 行插入失败（表 {}）：{}", index + 1, table, e))?;
                    count += 1;
                }
                tx.commit().await
                    .map_err(|e| anyhow!("提交导入事务失败：{}", e))?;
            }
            "sql" => {
                // 与 CSV 分支一致：整批脚本放在一个事务里，中途失败即整体回滚。
                // 注意：MySQL 的 DDL（CREATE/ALTER/DROP/TRUNCATE 等）会隐式提交，
                // 事务只能保证 DML 批次的原子性，DDL 无法回滚。
                let mut tx = conn.begin().await
                    .map_err(|e| anyhow!("开始导入事务失败：{}", e))?;
                // 执行 SQL 语句 - 按分号分割完整的 SQL 语句
                let statements = split_sql_statements(data);
                for (index, stmt) in statements.iter().enumerate() {
                    let stmt = stmt.trim();
                    if stmt.is_empty() || stmt.starts_with("--") {
                        continue;
                    }
                    // 导入脚本此前只拦只读模式，Safe 模式下仍会执行 DROP/TRUNCATE 等危险语句
                    crate::sql::safety::enforce(safety_mode, stmt).map_err(|e| {
                        anyhow!("第 {} 条 SQL 被安全模式拦截：{}\nStatement: {}", index + 1, e, stmt)
                    })?;
                    // 文本协议执行：DDL、存储过程等语句无法走预处理协议
                    Executor::execute(&mut *tx, stmt).await
                        .map_err(|e| anyhow!("第 {} 条 SQL 执行失败：{}\nStatement: {}", index + 1, e, stmt))?;
                    count += 1;
                }
                tx.commit().await
                    .map_err(|e| anyhow!("提交导入事务失败：{}", e))?;
            }
            _ => return Err(anyhow!("不支持的导入格式: {}", format)),
        }

        Ok(count)
    }

    /// 导入数据库SQL
    pub async fn import_database_sql(
        &self,
        connection_id: &str,
        database: &str,
        sql: &str,
    ) -> Result<()> {
        // 输入验证
        validate_identifier(database)
            .map_err(|e| anyhow!("数据库名验证失败: {}", e))?;

        // 验证 SQL 大小限制
        const MAX_SQL_SIZE: usize = 100_000_000; // 100MB
        if sql.len() > MAX_SQL_SIZE {
            return Err(anyhow!("SQL 文件大小不能超过 100MB"));
        }

        // 只取池的克隆后释放连接表读锁：大批量导入期间不应阻塞其他连接操作
        let (pool, safety_mode) = {
            let connections = self.connections.read().await;
            let conn = connections.get(connection_id)
                .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;

            // 只读连接不允许导入 SQL
            if conn.config.safety_mode == SafetyMode::ReadOnly {
                return Err(anyhow!("当前连接为只读模式，无法导入 SQL"));
            }

            let pool = conn.pool.as_ref()
                .ok_or_else(|| anyhow!("连接尚未建立"))?
                .as_mysql("导入 SQL")?
                .clone();
            (pool, conn.config.safety_mode)
        };

        // 从连接池获取单个连接，确保 USE 语句在整个导入过程中生效
        let mut conn = pool.acquire().await
            .map_err(|e| anyhow!("获取数据库连接失败: {}", e))?;

        // 切换到目标数据库（在同一连接上执行）- USE 语句不支持预处理协议
        // 使用 execute() 方法直接执行，避免生命周期问题
        let use_db = format!("USE `{}`", database);
        conn.execute(use_db.as_str()).await
            .map_err(|e| anyhow!("切换数据库失败: {}", e))?;

        // 安全分割 SQL 语句（忽略字符串字面量和注释中的分号）
        let statements = split_sql_statements(sql);

        // 整批脚本放进一个事务，中途失败即整体回滚，避免只导入一半。
        // 注意：MySQL 的 DDL 会隐式提交，事务只保护 DML 批次。
        let mut tx = conn.begin().await
            .map_err(|e| anyhow!("开始导入事务失败：{}", e))?;

        for (index, stmt) in statements.iter().enumerate() {
            let stmt = stmt.trim();
            if stmt.is_empty() {
                continue;
            }

            // 之前仅拦截只读模式，Safe 模式下脚本里的 DROP/TRUNCATE 依然会执行
            crate::sql::safety::enforce(safety_mode, stmt).map_err(|e| {
                anyhow!("第 {} 条 SQL 被安全模式拦截：{}\nStatement: {}", index + 1, e, stmt)
            })?;

            // 文本协议执行：DDL、存储过程等语句无法走预处理协议
            Executor::execute(&mut *tx, stmt).await
                .map_err(|e| anyhow!("第 {} 条 SQL 执行失败：{}\nStatement: {}", index + 1, e, stmt))?;
        }

        tx.commit().await
            .map_err(|e| anyhow!("提交导入事务失败：{}", e))?;

        Ok(())
    }
}

/// 读取表的列定义（名称 + DATA_TYPE），顺序与 `SELECT *` 一致。
///
/// 元数据查询失败时返回空列表并记一条告警，调用方回退到结果集自带的列信息。
async fn export_columns(
    conn: &mut MySqlConnection,
    database: &str,
    table: &str,
) -> Vec<ExportColumn> {
    let meta: Vec<(String, String)> = match sqlx::query_as(
        "SELECT COLUMN_NAME, DATA_TYPE FROM INFORMATION_SCHEMA.COLUMNS \
         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION",
    )
    .bind(database)
    .bind(table)
    .fetch_all(&mut *conn)
    .await
    {
        Ok(meta) => meta,
        Err(e) => {
            tracing::warn!("读取列元数据失败，改用结果集列信息: {}", e);
            Vec::new()
        }
    };

    meta.into_iter()
        .map(|(name, data_type)| ExportColumn { name, data_type })
        .collect()
}

/// 逐行流式读取整表，每读一行调用一次 `on_row`。
///
/// 内存占用与表大小无关；每轮都检查取消标志与导出超时上限，
/// 以便用户在千万行表上也能及时中断。
async fn stream_table_rows<F>(
    conn: &mut MySqlConnection,
    query_cancel: &AtomicBool,
    query_notify: &Notify,
    table: &str,
    mut on_row: F,
) -> Result<u64>
where
    F: FnMut(&sqlx::mysql::MySqlRow) -> Result<()>,
{
    let query = format!("SELECT * FROM `{}`", table);
    let mut rows = sqlx::query(&query).fetch(&mut *conn);
    let deadline = Instant::now() + MAX_EXPORT_DURATION;
    let mut count = 0u64;

    loop {
        if query_cancel.load(Ordering::SeqCst) {
            return Err(anyhow!("查询已被用户取消"));
        }
        if Instant::now() >= deadline {
            return Err(anyhow!(
                "导出超过 {} 分钟，已终止；请缩小数据范围后重试",
                MAX_EXPORT_DURATION.as_secs() / 60
            ));
        }

        let next = tokio::select! {
            item = rows.try_next() => item.map_err(|e| anyhow!("查询失败: {}", e))?,
            // 通知可能早于本轮检查到达，下一轮由 cancel 标志兜住
            _ = query_notify.notified() => continue,
            _ = tokio::time::sleep(Duration::from_millis(300)) => continue,
        };

        match next {
            Some(row) => {
                on_row(&row)?;
                count += 1;
            }
            None => break,
        }
    }

    Ok(count)
}

/// 查询表中所有二进制列（BLOB / BINARY / VARBINARY 等）的列名。
///
/// 元数据查询失败时返回空集合，退回「全部按文本导入」的旧行为。
async fn table_binary_columns(conn: &mut MySqlConnection, table: &str) -> HashSet<String> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT COLUMN_NAME, DATA_TYPE FROM INFORMATION_SCHEMA.COLUMNS \
         WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ?",
    )
    .bind(table)
    .fetch_all(&mut *conn)
    .await
    .unwrap_or_default();

    rows.into_iter()
        .filter(|(_, data_type)| matches!(classify_column(data_type), ColumnKind::Binary))
        .map(|(name, _)| name)
        .collect()
}

/// 把导出的 Base64 文本还原成 MySQL 十六进制字面量（`0x…`）。
///
/// - 空文本代表零长度二进制，写成 `''` 而不是 NULL；
/// - 导出时 CSV 公式中和可能给 `+` 开头的 Base64 补了前导单引号，这里先剥掉。
fn decode_base64_to_hex(value: &str) -> Result<String> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;

    let compact: String = value
        .trim_start_matches('\'')
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if compact.is_empty() {
        return Ok("''".to_string());
    }

    let bytes = STANDARD
        .decode(compact.as_bytes())
        .map_err(|e| anyhow!("Base64 解码失败: {}", e))?;
    let hex: String = bytes.iter().map(|b| format!("{:02X}", b)).collect();
    Ok(format!("0x{}", hex))
}

/// 计算两个时间点之间的毫秒数（保留小数，用于耗时统计）。
fn ms(end: Instant, start: Instant) -> f64 {
    end.duration_since(start).as_secs_f64() * 1000.0
}

/// 判断列类型是否参与关键字搜索。
///
/// BLOB/二进制/空间类型无法安全地做字符串包含匹配（或匹配结果没有意义），直接跳过。
pub fn is_searchable_type(column_type: &str) -> bool {
    let t = column_type.to_ascii_lowercase();
    !(t.contains("blob")
        || t.contains("binary")
        || t.starts_with("bit")
        || t.contains("geometry")
        || t.contains("point")
        || t.contains("linestring")
        || t.contains("polygon"))
}

/// sqlx 能按字符串直接解码的 PG 类型（其余文本类类型都先转 `::text`）
fn is_pg_text_native(udt_name: &str) -> bool {
    matches!(
        udt_name.to_lowercase().as_str(),
        "text" | "varchar" | "bpchar" | "name" | "citext"
    )
}

/// 构造 PG 查询的列表达式。
///
/// 已知类型交给 sqlx 按二进制解码；自定义类型（vector、枚举、域名）、数组、inet/interval 等
/// 没有对应的 Rust 解码器，统一 `::text` 以拿到可读文本（如 vector 显示为 `[1,2,3]`）。
fn pg_select_expression(column: &str, udt_name: &str) -> Result<String> {
    let quoted = quote_pg_identifier(column)?;
    if matches!(classify_pg_column(udt_name), PgColumnKind::Text) && !is_pg_text_native(udt_name) {
        // 保留原列名，列顺序与展示头保持一致
        Ok(format!("{}::text AS {}", quoted, quoted))
    } else {
        Ok(quoted)
    }
}

/// 判断 PostgreSQL 列是否参与关键字搜索。
///
/// 搜索时统一转 `::text`，因此只有没有有意义文本表示的 bytea 需要排除。
fn is_searchable_pg_type(udt_name: &str) -> bool {
    !udt_name.eq_ignore_ascii_case("bytea")
}

/// 转义 LIKE 模式中的通配符，避免用户输入的 `%`/`_` 被当成模式。
fn escape_like_keyword(keyword: &str) -> String {
    keyword
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
        .replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn searchable_type_skips_binary_like_columns() {
        assert!(is_searchable_type("varchar(255)"));
        assert!(is_searchable_type("text"));
        assert!(is_searchable_type("int(11)"));
        assert!(is_searchable_type("datetime"));
        assert!(!is_searchable_type("blob"));
        assert!(!is_searchable_type("longblob"));
        assert!(!is_searchable_type("varbinary(16)"));
        assert!(!is_searchable_type("bit(1)"));
        assert!(!is_searchable_type("geometry"));
    }

    #[test]
    fn base64_text_decodes_to_hex_literal() {
        // "PNG" 的 Base64 为 "UE5H"，导入时落成 0x504E47
        assert_eq!(decode_base64_to_hex("UE5H").unwrap(), "0x504E47");
        // 带空白与导出侧公式中和的前导单引号也要能还原
        assert_eq!(decode_base64_to_hex("'+Gk=").unwrap(), "0xF869");
        // 空文本是零长度二进制，不是 NULL
        assert_eq!(decode_base64_to_hex("").unwrap(), "''");
    }

    #[test]
    fn invalid_base64_is_rejected_with_reason() {
        let err = decode_base64_to_hex("not base64!").unwrap_err().to_string();
        assert!(err.contains("Base64"), "错误信息应说明 Base64 解码失败: {err}");
    }

    #[test]
    fn escape_like_keyword_escapes_wildcards_and_quotes() {
        assert_eq!(escape_like_keyword("abc"), "abc");
        assert_eq!(escape_like_keyword("50%"), "50\\%");
        assert_eq!(escape_like_keyword("a_b"), "a\\_b");
        assert_eq!(escape_like_keyword("it's"), "it''s");
        assert_eq!(escape_like_keyword("c:\\x"), "c:\\\\x");
    }

    #[test]
    fn change_target_table_uses_ast_not_text_match() {
        // 字符串字面量里出现目标表名不能算「引用了该表」
        assert_eq!(
            change_target_table(
                "DELETE FROM other WHERE note = 'users'",
                TableChangeKind::Delete
            )
            .unwrap(),
            "other"
        );
        // 库限定名取末段；别名不影响目标表
        assert_eq!(
            change_target_table(
                "UPDATE `db`.`users` AS u SET u.name = 'x' WHERE u.id = 1",
                TableChangeKind::Update
            )
            .unwrap(),
            "users"
        );
        assert_eq!(
            change_target_table("INSERT INTO `users` (id) VALUES (1)", TableChangeKind::Insert)
                .unwrap(),
            "users"
        );
        assert_eq!(
            change_target_table("DELETE FROM users WHERE id = 1", TableChangeKind::Delete).unwrap(),
            "users"
        );
    }

    #[test]
    fn change_target_table_rejects_multi_table_and_wrong_kind() {
        assert!(change_target_table(
            "UPDATE a JOIN users ON a.id = users.id SET a.x = 1",
            TableChangeKind::Update
        )
        .is_err());
        assert!(change_target_table(
            "DELETE a, users FROM a JOIN users ON a.id = users.id",
            TableChangeKind::Delete
        )
        .is_err());
        assert!(change_target_table("SELECT 1 FROM users", TableChangeKind::Delete).is_err());
        assert!(change_target_table("DELETE FROM", TableChangeKind::Delete).is_err());
    }

    #[test]
    fn validate_table_change_rejects_table_hidden_in_comment() {
        // 旧实现按文本包含判断，注释里的 users 会被误认为目标表
        let change = TableChange {
            kind: TableChangeKind::Update,
            sql: "UPDATE other /* users */ SET x = 1 WHERE id = 1".to_string(),
            table: "users".to_string(),
            where_clause: None,
        };
        let err = validate_table_change(&change, 1, SafetyMode::Off).unwrap_err();
        assert!(err.to_string().contains("不一致"), "{err}");
    }

    /// 真实 MySQL 回归用例的连接夹具。
    ///
    /// 必须指向**测试库**，通过以下环境变量提供：
    /// `AURORADB_TEST_MYSQL_HOST` / `PORT` / `USER` / `PASSWORD` / `DATABASE`。
    /// 未配置时用例静默跳过，保证默认 `cargo test` 不会误连数据库。
    async fn mysql_test_connection() -> Option<(ConnectionManager, String, String)> {
        let host = std::env::var("AURORADB_TEST_MYSQL_HOST").ok()?;
        let port = std::env::var("AURORADB_TEST_MYSQL_PORT")
            .ok()?
            .parse::<u16>()
            .ok()?;
        let username = std::env::var("AURORADB_TEST_MYSQL_USER").ok()?;
        let password = std::env::var("AURORADB_TEST_MYSQL_PASSWORD").unwrap_or_default();
        let database = std::env::var("AURORADB_TEST_MYSQL_DATABASE").ok()?;

        let manager = ConnectionManager::new();
        let id = manager
            .connect(ConnectionRequest {
                config: ConnectionConfig {
                    id: "qa-test".to_string(),
                    name: "qa-test".to_string(),
                    driver: DriverKind::MySql,
                    host,
                    port,
                    database: Some(database.clone()),
                    username,
                    password,
                    pool_config: PoolConfig::default(),
                    ssl_config: SslConfig::default(),
                    safety_mode: SafetyMode::Off,
                },
                ssh_config: None,
            })
            .await
            .ok()?;

        Some((manager, id, database))
    }

    fn paging_request(id: &str, database: &str, table: &str) -> DataQueryRequest {
        DataQueryRequest {
            connection_id: id.to_string(),
            database: database.to_string(),
            table: table.to_string(),
            columns: None,
            filter: Some(FilterCondition {
                column: "grp".to_string(),
                operator: FilterOperator::Eq,
                value: serde_json::json!(1),
            }),
            search: None,
            sort_column: None,
            sort_direction: None,
            page: 2,
            page_size: 3,
            after: None,
        }
    }

    #[tokio::test]
    #[ignore = "需要真实 MySQL 测试库：设置 AURORADB_TEST_MYSQL_* 后执行 cargo test -- --ignored"]
    async fn mysql_pagination_total_count_respects_filter() {
        let Some((manager, id, db)) = mysql_test_connection().await else {
            return;
        };
        let table = format!("qa_paging_{}", std::process::id());
        let ddl = format!("CREATE TABLE `{table}` (id INT PRIMARY KEY, grp INT NOT NULL)");
        manager.execute(&id, &db, &ddl).await.unwrap();

        // 30 行，其中 id 能被 3 整除的 10 行 grp=1
        let values: Vec<String> = (1..=30)
            .map(|i| format!("({i}, {})", if i % 3 == 0 { 1 } else { 0 }))
            .collect();
        let insert = format!("INSERT INTO `{table}` (id, grp) VALUES {}", values.join(", "));
        manager.execute(&id, &db, &insert).await.unwrap();

        let resp = manager
            .query_table(&id, &db, &table, paging_request(&id, &db, &table))
            .await
            .unwrap();

        // 总数必须是过滤后的 10 行，而不是全表 30 行；第二页仍返回 3 行
        assert_eq!(resp.total_count, 10, "分页总数未按 WHERE 过滤");
        assert_eq!(resp.rows.len(), 3, "第二页不足 3 行");

        let _ = manager
            .execute(&id, &db, &format!("DROP TABLE IF EXISTS `{table}`"))
            .await;
    }

    #[tokio::test]
    #[ignore = "需要真实 MySQL 测试库：设置 AURORADB_TEST_MYSQL_* 后执行 cargo test -- --ignored"]
    async fn mysql_apply_changes_rolls_back_whole_batch() {
        let Some((manager, id, db)) = mysql_test_connection().await else {
            return;
        };
        let table = format!("qa_atomic_{}", std::process::id());
        let ddl = format!("CREATE TABLE `{table}` (id INT PRIMARY KEY, note VARCHAR(32) NOT NULL)");
        manager.execute(&id, &db, &ddl).await.unwrap();

        // 第二条主键冲突：整批必须回滚，第一条也不能落库
        let changes = vec![
            TableChange {
                kind: TableChangeKind::Insert,
                sql: format!("INSERT INTO `{table}` (id, note) VALUES (1, 'a')"),
                table: table.clone(),
                where_clause: None,
            },
            TableChange {
                kind: TableChangeKind::Insert,
                sql: format!("INSERT INTO `{table}` (id, note) VALUES (1, 'b')"),
                table: table.clone(),
                where_clause: None,
            },
        ];
        let result = manager.apply_changes(&id, &db, changes).await;
        assert!(result.is_err(), "重复主键应导致整批失败");

        let mut req = paging_request(&id, &db, &table);
        req.filter = None;
        req.page = 1;
        req.page_size = 10;
        let resp = manager.query_table(&id, &db, &table, req).await.unwrap();
        assert_eq!(resp.total_count, 0, "事务回滚后不应留下半写数据");

        let _ = manager
            .execute(&id, &db, &format!("DROP TABLE IF EXISTS `{table}`"))
            .await;
    }
}
