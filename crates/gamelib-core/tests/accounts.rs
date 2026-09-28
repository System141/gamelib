//! Store accounts and libraries against fake GOG and itch.io services.

mod common;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use common::http::{Request, Response, TestServer};
use common::item;
use gamelib_core::app::{App, EVENT_FINISHED, EventSink, JobOptions};
use gamelib_core::db::Db;
use gamelib_core::db::write::upsert_games;
use gamelib_core::model::{SettingsPatch, Store};
use gamelib_core::record::GameRecord;
use gamelib_core::secrets::{GogTokens, SecretStore};
use gamelib_core::stores::{StoreEndpoints, StoreSyncOptions};
use gamelib_core::{Error, unix_now};
use serde_json::{Value, json};

const GOG_ACCESS: &str = "gog-access-1";
const GOG_ACCESS_REFRESHED: &str = "gog-access-2";
const ITCH_KEY: &str = "itch-key-123";

struct Fake {
    server: TestServer,
    refreshes: Arc<AtomicU32>,
}

/// GOG auth/embed/product APIs and itch.io's API, checking credentials like the real ones.
fn fake() -> Fake {
    fake_with(Duration::ZERO)
}

/// Like [`fake`], with a GOG catalog that takes `catalog_delay` to answer.
fn fake_with(catalog_delay: Duration) -> Fake {
    let refreshes = Arc::new(AtomicU32::new(0));
    let counter = refreshes.clone();
    let server = TestServer::start(move |req: &Request| {
        let bearer = req.header("authorization").unwrap_or_default().to_owned();
        let gog_ok = bearer == format!("Bearer {GOG_ACCESS}")
            || bearer == format!("Bearer {GOG_ACCESS_REFRESHED}");
        match req.path.as_str() {
            "/token" => {
                assert_eq!(req.param("client_id"), Some("46899977096215655"));
                match (req.param("grant_type"), req.param("code"), req.param("refresh_token")) {
                    (Some("authorization_code"), Some("good-code"), _) => {
                        Response::json(tokens(GOG_ACCESS, "refresh-1"))
                    }
                    (Some("refresh_token"), _, Some("refresh-1")) => {
                        counter.fetch_add(1, Ordering::SeqCst);
                        Response::json(tokens(GOG_ACCESS_REFRESHED, "refresh-2"))
                    }
                    _ => Response::status(400).with_body(r#"{"error":"invalid_grant"}"#),
                }
            }
            "/userData.json" if gog_ok => Response::json(r#"{"username":"tester","isLoggedIn":true}"#),
            "/user/data/games" if gog_ok => Response::json(r#"{"owned":[1207664643, 1207665503]}"#),
            "/userData.json" | "/user/data/games" => {
                Response::status(302).with_header("Location", "https://login.gog.com/")
            }
            "/products/1207665503" => Response::json(
                json!({"id": 1207665503u64, "title": "Terraria", "slug": "terraria", "game_type": "game",
                       "release_date": "2011-05-16T00:00:00+0300",
                       "content_system_compatibility": {"windows": true, "osx": false, "linux": true},
                       "links": {"product_card": "https://www.gog.com/game/terraria"}})
                .to_string(),
            ),
            "/v1/catalog" => {
                std::thread::sleep(catalog_delay);
                Response::json(
                json!({"productCount": 1, "products": [{
                    "id": "1207664643", "title": "The Witcher 3: Wild Hunt - Complete Edition", "productType": "game",
                    "releaseDate": "2015.05.19", "developers": ["CD PROJEKT RED"], "publishers": ["CD PROJEKT RED"],
                    "operatingSystems": ["windows"], "storeLink": "https://www.gog.com/en/game/the_witcher_3"}]})
                .to_string(),
                )
            }
            p if p.starts_with("/platforms/") => Response::status(404),
            // itch.io
            "/profile" if req.header("authorization") == Some(ITCH_KEY) => {
                Response::json(r#"{"user":{"id":42,"username":"player","display_name":"Player One"}}"#)
            }
            "/profile/owned-keys" if req.header("authorization") == Some(ITCH_KEY) => {
                let keys = if req.param("page") == Some("1") {
                    json!([
                        {"id": 900, "game_id": 11, "game": {"id": 11, "title": "Celeste", "classification": "game",
                         "min_price": 1999, "traits": ["p_windows"], "url": "https://mattmakesgames.itch.io/celeste",
                         "published_at": "2018-01-25 00:00:00", "user": {"username": "mattmakesgames", "display_name": "Maddy Makes Games"}}},
                        {"id": 901, "game_id": 12, "game": {"id": 12, "title": "Obscure Jam Game", "classification": "game",
                         "min_price": 0, "user": {"username": "someone"}}}
                    ])
                } else {
                    json!([])
                };
                Response::json(json!({"owned_keys": keys}).to_string())
            }
            "/search/games" if req.header("authorization") == Some(ITCH_KEY) => Response::json(
                json!({"games": [
                    {"id": 13, "title": "Celeste Fan Game", "classification": "game", "min_price": 0, "user": {"username": "fan"}},
                    {"id": 11, "title": "Celeste", "classification": "game", "min_price": 1999, "traits": ["p_windows"],
                     "published_at": "2018-01-25 00:00:00", "user": {"username": "mattmakesgames", "display_name": "Maddy Makes Games"}}
                ]})
                .to_string(),
            ),
            "/profile" | "/profile/owned-keys" | "/search/games" => {
                Response::status(401).with_body(r#"{"errors":["invalid key"]}"#)
            }
            _ => Response::status(404),
        }
    });
    Fake { server, refreshes }
}

fn tokens(access: &str, refresh: &str) -> String {
    json!({"expires_in": 3600, "token_type": "bearer", "access_token": access,
           "user_id": "4812", "refresh_token": refresh, "session_id": "s"})
    .to_string()
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "gamelib-accounts-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct Finished {
    events: Mutex<Vec<Value>>,
    ready: Condvar,
}

impl EventSink for Finished {
    fn emit(&self, event: &str, payload: Value) {
        if event == EVENT_FINISHED {
            self.events.lock().unwrap().push(payload);
            self.ready.notify_all();
        }
    }
}

impl Finished {
    fn wait(&self, n: usize) -> Value {
        let guard = self.events.lock().unwrap();
        let (guard, timeout) = self
            .ready
            .wait_timeout_while(guard, Duration::from_secs(20), |e| e.len() < n)
            .unwrap();
        assert!(!timeout.timed_out(), "job did not finish");
        guard[n - 1].clone()
    }
}

fn steam_game(appid: u32, name: &str, company: &str, release: i64) -> GameRecord {
    let mut rec = GameRecord::from_item(&item(appid, name, release, 1000, &[])).unwrap();
    rec.developers = json!([company]).to_string();
    rec.publishers = json!([company]).to_string();
    rec
}

fn app(dir: &TempDir, base: &str, sink: Arc<Finished>) -> App {
    let path = dir.0.join("gamelib.db");
    let mut db = Db::open(&path).unwrap();
    upsert_games(
        db.conn_mut(),
        &[
            steam_game(
                292030,
                "The Witcher 3: Wild Hunt",
                "CD PROJEKT RED",
                1_431_900_000,
            ),
            steam_game(105600, "Terraria", "Re-Logic", 1_305_500_000),
            steam_game(504230, "Celeste", "Maddy Makes Games Inc.", 1_516_860_000),
        ],
        unix_now(),
    )
    .unwrap();
    drop(db);
    let options = JobOptions {
        stores: StoreSyncOptions {
            endpoints: StoreEndpoints::all_at(base),
            catalog_delay: Duration::ZERO,
            gamesdb_delay: Duration::ZERO,
            ..Default::default()
        },
        ..Default::default()
    };
    App::with_options(path, sink, options).unwrap()
}

#[test]
fn gog_sign_in_reads_the_library() {
    let fake = fake();
    let dir = TempDir::new("gog");
    let sink = Arc::new(Finished::default());
    let app = app(&dir, &fake.server.base, sink.clone());

    assert!(app.accounts().unwrap().gog.is_none());
    assert!(
        app.gog_login_url()
            .contains("/auth?client_id=46899977096215655&redirect_uri=")
    );
    assert!(matches!(
        app.gog_login_with_code("nonsense!"),
        Err(Error::Invalid("gog_code"))
    ));
    assert!(matches!(
        app.gog_login_with_code(
            "https://embed.gog.com/on_login_success?origin=client&code=bad-code"
        ),
        Err(Error::Invalid("gog_code"))
    ));

    // Signing in starts a library job: owned games, one of them unknown to the catalog.
    let accounts = app
        .gog_login_with_code("https://embed.gog.com/on_login_success?origin=client&code=good-code")
        .unwrap();
    assert_eq!(accounts.gog.unwrap().username, "tester");
    let finished = sink.wait(1);
    assert_eq!(finished["kind"], "library");
    assert_eq!(finished["outcome"], "completed", "{finished}");
    assert_eq!(finished["library"]["gogOwned"], 2);

    let library = app.library(None).unwrap();
    let titles: Vec<&str> = library.iter().map(|i| i.title.as_str()).collect();
    assert_eq!(
        titles,
        ["Terraria"],
        "the Witcher product was never listed, so only the fetched one is known"
    );
    assert_eq!(library[0].appid, Some(105600), "matched to Steam by title");
    assert!(library[0].steam_header.is_some());

    // The credentials are not in the database or the UI data, and are private on disk.
    let raw = std::fs::read(dir.0.join("secrets.bin")).unwrap();
    assert!(!raw.is_empty());
    let db_bytes = std::fs::read(dir.0.join("gamelib.db")).unwrap();
    assert!(!String::from_utf8_lossy(&db_bytes).contains(GOG_ACCESS));

    // A store sync lists the catalog; the owned Witcher now shows up too.
    app.start_store_sync().unwrap();
    let finished = sink.wait(2);
    assert_eq!(finished["stores"]["library"]["gogOwned"], 2, "{finished}");
    let library = app.library(Some(Store::Gog)).unwrap();
    assert_eq!(library.len(), 2);
    let witcher = library
        .iter()
        .find(|i| i.product_id == "1207664643")
        .unwrap();
    assert_eq!(witcher.appid, Some(292030));
    assert_eq!(app.status().unwrap().catalog.store_counts.owned, 2);

    // Signing out forgets ownership.
    let accounts = app.sign_out(Store::Gog).unwrap();
    assert!(accounts.gog.is_none());
    assert!(app.library(None).unwrap().is_empty());
}

#[test]
fn expiring_gog_tokens_are_refreshed_and_saved() {
    let fake = fake();
    let dir = TempDir::new("refresh");
    let sink = Arc::new(Finished::default());
    let app = app(&dir, &fake.server.base, sink.clone());
    let secrets = SecretStore::new(&dir.0);
    secrets
        .update(|s| {
            s.gog = Some(GogTokens {
                access_token: "expired".into(),
                refresh_token: "refresh-1".into(),
                expires_at: unix_now() - 10,
                user_id: "4812".into(),
                username: Some("tester".into()),
            })
        })
        .unwrap();
    app.start_library_sync().unwrap();
    let finished = sink.wait(1);
    assert_eq!(finished["library"]["gogOwned"], 2, "{finished}");
    assert_eq!(fake.refreshes.load(Ordering::SeqCst), 1);
    let saved = secrets.load().unwrap().gog.unwrap();
    assert_eq!(saved.access_token, GOG_ACCESS_REFRESHED);
    assert_eq!(saved.refresh_token, "refresh-2");
    assert_eq!(
        saved.username.as_deref(),
        Some("tester"),
        "kept across the refresh"
    );
}

#[test]
fn a_refused_refresh_signs_out() {
    let fake = fake();
    let dir = TempDir::new("refused");
    let sink = Arc::new(Finished::default());
    let app = app(&dir, &fake.server.base, sink.clone());
    SecretStore::new(&dir.0)
        .update(|s| {
            s.gog = Some(GogTokens {
                access_token: "expired".into(),
                refresh_token: "revoked".into(),
                expires_at: 0,
                user_id: "1".into(),
                username: None,
            })
        })
        .unwrap();
    app.start_library_sync().unwrap();
    let finished = sink.wait(1);
    assert_eq!(finished["library"]["gogSignedOut"], true, "{finished}");
    assert!(app.accounts().unwrap().gog.is_none());
}

#[test]
fn itch_key_library_and_search() {
    let fake = fake();
    let dir = TempDir::new("itch");
    let sink = Arc::new(Finished::default());
    let app = app(&dir, &fake.server.base, sink.clone());

    assert!(matches!(
        app.itch_set_key("wrong"),
        Err(Error::Invalid("itch_key"))
    ));
    assert!(matches!(
        app.itch_set_key("  "),
        Err(Error::Invalid("itch_key"))
    ));
    assert!(app.accounts().unwrap().itch.is_none());

    let accounts = app.itch_set_key(&format!(" {ITCH_KEY} ")).unwrap();
    assert_eq!(accounts.itch.unwrap().username, "Player One");
    let finished = sink.wait(1);
    assert_eq!(finished["library"]["itchOwned"], 2, "{finished}");

    let library = app.library(Some(Store::Itch)).unwrap();
    assert_eq!(library.len(), 2);
    let celeste = library.iter().find(|i| i.title == "Celeste").unwrap();
    assert_eq!(
        celeste.appid,
        Some(504230),
        "title, developer and year agree"
    );
    assert!(
        library
            .iter()
            .any(|i| i.title == "Obscure Jam Game" && i.appid.is_none())
    );

    // Searching itch.io for a Steam game ranks the same-title result first.
    let hits = app.search_store(Store::Itch, 504230).unwrap();
    assert_eq!(hits[0].title, "Celeste");
    assert!(hits[0].score >= 0.85);
    assert_eq!(hits[1].score, 0.0);
    app.link_store_product(Store::Itch, "13", 504230).unwrap();
    let matches = app.store_matches(504230).unwrap();
    assert!(matches.iter().any(|m| m.product_id == "13" && m.confident));
    assert!(matches!(
        app.link_store_product(Store::Itch, "nope", 504230),
        Err(Error::NotFound)
    ));
    assert!(matches!(
        app.search_store(Store::Gog, 504230),
        Err(Error::Invalid("store"))
    ));
}

#[test]
fn settings_have_defaults_and_validate() {
    let fake = fake();
    let dir = TempDir::new("settings");
    let app = app(&dir, &fake.server.base, Arc::new(Finished::default()));
    let s = app.settings().unwrap();
    assert!(s.library_dir.ends_with("Games"));
    assert!(!s.keep_installers && s.auto_update);

    let games = dir.0.join("MyGames").display().to_string();
    let s = app
        .update_settings(&SettingsPatch {
            library_dir: Some(games.clone()),
            keep_installers: Some(true),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(s.library_dir, games);
    assert!(s.keep_installers && s.auto_update);
    assert!(matches!(
        app.update_settings(&SettingsPatch {
            library_dir: Some("relative/path".into()),
            ..Default::default()
        }),
        Err(Error::Invalid("library_dir"))
    ));
}

#[test]
fn signing_in_during_another_job_reads_the_library_afterwards() {
    let fake = fake_with(Duration::from_millis(800));
    let dir = TempDir::new("pending");
    let sink = Arc::new(Finished::default());
    let app = app(&dir, &fake.server.base, sink.clone());

    // A store sync is busy with the (slow) catalog while the itch.io key is saved.
    app.start_store_sync().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        app.itch_set_key(ITCH_KEY).unwrap().itch.unwrap().username,
        "Player One"
    );
    let first = sink.wait(1);
    assert_eq!(first["kind"], "stores");
    // The library job that could not start then runs by itself.
    let second = sink.wait(2);
    assert_eq!(second["kind"], "library", "{second}");
    assert_eq!(second["library"]["itchOwned"], 2, "{second}");
    assert_eq!(app.library(Some(Store::Itch)).unwrap().len(), 2);
}
