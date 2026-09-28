//! Reading the signed-in accounts' libraries (owned GOG and itch.io products) and tying the
//! products to Steam games.

use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use super::{Progress, StoreSyncOptions, check_gamesdb, gog, gog_account, itch, match_by_title};
use crate::db::{Db, stores as store_db};
use crate::http::{Counters, Pacer};
use crate::model::{LibraryReport, Store, SyncPhase, SyncProgress, WorkerKind};
use crate::secrets::{GogTokens, SecretStore};
use crate::{Error, Result, unix_now};

/// Whether any store account is signed in.
pub fn signed_in(secrets: &SecretStore) -> Result<bool> {
    let s = secrets.load()?;
    Ok(s.gog.is_some() || s.itch.is_some())
}

/// The library job: reads every signed-in account's library.
pub fn run_library_sync(
    db: &mut Db,
    secrets: &SecretStore,
    opts: &StoreSyncOptions,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(&SyncProgress),
) -> Result<LibraryReport> {
    let counters = Counters::default();
    let mut progress = Progress::new(WorkerKind::Library, on_progress);
    let report = sync_library(db, secrets, opts, cancel, &counters, &mut progress)?;
    progress.emit(SyncPhase::Finalizing, 0, 0);
    Ok(report)
}

pub(crate) fn sync_library(
    db: &mut Db,
    secrets: &SecretStore,
    opts: &StoreSyncOptions,
    cancel: &AtomicBool,
    counters: &Counters,
    progress: &mut Progress,
) -> Result<LibraryReport> {
    let mut report = LibraryReport::default();
    progress.emit(SyncPhase::Library, 0, 0);
    let (mut gog_read, mut itch_read) = (false, false);
    // Accounts are read again after each round, so one signed in meanwhile is read too.
    loop {
        let accounts = secrets.load()?;
        let gog = accounts.gog.is_some() && !gog_read;
        let itch = accounts.itch.filter(|_| !itch_read);
        if !gog && itch.is_none() {
            break;
        }
        if gog {
            gog_read = true;
            match gog_library(
                db,
                secrets,
                opts,
                cancel,
                counters,
                progress,
                &mut report.warnings,
            ) {
                Ok(n) => report.gog_owned = Some(n),
                Err(Error::Invalid("gog_session" | "gog_signed_out")) => {
                    sign_out(db, secrets, Store::Gog)?;
                    report.gog_signed_out = true;
                }
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(e) => report.warnings.push(format!("GOG: {e}")),
            }
        }
        if let Some(key) = itch {
            itch_read = true;
            match itch_library(db, &key.api_key, opts, cancel, counters, progress) {
                Ok(n) => report.itch_owned = Some(n),
                Err(Error::Invalid("itch_key")) => {
                    sign_out(db, secrets, Store::Itch)?;
                    report
                        .warnings
                        .push("itch.io: the API key was refused".into());
                }
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(e) => report.warnings.push(format!("itch.io: {e}")),
            }
        }
    }
    report.matched = store_db::owned_matched(db.conn())?;
    Ok(report)
}

/// Forgets a store account and what it owned.
pub fn sign_out(db: &Db, secrets: &SecretStore, store: Store) -> Result<()> {
    secrets.update(|s| match store {
        Store::Gog => s.gog = None,
        Store::Itch => s.itch = None,
    })?;
    store_db::clear_owned(db.conn(), store)
}

/// Refreshing rotates GOG's refresh token, so two threads (a library sync and a download) must
/// not refresh at once: the second would present a token GOG has already replaced, and fail.
static REFRESH: Mutex<()> = Mutex::new(());

/// A usable GOG session: refreshed (and saved) when the access token is about to expire.
pub fn gog_tokens(
    secrets: &SecretStore,
    opts: &StoreSyncOptions,
    http: &reqwest::blocking::Client,
    cancel: &AtomicBool,
    counters: &Counters,
) -> Result<GogTokens> {
    let _refreshing = REFRESH.lock().unwrap_or_else(|p| p.into_inner());
    let tokens = secrets
        .load()?
        .gog
        .ok_or(Error::Invalid("gog_signed_out"))?;
    if !gog_account::expiring(&tokens, unix_now()) {
        return Ok(tokens);
    }
    let fresh = gog_account::refresh(http, &opts.endpoints, &tokens, cancel, counters)?;
    secrets.update(|s| s.gog = Some(fresh.clone()))?;
    Ok(fresh)
}

fn gog_library(
    db: &mut Db,
    secrets: &SecretStore,
    opts: &StoreSyncOptions,
    cancel: &AtomicBool,
    counters: &Counters,
    progress: &mut Progress,
    warnings: &mut Vec<String>,
) -> Result<u32> {
    let http = gog_account::client()?;
    let tokens = gog_tokens(secrets, opts, &http, cancel, counters)?;
    let owned = gog_account::owned_ids(
        &http,
        &opts.endpoints,
        &tokens.access_token,
        cancel,
        counters,
    )?;

    // Owned products the catalog listing does not show (editions, DLC, retired games).
    let known = store_db::known_products(db.conn(), Store::Gog, &owned)?;
    let unknown: Vec<&String> = owned.iter().filter(|id| !known.contains(id)).collect();
    let pacer = Pacer::new(opts.catalog_delay);
    let api = crate::http::api_client(Duration::from_secs(30))?;
    let now = unix_now();
    for (i, id) in unknown.iter().enumerate() {
        progress.emit(SyncPhase::Library, i as u32, unknown.len() as u32);
        match gog::product_info(&api, &opts.endpoints, id, &pacer, cancel, counters) {
            Ok(Some(p)) => {
                store_db::insert_extra_product(db.conn(), &p, now)?;
            }
            Ok(None) => {}
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => {
                warnings.push(format!("GOG product {id}: {e}"));
                break;
            }
        }
    }

    let pairs: Vec<(String, Option<u64>)> = owned.iter().map(|id| (id.clone(), None)).collect();
    store_db::set_owned(db.conn_mut(), Store::Gog, &pairs)?;
    match_by_title(db, Store::Gog, Some(&owned), cancel, progress)?;
    if opts.gamesdb {
        check_gamesdb(db, &api, opts, true, cancel, counters, progress, warnings)?;
    }
    Ok(owned.len() as u32)
}

fn itch_library(
    db: &mut Db,
    key: &str,
    opts: &StoreSyncOptions,
    cancel: &AtomicBool,
    counters: &Counters,
    progress: &mut Progress,
) -> Result<u32> {
    let http = itch::client()?;
    let pacer = Pacer::new(Duration::from_millis(150));
    let owned = itch::owned_games(&http, &opts.endpoints, key, &pacer, cancel, counters)?;
    let products: Vec<_> = owned.iter().map(|o| o.product.clone()).collect();
    store_db::upsert_products(db.conn_mut(), &products, unix_now())?;
    let pairs: Vec<(String, Option<u64>)> = owned
        .iter()
        .map(|o| (o.product.product_id.clone(), Some(o.download_key_id)))
        .collect();
    store_db::set_owned(db.conn_mut(), Store::Itch, &pairs)?;
    let ids: Vec<String> = pairs.into_iter().map(|(id, _)| id).collect();
    progress.emit(SyncPhase::Library, ids.len() as u32, ids.len() as u32);
    match_by_title(db, Store::Itch, Some(&ids), cancel, progress)?;
    Ok(ids.len() as u32)
}
