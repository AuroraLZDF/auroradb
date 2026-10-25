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
    pub fn new() -> anyhow::Result<Self> {
        let data_dir = dirs::data_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not get data directory"))?
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
            .context("Failed to open system keyring entry")
    }

    fn write_secret(connection_id: &str, password: &str) -> anyhow::Result<()> {
        Self::entry(connection_id)?
            .set_password(password)
            .context("Failed to write password to system keyring")
    }

    fn read_secret(connection_id: &str) -> anyhow::Result<Option<String>> {
        match Self::entry(connection_id)?.get_password() {
            Ok(password) => Ok(Some(password)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(anyhow::Error::new(e).context("Failed to read password from system keyring")),
        }
    }

    fn remove_secret(connection_id: &str) -> anyhow::Result<()> {
        match Self::entry(connection_id)?.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(anyhow::Error::new(e).context("Failed to delete password from system keyring")),
        }
    }

    /// 组合元数据与凭据库中的密码
    fn resolve(&self, meta: PasswordMeta) -> Option<StoredPassword> {
        if let Some(password) = self
            .inner
            .lock()
            .unwrap()
            .secrets
            .get(&meta.connection_id)
            .cloned()
        {
            return Some(Self::assemble(meta, password));
        }

        match Self::read_secret(&meta.connection_id) {
            Ok(Some(password)) => {
                self.inner
                    .lock()
                    .unwrap()
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
        Self::write_secret(&password.connection_id, &password.password)?;

        let mut inner = self.inner.lock().unwrap();
        inner.meta.insert(
            password.connection_id.clone(),
            PasswordMeta {
                connection_id: password.connection_id.clone(),
                host: password.host.clone(),
                port: password.port,
                username: password.username.clone(),
            },
        );
        inner
            .secrets
            .insert(password.connection_id.clone(), password.password.clone());
        if let Err(e) = self.write_index(&inner) {
            tracing::error!("Failed to write password index: {}", e);
            return Err(e);
        }
        Ok(())
    }

    pub fn get(&self, connection_id: &str) -> Option<StoredPassword> {
        let meta = self.inner.lock().unwrap().meta.get(connection_id).cloned()?;
        self.resolve(meta)
    }

    pub fn delete(&self, connection_id: &str) -> anyhow::Result<()> {
        Self::remove_secret(connection_id)?;

        let mut inner = self.inner.lock().unwrap();
        inner.meta.remove(connection_id);
        inner.secrets.remove(connection_id);
        self.write_index(&inner)?;
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
            let inner = self.inner.lock().unwrap();
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
