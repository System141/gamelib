//! AnkerGames handler: slug lookup, the apostrophe variant and appid confirmation, against a
//! local server. The site's sitemaps are gone, so there is no catalog to parse.

mod common;

use common::http::{Request, Response, TestServer};
use gamelib_core::links::SiteHandler;
use gamelib_core::links::find::{self, FindQuery};
use gamelib_core::links::sites::ankergames::AnkerGames;
use gamelib_core::model::LinkKind;

/// A fake AnkerGames: only game pages, with the Steam appid each links to.
fn server() -> TestServer {
    TestServer::start(|req: &Request| match req.path.as_str() {
        p if p.starts_with("/game/") => match game_page(&p["/game/".len()..]) {
            Some(html) => Response::json(html),
            None => Response::status(404),
        },
        _ => Response::status(404),
    })
}

/// A game page whose title, description and Steam link match the slug.
fn game_page(slug: &str) -> Option<String> {
    let (title, appid, version, size) = match slug {
        "the-witcher-3-wild-hunt" => ("THE WITCHER 3: WILD HUNT", 292030, "v1.32 + DLC", "26.6 GB"),
        "elden-ring-nightreign" => (
            "ELDEN RING NIGHTREIGN",
            2778580,
            "v1.03.3 + Co-op",
            "48.2 GB",
        ),
        // The site's own slug for this title drops the apostrophe; the dashed spelling 404s.
        "assassins-creed-iv-black-flag" => {
            ("ASSASSIN'S CREED IV: BLACK FLAG", 33230, "v1.07", "12.5 GB")
        }
        _ => return None,
    };
    Some(format!(
        "<html><head><title>{title} Free Download ({version}) | AnkerGames</title>\
         <meta name=\"description\" content=\"Download {title} for free ({version}, {size}).\">\
         </head><body><a href=\"https://store.steampowered.com/app/{appid}\">Steam</a></body></html>"
    ))
}

fn query(appid: u32, title: &str) -> FindQuery {
    FindQuery {
        appid,
        title: title.into(),
    }
}

#[test]
fn exact_slug_match_confirmed_by_appid() {
    let server = server();
    let handler = AnkerGames::with_base(server.base.clone());
    let found = handler
        .find(
            &query(292030, "The Witcher 3: Wild Hunt"),
            &find::client().unwrap(),
        )
        .unwrap();
    assert_eq!(found.len(), 1);
    let link = &found[0];
    assert_eq!(link.site_id, "ankergames");
    assert_eq!(
        link.url,
        format!("{}/game/the-witcher-3-wild-hunt", server.base)
    );
    assert_eq!(link.label, "AnkerGames");
    assert_eq!(link.kind, LinkKind::Page);
    assert_eq!(link.version.as_deref(), Some("v1.32 + DLC"));
    assert_eq!(link.size.as_deref(), Some("26.6 GB"));
    assert!(link.needs_browser);
    assert!(!link.direct);
    assert_eq!(link.score, 1.0);
    // The exact slug is the only page fetched; there is no index request.
    assert_eq!(server.count("/game/"), 1);
}

#[test]
fn apostrophe_variant_is_tried_when_the_exact_slug_is_gone() {
    let server = server();
    let handler = AnkerGames::with_base(server.base.clone());
    let found = handler
        .find(
            &query(33230, "Assassin's Creed IV: Black Flag"),
            &find::client().unwrap(),
        )
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].url,
        format!("{}/game/assassins-creed-iv-black-flag", server.base)
    );
    assert_eq!(found[0].version.as_deref(), Some("v1.07"));
    // The dashed spelling is tried first and 404s; only then the site's own spelling.
    assert_eq!(server.count("/game/assassin-s-creed-iv-black-flag"), 1);
    assert_eq!(server.count("/game/assassins-creed-iv-black-flag"), 1);
}

#[test]
fn a_page_for_another_game_is_not_a_match() {
    let server = server();
    let handler = AnkerGames::with_base(server.base.clone());
    let found = handler
        .find(
            &query(730, "The Witcher 3: Wild Hunt"),
            &find::client().unwrap(),
        )
        .unwrap();
    assert!(found.is_empty());
}

#[test]
fn a_variant_page_for_another_game_is_not_a_match() {
    let server = server();
    let handler = AnkerGames::with_base(server.base.clone());
    let found = handler
        .find(
            &query(999, "Assassin's Creed IV: Black Flag"),
            &find::client().unwrap(),
        )
        .unwrap();
    assert!(found.is_empty());
    assert_eq!(server.count("/game/"), 2);
}

#[test]
fn a_title_without_a_slug_fetches_nothing() {
    let server = server();
    let handler = AnkerGames::with_base(server.base.clone());
    let found = handler
        .find(&query(292030, "ウィッチャー"), &find::client().unwrap())
        .unwrap();
    assert!(found.is_empty());
    assert_eq!(server.count("/game/"), 0);
}

#[test]
fn an_expected_page_keeps_its_version_and_size() {
    let server = server();
    let handler = AnkerGames::with_base(server.base.clone());
    let found = handler
        .find(
            &query(2778580, "ELDEN RING NIGHTREIGN"),
            &find::client().unwrap(),
        )
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].version.as_deref(), Some("v1.03.3 + Co-op"));
    assert_eq!(found[0].size.as_deref(), Some("48.2 GB"));
}
