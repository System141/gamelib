//! What helps decide on a purchase: reviews and system requirements from Steam's store, and
//! prices from IsThereAnyDeal with the user's key, against a fake server.

mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use common::http::{Request, Response, TestServer};
use gamelib_core::app::{App, EventSink, JobOptions};
use gamelib_core::db::Db;
use gamelib_core::stores::{StoreEndpoints, StoreSyncOptions};
use gamelib_core::{Error, unix_now};
use serde_json::{Value, json};

const KEY: &str = "itad-key-123";
const WITCHER: &str = "018d937f-1212-7232-b23f-a046f6fd4a57";

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "gamelib-{name}-{}-{}",
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

struct Quiet;

impl EventSink for Quiet {
    fn emit(&self, _event: &str, _payload: Value) {}
}

fn app(dir: &TempDir, base: &str) -> App {
    let path = dir.0.join("gamelib.db");
    drop(Db::open(&path).unwrap());
    let options = JobOptions {
        stores: StoreSyncOptions {
            endpoints: StoreEndpoints::all_at(base),
            catalog_delay: Duration::ZERO,
            gamesdb_delay: Duration::ZERO,
            ..Default::default()
        },
        ..Default::default()
    };
    App::with_options(path, Arc::new(Quiet), options).unwrap()
}

fn price(amount: f64) -> Value {
    json!({ "amount": amount, "amountInt": (amount * 100.0).round() as i64, "currency": "USD" })
}

fn itad(req: &Request) -> Response {
    // The key goes in a header only, never in the address.
    if req.param("key").is_some() {
        return Response::status(400);
    }
    if req.header("itad-api-key") != Some(KEY) {
        return Response::status(403)
            .with_body(r#"{"status_code":403,"reason_phrase":"Invalid or expired api key"}"#);
    }
    match req.path.as_str() {
        "/games/lookup/v1" => match req.param("appid") {
            Some("292030") => Response::json(json!({ "found": true, "game": { "id": WITCHER, "slug": "the-witcher-iii-wild-hunt", "title": "The Witcher 3", "type": "game", "mature": false, "assets": {} } }).to_string()),
            _ => Response::json(r#"{"found":false}"#),
        },
        "/games/prices/v3" => {
            assert_eq!(req.method, "POST");
            assert_eq!(req.param("country"), Some("TR"));
            assert!(req.body.contains(WITCHER), "{}", req.body);
            Response::json(
                json!([{
                    "id": WITCHER,
                    "historyLow": { "all": price(3.99), "y1": price(4.99), "m3": price(7.99) },
                    "deals": [
                        { "shop": { "id": 61, "name": "Steam" }, "price": price(9.99), "regular": price(39.99), "cut": 75, "voucher": null,
                          "storeLow": price(5.99), "flag": null, "drm": [{ "id": 61, "name": "Steam" }], "platforms": [],
                          "timestamp": "2026-09-20T17:00:00+02:00", "expiry": "2026-10-05T19:00:00+02:00", "url": "https://itad.link/018d937f/61/" },
                        { "shop": { "id": 35, "name": "GOG" }, "price": price(7.99), "regular": price(39.99), "cut": 80, "voucher": null,
                          "storeLow": null, "flag": "H", "drm": [{ "id": 1000, "name": "DRM Free" }], "platforms": [],
                          "timestamp": "2026-09-21T17:00:00+02:00", "expiry": null, "url": "https://itad.link/018d937f/35/" }
                    ]
                }])
                .to_string(),
            )
        }
        "/games/overview/v2" => Response::json(
            json!({
                "prices": [{
                    "id": WITCHER, "current": null, "bundled": 1,
                    "lowest": { "shop": { "id": 35, "name": "GOG" }, "price": price(3.99), "regular": price(39.99), "cut": 90, "timestamp": "2024-11-28T18:00:00+01:00" },
                    "urls": { "game": "https://isthereanydeal.com/game/thewitcheriiiwildhunt/info/" }
                }],
                "bundles": [{
                    "id": 1234, "title": "CD PROJEKT Bundle", "page": { "id": 7, "name": "Humble Bundle", "shopId": 37 },
                    "url": "https://www.humblebundle.com/games/cdpr", "details": "https://isthereanydeal.com/bundles/1234/",
                    "isMature": false, "publish": "2026-09-26T19:00:00+02:00", "expiry": "2026-10-10T19:00:00+02:00", "note": null,
                    "counts": { "games": 3, "media": 0 },
                    "tiers": [
                        { "price": price(1.0), "addon": false, "games": [{ "id": "another-game" }] },
                        { "price": price(20.0), "addon": false, "games": [{ "id": WITCHER }] },
                        { "price": price(12.0), "addon": false, "games": [{ "id": WITCHER }] }
                    ]
                }]
            })
            .to_string(),
        ),
        "/games/subs/v1" => Response::json(json!([{ "id": WITCHER, "subs": [{ "id": 1, "name": "PC Game Pass", "leaving": "2026-12-01T00:00:00Z" }] }]).to_string()),
        "/games/history/v2" => {
            assert_eq!(req.param("shops"), Some("61"));
            assert!(req.param("since").is_some_and(|s| s.ends_with("T00:00:00Z")));
            Response::json(
                json!([
                    { "timestamp": "2026-06-26T19:00:00+02:00", "shop": { "id": 61, "name": "Steam" }, "deal": { "price": price(9.99), "regular": price(39.99), "cut": 75 } },
                    { "timestamp": "2026-07-10T19:00:00+02:00", "shop": { "id": 61, "name": "Steam" }, "deal": null },
                    { "timestamp": "2025-11-27T19:00:00+01:00", "shop": { "id": 61, "name": "Steam" }, "deal": { "price": price(7.99), "regular": price(39.99), "cut": 80 } }
                ])
                .to_string(),
            )
        }
        _ => Response::status(404),
    }
}

fn store(req: &Request) -> Response {
    match req.path.as_str() {
        "/appreviews/292030" => match (req.param("filter"), req.param("language")) {
            (Some("all"), Some("turkish")) => {
                Response::json(include_str!("fixtures/appreviews_top.json"))
            }
            (Some("all"), Some("english")) => Response::json(r#"{"success":1,"reviews":[]}"#),
            (Some("recent"), _) => Response::json(include_str!("fixtures/appreviews_recent.json")),
            _ => Response::status(400),
        },
        "/api/appdetails" => {
            let all: Value =
                serde_json::from_str(include_str!("fixtures/requirements.json")).unwrap();
            let appid = req.param("appids").unwrap_or_default().to_owned();
            if req.param("l") != Some("english") {
                return Response::status(400);
            }
            match all.get(&appid) {
                Some(pc) => Response::json(json!({ appid: { "success": true, "data": { "pc_requirements": pc, "mac_requirements": [], "linux_requirements": [] } } }).to_string()),
                None => Response::json(json!({ appid: { "success": false } }).to_string()),
            }
        }
        _ => itad(req),
    }
}

#[test]
fn reviews_and_requirements_come_from_steams_store() {
    let dir = TempDir::new("purchase-steam");
    let server = TestServer::start(store);
    let app = app(&dir, &server.base);

    let reviews = app.game_reviews(292030).unwrap();
    assert_eq!(
        reviews.top.len(),
        3,
        "Turkish ones, and no English ones to add"
    );
    assert_eq!(reviews.top[0].language, "turkish");
    let recent = reviews.recent.unwrap();
    assert_eq!((recent.count, recent.positive), (6, 5));

    let req = app.game_requirements(1086940).unwrap();
    assert!(!req.minimum.lines.is_empty() && !req.recommended.lines.is_empty());
    assert!(
        req.minimum
            .checks
            .iter()
            .any(|c| format!("{:?}", c.kind) == "Memory")
    );
    assert!(!req.pc.os.is_empty());
    assert!(matches!(app.game_requirements(1), Err(Error::NotFound)));
}

#[test]
fn prices_need_a_key_that_is_checked_and_never_shown() {
    let dir = TempDir::new("purchase-itad");
    let server = TestServer::start(itad);
    let app = app(&dir, &server.base);

    assert!(
        app.game_prices(292030).unwrap().is_none(),
        "no key, no prices"
    );
    assert!(matches!(
        app.itad_set_key("wrong-key"),
        Err(Error::Invalid("itad_key"))
    ));
    assert!(matches!(
        app.itad_set_key("bad key!"),
        Err(Error::Invalid("itad_key"))
    ));
    assert!(app.accounts().unwrap().itad.is_none());

    let accounts = app.itad_set_key(&format!("  {KEY} ")).unwrap();
    assert!(accounts.itad.is_some());
    assert!(!serde_json::to_string(&accounts).unwrap().contains(KEY));

    let prices = app.game_prices(292030).unwrap().unwrap();
    assert!(prices.found);
    assert_eq!(
        prices.url.as_deref(),
        Some("https://isthereanydeal.com/game/thewitcheriiiwildhunt/info/")
    );
    let shops: Vec<_> = prices
        .deals
        .iter()
        .map(|d| (d.shop.as_str(), d.price.amount, d.cut))
        .collect();
    assert_eq!(
        shops,
        [("GOG", 7.99, 80), ("Steam", 9.99, 75)],
        "cheapest first"
    );
    assert_eq!(prices.deals[1].drm, ["Steam"]);
    assert!(prices.deals[1].expiry.is_some() && prices.deals[0].expiry.is_none());
    let lowest = prices.lowest.unwrap();
    assert_eq!(
        (lowest.shop.as_str(), lowest.price.amount, lowest.cut),
        ("GOG", 3.99, 90)
    );
    assert_eq!(prices.lowest_year.unwrap().amount, 4.99);
    assert_eq!(prices.lowest_months.unwrap().amount, 7.99);
    assert_eq!(prices.subscriptions.len(), 1);
    assert_eq!(prices.subscriptions[0].name, "PC Game Pass");
    assert!(prices.subscriptions[0].leaving.is_some());
    assert_eq!(prices.bundles.len(), 1);
    let bundle = &prices.bundles[0];
    assert_eq!(
        bundle.price.as_ref().unwrap().amount,
        12.0,
        "the cheapest tier with the game"
    );
    assert_eq!(
        bundle.url.as_deref(),
        Some("https://isthereanydeal.com/bundles/1234/"),
        "IsThereAnyDeal's page, not the seller's"
    );
    let history: Vec<_> = prices.history.iter().map(|p| p.price).collect();
    assert_eq!(history, [7.99, 9.99], "oldest first, removals left out");
    assert!(prices.history[0].at < prices.history[1].at);

    let unknown = app.game_prices(620).unwrap().unwrap();
    assert!(!unknown.found && unknown.deals.is_empty());

    for req in server.requests() {
        assert!(
            req.param("key").is_none() && req.header("itad-api-key").is_some(),
            "{req:?}"
        );
    }

    let accounts = app.itad_remove_key().unwrap();
    assert!(accounts.itad.is_none());
    assert!(app.game_prices(292030).unwrap().is_none());
}

#[test]
fn a_revoked_key_says_so() {
    let dir = TempDir::new("purchase-revoked");
    let server = TestServer::start(|req| {
        if req.param("appid") == Some("292030")
            && req.path == "/games/lookup/v1"
            && req.header("itad-api-key") == Some(KEY)
            && req.method == "GET"
        {
            // The check when saving passes; later the key is revoked.
            static CHECKED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !CHECKED.swap(true, std::sync::atomic::Ordering::SeqCst) {
                return Response::json(r#"{"found":true,"game":{"id":"x"}}"#);
            }
        }
        Response::status(403)
    });
    let app = app(&dir, &server.base);
    app.itad_set_key(KEY).unwrap();
    assert!(matches!(
        app.game_prices(292030),
        Err(Error::Invalid("itad_key"))
    ));
}
