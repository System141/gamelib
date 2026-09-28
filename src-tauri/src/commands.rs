//! IPC commands: thin wrappers over `gamelib_core::app::App`, which `gamelib-cli serve` shares.
//! Keep argument and result shapes in sync with `src/lib/api.ts`.

use std::sync::Arc;

use gamelib_core::app::{App, steam_url};
use gamelib_core::model::{
    Accounts, AppStatus, Download, DownloadList, FileOption, GameDetail, GameLink, GameMedia,
    GamePage, GameQuery, LibraryItem, LinkCheck, LinkInput, MatchState, OpenTarget, Settings,
    SettingsPatch, SiteInfo, Store, StoreMatch, StoreSearchHit, TagInfo,
};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
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

/// Searches a store (itch.io) for the game; results can then be tied to it.
#[tauri::command]
pub async fn search_store(
    app: State<'_, Arc<App>>,
    store: Store,
    appid: u32,
) -> CmdResult<Vec<StoreSearchHit>> {
    blocking(&app, move |app| app.search_store(store, appid)).await
}

#[tauri::command]
pub async fn link_store_product(
    app: State<'_, Arc<App>>,
    store: Store,
    product_id: String,
    appid: u32,
) -> CmdResult<()> {
    blocking(&app, move |app| {
        app.link_store_product(store, &product_id, appid)
    })
    .await
}

// --- accounts and library -------------------------------------------------------------------

#[tauri::command]
pub async fn get_accounts(app: State<'_, Arc<App>>) -> CmdResult<Accounts> {
    blocking(&app, App::accounts).await
}

#[tauri::command]
pub fn gog_login_url(app: State<'_, Arc<App>>) -> String {
    app.gog_login_url()
}

/// Opens GOG's login page in the default browser, for signing in with the redirect address.
#[tauri::command]
pub fn open_gog_login_page(handle: AppHandle, app: State<'_, Arc<App>>) -> CmdResult<()> {
    open_url(&handle, app.gog_login_url())
}

/// Signs in to GOG in a separate window showing GOG's own login page.
#[tauri::command]
pub async fn gog_login(handle: AppHandle, app: State<'_, Arc<App>>) -> CmdResult<Accounts> {
    let url = app.gog_login_url();
    let Some(redirect) = crate::login::gog_redirect(&handle, &url).await? else {
        return Err(gamelib_core::Error::Cancelled.into());
    };
    blocking(&app, move |app| app.gog_login_with_code(&redirect)).await
}

/// Finishes a GOG sign-in with the address the login page ended on (or its code).
#[tauri::command]
pub async fn gog_login_with_code(
    app: State<'_, Arc<App>>,
    redirect: String,
) -> CmdResult<Accounts> {
    blocking(&app, move |app| app.gog_login_with_code(&redirect)).await
}

#[tauri::command]
pub async fn itch_set_key(app: State<'_, Arc<App>>, key: String) -> CmdResult<Accounts> {
    blocking(&app, move |app| app.itch_set_key(&key)).await
}

#[tauri::command]
pub async fn sign_out(app: State<'_, Arc<App>>, store: Store) -> CmdResult<Accounts> {
    blocking(&app, move |app| app.sign_out(store)).await
}

#[tauri::command]
pub fn start_library_sync(app: State<'_, Arc<App>>) -> CmdResult<()> {
    Ok(app.start_library_sync()?)
}

#[tauri::command]
pub async fn get_library(
    app: State<'_, Arc<App>>,
    store: Option<Store>,
) -> CmdResult<Vec<LibraryItem>> {
    blocking(&app, move |app| app.library(store)).await
}

/// Opens a store page where account settings live (itch.io API keys).
#[tauri::command]
pub fn open_account_page(handle: AppHandle, store: Store) -> CmdResult<()> {
    open_url(&handle, gamelib_core::app::account_page(store).to_owned())
}

// --- settings -------------------------------------------------------------------------------

#[tauri::command]
pub async fn get_settings(app: State<'_, Arc<App>>) -> CmdResult<Settings> {
    blocking(&app, App::settings).await
}

#[tauri::command]
pub async fn update_settings(
    app: State<'_, Arc<App>>,
    patch: SettingsPatch,
) -> CmdResult<Settings> {
    blocking(&app, move |app| app.update_settings(&patch)).await
}

/// Lets the user choose the library folder; `None` if the dialog was cancelled.
#[tauri::command]
pub async fn pick_library_dir(
    handle: AppHandle,
    app: State<'_, Arc<App>>,
) -> CmdResult<Option<Settings>> {
    let current = blocking(&app, App::settings).await?.library_dir;
    let (tx, rx) = std::sync::mpsc::channel();
    handle
        .dialog()
        .file()
        .set_title("Kütüphane klasörü")
        .set_directory(&current)
        .pick_folder(move |picked| {
            let _ = tx.send(picked);
        });
    let picked = tauri::async_runtime::spawn_blocking(move || rx.recv().ok().flatten())
        .await
        .map_err(|e| CmdError::other(e.to_string()))?;
    let Some(dir) = picked.and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let patch = SettingsPatch {
        library_dir: Some(dir.display().to_string()),
        ..Default::default()
    };
    blocking(&app, move |app| app.update_settings(&patch))
        .await
        .map(Some)
}

// --- downloads ------------------------------------------------------------------------------

/// What can be downloaded for a store product, the best variant for this computer first.
#[tauri::command]
pub async fn get_store_files(
    app: State<'_, Arc<App>>,
    store: Store,
    product_id: String,
) -> CmdResult<Vec<FileOption>> {
    blocking(&app, move |app| app.store_files(store, &product_id)).await
}

#[tauri::command]
pub async fn enqueue_download(
    app: State<'_, Arc<App>>,
    store: Store,
    product_id: String,
    option_id: String,
) -> CmdResult<Download> {
    blocking(&app, move |app| {
        app.enqueue_download(store, &product_id, &option_id)
    })
    .await
}

#[tauri::command]
pub async fn get_downloads(app: State<'_, Arc<App>>) -> CmdResult<DownloadList> {
    blocking(&app, App::downloads).await
}

#[tauri::command]
pub async fn pause_download(app: State<'_, Arc<App>>, id: i64) -> CmdResult<()> {
    blocking(&app, move |app| app.pause_download(id)).await
}

#[tauri::command]
pub async fn resume_download(app: State<'_, Arc<App>>, id: i64) -> CmdResult<()> {
    blocking(&app, move |app| app.resume_download(id)).await
}

/// Cancels a download and deletes its files.
#[tauri::command]
pub async fn remove_download(app: State<'_, Arc<App>>, id: i64) -> CmdResult<()> {
    blocking(&app, move |app| app.remove_download(id)).await
}

#[tauri::command]
pub async fn clear_finished_downloads(app: State<'_, Arc<App>>) -> CmdResult<()> {
    blocking(&app, App::clear_finished_downloads).await
}

/// Opens a download's folder in the file manager.
#[tauri::command]
pub async fn open_download_folder(
    handle: AppHandle,
    app: State<'_, Arc<App>>,
    id: i64,
) -> CmdResult<()> {
    let dir = blocking(&app, move |app| app.download_folder(id)).await?;
    handle
        .opener()
        .open_path(dir.display().to_string(), None::<&str>)
        .map_err(|e| CmdError::other(e.to_string()))
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
