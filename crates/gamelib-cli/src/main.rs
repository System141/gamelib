//! Headless tool for the GameLib catalog: sync, query and inspect without the desktop app, or
//! serve the catalog to the browser preview.
//!
//! Run `gamelib-cli help` for usage.

mod serve;

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use gamelib_core::db::Db;
use gamelib_core::db::read::{get_game, list_tags, query_games, status};
use gamelib_core::db::write::{get_meta, meta_keys};
use gamelib_core::links::{SiteRegistry, resolve, validate};
use gamelib_core::model::{
    DeckFilter, GameDetail, GameMedia, GameQuery, Platform, SortKey, SyncProgress, TagInfo,
};
use gamelib_core::new_releases::{NewReleasesOptions, fetch_new_releases};
use gamelib_core::steam::SteamClient;
use gamelib_core::sync::{SyncOptions, run_sync};
use gamelib_core::{Error, Result, unix_now};
use serde::Serialize;

const USAGE: &str = "\
gamelib-cli [--db PATH] <command> [options]

Commands:
  serve          Serve the catalog to the browser preview (`pnpm dev`) on 127.0.0.1
                 [--port N] (default 1430)
  sync           Download the whole catalog
                 [--max-pages N] [--no-prune] [--fresh] [--no-featured] [--delay-ms N]
  new-releases   Fetch games released since the last check
                 [--days N] [--max-pages N]
  stats          Catalog summary
  query          Search the local catalog
                 [--search TEXT] [--tag ID]... [--sort relevance|popular|rating|newest|oldest|name]
                 [--platform win|mac|linux]... [--deck playable|verified] [--free] [--min-score 1-9]
                 [--within-days N] [--links] [--adult] [--limit N] [--offset N]
  game APPID     Show one stored game as JSON
  media APPID    Fetch the Turkish description and screenshots of a game
  check-link URL Follow a link's redirects without downloading it
  export-fixture Write UI mock data: --out PATH [--limit N] [--media N]

The database defaults to the desktop app's own file, so both see the same catalog:";

/// The desktop app's bundle identifier (`identifier` in src-tauri/tauri.conf.json). Tauri keeps
/// the app's local data under the OS local data directory in a folder with this name.
const APP_IDENTIFIER: &str = "com.gamelib.desktop";

/// The desktop app's database: Tauri's `app_local_data_dir()` plus `gamelib.db`
/// (e.g. `%LOCALAPPDATA%\com.gamelib.desktop\gamelib.db` on Windows).
fn default_db_path() -> PathBuf {
    dirs::data_local_dir()
        .map(|dir| dir.join(APP_IDENTIFIER).join("gamelib.db"))
        .unwrap_or_else(|| PathBuf::from("gamelib.db"))
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let db_path = take_value(&mut args, "--db")
        .map(PathBuf::from)
        .unwrap_or_else(default_db_path);
    if args.is_empty() || matches!(args[0].as_str(), "help" | "-h" | "--help") {
        println!("{USAGE}\n  {}", default_db_path().display());
        return ExitCode::SUCCESS;
    }
    let command = args.remove(0);
    let result = match command.as_str() {
        "serve" => cmd_serve(&db_path, args),
        "sync" => cmd_sync(&db_path, args),
        "new-releases" => cmd_new_releases(&db_path, args),
        "stats" => cmd_stats(&db_path),
        "query" => cmd_query(&db_path, args),
        "game" => cmd_game(&db_path, args),
        "media" => cmd_media(args),
        "check-link" => cmd_check_link(args),
        "export-fixture" => cmd_export_fixture(&db_path, args),
        other => Err(Error::Other(format!(
            "unknown command `{other}`\n\n{USAGE}"
        ))),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

// --- argument helpers -------------------------------------------------------

fn take_flag(args: &mut Vec<String>, flag: &str) -> bool {
    match args.iter().position(|a| a == flag) {
        Some(i) => {
            args.remove(i);
            true
        }
        None => false,
    }
}

fn take_value(args: &mut Vec<String>, flag: &str) -> Option<String> {
    let i = args.iter().position(|a| a == flag)?;
    args.remove(i);
    (i < args.len()).then(|| args.remove(i))
}

fn take_all(args: &mut Vec<String>, flag: &str) -> Vec<String> {
    let mut values = Vec::new();
    while let Some(v) = take_value(args, flag) {
        values.push(v);
    }
    values
}

fn parse_num<T: std::str::FromStr>(value: Option<String>, flag: &str) -> Result<Option<T>> {
    value
        .map(|v| {
            v.parse::<T>()
                .map_err(|_| Error::Other(format!("{flag} expects a number, got `{v}`")))
        })
        .transpose()
}

fn ensure_empty(args: &[String]) -> Result<()> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "unexpected arguments: {}",
            args.join(" ")
        )))
    }
}

fn print_progress(p: &SyncProgress) {
    let phase = format!("{:?}", p.phase).to_lowercase();
    if p.total > 0 {
        eprintln!(
            "[{phase}] page {}/{} — {}/{} games",
            p.page, p.pages, p.fetched, p.total
        );
    } else {
        eprintln!("[{phase}] page {} — {} games", p.page, p.fetched);
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

// --- commands ---------------------------------------------------------------

fn cmd_serve(db_path: &Path, mut args: Vec<String>) -> Result<()> {
    let port = parse_num(take_value(&mut args, "--port"), "--port")?.unwrap_or(serve::DEFAULT_PORT);
    ensure_empty(&args)?;
    serve::run(db_path, port)
}

fn cmd_sync(db_path: &Path, mut args: Vec<String>) -> Result<()> {
    let mut opts = SyncOptions {
        max_pages: parse_num(take_value(&mut args, "--max-pages"), "--max-pages")?,
        prune: !take_flag(&mut args, "--no-prune"),
        fresh: take_flag(&mut args, "--fresh"),
        featured: !take_flag(&mut args, "--no-featured"),
        ..SyncOptions::default()
    };
    if let Some(ms) = parse_num::<u64>(take_value(&mut args, "--delay-ms"), "--delay-ms")? {
        opts.delay = Duration::from_millis(ms);
    }
    ensure_empty(&args)?;
    let mut db = Db::open(db_path)?;
    let client = SteamClient::new()?;
    let report = run_sync(
        &mut db,
        &client,
        &opts,
        &AtomicBool::new(false),
        &mut print_progress,
    )?;
    print_json(&report)
}

fn cmd_new_releases(db_path: &Path, mut args: Vec<String>) -> Result<()> {
    let mut opts = NewReleasesOptions {
        days_override: parse_num(take_value(&mut args, "--days"), "--days")?,
        ..Default::default()
    };
    if let Some(max) = parse_num(take_value(&mut args, "--max-pages"), "--max-pages")? {
        opts.max_pages = max;
    }
    ensure_empty(&args)?;
    let mut db = Db::open(db_path)?;
    let client = SteamClient::new()?;
    let report = fetch_new_releases(
        &mut db,
        &client,
        &opts,
        &AtomicBool::new(false),
        &mut print_progress,
    )?;
    print_json(&report)
}

fn cmd_stats(db_path: &Path) -> Result<()> {
    let mut db = Db::open(db_path)?;
    let st = status(&mut db)?;
    let conn = db.conn();
    let count = |sql: &str| -> Result<i64> { Ok(conn.query_row(sql, [], |r| r.get(0))?) };
    let file_size = std::fs::metadata(db_path).map(|m| m.len()).unwrap_or(0);
    let meta = |key| {
        get_meta(conn, key)
            .ok()
            .flatten()
            .unwrap_or_else(|| "-".into())
    };
    println!(
        "database        {} ({:.1} MB)",
        db_path.display(),
        file_size as f64 / 1_048_576.0
    );
    println!("games (listed)  {}", st.game_count);
    println!(
        "  adult         {}",
        count("SELECT COUNT(*) FROM games WHERE adult = 1 AND delisted = 0")?
    );
    println!(
        "  delisted      {}",
        count("SELECT COUNT(*) FROM games WHERE delisted = 1")?
    );
    println!(
        "  no capsule    {}",
        count("SELECT COUNT(*) FROM games WHERE img_capsule IS NULL")?
    );
    println!(
        "  no header     {}",
        count("SELECT COUNT(*) FROM games WHERE img_header IS NULL")?
    );
    println!(
        "  no release    {}",
        count("SELECT COUNT(*) FROM games WHERE release_date IS NULL")?
    );
    println!(
        "  no desc       {}",
        count("SELECT COUNT(*) FROM games WHERE short_description IS NULL")?
    );
    println!(
        "  free          {}",
        count("SELECT COUNT(*) FROM games WHERE is_free = 1")?
    );
    println!("tags (in use)   {}", st.tag_count);
    println!(
        "game_tags rows  {}",
        count("SELECT COUNT(*) FROM game_tags")?
    );
    println!("linked games    {}", st.linked_game_count);
    println!("last sync       {}", meta(meta_keys::LAST_SYNC_AT));
    println!("watermark       {}", meta(meta_keys::RELEASE_WATERMARK));
    println!("new releases at {}", meta(meta_keys::LAST_NEW_RELEASES_AT));
    println!("resumable       {}", st.resumable);
    Ok(())
}

fn cmd_query(db_path: &Path, mut args: Vec<String>) -> Result<()> {
    let sort = match take_value(&mut args, "--sort").as_deref() {
        None => None,
        Some("relevance") => Some(SortKey::Relevance),
        Some("popular") => Some(SortKey::Popular),
        Some("rating") => Some(SortKey::Rating),
        Some("newest") => Some(SortKey::Newest),
        Some("oldest") => Some(SortKey::Oldest),
        Some("name") => Some(SortKey::Name),
        Some(other) => return Err(Error::Other(format!("unknown sort `{other}`"))),
    };
    let search = take_value(&mut args, "--search");
    let platforms = take_all(&mut args, "--platform")
        .iter()
        .map(|p| Platform::parse(p).ok_or_else(|| Error::Other(format!("unknown platform `{p}`"))))
        .collect::<Result<Vec<_>>>()?;
    let deck = match take_value(&mut args, "--deck").as_deref() {
        None => None,
        Some("playable") => Some(DeckFilter::Playable),
        Some("verified") => Some(DeckFilter::Verified),
        Some(other) => return Err(Error::Other(format!("unknown deck filter `{other}`"))),
    };
    let tags = take_all(&mut args, "--tag")
        .into_iter()
        .map(|t| parse_num::<u32>(Some(t), "--tag").map(Option::unwrap_or_default))
        .collect::<Result<Vec<_>>>()?;
    let query = GameQuery {
        sort: sort.unwrap_or(if search.is_some() {
            SortKey::Relevance
        } else {
            SortKey::Popular
        }),
        search,
        tags,
        platforms,
        deck,
        free_only: take_flag(&mut args, "--free"),
        min_review_score: parse_num(take_value(&mut args, "--min-score"), "--min-score")?,
        show_adult: take_flag(&mut args, "--adult"),
        released_within_days: parse_num(take_value(&mut args, "--within-days"), "--within-days")?,
        has_links: take_flag(&mut args, "--links"),
        offset: parse_num(take_value(&mut args, "--offset"), "--offset")?.unwrap_or(0),
        limit: parse_num(take_value(&mut args, "--limit"), "--limit")?.unwrap_or(15),
    };
    ensure_empty(&args)?;
    let mut db = Db::open(db_path)?;
    let started = Instant::now();
    let page = query_games(&mut db, &query, unix_now())?;
    let first_ms = started.elapsed().as_secs_f64() * 1000.0;
    let again = Instant::now();
    query_games(&mut db, &query, unix_now())?;
    let cached_ms = again.elapsed().as_secs_f64() * 1000.0;
    println!(
        "{} results  ({first_ms:.1} ms, repeated {cached_ms:.1} ms)",
        page.total
    );
    for g in &page.items {
        let year = g
            .release_date
            .map(|t| 1970 + t / 31_556_952)
            .map(|y| y.to_string())
            .unwrap_or_else(|| "----".into());
        let price = if g.is_free {
            "Free".to_owned()
        } else {
            g.price.clone().unwrap_or_else(|| "-".into())
        };
        println!(
            "{:>8}  {:<44}  {year}  {:>9} rev {:>3}%  {:>9}  art:{}",
            g.appid,
            truncate(&g.name, 44),
            g.review_count,
            g.review_pct,
            price,
            if g.capsule.is_some() {
                "capsule"
            } else if g.header.is_some() {
                "header"
            } else {
                "none"
            }
        );
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    }
}

fn cmd_game(db_path: &Path, mut args: Vec<String>) -> Result<()> {
    let appid: u32 =
        parse_num(args.pop(), "APPID")?.ok_or_else(|| Error::Other("usage: game APPID".into()))?;
    ensure_empty(&args)?;
    let db = Db::open(db_path)?;
    match get_game(db.conn(), appid)? {
        Some(game) => print_json(&game),
        None => Err(Error::NotFound),
    }
}

fn cmd_media(mut args: Vec<String>) -> Result<()> {
    let appid: u32 =
        parse_num(args.pop(), "APPID")?.ok_or_else(|| Error::Other("usage: media APPID".into()))?;
    ensure_empty(&args)?;
    let media = SteamClient::new()?.fetch_media(appid, &AtomicBool::new(false))?;
    print_json(&media)
}

fn cmd_check_link(mut args: Vec<String>) -> Result<()> {
    let input = args
        .pop()
        .ok_or_else(|| Error::Other("usage: check-link URL".into()))?;
    ensure_empty(&args)?;
    let url = validate::parse_link_url(&input)?;
    let sites = SiteRegistry::with_builtin_sites();
    let handler = sites.detect(&url);
    let url = handler.normalize(url);
    eprintln!("site: {} — {url}", handler.info().id);
    let check = handler.resolve(&url, &resolve::client()?);
    print_json(&check)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    generated_at: i64,
    games: Vec<GameDetail>,
    tags: Vec<TagInfo>,
    media: BTreeMap<u32, GameMedia>,
}

fn cmd_export_fixture(db_path: &Path, mut args: Vec<String>) -> Result<()> {
    let out = take_value(&mut args, "--out")
        .map(PathBuf::from)
        .ok_or_else(|| Error::Other("--out PATH is required".into()))?;
    let limit: u32 = parse_num(take_value(&mut args, "--limit"), "--limit")?.unwrap_or(240);
    let media_count: usize = parse_num(take_value(&mut args, "--media"), "--media")?.unwrap_or(12);
    ensure_empty(&args)?;

    let db = Db::open(db_path)?;
    let conn = db.conn();
    let now = unix_now();
    let ids = |sql: &str, params: &[&dyn rusqlite::ToSql]| -> Result<Vec<u32>> {
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt
            .query_map(params, |r| r.get(0))?
            .collect::<std::result::Result<Vec<u32>, _>>()?;
        Ok(rows)
    };
    let visible = "delisted = 0 AND adult = 0";
    let popular = ids(
        &format!("SELECT appid FROM games WHERE {visible} ORDER BY review_count DESC LIMIT ?1"),
        &[&limit],
    )?;
    let recent = ids(
        &format!(
            "SELECT appid FROM games WHERE {visible} AND release_date >= ?1 ORDER BY review_count DESC, release_date DESC LIMIT 24"
        ),
        &[&(now - 7 * 86_400)],
    )?;
    let no_capsule = ids(
        &format!(
            "SELECT appid FROM games WHERE {visible} AND img_capsule IS NULL ORDER BY review_count DESC LIMIT 8"
        ),
        &[],
    )?;

    let mut seen = HashSet::new();
    let mut games = Vec::new();
    for appid in popular.iter().chain(&recent).chain(&no_capsule) {
        if seen.insert(*appid)
            && let Some(game) = get_game(conn, *appid)?
        {
            games.push(game);
        }
    }

    let names: BTreeMap<u32, String> = list_tags(conn)?
        .into_iter()
        .map(|t| (t.tagid, t.name))
        .collect();
    let mut counts: BTreeMap<u32, u32> = BTreeMap::new();
    for g in &games {
        for t in &g.tags {
            *counts.entry(*t).or_default() += 1;
        }
    }
    let mut tags: Vec<TagInfo> = counts
        .into_iter()
        .filter_map(|(tagid, game_count)| {
            names.get(&tagid).map(|name| TagInfo {
                tagid,
                name: name.clone(),
                game_count,
            })
        })
        .collect();
    tags.sort_by(|a, b| {
        b.game_count
            .cmp(&a.game_count)
            .then_with(|| a.name.cmp(&b.name))
    });

    let client = SteamClient::new()?;
    let mut media = BTreeMap::new();
    for g in games.iter().take(media_count) {
        media.insert(
            g.card.appid,
            client.fetch_media(g.card.appid, &AtomicBool::new(false))?,
        );
        std::thread::sleep(Duration::from_millis(300));
    }

    let fixture = Fixture {
        generated_at: now,
        games,
        tags,
        media,
    };
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| Error::Other(e.to_string()))?;
    }
    std::fs::write(&out, serde_json::to_string(&fixture)?)
        .map_err(|e| Error::Other(e.to_string()))?;
    eprintln!(
        "wrote {} games, {} tags, media for {} games to {}",
        fixture.games.len(),
        fixture.tags.len(),
        fixture.media.len(),
        out.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default database must stay the desktop app's: same identifier, no directory override.
    #[test]
    fn default_db_is_the_desktop_apps() {
        let conf = include_str!("../../../src-tauri/tauri.conf.json");
        let conf: serde_json::Value = serde_json::from_str(conf).unwrap();
        assert_eq!(conf["identifier"], APP_IDENTIFIER);
        assert!(conf["app"].get("appDirectoriesOverride").is_none());
        let path = default_db_path();
        assert!(path.ends_with(Path::new(APP_IDENTIFIER).join("gamelib.db")));
    }
}
