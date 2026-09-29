//! Games installed outside GameLib: where to look on this computer, which Steam game each one
//! found is, and keeping the stored list current without losing what the user chose.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};

use super::scan::{Clue, Found, ScanPlan, path_key};
use crate::Result;
use crate::db::found::{self, FoundRow};
use crate::db::stores::CONFIDENT_SQL;
use crate::downloads::sources::this_platform;
use crate::model::{FoundMatch, InstallMethod, Installed, ScanReport, Settings, Store};
use crate::stores::matching::canonical_title;

/// Where Steam and Epic Games Launcher keep what they installed.
#[derive(Debug, Clone, Default)]
pub struct Launchers {
    /// Steam installations.
    pub steam: Vec<PathBuf>,
    /// Epic Games Launcher's manifests folder.
    pub epic: Option<PathBuf>,
}

impl Launchers {
    /// The launchers installed on this computer.
    pub fn detect() -> Self {
        let mut steam: Vec<PathBuf> = Vec::new();
        #[cfg(windows)]
        {
            steam.extend(super::windows::steam_path());
            steam.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
        }
        if let Some(home) = dirs::home_dir() {
            if cfg!(target_os = "linux") {
                steam.push(home.join(".steam").join("steam"));
                steam.push(home.join(".local").join("share").join("Steam"));
                // The Flatpak build.
                steam.push(home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
            }
            if cfg!(target_os = "macos") {
                steam.push(home.join("Library/Application Support/Steam"));
            }
        }
        steam.retain(|p| p.join("steamapps").is_dir());
        Self {
            steam,
            epic: epic_manifests().filter(|p| p.is_dir()),
        }
    }
}

fn epic_manifests() -> Option<PathBuf> {
    let data = if cfg!(windows) {
        std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .or_else(|| Some(PathBuf::from(r"C:\ProgramData")))
    } else if cfg!(target_os = "macos") {
        dirs::home_dir().map(|h| h.join("Library/Application Support"))
    } else {
        None
    };
    data.map(|d| {
        d.join("Epic")
            .join("EpicGamesLauncher")
            .join("Data")
            .join("Manifests")
    })
}

/// What a scan covers: the launchers, the library folder and the user's folders. Folders of games
/// already listed (`known`) are left alone.
pub fn plan(launchers: &Launchers, settings: &Settings, known: Vec<PathBuf>) -> ScanPlan {
    let mut folders = vec![PathBuf::from(&settings.library_dir)];
    folders.extend(settings.scan_dirs.iter().map(PathBuf::from));
    ScanPlan {
        steam: launchers.steam.clone(),
        epic: launchers.epic.clone(),
        folders,
        known,
        platform: this_platform(),
    }
}

/// A found game and the Steam game it is, when that is known.
pub type Identified = (Found, Option<(u32, FoundMatch)>);

/// Which Steam game each found one is, if that can be told: by its own clue (Steam's app id, the
/// GOG or itch.io game matched to Steam, a `steam_appid.txt`), otherwise by its title when only
/// one Steam game has it. Steam's own apps missing from the catalog (tools, runtimes,
/// soundtracks) are not games and are dropped.
pub fn identify(conn: &Connection, games: Vec<Found>) -> Result<Vec<Identified>> {
    let mut titles: Option<Titles> = None;
    let mut out = Vec::with_capacity(games.len());
    for game in games {
        let known = match &game.clue {
            Clue::Steam(appid) => {
                if !found::in_catalog(conn, *appid)? {
                    continue;
                }
                Some((*appid, FoundMatch::Steam))
            }
            Clue::Gog(id) => store_match(conn, Store::Gog, id)?.map(|a| (a, FoundMatch::Gog)),
            Clue::Itch(id) => store_match(conn, Store::Itch, id)?.map(|a| (a, FoundMatch::Itch)),
            Clue::SteamAppid(appid) => {
                found::in_catalog(conn, *appid)?.then_some((*appid, FoundMatch::SteamAppid))
            }
            Clue::Title => None,
        };
        let matched = match known {
            Some(m) => Some(m),
            None => {
                let titles = match &mut titles {
                    Some(t) => t,
                    None => titles.insert(Titles::load(conn)?),
                };
                titles.only(&game.title).map(|a| (a, FoundMatch::Title))
            }
        };
        out.push((game, matched));
    }
    Ok(out)
}

/// The Steam game a store product is matched to with confidence.
fn store_match(conn: &Connection, store: Store, product_id: &str) -> Result<Option<u32>> {
    Ok(conn
        .prepare_cached(&format!(
            "SELECT m.appid FROM store_matches m
             WHERE m.store = ?1 AND m.product_id = ?2 AND {CONFIDENT_SQL}
             ORDER BY m.score DESC, m.appid LIMIT 1"
        ))?
        .query_row(params![store.as_str(), product_id], |r| r.get(0))
        .optional()?)
}

/// Steam games by title, for found games that have nothing but a name.
struct Titles {
    exact: HashMap<String, Vec<u32>>,
    /// The same without spaces, for folder names such as "HollowKnight".
    compact: HashMap<String, Vec<u32>>,
}

impl Titles {
    fn load(conn: &Connection) -> Result<Self> {
        let mut titles = Titles {
            exact: HashMap::new(),
            compact: HashMap::new(),
        };
        let mut stmt = conn.prepare("SELECT appid, name FROM games WHERE delisted = 0")?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let name: String = r.get(1)?;
            let canonical = canonical_title(&name);
            if canonical.is_empty() {
                continue;
            }
            let appid: u32 = r.get(0)?;
            titles
                .compact
                .entry(canonical.replace(' ', ""))
                .or_default()
                .push(appid);
            titles.exact.entry(canonical).or_default().push(appid);
        }
        Ok(titles)
    }

    /// The only Steam game with this title; several (a remake, a namesake) mean no answer.
    fn only(&self, title: &str) -> Option<u32> {
        let canonical = canonical_title(title);
        if canonical.is_empty() {
            return None;
        }
        let ids = match self.exact.get(&canonical) {
            Some(ids) => ids,
            None => self.compact.get(&canonical.replace(' ', ""))?,
        };
        match ids.as_slice() {
            [only] => Some(*only),
            _ => None,
        }
    }
}

/// Stores what a scan found: new games are added, known ones refreshed (keeping whether the user
/// hid one, the Steam game they chose and the program they picked) and the ones no longer there
/// removed, except where a drive could not be read.
pub fn sync(
    conn: &mut Connection,
    games: Vec<Identified>,
    unreachable: &[PathBuf],
    now: i64,
) -> Result<ScanReport> {
    let tx = conn.transaction()?;
    let mut report = ScanReport::default();
    let mut seen = HashSet::new();
    for (game, matched) in &games {
        if !seen.insert(game.id.clone()) {
            continue;
        }
        let old = found::get(&tx, &game.id)?;
        found::upsert(&tx, &merge(game, *matched, old.as_ref(), now))?;
        report.found += 1;
        report.added += u32::from(old.is_none());
    }
    let unreachable: HashSet<String> = unreachable.iter().map(|p| path_key(p)).collect();
    for row in found::all(&tx)? {
        if !seen.contains(&row.id) && !unreachable.contains(&path_key(Path::new(&row.root))) {
            found::delete(&tx, &row.id)?;
            report.removed += 1;
        }
    }
    tx.commit()?;
    Ok(report)
}

fn merge(
    game: &Found,
    matched: Option<(u32, FoundMatch)>,
    old: Option<&FoundRow>,
    now: i64,
) -> FoundRow {
    let (appid, matched_by) = match old {
        Some(o) if o.matched_by == Some(FoundMatch::Manual) => (o.appid, o.matched_by),
        _ => (matched.map(|m| m.0), matched.map(|m| m.1)),
    };
    let chosen =
        old.filter(|o| o.exe_by_user && o.exe.as_deref().is_some_and(|e| Path::new(e).exists()));
    let launch = game.launch.as_ref();
    let (exe, args, workdir, exe_by_user) = match chosen {
        Some(o) => (o.exe.clone(), o.args.clone(), o.workdir.clone(), true),
        None => (
            launch.map(|l| display(&l.exe)),
            launch.map(|l| l.args.clone()).unwrap_or_default(),
            launch.and_then(|l| l.workdir.as_deref()).map(display),
            false,
        ),
    };
    FoundRow {
        id: game.id.clone(),
        source: game.source,
        title: game.title.clone(),
        dir: display(&game.dir),
        root: display(&game.root),
        appid,
        matched_by,
        exe,
        args,
        workdir,
        exe_by_user,
        launch_url: game.launch_url.clone(),
        candidates: game.candidates.iter().map(|c| display(c)).collect(),
        hidden: old.is_some_and(|o| o.hidden),
        found_at: old.map_or(now, |o| o.found_at),
        seen_at: now,
    }
}

/// Found games to show, by title: not hidden, their folder still there (a drive that is not
/// connected hides its games).
pub fn listed(conn: &Connection) -> Result<Vec<Installed>> {
    Ok(found::listed(conn, false)?
        .into_iter()
        .filter(|(row, _)| Path::new(&row.dir).is_dir())
        .map(|(row, name)| installed(row, name))
        .collect())
}

/// The games the user took off the list.
pub fn hidden(conn: &Connection) -> Result<Vec<Installed>> {
    Ok(found::listed(conn, true)?
        .into_iter()
        .map(|(row, name)| installed(row, name))
        .collect())
}

/// A found game as an installed one; it is called by its Steam game's name when it has one.
pub fn installed(row: FoundRow, steam_name: Option<String>) -> Installed {
    Installed {
        store: Store::Local,
        product_id: row.id,
        appid: row.appid,
        title: steam_name.unwrap_or(row.title),
        dir: Some(row.dir),
        exe: row.exe,
        args: row.args,
        workdir: row.workdir,
        method: InstallMethod::Found,
        candidates: row.candidates,
        option_label: None,
        installed_at: row.found_at,
        external: true,
        steam_header: None,
        source: Some(row.source),
        launch_url: row.launch_url,
        matched_by: row.matched_by,
    }
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::install::scan::Clue;
    use crate::model::FoundSource;

    fn db() -> Db {
        let db = Db::open_in_memory().unwrap();
        for (appid, name) in [
            (292030, "The Witcher 3: Wild Hunt"),
            (367520, "Hollow Knight"),
            (413150, "Stardew Valley"),
            (1, "Doom"),
            (2, "DOOM"),
        ] {
            db.conn()
                .execute(
                    "INSERT INTO games(appid, name, search_name, first_seen_at, synced_at)
                     VALUES (?1, ?2, lower(?2), 0, 0)",
                    params![appid, name],
                )
                .unwrap();
        }
        db.conn()
            .execute(
                "INSERT INTO store_matches(store, product_id, appid, method, score, state, updated_at)
                 VALUES ('gog', '1207664643', 292030, 'gamesdb', 1.0, 'auto', 0),
                        ('gog', '999', 1, 'title', 0.7, 'auto', 0)",
                [],
            )
            .unwrap();
        db
    }

    fn game(id: &str, title: &str, clue: Clue) -> Found {
        Found {
            id: id.into(),
            source: FoundSource::Folder,
            title: title.into(),
            dir: PathBuf::from("/games").join(title),
            root: PathBuf::from("/games"),
            clue,
            launch: None,
            launch_url: None,
            candidates: Vec::new(),
        }
    }

    #[test]
    fn tells_which_steam_game_it_is() {
        let db = db();
        let games = vec![
            game("steam:292030", "The Witcher 3", Clue::Steam(292030)),
            // A Steam tool, not in the catalog.
            game(
                "steam:228980",
                "Steamworks Common Redistributables",
                Clue::Steam(228980),
            ),
            game("gog", "The Witcher 3 GOG", Clue::Gog("1207664643".into())),
            // A weak store match is not taken; the title decides.
            game("weak", "Stardew Valley", Clue::Gog("999".into())),
            game("appid", "SV", Clue::SteamAppid(413150)),
            game("camel", "HollowKnight", Clue::Title),
            game("two", "Doom", Clue::Title),
            game("none", "Something Else", Clue::Title),
        ];
        let found = identify(db.conn(), games).unwrap();
        let got: Vec<(&str, Option<(u32, FoundMatch)>)> =
            found.iter().map(|(g, m)| (g.id.as_str(), *m)).collect();
        assert_eq!(
            got,
            [
                ("steam:292030", Some((292030, FoundMatch::Steam))),
                ("gog", Some((292030, FoundMatch::Gog))),
                ("weak", Some((413150, FoundMatch::Title))),
                ("appid", Some((413150, FoundMatch::SteamAppid))),
                ("camel", Some((367520, FoundMatch::Title))),
                ("two", None),
                ("none", None),
            ]
        );
    }

    #[test]
    fn scans_keep_what_the_user_chose() {
        let mut db = db();
        let base = std::env::temp_dir().join(format!("gamelib-found-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let chosen = base.join("chosen.exe");
        std::fs::write(&chosen, b"MZ").unwrap();

        let mut first = game("folder:/games/Hollow", "Hollow", Clue::Title);
        first.launch = Some(super::super::targets::LaunchTarget::plain(PathBuf::from(
            "/games/Hollow/hollow.exe",
        )));
        let other = game("folder:/games/Other", "Other", Clue::Title);
        let unplugged = Found {
            root: PathBuf::from("/unplugged"),
            ..game("folder:/unplugged/Old", "Old", Clue::Title)
        };
        let report = sync(
            db.conn_mut(),
            vec![
                (first.clone(), None),
                (other.clone(), Some((1, FoundMatch::Title))),
                (unplugged, None),
            ],
            &[],
            100,
        )
        .unwrap();
        assert_eq!(
            report,
            ScanReport {
                found: 3,
                added: 3,
                removed: 0
            }
        );

        // The user hides one, ties one to a Steam game by hand and picks what to start.
        let conn = db.conn();
        found::set_hidden(conn, "folder:/games/Other", true).unwrap();
        found::set_match(conn, "folder:/games/Hollow", Some(367520)).unwrap();
        found::set_launch(
            conn,
            "folder:/games/Hollow",
            &chosen.display().to_string(),
            None,
        )
        .unwrap();

        // The next scan finds the first two again (the second now with another guess), and the
        // unplugged drive's game is kept while its folder cannot be read.
        let report = sync(
            db.conn_mut(),
            vec![(first, None), (other, Some((2, FoundMatch::Title)))],
            &[PathBuf::from("/unplugged/")],
            200,
        )
        .unwrap();
        assert_eq!(report.removed, 0);
        let hollow = found::get(db.conn(), "folder:/games/Hollow")
            .unwrap()
            .unwrap();
        assert_eq!(
            (hollow.appid, hollow.matched_by),
            (Some(367520), Some(FoundMatch::Manual))
        );
        assert_eq!(
            hollow.exe.as_deref(),
            Some(chosen.display().to_string().as_str())
        );
        assert!(hollow.exe_by_user);
        assert_eq!((hollow.found_at, hollow.seen_at), (100, 200));
        let other = found::get(db.conn(), "folder:/games/Other")
            .unwrap()
            .unwrap();
        assert!(other.hidden);
        assert_eq!(other.appid, Some(2), "automatic matches follow the scan");
        assert!(
            found::get(db.conn(), "folder:/unplugged/Old")
                .unwrap()
                .is_some()
        );

        // Once the drive is readable and the game gone, it goes; a picked program that is gone
        // gives way to the scan's.
        std::fs::remove_file(&chosen).unwrap();
        let report = sync(
            db.conn_mut(),
            vec![(game("folder:/games/Hollow", "Hollow", Clue::Title), None)],
            &[],
            300,
        )
        .unwrap();
        assert_eq!(report.removed, 2);
        let hollow = found::get(db.conn(), "folder:/games/Hollow")
            .unwrap()
            .unwrap();
        assert_eq!(hollow.exe, None);
        assert!(!hollow.exe_by_user);
        assert_eq!(hollow.appid, Some(367520), "the user's match stays");
        std::fs::remove_dir_all(&base).unwrap();
    }
}
