//! Account credentials (GOG tokens, the itch.io API key), kept out of the database.
//!
//! They live in `secrets.bin` next to the database. On Windows the file is encrypted with DPAPI
//! for the current user; elsewhere it is plain JSON readable only by its owner (0600). Nothing
//! here is ever sent to the UI, logged or served by `gamelib-cli serve`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

const FILE_NAME: &str = "secrets.bin";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Secrets {
    #[serde(default)]
    pub gog: Option<GogTokens>,
    #[serde(default)]
    pub itch: Option<ItchKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GogTokens {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix time the access token stops working.
    pub expires_at: i64,
    pub user_id: String,
    pub username: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItchKey {
    pub api_key: String,
    pub user_id: u64,
    pub username: String,
}

pub struct SecretStore {
    path: PathBuf,
    /// Serializes read-modify-write cycles within this process.
    lock: Mutex<()>,
}

impl SecretStore {
    /// Credentials stored in `dir` (the database's folder).
    pub fn new(dir: &Path) -> Self {
        Self {
            path: dir.join(FILE_NAME),
            lock: Mutex::new(()),
        }
    }

    pub fn load(&self) -> Result<Secrets> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        self.read()
    }

    /// Applies `change` to the stored credentials and saves them.
    pub fn update<T>(&self, change: impl FnOnce(&mut Secrets) -> T) -> Result<T> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let mut secrets = self.read()?;
        let out = change(&mut secrets);
        self.write(&secrets)?;
        Ok(out)
    }

    fn read(&self) -> Result<Secrets> {
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Secrets::default()),
            Err(e) => return Err(io_error(&self.path, e)),
        };
        let plain = unprotect(&bytes)?;
        // A damaged or foreign file is treated as "signed out" rather than an error loop.
        Ok(serde_json::from_slice(&plain).unwrap_or_default())
    }

    fn write(&self, secrets: &Secrets) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| io_error(dir, e))?;
        }
        let plain = serde_json::to_vec(secrets)?;
        let data = protect(&plain)?;
        let tmp = self.path.with_extension("tmp");
        write_private(&tmp, &data).map_err(|e| io_error(&tmp, e))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| io_error(&self.path, e))
    }
}

fn io_error(path: &Path, e: std::io::Error) -> Error {
    Error::Other(format!("{}: {e}", path.display()))
}

#[cfg(unix)]
fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(data)?;
    file.sync_all()
}

#[cfg(not(unix))]
fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    // The data is DPAPI-encrypted; %LOCALAPPDATA% is already private to the user.
    std::fs::write(path, data)
}

#[cfg(windows)]
fn protect(data: &[u8]) -> Result<Vec<u8>> {
    dpapi::protect(data).map_err(|e| Error::Other(format!("DPAPI: {e}")))
}

#[cfg(windows)]
fn unprotect(data: &[u8]) -> Result<Vec<u8>> {
    // Unreadable (another user's or a copied file): start signed out.
    Ok(dpapi::unprotect(data).unwrap_or_default())
}

#[cfg(not(windows))]
fn protect(data: &[u8]) -> Result<Vec<u8>> {
    Ok(data.to_vec())
}

#[cfg(not(windows))]
fn unprotect(data: &[u8]) -> Result<Vec<u8>> {
    Ok(data.to_vec())
}

#[cfg(windows)]
mod dpapi {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };
    use windows::core::PCWSTR;

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        }
    }

    /// Copies DPAPI's output and frees it.
    fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        if out.pbData.is_null() {
            return Vec::new();
        }
        // SAFETY: DPAPI returned `cbData` bytes at `pbData`, allocated with LocalAlloc.
        let bytes = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
        // SAFETY: the buffer came from DPAPI and is freed exactly once.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(out.pbData.cast())));
        }
        bytes
    }

    pub fn protect(data: &[u8]) -> windows::core::Result<Vec<u8>> {
        let input = blob(data);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: `input` points at `data`, which outlives the call; `out` receives a new buffer.
        unsafe {
            CryptProtectData(
                &input,
                PCWSTR::null(),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )?;
        }
        Ok(take(out))
    }

    pub fn unprotect(data: &[u8]) -> windows::core::Result<Vec<u8>> {
        let input = blob(data);
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: as in `protect`.
        unsafe {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )?;
        }
        Ok(take(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gamelib-secrets-{tag}-{}-{}",
            std::process::id(),
            crate::unix_now()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn round_trip_and_update() {
        let dir = temp_dir("rt");
        let store = SecretStore::new(&dir);
        assert_eq!(store.load().unwrap(), Secrets::default());
        store
            .update(|s| {
                s.itch = Some(ItchKey {
                    api_key: "k".into(),
                    user_id: 7,
                    username: "leafo".into(),
                })
            })
            .unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.itch.unwrap().username, "leafo");
        assert!(loaded.gog.is_none());

        // Nothing readable as plain text on Windows; owner-only on Unix.
        let raw = std::fs::read(dir.join(FILE_NAME)).unwrap();
        #[cfg(windows)]
        assert!(!String::from_utf8_lossy(&raw).contains("leafo"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join(FILE_NAME))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
            assert!(!raw.is_empty());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn damaged_file_means_signed_out() {
        let dir = temp_dir("bad");
        std::fs::write(dir.join(FILE_NAME), b"\x00not json").unwrap();
        assert_eq!(SecretStore::new(&dir).load().unwrap(), Secrets::default());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
