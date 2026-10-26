//! 数据库完整备份与还原。
//!
//! 通过子进程调用官方客户端工具（`mysqldump` / `pg_dump` / `mysql` / `psql`），
//! 覆盖纯 SQL 导出的 10K 行上限，并天然包含 views / triggers / routines。
//!
//! 凭据处理：
//! - MySQL：写临时 `defaults-extra-file`（权限 0600），用完删除，避免密码暴露在进程列表
//! - PostgreSQL：`PGPASSWORD` 环境变量
//!
//! SSH 隧道复用：已建立的隧道绑定本地随机端口，dump/restore 工具指向 `127.0.0.1:<本地端口>`。

use super::driver::DriverKind;
use super::manager::ConnectionManager;
use super::validation::validate_identifier;
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

/// 单条备份记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupRecord {
    pub id: String,
    pub connection_id: String,
    pub database: String,
    pub driver: String,
    /// 备份文件绝对路径
    pub file_path: String,
    /// 文件大小（字节）
    pub file_size: u64,
    /// 创建时间（RFC3339 UTC）
    pub created_at: String,
    /// 用户可选标签，便于识别
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl BackupRecord {
    fn new(
        connection_id: &str,
        database: &str,
        driver: DriverKind,
        file_path: PathBuf,
        file_size: u64,
        label: Option<String>,
    ) -> Self {
        Self {
            id: format!(
                "bk_{}",
                chrono::Utc::now().format("%Y%m%d%H%M%S%3f")
            ),
            connection_id: connection_id.to_string(),
            database: database.to_string(),
            driver: driver.display_name().to_string(),
            file_path: file_path.to_string_lossy().to_string(),
            file_size,
            created_at: Utc::now().to_rfc3339(),
            label,
        }
    }
}

/// 备份历史：持久化在 `<app data dir>/mysql-client/backup_history.json`
pub struct BackupHistory {
    path: PathBuf,
}

impl BackupHistory {
    pub fn new() -> Self {
        let path = dirs::data_dir()
            .map(|dir| dir.join("mysql-client").join("backup_history.json"))
            .unwrap_or_else(|| PathBuf::from("backup_history.json"));
        Self { path }
    }

    /// 载入全部历史（空或损坏时返回空 vec）
    pub fn load(&self) -> Vec<BackupRecord> {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|txt| serde_json::from_str::<Vec<BackupRecord>>(&txt).ok())
            .unwrap_or_default()
    }

    /// 把新记录追加到历史文件
    pub fn append(&self, record: &BackupRecord) -> Result<()> {
        let mut list = self.load();
        list.push(record.clone());
        self.save(&list)
    }

    /// 按 ID 删除（同时尝试删除磁盘文件）
    pub fn remove(&self, id: &str) -> Result<()> {
        let mut list = self.load();
        let file_to_delete = list
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.file_path.clone());
        list.retain(|r| r.id != id);
        if let Some(path) = file_to_delete {
            let _ = std::fs::remove_file(&path);
        }
        self.save(&list)
    }

    /// 查询某连接某库的历史记录
    pub fn list_for_db(&self, connection_id: &str, database: &str) -> Vec<BackupRecord> {
        self.load()
            .into_iter()
            .filter(|r| r.connection_id == connection_id && r.database == database)
            .rev() // 最新在前
            .collect()
    }

    fn save(&self, list: &[BackupRecord]) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let json = serde_json::to_string_pretty(list)
            .map_err(|e| anyhow!("序列化备份历史失败：{}", e))?;
        std::fs::write(&self.path, json)
            .map_err(|e| anyhow!("写入备份历史失败：{}", e))?;
        Ok(())
    }
}

impl Default for BackupHistory {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionManager {
    /// 触发完整备份：根据驱动调用 mysqldump / pg_dump，输出到应用备份目录并登记历史。
    pub async fn backup_database(
        &self,
        connection_id: &str,
        database: &str,
        label: Option<String>,
    ) -> Result<BackupRecord> {
        // 取连接配置与隧道状态
        let (config, tunnel_port) = {
            let connections = self.connections.read().await;
            let conn = connections
                .get(connection_id)
                .ok_or_else(|| anyhow!("连接 {} 不存在或已断开", connection_id))?;
            let tunnel_port = conn.tunnel.as_ref().map(|t| t.local_port());
            (conn.config.clone(), tunnel_port)
        };

        if config.driver == DriverKind::Postgres {
            tracing::warn!("PostgreSQL 备份还原尚未实现，跳过 pg_dump 调用");
            return Err(anyhow!(
                "PostgreSQL 完整备份将在后续版本提供，请先使用导出 SQL 功能"
            ));
        }

        // 解析实际要连接的 host/port（SSH 隧道存在时走本地转发端口）
        let (host, port) = if let Some(local_port) = tunnel_port {
            ("127.0.0.1".to_string(), local_port)
        } else {
            (config.host.clone(), config.port)
        };

        // 检测 mysqldump 是否可用
        check_tool("mysqldump", "--version").await?;

        // 确保备份目录存在
        let backup_dir = dirs::data_dir()
            .map(|d| d.join("mysql-client").join("backups"))
            .unwrap_or_else(|| PathBuf::from("backups"));
        std::fs::create_dir_all(&backup_dir).ok();

        // 输出文件：库名_日期.sql
        let safe_db = database.replace(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-', "_");
        let filename = format!(
            "{}_{}.sql",
            safe_db,
            Utc::now().format("%Y%m%d_%H%M%S")
        );
        let output_path = backup_dir.join(&filename);

        // mysqldump 凭据临时文件
        let defaults_file = write_mysql_defaults(&config.username, &config.password)
            .context("创建 mysqldump 凭据临时文件失败")?;

        // 组装命令
        let status = Command::new("mysqldump")
            .arg(format!("--defaults-extra-file={}", defaults_file.display()))
            .arg("--single-transaction")
            .arg("--routines")
            .arg("--triggers")
            .arg("--events")
            .arg("--host")
            .arg(&host)
            .arg("--port")
            .arg(port.to_string())
            .arg(&config.username)
            .arg(database)
            .stdout(Stdio::from(std::fs::File::create(&output_path).map_err(|e| {
                anyhow!("创建备份输出文件失败：{}", e)
            })?))
            .stderr(Stdio::piped())
            .status()
            .await;

        // 无论成败都删除凭据临时文件
        let _ = tokio::fs::remove_file(&defaults_file).await;

        let exit = status.map_err(|e| anyhow!("启动 mysqldump 失败：{}", e))?;
        if !exit.success() {
            let stderr = read_last_stderr("mysqldump", exit.code()).await;
            // 清理半写的输出文件
            let _ = tokio::fs::remove_file(&output_path).await;
            return Err(anyhow!(
                "mysqldump 执行失败（退出码 {:?}）：{}",
                exit.code(),
                stderr,
            ));
        }

        // 读取文件大小
        let file_size = tokio::fs::metadata(&output_path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);

        if file_size == 0 {
            let _ = tokio::fs::remove_file(&output_path).await;
            return Err(anyhow!("mysqldump 未产生任何输出，备份已取消"));
        }

        let record = BackupRecord::new(
            connection_id,
            database,
            config.driver,
            output_path,
            file_size,
            label,
        );

        // 写历史
        let history = BackupHistory::new();
        history
            .append(&record)
            .unwrap_or_else(|e| tracing::warn!("写入备份历史失败：{}", e));

        Ok(record)
    }

    /// 导出整库 SQL 到用户指定路径（mysqldump 直写文件，不经内存/IPC）。
    ///
    /// 与 `backup_database` 的区别：输出路径由用户选择、不登记备份历史。
    /// 相比旧版逐表拼接 SQL 的实现，这里没有 1 万行上限，也不会在超时后静默跳过表，
    /// 并且天然包含视图、触发器、存储过程与事件。
    pub async fn export_database_sql_to_file(
        &self,
        connection_id: &str,
        database: &str,
        output_path: &str,
    ) -> Result<u64> {
        validate_identifier(database)
            .map_err(|e| anyhow!("数据库名验证失败：{}", e))?;

        let (config, tunnel_port) = {
            let connections = self.connections.read().await;
            let conn = connections
                .get(connection_id)
                .ok_or_else(|| anyhow!("连接 {} 不存在或已断开", connection_id))?;
            let tunnel_port = conn.tunnel.as_ref().map(|t| t.local_port());
            (conn.config.clone(), tunnel_port)
        };

        if config.driver == DriverKind::Postgres {
            return Err(anyhow!(
                "PostgreSQL 暂不支持整库导出，请在 SQL 编辑器中导出查询结果"
            ));
        }

        let (host, port) = if let Some(local_port) = tunnel_port {
            ("127.0.0.1".to_string(), local_port)
        } else {
            (config.host.clone(), config.port)
        };

        check_tool("mysqldump", "--version").await?;

        let output_path = PathBuf::from(output_path);
        if let Some(parent) = output_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).ok();
            }
        }

        let defaults_file = write_mysql_defaults(&config.username, &config.password)
            .context("创建 mysqldump 凭据临时文件失败")?;

        let status = Command::new("mysqldump")
            .arg(format!("--defaults-extra-file={}", defaults_file.display()))
            .arg("--single-transaction")
            .arg("--routines")
            .arg("--triggers")
            .arg("--events")
            .arg("--host")
            .arg(&host)
            .arg("--port")
            .arg(port.to_string())
            .arg(&config.username)
            .arg(database)
            .stdout(Stdio::from(std::fs::File::create(&output_path).map_err(|e| {
                anyhow!("创建导出文件失败：{}", e)
            })?))
            .stderr(Stdio::piped())
            .status()
            .await;

        let _ = tokio::fs::remove_file(&defaults_file).await;

        let exit = status.map_err(|e| anyhow!("启动 mysqldump 失败：{}", e))?;
        if !exit.success() {
            let _ = tokio::fs::remove_file(&output_path).await;
            return Err(anyhow!(
                "mysqldump 执行失败（退出码 {:?}），导出文件已删除",
                exit.code()
            ));
        }

        let file_size = tokio::fs::metadata(&output_path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);

        if file_size == 0 {
            let _ = tokio::fs::remove_file(&output_path).await;
            return Err(anyhow!("mysqldump 未产生任何输出，导出已取消"));
        }

        Ok(file_size)
    }

    /// 从 dump 文件还原数据库（MySQL 先用 stdin 管道实现）
    pub async fn restore_database(
        &self,
        connection_id: &str,
        database: &str,
        dump_path: &str,
    ) -> Result<()> {
        let (config, tunnel_port) = {
            let connections = self.connections.read().await;
            let conn = connections
                .get(connection_id)
                .ok_or_else(|| anyhow!("连接 {} 不存在或已断开", connection_id))?;
            let tunnel_port = conn.tunnel.as_ref().map(|t| t.local_port());
            (conn.config.clone(), tunnel_port)
        };

        if config.driver == DriverKind::Postgres {
            return Err(anyhow!(
                "PostgreSQL 还原将在后续版本提供"
            ));
        }

        let (host, port) = if let Some(local_port) = tunnel_port {
            ("127.0.0.1".to_string(), local_port)
        } else {
            (config.host.clone(), config.port)
        };

        check_tool("mysql", "--version").await?;

        let path = PathBuf::from(dump_path);
        if !path.is_file() {
            return Err(anyhow!("备份文件不存在：{}", dump_path));
        }

        let defaults_file = write_mysql_defaults(&config.username, &config.password)
            .context("创建 mysql 凭据临时文件失败")?;

        // 启动 mysql 进程，stdin 接备份文件
        let mut child = Command::new("mysql")
            .arg(format!("--defaults-extra-file={}", defaults_file.display()))
            .arg("--host")
            .arg(&host)
            .arg("--port")
            .arg(port.to_string())
            .arg(&config.username)
            .arg(database)
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow!("启动 mysql 客户端失败：{}", e))?;

        // 把备份文件内容写入子进程 stdin
        {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| anyhow!("无法获取 mysql 子进程 stdin"))?;
            let mut file = tokio::fs::File::open(&path)
                .await
                .map_err(|e| anyhow!("打开备份文件失败：{}", e))?;
            tokio::io::copy(&mut file, &mut stdin)
                .await
                .map_err(|e| anyhow!("向 mysql 写入备份数据失败：{}", e))?;
            // drop stdin，关闭管道让 mysql 知道输入结束
        }

        // 无论成败都删除凭据文件
        let _ = tokio::fs::remove_file(&defaults_file).await;

        let output = child
            .wait_with_output()
            .await
            .map_err(|e| anyhow!("等待 mysql 还原完成失败：{}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "mysql 还原失败（退出码 {:?}）：{}",
                output.status.code(),
                stderr.trim(),
            ));
        }

        Ok(())
    }

    /// 查询某连接某库的备份历史
    pub fn list_backup_history(
        &self,
        connection_id: &str,
        database: &str,
    ) -> Vec<BackupRecord> {
        BackupHistory::new().list_for_db(connection_id, database)
    }

    /// 从历史中删除一条记录（同时尝试清理磁盘文件）
    pub fn delete_backup_record(&self, id: &str) -> Result<()> {
        BackupHistory::new().remove(id)
    }
}

/// 写入 MySQL 凭据临时文件（权限 0600），返回文件路径
fn write_mysql_defaults(username: &str, password: &str) -> Result<PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "auroradb-mysql-defaults-{}.ini",
        std::process::id()
    ));

    let content = format!(
        "[client]\nuser={}\npassword={}\n",
        // mysqldump 对特殊字符敏感，简单起见直接写；密码通常不包含换行
        username, password
    );

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    use std::io::Write;
    let mut file = options
        .open(&path)
        .map_err(|e| anyhow!("创建凭据临时文件失败：{}", e))?;
    file.write_all(content.as_bytes())
        .map_err(|e| anyhow!("写入凭据失败：{}", e))?;

    Ok(path)
}

/// 检测外部工具是否可执行（mysqldump / mysql / pg_dump / psql）
async fn check_tool(name: &str, version_arg: &str) -> Result<()> {
    let result = Command::new(name)
        .arg(version_arg)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;

    match result {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => Ok(()), // 版本命令返回非 0 也算存在
        Err(e) => {
            let hint = if name == "mysqldump" {
                "请安装 MySQL 客户端工具（Debian/Ubuntu: apt install mysql-client-core / \
                 macOS: brew install mysql-client / Windows: 安装 MySQL Community Server 后添加到 PATH）"
            } else if name == "mysql" {
                "请安装 MySQL 客户端工具（Debian/Ubuntu: apt install mysql-client-core / \
                 macOS: brew install mysql-client / Windows: 安装 MySQL Community Server 后添加到 PATH）"
            } else {
                "请确保工具在 PATH 中"
            };
            Err(anyhow!(
                "未检测到 `{name}`（{e}）。{hint}",
                name = name,
                e = e,
                hint = hint,
            ))
        }
    }
}

/// 读取子进程 stderr 最后几行（用于错误提示）
async fn read_last_stderr(proc_name: &str, exit_code: Option<i32>) -> String {
    // 目前只返回退出码占位，因为 mysqldump 的 stderr 已经被子进程打印到终端
    // 后续可以捕获完整 stderr 并截断
    format!("{} 退出码 {:?}", proc_name, exit_code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_history_round_trip() {
        let dir = std::env::temp_dir().join(format!(
            "auroradb-backup-test-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let history_path = dir.join("backup_history.json");

        // 用临时路径替换 BackupHistory::new 的路径
        let record = BackupRecord::new(
            "c1",
            "mydb",
            DriverKind::MySql,
            PathBuf::from("/tmp/test.sql"),
            1024,
            Some("first backup".to_string()),
        );

        let json = serde_json::to_string_pretty(&[record.clone()]).unwrap();
        std::fs::write(&history_path, &json).unwrap();

        // 手动构造一个指向临时路径的 BackupHistory
        let bh = BackupHistory {
            path: history_path.clone(),
        };

        let loaded = bh.load();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, record.id);

        let filtered = bh.list_for_db("c1", "mydb");
        assert_eq!(filtered.len(), 1);

        bh.remove(&record.id).unwrap();
        let after = bh.load();
        assert!(after.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn backup_record_uses_mysql_driver_name() {
        let record = BackupRecord::new(
            "c1",
            "testdb",
            DriverKind::MySql,
            PathBuf::from("/tmp/x.sql"),
            0,
            None,
        );
        assert_eq!(record.driver, "MySQL");
    }
}
