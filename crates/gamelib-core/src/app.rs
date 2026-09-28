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

use crate::db::{Db, links, read, stores as store_db};
use crate::http::{self, Counters, Pacer};
use crate::links::{SiteRegistry, resolve, validate};
use crate::model::{
    AppStatus, GameDetail, GameLink, GameMedia, GamePage, GameQuery, LinkCheck, LinkInput,
    MatchState, NewReleasesReport, OpenTarget, Outcome, SiteInfo, Store, StoreMatch, StoresReport,
    SyncFinished, SyncProgress, SyncReport, TagInfo, WorkerKind,
};
use crate::new_releases::{NewReleasesOptions, fetch_new_releases};
use crate::steam::{CatalogSource, SteamClient};
use crate::stores::{StoreSyncOptions, gamesdb, gog, run_store_sync};
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

/// Settings for catalog jobs. The defaults download from Steam and the real stores.
#[derive(Clone)]
pub struct JobOptions {
    pub sync: SyncOptions,
    pub new_releases: NewReleasesOptions,
    pub stores: StoreSyncOptions,
    pub source: Arc<SourceFactory>,
}

impl Default for JobOptions {
    fn default() -> Self {
        Self {
            sync: SyncOptions::default(),
            new_releases: NewReleasesOptions::default(),
            stores: StoreSyncOptions::default(),
            source: Arc::new(steam_source),
        }
    }
}

/// A Steam game is looked up in GamesDB again after this long.
const LOOKUP_MAX_AGE: i64 = 30 * 86_400;

fn steam_source() -> Result<Box<dyn CatalogSource>> {
    Ok(Box::new(SteamClient::new()?))
}

/// The single background job slot: a full sync, a new-release check or store matching.
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
    Stores(StoresReport),
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
            let source = source()?;
            run_sync(db, source.as_ref(), &opts, cancel, progress).map(JobReport::Full)
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
                let source = source()?;
                fetch_new_releases(db, source.as_ref(), &opts, cancel, progress)
                    .map(JobReport::NewReleases)
            },
        )
    }

    /// Starts matching other stores (GOG) to the Steam catalog in the background.
    pub fn start_store_sync(&self) -> Result<()> {
        let opts = self.options.stores.clone();
        self.spawn_job(WorkerKind::Stores, move |db, _, cancel, progress| {
            run_store_sync(db, &opts, cancel, progress).map(JobReport::Stores)
        })
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

    // --- other stores -----------------------------------------------------------------------

    /// Store products matched to a Steam game (suggestions included, rejected ones left out).
    pub fn store_matches(&self, appid: u32) -> Result<Vec<StoreMatch>> {
        store_db::matches_for_game(lock(&self.reader).conn(), appid)
    }

    /// Asks GOG's GamesDB which GOG products a Steam game has (at most once a month per game),
    /// then returns the game's matches. No database lock is held during the request.
    pub fn refresh_store_matches(&self, appid: u32) -> Result<Vec<StoreMatch>> {
        let now = unix_now();
        let checked = store_db::last_lookup(lock(&self.reader).conn(), Store::Gog, appid)?;
        if checked.is_none_or(|t| now - t > LOOKUP_MAX_AGE) {
            let endpoints = &self.options.stores.endpoints;
            let client = http::api_client(std::time::Duration::from_secs(20))?;
            let counters = Counters::default();
            let pacer = Pacer::new(std::time::Duration::ZERO);
            let cancel = AtomicBool::new(false);
            let releases =
                gamesdb::for_steam(&client, endpoints, appid, &pacer, &cancel, &counters)?;
            // Products GamesDB knows but the catalog listing did not show yet.
            if let Some(r) = releases.as_ref().filter(|r| r.is_game()) {
                let known =
                    store_db::known_products(lock(&self.reader).conn(), Store::Gog, &r.gog)?;
                if known.is_empty() {
                    for id in r.gog.iter().take(2) {
                        if let Some(p) =
                            gog::product_info(&client, endpoints, id, &pacer, &cancel, &counters)?
                        {
                            store_db::insert_extra_product(lock(&self.writer).conn(), &p, now)?;
                        }
                    }
                }
            }
            store_db::apply_steam_lookup(
                lock(&self.writer).conn_mut(),
                Store::Gog,
                appid,
                releases.as_ref(),
                now,
            )?;
        }
        self.store_matches(appid)
    }

    /// The store page of a product. Only https pages on the store's own domains are returned.
    pub fn store_product_url(&self, store: Store, product_id: &str) -> Result<String> {
        let url = store_db::product_url(lock(&self.reader).conn(), store, product_id)?
            .ok_or(Error::NotFound)?;
        let url = validate::parse_link_url(&url)?;
        let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
        let own = store_domains(store)
            .iter()
            .any(|d| host == *d || host.ends_with(&format!(".{d}")));
        if url.scheme() != "https" || !own {
            return Err(Error::Invalid("url_host"));
        }
        Ok(url.into())
    }

    /// Confirms or rejects a match (or undoes that with `auto`).
    pub fn set_match_state(
        &self,
        store: Store,
        product_id: &str,
        appid: u32,
        state: MatchState,
    ) -> Result<()> {
        let changed = store_db::set_match_state(
            lock(&self.writer).conn(),
            store,
            product_id,
            appid,
            state,
            unix_now(),
        )?;
        if changed {
            Ok(())
        } else {
            Err(Error::NotFound)
        }
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
                &SourceFactory,
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
        OpenTarget::Install => format!("steam://install/{appid}"),
    }
}

/// Domains a store's product pages may live on.
fn store_domains(store: Store) -> &'static [&'static str] {
    match store {
        Store::Gog => &["gog.com"],
        Store::Itch => &["itch.io"],
    }
}

fn finished_event(kind: WorkerKind, result: Result<JobReport>) -> SyncFinished {
    let mut finished = SyncFinished {
        kind,
        outcome: Outcome::Completed,
        report: None,
        new_releases: None,
        stores: None,
        error: None,
    };
    match result {
        Ok(JobReport::Full(report)) => finished.report = Some(report),
        Ok(JobReport::NewReleases(report)) => finished.new_releases = Some(report),
        Ok(JobReport::Stores(report)) => finished.stores = Some(report),
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
