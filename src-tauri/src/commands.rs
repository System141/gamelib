//! IPC commands. Keep argument and result shapes in sync with `src/lib/api.ts`.

use std::sync::atomic::{AtomicBool, Ordering};

use gamelib_core::db::Db;
use gamelib_core::db::links::{self, get_link};
use gamelib_core::db::read;
use gamelib_core::links::{resolve, validate};
use gamelib_core::model::{
    CatalogStatus, GameDetail, GameLink, GameMedia, GamePage, GameQuery, LinkCheck, LinkInput,
    NewReleasesReport, SiteInfo, SyncProgress, SyncReport, TagInfo, WorkerKind,
};
use gamelib_core::new_releases::{NewReleasesOptions, fetch_new_releases as run_new_releases};
use gamelib_core::steam::SteamClient;
use gamelib_core::sync::{SyncOptions, run_sync};
use gamelib_core::{Error, unix_now};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;

use crate::error::{CmdError, CmdResult};
use crate::state::{AppState, lock};

pub const EVENT_PROGRESS: &str = "sync:progress";
pub const EVENT_FINISHED: &str = "sync:finished";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    #[serde(flatten)]
    pub catalog: CatalogStatus,
    pub worker: Option<WorkerKind>,
    pub progress: Option<SyncProgress>,
    pub db_path: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncFinished {
    pub kind: WorkerKind,
    pub outcome: Outcome,
    pub report: Option<SyncReport>,
    pub new_releases: Option<NewReleasesReport>,
    pub error: Option<CmdError>,
}

enum WorkerReport {
    Full(SyncReport),
    NewReleases(NewReleasesReport),
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenTarget {
    Web,
    Client,
}

// --- catalog --------------------------------------------------------------------------------

#[tauri::command]
pub async fn get_status(state: State<'_, AppState>) -> CmdResult<AppStatus> {
    let catalog = state.read(read::status).await?;
    let worker = state.worker.clone();
    let kind = *lock(&worker.kind);
    Ok(AppStatus {
        catalog,
        worker: kind,
        progress: if kind.is_some() {
            lock(&worker.last_progress).clone()
        } else {
            None
        },
        db_path: state.db_path.display().to_string(),
    })
}

#[tauri::command]
pub fn start_sync(
    app: AppHandle,
    state: State<'_, AppState>,
    fresh: Option<bool>,
) -> CmdResult<()> {
    let opts = SyncOptions {
        fresh: fresh.unwrap_or(false),
        ..SyncOptions::default()
    };
    spawn_worker(
        app,
        &state,
        WorkerKind::Full,
        move |db, client, cancel, progress| {
            run_sync(db, client, &opts, cancel, progress).map(WorkerReport::Full)
        },
    )
}

#[tauri::command]
pub fn fetch_new_releases(
    app: AppHandle,
    state: State<'_, AppState>,
    days: Option<u32>,
) -> CmdResult<()> {
    let opts = NewReleasesOptions {
        days_override: days,
        ..NewReleasesOptions::default()
    };
    spawn_worker(
        app,
        &state,
        WorkerKind::NewReleases,
        move |db, client, cancel, progress| {
            run_new_releases(db, client, &opts, cancel, progress).map(WorkerReport::NewReleases)
        },
    )
}

#[tauri::command]
pub fn cancel_sync(state: State<'_, AppState>) {
    state.worker.cancel.store(true, Ordering::Relaxed);
}

/// Runs a catalog job on its own thread with its own database connection, streaming progress
/// events and ending with exactly one `sync:finished` event.
fn spawn_worker<F>(app: AppHandle, state: &AppState, kind: WorkerKind, job: F) -> CmdResult<()>
where
    F: FnOnce(
            &mut Db,
            &SteamClient,
            &AtomicBool,
            &mut dyn FnMut(&SyncProgress),
        ) -> gamelib_core::Result<WorkerReport>
        + Send
        + 'static,
{
    let worker = state.worker.clone();
    if worker.running.swap(true, Ordering::SeqCst) {
        return Err(CmdError::busy());
    }
    worker.cancel.store(false, Ordering::Relaxed);
    *lock(&worker.kind) = Some(kind);
    *lock(&worker.last_progress) = None;
    let db_path = state.db_path.clone();
    let thread_worker = worker.clone();

    let spawned = std::thread::Builder::new()
        .name("gamelib-worker".into())
        .spawn(move || {
            let worker = thread_worker;
            let result = (|| {
                let mut db = Db::open(&db_path)?;
                let client = SteamClient::new()?;
                let mut on_progress = |p: &SyncProgress| {
                    *lock(&worker.last_progress) = Some(p.clone());
                    let _ = app.emit(EVENT_PROGRESS, p);
                };
                job(&mut db, &client, &worker.cancel, &mut on_progress)
            })();

            let mut finished = SyncFinished {
                kind,
                outcome: Outcome::Completed,
                report: None,
                new_releases: None,
                error: None,
            };
            match result {
                Ok(WorkerReport::Full(r)) => finished.report = Some(r),
                Ok(WorkerReport::NewReleases(r)) => finished.new_releases = Some(r),
                Err(Error::Cancelled) => finished.outcome = Outcome::Cancelled,
                Err(e) => {
                    finished.outcome = Outcome::Failed;
                    finished.error = Some(e.into());
                }
            }
            *lock(&worker.kind) = None;
            *lock(&worker.last_progress) = None;
            worker.running.store(false, Ordering::SeqCst);
            let _ = app.emit(EVENT_FINISHED, finished);
        });

    spawned.map(|_| ()).map_err(|e| {
        *lock(&worker.kind) = None;
        worker.running.store(false, Ordering::SeqCst);
        CmdError::other(e.to_string())
    })
}

#[tauri::command]
pub async fn query_games(state: State<'_, AppState>, params: GameQuery) -> CmdResult<GamePage> {
    state
        .read(move |db| read::query_games(db, &params, unix_now()))
        .await
}

#[tauri::command]
pub async fn get_game(state: State<'_, AppState>, appid: u32) -> CmdResult<Option<GameDetail>> {
    state.read(move |db| read::get_game(db.conn(), appid)).await
}

#[tauri::command]
pub async fn list_tags(state: State<'_, AppState>) -> CmdResult<Vec<TagInfo>> {
    state.read(|db| read::list_tags(db.conn())).await
}

#[tauri::command]
pub async fn get_game_media(appid: u32) -> CmdResult<GameMedia> {
    tauri::async_runtime::spawn_blocking(move || {
        SteamClient::new()?.fetch_media(appid, &AtomicBool::new(false))
    })
    .await
    .map_err(|e| CmdError::other(e.to_string()))?
    .map_err(CmdError::from)
}

// --- external links -------------------------------------------------------------------------

#[tauri::command]
pub fn list_sites(state: State<'_, AppState>) -> Vec<SiteInfo> {
    state.sites.list()
}

#[tauri::command]
pub async fn list_links(state: State<'_, AppState>, appid: u32) -> CmdResult<Vec<GameLink>> {
    state
        .read(move |db| links::list_links(db.conn(), appid))
        .await
}

#[tauri::command]
pub async fn save_link(state: State<'_, AppState>, input: LinkInput) -> CmdResult<GameLink> {
    let sites = state.sites.clone();
    state
        .write(move |db| links::save_link(db.conn(), &sites, &input, unix_now()))
        .await
}

#[tauri::command]
pub async fn delete_link(state: State<'_, AppState>, id: i64) -> CmdResult<bool> {
    state
        .write(move |db| links::delete_link(db.conn(), id))
        .await
}

/// Follows the link's redirects (no download) with its site's handler and stores the result.
#[tauri::command]
pub async fn check_link(state: State<'_, AppState>, id: i64) -> CmdResult<LinkCheck> {
    let link = state
        .read(move |db| get_link(db.conn(), id))
        .await?
        .ok_or(CmdError::from(Error::NotFound))?;
    let sites = state.sites.clone();
    let check = tauri::async_runtime::spawn_blocking(move || -> gamelib_core::Result<LinkCheck> {
        let url = validate::parse_link_url(&link.url)?;
        let handler = sites
            .get(&link.site_id)
            .unwrap_or_else(|| sites.detect(&url));
        Ok(handler.resolve(&url, &resolve::client()?))
    })
    .await
    .map_err(|e| CmdError::other(e.to_string()))??;
    let stored = check.clone();
    state
        .write(move |db| links::record_check(db.conn(), id, &stored))
        .await?;
    Ok(check)
}

/// Opens a saved link in the default browser. Only validated http(s) URLs are ever opened.
#[tauri::command]
pub async fn open_link(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let link = state
        .read(move |db| get_link(db.conn(), id))
        .await?
        .ok_or(CmdError::from(Error::NotFound))?;
    let url = validate::parse_link_url(&link.url)?;
    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map_err(|e| CmdError::other(e.to_string()))
}

#[tauri::command]
pub fn open_in_steam(app: AppHandle, appid: u32, target: OpenTarget) -> CmdResult<()> {
    let url = match target {
        OpenTarget::Web => format!("https://store.steampowered.com/app/{appid}/"),
        OpenTarget::Client => format!("steam://store/{appid}"),
    };
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| CmdError::other(e.to_string()))
}
