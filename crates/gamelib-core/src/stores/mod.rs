//! Other stores (GOG, itch.io): their products, and which Steam games they are.
//!
//! The "match stores" job reads GOG's public catalog, matches products to Steam games by title
//! ([`matching`]) and then checks uncertain or unmatched products against GOG's GamesDB id
//! cross-reference ([`gamesdb`]). Everything is stored in `store_products` / `store_matches`;
//! the user's verdicts on matches (`state`) are never overwritten.

pub mod gamesdb;
pub mod gog;
pub mod gog_account;
pub mod itch;
pub mod library;
pub mod matching;

use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use crate::db::write::{meta_keys, set_meta};
use crate::db::{Db, stores as store_db};
use crate::http::{self, Counters, Pacer};
use crate::model::{Store, StoresReport, SyncPhase, SyncProgress, WorkerKind};
use crate::secrets::SecretStore;
use crate::{Error, Result, unix_now};

/// Base URLs of the store services, replaceable in tests.
#[derive(Debug, Clone)]
pub struct StoreEndpoints {
    pub gog_catalog: String,
    pub gog_api: String,
    pub gog_embed: String,
    pub gog_auth: String,
    pub gamesdb: String,
    pub itch_api: String,
    /// store.steampowered.com: reviews and system requirements.
    pub steam_store: String,
    /// api.isthereanydeal.com: prices.
    pub itad_api: String,
}

impl Default for StoreEndpoints {
    fn default() -> Self {
        Self {
            gog_catalog: "https://catalog.gog.com".into(),
            gog_api: "https://api.gog.com".into(),
            gog_embed: "https://embed.gog.com".into(),
            gog_auth: "https://auth.gog.com".into(),
            gamesdb: "https://gamesdb.gog.com".into(),
            itch_api: "https://api.itch.io".into(),
            steam_store: "https://store.steampowered.com".into(),
            itad_api: crate::prices::itad::API.into(),
        }
    }
}

impl StoreEndpoints {
    /// Every service at one base URL (a local test server).
    pub fn all_at(base: &str) -> Self {
        let base = base.trim_end_matches('/').to_owned();
        Self {
            gog_catalog: base.clone(),
            gog_api: base.clone(),
            gog_embed: base.clone(),
            gog_auth: base.clone(),
            gamesdb: base.clone(),
            itch_api: base.clone(),
            steam_store: base.clone(),
            itad_api: base,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StoreSyncOptions {
    pub endpoints: StoreEndpoints,
    /// Pause between GOG catalog pages.
    pub catalog_delay: Duration,
    /// Pause between GamesDB lookups (about four per second).
    pub gamesdb_delay: Duration,
    /// Check uncertain and unmatched products against GamesDB.
    pub gamesdb: bool,
    /// At most this many GamesDB lookups per run; the rest wait for the next run.
    pub gamesdb_limit: Option<u32>,
}

impl Default for StoreSyncOptions {
    fn default() -> Self {
        Self {
            endpoints: StoreEndpoints::default(),
            catalog_delay: Duration::from_millis(250),
            gamesdb_delay: Duration::from_millis(250),
            gamesdb: true,
            gamesdb_limit: None,
        }
    }
}

/// A product as a store lists it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StoreProduct {
    pub store: Store,
    pub product_id: String,
    /// "game" or "pack".
    pub kind: String,
    pub title: String,
    pub slug: Option<String>,
    pub url: Option<String>,
    pub developers: Vec<String>,
    pub publishers: Vec<String>,
    /// Original release (Unix time).
    pub release_date: Option<i64>,
    /// Release on this store, for re-released classics.
    pub store_release_date: Option<i64>,
    pub cover: Option<String>,
    pub cover_wide: Option<String>,
    pub win: bool,
    pub mac: bool,
    pub linux: bool,
    pub price: Option<String>,
    pub is_free: bool,
}

/// Progress reporting for store jobs.
pub(crate) struct Progress<'a> {
    kind: WorkerKind,
    started_at: i64,
    sink: &'a mut dyn FnMut(&SyncProgress),
}

impl<'a> Progress<'a> {
    pub(crate) fn new(kind: WorkerKind, sink: &'a mut dyn FnMut(&SyncProgress)) -> Self {
        Self {
            kind,
            started_at: unix_now(),
            sink,
        }
    }

    pub(crate) fn emit(&mut self, phase: SyncPhase, fetched: u32, total: u32) {
        self.emit_pages(phase, fetched, total, 0, 0);
    }

    fn emit_pages(&mut self, phase: SyncPhase, fetched: u32, total: u32, page: u32, pages: u32) {
        (self.sink)(&SyncProgress {
            kind: self.kind,
            phase,
            fetched,
            total,
            page,
            pages,
            started_at: self.started_at,
            resumed: false,
        });
    }
}

/// Reads GOG's catalog, matches it to the Steam catalog by title, then checks what is still
/// uncertain against GamesDB, and finally reads the signed-in accounts' libraries. Stores
/// everything as it goes, so a cancelled run keeps its work.
pub fn run_store_sync(
    db: &mut Db,
    opts: &StoreSyncOptions,
    secrets: Option<&SecretStore>,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(&SyncProgress),
) -> Result<StoresReport> {
    let clock = Instant::now();
    let started_at = unix_now();
    let http = http::api_client(Duration::from_secs(60))?;
    let counters = Counters::default();
    let mut report = StoresReport::default();
    let mut progress = Progress::new(WorkerKind::Stores, on_progress);

    // 1. GOG catalog. Products are stamped with this run, strictly later than the previous
    // run even within the same second, so the ones not listed any more can be told apart.
    progress.emit(SyncPhase::GogCatalog, 0, 0);
    let run = started_at.max(store_db::last_seen(db.conn(), Store::Gog)? + 1);
    let pacer = Pacer::new(opts.catalog_delay);
    let mut after: Option<String> = None;
    let mut page_no = 0;
    loop {
        let page = gog::catalog_page(
            &http,
            &opts.endpoints,
            after.as_deref(),
            &pacer,
            cancel,
            &counters,
        )?;
        page_no += 1;
        report.inserted += store_db::upsert_products(db.conn_mut(), &page.products, run)?;
        report.catalog += page.products.len() as u32;
        let pages = page.total.div_ceil(gog::PAGE_SIZE).max(page_no);
        progress.emit_pages(
            SyncPhase::GogCatalog,
            report.catalog,
            page.total.max(report.catalog),
            page_no,
            pages,
        );
        match page.next {
            Some(next) => after = Some(next),
            None => break,
        }
    }
    if report.catalog > 0 {
        store_db::mark_unlisted(db.conn(), Store::Gog, run)?;
    }

    // 2. Title matching against the Steam catalog.
    match_by_title(db, Store::Gog, None, cancel, &mut progress)?;

    // 3. GamesDB for products without a certain match.
    if opts.gamesdb {
        let (checked, remaining) = check_gamesdb(
            db,
            &http,
            opts,
            false,
            cancel,
            &counters,
            &mut progress,
            &mut report.warnings,
        )?;
        report.checked = checked;
        report.remaining = remaining;
    }

    // 4. The signed-in accounts' libraries.
    if let Some(secrets) = secrets
        && library::signed_in(secrets)?
    {
        report.library = Some(library::sync_library(
            db,
            secrets,
            opts,
            cancel,
            &counters,
            &mut progress,
        )?);
    }

    progress.emit(SyncPhase::Finalizing, 0, 0);
    set_meta(db.conn(), meta_keys::LAST_STORE_SYNC_AT, unix_now())?;
    report.matched_games = store_db::matched_games(db.conn(), Store::Gog)?;
    (report.requests, report.retries) = counters.get();
    report.duration_ms = clock.elapsed().as_millis() as u64;
    Ok(report)
}

/// Title-matches a store's products (all of them, or `only` these) to the Steam catalog.
pub(crate) fn match_by_title(
    db: &mut Db,
    store: Store,
    only: Option<&[String]>,
    cancel: &AtomicBool,
    progress: &mut Progress,
) -> Result<()> {
    let products = store_db::products_for_matching(db.conn(), store, only)?;
    let total = products.len() as u32;
    progress.emit(SyncPhase::Matching, 0, total);
    if products.is_empty() {
        return Ok(());
    }
    let index = store_db::steam_index(db.conn())?;
    let now = unix_now();
    for (i, chunk) in products.chunks(500).enumerate() {
        crate::check_cancel(cancel)?;
        let tx = db.conn_mut().transaction()?;
        for p in chunk {
            let candidates = index.candidates(&p.canonical_title, &p.key);
            store_db::replace_title_matches(&tx, store, &p.product_id, &candidates, now)?;
        }
        tx.commit()?;
        let done = ((i + 1) * 500).min(products.len()) as u32;
        progress.emit(SyncPhase::Matching, done, total);
    }
    Ok(())
}

/// Checks GOG products without a certain match against GamesDB (only owned ones with
/// `owned_only`). GamesDB is optional: when it fails, the title matches stay and the rest is
/// tried on the next run. Returns (checked, remaining).
#[allow(clippy::too_many_arguments)]
pub(crate) fn check_gamesdb(
    db: &mut Db,
    http: &reqwest::blocking::Client,
    opts: &StoreSyncOptions,
    owned_only: bool,
    cancel: &AtomicBool,
    counters: &Counters,
    progress: &mut Progress,
    warnings: &mut Vec<String>,
) -> Result<(u32, u32)> {
    let todo = store_db::gamesdb_todo(db.conn(), Store::Gog, owned_only)?;
    let limit = opts.gamesdb_limit.map_or(todo.len(), |n| n as usize);
    let total = todo.len().min(limit) as u32;
    let pacer = Pacer::new(opts.gamesdb_delay);
    progress.emit(SyncPhase::GogIds, 0, total);
    let mut checked = 0;
    for id in todo.iter().take(limit) {
        let releases = match gamesdb::for_gog(http, &opts.endpoints, id, &pacer, cancel, counters) {
            Ok(r) => r,
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => {
                warnings.push(format!("GamesDB lookup stopped: {e}"));
                break;
            }
        };
        store_db::apply_gamesdb(db.conn_mut(), Store::Gog, id, releases.as_ref(), unix_now())?;
        checked += 1;
        if checked % 10 == 0 || checked == total {
            progress.emit(SyncPhase::GogIds, checked, total);
        }
    }
    Ok((checked, todo.len() as u32 - checked))
}
