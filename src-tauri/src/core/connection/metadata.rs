//! 元数据模块
//! 对象树、表结构、数据库信息的查询，带 TTL 缓存与超时兜底。

use super::driver::DriverKind;
use super::manager::{ConnectionManager, MetadataCacheValue};
use super::resultset::parse_column_type_precision;
use super::types::*;
use super::validation::*;
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// 元数据缓存 TTL：对象树/表结构在 TTL 内命中缓存，不重复查询 INFORMATION_SCHEMA
const METADATA_CACHE_TTL: Duration = Duration::from_secs(30);

/// 元数据查询兜底超时：单条 INFORMATION_SCHEMA 查询的上限，防止异常网络/高负载下无限挂起
const METADATA_QUERY_TIMEOUT: Duration = Duration::from_secs(30);

/// 元数据查询兜底：包一层超时，把超时与底层错误统一转为 anyhow 错误（不吞错）
pub(super) async fn with_metadata_timeout<T, E, F>(fut: F) -> Result<T>
where
    E: Into<anyhow::Error>,
    F: std::future::Future<Output = std::result::Result<T, E>>,
{
    match tokio::time::timeout(METADATA_QUERY_TIMEOUT, fut).await {
        Err(_) => Err(anyhow!(
            "元数据查询超时（{} 秒），请检查网络或数据库负载",
            METADATA_QUERY_TIMEOUT.as_secs()
        )),
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(e.into()),
    }
}

/// PG 对象的展示名：public 下直出表名，其他 schema 加前缀（树节点以此为 key，须同库唯一）
fn pg_display_name(schema: &str, name: &str) -> String {
    if schema == "public" {
        name.to_string()
    } else {
        format!("{}.{}", schema, name)
    }
}

/// 构造 PG 表/视图节点：展示名同时作为后续浏览数据时的表标识
fn pg_object(database: &str, schema: &str, name: &str, object_type: ObjectType) -> DatabaseObject {
    let display = pg_display_name(schema, name);
    DatabaseObject {
        id: format!("{}.{}", database, display),
        name: display,
        object_type,
        schema: Some(schema.to_string()),
        children: None,
        auto_increment: None,
        updated_at: None,
        size_bytes: None,
    }
}

impl ConnectionManager {
    /// 获取数据库对象树
    pub async fn get_object_tree(
        &self,
        connection_id: &str,
        database: Option<&str>,
    ) -> Result<Vec<DatabaseObject>> {
        // PG 的第一层是真实数据库（而非 schema），需要跨库建连，走独立实现
        if self.driver_of(connection_id).await? == DriverKind::Postgres {
            return self.get_postgres_object_tree(connection_id, database).await;
        }

        let connections = self.connections.read().await;

        let conn = connections
            .get(connection_id)
            .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;

        let cache_key = match database {
            Some(db) => format!("tree:mysql:db:{}", db),
            None => "tree:mysql:root".to_string(),
        };
        if let Some((at, MetadataCacheValue::ObjectTree(objects))) =
            conn.metadata_cache.lock().await.get(&cache_key)
        {
            if at.elapsed() < METADATA_CACHE_TTL {
                return Ok(objects.clone());
            }
        }

        let pool = conn
            .pool
            .as_ref()
            .ok_or_else(|| anyhow!("连接不可用: {}", connection_id))?
            .as_mysql("获取对象树")?;

        let mut objects = vec![];

        if let Some(db) = database {
            // 获取表 - 按字母顺序排序，顺带取自增值/修改时间/占用空间供中间面板展示
            let tables: Vec<(String, Option<i64>, Option<String>, Option<i64>)> =
                with_metadata_timeout(sqlx::query_as(
                    "SELECT TABLE_NAME,
                            CAST(AUTO_INCREMENT AS SIGNED),
                            CAST(UPDATE_TIME AS CHAR),
                            CAST(COALESCE(DATA_LENGTH, 0) + COALESCE(INDEX_LENGTH, 0) AS SIGNED)
                     FROM INFORMATION_SCHEMA.TABLES
                     WHERE TABLE_SCHEMA = ? AND TABLE_TYPE = 'BASE TABLE'
                     ORDER BY TABLE_NAME ASC",
                )
                .bind(db)
                .fetch_all(pool))
                .await?;

            for (table_name, auto_increment, updated_at, size_bytes) in tables {
                objects.push(DatabaseObject {
                    id: format!("{}.{}", db, table_name),
                    name: table_name,
                    object_type: ObjectType::Table,
                    schema: Some(db.to_string()),
                    children: None,
                    auto_increment,
                    updated_at,
                    size_bytes,
                });
            }

            // 获取视图 - 按字母顺序排序
            let views: Vec<(String,)> = with_metadata_timeout(sqlx::query_as(
                "SELECT TABLE_NAME FROM INFORMATION_SCHEMA.TABLES 
                 WHERE TABLE_SCHEMA = ? AND TABLE_TYPE = 'VIEW'
                 ORDER BY TABLE_NAME ASC",
            )
            .bind(db)
            .fetch_all(pool))
            .await?;

            for (view_name,) in views {
                objects.push(DatabaseObject {
                    id: format!("{}.{}", db, view_name),
                    name: view_name,
                    object_type: ObjectType::View,
                    schema: Some(db.to_string()),
                    children: None,
                    auto_increment: None,
                    updated_at: None,
                    size_bytes: None,
                });
            }
        } else {
            // 获取数据库列表 - 按字母顺序排序
            let databases: Vec<(String,)> = with_metadata_timeout(sqlx::query_as(
                "SELECT SCHEMA_NAME FROM INFORMATION_SCHEMA.SCHEMATA 
                 WHERE SCHEMA_NAME NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys')
                 ORDER BY SCHEMA_NAME ASC",
            )
            .fetch_all(pool))
            .await?;

            for (db_name,) in databases {
                objects.push(DatabaseObject {
                    id: db_name.clone(),
                    name: db_name,
                    object_type: ObjectType::Database,
                    schema: None,
                    children: None,
                    auto_increment: None,
                    updated_at: None,
                    size_bytes: None,
                });
            }
        }

        conn.metadata_cache.lock().await.insert(
            cache_key,
            (Instant::now(), MetadataCacheValue::ObjectTree(objects.clone())),
        );

        Ok(objects)
    }

    /// PostgreSQL 对象树
    ///
    /// 第一层是数据库（`pg_database`）；展开某个库时切换到该库的连接池，跨用户 schema 汇总
    /// 表/视图。展示名对 public 直出表名、其余加 `schema.` 前缀（保证同一库内唯一，树节点以
    /// 展示名作为后续查询的标识）。
    async fn get_postgres_object_tree(
        &self,
        connection_id: &str,
        database: Option<&str>,
    ) -> Result<Vec<DatabaseObject>> {
        let cache_key = match database {
            Some(db) => format!("tree:postgres:db:{}", db),
            None => "tree:postgres:root".to_string(),
        };

        let cached = {
            let connections = self.connections.read().await;
            let conn = connections
                .get(connection_id)
                .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;
            let cached = conn
                .metadata_cache
                .lock()
                .await
                .get(&cache_key)
                .and_then(|(at, value)| match value {
                    MetadataCacheValue::ObjectTree(objects) if at.elapsed() < METADATA_CACHE_TTL => {
                        Some(objects.clone())
                    }
                    _ => None,
                });
            cached
        };
        if let Some(objects) = cached {
            return Ok(objects);
        }

        let mut objects = vec![];

        match database {
            Some(db) => {
                let pool = self.pg_pool_for(connection_id, db).await?;

                // table_type 分两段查询，保持「表在前、视图在后」的顺序
                let tables: Vec<(String, String)> = with_metadata_timeout(sqlx::query_as(
                    "SELECT table_schema, table_name FROM information_schema.tables
                     WHERE table_schema NOT IN ('pg_catalog', 'information_schema')
                       AND table_schema NOT LIKE 'pg\\_%'
                       AND table_type = 'BASE TABLE'
                     ORDER BY (table_schema = 'public') DESC, table_schema ASC, table_name ASC",
                )
                .fetch_all(&pool))
                .await?;

                for (schema, table_name) in tables {
                    objects.push(pg_object(db, &schema, &table_name, ObjectType::Table));
                }

                let views: Vec<(String, String)> = with_metadata_timeout(sqlx::query_as(
                    "SELECT table_schema, table_name FROM information_schema.tables
                     WHERE table_schema NOT IN ('pg_catalog', 'information_schema')
                       AND table_schema NOT LIKE 'pg\\_%'
                       AND table_type = 'VIEW'
                     ORDER BY (table_schema = 'public') DESC, table_schema ASC, table_name ASC",
                )
                .fetch_all(&pool))
                .await?;

                for (schema, view_name) in views {
                    objects.push(pg_object(db, &schema, &view_name, ObjectType::View));
                }
            }
            None => {
                // 第一层为真实数据库：排除模板库与不允许连接的库
                let (pool, _) = self.pg_main_pool(connection_id).await?;
                let databases: Vec<(String,)> = with_metadata_timeout(sqlx::query_as(
                    "SELECT datname FROM pg_database
                     WHERE datistemplate = false AND datallowconn = true
                     ORDER BY datname ASC",
                )
                .fetch_all(&pool))
                .await?;

                for (db_name,) in databases {
                    objects.push(DatabaseObject {
                        id: db_name.clone(),
                        name: db_name,
                        object_type: ObjectType::Database,
                        schema: None,
                        children: None,
                        auto_increment: None,
                        updated_at: None,
                        size_bytes: None,
                    });
                }
            }
        }

        let connections = self.connections.read().await;
        if let Some(conn) = connections.get(connection_id) {
            conn.metadata_cache.lock().await.insert(
                cache_key,
                (Instant::now(), MetadataCacheValue::ObjectTree(objects.clone())),
            );
        }

        Ok(objects)
    }

    /// 获取数据库 schema 快照（表/视图 + 列），供 SQL 编辑器补全使用
    ///
    /// 只发一条 INFORMATION_SCHEMA 查询，避免按表逐个拉列造成 N+1；
    /// 结果按「连接 + 数据库」缓存，与对象树/表结构共用 TTL。
    pub async fn get_schema_snapshot(
        &self,
        connection_id: &str,
        database: &str,
    ) -> Result<SchemaSnapshot> {
        validate_identifier(database)
            .map_err(|e| anyhow!("数据库名验证失败: {}", e))?;

        let connections = self.connections.read().await;
        let conn = connections
            .get(connection_id)
            .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;

        let pool = conn
            .pool
            .as_ref()
            .ok_or_else(|| anyhow!("连接尚未建立"))?
            .as_mysql("Schema 快照")?;

        let cache_key = format!("snapshot:db:{}", database);
        if let Some((at, MetadataCacheValue::SchemaSnapshot(snapshot))) =
            conn.metadata_cache.lock().await.get(&cache_key)
        {
            if at.elapsed() < METADATA_CACHE_TTL {
                return Ok(snapshot.clone());
            }
        }

        let rows = with_metadata_timeout(
            sqlx::query_as::<_, (String, String, Option<String>, Option<String>)>(
                "SELECT t.TABLE_NAME, t.TABLE_TYPE, c.COLUMN_NAME, c.DATA_TYPE
                 FROM INFORMATION_SCHEMA.TABLES t
                 LEFT JOIN INFORMATION_SCHEMA.COLUMNS c
                   ON c.TABLE_SCHEMA = t.TABLE_SCHEMA AND c.TABLE_NAME = t.TABLE_NAME
                 WHERE t.TABLE_SCHEMA = ? AND t.TABLE_TYPE IN ('BASE TABLE', 'VIEW')
                 ORDER BY t.TABLE_NAME ASC, c.ORDINAL_POSITION ASC",
            )
            .bind(database)
            .fetch_all(pool),
        )
        .await?;

        // 按表名聚合列；用索引表而不是比较相邻行，避免大小写不敏感排序下同名表被拆开
        let mut tables: Vec<SchemaTableInfo> = Vec::new();
        let mut index_by_name: HashMap<String, usize> = HashMap::new();
        for (table_name, table_type, column_name, data_type) in rows {
            let idx = match index_by_name.get(&table_name) {
                Some(idx) => *idx,
                None => {
                    let idx = tables.len();
                    tables.push(SchemaTableInfo {
                        name: table_name.clone(),
                        object_type: if table_type == "VIEW" {
                            ObjectType::View
                        } else {
                            ObjectType::Table
                        },
                        columns: Vec::new(),
                    });
                    index_by_name.insert(table_name, idx);
                    idx
                }
            };

            if let (Some(column_name), Some(data_type)) = (column_name, data_type) {
                tables[idx].columns.push(SchemaColumnInfo {
                    name: column_name,
                    data_type,
                });
            }
        }

        let snapshot = SchemaSnapshot {
            database: database.to_string(),
            tables,
        };

        conn.metadata_cache.lock().await.insert(
            cache_key,
            (
                Instant::now(),
                MetadataCacheValue::SchemaSnapshot(snapshot.clone()),
            ),
        );

        Ok(snapshot)
    }

    /// 获取表结构信息
    pub async fn get_table_structure(
        &self,
        connection_id: &str,
        database: &str,
        table: &str,
    ) -> Result<TableStructure> {
        // 输入验证
        validate_identifier(database)
            .map_err(|e| anyhow!("数据库名验证失败: {}", e))?;
        validate_identifier(table)
            .map_err(|e| anyhow!("表名验证失败: {}", e))?;

        let connections = self.connections.read().await;
        let conn = connections.get(connection_id)
            .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;

        let pool = conn.pool.as_ref()
            .ok_or_else(|| anyhow!("连接尚未建立"))?
            .as_mysql("表结构")?;

        // 元数据缓存：表结构按「数据库.表」区分
        let cache_key = format!("table:{}:{}", database, table);
        if let Some((at, MetadataCacheValue::TableStructure(s))) =
            conn.metadata_cache.lock().await.get(&cache_key)
        {
            if at.elapsed() < METADATA_CACHE_TTL {
                return Ok(s.clone());
            }
        }

        // 获取表的基本信息
        let table_info = with_metadata_timeout(sqlx::query_as::<_, (String, Option<String>, Option<String>, Option<String>, Option<String>, Option<String>, String)>(
            "SELECT TABLE_NAME, ENGINE, TABLE_COLLATION, TABLE_COMMENT, 
             CAST(CREATE_TIME AS CHAR), CAST(UPDATE_TIME AS CHAR), CAST(TABLE_ROWS AS CHAR)
             FROM INFORMATION_SCHEMA.TABLES
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?"
        )
        .bind(database)
        .bind(table)
        .fetch_one(pool))
        .await?;

        // 获取列信息
        // 使用 CAST 将数值转为字符串，兼容不同 MySQL 版本
        let columns = with_metadata_timeout(sqlx::query_as::<_, (String, String, String, String, Option<String>, String, Option<String>, String, String)>(
            "SELECT COLUMN_NAME, DATA_TYPE, COLUMN_TYPE, IS_NULLABLE, COLUMN_DEFAULT,
                    EXTRA, COLUMN_COMMENT, CAST(COALESCE(CHARACTER_MAXIMUM_LENGTH, 0) AS CHAR), CAST(ORDINAL_POSITION AS CHAR)
             FROM INFORMATION_SCHEMA.COLUMNS
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?
             ORDER BY ORDINAL_POSITION"
        )
        .bind(database)
        .bind(table)
        .fetch_all(pool))
        .await?;

        let column_info: Vec<TableColumnInfo> = columns.iter().map(|col| {
            let is_auto_increment = col.5.to_lowercase().contains("auto_increment");
            let on_update_current_timestamp = col.5.to_lowercase().contains("on update");

            // 从 COLUMN_TYPE（如 decimal(10,2)、int(11) unsigned）解析精度/小数位/显示宽度。
            let data_type = col.1.clone();
            let lower_type = data_type.to_lowercase();
            let has_precision = matches!(
                lower_type.as_str(),
                "decimal" | "numeric" | "float" | "double" | "real"
            );
            let is_integer = matches!(
                lower_type.as_str(),
                "tinyint" | "smallint" | "mediumint" | "int" | "integer" | "bigint"
            );

            let (length, decimals) = if has_precision || is_integer {
                parse_column_type_precision(&col.2)
            } else {
                let length = if col.7.is_empty() || col.7 == "0" {
                    None
                } else {
                    col.7.parse::<u32>().ok()
                };
                (length, None)
            };

            // 无符号标识只出现在 COLUMN_TYPE 中，如 int(11) unsigned
            let unsigned = col.2.to_lowercase().contains("unsigned");

            TableColumnInfo {
                name: col.0.clone(),
                data_type,
                length,
                decimals,
                is_nullable: col.3 == "YES",
                default_value: col.4.clone(),
                is_auto_increment,
                is_primary: false, // Will be set later from indexes
                unsigned,
                on_update_current_timestamp,
                comment: match &col.6 {
                    Some(s) if !s.is_empty() => Some(s.clone()),
                    _ => None,
                },
            }
        }).collect();

        // 获取索引信息
        // 使用 CAST 将数值转为字符串，兼容不同 MySQL 版本
        let indexes = with_metadata_timeout(sqlx::query_as::<_, (String, String, String, String, i64)>(
            "SELECT INDEX_NAME, COLUMN_NAME, INDEX_TYPE, CAST(SEQ_IN_INDEX AS CHAR), NON_UNIQUE
             FROM INFORMATION_SCHEMA.STATISTICS
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?
             ORDER BY INDEX_NAME, SEQ_IN_INDEX"
        )
        .bind(database)
        .bind(table)
        .fetch_all(pool))
        .await?;

        let mut index_map: HashMap<String, (String, Vec<String>, bool)> = HashMap::new();
        for idx in &indexes {
            let index_name = idx.0.clone();
            let col_name = idx.1.clone();
            let index_type = idx.2.clone();
            // NON_UNIQUE = 0 表示唯一索引；主键索引名固定为 PRIMARY
            let is_unique = index_name == "PRIMARY" || idx.4 == 0;

            if let Some((_, cols, _)) = index_map.get_mut(&index_name) {
                cols.push(col_name);
            } else {
                index_map.insert(index_name, (index_type, vec![col_name], is_unique));
            }
        }

        let index_info: Vec<TableIndexInfo> = index_map.into_iter().map(|(name, (ty, cols, unique))| {
            let name_clone = name.clone();
            TableIndexInfo {
                name,
                r#type: ty,
                columns: cols,
                is_unique: unique || name_clone == "PRIMARY",
            }
        }).collect();

        // 标记主键列
        let mut columns_with_pk = column_info.clone();
        for idx in &index_info {
            if idx.name == "PRIMARY" {
                for col_name in &idx.columns {
                    for col in &mut columns_with_pk {
                        if col.name == *col_name {
                            col.is_primary = true;
                        }
                    }
                }
            }
        }

        // 获取外键信息 - 从 REFERENTIAL_CONSTRAINTS 获取更新/删除规则
        // REFERENCED_TABLE_NAME IS NOT NULL 已足够识别外键，不再按约束名前缀过滤，
        // 避免漏掉命名不规范（非 fk_ 前缀）的外键约束。
        let foreign_keys = with_metadata_timeout(sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT 
                kcu.CONSTRAINT_NAME, 
                kcu.COLUMN_NAME, 
                kcu.REFERENCED_TABLE_NAME,
                kcu.REFERENCED_COLUMN_NAME
             FROM INFORMATION_SCHEMA.KEY_COLUMN_USAGE kcu
             WHERE kcu.TABLE_SCHEMA = ? 
               AND kcu.TABLE_NAME = ?
               AND kcu.REFERENCED_TABLE_NAME IS NOT NULL"
        )
        .bind(database)
        .bind(table)
        .fetch_all(pool))
        .await?;

        // 获取外键的更新/删除规则
        let mut fk_info: Vec<TableForeignKeyInfo> = Vec::new();
        for fk in &foreign_keys {
            // 尝试获取外键约束的详细信息
            let fk_detail = with_metadata_timeout(sqlx::query_as::<_, (Option<String>, Option<String>)>(
                "SELECT rc.DELETE_RULE, rc.UPDATE_RULE
                 FROM INFORMATION_SCHEMA.REFERENTIAL_CONSTRAINTS rc
                 WHERE rc.CONSTRAINT_SCHEMA = ?
                   AND rc.CONSTRAINT_NAME = ?"
            )
            .bind(database)
            .bind(&fk.0)
            .fetch_optional(pool))
            .await
            // 不能静默回落 RESTRICT：会把 CASCADE / SET NULL 误报成 RESTRICT
            .map_err(|e| anyhow!("查询外键 {} 的更新/删除规则失败: {}", fk.0, e))?;

            let (on_delete, on_update) = match fk_detail {
                Some(d) => (
                    d.0.unwrap_or_else(|| "RESTRICT".to_string()),
                    d.1.unwrap_or_else(|| "RESTRICT".to_string())
                ),
                None => ("RESTRICT".to_string(), "RESTRICT".to_string())
            };

            fk_info.push(TableForeignKeyInfo {
                name: fk.0.clone(),
                column: fk.1.clone(),
                ref_table: fk.2.clone(),
                ref_column: fk.3.clone(),
                on_delete,
                on_update,
            });
        }

        // 获取建表语句 - SHOW CREATE TABLE 返回两列: Table 和 Create Table
        let create_stmt = with_metadata_timeout(sqlx::query_as::<_, (String, String)>(
            &format!("SHOW CREATE TABLE `{}`.`{}`", database, table)
        )
        .fetch_one(pool))
        .await?;

        // 获取表大小信息
        // 使用 CAST 将数值转为字符串，兼容不同 MySQL 版本
        let size_info = with_metadata_timeout(sqlx::query_as::<_, (Option<String>, Option<String>)>(
            "SELECT CAST(DATA_LENGTH AS CHAR), CAST(INDEX_LENGTH AS CHAR)
             FROM INFORMATION_SCHEMA.TABLES
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?"
        )
        .bind(database)
        .bind(table)
        .fetch_one(pool))
        .await?;

        // 解析时间
        let parse_time = |opt: Option<String>| -> Option<String> {
            opt.and_then(|s| if s.is_empty() { None } else { Some(s) })
        };

        // 解析行数
        let row_count = if table_info.6.is_empty() { 
            None 
        } else { 
            table_info.6.parse::<u64>().ok() 
        };

        // 从排序规则解析字符集
        let charset = table_info.2.as_ref().and_then(|s| {
            if s.is_empty() { 
                None 
            } else { 
                // utf8mb4_unicode_ci -> utf8mb4
                s.split('_').next().map(|c| c.to_string())
            }
        });

        let structure = TableStructure {
            columns: columns_with_pk,
            indexes: index_info,
            foreign_keys: fk_info,
            create_statement: create_stmt.1,
            engine: table_info.1.and_then(|s| if s.is_empty() { None } else { Some(s) }),
            charset,
            collation: table_info.2.and_then(|s| if s.is_empty() { None } else { Some(s) }),
            row_count,
            data_size: size_info.0.and_then(|s| s.parse().ok()),
            index_size: size_info.1.and_then(|s| s.parse().ok()),
            create_time: parse_time(table_info.4),
            update_time: parse_time(table_info.5),
        };

        conn.metadata_cache.lock().await.insert(
            cache_key,
            (Instant::now(), MetadataCacheValue::TableStructure(structure.clone())),
        );

        Ok(structure)
    }

    /// 获取数据库信息
    pub async fn get_database_info(
        &self,
        connection_id: &str,
        database: &str,
    ) -> Result<DatabaseInfo> {
        let connections = self.connections.read().await;
        let conn = connections.get(connection_id)
            .ok_or_else(|| anyhow!("连接不存在: {}", connection_id))?;

        let pool = conn.pool.as_ref()
            .ok_or_else(|| anyhow!("连接尚未建立"))?
            .as_mysql("数据库信息")?;

        // 获取数据库基本信息
        let db_info: (Option<String>, Option<String>, Option<String>) = with_metadata_timeout(sqlx::query_as(
            "SELECT DEFAULT_CHARACTER_SET_NAME, DEFAULT_COLLATION_NAME, 
                    CAST(COALESCE((SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = ?), 0) AS CHAR)
             FROM INFORMATION_SCHEMA.SCHEMATA 
             WHERE SCHEMA_NAME = ?"
        )
        .bind(database)
        .bind(database)
        .fetch_one(pool))
        .await
        .map_err(|e| anyhow!("查询数据库信息失败: {}", e))?;

        // 获取数据库大小
        // 使用 CAST 将数值转为字符串，兼容不同 MySQL 版本
        let size_info = with_metadata_timeout(sqlx::query_as::<_, (Option<String>, Option<String>)>(
            "SELECT CAST(SUM(DATA_LENGTH) AS CHAR), CAST(SUM(INDEX_LENGTH) AS CHAR)
             FROM INFORMATION_SCHEMA.TABLES
             WHERE TABLE_SCHEMA = ?"
        )
        .bind(database)
        .fetch_one(pool))
        .await;

        // 查询失败时保持「未知」（界面显示 `-`），不能伪装成 0 字节
        let (data_size, index_size, total_size) = match size_info {
            Ok((data, index)) => {
                let data = data.and_then(|s| s.parse::<u64>().ok());
                let index = index.and_then(|s| s.parse::<u64>().ok());
                let total = match (data, index) {
                    (Some(d), Some(i)) => Some(d + i),
                    _ => None,
                };
                (data, index, total)
            }
            Err(e) => {
                tracing::warn!("查询数据库 {} 的占用空间失败：{}", database, e);
                (None, None, None)
            }
        };

        Ok(DatabaseInfo {
            name: database.to_string(),
            charset: db_info.0.filter(|s: &String| !s.is_empty()),
            collation: db_info.1.filter(|s: &String| !s.is_empty()),
            table_count: db_info.2.and_then(|s: String| s.parse().ok()).unwrap_or(0),
            data_size,
            index_size,
            total_size,
            create_time: None, // MySQL 不直接提供数据库创建时间
        })
    }
}
