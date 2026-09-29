//! Finding games installed outside GameLib: in Steam's libraries (`appmanifest_*.acf`), in Epic
//! Games Launcher's manifests (`*.item`) and in game folders (the library folder and folders the
//! user adds). This only reads the disk; [`super::found`] ties the games to Steam's.

use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::targets::{self, LaunchTarget};
use super::vdf;
use crate::model::{FoundSource, Platform};

/// Steam's "fully installed" state flag.
const STEAM_INSTALLED: u64 = 4;
/// Manifests and receipts are small; anything larger is not one.
const MAX_FILE: u64 = 4 << 20;
/// Folders in a scanned folder that are never games, including a system drive's own (when a
/// whole drive is added).
const NOT_GAMES: &[&str] = &[
    "steamapps",
    "$recycle.bin",
    "system volume information",
    "windows",
    "program files",
    "program files (x86)",
    "programdata",
    "users",
    "perflogs",
    "recovery",
    "msocache",
    "$windows.~bt",
    "$windows.~ws",
    "windowsapps",
    "xboxgames",
];

/// Where to look.
#[derive(Debug, Clone)]
pub struct ScanPlan {
    /// Steam installations (each with `steamapps/libraryfolders.vdf`).
    pub steam: Vec<PathBuf>,
    /// Epic Games Launcher's manifests folder.
    pub epic: Option<PathBuf>,
    /// Folders whose game folders count: the library folder, then the user's.
    pub folders: Vec<PathBuf>,
    /// Folders of games already listed (GameLib's installs, GOG Galaxy's): never reported.
    pub known: Vec<PathBuf>,
    pub platform: Platform,
}

/// A game found on the disk.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    /// `steam:<appid>`, `epic:<app name>` or `folder:<path>`.
    pub id: String,
    pub source: FoundSource,
    pub title: String,
    pub dir: PathBuf,
    /// The Steam library, manifests folder or scanned folder it was found in.
    pub root: PathBuf,
    pub clue: Clue,
    pub launch: Option<LaunchTarget>,
    /// Games that start through their launcher.
    pub launch_url: Option<String>,
    pub candidates: Vec<PathBuf>,
}

/// What tells which game a found one is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Clue {
    /// Steam's own app id.
    Steam(u32),
    /// A GOG product (`goggame-<id>.info`).
    Gog(String),
    /// An itch.io game (the itch app's receipt).
    Itch(String),
    /// A `steam_appid.txt` next to the game.
    SteamAppid(u32),
    /// Only the title.
    Title,
}

#[derive(Debug, Default)]
pub struct Scan {
    pub games: Vec<Found>,
    /// Places that could not be read (a drive that is not connected): what was found there
    /// before is kept.
    pub unreachable: Vec<PathBuf>,
}

pub fn scan(plan: &ScanPlan) -> Scan {
    let mut out = Scan::default();
    let mut libraries = HashSet::new();
    for steam in &plan.steam {
        steam_games(steam, &mut libraries, &mut out);
    }
    if let Some(manifests) = &plan.epic {
        epic_games(manifests, &mut out);
    }
    // Folders already listed, found in a launcher's library or scanned on their own are not
    // game folders of another scanned folder.
    let taken: Vec<String> = plan
        .known
        .iter()
        .chain(out.games.iter().map(|g| &g.dir))
        .chain(&plan.folders)
        .map(|p| path_key(p))
        .collect();
    let mut roots = HashSet::new();
    for root in &plan.folders {
        if roots.insert(path_key(root)) {
            folder_games(root, plan.platform, &taken, &mut out);
        }
    }
    let mut ids = HashSet::new();
    out.games.retain(|g| ids.insert(g.id.clone()));
    out
}

// --- Steam -------------------------------------------------------------------------------------

fn steam_games(steam: &Path, seen: &mut HashSet<String>, out: &mut Scan) {
    let mut libraries = vec![steam.to_path_buf()];
    for file in [
        steam.join("steamapps").join("libraryfolders.vdf"),
        steam.join("config").join("libraryfolders.vdf"),
    ] {
        let Some(root) = read_small(&file).and_then(|t| vdf::parse(&t)) else {
            continue;
        };
        let Some(folders) = root.get("libraryfolders") else {
            continue;
        };
        for (key, entry) in folders.entries() {
            // Libraries are numbered; current files hold `{ "path": … }`, old ones the path.
            if !key.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            if let Some(path) = entry.str("path").or_else(|| entry.as_str()) {
                libraries.push(PathBuf::from(path));
            }
        }
    }
    for library in libraries {
        if !seen.insert(real_key(&library)) {
            continue;
        }
        let apps = library.join("steamapps");
        let Ok(entries) = fs::read_dir(&apps) else {
            out.unreachable.push(library);
            continue;
        };
        let mut manifests: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("appmanifest_") && n.ends_with(".acf"))
            })
            .collect();
        manifests.sort();
        out.games.extend(
            manifests
                .iter()
                .filter_map(|m| steam_app(m, &apps, &library)),
        );
    }
}

fn steam_app(manifest: &Path, apps: &Path, library: &Path) -> Option<Found> {
    let root = vdf::parse(&read_small(manifest)?)?;
    let state = root.get("AppState")?;
    let appid: u32 = state.str("appid")?.trim().parse().ok()?;
    let flags: u64 = state
        .str("StateFlags")
        .and_then(|f| f.trim().parse().ok())
        .unwrap_or(0);
    let folder = state.str("installdir")?.trim();
    if flags & STEAM_INSTALLED == 0 || !plain_name(folder) {
        return None;
    }
    let dir = apps.join("common").join(folder);
    if !dir.is_dir() {
        return None;
    }
    Some(Found {
        id: format!("steam:{appid}"),
        source: FoundSource::Steam,
        title: state.str("name").unwrap_or(folder).trim().to_owned(),
        dir,
        root: library.to_path_buf(),
        clue: Clue::Steam(appid),
        launch: None,
        launch_url: Some(format!("steam://rungameid/{appid}")),
        candidates: Vec::new(),
    })
}

// --- Epic --------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct EpicItem {
    #[serde(rename = "bIsIncompleteInstall", default)]
    incomplete: bool,
    #[serde(rename = "DisplayName", default)]
    display_name: String,
    #[serde(rename = "InstallLocation", default)]
    install_location: String,
    #[serde(rename = "AppName", default)]
    app_name: String,
    #[serde(rename = "MainGameAppName", default)]
    main_game_app_name: String,
    #[serde(rename = "CatalogNamespace", default)]
    namespace: String,
    #[serde(rename = "CatalogItemId", default)]
    item_id: String,
    #[serde(rename = "AppCategories", default)]
    categories: Vec<String>,
}

/// Without the folder the launcher is not installed, which is no reason to keep old entries, so
/// it never counts as unreachable.
fn epic_games(manifests: &Path, out: &mut Scan) {
    let Ok(entries) = fs::read_dir(manifests) else {
        return;
    };
    let mut items: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("item"))
        })
        .collect();
    items.sort();
    for path in items {
        let item = read_small(&path).and_then(|t| serde_json::from_str::<EpicItem>(&t).ok());
        if let Some(game) = item.and_then(|i| epic_game(i, manifests)) {
            out.games.push(game);
        }
    }
}

fn epic_game(item: EpicItem, manifests: &Path) -> Option<Found> {
    // Half-installed games, add-ons and what is not a game (Unreal Engine) are left out.
    let add_on = !item.main_game_app_name.is_empty() && item.main_game_app_name != item.app_name;
    let game = item.categories.is_empty() || item.categories.iter().any(|c| c == "games");
    if item.incomplete || add_on || !game || item.app_name.is_empty() {
        return None;
    }
    let dir = PathBuf::from(&item.install_location);
    if item.install_location.is_empty() || !dir.is_dir() {
        return None;
    }
    let app = if item.namespace.is_empty() || item.item_id.is_empty() {
        encode(&item.app_name)
    } else {
        format!(
            "{}%3A{}%3A{}",
            encode(&item.namespace),
            encode(&item.item_id),
            encode(&item.app_name)
        )
    };
    let title = match item.display_name.trim() {
        "" => item.app_name.clone(),
        name => name.to_owned(),
    };
    Some(Found {
        id: format!("epic:{}", item.app_name),
        source: FoundSource::Epic,
        title,
        dir,
        root: manifests.to_path_buf(),
        clue: Clue::Title,
        launch: None,
        launch_url: Some(format!(
            "com.epicgames.launcher://apps/{app}?action=launch&silent=true"
        )),
        candidates: Vec::new(),
    })
}

// --- game folders ------------------------------------------------------------------------------

fn folder_games(root: &Path, platform: Platform, taken: &[String], out: &mut Scan) {
    let Ok(entries) = fs::read_dir(root) else {
        out.unreachable.push(root.to_path_buf());
        return;
    };
    // A folder the user added may itself be one game.
    if has_marker(root) {
        out.games.extend(folder_game(root, root, platform));
        return;
    }
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if name.starts_with('.') || NOT_GAMES.contains(&name.as_str()) {
            continue;
        }
        let key = path_key(&dir);
        let inside = format!("{key}/");
        if taken.iter().any(|t| *t == key || t.starts_with(&inside)) {
            continue;
        }
        out.games.extend(folder_game(&dir, root, platform));
    }
}

/// A game folder: one with a program for this system, told apart by a store's files in it when
/// there are some.
fn folder_game(dir: &Path, root: &Path, platform: Platform) -> Option<Found> {
    let gog = gog_info(dir);
    let itch = itch_receipt(dir);
    let described = gog
        .as_ref()
        .and_then(|g| targets::gog_play_task(dir, &g.id))
        .filter(|t| t.exe.exists())
        .or_else(|| targets::itch_manifest(dir, platform));
    let candidates = targets::candidates(dir, platform);
    if described.is_none() && candidates.is_empty() {
        return None;
    }
    let launch = described.or_else(|| candidates.first().cloned().map(LaunchTarget::plain));
    let appid = steam_appid(dir).or_else(|| {
        launch
            .as_ref()
            .and_then(|l| l.exe.parent())
            .and_then(steam_appid)
    });
    let folder = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.display().to_string());
    let (title, clue) = match (gog, itch, appid) {
        (Some(g), ..) => (g.name.unwrap_or(folder), Clue::Gog(g.id)),
        (None, Some(i), _) => (i.title.unwrap_or(folder), Clue::Itch(i.id)),
        (None, None, Some(appid)) => (folder, Clue::SteamAppid(appid)),
        (None, None, None) => (folder, Clue::Title),
    };
    Some(Found {
        id: format!("folder:{}", dir.display()),
        source: FoundSource::Folder,
        title,
        dir: dir.to_path_buf(),
        root: root.to_path_buf(),
        clue,
        launch,
        launch_url: None,
        candidates,
    })
}

/// Files that only a game's folder has.
fn has_marker(dir: &Path) -> bool {
    dir.join("steam_appid.txt").is_file()
        || dir.join(".itch.toml").is_file()
        || dir.join(".itch").join("receipt.json.gz").is_file()
        || fs::read_dir(dir).is_ok_and(|entries| {
            entries.flatten().any(|e| {
                let name = e.file_name().to_string_lossy().to_lowercase();
                name.starts_with("goggame-") && name.ends_with(".info")
            })
        })
}

struct GogInfo {
    id: String,
    name: Option<String>,
}

/// The GOG game a folder holds, from its `goggame-<id>.info` (the base game's, when add-ons
/// have files of their own).
fn gog_info(dir: &Path) -> Option<GogInfo> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Info {
        game_id: Option<String>,
        root_game_id: Option<String>,
        name: Option<String>,
    }
    let mut infos: Vec<Info> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            name.starts_with("goggame-") && name.ends_with(".info")
        })
        .filter_map(|e| read_small(&e.path()))
        .filter_map(|t| serde_json::from_str::<Info>(&t).ok())
        .filter(|i| {
            i.game_id
                .as_deref()
                .is_some_and(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
        })
        .collect();
    infos.sort_by_key(|i| {
        (
            i.root_game_id.is_some() && i.root_game_id != i.game_id,
            i.game_id.clone(),
        )
    });
    let info = infos.into_iter().next()?;
    Some(GogInfo {
        id: info.game_id?,
        name: info
            .name
            .map(|n| n.trim().to_owned())
            .filter(|n| !n.is_empty()),
    })
}

struct ItchGame {
    id: String,
    title: Option<String>,
}

/// The itch.io game a folder holds, from the receipt the itch app leaves.
fn itch_receipt(dir: &Path) -> Option<ItchGame> {
    #[derive(Deserialize)]
    struct Receipt {
        game: Option<Game>,
    }
    #[derive(Deserialize)]
    struct Game {
        id: Option<u64>,
        title: Option<String>,
    }
    let file = fs::File::open(dir.join(".itch").join("receipt.json.gz")).ok()?;
    let mut text = String::new();
    flate2::read::GzDecoder::new(file)
        .take(MAX_FILE)
        .read_to_string(&mut text)
        .ok()?;
    let game = serde_json::from_str::<Receipt>(&text).ok()?.game?;
    Some(ItchGame {
        id: game.id?.to_string(),
        title: game
            .title
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty()),
    })
}

/// The app id in a `steam_appid.txt`.
fn steam_appid(dir: &Path) -> Option<u32> {
    read_small(&dir.join("steam_appid.txt"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
        .filter(|&id: &u32| id > 0)
}

// --- helpers -----------------------------------------------------------------------------------

/// A small text file (without a byte order mark).
fn read_small(path: &Path) -> Option<String> {
    if fs::metadata(path).ok()?.len() > MAX_FILE {
        return None;
    }
    let text = fs::read_to_string(path).ok()?;
    Some(text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned())
}

/// One folder name, nothing that leaves the folder.
fn plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':'])
}

/// Percent-encoding for a part of an address.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// A path as compared: one kind of separator, no trailing one, and on Windows without case.
pub fn path_key(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let s = s.trim_end_matches('/');
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s.to_owned()
    }
}

/// [`path_key`] after following links (`~/.steam/steam` is a link to Steam's folder).
fn real_key(path: &Path) -> String {
    path_key(&fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gamelib-scan-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn manifest(apps: &Path, appid: u32, name: &str, dir: &str, flags: u32) {
        write(
            &apps.join(format!("appmanifest_{appid}.acf")),
            &format!(
                "\"AppState\"\n{{\n\t\"appid\"\t\t\"{appid}\"\n\t\"name\"\t\t\"{name}\"\n\t\"StateFlags\"\t\t\"{flags}\"\n\t\"installdir\"\t\t\"{dir}\"\n}}\n"
            ),
        );
        fs::create_dir_all(apps.join("common").join(dir)).unwrap();
    }

    fn plan(base: &Path) -> ScanPlan {
        ScanPlan {
            steam: vec![base.join("Steam")],
            epic: Some(base.join("Manifests")),
            folders: vec![base.join("Games")],
            known: vec![base.join("Games").join("Installed by GameLib")],
            platform: Platform::Win,
        }
    }

    #[test]
    fn finds_steam_games_in_every_library() {
        let base = temp("steam");
        let steam = base.join("Steam");
        let second = base.join("SteamLibrary");
        let vdf_path = |p: &Path| p.display().to_string().replace('\\', "\\\\");
        write(
            &steam.join("steamapps").join("libraryfolders.vdf"),
            &format!(
                "\"libraryfolders\"\n{{\n\t\"contentstatsid\"\t\"-1\"\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n\t\"2\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}\n",
                vdf_path(&steam),
                vdf_path(&second),
                vdf_path(&base.join("Unplugged"))
            ),
        );
        let apps = steam.join("steamapps");
        manifest(
            &apps,
            292030,
            "The Witcher 3: Wild Hunt",
            "The Witcher 3",
            4,
        );
        // Still downloading, and an update pending on a fully installed one.
        manifest(&apps, 620, "Portal 2", "Portal 2", 1026);
        manifest(&apps, 413150, "Stardew Valley", "Stardew Valley", 6);
        // A manifest pointing outside its library is ignored.
        manifest(&apps, 1, "Bad", "..", 4);
        manifest(
            &second.join("steamapps"),
            1091500,
            "Cyberpunk 2077",
            "Cyberpunk 2077",
            4,
        );

        let found = scan(&plan(&base));
        let ids: Vec<&str> = found.games.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ids, ["steam:292030", "steam:413150", "steam:1091500"]);
        let witcher = &found.games[0];
        assert_eq!(witcher.title, "The Witcher 3: Wild Hunt");
        assert_eq!(witcher.clue, Clue::Steam(292030));
        assert_eq!(witcher.dir, apps.join("common").join("The Witcher 3"));
        assert_eq!(witcher.root, steam);
        assert_eq!(
            witcher.launch_url.as_deref(),
            Some("steam://rungameid/292030")
        );
        assert_eq!(found.games[2].root, second);
        assert!(
            found.unreachable.contains(&base.join("Unplugged")),
            "a drive that is not there"
        );
        assert!(!found.unreachable.contains(&steam) && !found.unreachable.contains(&second));
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn finds_epic_games_but_not_add_ons() {
        let base = temp("epic");
        let manifests = base.join("Manifests");
        let game_dir = base.join("Epic Games").join("Control");
        fs::create_dir_all(&game_dir).unwrap();
        let item = |name: &str, app: &str, main: &str, incomplete: bool, categories: &str| {
            write(
                &manifests.join(format!("{name}.item")),
                &serde_json::json!({
                    "bIsIncompleteInstall": incomplete,
                    "DisplayName": "Control",
                    "InstallLocation": game_dir.display().to_string(),
                    "AppName": app,
                    "MainGameAppName": main,
                    "CatalogNamespace": "calluna",
                    "CatalogItemId": "c8e4d9b0",
                    "AppCategories": serde_json::from_str::<serde_json::Value>(categories).unwrap(),
                })
                .to_string(),
            )
        };
        item(
            "A",
            "Calluna",
            "Calluna",
            false,
            r#"["public","games","applications"]"#,
        );
        item(
            "B",
            "CallunaDLC",
            "Calluna",
            false,
            r#"["public","addons"]"#,
        );
        item("C", "Half", "Half", true, r#"["games"]"#);
        item("D", "UE_5.4", "UE_5.4", false, r#"["engines"]"#);
        write(&manifests.join("broken.item"), "{ not json");

        let found = scan(&plan(&base));
        assert_eq!(found.games.len(), 1, "{:?}", found.games);
        let control = &found.games[0];
        assert_eq!(control.id, "epic:Calluna");
        assert_eq!(control.title, "Control");
        assert_eq!(control.clue, Clue::Title);
        assert_eq!(
            control.launch_url.as_deref(),
            Some(
                "com.epicgames.launcher://apps/calluna%3Ac8e4d9b0%3ACalluna?action=launch&silent=true"
            )
        );
        assert!(found.unreachable.contains(&base.join("Steam")));
        assert!(!found.unreachable.contains(&manifests));
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn finds_game_folders_and_what_they_are() {
        let base = temp("folders");
        let games = base.join("Games");
        let touch = |p: &Path| write(p, "MZ");
        // GOG: the info file names the game and what to start.
        let witcher = games.join("Witcher 3 GOG");
        write(
            &witcher.join("goggame-1207664643.info"),
            r#"{"gameId":"1207664643","rootGameId":"1207664643","name":"The Witcher 3: Wild Hunt","playTasks":[{"isPrimary":true,"type":"FileTask","path":"bin\\x64\\witcher3.exe","workingDir":"bin\\x64"}]}"#,
        );
        write(
            &witcher.join("goggame-1640424747.info"),
            r#"{"gameId":"1640424747","rootGameId":"1207664643","name":"Hearts of Stone"}"#,
        );
        touch(&witcher.join("bin").join("x64").join("witcher3.exe"));
        // itch.io: the itch app's receipt.
        let itch = games.join("itch-game");
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(br#"{"game":{"id":123456,"title":"A Short Hike"},"upload":{"id":1}}"#)
            .unwrap();
        fs::create_dir_all(itch.join(".itch")).unwrap();
        fs::write(
            itch.join(".itch").join("receipt.json.gz"),
            gz.finish().unwrap(),
        )
        .unwrap();
        touch(&itch.join("AShortHike.exe"));
        // A steam_appid.txt next to the program.
        let stardew = games.join("SV");
        touch(&stardew.join("Stardew Valley.exe"));
        write(&stardew.join("steam_appid.txt"), "413150\n");
        // Only a name to go by; tools come after the game.
        let hollow = games.join("Hollow Knight");
        touch(&hollow.join("hollow_knight.exe"));
        touch(&hollow.join("UnityCrashHandler64.exe"));
        // Not games: no program, hidden, already installed by GameLib.
        write(&games.join("Notes").join("readme.txt"), "x");
        touch(&games.join(".gamelib").join("tmp").join("x.exe"));
        touch(&games.join("Installed by GameLib").join("game.exe"));

        let found = scan(&plan(&base));
        let summary: Vec<(String, String, Clue)> = found
            .games
            .iter()
            .map(|g| {
                (
                    g.dir.file_name().unwrap().to_string_lossy().into_owned(),
                    g.title.clone(),
                    g.clue.clone(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("Hollow Knight".into(), "Hollow Knight".into(), Clue::Title),
                ("SV".into(), "SV".into(), Clue::SteamAppid(413150)),
                (
                    "Witcher 3 GOG".into(),
                    "The Witcher 3: Wild Hunt".into(),
                    Clue::Gog("1207664643".into())
                ),
                (
                    "itch-game".into(),
                    "A Short Hike".into(),
                    Clue::Itch("123456".into())
                ),
            ]
        );
        let by_dir = |name: &str| {
            found
                .games
                .iter()
                .find(|g| g.dir == games.join(name))
                .unwrap()
        };
        let w = by_dir("Witcher 3 GOG");
        assert_eq!(w.id, format!("folder:{}", witcher.display()));
        assert_eq!(w.source, FoundSource::Folder);
        assert_eq!(w.root, games);
        let launch = w.launch.as_ref().unwrap();
        assert_eq!(
            launch.exe,
            witcher.join("bin").join("x64").join("witcher3.exe")
        );
        assert_eq!(launch.workdir, Some(witcher.join("bin").join("x64")));
        assert_eq!(
            by_dir("Hollow Knight").launch.as_ref().unwrap().exe,
            hollow.join("hollow_knight.exe")
        );
        assert!(by_dir("Hollow Knight").launch_url.is_none());
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_folder_added_on_its_own_can_be_one_game() {
        let base = temp("single");
        let game = base.join("Celeste");
        write(&game.join("Celeste.exe"), "MZ");
        write(&game.join("steam_appid.txt"), "504230");
        write(&game.join("Content").join("tool.exe"), "MZ");
        let found = scan(&ScanPlan {
            steam: Vec::new(),
            epic: None,
            folders: vec![game.clone(), base.join("Missing")],
            known: Vec::new(),
            platform: Platform::Win,
        });
        assert_eq!(found.games.len(), 1);
        assert_eq!(found.games[0].clue, Clue::SteamAppid(504230));
        assert_eq!(found.games[0].dir, game);
        assert_eq!(found.unreachable, [base.join("Missing")]);
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn a_whole_drive_skips_the_systems_folders() {
        let base = temp("drive");
        let exe = |p: &Path| write(p, "MZ");
        exe(&base.join("Windows").join("explorer.exe"));
        exe(&base.join("Program Files").join("App").join("app.exe"));
        exe(&base.join("Celeste").join("Celeste.exe"));
        let found = scan(&ScanPlan {
            steam: Vec::new(),
            epic: None,
            folders: vec![base.clone()],
            known: Vec::new(),
            platform: Platform::Win,
        });
        let dirs: Vec<&Path> = found.games.iter().map(|g| g.dir.as_path()).collect();
        assert_eq!(dirs, [base.join("Celeste").as_path()]);
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn paths_compare_without_trailing_separators() {
        assert_eq!(path_key(Path::new("/a/b/")), path_key(Path::new("/a/b")));
        assert_eq!(encode("a b:c"), "a%20b%3Ac");
        assert!(plain_name("The Witcher 3") && !plain_name("..") && !plain_name("a/b"));
    }
}
