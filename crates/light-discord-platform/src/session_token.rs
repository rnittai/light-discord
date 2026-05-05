use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionTokenStore {
    Keyring,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSessionToken {
    pub token: String,
    pub store: SessionTokenStore,
}

pub fn load_session_token(server_addr: &str) -> anyhow::Result<Option<StoredSessionToken>> {
    let key = derive_key(server_addr);

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    if let Some(token) = keyring_load(&key) {
        return Ok(Some(StoredSessionToken {
            token,
            store: SessionTokenStore::Keyring,
        }));
    }

    let path = fallback_path(&key)?;
    match read_fallback_file(&path)? {
        Some(token) => Ok(Some(StoredSessionToken {
            token,
            store: SessionTokenStore::File,
        })),
        None => Ok(None),
    }
}

pub fn save_session_token(
    server_addr: &str,
    session_token: &str,
) -> anyhow::Result<SessionTokenStore> {
    if session_token.trim().is_empty() {
        return Err(anyhow::anyhow!(
            "session token must not be empty or whitespace-only"
        ));
    }

    let key = derive_key(server_addr);

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    if keyring_save(&key, session_token) {
        return Ok(SessionTokenStore::Keyring);
    }

    let path = fallback_path(&key)?;
    write_fallback_file(&path, session_token)?;
    Ok(SessionTokenStore::File)
}

pub fn delete_session_token(server_addr: &str) -> anyhow::Result<()> {
    let key = derive_key(server_addr);

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    keyring_delete(&key);

    let path = fallback_path(&key)?;
    delete_fallback_file(&path)
}

fn derive_key(server_addr: &str) -> String {
    use sha2::Digest;
    let hash = sha2::Sha256::digest(server_addr.as_bytes());
    bytes_to_hex(&hash)
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn fallback_path(key: &str) -> anyhow::Result<PathBuf> {
    let mut dir = fallback_dir()?;
    dir.push(format!("{key}.token"));
    Ok(dir)
}

fn fallback_dir() -> anyhow::Result<PathBuf> {
    if let Ok(root) = std::env::var("LIGHT_DISCORD_CONFIG_DIR") {
        let mut p = PathBuf::from(root);
        p.push("session-tokens");
        return Ok(p);
    }
    os_config_dir()
}

#[cfg(target_os = "linux")]
fn os_config_dir() -> anyhow::Result<PathBuf> {
    let base = match std::env::var("XDG_CONFIG_HOME") {
        Ok(v) => PathBuf::from(v),
        Err(_) => {
            let home = std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME is not set"))?;
            PathBuf::from(home).join(".config")
        }
    };
    Ok(base.join("light-discord").join("session-tokens"))
}

#[cfg(target_os = "windows")]
fn os_config_dir() -> anyhow::Result<PathBuf> {
    let base = match std::env::var("APPDATA") {
        Ok(v) => PathBuf::from(v),
        Err(_) => {
            let profile = std::env::var("USERPROFILE")
                .map_err(|_| anyhow::anyhow!("USERPROFILE is not set"))?;
            PathBuf::from(profile).join("AppData").join("Roaming")
        }
    };
    Ok(base.join("LightDiscord").join("session-tokens"))
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn os_config_dir() -> anyhow::Result<PathBuf> {
    anyhow::bail!("session token storage is not supported on this platform")
}

fn read_fallback_file(path: &Path) -> anyhow::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

#[cfg(unix)]
fn write_fallback_file(path: &Path, token: &str) -> anyhow::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(token.as_bytes())?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn write_fallback_file(path: &Path, token: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, token)?;
    Ok(())
}

fn delete_fallback_file(path: &Path) -> anyhow::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn keyring_load(key: &str) -> Option<String> {
    let entry = keyring::Entry::new("light-discord", key).ok()?;
    match entry.get_password() {
        Ok(pw) => Some(pw),
        Err(keyring::Error::NoEntry) => None,
        Err(_) => None,
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn keyring_save(key: &str, token: &str) -> bool {
    match keyring::Entry::new("light-discord", key) {
        Ok(entry) => entry.set_password(token).is_ok(),
        Err(_) => false,
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn keyring_delete(key: &str) {
    if let Ok(entry) = keyring::Entry::new("light-discord", key) {
        let _ = entry.delete_credential();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn test_derive_key_length() {
        let key = derive_key("localhost:8080");
        assert_eq!(key.len(), 64, "SHA-256 hex output must be 64 characters");
    }

    #[test]
    fn test_derive_key_stability() {
        let a = derive_key("example.com:443");
        let b = derive_key("example.com:443");
        assert_eq!(a, b, "derive_key must be deterministic");
    }

    #[test]
    fn test_derive_key_distinct_inputs() {
        let a = derive_key("host-a:8080");
        let b = derive_key("host-b:8080");
        assert_ne!(a, b);
    }

    #[test]
    fn test_derive_key_is_lowercase_hex() {
        let key = derive_key("some-server:9000");
        assert!(
            key.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
            "key must be lowercase hex"
        );
    }

    #[test]
    fn test_fallback_path_filename_format() {
        let key = derive_key("testhost:1234");
        let filename = format!("{key}.token");
        assert!(filename.starts_with(&key));
        assert!(filename.ends_with(".token"));
        assert_eq!(filename.len(), 64 + ".token".len());
    }

    #[test]
    fn test_fallback_file_write_read_delete() {
        let dir = std::env::temp_dir().join(format!("ld_test_{}", std::process::id()));
        let path = dir.join("test_session.token");

        write_fallback_file(&path, "my-secret-session-token").unwrap();

        let read_back = read_fallback_file(&path).unwrap();
        assert_eq!(read_back, Some("my-secret-session-token".to_string()));

        delete_fallback_file(&path).unwrap();

        let after_delete = read_fallback_file(&path).unwrap();
        assert_eq!(after_delete, None);

        // Deleting a missing file must succeed (idempotent).
        delete_fallback_file(&path).unwrap();

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_fallback_path_uses_config_dir_override() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let previous = std::env::var_os("LIGHT_DISCORD_CONFIG_DIR");
        let dir = std::env::temp_dir().join(format!(
            "ld_config_override_{}_{}",
            std::process::id(),
            derive_key("override-test")
        ));

        unsafe {
            std::env::set_var("LIGHT_DISCORD_CONFIG_DIR", &dir);
        }
        let path = fallback_path(&derive_key("server.example:41610")).unwrap();

        match previous {
            Some(value) => unsafe {
                std::env::set_var("LIGHT_DISCORD_CONFIG_DIR", value);
            },
            None => unsafe {
                std::env::remove_var("LIGHT_DISCORD_CONFIG_DIR");
            },
        }
        let _ = std::fs::remove_dir_all(&dir);

        assert!(path.starts_with(dir.join("session-tokens")));
        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("token"));
    }

    #[cfg(unix)]
    #[test]
    fn test_fallback_file_permissions_are_restricted_after_overwrite() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!(
            "ld_test_mode_{}_{}",
            std::process::id(),
            derive_key("mode-test")
        ));
        let path = dir.join("test_session.token");

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, "old-token").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        write_fallback_file(&path, "new-token").unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_fallback_file_read_missing() {
        let path = std::env::temp_dir().join("ld_test_nonexistent_abc123.token");
        // Ensure it does not exist.
        let _ = std::fs::remove_file(&path);
        let result = read_fallback_file(&path).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_save_rejects_empty_token() {
        let result = save_session_token("host:8080", "");
        assert!(result.is_err());
    }

    #[test]
    fn test_save_rejects_whitespace_only_token() {
        let result = save_session_token("host:8080", "   \t\n");
        assert!(result.is_err());
    }
}
