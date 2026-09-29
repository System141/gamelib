//! AstralGames handler: slug lookup against a local server, where the page must carry the
//! Steam appid the query is for.

mod common;

use common::http::{Request, Response, TestServer};
use gamelib_core::links::SiteHandler;
use gamelib_core::links::find::{self, FindQuery};
use gamelib_core::links::sites::astralgames::AstralGames;
use gamelib_core::model::LinkKind;

/// A fake AstralGames: only game pages, one with `app_<id>_` assets and one with a
/// `steam/apps/<id>` link.
fn server() -> TestServer {
    TestServer::start(|req: &Request| {
        match req.path.as_str() {
        "/game/elden-ring" => Response::status(200).with_body(
            "<html><head><meta property=\"og:image\" content=\"/uploads/app_1245620_header.jpg\">\
             </head><body><h1>ELDEN RING</h1></body></html>",
        ),
        "/game/hollow-knight" => Response::status(200).with_body(
            "<html><body><img src=\"https://cdn.example.net/steam/apps/367520/header.jpg\"></body></html>",
        ),
        _ => Response::status(404),
    }
    })
}

fn query(appid: u32, title: &str) -> FindQuery {
    FindQuery {
        appid,
        title: title.into(),
    }
}

#[test]
fn a_slug_page_is_confirmed_by_its_app_asset() {
    let server = server();
    let handler = AstralGames::with_base(server.base.clone());
    let found = handler
        .find(&query(1245620, "Elden Ring"), &find::client().unwrap())
        .unwrap();
    assert_eq!(found.len(), 1);
    let link = &found[0];
    assert_eq!(link.site_id, "astralgames");
    assert_eq!(link.url, format!("{}/game/elden-ring", server.base));
    assert_eq!(link.label, "AstralGames");
    assert_eq!(link.kind, LinkKind::Page);
    assert_eq!(link.version, None);
    assert_eq!(link.size, None);
    assert_eq!(link.notes, None);
    assert_eq!(link.score, 1.0);
    assert!(link.needs_browser);
    assert!(!link.direct);
    assert_eq!(server.count("/game/elden-ring"), 1);
}

#[test]
fn a_steam_apps_link_is_the_fallback_marker() {
    let server = server();
    let handler = AstralGames::with_base(server.base.clone());
    let found = handler
        .find(&query(367520, "Hollow Knight"), &find::client().unwrap())
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].url, format!("{}/game/hollow-knight", server.base));
}

#[test]
fn a_page_for_another_game_is_not_a_match() {
    let server = server();
    let handler = AstralGames::with_base(server.base.clone());
    let found = handler
        .find(&query(999, "Elden Ring"), &find::client().unwrap())
        .unwrap();
    assert!(found.is_empty());
}

#[test]
fn a_missing_page_is_not_a_match() {
    let server = server();
    let handler = AstralGames::with_base(server.base.clone());
    let found = handler
        .find(&query(1245620, "Some Other Game"), &find::client().unwrap())
        .unwrap();
    assert!(found.is_empty());
    assert_eq!(server.count("/game/some-other-game"), 1);
}

#[test]
fn a_title_without_a_slug_fetches_nothing() {
    let server = server();
    let handler = AstralGames::with_base(server.base.clone());
    let found = handler
        .find(&query(1245620, "ウィッチャー"), &find::client().unwrap())
        .unwrap();
    assert!(found.is_empty());
    assert_eq!(server.count("/game/"), 0);
}
