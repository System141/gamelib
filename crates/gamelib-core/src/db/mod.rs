//! Local SQLite catalog.
//!
//! One file holds the Steam catalog (refreshable) and the user's own data (external links).
//! WAL mode lets the UI read while a sync writes from another connection.

pub mod downloads;
pub mod installs;
pub mod links;
pub mod read;
pub mod schema;
pub mod stores;
pub mod write;

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use crate::Result;

pub struct Db {
    conn: Connection,
    /// Cached result counts keyed by query, valid for one `data_version`.
    count_cache: HashMap<String, u32>,
    count_cache_version: i64,
}

impl Db {
    /// Opens (or creates) a database file and brings the schema up to date.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)
                .map_err(|e| crate::Error::Other(format!("{}: {e}", dir.display())))?;
        }
        Self::init(Connection::open(path)?, false)
    }

    /// Opens a connection that refuses writes, for UI queries. Call [`Db::open`] first so the
    /// schema exists.
    pub fn open_reader(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        Self::init(conn, true)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?, false)
    }

    fn init(mut conn: Connection, read_only: bool) -> Result<Self> {
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get::<_, String>(0))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "temp_store", "MEMORY")?;
        conn.pragma_update(None, "cache_size", -32000)?;
        conn.set_prepared_statement_cache_capacity(64);
        if read_only {
            conn.pragma_update(None, "query_only", true)?;
        } else {
            schema::migrate(&mut conn)?;
        }
        Ok(Self {
            conn,
            count_cache: HashMap::new(),
            count_cache_version: -1,
        })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Changes whenever another connection commits (and on local writes via `total_changes`).
    fn data_version(&self) -> Result<i64> {
        let version: i64 = self
            .conn
            .pragma_query_value(None, "data_version", |r| r.get(0))?;
        Ok(version
            .wrapping_mul(1_000_003)
            .wrapping_add(self.conn.total_changes() as i64))
    }

    /// Runs `count` unless an identical query was counted since the last data change.
    pub(crate) fn cached_count(
        &mut self,
        key: &str,
        count: impl FnOnce(&Connection) -> Result<u32>,
    ) -> Result<u32> {
        let version = self.data_version()?;
        if version != self.count_cache_version {
            self.count_cache.clear();
            self.count_cache_version = version;
        }
        if let Some(&n) = self.count_cache.get(key) {
            return Ok(n);
        }
        let n = count(&self.conn)?;
        if self.count_cache.len() >= 64 {
            self.count_cache.clear();
        }
        self.count_cache.insert(key.to_owned(), n);
        Ok(n)
    }
}
