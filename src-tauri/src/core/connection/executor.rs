//! SQL 执行模块
//! SQL 执行、表数据查询、取消、导入导出等。

use super::driver::{AnyPool, DriverKind};
use super::export::{self, ExportColumn, TableExportData};
use super::manager::{ConnectionManager, ManagedConnection};
use super::resultset::{
    classify_column, classify_pg_column, convert_pg_value, convert_value, format_sql_value,
    quote_value, ColumnKind, PgColumnKind,
};
use super::types::*;
use super::validation::*;
use anyhow::{anyhow, Context, Result};
use crate::core::sql::{rewrite_table_names, split_sql_statements};
use sqlx::mysql::MySqlPool;
use sqlx::postgres::{PgConnection, PgPool};
use sqlx::{Column, Executor, MySql, MySqlConnection, Postgres, Row, Statement};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Notify;

/// 单条 SQL 执行超时：交互式查询的宽松上限，超时即丢弃查询 future（连接关闭，服务端随之终止查询）
const MAX_QUERY_DURATION: Duration = Duration::from_secs(300);

/// SELECT 结果集行数上限：超出截断，防止全量进内存与全量 JSON 过 IPC 撑爆前端
const MAX_RESULT_ROWS: usize = 10_000;

/// 执行带超时与取消语义的查询。
///
/// 取消/超时即丢弃查询 future：该物理连接协议流未完成，sqlx 将其关闭而非归还连接池，
/// MySQL 服务端检测到客户端断开后会终止该查询，因此无需额外发起 KILL QUERY。
async fn run_query<R, F>(fut: F, cancel: &AtomicBool, notify: &Notify) -> Result<R>
where
    F: std::future::Future<Output = std::result::Result<R, sqlx::Error>>,
{
    // 本次执行开始前已有取消请求（先于执行到达）时立即拒绝
    if cancel.load(Ordering::SeqCst) {
        return Err(anyhow!("查询已被用户取消"));
    }
    tokio::pin!(fut);
    let outcome = tokio::time::timeout(MAX_QUERY_DURATION, async {
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
            MAX_QUERY_DURATION.as_secs()
        )),
        Ok(None) => Err(anyhow!("查询已被用户取消")),
        Ok(Some(Ok(r))) => Ok(r),
        Ok(Some(Err(e))) => Err(e.into()),
    }
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
            crate::core::error::translate(&e)
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
) -> Result<usize> {
    // 前端生成的语句使用不带库名的表名，统一在事务连接上切换库
    let use_sql = format!("USE `{}`", database);
    Executor::execute(&mut *txn, use_sql.as_str())
        .await
        .map_err(|e| anyhow!("切换数据库失败：{}", crate::core::error::translate(&e)))?;

    let mut unchanged = 0usize;
    // 同一次保存中的表名去重，避免每行变更都查一次元数据
    let mut checked_tables: HashSet<String> = HashSet::new();
    for (index, change) in changes.iter().enumerate() {
        let position = index + 1;
        validate_identifier(&change.table)
            .map_err(|e| anyhow!("第 {} 条变更的表名不合法：{}", position, e))?;

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
                    crate::core::error::translate(&e)
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
                                    crate::core::error::translate(&e)
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

impl ConnectionManager {
    /// 执行SQL
    pub async fn execute(&self, connection_id: &str, database: &str, sql: &str) -> Result<QueryResult> {
        // 输入验证
        validate_identifier(database)
            .map_err(|e| anyhow!("数据库名验证失败: {}", e))?;

        // 验证 SQL 长度限制（防止内存攻击）
        const MAX_SQL_LENGTH: usize = 10_000_000; // 10MB
        if sql.len() > MAX_SQL_LENGTH {
            return Err(anyhow!("SQL 长度不能超过 10MB"));
        }

        let connections = self.connections.read().await;

        let conn = connections
            .get(connection_id)
            .ok_or_else(|| anyhow!("Connection {} not found", connection_id))?;

        // 访问模式守卫：只读模式拒绝非查询语句，安全模式拒绝不带 WHERE 的 UPDATE/DELETE
        crate::core::sql::safety::enforce(conn.config.safety_mode, sql)?;

        // 池的克隆只是 Arc 计数增加，用于把具体驱动类型交给对应执行路径
        let pool = conn
            .pool
            .as_ref()
            .ok_or_else(|| anyhow!("Connection {} not available", connection_id))?
            .clone();

        // 复位上一次的取消标志，确保新查询不被旧取消信号误杀
        conn.query_cancel.store(false, Ordering::SeqCst);

        match pool {
            AnyPool::MySql(pool) => self.execute_mysql(conn, &pool, connection_id, database, sql).await,
            AnyPool::Postgres(pool) => self.execute_postgres(conn, &pool, connection_id, database, sql).await,
        }
    }

    /// MySQL 执行路径（原有逻辑）
    async fn execute_mysql(
        &self,
        conn: &ManagedConnection,
        pool: &MySqlPool,
        connection_id: &str,
        database: &str,
        sql: &str,
    ) -> Result<QueryResult> {
        let t_start = Instant::now();

        // 在 SQL 中为表名添加数据库前缀（替换表名引用为 db.table 格式）。
        // 返回的 bool 表示 sqlparser 是否成功解析：标准 DML/DDL 语句会成功解析，
        // 表名已带数据库前缀，无需再执行 USE；SHOW/DESC/CALL 等 MySQL 特有语法解析失败，需先 USE。
        let (modified_sql, parsed) = rewrite_table_names(database, sql, DriverKind::MySql);
        let t_rewrite = Instant::now();

        // 判断是否为不支持预处理协议的语句（DDL 类）
        let upper_sql = modified_sql.trim().to_uppercase();
        let is_ddl = upper_sql.starts_with("ALTER DATABASE")
            || upper_sql.starts_with("CREATE DATABASE")
            || upper_sql.starts_with("DROP DATABASE")
            || upper_sql.starts_with("ALTER USER")
            || upper_sql.starts_with("CREATE USER")
            || upper_sql.starts_with("DROP USER")
            || upper_sql.starts_with("GRANT")
            || upper_sql.starts_with("REVOKE")
            || upper_sql.starts_with("SET ")
            || upper_sql.starts_with("USE ");

        // 判断是否为 SHOW/DESC 语句（需要数据库上下文）
        let needs_db_context = upper_sql.starts_with("SHOW ") || upper_sql.starts_with("DESCRIBE ") || upper_sql.starts_with("DESC ");

        // 判断是否为 DML 语句：走 execute 拿真实影响行数（此前走 fetch_all 恒返回 0）
        let is_dml = upper_sql.starts_with("INSERT")
            || upper_sql.starts_with("UPDATE")
            || upper_sql.starts_with("DELETE")
            || upper_sql.starts_with("REPLACE");

        // 选择执行目标，保证「USE 与查询在同一物理连接」：
        // - 事务进行中：钉扎在事务连接上执行；
        // - 需要会话上下文（解析失败或 SHOW/DESC）：从池中取一条固定连接承载 USE + 查询，并发执行时各取各的连接、互不干扰；
        // - 其余无状态语句：直接在池上执行。
        let need_session = !parsed || needs_db_context;
        let mut txn_guard = conn.transaction.lock().await;
        let mut acquired: Option<sqlx::pool::PoolConnection<MySql>> = None;
        if txn_guard.is_none() && need_session {
            acquired = Some(pool.acquire().await.context("获取数据库连接失败")?);
        }
        // 统一「固定单条物理连接」：事务进行中优先钉扎事务连接，否则为会话上下文取出的连接
        let mut session_conn: Option<&mut MySqlConnection> = match txn_guard.as_mut() {
            Some(txn) => Some(txn.as_mysql_connection()?),
            None => acquired.as_mut().map(|c| &mut **c),
        };
        if need_session {
            let use_sql = format!("USE `{}`", database);
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

        // SELECT 等需要结果集的语句：使用 raw_sql（文本协议）执行，避免预处理语句额外的 COM_STMT_PREPARE 往返
        let result = match session_conn.as_mut() {
            Some(c) => {
                let fut = Executor::fetch_all(&mut **c, modified_sql.as_str());
                run_query(fut, &conn.query_cancel, &conn.query_notify).await?
            }
            None => {
                let fut = sqlx::raw_sql(&modified_sql).fetch_all(pool);
                run_query(fut, &conn.query_cancel, &conn.query_notify).await?
            }
        };
        let t_fetch = Instant::now();

        let execution_time_ms = t_start.elapsed().as_millis() as u64;

        if result.is_empty() {
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
        let columns: Vec<ColumnDefinition> = result[0]
            .columns()
            .iter()
            .map(|col| ColumnDefinition {
                name: col.name().to_string(),
                data_type: col.type_info().to_string(),
                is_nullable: true,
                is_primary: false,
                is_auto_increment: false,
            })
            .collect();
        let t_columns = Instant::now();

        // 列级预分类：每列只做一次类型匹配，避免在逐单元格转换时重复小写化与字符串比较
        let kinds: Vec<ColumnKind> = columns.iter().map(|c| classify_column(&c.data_type)).collect();

        // 转换数据（超出上限即截断，避免全量进内存/全量 JSON 过 IPC）
        let total_rows = result.len();
        let truncated = total_rows > MAX_RESULT_ROWS;
        let mut rows = vec![];
        for row in result.iter().take(MAX_RESULT_ROWS) {
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

    /// PostgreSQL 执行路径
    ///
    /// 与 MySQL 路径的差异：
    /// - 不做库名前缀重写（PG 的 database 与 schema 是两层）；
    /// - `SET search_path` 是会话级设置，必须与查询跑在同一条物理连接上，因此始终钉扎一条连接；
    /// - 空结果集没有列元数据，而 sqlx-postgres 未实现 `Describe for &str`，改用 `prepare`
    ///   （只解析、不执行）取列头。
    async fn execute_postgres(
        &self,
        conn: &ManagedConnection,
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
            None => &mut **acquired.as_mut().expect("连接刚刚获取"),
        };

        // PG 的 database 在连接时已固定、无法切换；`database` 参数取自侧栏选择，
        // 只有它与连接库不同时才按 schema 处理并设置 search_path，
        // 否则会把库名当成 schema 覆盖掉服务端的默认 search_path。
        // 空串表示「未选择库」（SQL 编辑器在 PG 下的默认值），保持服务端默认 search_path。
        if !database.is_empty() && conn.config.database.as_deref() != Some(database) {
            let set_sql = DriverKind::Postgres.switch_database_sql(database);
            Executor::execute(&mut *pg_conn, set_sql.as_str())
                .await
                .map_err(|e| anyhow!("切换模式失败：{}", crate::core::error::translate(&e)))?;
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

        let result = {
            let fut = Executor::fetch_all(&mut *pg_conn, modified_sql.as_str());
            run_query(fut, &conn.query_cancel, &conn.query_notify).await?
        };
        let t_fetch = Instant::now();

        let execution_time_ms = t_start.elapsed().as_millis() as u64;

        if result.is_empty() {
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

        let columns: Vec<ColumnDefinition> = result[0]
            .columns()
            .iter()
            .map(|col| ColumnDefinition {
                name: col.name().to_string(),
                data_type: col.type_info().to_string(),
                is_nullable: true,
                is_primary: false,
                is_auto_increment: false,
            })
            .collect();
        let t_columns = Instant::now();

        let kinds: Vec<PgColumnKind> = columns.iter().map(|c| classify_pg_column(&c.data_type)).collect();

        let total_rows = result.len();
        let truncated = total_rows > MAX_RESULT_ROWS;
        let mut rows = vec![];
        for row in result.iter().take(MAX_RESULT_ROWS) {
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

        let connections = self.connections.read().await;
        let conn = connections
            .get(connection_id)
            .ok_or_else(|| anyhow!("Connection {} not found", connection_id))?;
        let pool = conn
            .pool
            .as_ref()
            .ok_or_else(|| anyhow!("Connection {} not available", connection_id))?
            .as_mysql("保存表数据变更")?;

        // 只读连接不允许通过表数据编辑面板写入
        if conn.config.safety_mode == SafetyMode::ReadOnly {
            return Err(anyhow!("当前连接为只读模式，无法保存表数据变更"));
        }

        if changes.is_empty() {
            return Ok(ApplyChangesResult { applied: 0, unchanged: 0 });
        }

        // 先取事务连接再拿事务锁：避免持锁等待池连接，与并发查询互相阻塞
        let mut txn = pool.begin().await.context("开始保存事务失败")?;
        let txn_guard = conn.transaction.lock().await;
        if txn_guard.is_some() {
            // 用户已手动开启事务：两套事务并发修改同一批行会互相锁住，直接拒绝
            let _ = txn.rollback().await;
            return Err(anyhow!("当前连接有未提交的事务，请先 COMMIT 或 ROLLBACK 再保存"));
        }

        let unchanged = match apply_changes_in_txn(&mut txn, database, &changes).await {
            Ok(count) => count,
            Err(e) => {
                let _ = txn.rollback().await;
                return Err(e);
            }
        };

        if let Err(e) = txn.commit().await {
            return Err(anyhow!("提交保存事务失败：{}", crate::core::error::translate(&e)));
        }
        drop(txn_guard);

        Ok(ApplyChangesResult {
            applied: changes.len(),
            unchanged,
        })
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

        let connections = self.connections.read().await;

        let conn = connections
            .get(connection_id)
            .ok_or_else(|| anyhow!("Connection {} not found", connection_id))?;

        let pool = conn
            .pool
            .as_ref()
            .ok_or_else(|| anyhow!("Connection {} not available", connection_id))?
            .as_mysql("浏览表数据")?;

        let start = Instant::now();

        // 获取主键列名
        let pk_columns: Vec<String> = sqlx::query_as::<_, (String,)>(
            "SELECT COLUMN_NAME FROM INFORMATION_SCHEMA.STATISTICS
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND INDEX_NAME = 'PRIMARY'
             ORDER BY SEQ_IN_INDEX"
        )
        .bind(database)
        .bind(table)
        .fetch_all(pool)
        .await
        .map(|v| v.into_iter().map(|(name,)| name).collect())
        .unwrap_or_default();

        // 获取列类型（含精度信息，如 varchar(255)、decimal(10,2)、int(11)）与表内列顺序
        let column_meta: Vec<(String, String)> = sqlx::query_as::<_, (String, String)>(
            "SELECT COLUMN_NAME, COLUMN_TYPE FROM INFORMATION_SCHEMA.COLUMNS
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?
             ORDER BY ORDINAL_POSITION"
        )
        .bind(database)
        .bind(table)
        .fetch_all(pool)
        .await
        .unwrap_or_default();
        let column_types: HashMap<String, String> = column_meta.iter().cloned().collect();

        // 构建查询SQL
        let mut sql = format!("SELECT ");

        match &selected_columns {
            Some(cols) => sql.push_str(&cols.join(", ")),
            None => sql.push('*'),
        }

        sql.push_str(&format!(" FROM `{}`.`{}`", database, table));

        // WHERE 子句单独构建，SELECT 与 COUNT 复用，保证带过滤时分页总数正确
        let mut where_clause = String::new();
        if let Some(filter) = &request.filter {
            where_clause.push_str(&format!(" WHERE `{}` ", filter.column));
            match filter.operator {
                FilterOperator::Eq => {
                    let val = quote_value(&filter.value)?;
                    where_clause.push_str(&format!("= {}", val));
                }
                FilterOperator::Ne => {
                    let val = quote_value(&filter.value)?;
                    where_clause.push_str(&format!("!= {}", val));
                }
                FilterOperator::Gt => {
                    let val = quote_value(&filter.value)?;
                    where_clause.push_str(&format!("> {}", val));
                }
                FilterOperator::Gte => {
                    let val = quote_value(&filter.value)?;
                    where_clause.push_str(&format!(">= {}", val));
                }
                FilterOperator::Lt => {
                    let val = quote_value(&filter.value)?;
                    where_clause.push_str(&format!("< {}", val));
                }
                FilterOperator::Lte => {
                    let val = quote_value(&filter.value)?;
                    where_clause.push_str(&format!("<= {}", val));
                }
                FilterOperator::Like => {
                    let val = quote_value(&filter.value)?;
                    where_clause.push_str(&format!("LIKE {}", val));
                }
                FilterOperator::IsNull => {
                    where_clause.push_str("IS NULL");
                }
                FilterOperator::IsNotNull => {
                    where_clause.push_str("IS NOT NULL");
                }
            }
        }
        sql.push_str(&where_clause);

        // ORDER BY - 只在排序列有效时添加
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

        // LIMIT
        let offset = (request.page - 1) * request.page_size;
        sql.push_str(&format!(" LIMIT {} OFFSET {}", request.page_size, offset));

        // 执行查询
        let result = sqlx::query(&sql).fetch_all(pool).await?;

        // 获取总行数（复用 WHERE 子句，带过滤时分页总数正确）
        let count_sql = format!(
            "SELECT COUNT(*) FROM `{}`.`{}`{}",
            database, table, where_clause
        );
        let total_count: i64 = sqlx::query_scalar(&count_sql).fetch_one(pool).await?;

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

        Ok(DataQueryResponse {
            columns,
            rows,
            total_count: total_count as u64,
            page: request.page,
            page_size: request.page_size,
            execution_time_ms,
        })
    }

    /// 取消当前连接上正在执行的查询。
    ///
    /// 置位取消标志并唤醒等待中的执行：执行中的查询丢弃其 future，物理连接关闭、
    /// 服务端查询随客户端断开而终止。若此时有进行中的事务，一并丢弃（Drop 自动回滚），
    /// 避免留下一条已不可用的事务连接。
    pub async fn cancel_query(&self, connection_id: &str) -> Result<()> {
        let connections = self.connections.read().await;
        let conn = connections.get(connection_id)
            .ok_or_else(|| anyhow!("Connection not found: {}", connection_id))?;

        // 必须先置位并唤醒，再去抢事务锁：查询执行期间持有同一把锁，
        // 先抢锁会让取消请求一直阻塞到查询自然结束，取消形同失效。
        conn.query_cancel.store(true, Ordering::SeqCst);
        conn.query_notify.notify_waiters();

        // 置位后执行方会立即丢弃查询 future 并释放锁，此处的等待是毫秒级的。
        // 若有进行中的事务，取消查询会破坏其连接协议流，直接丢弃事务（自动回滚）
        let mut txn_guard = conn.transaction.lock().await;
        if txn_guard.is_some() {
            *txn_guard = None;
        }
        Ok(())
    }

    /// 读取表数据并转换为类型化的导出行。
    ///
    /// 列类型取自 INFORMATION_SCHEMA，逐单元格经 `convert_value` 读取，
    /// 保留 NULL / 数字 / 日期 / 二进制的真实语义，供 CSV、JSON 与 Excel 导出复用。
    pub async fn fetch_table_export(
        &self,
        connection_id: &str,
        database: &str,
        table: &str,
    ) -> Result<TableExportData> {
        validate_identifier(database)
            .map_err(|e| anyhow!("数据库名验证失败: {}", e))?;
        validate_identifier(table)
            .map_err(|e| anyhow!("表名验证失败: {}", e))?;

        let pool = {
            let connections = self.connections.read().await;
            let conn = connections.get(connection_id)
                .ok_or_else(|| anyhow!("Connection not found: {}", connection_id))?;
            conn.pool.as_ref()
                .ok_or_else(|| anyhow!("Connection not established"))?
                .as_mysql("导出表数据")?
                .clone()
        };

        let mut conn = pool.acquire().await
            .map_err(|e| anyhow!("Failed to acquire connection: {}", e))?;

        // 列名与类型以 INFORMATION_SCHEMA 为准，顺序与 SELECT * 一致
        let column_meta: Vec<(String, String)> = sqlx::query_as(
            "SELECT COLUMN_NAME, DATA_TYPE FROM INFORMATION_SCHEMA.COLUMNS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION"
        )
        .bind(database)
        .bind(table)
        .fetch_all(&mut *conn).await
        .unwrap_or_default();

        let use_db = format!("USE `{}`", database);
        conn.execute(use_db.as_str()).await
            .map_err(|e| anyhow!("Failed to use database: {}", e))?;

        let query = format!("SELECT * FROM `{}`", table);
        let rows = sqlx::query(&query).fetch_all(&mut *conn).await
            .map_err(|e| anyhow!("Query failed: {}", e))?;

        // 元数据缺失时退回结果集自身的列信息
        let columns: Vec<ExportColumn> = if !column_meta.is_empty() {
            column_meta.into_iter()
                .map(|(name, data_type)| ExportColumn { name, data_type })
                .collect()
        } else {
            rows.first()
                .map(|row| row.columns().iter().map(|c| ExportColumn {
                    name: c.name().to_string(),
                    data_type: c.type_info().to_string(),
                }).collect())
                .unwrap_or_default()
        };

        let kinds: Vec<ColumnKind> = columns.iter()
            .map(|col| classify_column(&col.data_type))
            .collect();

        let mut typed_rows = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut values = Vec::with_capacity(kinds.len());
            for (index, kind) in kinds.iter().enumerate() {
                values.push(convert_value(row, index, *kind)?);
            }
            typed_rows.push(values);
        }

        Ok(TableExportData { columns, rows: typed_rows })
    }

    /// 导出表数据
    pub async fn export_table_data(
        &self,
        connection_id: &str,
        database: &str,
        table: &str,
        format: &str,
    ) -> Result<String> {
        // 验证导出格式（Excel 走独立命令，此处仅返回文本）
        let allowed_formats = ["csv", "sql", "json"];
        if !allowed_formats.contains(&format.to_lowercase().as_str()) {
            return Err(anyhow!("不支持的导出格式: {}", format));
        }

        match format.to_lowercase().as_str() {
            "csv" => {
                let data = self.fetch_table_export(connection_id, database, table).await?;
                Ok(export::to_csv(&data))
            }
            "json" => {
                let data = self.fetch_table_export(connection_id, database, table).await?;
                export::to_json(&data)
            }
            "sql" => {
                // 输入验证
                validate_identifier(database)
                    .map_err(|e| anyhow!("数据库名验证失败: {}", e))?;
                validate_identifier(table)
                    .map_err(|e| anyhow!("表名验证失败: {}", e))?;

                let connections = self.connections.read().await;
                let conn = connections.get(connection_id)
                    .ok_or_else(|| anyhow!("Connection not found: {}", connection_id))?;

                let pool = conn.pool.as_ref()
                    .ok_or_else(|| anyhow!("Connection not established"))?
                    .as_mysql("导出表数据")?;

                // 从连接池获取单个连接，确保 USE 语句和查询在同一个连接上执行
                let mut conn = pool.acquire().await
                    .map_err(|e| anyhow!("Failed to acquire connection: {}", e))?;

                // 使用当前数据库 - USE 语句不支持预处理协议
                let use_db = format!("USE `{}`", database);
                conn.execute(use_db.as_str()).await
                    .map_err(|e| anyhow!("Failed to use database: {}", e))?;

                // 查询数据
                let query = format!("SELECT * FROM `{}`", table);
                let rows = sqlx::query(&query).fetch_all(&mut *conn).await
                    .map_err(|e| anyhow!("Query failed: {}", e))?;

                let columns: Vec<String> = rows.first()
                    .map(|row| row.columns().iter().map(|c| c.name().to_string()).collect())
                    .unwrap_or_default();

                let mut sql = String::new();
                // 获取列类型信息
                let column_types: Vec<(String, String)> = match sqlx::query_as(
                    "SELECT COLUMN_NAME, DATA_TYPE FROM INFORMATION_SCHEMA.COLUMNS 
                     WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION"
                )
                .bind(table)
                .fetch_all(&mut *conn).await {
                    Ok(types) => types,
                    Err(_) => columns.iter().map(|c| (c.clone(), "varchar".to_string())).collect(),
                };

                for row in &rows {
                    let values: Vec<String> = column_types.iter().map(|(col_name, data_type)| {
                        let val = format_sql_value(row, col_name, data_type);
                        if val == "NULL" {
                            format!("`{}` = NULL", col_name)
                        } else {
                            format!("`{}` = {}", col_name, val)
                        }
                    }).collect();
                    sql.push_str(&format!("INSERT INTO `{}` SET {};\n", table, values.join(", ")));
                }
                Ok(sql)
            }
            _ => Err(anyhow!("Unsupported export format: {}", format)),
        }
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

        let connections = self.connections.read().await;
        let conn = connections.get(connection_id)
            .ok_or_else(|| anyhow!("Connection not found: {}", connection_id))?;

        let pool = conn.pool.as_ref()
            .ok_or_else(|| anyhow!("Connection not established"))?
            .as_mysql("导入数据")?;

        // 只读连接不允许导入数据
        if conn.config.safety_mode == SafetyMode::ReadOnly {
            return Err(anyhow!("当前连接为只读模式，无法导入数据"));
        }

        // 从连接池获取单个连接，确保 USE 语句和所有导入操作在同一个连接上执行
        let mut conn = pool.acquire().await
            .map_err(|e| anyhow!("Failed to acquire connection: {}", e))?;

        // 使用当前数据库 - USE 语句不支持预处理协议
        let use_db = format!("USE `{}`", database);
        conn.execute(use_db.as_str()).await
            .map_err(|e| anyhow!("Failed to use database: {}", e))?;

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

                let columns_str: String = headers.iter()
                    .map(|h| format!("`{}`", escape_string(h)))
                    .collect::<Vec<_>>()
                    .join(", ");

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

                    let values_str: String = record.iter().map(|field| {
                        // 未加引号的空字段与 NULL 视为 SQL NULL；带引号的一律按文本处理
                        if !field.quoted && (field.value.is_empty() || field.value == "NULL") {
                            "NULL".to_string()
                        } else {
                            format!("'{}'", escape_string(&field.value))
                        }
                    }).collect::<Vec<_>>().join(", ");

                    let query = format!("INSERT INTO `{}` ({}) VALUES ({})", table, columns_str, values_str);
                    sqlx::query(&query).execute(&mut *conn).await
                        .map_err(|e| anyhow!("Insert failed: {}", e))?;
                    count += 1;
                }
            }
            "sql" => {
                // 执行 SQL 语句 - 按分号分割完整的 SQL 语句
                let statements = split_sql_statements(data);
                for stmt in statements {
                    let stmt = stmt.trim();
                    if stmt.is_empty() || stmt.starts_with("--") {
                        continue;
                    }
                    sqlx::query(stmt).execute(&mut *conn).await
                        .map_err(|e| anyhow!("SQL execution failed: {}\nStatement: {}", e, stmt))?;
                    count += 1;
                }
            }
            _ => return Err(anyhow!("Unsupported import format: {}", format)),
        }

        Ok(count)
    }

    /// 导出数据库SQL
    pub async fn export_database_sql(
        &self,
        connection_id: &str,
        database: &str,
    ) -> Result<String> {
        tracing::info!("Starting export_database_sql for {}.{}", connection_id, database);

        // 输入验证
        validate_identifier(database)
            .map_err(|e| anyhow!("数据库名验证失败: {}", e))?;

        // 获取 pool 后立即释放 connections 锁
        tracing::info!("Getting pool for connection: {}", connection_id);
        let pool = {
            let connections = self.connections.read().await;
            let conn = connections.get(connection_id)
                .ok_or_else(|| anyhow!("Connection not found: {}", connection_id))?;
            let pool = conn.pool.as_ref()
                .ok_or_else(|| anyhow!("Connection not established"))?
                .as_mysql("导出数据库")?
                .clone();
            tracing::info!("Got pool, releasing connections lock");
            pool
        };

        tracing::info!("Fetching tables for database: {}", database);

        // 获取所有表 - 添加超时防止卡住
        let tables: Vec<(String,)> = match tokio::time::timeout(
            std::time::Duration::from_secs(10),
            sqlx::query_as(
                "SELECT TABLE_NAME FROM INFORMATION_SCHEMA.TABLES 
                 WHERE TABLE_SCHEMA = ? AND TABLE_TYPE = 'BASE TABLE'
                 ORDER BY TABLE_NAME ASC",
            )
            .bind(database)
            .fetch_all(&pool)
        ).await {
            Ok(result) => {
                tracing::info!("Tables query succeeded");
                result?
            },
            Err(_) => {
                tracing::error!("Tables query timed out");
                return Err(anyhow!("获取表列表超时"))
            },
        };

        tracing::info!("Found {} tables", tables.len());

        let mut sql_parts: Vec<String> = vec![
            format!("-- Database: {}\n", database),
            format!("-- Export Time: {}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")),
            "SET FOREIGN_KEY_CHECKS=0;\n".to_string(),
        ];

        for (table_name,) in tables {
            tracing::info!("Exporting table: {}", table_name);

            // 获取建表语句 - 添加超时
            let create_result: Result<(String, String), _> = match tokio::time::timeout(
                std::time::Duration::from_secs(10),
                sqlx::query_as(&format!("SHOW CREATE TABLE `{}`.`{}`", database, table_name))
                    .fetch_one(&pool)
            ).await {
                Ok(res) => res,
                Err(_) => {
                    tracing::error!("SHOW CREATE TABLE timeout for {}", table_name);
                    continue;
                }
            };

            if let Ok((_, create_stmt)) = create_result {
                sql_parts.push(format!("\n-- Table structure for `{}`\n", table_name));
                sql_parts.push(format!("DROP TABLE IF EXISTS `{}`;\n", table_name));
                sql_parts.push(format!("{};\n", create_stmt));

                tracing::info!("Fetching data for table: {}", table_name);

                // 先获取列信息
                let column_info: Vec<(String, String)> = match tokio::time::timeout(
                    std::time::Duration::from_secs(10),
                    sqlx::query_as(
                        "SELECT COLUMN_NAME, DATA_TYPE 
                         FROM INFORMATION_SCHEMA.COLUMNS 
                         WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?
                         ORDER BY ORDINAL_POSITION",
                    )
                    .bind(database)
                    .bind(&table_name)
                    .fetch_all(&pool)
                ).await {
                    Ok(res) => res.unwrap_or_default(),
                    Err(_) => {
                        tracing::error!("Failed to get column info for {}", table_name);
                        continue;
                    }
                };

                if column_info.is_empty() {
                    tracing::warn!("No columns found for table {}", table_name);
                    continue;
                }

                // 构建 SELECT 语句，直接查询所有列
                // BIT 类型在 format_sql_value 中处理
                let select_cols: Vec<String> = column_info.iter().map(|(col_name, _)| {
                    format!("`{}`", col_name)
                }).collect();

                let columns: Vec<(String, String)> = column_info.clone();

                // 获取表数据 - 添加超时和行数限制
                let data: Vec<sqlx::mysql::MySqlRow> = match tokio::time::timeout(
                    std::time::Duration::from_secs(10),
                    sqlx::query(&format!("SELECT {} FROM `{}`.`{}` LIMIT 10000", 
                        select_cols.join(", "), database, table_name))
                        .fetch_all(&pool)
                ).await {
                    Ok(res) => res?,
                    Err(_) => {
                        tracing::error!("SELECT timeout for {}", table_name);
                        continue;
                    }
                };

                tracing::info!("Table {} has {} rows", table_name, data.len());

                if !data.is_empty() {
                    sql_parts.push(format!("\n-- Dumping data for `{}`\n", table_name));

                    tracing::info!("Table {} columns: {:?}", table_name, columns);

                    for (row_idx, row) in data.iter().enumerate() {
                        let values: Vec<String> = columns.iter().map(|(col_name, data_type)| {
                            tracing::debug!("Exporting row {} column '{}' type '{}'", row_idx, col_name, data_type);
                            format_sql_value(row, col_name, data_type)
                        }).collect();

                        sql_parts.push(format!(
                            "INSERT INTO `{}` ({}) VALUES ({});\n",
                            table_name,
                            columns.iter().map(|(c, _)| format!("`{}`", c)).collect::<Vec<_>>().join(", "),
                            values.join(", ")
                        ));
                    }
                }
            }
        }

        sql_parts.push("\nSET FOREIGN_KEY_CHECKS=1;\n".to_string());

        tracing::info!("Export completed");

        Ok(sql_parts.join(""))
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

        let connections = self.connections.read().await;
        let conn = connections.get(connection_id)
            .ok_or_else(|| anyhow!("Connection not found: {}", connection_id))?;

        let pool = conn.pool.as_ref()
            .ok_or_else(|| anyhow!("Connection not established"))?
            .as_mysql("导入 SQL")?;

        // 只读连接不允许导入 SQL
        if conn.config.safety_mode == SafetyMode::ReadOnly {
            return Err(anyhow!("当前连接为只读模式，无法导入 SQL"));
        }

        // 从连接池获取单个连接，确保 USE 语句在整个导入过程中生效
        let mut conn = pool.acquire().await
            .map_err(|e| anyhow!("Failed to acquire connection: {}", e))?;

        // 切换到目标数据库（在同一连接上执行）- USE 语句不支持预处理协议
        // 使用 execute() 方法直接执行，避免生命周期问题
        let use_db = format!("USE `{}`", database);
        conn.execute(use_db.as_str()).await
            .map_err(|e| anyhow!("Failed to use database: {}", e))?;

        // 安全分割 SQL 语句（忽略字符串字面量和注释中的分号）
        let statements = split_sql_statements(sql);

        for stmt in statements {
            let stmt = stmt.trim();
            if stmt.is_empty() {
                continue;
            }

            sqlx::query(stmt).execute(&mut *conn).await
                .map_err(|e| anyhow!("SQL execution failed: {}\nStatement: {}", e, stmt))?;
        }

        Ok(())
    }
}

/// 计算两个时间点之间的毫秒数（保留小数，用于耗时统计）。
fn ms(end: Instant, start: Instant) -> f64 {
    end.duration_since(start).as_secs_f64() * 1000.0
}
