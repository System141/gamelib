mod commands;
mod error;
mod state;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Local (non-roaming) data dir: the catalog is ~150 MB and can always be re-downloaded.
            let dir = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            app.manage(state::AppState::open(dir.join("gamelib.db"))?);
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
            commands::list_sites,
            commands::list_links,
            commands::save_link,
            commands::delete_link,
            commands::check_link,
            commands::open_link,
            commands::open_in_steam,
        ])
        .run(tauri::generate_context!())
        .expect("error while running GameLib");
}
