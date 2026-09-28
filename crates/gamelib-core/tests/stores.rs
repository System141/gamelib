//! Store matching against a fake GOG catalog and GamesDB served locally.

mod common;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use common::http::{Request, Response, TestServer};
use common::item;
use gamelib_core::app::{App, EventSink, JobOptions};
use gamelib_core::db::Db;
use gamelib_core::db::read::{query_games, status};
use gamelib_core::db::stores::{matches_for_game, set_match_state};
use gamelib_core::db::write::upsert_games;
use gamelib_core::model::{GameQuery, MatchMethod, MatchState, Store, StoreMatch};
use gamelib_core::record::GameRecord;
use gamelib_core::stores::{StoreEndpoints, StoreSyncOptions, run_store_sync};
use gamelib_core::unix_now;
use serde_json::{Value, json};

const WITCHER_GOG: &str = "1207664643";
const DOOM_GOG: &str = "1100000001";
const HOMM3_GOG: &str = "1207658691";
const STARDEW_GOG: &str = "1453375253";
const TERRARIA_GOG: &str = "1207665503";

fn day(y: i64, m: u32, d: u32) -> i64 {
    // Days from civil (UTC), enough for test dates.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) * 86_400
}

fn steam_game(appid: u32, name: &str, company: &str, release: i64, reviews: i64) -> GameRecord {
    let mut rec = GameRecord::from_item(&item(appid, name, release, reviews, &[])).unwrap();
    rec.developers = json!([company]).to_string();
    rec.publishers = json!([company]).to_string();
    rec
}

fn steam_catalog() -> Vec<GameRecord> {
    vec![
        steam_game(
            292030,
            "The Witcher 3: Wild Hunt",
            "CD PROJEKT RED",
            day(2015, 5, 18),
            900_000,
        ),
        steam_game(379720, "DOOM", "id Software", day(2016, 5, 12), 200_000),
        steam_game(999001, "Doom", "Random Indie", day(2019, 1, 1), 12),
        steam_game(
            297000,
            "Heroes of Might & Magic III - HD Edition",
            "Ubisoft",
            day(2015, 1, 29),
            9_000,
        ),
        steam_game(
            413150,
            "Stardew Valley",
            "ConcernedApe",
            day(2016, 2, 26),
            700_000,
        ),
        steam_game(105600, "Terraria", "Re-Logic", day(2011, 5, 16), 1_000_000),
    ]
}

fn gog_product(id: &str, title: &str, company: &str, release: &str, kind: &str) -> Value {
    let slug = title
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric(), "_");
    json!({
        "id": id, "slug": slug, "title": title, "productType": kind,
        "releaseDate": release, "storeReleaseDate": release,
        "developers": [company], "publishers": [company],
        "operatingSystems": ["windows", "osx"],
        "price": { "final": "$9.99", "finalMoney": { "amount": "9.99", "currency": "USD" } },
        "coverHorizontal": "https://images.gog-statics.com/h.png",
        "coverVertical": "https://images.gog-statics.com/v.jpg",
        "storeLink": format!("https://www.gog.com/en/game/{slug}"),
    })
}

fn gog_catalog() -> Vec<Value> {
    let mut products = vec![
        gog_product(
            WITCHER_GOG,
            "The Witcher 3: Wild Hunt - Complete Edition",
            "CD PROJEKT RED",
            "2015.05.19",
            "game",
        ),
        gog_product(DOOM_GOG, "Doom", "id Software", "1993.12.10", "game"),
        gog_product(
            HOMM3_GOG,
            "Heroes of Might and Magic® 3: Complete",
            "Ubisoft",
            "1999.02.28",
            "game",
        ),
        gog_product(
            STARDEW_GOG,
            "Stardew Valley",
            "ConcernedApe",
            "2016.02.26",
            "game",
        ),
        gog_product("10", "M.A.X. + M.A.X. 2", "Interplay", "1998.07.31", "pack"),
    ];
    // Enough unrelated products for a second catalog page.
    for n in 0..100 {
        products.push(gog_product(
            &format!("{}", 2_000_000_000u64 + n),
            &format!("Filler Game {n}"),
            "Nobody",
            "2020.01.01",
            "game",
        ));
    }
    products
}

/// GOG catalog (cursor paging), GamesDB and the product API, from shared mutable data.
fn fake_stores(catalog: Arc<Mutex<Vec<Value>>>) -> TestServer {
    TestServer::start(move |req: &Request| {
        let path = req.path.as_str();
        if path == "/v1/catalog" {
            let after: u64 = req.param("searchAfter").unwrap_or("0").parse().unwrap();
            let limit: usize = req.param("limit").unwrap_or("48").parse().unwrap();
            let mut all = catalog.lock().unwrap().clone();
            all.sort_by_key(|p| p["id"].as_str().unwrap().parse::<u64>().unwrap());
            let total = all.len();
            let page: Vec<Value> = all
                .into_iter()
                .filter(|p| p["id"].as_str().unwrap().parse::<u64>().unwrap() > after)
                .take(limit)
                .collect();
            return Response::json(json!({ "productCount": total, "products": page }).to_string());
        }
        if let Some(id) = path.strip_prefix("/platforms/gog/external_releases/") {
            let steam = match id {
                DOOM_GOG => Some("999001"),
                HOMM3_GOG => Some("297000"),
                _ => None,
            };
            return match steam {
                Some(appid) => Response::json(releases(&[("gog", id), ("steam", appid)])),
                None => Response::status(404).with_body(r#"{"error":"not_found"}"#),
            };
        }
        if let Some(appid) = path.strip_prefix("/platforms/steam/external_releases/") {
            return match appid {
                "379720" => Response::json(releases(&[("steam", "379720"), ("gog", DOOM_GOG)])),
                "105600" => Response::json(releases(&[("steam", "105600"), ("gog", TERRARIA_GOG)])),
                _ => Response::status(404),
            };
        }
        if path == format!("/products/{TERRARIA_GOG}") {
            return Response::json(
                json!({
                    "id": 1207665503u64, "title": "Terraria", "slug": "terraria", "game_type": "game",
                    "content_system_compatibility": { "windows": true, "osx": true, "linux": true },
                    "links": { "product_card": "https://www.gog.com/game/terraria" },
                    "images": { "logo2x": "//images-1.gog-statics.com/terraria_glx_logo_2x.jpg" }
                })
                .to_string(),
            );
        }
        Response::status(404)
    })
}

fn releases(list: &[(&str, &str)]) -> String {
    let releases: Vec<Value> = list
        .iter()
        .map(|(platform, id)| json!({ "platform_id": platform, "external_id": id }))
        .collect();
    json!({ "type": "game", "game": { "releases": releases } }).to_string()
}

fn options(base: &str) -> StoreSyncOptions {
    StoreSyncOptions {
        endpoints: StoreEndpoints::all_at(base),
        catalog_delay: Duration::ZERO,
        gamesdb_delay: Duration::ZERO,
        ..Default::default()
    }
}

fn db() -> Db {
    let mut db = Db::open_in_memory().unwrap();
    upsert_games(db.conn_mut(), &steam_catalog(), unix_now()).unwrap();
    db
}

fn gog_matches(db: &Db, appid: u32) -> Vec<StoreMatch> {
    matches_for_game(db.conn(), appid)
        .unwrap()
        .into_iter()
        .filter(|m| m.store == Store::Gog)
        .collect()
}

fn sync(db: &mut Db, base: &str) -> gamelib_core::model::StoresReport {
    let mut phases = Vec::new();
    let report = run_store_sync(
        db,
        &options(base),
        None,
        &AtomicBool::new(false),
        &mut |p| phases.push(p.phase),
    )
    .unwrap();
    assert!(!phases.is_empty());
    report
}

#[test]
fn matches_by_title_then_gamesdb() {
    let catalog = Arc::new(Mutex::new(gog_catalog()));
    let server = fake_stores(catalog.clone());
    let mut db = db();

    let report = sync(&mut db, &server.base);
    assert_eq!(report.catalog, 105);
    assert_eq!(report.inserted, 105);
    assert_eq!(server.count("/v1/catalog"), 2, "two cursor pages");
    // Witcher and Stardew matched with certainty; the pack is never looked up.
    assert_eq!(report.checked, 102);
    assert_eq!(report.remaining, 0);
    assert_eq!(server.count("/platforms/gog/"), 102);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    // Title match with company and year: certain.
    let witcher = gog_matches(&db, 292030);
    assert_eq!(witcher.len(), 1);
    assert_eq!(witcher[0].product_id, WITCHER_GOG);
    assert_eq!(witcher[0].method, MatchMethod::Title);
    assert!(witcher[0].confident && (witcher[0].score - 1.0).abs() < 1e-6);
    assert_eq!(
        witcher[0].title,
        "The Witcher 3: Wild Hunt - Complete Edition"
    );
    assert_eq!(witcher[0].price.as_deref(), Some("$9.99"));

    // Different titles, tied together by GamesDB.
    let homm = gog_matches(&db, 297000);
    assert_eq!(homm.len(), 1);
    assert_eq!(homm[0].method, MatchMethod::Gamesdb);

    // GamesDB overrules the title guess (DOOM 2016 is not the 1993 Doom).
    assert!(gog_matches(&db, 379720).is_empty());
    let doom = gog_matches(&db, 999001);
    assert_eq!(doom.len(), 1);
    assert_eq!(
        (doom[0].product_id.as_str(), doom[0].method),
        (DOOM_GOG, MatchMethod::Gamesdb)
    );

    let q = GameQuery {
        stores: vec![Store::Gog],
        limit: 50,
        ..Default::default()
    };
    let page = query_games(&mut db, &q, unix_now()).unwrap();
    let mut appids: Vec<u32> = page.items.iter().map(|g| g.appid).collect();
    appids.sort();
    assert_eq!(appids, [292030, 297000, 413150, 999001]);
    assert!(page.items.iter().all(|g| g.stores == [Store::Gog]));

    let all = query_games(&mut db, &GameQuery::default(), unix_now()).unwrap();
    let terraria = all.items.iter().find(|g| g.appid == 105600).unwrap();
    assert!(terraria.stores.is_empty());

    let counts = status(&mut db).unwrap().store_counts;
    assert_eq!((counts.gog, counts.gog_products), (4, 105));
    assert!(counts.last_store_sync_at.is_some());
}

#[test]
fn user_verdicts_survive_refreshes() {
    let catalog = Arc::new(Mutex::new(gog_catalog()));
    let server = fake_stores(catalog.clone());
    let mut db = db();
    sync(&mut db, &server.base);

    let now = unix_now();
    assert!(
        set_match_state(
            db.conn(),
            Store::Gog,
            STARDEW_GOG,
            413150,
            MatchState::Rejected,
            now
        )
        .unwrap()
    );
    assert!(
        set_match_state(
            db.conn(),
            Store::Gog,
            WITCHER_GOG,
            292030,
            MatchState::Confirmed,
            now
        )
        .unwrap()
    );
    assert!(
        !set_match_state(
            db.conn(),
            Store::Gog,
            STARDEW_GOG,
            1,
            MatchState::Confirmed,
            now
        )
        .unwrap()
    );

    // One filler product left the store.
    catalog.lock().unwrap().retain(|p| p["id"] != "2000000050");
    let before = server.count("/platforms/gog/");
    let report = sync(&mut db, &server.base);
    assert_eq!(report.inserted, 0);
    // Only Stardew is asked about again: its certain title match was rejected.
    assert_eq!(report.checked, 1);
    assert_eq!(server.count("/platforms/gog/"), before + 1);

    assert!(
        gog_matches(&db, 413150).is_empty(),
        "rejected stays rejected"
    );
    let witcher = gog_matches(&db, 292030);
    assert_eq!(witcher[0].state, MatchState::Confirmed);

    let unlisted: i64 = db
        .conn()
        .query_row(
            "SELECT in_catalog FROM store_products WHERE product_id = '2000000050'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(unlisted, 0);
    assert_eq!(status(&mut db).unwrap().store_counts.gog, 3);
}

#[test]
fn gamesdb_can_be_skipped_and_limited() {
    let server = fake_stores(Arc::new(Mutex::new(gog_catalog())));
    let mut db = db();
    let mut opts = options(&server.base);
    opts.gamesdb_limit = Some(10);
    let report =
        run_store_sync(&mut db, &opts, None, &AtomicBool::new(false), &mut |_| {}).unwrap();
    assert_eq!((report.checked, report.remaining), (10, 92));

    opts.gamesdb = false;
    let report =
        run_store_sync(&mut db, &opts, None, &AtomicBool::new(false), &mut |_| {}).unwrap();
    assert_eq!(report.checked, 0);
    assert_eq!(server.count("/platforms/gog/"), 10);
}

#[test]
fn unreachable_gamesdb_keeps_title_matches() {
    // Catalog only; every other request fails with 500 after retries would take too long, so
    // answer 400 (not retried).
    let catalog = gog_catalog();
    let server = TestServer::start(move |req: &Request| {
        if req.path == "/v1/catalog" {
            let after: u64 = req.param("searchAfter").unwrap().parse().unwrap();
            let mut sorted: Vec<&Value> = catalog.iter().collect();
            sorted.sort_by_key(|p| p["id"].as_str().unwrap().parse::<u64>().unwrap());
            let page: Vec<&Value> = sorted
                .into_iter()
                .filter(|p| p["id"].as_str().unwrap().parse::<u64>().unwrap() > after)
                .take(100)
                .collect();
            return Response::json(json!({ "productCount": 105, "products": page }).to_string());
        }
        Response::status(400)
    });
    let mut db = db();
    let report = sync(&mut db, &server.base);
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    assert_eq!(report.checked, 0);
    assert_eq!(gog_matches(&db, 292030).len(), 1);
}

#[test]
fn cancelling_keeps_the_catalog() {
    let server = fake_stores(Arc::new(Mutex::new(gog_catalog())));
    let mut db = db();
    let cancel = AtomicBool::new(false);
    let result = run_store_sync(&mut db, &options(&server.base), None, &cancel, &mut |p| {
        if p.phase == gamelib_core::model::SyncPhase::GogIds && p.fetched >= 20 {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    });
    assert!(matches!(result, Err(gamelib_core::Error::Cancelled)));
    assert_eq!(status(&mut db).unwrap().store_counts.gog_products, 105);
    assert_eq!(gog_matches(&db, 292030).len(), 1);
}

// --- through the App ------------------------------------------------------------------------

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "gamelib-stores-{tag}-{}-{}",
            std::process::id(),
            unix_now()
        ));
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
        if event == gamelib_core::app::EVENT_FINISHED {
            self.events.lock().unwrap().push(payload);
            self.ready.notify_all();
        }
    }
}

impl Finished {
    fn wait(&self) -> Value {
        let guard = self.events.lock().unwrap();
        let (guard, timeout) = self
            .ready
            .wait_timeout_while(guard, Duration::from_secs(20), |e| e.is_empty())
            .unwrap();
        assert!(!timeout.timed_out(), "job did not finish");
        guard[0].clone()
    }
}

fn app(dir: &TempDir, base: &str, sink: Arc<Finished>) -> App {
    let path = dir.0.join("gamelib.db");
    let mut db = Db::open(&path).unwrap();
    upsert_games(db.conn_mut(), &steam_catalog(), unix_now()).unwrap();
    drop(db);
    let options = JobOptions {
        stores: options(base),
        ..Default::default()
    };
    App::with_options(path, sink, options).unwrap()
}

#[test]
fn store_job_and_on_demand_lookups() {
    let server = fake_stores(Arc::new(Mutex::new(gog_catalog())));
    let dir = TempDir::new("app");
    let sink = Arc::new(Finished::default());
    let app = app(&dir, &server.base, sink.clone());

    app.start_store_sync().unwrap();
    let finished = sink.wait();
    assert_eq!(finished["kind"], "stores");
    assert_eq!(finished["outcome"], "completed");
    assert_eq!(finished["stores"]["catalog"], 105);
    assert_eq!(app.status().unwrap().catalog.store_counts.gog, 4);

    // DOOM (2016): GamesDB knows its GOG product, which title matching had rejected by year.
    let doom = app.refresh_store_matches(379720).unwrap();
    assert_eq!(doom.len(), 1);
    assert_eq!(doom[0].method, MatchMethod::Gamesdb);
    let lookups = server.count("/platforms/steam/");
    app.refresh_store_matches(379720).unwrap();
    assert_eq!(
        server.count("/platforms/steam/"),
        lookups,
        "looked up once a month"
    );

    // Terraria's GOG product is not in the catalog: fetched from the product API.
    let terraria = app.refresh_store_matches(105600).unwrap();
    assert_eq!(terraria.len(), 1);
    assert_eq!(terraria[0].product_id, TERRARIA_GOG);
    assert_eq!(terraria[0].title, "Terraria");
    assert_eq!(
        terraria[0].cover_wide.as_deref(),
        Some("https://images-1.gog-statics.com/terraria_glx_logo_2x.jpg")
    );

    // Unknown to GamesDB: the title match stays, no error.
    assert_eq!(app.refresh_store_matches(413150).unwrap().len(), 1);

    app.set_match_state(Store::Gog, TERRARIA_GOG, 105600, MatchState::Rejected)
        .unwrap();
    assert!(app.store_matches(105600).unwrap().is_empty());
    assert!(matches!(
        app.set_match_state(Store::Gog, "nope", 1, MatchState::Confirmed),
        Err(gamelib_core::Error::NotFound)
    ));
}
