//! 数据库 SQL 导入导出（免费版）。
//!
//! 通过子进程调用官方客户端工具（`mysqldump` / `mysql`），覆盖纯 SQL 导出的 10K 行上限，
//! 并天然包含 views / triggers / routines。
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

        let (config, ssl_config, tunnel_port, tls_disabled) = {
            let connections = self.connections.read().await;
            let conn = connections
                .get(connection_id)
                .ok_or_else(|| anyhow!("连接 {} 不存在或已断开", connection_id))?;
            let tunnel_port = conn.tunnel.as_ref().map(|t| t.local_port());
            (
                conn.config.clone(),
                conn.effective_ssl_config(),
                tunnel_port,
                conn.tls_disabled,
            )
        };

        let (host, port) = if let Some(local_port) = tunnel_port {
            ("127.0.0.1".to_string(), local_port)
        } else {
            (config.host.clone(), config.port)
        };

        let output_path = PathBuf::from(output_path);

        if config.driver == DriverKind::Postgres {
            // 连接池的连接超时（秒）直接复用给 pg_dump：主机不可达时不必等满 CLIENT_TIMEOUT
            return run_pg_dump_to_file_with_connect_timeout(
                &host,
                port,
                database,
                &config.username,
                &config.password,
                &ssl_config,
                tls_disabled,
                config.pool_config.connect_timeout,
                &output_path,
            )
            .await;
        }

        run_mysqldump_to_file(
            &host,
            port,
            database,
            &config.username,
            &config.password,
            &ssl_config,
            &output_path,
        )
        .await
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

        let (config, ssl_config, tunnel_port, tls_disabled) = {
            let connections = self.connections.read().await;
            let conn = connections
                .get(connection_id)
                .ok_or_else(|| anyhow!("连接 {} 不存在或已断开", connection_id))?;
            let tunnel_port = conn.tunnel.as_ref().map(|t| t.local_port());
            (
                conn.config.clone(),
                conn.effective_ssl_config(),
                tunnel_port,
                conn.tls_disabled,
            )
        };

        let (host, port) = if let Some(local_port) = tunnel_port {
            ("127.0.0.1".to_string(), local_port)
        } else {
            (config.host.clone(), config.port)
        };

        let path = PathBuf::from(file_path);
        if !path.is_file() {
            return Err(anyhow!("SQL 文件不存在：{}", file_path));
        }

        if config.driver == DriverKind::Postgres {
            run_pg_script_file_with_connect_timeout(
                &host,
                port,
                database,
                &config.username,
                &config.password,
                &ssl_config,
                tls_disabled,
                config.pool_config.connect_timeout,
                &path,
                "导入",
                reporter,
            )
            .await?;
        } else {
            run_mysql_script_file(
                &host,
                port,
                database,
                &config.username,
                &config.password,
                &ssl_config,
                &path,
                "导入",
                reporter,
            )
            .await?;
        }
        // 导入脚本会创建表，清空缓存让刷新能拿到最新列表
        self.invalidate_metadata_cache(connection_id).await;
        Ok(())
    }
}

/// 写入 MySQL 凭据临时文件（Unix 权限 0600），返回文件路径
///
/// Windows 没有 0600 语义：临时文件位于 `%LOCALAPPDATA%\Temp`，该目录的 ACL 只授权
/// 当前用户、SYSTEM 与 Administrators，其他用户不可读；且文件在 dump 结束（含失败、
/// 超时）后立即删除，不再额外收紧 ACL。
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
            // PG 与 MySQL 属于不同的客户端工具包，安装提示要分开，否则用户会装错东西
            if name == "pg_dump" || name == "psql" {
                return Err(anyhow!(
                    "未找到 {name} 可执行文件，请安装 PostgreSQL 客户端工具\
                     （如 apt install postgresql-client）：{e}"
                ));
            }
            // mysqldump 与 mysql 同属 MySQL 客户端工具包，缺失时给同一条安装提示
            let hint = if name == "mysqldump" || name == "mysql" {
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
    /// 已送入数据库客户端的字节数
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

/// 启动导入进度转发任务：把流水消息按 ~150ms 一批合并后推给前端。
///
/// MySQL（`-v` 逐条回显）与 PostgreSQL（stderr 告警）两条子进程路径共用。
/// `reporter` 为 None 时返回 None：调用方仍照常向通道发送消息，unbounded channel 不阻塞，
/// 只是没人接收（保证未开启进度展示时零额外开销）。
fn spawn_import_forwarder(
    reporter: Option<ImportReporter>,
    total: u64,
    written_bytes: Arc<AtomicU64>,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<ImportMsg>,
) -> Option<tokio::task::JoinHandle<()>> {
    reporter.map(|report| {
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
    })
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
// 参数多但都是一次性调用所需的连接信息，拆成结构体反而让调用处更繁琐
#[allow(clippy::too_many_arguments)]
pub async fn run_mysql_script_file(
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
        // 开启 `-v` 才能拿到逐条语句回显，用于展示实时进度；stderr 也一并收集
        command.arg("-v").stdout(Stdio::piped());
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            let _ = tokio::fs::remove_file(&defaults_file).await;
            return Err(anyhow!("启动 mysql 客户端失败：{}", e));
        }
    };

    // 进度事件按 ~150ms 一批合并后推给前端（转发任务由 MySQL / PostgreSQL 共用）
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ImportMsg>();
    let written_bytes = Arc::new(AtomicU64::new(0));
    let forwarder = spawn_import_forwarder(reporter, total, written_bytes.clone(), rx);

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
                            let level = if text.starts_with("ERROR") {
                                "error"
                            } else {
                                "warn"
                            };
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
pub async fn run_mysqldump_to_file(
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

// ---------------------------------------------------------------------------
// PostgreSQL：pg_dump / psql 子进程
//
// 与 MySQL 路径的差异：
// - 凭据只通过 `PGPASSWORD` 环境变量传递（不落临时文件，也不出现在命令行参数里，
//   避免 `ps` / 进程审计泄漏密码）；
// - SSL 只有环境变量入口（`PGSSLMODE` / `PGSSL*`），没有命令行参数；
// - pg_dump 的 plain 格式输出到 stdout，由调用方重定向到 `.part` 临时文件；
// - psql 没有 `mysql -v` 那样稳定的逐条语句回显，进度策略见 `run_pg_script_file`。
// ---------------------------------------------------------------------------

/// 外部工具实际使用的 SSL 配置：连接因 TLS 握手失败回退明文时必须强制 Disabled，
/// 否则 pg_dump/psql 会按「首选」继续尝试 TLS，与已建立的连接状态不一致。
fn pg_effective_ssl(ssl: &SslConfig, tls_disabled: bool) -> SslConfig {
    let mut effective = ssl.clone();
    if tls_disabled {
        effective.mode = SslMode::Disabled;
    }
    effective
}

/// 把 SSL 配置翻译成 libpq 环境变量（pg_dump / psql 都只认这些环境变量）。
///
/// Disabled 模式下不传任何证书变量：libpq 在 `sslmode=disable` 时忽略它们，
/// 但显式不传可以避免证书路径失效时报出与用户意图无关的错误。
fn pg_ssl_env(ssl: &SslConfig) -> Vec<(String, String)> {
    let mode = match ssl.mode {
        SslMode::Disabled => "disable",
        SslMode::Preferred => "prefer",
        SslMode::Required => "require",
        SslMode::VerifyCa => "verify-ca",
        SslMode::VerifyIdentity => "verify-full",
    };
    let mut env = vec![("PGSSLMODE".to_string(), mode.to_string())];

    if ssl.mode != SslMode::Disabled {
        // 路径为空串时视为未配置（与连接池的 non_empty_path 语义一致）
        for (key, value) in [
            ("PGSSLROOTCERT", ssl.ca_path.as_deref()),
            ("PGSSLCERT", ssl.cert_path.as_deref()),
            ("PGSSLKEY", ssl.key_path.as_deref()),
        ] {
            if let Some(path) = value.map(str::trim).filter(|path| !path.is_empty()) {
                env.push((key.to_string(), path.to_string()));
            }
        }
    }

    env
}

/// pg_dump 参数（纯函数，便于单测）。
///
/// 密码与 SSL 不在此处：分别走 `PGPASSWORD` 与 `PG*` 环境变量，避免出现在进程列表。
/// 用 `--dbname` 而不是位置参数，保持与 MySQL 路径一致的显式语义。
/// `--clean` 不默认加：导出文件应当是「可直接应用到空库」的纯建库脚本，
/// 需要覆盖已有对象由调用方/用户自行决定（可通过 psql 的 `--set ON_ERROR_STOP` 行为观察）。
fn build_pg_dump_args(host: &str, port: u16, database: &str, username: &str) -> Vec<String> {
    vec![
        "--host".to_string(),
        host.to_string(),
        "--port".to_string(),
        port.to_string(),
        "--username".to_string(),
        username.to_string(),
        "--dbname".to_string(),
        database.to_string(),
        "--no-password".to_string(),
        "--format=plain".to_string(),
        "--encoding=UTF8".to_string(),
        "--no-owner".to_string(),
        "--no-privileges".to_string(),
    ]
}

/// psql 还原参数（纯函数，便于单测）。
///
/// - `--no-psqlrc`：忽略用户 `~/.psqlrc`，否则本地的 `ECHO`/`ON_ERROR_STOP` 设置会改变
///   还原语义，导致「本机成功、别人机器失败」这类难排查问题；
/// - `--set=ON_ERROR_STOP=on`：遇到第一条错误立即退出并返回非零退出码，
///   与 mysql 客户端默认遇错即停的行为对齐，避免把部分失败的还原当成功。
fn build_pg_script_args(host: &str, port: u16, database: &str, username: &str) -> Vec<String> {
    vec![
        "--host".to_string(),
        host.to_string(),
        "--port".to_string(),
        port.to_string(),
        "--username".to_string(),
        username.to_string(),
        "--dbname".to_string(),
        database.to_string(),
        "--no-password".to_string(),
        "--no-psqlrc".to_string(),
        "--set=ON_ERROR_STOP=on".to_string(),
    ]
}

/// pg 客户端工具的默认连接超时（秒），与连接池默认值保持一致。
///
/// pg_dump/psql 没有 `--connect-timeout` 参数，主机不可达时 TCP 连接会挂到内核超时
/// （可达数十分钟），必须通过 `PGCONNECT_TIMEOUT` 让它们快速失败。
const DEFAULT_PG_CONNECT_TIMEOUT_SECS: u64 = 10;

/// 对外部工具构造统一的子进程环境：密码 + SSL 配置 + 连接超时。
fn pg_client_env(
    password: &str,
    ssl: &SslConfig,
    tls_disabled: bool,
    connect_timeout_secs: u64,
) -> Vec<(String, String)> {
    let mut env = vec![
        ("PGPASSWORD".to_string(), password.to_string()),
        (
            "PGCONNECT_TIMEOUT".to_string(),
            connect_timeout_secs.max(1).to_string(),
        ),
    ];
    env.extend(pg_ssl_env(&pg_effective_ssl(ssl, tls_disabled)));
    env
}

/// 用 pg_dump 把整库导出为 plain SQL 文件，返回文件字节数。
///
/// 先写同目录 `.part` 临时文件，校验 pg_dump 的完成标记后才替换目标文件：
/// 失败/超时不会破坏用户已有的同名备份，也不会留下半截 dump 被误当成有效备份。
///
/// 注意：pg_dump 要求客户端版本不低于服务端主版本，版本不匹配时 stderr 会给出提示，
/// 这里原样拼进错误文案，方便用户升级客户端。
///
/// 使用默认连接超时；持有连接池配置的调用方应改用
/// [`run_pg_dump_to_file_with_connect_timeout`]。
#[allow(clippy::too_many_arguments)]
pub async fn run_pg_dump_to_file(
    host: &str,
    port: u16,
    database: &str,
    username: &str,
    password: &str,
    ssl: &SslConfig,
    tls_disabled: bool,
    output_path: &Path,
) -> Result<u64> {
    run_pg_dump_to_file_with_connect_timeout(
        host,
        port,
        database,
        username,
        password,
        ssl,
        tls_disabled,
        DEFAULT_PG_CONNECT_TIMEOUT_SECS,
        output_path,
    )
    .await
}

/// 与 [`run_pg_dump_to_file`] 相同，但连接超时取自连接池配置（`PoolConfig.connect_timeout`）。
#[allow(clippy::too_many_arguments)]
pub async fn run_pg_dump_to_file_with_connect_timeout(
    host: &str,
    port: u16,
    database: &str,
    username: &str,
    password: &str,
    ssl: &SslConfig,
    tls_disabled: bool,
    connect_timeout_secs: u64,
    output_path: &Path,
) -> Result<u64> {
    check_tool("pg_dump", "--version").await?;

    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).ok();
        }
    }

    let temp_path = dump_part_path(output_path);
    let stdout_file =
        std::fs::File::create(&temp_path).map_err(|e| anyhow!("创建导出临时文件失败：{}", e))?;

    let child = match Command::new("pg_dump")
        .args(build_pg_dump_args(host, port, database, username))
        .envs(pg_client_env(
            password,
            ssl,
            tls_disabled,
            connect_timeout_secs,
        ))
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            let _ = std::fs::remove_file(&temp_path);
            return Err(anyhow!("启动 pg_dump 失败：{}", e));
        }
    };

    // wait_with_output 并发读完 stderr，避免管道写满导致子进程卡住；
    // 外层超时防止服务端无响应时导出无限挂起
    let output = match tokio::time::timeout(CLIENT_TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            let _ = std::fs::remove_file(&temp_path);
            return Err(anyhow!("等待 pg_dump 完成失败：{}", e));
        }
        Err(_) => {
            let _ = std::fs::remove_file(&temp_path);
            return Err(anyhow!(
                "pg_dump 超时（超过 {} 分钟），已终止子进程并删除临时文件",
                CLIENT_TIMEOUT.as_secs() / 60
            ));
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = std::fs::remove_file(&temp_path);
        return Err(anyhow!(
            "pg_dump 执行失败（退出码 {:?}）：{}",
            output.status.code(),
            tail_chars(stderr.trim(), 800)
        ));
    }

    if let Err(e) = verify_pg_dump_complete(&temp_path).await {
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

/// 校验 pg_dump plain 格式的完成标记。
///
/// pg_dump 正常结束时会写出 `-- PostgreSQL database dump complete`；
/// 缺少该行说明导出被中断（磁盘满、进程被杀等），必须丢弃临时文件。
async fn verify_pg_dump_complete(path: &Path) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};

    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|e| anyhow!("读取导出文件失败：{}", e))?;
    let len = file.metadata().await.map(|m| m.len()).unwrap_or(0);
    if len == 0 {
        return Err(anyhow!("pg_dump 未产生任何输出，导出已取消"));
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

    if !tail.contains("PostgreSQL database dump complete") {
        return Err(anyhow!(
            "导出文件不完整（缺少 pg_dump 完成标记），已放弃写入，原文件保持不变"
        ));
    }

    Ok(())
}

/// 把 SQL 脚本流式送入 `psql` 执行（PostgreSQL 还原 / 导入共用）。
///
/// 进度策略（psql 没有 `mysql -v` 的逐条回显）：
/// - `bytes` / `total`：实际写入子进程 stdin 的字节数，前端进度条据此推进；
/// - `lines`：只转发 psql stderr 上的 ERROR/WARNING 等服务端提示，不伪造「逐条语句成功」；
/// - `done` / `success`：无论成功、失败、超时都必定发送一次，避免前端进度条永久卡住。
///
/// 使用默认连接超时；持有连接池配置的调用方应改用
/// [`run_pg_script_file_with_connect_timeout`]。
#[allow(clippy::too_many_arguments)]
pub async fn run_pg_script_file(
    host: &str,
    port: u16,
    database: &str,
    username: &str,
    password: &str,
    ssl: &SslConfig,
    tls_disabled: bool,
    path: &Path,
    action: &str,
    reporter: Option<ImportReporter>,
) -> Result<()> {
    run_pg_script_file_with_connect_timeout(
        host,
        port,
        database,
        username,
        password,
        ssl,
        tls_disabled,
        DEFAULT_PG_CONNECT_TIMEOUT_SECS,
        path,
        action,
        reporter,
    )
    .await
}

/// 与 [`run_pg_script_file`] 相同，但连接超时取自连接池配置（`PoolConfig.connect_timeout`）。
#[allow(clippy::too_many_arguments)]
pub async fn run_pg_script_file_with_connect_timeout(
    host: &str,
    port: u16,
    database: &str,
    username: &str,
    password: &str,
    ssl: &SslConfig,
    tls_disabled: bool,
    connect_timeout_secs: u64,
    path: &Path,
    action: &str,
    reporter: Option<ImportReporter>,
) -> Result<()> {
    check_tool("psql", "--version").await?;

    let streaming = reporter.is_some();
    let total = if streaming {
        std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
    } else {
        0
    };

    let mut command = Command::new("psql");
    command
        .args(build_pg_script_args(host, port, database, username))
        .envs(pg_client_env(
            password,
            ssl,
            tls_disabled,
            connect_timeout_secs,
        ))
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        // 非进度模式下丢弃 stdout：导入脚本里偶发 SELECT 的输出不应写进 GUI 进程的 stdout
        .stdout(if streaming {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .kill_on_drop(true);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => return Err(anyhow!("启动 psql 客户端失败：{}", e)),
    };

    // 进度事件按 ~150ms 一批合并后推给前端（转发任务由 MySQL / PostgreSQL 共用）
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ImportMsg>();
    let written_bytes = Arc::new(AtomicU64::new(0));
    let forwarder = spawn_import_forwarder(reporter, total, written_bytes.clone(), rx);

    // 开启进度展示时把 psql 的 stdout 排空（脚本里的 SELECT/setval 等输出），
    // 否则管道写满会让子进程阻塞，表现为导入「卡住」。
    // 直接丢进 sink 而不是读进 Vec：大 dump 的 SELECT 输出可能有几十 MB，不能线性占内存。
    let stdout_task = if streaming {
        child.stdout.take().map(|mut stdout| {
            tokio::spawn(async move {
                let mut sink = tokio::io::sink();
                let _ = tokio::io::copy(&mut stdout, &mut sink).await;
            })
        })
    } else {
        None
    };

    // 并发读取 stderr：收集原始文本用于错误文案，同时把 ERROR/WARNING 实时推给前端
    let mut child_stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            if let Some(task) = stdout_task {
                task.abort();
            }
            let _ = child.kill().await;
            return Err(anyhow!("无法获取 psql 子进程 stderr"));
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
                        if !text.is_empty() {
                            // psql 的错误/告警形如 `psql:/path.sql:12: ERROR: ...`
                            let level =
                                if text.contains("ERROR:") || text.starts_with("psql: error") {
                                    "error"
                                } else {
                                    "warn"
                                };
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

    let mut stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => {
            abort_pg_tasks(&mut child, stdout_task, stderr_task).await;
            return Err(anyhow!("无法获取 psql 子进程 stdin"));
        }
    };
    let counter = written_bytes.clone();

    let operation = async {
        let write_result = match tokio::fs::File::open(path).await {
            Ok(mut file) => copy_into_stdin(&mut file, &mut stdin, &counter).await,
            Err(e) => Err(e),
        };
        // 关闭 stdin，让 psql 知道脚本结束（COPY ... FROM stdin 的数据块也已随脚本写完）
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
            let _ = tx.send(ImportMsg::Done(false));
            drop(tx);
            if let Some(forwarder) = forwarder {
                let _ = forwarder.await;
            }
            return Err(anyhow!(
                "psql {}超时（超过 {} 分钟），已终止子进程",
                action,
                CLIENT_TIMEOUT.as_secs() / 60
            ));
        }
    };

    if let Some(task) = stdout_task {
        let _ = task.await;
    }

    let status = status.map_err(|e| anyhow!("等待 psql {}完成失败：{}", action, e))?;
    let stderr_bytes = stderr_task.await.unwrap_or_default();
    let stderr_text = String::from_utf8_lossy(&stderr_bytes);
    let stderr_tail = tail_chars(stderr_text.trim(), 800);

    // psql 退出码 0 不等于还原成功：若脚本文件读取/写入 stdin 失败，psql 收到的是
    // 不完整脚本但仍可能正常退出，done 必须同时要求两端都成功，
    // 否则前端会先收到「成功」再收到返回的错误。
    let succeeded = status.success() && write_result.is_ok();
    // psql 逐条执行脚本，失败时 stderr 尾部就是最关键的报错；作为 error 流水推给前端
    if !succeeded && !stderr_tail.is_empty() {
        let _ = tx.send(ImportMsg::Line(ImportLogLine {
            no: None,
            level: "error",
            text: stderr_tail.clone(),
            elapsed_ms: None,
        }));
    }
    // done 在成功/失败路径都恰好发送一次（超时与 stdin 获取失败路径已单独处理）
    let _ = tx.send(ImportMsg::Done(succeeded));
    drop(tx);
    if let Some(forwarder) = forwarder {
        let _ = forwarder.await;
    }

    if let Err(e) = write_result {
        return Err(anyhow!(
            "向 psql 写入{}数据失败：{}；psql 输出：{}",
            action,
            e,
            stderr_tail
        ));
    }

    if !status.success() {
        return Err(anyhow!(
            "psql {}失败（退出码 {:?}）：{}",
            action,
            status.code(),
            stderr_tail
        ));
    }

    Ok(())
}

/// 子进程 stdin 获取失败时的清理：杀子进程并终止仍在运行的读取任务
async fn abort_pg_tasks(
    child: &mut tokio::process::Child,
    stdout_task: Option<tokio::task::JoinHandle<()>>,
    stderr_task: tokio::task::JoinHandle<Vec<u8>>,
) {
    if let Some(task) = stdout_task {
        task.abort();
    }
    stderr_task.abort();
    let _ = child.kill().await;
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

    // ---------------------------------------------------------------------
    // PostgreSQL：pg_dump / psql 参数与 SSL 环境变量
    // ---------------------------------------------------------------------

    #[test]
    fn pg_dump_args_are_plain_sql_without_clean() {
        let args = build_pg_dump_args("db.internal", 5432, "shop", "aurora");
        assert_eq!(
            args,
            vec![
                "--host",
                "db.internal",
                "--port",
                "5432",
                "--username",
                "aurora",
                "--dbname",
                "shop",
                "--no-password",
                "--format=plain",
                "--encoding=UTF8",
                "--no-owner",
                "--no-privileges",
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
        );
        // --clean 不默认加：导出脚本应能直接应用到空库
        assert!(!args.iter().any(|arg| arg == "--clean"));
    }

    #[test]
    fn pg_dump_args_follow_tunnel_local_port() {
        // SSH 隧道场景：主机被替换成 127.0.0.1，端口是隧道的本地端口
        let args = build_pg_dump_args("127.0.0.1", 49152, "shop", "aurora");
        assert_eq!(args[1], "127.0.0.1");
        assert_eq!(args[3], "49152");
    }

    #[test]
    fn pg_script_args_stop_on_first_error() {
        let args = build_pg_script_args("127.0.0.1", 5432, "shop", "aurora");
        // 与 mysql 客户端「遇错即停」对齐
        assert!(args.iter().any(|arg| arg == "--set=ON_ERROR_STOP=on"));
        // 用户 ~/.psqlrc 不能改变还原语义
        assert!(args.iter().any(|arg| arg == "--no-psqlrc"));
        assert!(args.iter().any(|arg| arg == "--no-password"));
    }

    #[test]
    fn pg_ssl_env_maps_connection_modes() {
        let cases = [
            (SslMode::Disabled, "disable"),
            (SslMode::Preferred, "prefer"),
            (SslMode::Required, "require"),
            (SslMode::VerifyCa, "verify-ca"),
            (SslMode::VerifyIdentity, "verify-full"),
        ];
        for (mode, expected) in cases {
            let env = pg_ssl_env(&SslConfig {
                mode,
                ..SslConfig::default()
            });
            assert_eq!(
                env,
                vec![("PGSSLMODE".to_string(), expected.to_string())],
                "模式 {mode:?} 应映射为 {expected}"
            );
        }
        // 默认配置是「首选加密」，与连接池默认保持一致
        assert_eq!(
            pg_ssl_env(&SslConfig::default()),
            vec![("PGSSLMODE".to_string(), "prefer".to_string())]
        );
    }

    #[test]
    fn pg_ssl_env_passes_only_configured_certificate_paths() {
        let ssl = SslConfig {
            mode: SslMode::VerifyIdentity,
            ca_path: Some(" /etc/ca.pem ".to_string()),
            cert_path: Some("/etc/client.pem".to_string()),
            key_path: Some("/etc/client.key".to_string()),
        };
        let env = pg_ssl_env(&ssl);
        assert!(env.contains(&("PGSSLROOTCERT".to_string(), "/etc/ca.pem".to_string())));
        assert!(env.contains(&("PGSSLCERT".to_string(), "/etc/client.pem".to_string())));
        assert!(env.contains(&("PGSSLKEY".to_string(), "/etc/client.key".to_string())));

        // 空串/空白路径视为未配置，不传给 libpq
        let blank = SslConfig {
            mode: SslMode::Required,
            ca_path: Some("   ".to_string()),
            cert_path: None,
            key_path: None,
        };
        assert_eq!(
            pg_ssl_env(&blank),
            vec![("PGSSLMODE".to_string(), "require".to_string())]
        );

        // disable 模式下即使传了证书路径也不应写入环境变量
        let disabled = SslConfig {
            mode: SslMode::Disabled,
            ca_path: Some("/etc/ca.pem".to_string()),
            cert_path: None,
            key_path: None,
        };
        assert_eq!(
            pg_ssl_env(&disabled),
            vec![("PGSSLMODE".to_string(), "disable".to_string())]
        );
    }

    #[test]
    fn pg_tls_fallback_forces_disable_and_keeps_password_in_env() {
        let required = SslConfig {
            mode: SslMode::Required,
            ca_path: Some("/etc/ca.pem".to_string()),
            cert_path: None,
            key_path: None,
        };
        // MySQL「首选」TLS 握手失败会回退明文；PostgreSQL 必须同步降级成 disable，
        // 否则外部工具仍会尝试 TLS，与已建立的连接状态不一致
        assert_eq!(pg_effective_ssl(&required, true).mode, SslMode::Disabled);
        assert_eq!(pg_effective_ssl(&required, false).mode, SslMode::Required);
        assert_eq!(
            pg_ssl_env(&pg_effective_ssl(&required, true)),
            vec![("PGSSLMODE".to_string(), "disable".to_string())]
        );

        // 密码只走 PGPASSWORD 环境变量（不进命令行参数、不落临时文件）
        let env = pg_client_env("s3cret", &required, false, DEFAULT_PG_CONNECT_TIMEOUT_SECS);
        assert!(env.contains(&("PGPASSWORD".to_string(), "s3cret".to_string())));
        assert!(env.contains(&("PGSSLMODE".to_string(), "require".to_string())));
        assert!(env.contains(&("PGSSLROOTCERT".to_string(), "/etc/ca.pem".to_string())));
        // 主机不可达时 pg_dump/psql 不能等满 CLIENT_TIMEOUT：默认连接超时来自连接池默认值
        assert!(env.contains(&("PGCONNECT_TIMEOUT".to_string(), "10".to_string())));
        // 非法/极端配置也要给出合法值（libpq 要求正整数秒）
        let env = pg_client_env("p", &required, false, 0);
        assert!(env.contains(&("PGCONNECT_TIMEOUT".to_string(), "1".to_string())));
    }

    #[tokio::test]
    async fn pg_dump_completion_marker_is_verified_at_tail_boundary() {
        let dir = std::env::temp_dir().join(format!(
            "auroradb-pg-dump-verify-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let marker = "-- PostgreSQL database dump complete\n";

        // 完整导出：完成标记在文件末尾 → 通过
        let complete = dir.join("complete.sql");
        std::fs::write(
            &complete,
            format!("-- PostgreSQL database dump\nCREATE TABLE t (id int);\n{marker}"),
        )
        .unwrap();
        assert!(verify_pg_dump_complete(&complete).await.is_ok());

        // 被截断的导出（没有完成标记）→ 拒绝
        let truncated = dir.join("truncated.sql");
        std::fs::write(
            &truncated,
            "-- PostgreSQL database dump\nCREATE TABLE t (id int);\n",
        )
        .unwrap();
        assert!(verify_pg_dump_complete(&truncated).await.is_err());

        // 空文件 → 拒绝
        let empty = dir.join("empty.sql");
        std::fs::write(&empty, "").unwrap();
        assert!(verify_pg_dump_complete(&empty).await.is_err());

        // 只读尾部 1024 字节：标记必须落在尾窗内；
        // 标记之前填充 1000 字节仍在窗内 → 通过
        let near_edge = dir.join("near_edge.sql");
        let mut content = String::from("-- PostgreSQL database dump\n");
        content.push_str(&"x".repeat(1000));
        content.push('\n');
        content.push_str(marker);
        std::fs::write(&near_edge, content).unwrap();
        assert!(verify_pg_dump_complete(&near_edge).await.is_ok());

        // 标记之后填充超过 1024 字节的注释，标记被挤出尾窗 → 拒绝（不会误判为完整）
        let pushed_out = dir.join("pushed_out.sql");
        let content = format!("-- PostgreSQL database dump\n{marker}{}", "y".repeat(1100));
        std::fs::write(&pushed_out, content).unwrap();
        assert!(verify_pg_dump_complete(&pushed_out).await.is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---------------------------------------------------------------------
    // PostgreSQL 真实库轮转：本机/CI 缺少 pg_dump、psql 或未配置
    // AURORADB_TEST_PG_* 时自动跳过（打印跳过原因，不算失败）
    // ---------------------------------------------------------------------

    fn pg_test_env() -> Option<(String, u16, String, String, String)> {
        let host = std::env::var("AURORADB_TEST_PG_HOST").ok()?;
        let port = std::env::var("AURORADB_TEST_PG_PORT")
            .ok()?
            .parse::<u16>()
            .ok()?;
        let username = std::env::var("AURORADB_TEST_PG_USER").ok()?;
        let password = std::env::var("AURORADB_TEST_PG_PASSWORD").unwrap_or_default();
        let database = std::env::var("AURORADB_TEST_PG_DATABASE").ok()?;
        Some((host, port, username, password, database))
    }

    fn pg_pool_url(
        host: &str,
        port: u16,
        username: &str,
        password: &str,
        database: &str,
    ) -> String {
        format!("postgres://{username}:{password}@{host}:{port}/{database}")
    }

    /// 整库导出 → 还原到另一个空库：数据与表结构应完整迁移，进度事件必须正确收尾。
    ///
    /// 只在提供真实 PostgreSQL 与 pg_dump/psql 时运行；否则打印跳过原因并返回。
    /// 测试库名带进程号并在结束时删除，避免并行执行时互相干扰。
    #[tokio::test]
    #[ignore = "需要真实 PostgreSQL 与 pg_dump/psql：设置 AURORADB_TEST_PG_* 后执行 cargo test -- --ignored"]
    async fn pg_dump_and_psql_restore_roundtrip() {
        let Some((host, port, username, password, database)) = pg_test_env() else {
            eprintln!("跳过：未设置 AURORADB_TEST_PG_* 环境变量");
            return;
        };
        if let Err(e) = check_tool("pg_dump", "--version").await {
            eprintln!("跳过：{e}");
            return;
        }
        if let Err(e) = check_tool("psql", "--version").await {
            eprintln!("跳过：{e}");
            return;
        }

        let pid = std::process::id();
        let src_db = format!("qa_pg_src_{pid}");
        let dst_db = format!("qa_pg_dst_{pid}");
        let table = format!("qa_rows_{pid}");
        let dump_file = std::env::temp_dir().join(format!("qa_pg_dump_{pid}.sql"));

        let admin = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&pg_pool_url(&host, port, &username, &password, &database))
            .await
            .expect("连接 PostgreSQL 维护库失败");

        // 兼容上次异常中断：先删同名库（WITH (FORCE) 断开残留连接）
        for name in [&src_db, &dst_db] {
            let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)"))
                .execute(&admin)
                .await;
        }
        for name in [&src_db, &dst_db] {
            sqlx::query(&format!("CREATE DATABASE \"{name}\""))
                .execute(&admin)
                .await
                .expect("创建测试库失败");
        }

        // 源库：覆盖文本（含中文与单引号）、二进制、时间戳与 NULL
        let src = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&pg_pool_url(&host, port, &username, &password, &src_db))
            .await
            .expect("连接源库失败");
        sqlx::query(&format!(
            "CREATE TABLE \"{table}\" (id INT PRIMARY KEY, note TEXT, payload BYTEA, created TIMESTAMPTZ)"
        ))
        .execute(&src)
        .await
        .expect("创建源表失败");
        sqlx::query(&format!(
            "INSERT INTO \"{table}\" (id, note, payload, created) VALUES \
             (1, '中文 O''Brien', '\\xdeadbeef'::bytea, '2024-01-02 03:04:05+00'), \
             (2, NULL, NULL, NULL)"
        ))
        .execute(&src)
        .await
        .expect("写入源数据失败");
        src.close().await;

        // 导出：plain SQL，且必须带 pg_dump 完成标记
        let size = run_pg_dump_to_file(
            &host,
            port,
            &src_db,
            &username,
            &password,
            &SslConfig::default(),
            false,
            &dump_file,
        )
        .await
        .expect("pg_dump 导出失败");
        assert!(size > 0, "导出文件不应为空");
        let content = std::fs::read_to_string(&dump_file).expect("读取导出文件失败");
        assert!(
            content.contains("PostgreSQL database dump complete"),
            "导出文件缺少完成标记：{content}"
        );

        // 还原：进度事件最后的 done/success/bytes 必须正确，进度条才能正常结束
        let events: Arc<std::sync::Mutex<Vec<ImportProgress>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = events.clone();
        let reporter: ImportReporter = Arc::new(move |progress| {
            sink.lock().unwrap().push(progress);
        });
        run_pg_script_file(
            &host,
            port,
            &dst_db,
            &username,
            &password,
            &SslConfig::default(),
            false,
            &dump_file,
            "还原",
            Some(reporter),
        )
        .await
        .expect("psql 还原失败");

        {
            // 作用域收紧：断言后立即释放锁，避免在后续 await 期间持有 std::sync::Mutex
            let events = events.lock().unwrap();
            let last = events.last().expect("还原应至少推送一次进度事件");
            assert!(last.done, "最后一条进度事件必须标记结束");
            assert!(last.success, "还原成功时最后一条事件必须标记成功");
            assert_eq!(last.total, size, "进度事件的总字节数应等于导出文件大小");
            assert_eq!(last.bytes, size, "字节进度应覆盖整个文件");
        }

        // 目标库：表结构与数据都应在
        let dst = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&pg_pool_url(&host, port, &username, &password, &dst_db))
            .await
            .expect("连接目标库失败");
        let rows: Vec<(i32, Option<String>)> = sqlx::query_as::<_, (i32, Option<String>)>(
            &format!("SELECT id, note FROM \"{table}\" ORDER BY id"),
        )
        .fetch_all(&dst)
        .await
        .expect("查询还原后的数据失败");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, 1);
        assert_eq!(rows[0].1.as_deref(), Some("中文 O'Brien"));
        assert_eq!(rows[1].1, None);
        dst.close().await;
        drop(events);

        // 清理：对象名带 pid，用后即删
        for name in [&src_db, &dst_db] {
            let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)"))
                .execute(&admin)
                .await;
        }
        let _ = std::fs::remove_file(&dump_file);
        admin.close().await;
    }
}
