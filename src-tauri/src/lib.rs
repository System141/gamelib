mod browser;
mod commands;
mod error;
mod login;
mod updates;

use std::sync::Arc;

use gamelib_core::app::{App, EventSink};
use tauri::{AppHandle, Emitter, Manager};

/// Forwards job and download events to the webview.
struct TauriSink(AppHandle);

impl EventSink for TauriSink {
    fn emit(&self, event: &str, payload: serde_json::Value) {
        let _ = self.0.emit(event, payload);
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updates::PendingUpdate::default())
        .setup(|app| {
            // Local (non-roaming) data dir: the catalog is ~150 MB and can always be re-downloaded.
            // `gamelib-cli` uses the same file by default (see crates/gamelib-cli/src/main.rs).
            let dir = app.path().app_local_data_dir()?;
            let sink = Arc::new(TauriSink(app.handle().clone()));
            let core = Arc::new(App::open(dir.join("gamelib.db"), sink)?);
            // Downloads interrupted by the last exit continue now.
            if let Err(e) = core.start_downloads() {
                eprintln!("could not start downloads: {e}");
            }
            app.manage(core);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::start_sync,
            commands::fetch_new_releases,
            commands::cancel_sync,
            commands::query_games,
            commands::get_game,
            commands::list_tags,
            commands::get_game_media,
            commands::get_game_reviews,
            commands::get_game_requirements,
            commands::get_game_prices,
            commands::itad_set_key,
            commands::itad_remove_key,
            commands::open_price_link,
            commands::start_store_sync,
            commands::get_store_matches,
            commands::refresh_store_matches,
            commands::set_match_state,
            commands::open_store_page,
            commands::search_store,
            commands::link_store_product,
            commands::get_accounts,
            commands::gog_login_url,
            commands::open_gog_login_page,
            commands::gog_login,
            commands::gog_login_with_code,
            commands::itch_set_key,
            commands::sign_out,
            commands::start_library_sync,
            commands::get_library,
            commands::open_account_page,
            commands::get_settings,
            commands::update_settings,
            commands::pick_library_dir,
            commands::get_store_files,
            commands::enqueue_download,
            commands::enqueue_torrent,
            commands::get_downloads,
            commands::pause_download,
            commands::resume_download,
            commands::remove_download,
            commands::clear_finished_downloads,
            commands::open_download_folder,
            commands::approve_install,
            commands::retry_install,
            commands::get_installs,
            commands::launch_game,
            commands::uninstall_game,
            commands::open_install_folder,
            commands::set_launch_target,
            commands::pick_launch_target,
            updates::get_update_status,
            updates::check_update,
            updates::install_update,
            updates::open_release_page,
            commands::list_sites,
            commands::list_links,
            commands::find_links,
            commands::save_link,
            commands::delete_link,
            commands::check_link,
            commands::open_link,
            commands::open_in_steam,
            commands::open_browser,
            commands::open_search,
        ])
        .run(tauri::generate_context!())
        .expect("error while running GameLib");
}
