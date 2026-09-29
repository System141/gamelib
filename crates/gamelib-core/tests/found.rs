//! Games installed outside GameLib: found in Steam's and Epic's libraries and in game folders,
//! tied to Steam games, started, hidden and matched by hand.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use gamelib_core::app::{App, EventSink, JobOptions};
use gamelib_core::db::installs::{self, InstallRow};
use gamelib_core::db::{Db, write::upsert_games};
use gamelib_core::install::found::Launchers;
use gamelib_core::model::{
    FoundMatch, FoundSource, InstallMethod, Installed, SettingsPatch, Store,
};
use gamelib_core::record::GameRecord;
use gamelib_core::{Error, unix_now};
use serde_json::Value;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "gamelib-found-{name}-{}-{}",
            std::process::id(),
            unix_now()
        ));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct Events(Mutex<Vec<(String, Value)>>);

impl EventSink for Events {
    fn emit(&self, event: &str, payload: Value) {
        self.0.lock().unwrap().push((event.to_owned(), payload));
    }
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A program for this system: `name.exe` on Windows, an `.app` bundle on macOS, an executable
/// `name.x86_64` on Linux.
fn program(dir: &Path, name: &str) -> PathBuf {
    if cfg!(windows) {
        let path = dir.join(format!("{name}.exe"));
        write(&path, "MZ");
        path
    } else if cfg!(target_os = "macos") {
        let path = dir.join(format!("{name}.app"));
        write(&path.join("Contents").join("MacOS").join(name), "#!/bin/sh");
        path
    } else {
        let path = dir.join(format!("{name}.x86_64"));
        write(&path, "#!/bin/sh");
        path
    }
}

/// A Steam installation with two games installed and one still downloading, Epic's manifests
/// with one game, and a library folder with a GOG game, a game known only by its folder name,
/// a folder that is not a game and a game GameLib installed itself.
fn machine(base: &Path) -> Launchers {
    let steam = base.join("Steam");
    let apps = steam.join("steamapps");
    write(
        &apps.join("libraryfolders.vdf"),
        &format!(
            "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}\n",
            steam.display().to_string().replace('\\', "\\\\")
        ),
    );
    for (appid, name, dir, flags) in [
        (292030, "The Witcher 3: Wild Hunt", "The Witcher 3", 4),
        (
            228980,
            "Steamworks Common Redistributables",
            "Steamworks Shared",
            4,
        ),
        (413150, "Stardew Valley", "Stardew Valley", 1026),
    ] {
        write(
            &apps.join(format!("appmanifest_{appid}.acf")),
            &format!(
                "\"AppState\"\n{{\n\t\"appid\"\t\t\"{appid}\"\n\t\"name\"\t\t\"{name}\"\n\t\"StateFlags\"\t\t\"{flags}\"\n\t\"installdir\"\t\t\"{dir}\"\n}}\n"
            ),
        );
        fs::create_dir_all(apps.join("common").join(dir)).unwrap();
    }

    let manifests = base.join("Epic").join("Manifests");
    let control = base.join("Epic Games").join("Control");
    fs::create_dir_all(&control).unwrap();
    write(
        &manifests.join("control.item"),
        &serde_json::json!({
            "DisplayName": "Control",
            "InstallLocation": control.display().to_string(),
            "AppName": "Calluna",
            "MainGameAppName": "Calluna",
            "CatalogNamespace": "calluna",
            "CatalogItemId": "c8e4",
            "AppCategories": ["public", "games"],
        })
        .to_string(),
    );

    let games = base.join("Games");
    let terraria = games.join("Terraria GOG");
    write(
        &terraria.join("goggame-1207665503.info"),
        r#"{"gameId":"1207665503","rootGameId":"1207665503","name":"Terraria","playTasks":[{"isPrimary":true,"type":"FileTask","path":"Terraria.exe"}]}"#,
    );
    write(&terraria.join("Terraria.exe"), "MZ");
    program(&games.join("HollowKnight"), "hollow_knight");
    program(&games.join("HollowKnight").join("Tools"), "modinstaller");
    write(&games.join("Screenshots").join("shot.png"), "png");
    program(&games.join("Installed Here"), "game");

    Launchers {
        steam: vec![steam],
        epic: Some(manifests),
    }
}

fn app(dir: &TempDir, events: Arc<Events>) -> App {
    let path = dir.0.join("gamelib.db");
    let mut db = Db::open(&path).unwrap();
    let records: Vec<GameRecord> = [
        (292030, "The Witcher 3: Wild Hunt"),
        (413150, "Stardew Valley"),
        (105600, "Terraria"),
        (367520, "Hollow Knight"),
        (870780, "Control Ultimate Edition"),
        // A namesake: with two, a title alone decides nothing.
        (999999, "Control"),
    ]
    .iter()
    .map(|(appid, name)| {
        GameRecord::from_item(&common::item(*appid, name, 1_500_000_000, 1000, &[19])).unwrap()
    })
    .collect();
    upsert_games(db.conn_mut(), &records, unix_now()).unwrap();
    // GameLib installed one game into the library itself.
    installs::upsert(
        db.conn(),
        &InstallRow {
            installed: Installed {
                store: Store::Gog,
                product_id: "1".into(),
                appid: None,
                title: "Installed Here".into(),
                dir: Some(
                    dir.0
                        .join("Games")
                        .join("Installed Here")
                        .display()
                        .to_string(),
                ),
                exe: None,
                args: String::new(),
                workdir: None,
                method: InstallMethod::Archive,
                candidates: Vec::new(),
                option_label: None,
                installed_at: 1,
                external: false,
                steam_header: None,
                source: None,
                launch_url: None,
                matched_by: None,
            },
            uninstaller: None,
        },
    )
    .unwrap();
    drop(db);
    let options = JobOptions {
        launchers: Some(machine(&dir.0)),
        ..Default::default()
    };
    let app = App::with_options(path, events, options).unwrap();
    app.update_settings(&SettingsPatch {
        library_dir: Some(dir.0.join("Games").display().to_string()),
        ..Default::default()
    })
    .unwrap();
    app
}

fn find<'a>(list: &'a [Installed], id: &str) -> &'a Installed {
    list.iter()
        .find(|i| i.product_id == id)
        .unwrap_or_else(|| panic!("{id} in {list:#?}"))
}

#[test]
fn finds_games_in_launchers_and_folders() {
    let dir = TempDir::new("scan");
    let events = Arc::new(Events::default());
    let app = app(&dir, events.clone());

    let report = app.scan_installed().unwrap();
    assert_eq!((report.found, report.added, report.removed), (4, 4, 0));
    assert!(
        events
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|(e, p)| e == "install:changed" && p["store"] == "local")
    );

    let list = app.installs().unwrap();
    let titles: Vec<&str> = list.iter().map(|i| i.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "Control",
            "Hollow Knight",
            "Installed Here",
            "Terraria",
            "The Witcher 3: Wild Hunt"
        ],
        "by title; the Steam redistributables and the half-downloaded game are not games"
    );

    let witcher = find(&list, "steam:292030");
    assert_eq!(witcher.store, Store::Local);
    assert_eq!(witcher.method, InstallMethod::Found);
    assert_eq!(witcher.source, Some(FoundSource::Steam));
    assert_eq!(
        (witcher.appid, witcher.matched_by),
        (Some(292030), Some(FoundMatch::Steam))
    );
    assert!(witcher.external && witcher.exe.is_none());
    assert!(witcher.steam_header.is_some());
    assert_eq!(
        app.launch_game(Store::Local, "steam:292030")
            .unwrap()
            .as_deref(),
        Some("steam://rungameid/292030"),
        "Steam starts its games"
    );
    assert_eq!(
        app.uninstall_game(Store::Local, "steam:292030")
            .unwrap()
            .as_deref(),
        Some("steam://uninstall/292030"),
        "and removes them"
    );

    let control = find(&list, "epic:Calluna");
    assert_eq!(control.source, Some(FoundSource::Epic));
    assert_eq!(
        (control.appid, control.matched_by),
        (None, None),
        "two Steam games are called that"
    );
    assert!(
        control
            .launch_url
            .as_deref()
            .unwrap()
            .starts_with("com.epicgames.launcher://apps/calluna%3Ac8e4%3ACalluna")
    );

    // GOG's info file names the game; the name finds it in the catalog.
    let terraria_id = format!(
        "folder:{}",
        dir.0.join("Games").join("Terraria GOG").display()
    );
    let terraria = find(&list, &terraria_id);
    assert_eq!(
        (terraria.appid, terraria.matched_by),
        (Some(105600), Some(FoundMatch::Title))
    );
    assert!(terraria.exe.as_deref().unwrap().ends_with("Terraria.exe"));

    let hollow_id = format!(
        "folder:{}",
        dir.0.join("Games").join("HollowKnight").display()
    );
    let hollow = find(&list, &hollow_id);
    assert_eq!(hollow.appid, Some(367520), "the folder name without spaces");
    assert_eq!(hollow.title, "Hollow Knight", "shown by its Steam name");
    assert!(hollow.exe.as_deref().unwrap().contains("hollow_knight"));
    assert_eq!(hollow.candidates.len(), 2);
    assert!(matches!(
        app.uninstall_game(Store::Local, &hollow_id),
        Err(Error::Invalid("found_uninstall"))
    ));

    // What a Steam or Epic game starts is up to its launcher.
    let tool = hollow.candidates[1].clone();
    assert!(matches!(
        app.set_launch_target(Store::Local, "epic:Calluna", &tool),
        Err(Error::Invalid("launch_target"))
    ));
    let picked = app
        .set_launch_target(Store::Local, &hollow_id, &tool)
        .unwrap();
    assert_eq!(picked.exe.as_deref(), Some(tool.as_str()));
}

#[test]
fn hiding_matching_and_rescans() {
    let dir = TempDir::new("choices");
    let app = app(&dir, Arc::new(Events::default()));
    app.scan_installed().unwrap();
    let hollow_id = format!(
        "folder:{}",
        dir.0.join("Games").join("HollowKnight").display()
    );

    // Epic's Control is Steam's "Control Ultimate Edition", not its namesake: the user says so.
    let control = app.match_found("epic:Calluna", Some(870780)).unwrap();
    assert_eq!(
        (control.appid, control.matched_by),
        (Some(870780), Some(FoundMatch::Manual))
    );
    assert_eq!(control.title, "Control Ultimate Edition");
    assert!(control.steam_header.is_some());
    assert!(matches!(
        app.match_found("epic:Calluna", Some(1)),
        Err(Error::NotFound)
    ));
    // Not a Steam game at all.
    let none = app.match_found(&hollow_id, None).unwrap();
    assert_eq!(
        (none.appid, none.matched_by),
        (None, Some(FoundMatch::Manual))
    );

    app.set_found_hidden(&hollow_id, true).unwrap();
    assert!(
        app.installs()
            .unwrap()
            .iter()
            .all(|i| i.product_id != hollow_id)
    );
    assert_eq!(app.hidden_found().unwrap()[0].product_id, hollow_id);

    // A rescan keeps all of it.
    let report = app.scan_installed().unwrap();
    assert_eq!((report.added, report.removed), (0, 0));
    let list = app.installs().unwrap();
    assert_eq!(find(&list, "epic:Calluna").appid, Some(870780));
    assert!(list.iter().all(|i| i.product_id != hollow_id));
    let hidden = app.hidden_found().unwrap();
    assert_eq!(hidden.len(), 1);
    assert_eq!(hidden[0].appid, None);

    app.set_found_hidden(&hollow_id, false).unwrap();
    assert!(
        app.installs()
            .unwrap()
            .iter()
            .any(|i| i.product_id == hollow_id)
    );

    // A folder the user adds is scanned too; a game removed from the disk goes.
    let extra = dir.0.join("D").join("Oyunlar");
    program(&extra.join("Stardew Valley"), "Stardew Valley");
    app.update_settings(&SettingsPatch {
        scan_dirs: Some(vec![
            extra.display().to_string(),
            format!("{}/", extra.display()),
        ]),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        app.settings().unwrap().scan_dirs.len(),
        1,
        "each folder once"
    );
    fs::remove_dir_all(dir.0.join("Games").join("Terraria GOG")).unwrap();
    let report = app.scan_installed().unwrap();
    assert_eq!((report.added, report.removed), (1, 1));
    let list = app.installs().unwrap();
    let stardew = list
        .iter()
        .find(|i| i.product_id.ends_with("Stardew Valley"))
        .unwrap();
    assert_eq!(stardew.appid, Some(413150));

    assert!(matches!(
        app.update_settings(&SettingsPatch {
            scan_dirs: Some(vec!["relative/path".into()]),
            ..Default::default()
        }),
        Err(Error::Invalid("scan_dir"))
    ));
}
