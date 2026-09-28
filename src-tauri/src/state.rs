use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, MutexGuard};

use gamelib_core::db::Db;
use gamelib_core::links::SiteRegistry;
use gamelib_core::model::{SyncProgress, WorkerKind};

use crate::error::{CmdError, CmdResult};

pub struct AppState {
    pub db_path: PathBuf,
    /// Read-only connection for UI queries.
    reader: Arc<Mutex<Db>>,
    /// Connection for user data (links). Catalog downloads open their own connection.
    writer: Arc<Mutex<Db>>,
    pub sites: Arc<SiteRegistry>,
    pub worker: Arc<WorkerController>,
}

/// The single background job slot: a full sync or a new-release check.
#[derive(Default)]
pub struct WorkerController {
    pub running: AtomicBool,
    pub cancel: AtomicBool,
    pub kind: Mutex<Option<WorkerKind>>,
    /// Latest progress, so a reloaded UI can catch up without waiting for the next event.
    pub last_progress: Mutex<Option<SyncProgress>>,
}

impl AppState {
    pub fn open(db_path: PathBuf) -> gamelib_core::Result<Self> {
        // The writer runs migrations, so open it before the read-only connection.
        let writer = Db::open(&db_path)?;
        let reader = Db::open_reader(&db_path)?;
        Ok(Self {
            db_path,
            reader: Arc::new(Mutex::new(reader)),
            writer: Arc::new(Mutex::new(writer)),
            sites: Arc::new(SiteRegistry::with_builtin_sites()),
            worker: Arc::new(WorkerController::default()),
        })
    }

    /// Runs `f` with the read connection on the blocking thread pool.
    pub async fn read<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Db) -> gamelib_core::Result<T> + Send + 'static,
    ) -> CmdResult<T> {
        run_blocking(self.reader.clone(), f).await
    }

    /// Runs `f` with the user-data connection on the blocking thread pool.
    pub async fn write<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Db) -> gamelib_core::Result<T> + Send + 'static,
    ) -> CmdResult<T> {
        run_blocking(self.writer.clone(), f).await
    }
}

async fn run_blocking<T: Send + 'static>(
    db: Arc<Mutex<Db>>,
    f: impl FnOnce(&mut Db) -> gamelib_core::Result<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(move || f(&mut lock(&db)).map_err(CmdError::from))
        .await
        .map_err(|e| CmdError::other(e.to_string()))?
}

/// Locks a mutex, recovering from poisoning (a panicked query must not brick the app).
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
