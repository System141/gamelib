//! External links: storage, site detection and redirect checks against a local HTTP server.

mod common;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

use common::item;
use gamelib_core::db::Db;
use gamelib_core::db::links::{delete_link, get_link, list_links, record_check, save_link};
use gamelib_core::db::read::{query_games, status};
use gamelib_core::db::write::upsert_games;
use gamelib_core::links::resolve::{self, follow_redirects};
use gamelib_core::links::{SiteHandler, SiteRegistry};
use gamelib_core::model::{CheckStatus, GameQuery, LinkInput, LinkKind, Platform, SiteInfo};
use gamelib_core::record::GameRecord;
use gamelib_core::{Error, unix_now};
use reqwest::Url;

const NOW: i64 = 1_790_000_000;

fn db_with_game() -> Db {
    let db = Db::open_in_memory().unwrap();
    let record = GameRecord::from_item(&item(570, "Dota 2", NOW, 10, &[19])).unwrap();
    let other = GameRecord::from_item(&item(730, "Counter-Strike 2", NOW, 20, &[19])).unwrap();
    upsert_games(db.conn(), &[record, other], NOW).unwrap();
    db
}

fn input(url: &str) -> LinkInput {
    LinkInput {
        id: None,
        appid: 570,
        url: url.into(),
        label: Some("  Windows kurulum  ".into()),
        kind: LinkKind::Download,
        platform: Some(Platform::Win),
        version: Some("7.36".into()),
        notes: None,
    }
}

/// A site handler for tests: owns example.org and forces https.
struct TestSite(SiteInfo);

impl SiteHandler for TestSite {
    fn info(&self) -> &SiteInfo {
        &self.0
    }
    fn normalize(&self, mut url: Url) -> Url {
        let _ = url.set_scheme("https");
        url
    }
}

fn registry() -> SiteRegistry {
    SiteRegistry::new(vec![Box::new(TestSite(SiteInfo {
        id: "test-site".into(),
        name: "Test Site".into(),
        homepage: Some("https://example.org".into()),
        domains: vec!["example.org".into()],
        color: "#123456".into(),
    }))])
}

#[test]
fn create_update_delete() {
    let db = db_with_game();
    let sites = registry();
    let link = save_link(
        db.conn(),
        &sites,
        &input("https://files.example.com/dota.zip?utm_source=forum&id=3"),
        NOW,
    )
    .unwrap();
    assert_eq!(link.url, "https://files.example.com/dota.zip?id=3");
    assert_eq!(link.host, "files.example.com");
    assert_eq!(link.site_id, "generic");
    assert_eq!(link.label.as_deref(), Some("Windows kurulum"));
    assert_eq!(link.platform, Some(Platform::Win));
    assert!(!link.insecure);
    assert!(link.last_check.is_none());

    // Record a check, then edit only the label: the check stays.
    let check = follow_redirects(
        &Url::parse(&format!("{}/final", server())).unwrap(),
        &resolve::client().unwrap(),
    );
    record_check(db.conn(), link.id, &check).unwrap();
    let edited = save_link(
        db.conn(),
        &sites,
        &LinkInput {
            id: Some(link.id),
            label: Some("Yeni".into()),
            ..input(&link.url)
        },
        NOW + 1,
    )
    .unwrap();
    assert_eq!(edited.label.as_deref(), Some("Yeni"));
    let summary = edited.last_check.expect("check kept");
    assert_eq!(summary.status, CheckStatus::Ok);
    assert_eq!(summary.file_name.as_deref(), Some("game.zip"));
    assert_eq!(edited.updated_at, NOW + 1);

    // Changing the URL clears it.
    let moved = save_link(
        db.conn(),
        &sites,
        &LinkInput {
            id: Some(link.id),
            ..input("http://mirror.example.com/d.zip")
        },
        NOW + 2,
    )
    .unwrap();
    assert!(moved.last_check.is_none());
    assert!(moved.insecure);

    assert_eq!(list_links(db.conn(), 570).unwrap().len(), 1);
    assert!(delete_link(db.conn(), link.id).unwrap());
    assert!(!delete_link(db.conn(), link.id).unwrap());
    assert!(get_link(db.conn(), link.id).unwrap().is_none());
}

#[test]
fn site_specific_handler_is_detected() {
    let db = db_with_game();
    let link = save_link(
        db.conn(),
        &registry(),
        &input("http://dl.example.org/dota"),
        NOW,
    )
    .unwrap();
    assert_eq!(link.site_id, "test-site");
    assert_eq!(
        link.url, "https://dl.example.org/dota",
        "handler normalisation applied"
    );
    let sites = registry().list();
    assert_eq!(
        sites.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        ["test-site", "generic"]
    );
    assert!(registry().get("generic").is_some());
    assert!(registry().get("nope").is_none());
}

#[test]
fn rejects_bad_input() {
    let db = db_with_game();
    let sites = registry();
    let err = |i: LinkInput| save_link(db.conn(), &sites, &i, NOW).unwrap_err();
    assert!(matches!(
        err(input("javascript:alert(1)")),
        Error::Invalid("url_parse")
    ));
    assert!(matches!(
        err(input("file:///etc/passwd")),
        Error::Invalid("url_scheme")
    ));
    assert!(matches!(
        err(LinkInput {
            appid: 1,
            ..input("https://example.com")
        }),
        Error::NotFound
    ));
    assert!(matches!(
        err(LinkInput {
            label: Some("x".repeat(200)),
            ..input("https://example.com")
        }),
        Error::Invalid("label_too_long")
    ));
    assert!(matches!(
        err(LinkInput {
            id: Some(999),
            ..input("https://example.com")
        }),
        Error::NotFound
    ));
}

#[test]
fn cards_count_links_and_filter() {
    let mut db = db_with_game();
    let sites = registry();
    save_link(db.conn(), &sites, &input("https://a.example.com/1"), NOW).unwrap();
    save_link(db.conn(), &sites, &input("https://b.example.com/2"), NOW).unwrap();
    let linked = query_games(
        &mut db,
        &GameQuery {
            has_links: true,
            ..Default::default()
        },
        NOW,
    )
    .unwrap();
    assert_eq!(linked.total, 1);
    assert_eq!(linked.items[0].appid, 570);
    assert_eq!(linked.items[0].link_count, 2);
    assert_eq!(status(&mut db).unwrap().linked_game_count, 1);
}

// --- redirect checks against a local server -------------------------------------------------

/// Starts a tiny HTTP/1.1 server with fixed routes and returns its base URL.
fn server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let routes_base = base.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let base = routes_base.clone();
            std::thread::spawn(move || handle(stream, &base));
        }
    });
    base
}

fn handle(mut stream: TcpStream, base: &str) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let request = String::from_utf8_lossy(&buf).to_string();
    let mut first = request
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace();
    let method = first.next().unwrap_or_default().to_owned();
    let path = first.next().unwrap_or("/").to_owned();
    let ranged = request
        .to_ascii_lowercase()
        .contains("\r\nrange: bytes=0-0");

    let (status, headers, body): (&str, Vec<String>, &[u8]) = match path.as_str() {
        "/start" => (
            "301 Moved Permanently",
            vec!["Location: /step2".into()],
            b"",
        ),
        "/step2" => ("302 Found", vec![format!("Location: {base}/final")], b""),
        "/final" => (
            "200 OK",
            vec![
                "Content-Type: application/zip".into(),
                "Content-Disposition: attachment; filename=\"game.zip\"".into(),
                "Content-Length: 123456".into(),
            ],
            b"",
        ),
        "/loop" => ("302 Found", vec!["Location: /loop".into()], b""),
        "/nohead" if method == "HEAD" => ("405 Method Not Allowed", vec![], b""),
        "/nohead" if ranged => (
            "206 Partial Content",
            vec![
                "Content-Type: application/octet-stream".into(),
                "Content-Range: bytes 0-0/5000".into(),
            ],
            b"x",
        ),
        "/page" => (
            "200 OK",
            vec!["Content-Type: text/html; charset=utf-8".into()],
            b"<html></html>",
        ),
        "/relative/start" => ("302 Found", vec!["Location: next".into()], b""),
        "/relative/next" => ("200 OK", vec!["Content-Type: text/html".into()], b"ok"),
        "/custom" => (
            "302 Found",
            vec!["Location: magnet:?xt=urn:btih:abc".into()],
            b"",
        ),
        p if p.starts_with("/chain/") => {
            let n: u32 = p["/chain/".len()..].parse().unwrap_or(0);
            (
                "302 Found",
                vec![format!("Location: /chain/{}", n + 1)],
                b"",
            )
        }
        _ => ("404 Not Found", vec![], b""),
    };
    let mut response = format!("HTTP/1.1 {status}\r\nConnection: close\r\n");
    for h in &headers {
        response.push_str(h);
        response.push_str("\r\n");
    }
    if !headers.iter().any(|h| h.starts_with("Content-Length")) {
        response.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    response.push_str("\r\n");
    let _ = stream.write_all(response.as_bytes());
    if method != "HEAD" {
        let _ = stream.write_all(body);
    }
}

fn check(base: &str, path: &str) -> gamelib_core::model::LinkCheck {
    follow_redirects(
        &Url::parse(&format!("{base}{path}")).unwrap(),
        &resolve::client().unwrap(),
    )
}

#[test]
fn follows_redirect_chain_to_a_file() {
    let base = server();
    let c = check(&base, "/start");
    assert_eq!(c.status, CheckStatus::Ok);
    assert_eq!(
        c.hops.iter().map(|h| h.status).collect::<Vec<_>>(),
        [301, 302, 200]
    );
    assert_eq!(c.redirects(), 2);
    assert_eq!(
        c.final_url.as_deref(),
        Some(format!("{base}/final").as_str())
    );
    assert_eq!(c.final_host.as_deref(), Some("127.0.0.1"));
    assert!(c.is_file);
    assert_eq!(c.file_name.as_deref(), Some("game.zip"));
    assert_eq!(c.size_bytes, Some(123_456));
    assert_eq!(c.content_type.as_deref(), Some("application/zip"));
    assert!(c.checked_at >= unix_now() - 5);
}

#[test]
fn falls_back_to_ranged_get_without_reading_the_body() {
    let base = server();
    let c = check(&base, "/nohead");
    assert_eq!(c.status, CheckStatus::Ok);
    assert_eq!(c.http_status, Some(206));
    assert!(c.is_file);
    assert_eq!(c.size_bytes, Some(5000), "total from Content-Range");
}

#[test]
fn web_pages_are_not_files() {
    let base = server();
    let c = check(&base, "/page");
    assert_eq!(c.status, CheckStatus::Ok);
    assert!(!c.is_file);
    assert_eq!(c.size_bytes, None);
    assert_eq!(c.content_type.as_deref(), Some("text/html"));
    let relative = check(&base, "/relative/start");
    assert_eq!(relative.status, CheckStatus::Ok);
    assert_eq!(
        relative.final_url.as_deref(),
        Some(format!("{base}/relative/next").as_str())
    );
}

#[test]
fn reports_broken_links_loops_and_limits() {
    let base = server();
    let missing = check(&base, "/missing");
    assert_eq!(
        (missing.status, missing.http_status),
        (CheckStatus::Broken, Some(404))
    );
    assert_eq!(check(&base, "/loop").status, CheckStatus::Loop);
    let chain = check(&base, "/chain/0");
    assert_eq!(chain.status, CheckStatus::TooManyRedirects);
    assert_eq!(chain.hops.len(), resolve::MAX_REDIRECTS + 1);
    let custom = check(&base, "/custom");
    assert_eq!(custom.status, CheckStatus::UnsupportedScheme);
    assert_eq!(custom.final_url.as_deref(), Some("magnet:?xt=urn:btih:abc"));
}

#[test]
fn unreachable_hosts_are_network_errors() {
    // Port 9 (discard) on localhost is closed in test environments.
    let c = follow_redirects(
        &Url::parse("http://127.0.0.1:9/x").unwrap(),
        &resolve::client().unwrap(),
    );
    assert!(
        matches!(c.status, CheckStatus::Network | CheckStatus::Timeout),
        "{:?}",
        c.status
    );
    assert!(c.message.is_some());
}
