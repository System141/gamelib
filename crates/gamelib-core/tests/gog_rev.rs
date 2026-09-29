//! GoG Revived handler: the sitemap catalog and the heading confirmation against a local
//! server.

mod common;

use common::http::{Request, Response, TestServer};
use gamelib_core::links::SiteHandler;
use gamelib_core::links::find::{self, FindQuery};
use gamelib_core::links::sites::gog_rev::GoGRevived;
use gamelib_core::model::LinkKind;

/// A fake GoG Revived: a sitemap index, one sitemap listing game pages plus noise, and pages
/// whose heading the test controls.
fn server(heading: &str) -> TestServer {
    let heading = heading.to_owned();
    TestServer::start(move |req: &Request| {
        let base = format!("http://{}", req.header("host").unwrap_or_default());
        match req.path.as_str() {
            "/sitemap-index.xml" => Response::status(200).with_body(format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
                 <sitemapindex><sitemap><loc>{base}/sitemap-1.xml</loc></sitemap>\
                 <sitemap><loc>https://elsewhere.example/sitemap-2.xml</loc></sitemap></sitemapindex>"
            )),
            "/sitemap-1.xml" => Response::status(200).with_body(format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
                 <urlset><url><loc>{base}/games/hollow_knight/</loc></url>\
                 <url><loc>{base}/games/elden_ring/</loc></url>\
                 <url><loc>{base}/news/update/</loc></url>\
                 <url><loc>not a url</loc></url></urlset>"
            )),
            "/games/elden_ring/" => Response::status(200)
                .with_body(format!("<html><body><h1 class=\"title\">{heading}</h1></body></html>")),
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
fn an_underscore_slug_page_is_confirmed_by_its_heading() {
    let server = server("ELDEN RING");
    let handler = GoGRevived::with_base(server.base.clone());
    let found = handler
        .find(&query(1245620, "Elden Ring"), &find::client().unwrap())
        .unwrap();
    assert_eq!(found.len(), 1);
    let link = &found[0];
    assert_eq!(link.site_id, "gog-rev");
    assert_eq!(link.url, format!("{}/games/elden_ring/", server.base));
    assert_eq!(link.label, "GoG Revived");
    assert_eq!(link.kind, LinkKind::Page);
    assert_eq!(link.version, None);
    assert_eq!(link.size, None);
    assert_eq!(link.notes, None);
    // Confirmed only by the page's own heading.
    assert_eq!(link.score, 0.75);
    assert!(link.needs_browser);
    assert!(!link.direct);
    // The index is read once, the sitemap once, and only the matching slug's page is fetched.
    assert_eq!(server.count("/sitemap-index.xml"), 1);
    assert_eq!(server.count("/sitemap-1.xml"), 1);
    assert_eq!(server.count("/games/elden_ring/"), 1);
    assert_eq!(server.count("/games/hollow_knight/"), 0);
}

#[test]
fn a_heading_that_names_another_game_is_not_a_match() {
    let server = server("Some Other Game");
    let handler = GoGRevived::with_base(server.base.clone());
    let found = handler
        .find(&query(1245620, "Elden Ring"), &find::client().unwrap())
        .unwrap();
    assert!(found.is_empty());
}

#[test]
fn the_catalog_is_reused_on_the_next_search() {
    let server = server("ELDEN RING");
    let handler = GoGRevived::with_base(server.base.clone());
    let http = find::client().unwrap();
    for _ in 0..2 {
        let found = handler.find(&query(1245620, "Elden Ring"), &http).unwrap();
        assert_eq!(found.len(), 1);
    }
    assert_eq!(server.count("/sitemap-index.xml"), 1, "catalog not re-read");
    assert_eq!(server.count("/sitemap-1.xml"), 1, "sitemap not re-read");
    assert_eq!(server.count("/games/elden_ring/"), 2, "pages still checked");
}
