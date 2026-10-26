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
use std::path::{Path, PathBuf};
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

        let file_size = run_mysqldump_to_file(
            &host,
            port,
            database,
            &config.username,
            &config.password,
            &output_path,
        )
        .await?;

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

        let output_path = PathBuf::from(output_path);

        run_mysqldump_to_file(
            &host,
            port,
            database,
            &config.username,
            &config.password,
            &output_path,
        )
        .await
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

        let path = PathBuf::from(dump_path);
        if !path.is_file() {
            return Err(anyhow!("备份文件不存在：{}", dump_path));
        }

        run_mysql_script_file(&host, port, database, &config.username, &config.password, &path, "还原")
            .await
    }

    /// 流式导入 SQL 文件（大文件不经过 IPC / 内存，因此没有 10MB 上限）
    ///
    /// 由 `mysql` 客户端逐条执行文件中的语句，天然支持 `DELIMITER`、触发器与存储过程，
    /// 而应用内 `import_database_sql` 的内存解析做不到这两点。
    pub async fn import_sql_file(
        &self,
        connection_id: &str,
        database: &str,
        file_path: &str,
    ) -> Result<()> {
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
                "PostgreSQL 暂不支持导入 SQL 文件，请在 SQL 编辑器中执行"
            ));
        }

        let (host, port) = if let Some(local_port) = tunnel_port {
            ("127.0.0.1".to_string(), local_port)
        } else {
            (config.host.clone(), config.port)
        };

        let path = PathBuf::from(file_path);
        if !path.is_file() {
            return Err(anyhow!("SQL 文件不存在：{}", file_path));
        }

        run_mysql_script_file(&host, port, database, &config.username, &config.password, &path, "导入")
            .await
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
///
/// 值统一用双引号包裹并转义，避免密码含 `#`、`;`、空格、引号、反斜杠时被 option file
/// 解析器截断（这是导致 dump 连接失败、导出文件不完整的常见原因）。
fn write_mysql_defaults(username: &str, password: &str) -> Result<PathBuf> {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let path = std::env::temp_dir().join(format!(
        "auroradb-mysql-defaults-{}-{}.ini",
        std::process::id(),
        unique
    ));

    let content = format!(
        "[client]\nuser={}\npassword={}\n",
        quote_option_value(username),
        quote_option_value(password)
    );

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);

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

/// MySQL option file 值的转义（双引号包裹，转义反斜杠/引号/控制字符）
fn quote_option_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
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

/// 把 SQL 脚本文件流式送入 `mysql` 客户端执行（还原 / 导入共用）。
///
/// 文件内容直接经 stdin 管道传给子进程，全程不做字符串拼接、不经过 IPC，
/// 因此文件大小不受限制；`mysql` 客户端本身支持 `DELIMITER` 与多语句脚本。
async fn run_mysql_script_file(
    host: &str,
    port: u16,
    database: &str,
    username: &str,
    password: &str,
    path: &Path,
    action: &str,
) -> Result<()> {
    check_tool("mysql", "--version").await?;

    let defaults_file = write_mysql_defaults(username, password)
        .context("创建 mysql 凭据临时文件失败")?;

    // 启动 mysql 进程，stdin 接脚本文件
    let mut child = match Command::new("mysql")
        .arg(format!("--defaults-extra-file={}", defaults_file.display()))
        .arg("--host")
        .arg(host)
        .arg("--port")
        .arg(port.to_string())
        // 注意：数据库名必须通过 --database 传递。作为位置参数会被 mysql 当成库名后
        // 忽略后续参数，而用户名更不能出现在位置参数里（会被当成库名）。
        .arg(format!("--database={}", database))
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            let _ = tokio::fs::remove_file(&defaults_file).await;
            return Err(anyhow!("启动 mysql 客户端失败：{}", e));
        }
    };

    // 并发读取 stderr，避免大脚本输出大量 warning 时管道写满、子进程阻塞
    let mut child_stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            let _ = tokio::fs::remove_file(&defaults_file).await;
            let _ = child.kill().await;
            return Err(anyhow!("无法获取 mysql 子进程 stderr"));
        }
    };
    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = tokio::io::AsyncReadExt::read_to_end(&mut child_stderr, &mut buf).await;
        buf
    });

    // 把脚本文件内容写入子进程 stdin
    let mut stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => {
            let _ = tokio::fs::remove_file(&defaults_file).await;
            let _ = child.kill().await;
            return Err(anyhow!("无法获取 mysql 子进程 stdin"));
        }
    };
    let write_result = match tokio::fs::File::open(path).await {
        Ok(mut file) => tokio::io::copy(&mut file, &mut stdin).await,
        Err(e) => Err(e),
    };
    // 关闭 stdin，让 mysql 知道输入结束
    drop(stdin);

    // 无论成败都删除凭据文件
    let _ = tokio::fs::remove_file(&defaults_file).await;

    let status = child
        .wait()
        .await
        .map_err(|e| anyhow!("等待 mysql {}完成失败：{}", action, e))?;
    let stderr_bytes = stderr_task.await.unwrap_or_default();
    let stderr_text = String::from_utf8_lossy(&stderr_bytes);
    let stderr_tail = tail_chars(stderr_text.trim(), 800);

    if let Err(e) = write_result {
        return Err(anyhow!(
            "向 mysql 写入{}数据失败：{}；mysql 输出：{}",
            action,
            e,
            stderr_tail
        ));
    }

    if !status.success() {
        return Err(anyhow!(
            "mysql {}失败（退出码 {:?}）：{}",
            action,
            status.code(),
            stderr_tail
        ));
    }

    Ok(())
}

/// 执行 mysqldump 把整库写入 `output_path`，返回文件字节数。
///
/// 关键点：
/// - 库名通过唯一的**位置参数**传入，用户名只走 defaults file（位置参数放用户名会被
///   当成库名，导致 dump 的是错误的库甚至报错）
/// - 先写同目录 `.part` 临时文件，确认 dump footer 完整后才替换目标文件，
///   避免失败时破坏用户已有的同名文件、也避免留下半截 dump 被当作有效备份
async fn run_mysqldump_to_file(
    host: &str,
    port: u16,
    database: &str,
    username: &str,
    password: &str,
    output_path: &Path,
) -> Result<u64> {
    check_tool("mysqldump", "--version").await?;

    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).ok();
        }
    }

    let defaults_file = write_mysql_defaults(username, password)
        .context("创建 mysqldump 凭据临时文件失败")?;

    let result = dump_to_temp_file(&defaults_file, host, port, database, output_path).await;

    let _ = tokio::fs::remove_file(&defaults_file).await;
    result
}

async fn dump_to_temp_file(
    defaults_file: &Path,
    host: &str,
    port: u16,
    database: &str,
    output_path: &Path,
) -> Result<u64> {
    let temp_path = dump_part_path(output_path);

    let stdout_file = std::fs::File::create(&temp_path)
        .map_err(|e| anyhow!("创建导出临时文件失败：{}", e))?;

    let child = match Command::new("mysqldump")
        .arg(format!("--defaults-extra-file={}", defaults_file.display()))
        .arg("--single-transaction")
        .arg("--routines")
        .arg("--triggers")
        .arg("--events")
        .arg("--host")
        .arg(host)
        .arg("--port")
        .arg(port.to_string())
        // 只有一个位置参数 = 要导出的库名
        .arg(database)
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            let _ = std::fs::remove_file(&temp_path);
            return Err(anyhow!("启动 mysqldump 失败：{}", e));
        }
    };

    // wait_with_output 会并发读完 stderr，避免管道写满导致子进程阻塞
    let output = match child.wait_with_output().await {
        Ok(output) => output,
        Err(e) => {
            let _ = std::fs::remove_file(&temp_path);
            return Err(anyhow!("等待 mysqldump 完成失败：{}", e));
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = std::fs::remove_file(&temp_path);
        return Err(anyhow!(
            "mysqldump 执行失败（退出码 {:?}）：{}",
            output.status.code(),
            tail_chars(stderr.trim(), 800)
        ));
    }

    // 校验 footer：mysqldump 正常结束时一定写出 `-- Dump completed on ...`
    if let Err(e) = verify_dump_complete(&temp_path).await {
        let _ = std::fs::remove_file(&temp_path);
        return Err(e);
    }

    let file_size = tokio::fs::metadata(&temp_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    replace_file(&temp_path, output_path)?;

    Ok(file_size)
}

/// 生成 `.part` 临时文件路径（与目标同目录，保证 rename 不跨文件系统）
fn dump_part_path(output_path: &Path) -> PathBuf {
    let mut name = output_path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| "dump.sql".into());
    name.push(".part");
    output_path.with_file_name(name)
}

/// 校验 dump 文件尾部是否包含 mysqldump 的完成标记
async fn verify_dump_complete(path: &Path) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};

    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| anyhow!("读取导出文件失败：{}", e))?;
    let len = file
        .metadata()
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    if len == 0 {
        return Err(anyhow!("mysqldump 未产生任何输出，导出已取消"));
    }

    let tail_len = len.min(1024);
    file.seek(std::io::SeekFrom::End(-(tail_len as i64)))
        .await
        .map_err(|e| anyhow!("读取导出文件失败：{}", e))?;
    let mut buf = Vec::with_capacity(tail_len as usize);
    file.read_to_end(&mut buf)
        .await
        .map_err(|e| anyhow!("读取导出文件失败：{}", e))?;
    let tail = String::from_utf8_lossy(&buf);

    if !tail.contains("-- Dump completed") {
        return Err(anyhow!(
            "导出文件不完整（缺少 mysqldump 完成标记），已放弃写入，原文件保持不变"
        ));
    }

    Ok(())
}

/// 用临时文件替换目标文件（Windows 下目标已存在时先删除）
fn replace_file(temp_path: &Path, output_path: &Path) -> Result<()> {
    match std::fs::rename(temp_path, output_path) {
        Ok(()) => Ok(()),
        Err(first_err) => {
            let retry = if output_path.exists() {
                std::fs::remove_file(output_path)
                    .and_then(|_| std::fs::rename(temp_path, output_path))
            } else {
                Err(first_err)
            };

            match retry {
                Ok(()) => Ok(()),
                Err(e) => {
                    let _ = std::fs::remove_file(temp_path);
                    Err(anyhow!("写入导出文件失败：{}", e))
                }
            }
        }
    }
}

/// 取字符串末尾至多 `max` 个字符（用于错误提示，避免刷屏）
fn tail_chars(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    text.chars().skip(count - max).collect()
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
    fn quotes_special_chars_in_option_file() {
        // `#`/`;` 等字符未加引号会被 option file 当作注释，导致密码被截断
        assert_eq!(quote_option_value("p#ss;word"), "\"p#ss;word\"");
        assert_eq!(quote_option_value("a\\b\"c"), "\"a\\\\b\\\"c\"");
        assert_eq!(quote_option_value(""), "\"\"");
    }

    #[tokio::test]
    async fn dump_without_completion_marker_is_rejected() {
        let dir = std::env::temp_dir().join(format!(
            "auroradb-dump-verify-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let truncated = dir.join("truncated.sql");
        std::fs::write(&truncated, "-- MySQL dump\nCREATE TABLE t (id int);\n").unwrap();
        assert!(verify_dump_complete(&truncated).await.is_err());

        let complete = dir.join("complete.sql");
        std::fs::write(
            &complete,
            "-- MySQL dump\nCREATE TABLE t (id int);\n-- Dump completed on 2026-01-01  0:00:00\n",
        )
        .unwrap();
        assert!(verify_dump_complete(&complete).await.is_ok());

        let empty = dir.join("empty.sql");
        std::fs::write(&empty, "").unwrap();
        assert!(verify_dump_complete(&empty).await.is_err());

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
