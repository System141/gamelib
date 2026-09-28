//! IPC commands: thin wrappers over `gamelib_core::app::App`, which `gamelib-cli serve` shares.
//! Keep argument and result shapes in sync with `src/lib/api.ts`.

use std::sync::Arc;

use gamelib_core::app::{App, steam_url};
use gamelib_core::model::{
    AppStatus, GameDetail, GameLink, GameMedia, GamePage, GameQuery, LinkCheck, LinkInput,
    MatchState, OpenTarget, SiteInfo, Store, StoreMatch, TagInfo,
};
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::error::{CmdError, CmdResult};

/// Runs `f` on the blocking thread pool: queries and link checks must not stall the UI thread.
async fn blocking<T: Send + 'static>(
    app: &State<'_, Arc<App>>,
    f: impl FnOnce(&App) -> gamelib_core::Result<T> + Send + 'static,
) -> CmdResult<T> {
    let app = Arc::clone(app);
    tauri::async_runtime::spawn_blocking(move || f(&app).map_err(CmdError::from))
        .await
        .map_err(|e| CmdError::other(e.to_string()))?
}

fn open_url(handle: &AppHandle, url: String) -> CmdResult<()> {
    handle
        .opener()
        .open_url(url, None::<&str>)
        .map_err(|e| CmdError::other(e.to_string()))
}

// --- catalog --------------------------------------------------------------------------------

#[tauri::command]
pub async fn get_status(app: State<'_, Arc<App>>) -> CmdResult<AppStatus> {
    blocking(&app, App::status).await
}

#[tauri::command]
pub fn start_sync(app: State<'_, Arc<App>>, fresh: Option<bool>) -> CmdResult<()> {
    Ok(app.start_sync(fresh.unwrap_or(false))?)
}

#[tauri::command]
pub fn fetch_new_releases(app: State<'_, Arc<App>>, days: Option<u32>) -> CmdResult<()> {
    Ok(app.fetch_new_releases(days)?)
}

#[tauri::command]
pub fn cancel_sync(app: State<'_, Arc<App>>) {
    app.cancel_job();
}

#[tauri::command]
pub async fn query_games(app: State<'_, Arc<App>>, params: GameQuery) -> CmdResult<GamePage> {
    blocking(&app, move |app| app.query_games(&params)).await
}

#[tauri::command]
pub async fn get_game(app: State<'_, Arc<App>>, appid: u32) -> CmdResult<Option<GameDetail>> {
    blocking(&app, move |app| app.get_game(appid)).await
}

#[tauri::command]
pub async fn list_tags(app: State<'_, Arc<App>>) -> CmdResult<Vec<TagInfo>> {
    blocking(&app, App::list_tags).await
}

#[tauri::command]
pub async fn get_game_media(app: State<'_, Arc<App>>, appid: u32) -> CmdResult<GameMedia> {
    blocking(&app, move |app| app.game_media(appid)).await
}

// --- other stores ---------------------------------------------------------------------------

#[tauri::command]
pub fn start_store_sync(app: State<'_, Arc<App>>) -> CmdResult<()> {
    Ok(app.start_store_sync()?)
}

#[tauri::command]
pub async fn get_store_matches(app: State<'_, Arc<App>>, appid: u32) -> CmdResult<Vec<StoreMatch>> {
    blocking(&app, move |app| app.store_matches(appid)).await
}

/// Looks the game up in GOG's GamesDB (at most monthly), then returns its matches.
#[tauri::command]
pub async fn refresh_store_matches(
    app: State<'_, Arc<App>>,
    appid: u32,
) -> CmdResult<Vec<StoreMatch>> {
    blocking(&app, move |app| app.refresh_store_matches(appid)).await
}

#[tauri::command]
pub async fn set_match_state(
    app: State<'_, Arc<App>>,
    store: Store,
    product_id: String,
    appid: u32,
    state: MatchState,
) -> CmdResult<()> {
    blocking(&app, move |app| {
        app.set_match_state(store, &product_id, appid, state)
    })
    .await
}

/// Opens a product's page on its store (https, store domains only).
#[tauri::command]
pub async fn open_store_page(
    handle: AppHandle,
    app: State<'_, Arc<App>>,
    store: Store,
    product_id: String,
) -> CmdResult<()> {
    let url = blocking(&app, move |app| app.store_product_url(store, &product_id)).await?;
    open_url(&handle, url)
}

// --- external links -------------------------------------------------------------------------

#[tauri::command]
pub fn list_sites(app: State<'_, Arc<App>>) -> Vec<SiteInfo> {
    app.list_sites()
}

#[tauri::command]
pub async fn list_links(app: State<'_, Arc<App>>, appid: u32) -> CmdResult<Vec<GameLink>> {
    blocking(&app, move |app| app.list_links(appid)).await
}

#[tauri::command]
pub async fn save_link(app: State<'_, Arc<App>>, input: LinkInput) -> CmdResult<GameLink> {
    blocking(&app, move |app| app.save_link(&input)).await
}

#[tauri::command]
pub async fn delete_link(app: State<'_, Arc<App>>, id: i64) -> CmdResult<bool> {
    blocking(&app, move |app| app.delete_link(id)).await
}

/// Follows the link's redirects (no download) with its site's handler and stores the result.
#[tauri::command]
pub async fn check_link(app: State<'_, Arc<App>>, id: i64) -> CmdResult<LinkCheck> {
    blocking(&app, move |app| app.check_link(id)).await
}

/// Opens a saved link in the default browser. Only validated http(s) URLs are ever opened.
#[tauri::command]
pub async fn open_link(handle: AppHandle, app: State<'_, Arc<App>>, id: i64) -> CmdResult<()> {
    let url = blocking(&app, move |app| app.link_url(id)).await?;
    open_url(&handle, url)
}

#[tauri::command]
pub fn open_in_steam(handle: AppHandle, appid: u32, target: OpenTarget) -> CmdResult<()> {
    open_url(&handle, steam_url(appid, target))
}
