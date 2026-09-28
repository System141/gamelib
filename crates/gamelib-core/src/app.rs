//! The command layer shared by the desktop app (Tauri) and `gamelib-cli serve`.
//!
//! [`App`] owns the database connections and the single background job slot. Every frontend
//! command maps to one blocking method here, so the shells only translate calls and events.

use std::any::Any;
use std::fs::{File, OpenOptions, TryLockError};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;

use crate::db::{Db, links, read};
use crate::links::{SiteRegistry, resolve, validate};
use crate::model::{
    AppStatus, GameDetail, GameLink, GameMedia, GamePage, GameQuery, LinkCheck, LinkInput,
    NewReleasesReport, OpenTarget, Outcome, SiteInfo, SyncFinished, SyncProgress, SyncReport,
    TagInfo, WorkerKind,
};
use crate::new_releases::{NewReleasesOptions, fetch_new_releases};
use crate::steam::{CatalogSource, SteamClient};
use crate::sync::{SyncOptions, run_sync};
use crate::{Error, Result, unix_now};

/// Sent for every page a job processes, with a [`SyncProgress`] payload.
pub const EVENT_PROGRESS: &str = "sync:progress";
/// Sent exactly once when a job ends, with a [`SyncFinished`] payload.
pub const EVENT_FINISHED: &str = "sync:finished";

/// Delivers job events to the UI (Tauri events, or server-sent events for the browser).
pub trait EventSink: Send + Sync {
    fn emit(&self, event: &str, payload: serde_json::Value);
}

/// Creates the catalog source a job downloads from. Called on the job's own thread.
pub type SourceFactory = dyn Fn() -> Result<Box<dyn CatalogSource>> + Send + Sync;

/// Settings for catalog jobs. The defaults download from Steam.
#[derive(Clone)]
pub struct JobOptions {
    pub sync: SyncOptions,
    pub new_releases: NewReleasesOptions,
    pub source: Arc<SourceFactory>,
}

impl Default for JobOptions {
    fn default() -> Self {
        Self {
            sync: SyncOptions::default(),
            new_releases: NewReleasesOptions::default(),
            source: Arc::new(steam_source),
        }
    }
}

fn steam_source() -> Result<Box<dyn CatalogSource>> {
    Ok(Box::new(SteamClient::new()?))
}

/// The single background job slot: a full sync or a new-release check.
#[derive(Default)]
struct JobSlot {
    running: AtomicBool,
    cancel: AtomicBool,
    kind: Mutex<Option<WorkerKind>>,
    /// Latest progress, so a reloaded UI can catch up without waiting for the next event.
    last_progress: Mutex<Option<SyncProgress>>,
}

enum JobReport {
    Full(SyncReport),
    NewReleases(NewReleasesReport),
}

pub struct App {
    db_path: PathBuf,
    /// Read-only connection for UI queries.
    reader: Mutex<Db>,
    /// Connection for user data (links). Catalog jobs open their own.
    writer: Mutex<Db>,
    sites: SiteRegistry,
    job: Arc<JobSlot>,
    sink: Arc<dyn EventSink>,
    options: JobOptions,
}

impl App {
    /// Opens (or creates) the catalog at `db_path`; jobs download from Steam.
    pub fn open(db_path: impl Into<PathBuf>, sink: Arc<dyn EventSink>) -> Result<Self> {
        Self::with_options(db_path, sink, JobOptions::default())
    }

    pub fn with_options(
        db_path: impl Into<PathBuf>,
        sink: Arc<dyn EventSink>,
        options: JobOptions,
    ) -> Result<Self> {
        let db_path = db_path.into();
        // The writer runs migrations, so open it before the read-only connection.
        let writer = Db::open(&db_path)?;
        let reader = Db::open_reader(&db_path)?;
        Ok(Self {
            db_path,
            reader: Mutex::new(reader),
            writer: Mutex::new(writer),
            sites: SiteRegistry::with_builtin_sites(),
            job: Arc::default(),
            sink,
            options,
        })
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    // --- catalog ----------------------------------------------------------------------------

    pub fn status(&self) -> Result<AppStatus> {
        let catalog = read::status(&mut lock(&self.reader))?;
        let worker = *lock(&self.job.kind);
        let progress = match worker {
            Some(_) => lock(&self.job.last_progress).clone(),
            None => None,
        };
        Ok(AppStatus {
            catalog,
            worker,
            progress,
            db_path: self.db_path.display().to_string(),
        })
    }

    /// Starts downloading the whole catalog in the background, resuming an interrupted run
    /// unless `fresh`.
    pub fn start_sync(&self, fresh: bool) -> Result<()> {
        let opts = SyncOptions {
            fresh,
            ..self.options.sync.clone()
        };
        self.spawn_job(WorkerKind::Full, move |db, source, cancel, progress| {
            run_sync(db, source, &opts, cancel, progress).map(JobReport::Full)
        })
    }

    /// Starts fetching games released since the last check (or in the last `days` days).
    pub fn fetch_new_releases(&self, days: Option<u32>) -> Result<()> {
        let opts = NewReleasesOptions {
            days_override: days,
            ..self.options.new_releases.clone()
        };
        self.spawn_job(
            WorkerKind::NewReleases,
            move |db, source, cancel, progress| {
                fetch_new_releases(db, source, &opts, cancel, progress).map(JobReport::NewReleases)
            },
        )
    }

    /// Asks the running job to stop; it then finishes with the `cancelled` outcome.
    pub fn cancel_job(&self) {
        self.job.cancel.store(true, Ordering::Relaxed);
    }

    pub fn query_games(&self, params: &GameQuery) -> Result<GamePage> {
        read::query_games(&mut lock(&self.reader), params, unix_now())
    }

    pub fn get_game(&self, appid: u32) -> Result<Option<GameDetail>> {
        read::get_game(lock(&self.reader).conn(), appid)
    }

    pub fn list_tags(&self) -> Result<Vec<TagInfo>> {
        read::list_tags(lock(&self.reader).conn())
    }

    /// Turkish description and screenshots, fetched from Steam when a game is opened.
    pub fn game_media(&self, appid: u32) -> Result<GameMedia> {
        SteamClient::new()?.fetch_media(appid, &AtomicBool::new(false))
    }

    // --- external links ---------------------------------------------------------------------

    pub fn list_sites(&self) -> Vec<SiteInfo> {
        self.sites.list()
    }

    pub fn list_links(&self, appid: u32) -> Result<Vec<GameLink>> {
        links::list_links(lock(&self.reader).conn(), appid)
    }

    pub fn save_link(&self, input: &LinkInput) -> Result<GameLink> {
        links::save_link(lock(&self.writer).conn(), &self.sites, input, unix_now())
    }

    pub fn delete_link(&self, id: i64) -> Result<bool> {
        links::delete_link(lock(&self.writer).conn(), id)
    }

    /// Follows the link's redirects (no download) with its site's handler and stores the result.
    /// No database lock is held while the requests run.
    pub fn check_link(&self, id: i64) -> Result<LinkCheck> {
        let link = self.link(id)?;
        let url = validate::parse_link_url(&link.url)?;
        let handler = self
            .sites
            .get(&link.site_id)
            .unwrap_or_else(|| self.sites.detect(&url));
        let check = handler.resolve(&url, &resolve::client()?);
        links::record_check(lock(&self.writer).conn(), id, &check)?;
        Ok(check)
    }

    /// The address to open for a saved link. Only validated http(s) URLs are returned.
    pub fn link_url(&self, id: i64) -> Result<String> {
        let link = self.link(id)?;
        Ok(validate::parse_link_url(&link.url)?.into())
    }

    fn link(&self, id: i64) -> Result<GameLink> {
        links::get_link(lock(&self.reader).conn(), id)?.ok_or(Error::NotFound)
    }

    // --- jobs -------------------------------------------------------------------------------

    /// Runs a catalog job on its own thread with its own connection, streaming progress events
    /// and ending with exactly one `sync:finished` event, also when the job fails or panics.
    fn spawn_job<F>(&self, kind: WorkerKind, job: F) -> Result<()>
    where
        F: FnOnce(
                &mut Db,
                &dyn CatalogSource,
                &AtomicBool,
                &mut dyn FnMut(&SyncProgress),
            ) -> Result<JobReport>
            + Send
            + 'static,
    {
        let slot = self.job.clone();
        if slot.running.swap(true, Ordering::SeqCst) {
            return Err(Error::Busy);
        }
        let file_lock = match JobLock::acquire(&self.db_path) {
            Ok(file_lock) => file_lock,
            Err(e) => {
                slot.running.store(false, Ordering::SeqCst);
                return Err(e);
            }
        };
        slot.cancel.store(false, Ordering::Relaxed);
        *lock(&slot.kind) = Some(kind);
        *lock(&slot.last_progress) = None;

        let db_path = self.db_path.clone();
        let sink = self.sink.clone();
        let source = self.options.source.clone();
        let thread_slot = slot.clone();
        let spawned = std::thread::Builder::new()
            .name("gamelib-job".into())
            .spawn(move || {
                let slot = thread_slot;
                let result = catch_unwind(AssertUnwindSafe(|| {
                    let mut db = Db::open(&db_path)?;
                    let source = source()?;
                    let mut on_progress = |p: &SyncProgress| {
                        *lock(&slot.last_progress) = Some(p.clone());
                        sink.emit(EVENT_PROGRESS, to_json(p));
                    };
                    job(&mut db, source.as_ref(), &slot.cancel, &mut on_progress)
                }))
                .unwrap_or_else(|panic| {
                    Err(Error::Other(format!(
                        "catalog job panicked: {}",
                        panic_message(panic.as_ref())
                    )))
                });
                let finished = finished_event(kind, result);
                // Free the slot before announcing the end, so the UI can start the next job.
                *lock(&slot.kind) = None;
                *lock(&slot.last_progress) = None;
                drop(file_lock);
                slot.running.store(false, Ordering::SeqCst);
                sink.emit(EVENT_FINISHED, to_json(&finished));
            });

        spawned.map(drop).map_err(|e| {
            *lock(&slot.kind) = None;
            slot.running.store(false, Ordering::SeqCst);
            Error::Other(format!("could not start the job thread: {e}"))
        })
    }
}

/// Store page of a game, on the web or in the Steam client.
pub fn steam_url(appid: u32, target: OpenTarget) -> String {
    match target {
        OpenTarget::Web => format!("https://store.steampowered.com/app/{appid}/"),
        OpenTarget::Client => format!("steam://store/{appid}"),
    }
}

fn finished_event(kind: WorkerKind, result: Result<JobReport>) -> SyncFinished {
    let mut finished = SyncFinished {
        kind,
        outcome: Outcome::Completed,
        report: None,
        new_releases: None,
        error: None,
    };
    match result {
        Ok(JobReport::Full(report)) => finished.report = Some(report),
        Ok(JobReport::NewReleases(report)) => finished.new_releases = Some(report),
        Err(Error::Cancelled) => finished.outcome = Outcome::Cancelled,
        Err(e) => {
            finished.outcome = Outcome::Failed;
            finished.error = Some(e.into());
        }
    }
    finished
}

/// Advisory lock on `<db>.job-lock`, held while a job runs, so two processes sharing a database
/// (the desktop app and `gamelib-cli serve`) never download into it at the same time.
struct JobLock {
    /// Closing the file releases the lock.
    _file: Option<File>,
}

impl JobLock {
    fn acquire(db_path: &Path) -> Result<Self> {
        let mut path = db_path.as_os_str().to_owned();
        path.push(".job-lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|e| Error::Other(format!("{}: {e}", Path::new(&path).display())))?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: Some(file) }),
            Err(TryLockError::WouldBlock) => Err(Error::Busy),
            // Some file systems cannot lock; the in-process slot still prevents double starts.
            Err(TryLockError::Error(_)) => Ok(Self { _file: None }),
        }
    }
}

fn to_json(value: &impl Serialize) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
}

fn panic_message(panic: &(dyn Any + Send)) -> &str {
    panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown panic")
}

/// Locks a mutex, recovering from poisoning (a panicked query must not brick the app).
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
