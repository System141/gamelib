//! GameBounty handler: search-card filtering and cover-image appid confirmation against a
//! local server.

mod common;

use common::http::{Request, Response, TestServer};
use gamelib_core::links::SiteHandler;
use gamelib_core::links::find::{self, FindQuery};
use gamelib_core::links::sites::gamebounty::GameBounty;
use gamelib_core::model::LinkKind;

/// A fake GameBounty: the search page serves `cards` (with `{base}` in the addresses), and the
/// one real card page names the Steam appid in its cover image.
fn server(cards: &str) -> TestServer {
    let cards = cards.to_owned();
    TestServer::start(move |req: &Request| {
        let base = format!("http://{}", req.header("host").unwrap_or_default());
        match req.path.as_str() {
            "/search" => Response::status(200)
                .with_header("Content-Type", "text/html")
                .with_body(format!(
                    "<html><body>{}</body></html>",
                    cards.replace("{base}", &base)
                )),
            "/elden-ring-free-pc-download" => Response::status(200).with_body(
                "<html><head><meta property=\"og:image\" \
                 content=\"https://cdn.example.net/steam/apps/1245620/header.jpg\"></head></html>",
            ),
            _ => Response::status(404),
        }
    })
}

/// The search results of the site: one card for a namesake, one for the game, one on a foreign
/// host naming the game.
const CARDS: &str = "\
<a class=\"card\" href=\"{base}/elden-ring-nightreign-free-pc-download\"><h3>Elden Ring Nightreign</h3></a>\
<a class=\"card\" href=\"{base}/elden-ring-free-pc-download\"><h3>ELDEN RING</h3></a>\
<a class=\"card\" href=\"https://elsewhere.example/elden-ring-free-pc-download\"><h3>Elden Ring</h3></a>";

fn query(appid: u32, title: &str) -> FindQuery {
    FindQuery {
        appid,
        title: title.into(),
    }
}

#[test]
fn the_search_hit_is_confirmed_by_the_cover_appid() {
    let server = server(CARDS);
    let handler = GameBounty::with_base(server.base.clone());
    let found = handler
        .find(&query(1245620, "Elden Ring"), &find::client().unwrap())
        .unwrap();
    assert_eq!(found.len(), 1);
    let link = &found[0];
    assert_eq!(link.site_id, "gamebounty");
    assert_eq!(
        link.url,
        format!("{}/elden-ring-free-pc-download", server.base)
    );
    assert_eq!(link.label, "GameBounty");
    assert_eq!(link.kind, LinkKind::Page);
    assert_eq!(link.version, None);
    assert_eq!(link.size, None);
    assert_eq!(link.notes, None);
    assert_eq!(link.score, 1.0);
    assert!(link.needs_browser);
    assert!(!link.direct);
    // The search is asked with the title, and the namesake's card is never fetched.
    let search = &server.requests()[0];
    assert_eq!(search.path, "/search");
    assert_eq!(search.param("q"), Some("Elden Ring"));
    assert_eq!(server.count("/elden-ring-nightreign-free-pc-download"), 0);
    assert_eq!(server.count("/elden-ring-free-pc-download"), 1);
}

#[test]
fn a_page_for_another_game_is_not_a_match() {
    let server = server(CARDS);
    let handler = GameBounty::with_base(server.base.clone());
    let found = handler
        .find(&query(999, "Elden Ring"), &find::client().unwrap())
        .unwrap();
    assert!(found.is_empty());
}

#[test]
fn a_dead_candidate_falls_through_to_the_next() {
    let server = server(
        "<a href=\"{base}/elden-ring-old-free-pc-download\"><h3>Elden Ring</h3></a>\
         <a href=\"{base}/elden-ring-free-pc-download\"><h3>Elden Ring</h3></a>",
    );
    let handler = GameBounty::with_base(server.base.clone());
    let found = handler
        .find(&query(1245620, "Elden Ring"), &find::client().unwrap())
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].url,
        format!("{}/elden-ring-free-pc-download", server.base)
    );
    assert_eq!(server.count("/elden-ring-old-free-pc-download"), 1);
}

#[test]
fn an_empty_search_falls_back_to_the_title_slug() {
    let server = server("");
    let handler = GameBounty::with_base(server.base.clone());
    let found = handler
        .find(&query(1245620, "Elden Ring"), &find::client().unwrap())
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].url,
        format!("{}/elden-ring-free-pc-download", server.base)
    );
}

#[test]
fn a_title_without_a_slug_stops_after_the_search() {
    let server = server("");
    let handler = GameBounty::with_base(server.base.clone());
    let found = handler
        .find(&query(1245620, "ウィッチャー"), &find::client().unwrap())
        .unwrap();
    assert!(found.is_empty());
    assert_eq!(server.requests().len(), 1, "only the search went out");
}

#[test]
fn the_query_title_is_sent_encoded() {
    let server = server("");
    let handler = GameBounty::with_base(server.base.clone());
    let found = handler
        .find(
            &query(387290, "Ori & the Blind Forest"),
            &find::client().unwrap(),
        )
        .unwrap();
    assert!(found.is_empty());
    let search = &server.requests()[0];
    assert_eq!(search.param("q"), Some("Ori & the Blind Forest"));
    // The fallback slug is tried and 404s.
    assert_eq!(server.count("/ori-the-blind-forest-free-pc-download"), 1);
}
