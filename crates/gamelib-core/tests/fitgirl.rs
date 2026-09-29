//! FitGirl Repacks handler: WordPress search, magnet extraction and mirror hosts, against a local server.

mod common;

use common::http::{Request, Response, TestServer};
use gamelib_core::links::SiteHandler;
use gamelib_core::links::find::{self, FindQuery};
use gamelib_core::links::sites::fitgirl::FitGirl;
use gamelib_core::model::LinkKind;

/// The base URL of the server a request came in on.
fn base(req: &Request) -> String {
    format!("http://{}", req.header("host").unwrap())
}

/// A fake FitGirl Repacks: a WordPress search page and two game pages.
fn server() -> TestServer {
    TestServer::start(|req: &Request| {
        let base = base(req);
        match req.path.as_str() {
            "/" if req.param("s").is_some() => Response::json(search_page(&base)),
            "/the-witcher-3-wild-hunt-complete-edition/" => Response::json(witcher_page()),
            "/cyberpunk-2077/" => Response::json(cyberpunk_page()),
            _ => Response::status(404),
        }
    })
}

/// The search results page: two posts whose titles carry version markers.
fn search_page(base: &str) -> String {
    format!(
        "<html><body>\
         <h1 class=\"entry-title\"><a href=\"{base}/the-witcher-3-wild-hunt-complete-edition/\" rel=\"bookmark\">The Witcher 3: Wild Hunt &#8211; Complete Edition &#8211; GOG/Steam v4.00 + All DLCs + Bonus Content</a></h1>\
         <h2 class=\"entry-title\"><a href=\"{base}/cyberpunk-2077/\" rel=\"bookmark\">Cyberpunk 2077: Ultimate Edition &#8211; v2.3 + All DLCs + Bonus Content + REDmod</a></h2>\
         </body></html>"
    )
}

/// A game page with a magnet (entity-encoded ampersands), a second magnet, size and mirrors.
fn witcher_page() -> &'static str {
    "<html><head><title>The Witcher 3: Wild Hunt - Complete Edition - GOG/Steam v4.00 + All DLCs + Bonus Content - FitGirl Repacks</title></head>\
     <body>\
     <p>Repack Size: <strong>from 31.7 GB</strong> [Selective Download]</p>\
     <a href=\"magnet:?xt=urn:btih:7F9F2EA2A2BD89C65D14ED816987938FD5D48B07&#038;dn=witcher&#038;tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337\">magnet</a>\
     <a href=\"magnet:?xt=urn:btih:SECONDMAGNET&#038;dn=mirror\">second magnet</a>\
     <h3>Download Mirrors (Direct Links)</h3>\
     <a href=\"https://paste.fitgirl-repacks.site/?abc#xyz\">Filehoster: DataNodes</a>\
     <a href=\"https://www.internetdownloadmanager.com/download.html\">IDM</a>\
     <a href=\"https://datanodes.to/abc\">datanodes</a>\
     <a href=\"https://1337x.to/torrent/xyz\">1337x</a>\
     <a href=\"https://datanodes.to/def\">datanodes again</a>\
     <h3>Download Mirrors (Torrent)</h3>\
     <a href=\"https://fuckingfast.co/ghi\">fuckingfast</a>\
     </body></html>"
}

/// A game page with a magnet, size and version, but no mirrors.
fn cyberpunk_page() -> &'static str {
    "<html><head><title>Cyberpunk 2077: Ultimate Edition - v2.3 + All DLCs + Bonus Content + REDmod - FitGirl Repacks</title></head>\
     <body>\
     <p>Repack Size: <strong>from 92.4 GB</strong></p>\
     <a href=\"magnet:?xt=urn:btih:CP2077BTIH&#038;dn=cp\">magnet</a>\
     </body></html>"
}

fn query(appid: u32, title: &str) -> FindQuery {
    FindQuery {
        appid,
        title: title.into(),
    }
}

#[test]
fn matching_result_yields_magnet() {
    let server = server();
    let handler = FitGirl::with_base(server.base.clone());
    let found = handler
        .find(
            &query(292030, "The Witcher 3: Wild Hunt"),
            &find::client().unwrap(),
        )
        .unwrap();
    assert!(!found.is_empty());
    let magnet = &found[0];
    assert_eq!(magnet.site_id, "fitgirl");
    assert_eq!(magnet.kind, LinkKind::Download);
    assert!(magnet.direct);
    assert!(!magnet.needs_browser);
    assert_eq!(
        magnet.url,
        "magnet:?xt=urn:btih:7F9F2EA2A2BD89C65D14ED816987938FD5D48B07&dn=witcher&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337"
    );
    assert!(!magnet.url.contains("&#038;"));
    assert_eq!(
        magnet.version.as_deref(),
        Some("v4.00 + All DLCs + Bonus Content")
    );
    assert_eq!(magnet.size.as_deref(), Some("from 31.7 GB"));
    // The mirrors follow the magnet, one per distinct host, below it in score.
    let mirrors = &found[1..];
    assert_eq!(mirrors.len(), 2);
    assert_eq!(mirrors[0].label, "datanodes.to");
    assert_eq!(mirrors[0].url, "https://datanodes.to/abc");
    assert_eq!(mirrors[0].kind, LinkKind::Page);
    assert!(!mirrors[0].direct);
    assert!(mirrors[0].needs_browser);
    assert!(mirrors[0].score < magnet.score);
    assert_eq!(mirrors[1].label, "1337x.to");
    assert_eq!(mirrors[1].url, "https://1337x.to/torrent/xyz");
}

#[test]
fn non_matching_results_yield_nothing() {
    let server = server();
    let handler = FitGirl::with_base(server.base.clone());
    let found = handler
        .find(&query(620, "Portal 2"), &find::client().unwrap())
        .unwrap();
    assert!(found.is_empty());
}

#[test]
fn only_the_first_magnet_is_returned() {
    let server = server();
    let handler = FitGirl::with_base(server.base.clone());
    let found = handler
        .find(
            &query(292030, "The Witcher 3: Wild Hunt"),
            &find::client().unwrap(),
        )
        .unwrap();
    assert!(
        found[0]
            .url
            .starts_with("magnet:?xt=urn:btih:7F9F2EA2A2BD89C65D14ED816987938FD5D48B07")
    );
    assert!(!found.iter().any(|l| l.url.contains("SECONDMAGNET")));
}

#[test]
fn size_and_version_come_from_the_page() {
    let server = server();
    let handler = FitGirl::with_base(server.base.clone());
    let found = handler
        .find(&query(1091500, "Cyberpunk 2077"), &find::client().unwrap())
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].version.as_deref(),
        Some("v2.3 + All DLCs + Bonus Content + REDmod")
    );
    assert_eq!(found[0].size.as_deref(), Some("from 92.4 GB"));
}

#[test]
fn mirror_noise_is_filtered_out() {
    let server = server();
    let handler = FitGirl::with_base(server.base.clone());
    let found = handler
        .find(
            &query(292030, "The Witcher 3: Wild Hunt"),
            &find::client().unwrap(),
        )
        .unwrap();
    let hosts: Vec<&str> = found.iter().map(|f| f.label.as_str()).collect();
    // The IDM advert and the site's own paste landing page are not download mirrors.
    assert!(!hosts.iter().any(|h| h.contains("internetdownloadmanager")));
    assert!(
        !hosts
            .iter()
            .any(|h| h.contains("paste.fitgirl-repacks.site"))
    );
    // The real file hosts survive, one entry each.
    assert!(hosts.contains(&"datanodes.to"));
    assert!(hosts.contains(&"1337x.to"));
    assert_eq!(hosts.iter().filter(|h| **h == "datanodes.to").count(), 1);
}
