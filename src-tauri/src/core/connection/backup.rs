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
//!
//! TLS：外部客户端不会继承应用内的 SSL 配置，必须显式传递，否则遇到自签证书会被
//! MariaDB 11.x 客户端默认开启的证书校验拒绝。

use super::driver::DriverKind;
use super::manager::ConnectionManager;
use super::types::{SslConfig, SslMode};
use super::validation::validate_identifier;
use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

/// 外部客户端工具的最长执行时间：避免子进程卡死（网络僵死、服务端无响应）时界面永久转圈
const CLIENT_TIMEOUT: Duration = Duration::from_secs(3600);
/// 版本探测的短超时
const VERSION_CHECK_TIMEOUT: Duration = Duration::from_secs(15);

impl ConnectionManager {
    /// 外部客户端执行的脚本绕过了连接池，元数据缓存不会自动失效，
    /// 必须手动清空，否则对象树/表结构会在 TTL（30s）内返回执行前的旧快照。
    async fn invalidate_metadata_cache(&self, connection_id: &str) {
        let connections = self.connections.read().await;
        if let Some(conn) = connections.get(connection_id) {
            conn.metadata_cache.lock().await.clear();
        }
    }

    /// 触发完整备份：根据驱动调用 mysqldump / pg_dump，写入用户选择的路径。
    pub async fn backup_database(
        &self,
        connection_id: &str,
        database: &str,
        output_path: &str,
    ) -> Result<u64> {
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

        // 确保输出目录存在（用户可能在对话框中输入了尚不存在的目录）
        let output_path = PathBuf::from(output_path);
        if let Some(parent) = output_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).ok();
            }
        }

        run_mysqldump_to_file(
            &host,
            port,
            database,
            &config.username,
            &config.password,
            &config.ssl_config,
            &output_path,
        )
        .await
    }

    /// 导出整库 SQL 到用户指定路径（mysqldump 直写文件，不经内存/IPC）。
    ///
    /// 相比旧版逐表拼接 SQL 的实现，这里没有 1 万行上限，也不会在超时后静默跳过表，
    /// 并且天然包含视图、触发器、存储过程与事件。
    pub async fn export_database_sql_to_file(
        &self,
        connection_id: &str,
        database: &str,
        output_path: &str,
    ) -> Result<u64> {
        validate_identifier(database).map_err(|e| anyhow!("数据库名验证失败：{}", e))?;

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
            &config.ssl_config,
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
            return Err(anyhow!("PostgreSQL 还原将在后续版本提供"));
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

        run_mysql_script_file(
            &host,
            port,
            database,
            &config.username,
            &config.password,
            &config.ssl_config,
            &path,
            "还原",
            None,
        )
        .await?;
        // 还原脚本可能重建表结构，清空缓存让刷新能拿到最新列表
        self.invalidate_metadata_cache(connection_id).await;
        Ok(())
    }

    /// 流式导入 SQL 文件（大文件不经过 IPC / 内存，因此没有 10MB 上限）
    ///
    /// 由 `mysql` 客户端逐条执行文件中的语句，天然支持 `DELIMITER`、触发器与存储过程，
    /// 而应用内 `import_database_sql` 的内存解析做不到这两点。
    /// 传入 `reporter` 时读取客户端 `-v` 回显，实时推送逐条语句的执行流水。
    pub async fn import_sql_file(
        &self,
        connection_id: &str,
        database: &str,
        file_path: &str,
        reporter: Option<ImportReporter>,
    ) -> Result<()> {
        validate_identifier(database).map_err(|e| anyhow!("数据库名验证失败：{}", e))?;

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

        run_mysql_script_file(
            &host,
            port,
            database,
            &config.username,
            &config.password,
            &config.ssl_config,
            &path,
            "导入",
            reporter,
        )
        .await?;
        // 导入脚本会创建表，清空缓存让刷新能拿到最新列表
        self.invalidate_metadata_cache(connection_id).await;
        Ok(())
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

/// MySQL 客户端工具的实现分支：两者 TLS 参数不同
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MysqlClientFlavor {
    /// Oracle MySQL：使用 `--ssl-mode`
    MySql,
    /// MariaDB：使用 `--ssl` / `--ssl-verify-server-cert`
    MariaDb,
}

/// 检测外部工具是否可执行（mysqldump / mysql / pg_dump / psql），返回版本信息
async fn check_tool(name: &str, version_arg: &str) -> Result<String> {
    let result = tokio::time::timeout(
        VERSION_CHECK_TIMEOUT,
        Command::new(name)
            .arg(version_arg)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output(),
    )
    .await;

    match result {
        Ok(Ok(output)) => Ok(format!(
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )),
        Ok(Err(e)) => {
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
        Err(_) => Err(anyhow!(
            "检测 `{}` 版本超时（超过 {} 秒），请检查该工具是否可正常执行",
            name,
            VERSION_CHECK_TIMEOUT.as_secs()
        )),
    }
}

/// 从 `--version` 输出判断客户端实现
fn detect_flavor(version: &str) -> MysqlClientFlavor {
    if version.to_ascii_lowercase().contains("mariadb") {
        MysqlClientFlavor::MariaDb
    } else {
        MysqlClientFlavor::MySql
    }
}

/// 把应用的 SSL 配置翻译为外部客户端参数
///
/// MariaDB 11.4+ 默认开启 `ssl-verify-server-cert`，因此「首选 / 必须加密」必须显式
/// 关闭校验，否则连接自签证书的服务端会直接失败。
fn mysql_ssl_args(flavor: MysqlClientFlavor, ssl: &SslConfig) -> Vec<String> {
    let mut args = Vec::new();
    let ca = ssl
        .ca_path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty());
    let cert = ssl
        .cert_path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty());
    let key = ssl
        .key_path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty());

    match flavor {
        MysqlClientFlavor::MySql => {
            let mode = match ssl.mode {
                SslMode::Disabled => "DISABLED",
                SslMode::Preferred => "PREFERRED",
                SslMode::Required => "REQUIRED",
                SslMode::VerifyCa => "VERIFY_CA",
                SslMode::VerifyIdentity => "VERIFY_IDENTITY",
            };
            args.push(format!("--ssl-mode={}", mode));
        }
        MysqlClientFlavor::MariaDb => match ssl.mode {
            SslMode::Disabled => args.push("--skip-ssl".to_string()),
            // 加密但不校验服务端证书（MariaDB 客户端 ssl 默认开启）
            SslMode::Preferred => args.push("--skip-ssl-verify-server-cert".to_string()),
            SslMode::Required => {
                args.push("--ssl".to_string());
                args.push("--skip-ssl-verify-server-cert".to_string());
            }
            // MariaDB 只有「校验/不校验」两档，verify-ca 与 verify-identity 同样处理
            SslMode::VerifyCa | SslMode::VerifyIdentity => {
                args.push("--ssl".to_string());
                args.push("--ssl-verify-server-cert".to_string());
            }
        },
    }

    if ssl.mode != SslMode::Disabled {
        if let Some(path) = ca {
            args.push(format!("--ssl-ca={}", path));
        }
        if let Some(path) = cert {
            args.push(format!("--ssl-cert={}", path));
        }
        if let Some(path) = key {
            args.push(format!("--ssl-key={}", path));
        }
    }

    args
}

/// 导入流水中的一行
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportLogLine {
    /// 语句序号（从 1 开始）；服务端输出等非语句信息为 None
    pub no: Option<u64>,
    /// success / error / warn
    pub level: &'static str,
    pub text: String,
    /// 语句执行耗时（毫秒）
    pub elapsed_ms: Option<u64>,
}

/// 导入进度事件负载：按批次合并，避免大脚本逐条语句刷爆 IPC
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    /// 已送入 mysql 的字节数
    pub bytes: u64,
    /// 脚本文件总字节数
    pub total: u64,
    pub lines: Vec<ImportLogLine>,
    /// 是否已结束
    pub done: bool,
    /// 结束时的结果（执行中恒为 false）
    pub success: bool,
}

/// 进度回调，由命令层包装成 Tauri 事件
pub type ImportReporter = Arc<dyn Fn(ImportProgress) + Send + Sync>;

/// 子进程输出到转发任务的消息
enum ImportMsg {
    Line(ImportLogLine),
    Done(bool),
}

/// `mysql -v` 在每条语句前后各输出一行 `--------------`
fn is_echo_separator(line: &str) -> bool {
    line.len() >= 10 && line.chars().all(|c| c == '-')
}

/// 折叠空白并截断到 `max` 个字符，用于日志展示
fn collapse_text(raw: &str, max: usize) -> String {
    let mut text = String::new();
    let mut last_space = false;
    for ch in raw.chars() {
        if ch.is_whitespace() {
            if !last_space && !text.is_empty() {
                text.push(' ');
            }
            last_space = true;
        } else {
            text.push(ch);
            last_space = false;
        }
    }
    let trimmed = text.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// 把文件内容写入子进程 stdin，同时记录已发送字节数供进度条使用
async fn copy_into_stdin(
    file: &mut tokio::fs::File,
    stdin: &mut tokio::process::ChildStdin,
    counter: &AtomicU64,
) -> std::io::Result<()> {
    let mut buffer = vec![0u8; 64 * 1024];
    let mut written = 0u64;
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        stdin.write_all(&buffer[..read]).await?;
        written += read as u64;
        counter.store(written, Ordering::Relaxed);
    }
    stdin.flush().await
}

/// 把 SQL 脚本文件流式送入 `mysql` 客户端执行（还原 / 导入共用）。
///
/// 文件内容直接经 stdin 管道传给子进程，全程不做字符串拼接、不经过 IPC，
/// 因此文件大小不受限制；`mysql` 客户端本身支持 `DELIMITER` 与多语句脚本。
/// 传入 `reporter` 时额外开启 `-v`，解析客户端回显并实时推送逐条语句流水。
async fn run_mysql_script_file(
    host: &str,
    port: u16,
    database: &str,
    username: &str,
    password: &str,
    ssl: &SslConfig,
    path: &Path,
    action: &str,
    reporter: Option<ImportReporter>,
) -> Result<()> {
    let version = check_tool("mysql", "--version").await?;
    let ssl_args = mysql_ssl_args(detect_flavor(&version), ssl);

    let defaults_file =
        write_mysql_defaults(username, password).context("创建 mysql 凭据临时文件失败")?;

    let streaming = reporter.is_some();
    let total = if streaming {
        std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
    } else {
        0
    };

    // 启动 mysql 进程，stdin 接脚本文件
    let mut command = Command::new("mysql");
    command
        .arg(format!("--defaults-extra-file={}", defaults_file.display()))
        .args(&ssl_args)
        .arg("--host")
        .arg(host)
        .arg("--port")
        .arg(port.to_string())
        // 注意：数据库名必须通过 --database 传递。作为位置参数会被 mysql 当成库名后
        // 忽略后续参数，而用户名更不能出现在位置参数里（会被当成库名）。
        .arg(format!("--database={}", database))
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if streaming {
        // 还原不需要流水，避免额外输出拖慢大文件
        command.arg("-v").stdout(Stdio::piped());
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            let _ = tokio::fs::remove_file(&defaults_file).await;
            return Err(anyhow!("启动 mysql 客户端失败：{}", e));
        }
    };

    // 进度事件按 ~150ms 一批合并后推给前端
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ImportMsg>();
    let written_bytes = Arc::new(AtomicU64::new(0));
    let forwarder = reporter.map(|report| {
        let written_bytes = written_bytes.clone();
        tokio::spawn(async move {
            let mut pending: Vec<ImportLogLine> = Vec::new();
            let mut last_bytes = 0u64;
            let mut ticker = tokio::time::interval(Duration::from_millis(150));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    message = rx.recv() => match message {
                        Some(ImportMsg::Line(line)) => {
                            pending.push(line);
                            if pending.len() >= 50 {
                                let bytes = written_bytes.load(Ordering::Relaxed);
                                last_bytes = bytes;
                                report(ImportProgress {
                                    bytes,
                                    total,
                                    lines: std::mem::take(&mut pending),
                                    done: false,
                                    success: false,
                                });
                            }
                        }
                        Some(ImportMsg::Done(success)) => {
                            let bytes = written_bytes.load(Ordering::Relaxed);
                            report(ImportProgress {
                                bytes,
                                total,
                                lines: std::mem::take(&mut pending),
                                done: true,
                                success,
                            });
                            return;
                        }
                        // 发送端全部释放却没收到 Done：按失败收尾，避免前端一直转圈
                        None => {
                            report(ImportProgress {
                                bytes: written_bytes.load(Ordering::Relaxed),
                                total,
                                lines: std::mem::take(&mut pending),
                                done: true,
                                success: false,
                            });
                            return;
                        }
                    },
                    _ = ticker.tick() => {
                        let bytes = written_bytes.load(Ordering::Relaxed);
                        if pending.is_empty() && bytes == last_bytes {
                            continue;
                        }
                        last_bytes = bytes;
                        report(ImportProgress {
                            bytes,
                            total,
                            lines: std::mem::take(&mut pending),
                            done: false,
                            success: false,
                        });
                    }
                }
            }
        })
    });

    // 解析 `-v` 回显：分隔线之间是一条语句。mysql 逐条同步执行，所以
    // 「下一条语句回显结束」即代表上一条已成功执行完。
    let stdout_task = if streaming {
        child.stdout.take().map(|stdout| {
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                let mut in_echo = false;
                let mut buffer = String::new();
                let mut previous: Option<(u64, String, Instant)> = None;
                let mut seq = 0u64;
                while let Ok(Some(line)) = lines.next_line().await {
                    if is_echo_separator(line.trim()) {
                        if !in_echo {
                            in_echo = true;
                            buffer.clear();
                            continue;
                        }
                        in_echo = false;
                        let text = collapse_text(&buffer, 160);
                        if text.is_empty() {
                            continue;
                        }
                        if let Some((no, previous_text, started)) = previous.take() {
                            let _ = tx.send(ImportMsg::Line(ImportLogLine {
                                no: Some(no),
                                level: "success",
                                text: previous_text,
                                elapsed_ms: Some(started.elapsed().as_millis() as u64),
                            }));
                        }
                        seq += 1;
                        previous = Some((seq, text, Instant::now()));
                    } else if in_echo {
                        buffer.push_str(&line);
                        buffer.push('\n');
                    }
                }
                // 最后一条语句的状态要等子进程退出码才能确定，交给调用方处理
                previous
            })
        })
    } else {
        None
    };

    // 并发读取 stderr，避免大脚本输出大量 warning 时管道写满、子进程阻塞；
    // 同时把服务端错误/警告实时推给前端
    let mut child_stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            let _ = tokio::fs::remove_file(&defaults_file).await;
            let _ = child.kill().await;
            return Err(anyhow!("无法获取 mysql 子进程 stderr"));
        }
    };
    let stderr_stream = streaming.then(|| tx.clone());
    let stderr_task = tokio::spawn(async move {
        let mut reader = BufReader::new(&mut child_stderr);
        let mut raw = Vec::new();
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    raw.extend_from_slice(line.as_bytes());
                    if let Some(tx) = &stderr_stream {
                        let text = collapse_text(&line, 400);
                        // 失败语句在 stderr 里也会带一层分隔线，跳过以免流水出现孤立横线
                        if !text.is_empty() && !is_echo_separator(&text) {
                            let level = if text.starts_with("ERROR") { "error" } else { "warn" };
                            let _ = tx.send(ImportMsg::Line(ImportLogLine {
                                no: None,
                                level,
                                text,
                                elapsed_ms: None,
                            }));
                        }
                    }
                }
            }
        }
        raw
    });

    // 把脚本文件内容写入子进程 stdin，并等待其退出；整体受超时保护
    let mut stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => {
            let _ = tokio::fs::remove_file(&defaults_file).await;
            let _ = child.kill().await;
            return Err(anyhow!("无法获取 mysql 子进程 stdin"));
        }
    };
    let counter = written_bytes.clone();

    let operation = async {
        let write_result = match tokio::fs::File::open(path).await {
            Ok(mut file) => copy_into_stdin(&mut file, &mut stdin, &counter).await,
            Err(e) => Err(e),
        };
        // 关闭 stdin，让 mysql 知道输入结束
        drop(stdin);
        let status = child.wait().await;
        (write_result, status)
    };

    let (write_result, status) = match tokio::time::timeout(CLIENT_TIMEOUT, operation).await {
        Ok(result) => result,
        Err(_) => {
            stderr_task.abort();
            if let Some(task) = stdout_task {
                task.abort();
            }
            let _ = child.kill().await;
            let _ = tokio::fs::remove_file(&defaults_file).await;
            let _ = tx.send(ImportMsg::Done(false));
            drop(tx);
            if let Some(forwarder) = forwarder {
                let _ = forwarder.await;
            }
            return Err(anyhow!(
                "mysql {}超时（超过 {} 分钟），已终止子进程",
                action,
                CLIENT_TIMEOUT.as_secs() / 60
            ));
        }
    };

    // 无论成败都删除凭据文件
    let _ = tokio::fs::remove_file(&defaults_file).await;

    let status = status.map_err(|e| anyhow!("等待 mysql {}完成失败：{}", action, e))?;
    let stderr_bytes = stderr_task.await.unwrap_or_default();
    let stderr_text = String::from_utf8_lossy(&stderr_bytes);
    let stderr_tail = tail_chars(stderr_text.trim(), 800);

    // 最后一条语句随子进程退出结束；失败时它通常就是出错的那条
    if let Some(task) = stdout_task {
        if let Ok(Some((no, text, started))) = task.await {
            let elapsed_ms = started.elapsed().as_millis() as u64;
            if status.success() {
                let _ = tx.send(ImportMsg::Line(ImportLogLine {
                    no: Some(no),
                    level: "success",
                    text,
                    elapsed_ms: Some(elapsed_ms),
                }));
            } else {
                let _ = tx.send(ImportMsg::Line(ImportLogLine {
                    no: Some(no),
                    level: "error",
                    text,
                    elapsed_ms: Some(elapsed_ms),
                }));
                if !stderr_tail.is_empty() {
                    let _ = tx.send(ImportMsg::Line(ImportLogLine {
                        no: None,
                        level: "error",
                        text: stderr_tail.clone(),
                        elapsed_ms: None,
                    }));
                }
            }
        }
    }
    let _ = tx.send(ImportMsg::Done(status.success()));
    drop(tx);
    if let Some(forwarder) = forwarder {
        let _ = forwarder.await;
    }

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
    ssl: &SslConfig,
    output_path: &Path,
) -> Result<u64> {
    let version = check_tool("mysqldump", "--version").await?;
    let ssl_args = mysql_ssl_args(detect_flavor(&version), ssl);

    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).ok();
        }
    }

    let defaults_file =
        write_mysql_defaults(username, password).context("创建 mysqldump 凭据临时文件失败")?;

    let result =
        dump_to_temp_file(&defaults_file, host, port, database, &ssl_args, output_path).await;

    let _ = tokio::fs::remove_file(&defaults_file).await;
    result
}

async fn dump_to_temp_file(
    defaults_file: &Path,
    host: &str,
    port: u16,
    database: &str,
    ssl_args: &[String],
    output_path: &Path,
) -> Result<u64> {
    let temp_path = dump_part_path(output_path);

    let stdout_file =
        std::fs::File::create(&temp_path).map_err(|e| anyhow!("创建导出临时文件失败：{}", e))?;

    let child = match Command::new("mysqldump")
        .arg(format!("--defaults-extra-file={}", defaults_file.display()))
        .args(ssl_args)
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
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            let _ = std::fs::remove_file(&temp_path);
            return Err(anyhow!("启动 mysqldump 失败：{}", e));
        }
    };

    // wait_with_output 会并发读完 stderr，避免管道写满导致子进程阻塞；
    // 外层超时防止服务端无响应时导出无限挂起
    let output = match tokio::time::timeout(CLIENT_TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            let _ = std::fs::remove_file(&temp_path);
            return Err(anyhow!("等待 mysqldump 完成失败：{}", e));
        }
        Err(_) => {
            let _ = std::fs::remove_file(&temp_path);
            return Err(anyhow!(
                "mysqldump 超时（超过 {} 分钟），已终止子进程并删除临时文件",
                CLIENT_TIMEOUT.as_secs() / 60
            ));
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
    let len = file.metadata().await.map(|m| m.len()).unwrap_or(0);
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
    fn detects_client_flavor_from_version_output() {
        assert_eq!(
            detect_flavor("mysqldump from 11.8.1-MariaDB, client 10.19 for debian-linux-gnu"),
            MysqlClientFlavor::MariaDb
        );
        assert_eq!(
            detect_flavor("mysqldump  Ver 8.0.36 for Linux on x86_64 (MySQL Community Server)"),
            MysqlClientFlavor::MySql
        );
    }

    #[test]
    fn mysql_ssl_args_follow_connection_mode() {
        // MySQL 客户端：--ssl-mode 一一对应
        let mysql = MysqlClientFlavor::MySql;
        assert_eq!(
            mysql_ssl_args(mysql, &SslConfig::default()),
            vec!["--ssl-mode=PREFERRED".to_string()]
        );
        assert_eq!(
            mysql_ssl_args(
                mysql,
                &SslConfig {
                    mode: SslMode::VerifyCa,
                    ca_path: Some("/etc/ca.pem".to_string()),
                    cert_path: None,
                    key_path: None,
                }
            ),
            vec![
                "--ssl-mode=VERIFY_CA".to_string(),
                "--ssl-ca=/etc/ca.pem".to_string()
            ]
        );

        // MariaDB 客户端：默认会校验证书，未启用校验的模式必须显式关闭
        let mariadb = MysqlClientFlavor::MariaDb;
        assert_eq!(
            mysql_ssl_args(mariadb, &SslConfig::default()),
            vec!["--skip-ssl-verify-server-cert".to_string()]
        );
        assert_eq!(
            mysql_ssl_args(
                mariadb,
                &SslConfig {
                    mode: SslMode::VerifyIdentity,
                    ca_path: Some("/etc/ca.pem".to_string()),
                    cert_path: Some("/etc/client.pem".to_string()),
                    key_path: Some("/etc/client.key".to_string()),
                }
            ),
            vec![
                "--ssl".to_string(),
                "--ssl-verify-server-cert".to_string(),
                "--ssl-ca=/etc/ca.pem".to_string(),
                "--ssl-cert=/etc/client.pem".to_string(),
                "--ssl-key=/etc/client.key".to_string()
            ]
        );
        assert_eq!(
            mysql_ssl_args(
                mariadb,
                &SslConfig {
                    mode: SslMode::Disabled,
                    ca_path: Some("/etc/ca.pem".to_string()),
                    cert_path: None,
                    key_path: None,
                }
            ),
            vec!["--skip-ssl".to_string()]
        );
    }

    #[test]
    fn echo_separator_matches_mysql_verbose_output() {
        // `mysql -v` 用 14 个短横线包裹每条语句的回显
        assert!(is_echo_separator("--------------"));
        assert!(!is_echo_separator("---------"));
        assert!(!is_echo_separator("-- comment ----------"));
        assert!(!is_echo_separator(""));
    }

    #[test]
    fn collapse_text_flattens_multiline_statements() {
        assert_eq!(
            collapse_text("INSERT INTO t\n  (a, b)\nVALUES\n  (1, 2);", 160),
            "INSERT INTO t (a, b) VALUES (1, 2);"
        );
        // 超长语句截断并补省略号，避免单行日志撑爆事件负载
        let long = "a".repeat(200);
        let collapsed = collapse_text(&long, 20);
        assert_eq!(collapsed.chars().count(), 20);
        assert!(collapsed.ends_with('…'));
    }
}
