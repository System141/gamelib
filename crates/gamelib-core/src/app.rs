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

use crate::db::downloads::{self as download_db, NewDownload};
use crate::db::write::{get_meta, set_meta};
use crate::db::{Db, links, read, stores as store_db};
use crate::downloads::{DownloadManager, sources};
use crate::http::{self, Counters, Pacer};
use crate::install::{self, InstallManager};
use crate::links::{SiteRegistry, resolve, validate};
use crate::model::{
    Account, Accounts, AppStatus, Download, DownloadList, FileOption, GameDetail, GameLink,
    GameMedia, GamePage, GameQuery, Installed, LibraryItem, LibraryReport, LinkCheck, LinkInput,
    MatchState, NewReleasesReport, OpenTarget, Outcome, Settings, SettingsPatch, SiteInfo, Store,
    StoreMatch, StoreSearchHit, StoresReport, SyncFinished, SyncProgress, SyncReport, TagInfo,
    WorkerKind,
};
use crate::new_releases::{NewReleasesOptions, fetch_new_releases};
use crate::secrets::{ItchKey, SecretStore};
use crate::steam::{CatalogSource, SteamClient};
use crate::stores::matching::{self, MatchKey};
use crate::stores::{StoreSyncOptions, gamesdb, gog, gog_account, itch, library, run_store_sync};
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

mod settings_keys {
    pub const LIBRARY_DIR: &str = "settings.library_dir";
    pub const KEEP_INSTALLERS: &str = "settings.keep_installers";
    pub const AUTO_UPDATE: &str = "settings.auto_update";
}

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
    /// Someone signed in while another job ran: read the libraries once it ends.
    library_pending: AtomicBool,
}

/// What starting a job needs, shared with running jobs so one can start the next.
struct Jobs {
    slot: JobSlot,
    db_path: PathBuf,
    sink: Arc<dyn EventSink>,
    source: Arc<SourceFactory>,
    stores: StoreSyncOptions,
    secrets: Arc<SecretStore>,
}

enum JobReport {
    Full(SyncReport),
    NewReleases(NewReleasesReport),
    Stores(StoresReport),
    Library(LibraryReport),
}

pub struct App {
    db_path: PathBuf,
    /// Read-only connection for UI queries.
    reader: Mutex<Db>,
    /// Connection for user data (links). Catalog jobs open their own.
    writer: Mutex<Db>,
    sites: SiteRegistry,
    /// Store account credentials, next to the database.
    secrets: Arc<SecretStore>,
    jobs: Arc<Jobs>,
    /// The download queue; its worker runs only once [`App::start_downloads`] is called.
    downloads: DownloadManager,
    /// Installs finished downloads, on a thread of its own (also started by `start_downloads`).
    installs: InstallManager,
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
        let dir = db_path
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let secrets = Arc::new(SecretStore::new(dir));
        let installs = InstallManager::new(db_path.clone(), sink.clone());
        let downloads = DownloadManager::new(
            db_path.clone(),
            sink.clone(),
            secrets.clone(),
            options.stores.clone(),
            installs.notifier(),
        );
        let jobs = Arc::new(Jobs {
            slot: JobSlot::default(),
            db_path: db_path.clone(),
            sink: sink.clone(),
            source: options.source.clone(),
            stores: options.stores.clone(),
            secrets: secrets.clone(),
        });
        Ok(Self {
            db_path,
            reader: Mutex::new(reader),
            writer: Mutex::new(writer),
            sites: SiteRegistry::with_builtin_sites(),
            secrets,
            jobs,
            downloads,
            installs,
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
        let worker = *lock(&self.jobs.slot.kind);
        let progress = match worker {
            Some(_) => lock(&self.jobs.slot.last_progress).clone(),
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

    /// Starts matching other stores (GOG) to the Steam catalog in the background, then reads
    /// the signed-in accounts' libraries.
    pub fn start_store_sync(&self) -> Result<()> {
        let opts = self.options.stores.clone();
        let secrets = self.secrets.clone();
        self.spawn_job(WorkerKind::Stores, move |db, _, cancel, progress| {
            run_store_sync(db, &opts, Some(&secrets), cancel, progress).map(JobReport::Stores)
        })
    }

    /// Starts reading the signed-in accounts' libraries in the background.
    pub fn start_library_sync(&self) -> Result<()> {
        self.spawn_job(WorkerKind::Library, library_job(&self.jobs))
    }

    /// Asks the running job to stop; it then finishes with the `cancelled` outcome.
    pub fn cancel_job(&self) {
        self.jobs.slot.cancel.store(true, Ordering::Relaxed);
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

    /// Searches a store for a Steam game (itch.io only; GOG is matched from its catalog), best
    /// first. Results are remembered so one can be tied to the game.
    pub fn search_store(&self, store: Store, appid: u32) -> Result<Vec<StoreSearchHit>> {
        if store != Store::Itch {
            return Err(Error::Invalid("store"));
        }
        let key = self
            .secrets
            .load()?
            .itch
            .ok_or(Error::Invalid("itch_signed_out"))?;
        let game = self.get_game(appid)?.ok_or(Error::NotFound)?;
        let client = itch::client()?;
        let found = itch::search(
            &client,
            &self.options.stores.endpoints,
            &key.api_key,
            &game.card.name,
            &AtomicBool::new(false),
            &Counters::default(),
        )?;
        let steam_key = MatchKey::new(
            game.developers
                .iter()
                .chain(&game.publishers)
                .map(String::as_str),
            [game.card.release_date, game.original_release_date],
        );
        let steam_title = matching::canonical_title(&game.card.name);
        let now = unix_now();
        let mut hits = Vec::new();
        {
            let writer = lock(&self.writer);
            for p in found {
                store_db::insert_extra_product(writer.conn(), &p, now)?;
                let key = MatchKey::new(p.developers.iter().map(String::as_str), [p.release_date]);
                let score = if matching::canonical_title(&p.title) == steam_title {
                    matching::score(&key, &steam_key)
                } else {
                    0.0
                };
                hits.push(StoreSearchHit {
                    store: p.store,
                    product_id: p.product_id,
                    title: p.title,
                    url: p.url,
                    cover_wide: p.cover_wide,
                    developer: p.developers.into_iter().next(),
                    price: p.price,
                    is_free: p.is_free,
                    win: p.win,
                    mac: p.mac,
                    linux: p.linux,
                    score,
                });
            }
        }
        hits.sort_by(|a, b| b.score.total_cmp(&a.score));
        Ok(hits)
    }

    /// Ties a store product to a Steam game by hand.
    pub fn link_store_product(&self, store: Store, product_id: &str, appid: u32) -> Result<()> {
        let writer = lock(&self.writer);
        let known = store_db::known_products(writer.conn(), store, &[product_id.to_owned()])?;
        if known.is_empty() || read::get_game(writer.conn(), appid)?.is_none() {
            return Err(Error::NotFound);
        }
        store_db::set_manual_match(writer.conn(), store, product_id, appid, unix_now())
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

    // --- accounts and library ---------------------------------------------------------------

    /// Who is signed in where. Tokens never leave the core.
    pub fn accounts(&self) -> Result<Accounts> {
        let s = self.secrets.load()?;
        Ok(Accounts {
            gog: s.gog.map(|t| Account {
                username: t.username.unwrap_or(t.user_id),
            }),
            itch: s.itch.map(|k| Account {
                username: k.username,
            }),
        })
    }

    /// GOG's sign-in page; it ends on a redirect that [`App::gog_login_with_code`] accepts.
    pub fn gog_login_url(&self) -> String {
        gog_account::login_url(&self.options.stores.endpoints)
    }

    /// Finishes a GOG sign-in with the redirect address (or the bare code), then reads the
    /// library in the background.
    pub fn gog_login_with_code(&self, redirect: &str) -> Result<Accounts> {
        let code = gog_account::code_from_redirect(redirect).ok_or(Error::Invalid("gog_code"))?;
        let client = gog_account::client()?;
        let cancel = AtomicBool::new(false);
        let counters = Counters::default();
        let endpoints = &self.options.stores.endpoints;
        let mut tokens = gog_account::exchange_code(&client, endpoints, &code, &cancel, &counters)?;
        tokens.username =
            gog_account::username(&client, endpoints, &tokens.access_token, &cancel, &counters)
                .ok()
                .flatten();
        self.secrets.update(|s| s.gog = Some(tokens))?;
        self.after_sign_in();
        self.accounts()
    }

    /// Saves an itch.io API key after checking it, then reads the library in the background.
    pub fn itch_set_key(&self, key: &str) -> Result<Accounts> {
        let key = key.trim();
        let user = itch::profile(
            &itch::client()?,
            &self.options.stores.endpoints,
            key,
            &AtomicBool::new(false),
            &Counters::default(),
        )?;
        self.secrets.update(|s| {
            s.itch = Some(ItchKey {
                api_key: key.to_owned(),
                user_id: user.id,
                username: user.username,
            })
        })?;
        self.after_sign_in();
        self.accounts()
    }

    /// Signs out of a store and forgets what it owned.
    pub fn sign_out(&self, store: Store) -> Result<Accounts> {
        library::sign_out(&lock(&self.writer), &self.secrets, store)?;
        self.accounts()
    }

    fn after_sign_in(&self) {
        if let Err(Error::Busy) = self.start_library_sync() {
            // Another job runs: it starts the library job when it ends.
            self.jobs.slot.library_pending.store(true, Ordering::SeqCst);
            // Unless it ended just now, before seeing the flag.
            if self.start_library_sync().is_ok() {
                self.jobs
                    .slot
                    .library_pending
                    .store(false, Ordering::SeqCst);
            }
        }
    }

    /// The user's games on the signed-in stores.
    pub fn library(&self, store: Option<Store>) -> Result<Vec<LibraryItem>> {
        store_db::library(lock(&self.reader).conn(), store)
    }

    // --- settings -----------------------------------------------------------------------------

    pub fn settings(&self) -> Result<Settings> {
        read_settings(lock(&self.reader).conn())
    }

    pub fn update_settings(&self, patch: &SettingsPatch) -> Result<Settings> {
        {
            let writer = lock(&self.writer);
            let conn = writer.conn();
            if let Some(dir) = &patch.library_dir {
                let dir = dir.trim();
                if dir.is_empty() || !Path::new(dir).is_absolute() {
                    return Err(Error::Invalid("library_dir"));
                }
                set_meta(conn, settings_keys::LIBRARY_DIR, dir)?;
            }
            if let Some(v) = patch.keep_installers {
                set_meta(conn, settings_keys::KEEP_INSTALLERS, u8::from(v))?;
            }
            if let Some(v) = patch.auto_update {
                set_meta(conn, settings_keys::AUTO_UPDATE, u8::from(v))?;
            }
        }
        self.settings()
    }

    // --- downloads ----------------------------------------------------------------------------

    /// Starts the download and install workers, which continue what the last exit
    /// interrupted. Only the desktop app downloads; the preview server never calls this.
    pub fn start_downloads(&self) -> Result<()> {
        let writer = lock(&self.writer);
        self.installs.start(writer.conn())?;
        self.downloads.start(writer.conn())
    }

    /// What can be downloaded for a store product, the best variant for this computer first.
    pub fn store_files(&self, store: Store, product_id: &str) -> Result<Vec<FileOption>> {
        let owned_key = download_db::owned_key(lock(&self.reader).conn(), store, product_id)?;
        let offers = sources::offers(
            store,
            product_id,
            owned_key.as_deref(),
            &self.secrets,
            &self.options.stores,
        )?;
        Ok(offers.into_iter().map(|o| o.option).collect())
    }

    /// Queues a variant of a store product (see [`App::store_files`]) for download into the
    /// library folder.
    pub fn enqueue_download(
        &self,
        store: Store,
        product_id: &str,
        option_id: &str,
    ) -> Result<Download> {
        let (title, appid, owned_key) = {
            let reader = lock(&self.reader);
            let (title, appid) = store_db::product_summary(reader.conn(), store, product_id)?
                .ok_or(Error::NotFound)?;
            let owned_key = download_db::owned_key(reader.conn(), store, product_id)?;
            (title, appid, owned_key)
        };
        let offers = sources::offers(
            store,
            product_id,
            owned_key.as_deref(),
            &self.secrets,
            &self.options.stores,
        )?;
        let offer = offers
            .into_iter()
            .find(|o| o.option.id == option_id && !o.files.is_empty())
            .ok_or(Error::Invalid("no_files"))?;
        let library_dir = PathBuf::from(self.settings()?.library_dir);
        let new = NewDownload {
            store,
            product_id,
            appid,
            title: &title,
            option_id,
            option_label: Some(&offer.option.label),
            platform: offer.option.platform,
            files: &offer.files,
        };
        let mut writer = lock(&self.writer);
        self.downloads
            .enqueue(writer.conn_mut(), &new, &library_dir)
    }

    pub fn downloads(&self) -> Result<DownloadList> {
        Ok(DownloadList {
            items: self.downloads.list(lock(&self.reader).conn())?,
            live: self.downloads.live(),
            installing: self.installs.live(),
        })
    }

    pub fn pause_download(&self, id: i64) -> Result<()> {
        self.downloads.pause(lock(&self.writer).conn(), id)
    }

    pub fn resume_download(&self, id: i64) -> Result<()> {
        self.downloads.resume(lock(&self.writer).conn(), id)
    }

    /// Cancels a download and deletes its files.
    pub fn remove_download(&self, id: i64) -> Result<()> {
        self.downloads.remove(lock(&self.writer).conn(), id)
    }

    pub fn clear_finished_downloads(&self) -> Result<()> {
        self.downloads.clear_completed(lock(&self.writer).conn())
    }

    /// The folder a download's files are in, if it exists.
    pub fn download_folder(&self, id: i64) -> Result<PathBuf> {
        let d = download_db::get(lock(&self.reader).conn(), id)?.ok_or(Error::NotFound)?;
        let dir = PathBuf::from(d.dir);
        if dir.is_dir() {
            Ok(dir)
        } else {
            Err(Error::NotFound)
        }
    }

    /// Lets someone else's installer (an itch.io upload) run.
    pub fn approve_install(&self, download_id: i64) -> Result<()> {
        self.installs
            .approve(lock(&self.writer).conn(), download_id)
    }

    /// Installs a finished download again after a failure (or one from before installs).
    pub fn retry_install(&self, download_id: i64) -> Result<()> {
        self.installs.retry(lock(&self.writer).conn(), download_id)
    }

    // --- installed games ------------------------------------------------------------------------

    pub fn installs(&self) -> Result<Vec<Installed>> {
        install::list(lock(&self.reader).conn())
    }

    pub fn launch_game(&self, store: Store, product_id: &str) -> Result<()> {
        install::launch(lock(&self.reader).conn(), store, product_id)
    }

    pub fn install_folder(&self, store: Store, product_id: &str) -> Result<PathBuf> {
        install::folder(lock(&self.reader).conn(), store, product_id)
    }

    /// Chooses what "Oyna" starts.
    pub fn set_launch_target(
        &self,
        store: Store,
        product_id: &str,
        exe: &str,
    ) -> Result<Installed> {
        let installed =
            install::set_launch_target(lock(&self.writer).conn(), store, product_id, exe)?;
        self.install_changed(store, product_id);
        Ok(installed)
    }

    /// Uninstalls a game (see [`install::remove`]). An uninstaller may ask for administrator
    /// rights and take a while; no connection is held meanwhile.
    pub fn uninstall_game(&self, store: Store, product_id: &str) -> Result<()> {
        let removal = install::removal(lock(&self.reader).conn(), store, product_id)?;
        let library = PathBuf::from(self.settings()?.library_dir);
        install::remove(&removal, &library)?;
        install::forget(lock(&self.writer).conn(), store, product_id)?;
        self.install_changed(store, product_id);
        Ok(())
    }

    fn install_changed(&self, store: Store, product_id: &str) {
        self.sink.emit(
            install::EVENT_CHANGED,
            serde_json::json!({ "store": store, "productId": product_id }),
        );
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
        spawn_job(&self.jobs, kind, job)
    }
}

/// What a job runs on its thread: given its own connection, the Steam source, the cancel flag
/// and a progress callback.
type Job = Box<
    dyn FnOnce(
            &mut Db,
            &SourceFactory,
            &AtomicBool,
            &mut dyn FnMut(&SyncProgress),
        ) -> Result<JobReport>
        + Send,
>;

/// The library job: reads the signed-in accounts' libraries.
fn library_job(jobs: &Jobs) -> Job {
    let opts = jobs.stores.clone();
    let secrets = jobs.secrets.clone();
    Box::new(move |db, _, cancel, progress| {
        library::run_library_sync(db, &secrets, &opts, cancel, progress).map(JobReport::Library)
    })
}

/// Runs a catalog job on its own thread with its own connection, streaming progress events
/// and ending with exactly one `sync:finished` event, also when the job fails or panics. A
/// library read asked for meanwhile starts right after.
fn spawn_job<F>(jobs: &Arc<Jobs>, kind: WorkerKind, job: F) -> Result<()>
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
    let slot = &jobs.slot;
    if slot.running.swap(true, Ordering::SeqCst) {
        return Err(Error::Busy);
    }
    let file_lock = match JobLock::acquire(&jobs.db_path) {
        Ok(file_lock) => file_lock,
        Err(e) => {
            slot.running.store(false, Ordering::SeqCst);
            return Err(e);
        }
    };
    slot.cancel.store(false, Ordering::Relaxed);
    *lock(&slot.kind) = Some(kind);
    *lock(&slot.last_progress) = None;

    let thread_jobs = jobs.clone();
    let spawned = std::thread::Builder::new()
        .name("gamelib-job".into())
        .spawn(move || {
            let jobs = thread_jobs;
            let slot = &jobs.slot;
            let result = catch_unwind(AssertUnwindSafe(|| {
                let mut db = Db::open(&jobs.db_path)?;
                let mut on_progress = |p: &SyncProgress| {
                    *lock(&slot.last_progress) = Some(p.clone());
                    jobs.sink.emit(EVENT_PROGRESS, to_json(p));
                };
                job(
                    &mut db,
                    jobs.source.as_ref(),
                    &slot.cancel,
                    &mut on_progress,
                )
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
            jobs.sink.emit(EVENT_FINISHED, to_json(&finished));
            if slot.library_pending.swap(false, Ordering::SeqCst) {
                let _ = spawn_job(&jobs, WorkerKind::Library, library_job(&jobs));
            }
        });

    spawned.map(drop).map_err(|e| {
        *lock(&slot.kind) = None;
        slot.running.store(false, Ordering::SeqCst);
        Error::Other(format!("could not start the job thread: {e}"))
    })
}

/// Store page of a game, on the web or in the Steam client.
pub fn steam_url(appid: u32, target: OpenTarget) -> String {
    match target {
        OpenTarget::Web => format!("https://store.steampowered.com/app/{appid}/"),
        OpenTarget::Client => format!("steam://store/{appid}"),
        OpenTarget::Install => format!("steam://install/{appid}"),
    }
}

/// The user's settings, with defaults for what was never set.
pub(crate) fn read_settings(conn: &rusqlite::Connection) -> Result<Settings> {
    let flag = |key: &str, default: bool| -> Result<bool> {
        Ok(get_meta(conn, key)?.map_or(default, |v| v == "1"))
    };
    Ok(Settings {
        library_dir: get_meta(conn, settings_keys::LIBRARY_DIR)?
            .unwrap_or_else(default_library_dir),
        keep_installers: flag(settings_keys::KEEP_INSTALLERS, false)?,
        auto_update: flag(settings_keys::AUTO_UPDATE, true)?,
    })
}

/// `~/Games` (e.g. `C:\\Users\\<user>\\Games`).
fn default_library_dir() -> String {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Games")
        .display()
        .to_string()
}

/// Where a store's account settings are (itch.io: API keys).
pub fn account_page(store: Store) -> &'static str {
    match store {
        Store::Gog => "https://www.gog.com/account",
        Store::Itch => "https://itch.io/user/settings/api-keys",
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
        library: None,
        error: None,
    };
    match result {
        Ok(JobReport::Full(report)) => finished.report = Some(report),
        Ok(JobReport::NewReleases(report)) => finished.new_releases = Some(report),
        Ok(JobReport::Stores(report)) => finished.stores = Some(report),
        Ok(JobReport::Library(report)) => finished.library = Some(report),
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
