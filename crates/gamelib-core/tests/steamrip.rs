//! SteamRIP handler: the REST search against a local server, where the post's declared Steam
//! appid and address are the confirmation.

mod common;

use common::http::{Request, Response, TestServer};
use gamelib_core::links::SiteHandler;
use gamelib_core::links::find::{self, FindQuery};
use gamelib_core::links::sites::steamrip::SteamRIP;
use gamelib_core::model::LinkKind;

/// A fake SteamRIP: the REST search serves `posts` as its JSON array, with `{base}` in the
/// addresses.
fn server(posts: &str) -> TestServer {
    let posts = posts.to_owned();
    TestServer::start(move |req: &Request| {
        let base = format!("http://{}", req.header("host").unwrap_or_default());
        match req.path.as_str() {
            "/wp-json/wp/v2/posts" => Response::json(posts.replace("{base}", &base)),
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
fn the_post_with_the_target_appid_is_selected() {
    let server = server(
        r#"[
            {"link": "{base}/other-game-free-download/", "acf": {"steam_app_id": 999}},
            {"link": "{base}/elden-ring-free-download/", "acf": {"steam_app_id": "1245620"}},
            {"link": "{base}/elden-ring-second-free-download/", "acf": {"steam_app_id": 1245620}}
        ]"#,
    );
    let handler = SteamRIP::with_base(server.base.clone());
    let found = handler
        .find(&query(1245620, "Elden Ring"), &find::client().unwrap())
        .unwrap();
    assert_eq!(found.len(), 1);
    let link = &found[0];
    assert_eq!(link.site_id, "steamrip");
    assert_eq!(
        link.url,
        format!("{}/elden-ring-free-download/", server.base)
    );
    assert_eq!(link.label, "SteamRIP");
    assert_eq!(link.kind, LinkKind::Page);
    assert_eq!(link.version, None);
    assert_eq!(link.size, None);
    assert_eq!(link.notes, None);
    assert_eq!(link.score, 1.0);
    assert!(link.needs_browser);
    assert!(!link.direct);
    // The post carries everything the answer needs, so no page is fetched.
    assert_eq!(server.requests().len(), 1);
    let search = &server.requests()[0];
    assert_eq!(search.param("search"), Some("Elden Ring"));
    assert_eq!(search.param("per_page"), Some("100"));
    assert_eq!(search.param("_fields"), Some("link,acf"));
}

#[test]
fn posts_without_the_appid_or_from_another_host_are_skipped() {
    let server = server(
        r#"[
            {"link": "{base}/no-fields-free-download/", "acf": []},
            {"link": "{base}/missing-appid-free-download/"},
            {"link": "https://elsewhere.example/elden-ring-free-download/", "acf": {"steam_app_id": 1245620}},
            {"link": "{base}/elden-ring-free-download/", "acf": {"steam_app_id": 1245620}}
        ]"#,
    );
    let handler = SteamRIP::with_base(server.base.clone());
    let found = handler
        .find(&query(1245620, "Elden Ring"), &find::client().unwrap())
        .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].url,
        format!("{}/elden-ring-free-download/", server.base)
    );
}

#[test]
fn no_matching_appid_is_an_empty_result() {
    let server = server(
        r#"[
            {"link": "{base}/other-game-free-download/", "acf": {"steam_app_id": 999}},
            {"link": "{base}/no-appid-free-download/", "acf": []}
        ]"#,
    );
    let handler = SteamRIP::with_base(server.base.clone());
    let found = handler
        .find(&query(1245620, "Elden Ring"), &find::client().unwrap())
        .unwrap();
    assert!(found.is_empty());
}
