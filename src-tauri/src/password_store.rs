use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::State;

/// 系统凭据库中的服务名（与 tauri.conf.json 的 identifier 保持一致）
const KEYRING_SERVICE: &str = "com.auroradb.app";
/// 连接元数据索引（不含密码）
const INDEX_FILE: &str = "connection_passwords.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPassword {
    pub connection_id: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

/// 落盘的连接元数据，**不包含密码**；密码本身存放在系统凭据库
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PasswordMeta {
    connection_id: String,
    host: String,
    port: u16,
    username: String,
}

#[derive(Default)]
struct Inner {
    meta: HashMap<String, PasswordMeta>,
    /// 进程内密码缓存，避免每次读取都请求系统凭据库
    secrets: HashMap<String, String>,
}

/// 密码存储管理器
///
/// 密码写入系统凭据库（Linux: Secret Service / macOS: Keychain / Windows: Credential Manager），
/// 本地仅保留连接元数据索引。
pub struct PasswordStore {
    inner: Mutex<Inner>,
    index_path: PathBuf,
}

impl PasswordStore {
    /// 取内部锁；锁被 panic 污染时仍可恢复，避免整个密码功能不可用
    fn lock_inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|poisoned| {
            tracing::warn!("Password store lock was poisoned, recovering inner state");
            poisoned.into_inner()
        })
    }

    pub fn new() -> anyhow::Result<Self> {
        let data_dir = dirs::data_dir()
            .ok_or_else(|| anyhow::anyhow!("无法获取数据目录"))?
            .join("mysql-client");

        fs::create_dir_all(&data_dir)?;

        let index_path = data_dir.join(INDEX_FILE);
        let inner = match Self::load_index(&index_path) {
            Ok(meta) => Inner {
                meta,
                secrets: HashMap::new(),
            },
            Err(e) => {
                tracing::warn!("Failed to load password index: {}", e);
                Inner::default()
            }
        };

        Ok(Self {
            inner: Mutex::new(inner),
            index_path,
        })
    }

    fn load_index(path: &Path) -> anyhow::Result<HashMap<String, PasswordMeta>> {
        if !path.exists() {
            return Ok(HashMap::new());
        }
        let data = fs::read(path)?;
        let list: Vec<PasswordMeta> = serde_json::from_slice(&data)?;
        Ok(list
            .into_iter()
            .map(|m| (m.connection_id.clone(), m))
            .collect())
    }

    fn write_index(&self, inner: &Inner) -> anyhow::Result<()> {
        let list: Vec<&PasswordMeta> = inner.meta.values().collect();
        let data = serde_json::to_vec(&list)?;
        fs::write(&self.index_path, data)?;
        Self::restrict_permissions(&self.index_path);
        Ok(())
    }

    /// 索引只含主机/用户名等元数据，但仍按 0600 收紧权限
    fn restrict_permissions(path: &Path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
        }
        #[cfg(not(unix))]
        let _ = path;
    }

    fn entry(connection_id: &str) -> anyhow::Result<keyring::Entry> {
        keyring::Entry::new(KEYRING_SERVICE, connection_id)
            .context("无法打开系统密钥环条目")
    }

    fn write_secret(connection_id: &str, password: &str) -> anyhow::Result<()> {
        Self::entry(connection_id)?
            .set_password(password)
            .context("无法将密码写入系统密钥环")
    }

    fn read_secret(connection_id: &str) -> anyhow::Result<Option<String>> {
        match Self::entry(connection_id)?.get_password() {
            Ok(password) => Ok(Some(password)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(anyhow::Error::new(e).context("无法从系统密钥环读取密码")),
        }
    }

    fn remove_secret(connection_id: &str) -> anyhow::Result<()> {
        match Self::entry(connection_id)?.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(anyhow::Error::new(e).context("无法从系统密钥环删除密码")),
        }
    }

    /// 组合元数据与凭据库中的密码
    fn resolve(&self, meta: PasswordMeta) -> Option<StoredPassword> {
        if let Some(password) = self
            .lock_inner()
            .secrets
            .get(&meta.connection_id)
            .cloned()
        {
            return Some(Self::assemble(meta, password));
        }

        match Self::read_secret(&meta.connection_id) {
            Ok(Some(password)) => {
                self.lock_inner()
                    .secrets
                    .insert(meta.connection_id.clone(), password.clone());
                Some(Self::assemble(meta, password))
            }
            Ok(None) => {
                tracing::warn!(
                    "No password found in system keyring for connection {}",
                    meta.connection_id
                );
                None
            }
            Err(e) => {
                tracing::error!("Failed to read password from system keyring: {}", e);
                None
            }
        }
    }

    fn assemble(meta: PasswordMeta, password: String) -> StoredPassword {
        StoredPassword {
            connection_id: meta.connection_id,
            host: meta.host,
            port: meta.port,
            username: meta.username,
            password,
        }
    }

    pub fn save(&self, password: StoredPassword) -> anyhow::Result<()> {
        let previous_keyring_secret =
            Self::read_secret(&password.connection_id).unwrap_or(None);
        Self::write_secret(&password.connection_id, &password.password)?;

        let mut inner = self.lock_inner();
        let previous_meta = inner.meta.insert(
            password.connection_id.clone(),
            PasswordMeta {
                connection_id: password.connection_id.clone(),
                host: password.host.clone(),
                port: password.port,
                username: password.username.clone(),
            },
        );
        let previous_secret = inner
            .secrets
            .insert(password.connection_id.clone(), password.password.clone());
        if let Err(e) = self.write_index(&inner) {
            // 索引写失败时回滚内存与凭据库，避免「索引与凭据库不一致」的半成功状态
            tracing::error!("Failed to write password index: {}", e);
            match previous_meta {
                Some(meta) => {
                    inner.meta.insert(password.connection_id.clone(), meta);
                }
                None => {
                    inner.meta.remove(&password.connection_id);
                }
            }
            match previous_secret {
                Some(secret) => {
                    inner.secrets.insert(password.connection_id.clone(), secret);
                }
                None => {
                    inner.secrets.remove(&password.connection_id);
                }
            }
            let revert = match previous_keyring_secret {
                Some(secret) => Self::write_secret(&password.connection_id, &secret),
                None => Self::remove_secret(&password.connection_id),
            };
            if let Err(revert_err) = revert {
                tracing::error!("Failed to roll back password in system keyring: {}", revert_err);
            }
            return Err(e);
        }
        Ok(())
    }

    pub fn get(&self, connection_id: &str) -> Option<StoredPassword> {
        let meta = self.lock_inner().meta.get(connection_id).cloned()?;
        self.resolve(meta)
    }

    pub fn delete(&self, connection_id: &str) -> anyhow::Result<()> {
        let mut inner = self.lock_inner();
        let removed_meta = inner.meta.remove(connection_id);
        let removed_secret = inner.secrets.remove(connection_id);
        // 先落索引再删凭据：索引写失败可恢复内存状态，不会出现「密码已删但索引还在」
        if let Err(e) = self.write_index(&inner) {
            if let Some(meta) = removed_meta {
                inner.meta.insert(connection_id.to_string(), meta);
            }
            if let Some(secret) = removed_secret {
                inner.secrets.insert(connection_id.to_string(), secret);
            }
            return Err(e);
        }
        drop(inner);

        Self::remove_secret(connection_id)?;
        Ok(())
    }

    /// 查找匹配的连接密码
    pub fn find_by_host_port_user(
        &self,
        host: &str,
        port: u16,
        username: &str,
    ) -> Option<StoredPassword> {
        let meta = {
            let inner = self.lock_inner();
            inner
                .meta
                .values()
                .find(|m| m.host == host && m.port == port && m.username == username)
                .cloned()
        }?;
        self.resolve(meta)
    }
}

// Tauri Commands

#[tauri::command]
pub fn save_connection_password(
    store: State<'_, PasswordStore>,
    connection_id: String,
    host: String,
    port: u16,
    username: String,
    password: String,
) -> Result<(), String> {
    let stored = StoredPassword {
        connection_id,
        host,
        port,
        username,
        password,
    };
    store.save(stored).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_connection_password(
    store: State<'_, PasswordStore>,
    connection_id: String,
) -> Result<Option<StoredPassword>, String> {
    Ok(store.get(&connection_id))
}

#[tauri::command]
pub fn delete_connection_password(
    store: State<'_, PasswordStore>,
    connection_id: String,
) -> Result<(), String> {
    store.delete(&connection_id).map_err(|e| e.to_string())?;
    Ok(())
}

/// 根据连接信息查找密码
#[tauri::command]
pub fn find_connection_password(
    store: State<'_, PasswordStore>,
    host: String,
    port: u16,
    username: String,
) -> Result<Option<StoredPassword>, String> {
    Ok(store.find_by_host_port_user(&host, port, &username))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_roundtrip_keeps_meta_only() {
        let meta = vec![PasswordMeta {
            connection_id: "c1".into(),
            host: "localhost".into(),
            port: 3306,
            username: "root".into(),
        }];
        let data = serde_json::to_vec(&meta).unwrap();
        let text = String::from_utf8(data.clone()).unwrap();
        assert!(!text.contains("password"));
        let parsed: Vec<PasswordMeta> = serde_json::from_slice(&data).unwrap();
        assert_eq!(parsed[0].connection_id, "c1");
        assert_eq!(parsed[0].port, 3306);
    }
}
