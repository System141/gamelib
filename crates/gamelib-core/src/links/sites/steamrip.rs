//! SteamRIP: WordPress game pages, found through the site's REST search, which carries each
//! post's Steam appid in its custom fields. No page has to be fetched to confirm a result.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use reqwest::Url;
use reqwest::blocking::Client;
use serde::Deserialize;

use crate::Error;
use crate::Result;
use crate::http::{Counters, Pacer};
use crate::links::SiteHandler;
use crate::links::find::FindQuery;
use crate::model::{FoundLink, LinkKind, SiteInfo};

/// Space requests to the site, which sits behind Cloudflare.
const PACE: Duration = Duration::from_millis(250);
const SITE_ID: &str = "steamrip";
/// The fields asked of the REST search: the post address and its custom fields.
const FIELDS: &str = "link,acf";

/// Searches SteamRIP's REST API and takes the post whose declared Steam appid matches.
pub struct SteamRIP {
    info: SiteInfo,
    base: String,
}

impl SteamRIP {
    pub fn new() -> Self {
        Self::with_base("https://steamrip.com".into())
    }

    /// Points the handler at a test server instead of the real site.
    pub fn with_base(base: String) -> Self {
        Self {
            info: SiteInfo {
                id: SITE_ID.into(),
                name: "SteamRIP".into(),
                homepage: Some("https://steamrip.com".into()),
                domains: vec!["steamrip.com".into()],
                color: "#7c3aed".into(),
                browser_required: false,
            },
            base,
        }
    }
}

impl Default for SteamRIP {
    fn default() -> Self {
        Self::new()
    }
}

/// One `wp/v2/posts` row, narrowed to the fields the search asks for. The custom-field block
/// also comes back as an empty array when a post has none set, so it stays untyped.
#[derive(Deserialize)]
struct Post {
    link: Option<String>,
    #[serde(default)]
    acf: Option<serde_json::Value>,
}

impl SiteHandler for SteamRIP {
    fn info(&self) -> &SiteInfo {
        &self.info
    }

    fn find(&self, query: &FindQuery, http: &Client) -> Result<Vec<FoundLink>> {
        let cancel = AtomicBool::new(false);
        let counters = Counters::default();
        let pacer = Pacer::new(PACE);
        let mut url = Url::parse(&format!("{}/wp-json/wp/v2/posts", self.base))
            .map_err(|_| Error::Invalid("url_parse"))?;
        url.query_pairs_mut()
            .append_pair("search", &query.title)
            .append_pair("per_page", "100")
            .append_pair("_fields", FIELDS);
        pacer.wait(&cancel)?;
        let posts: Vec<Post> =
            crate::http::get_json(http, url.as_str(), |r| r, &cancel, &counters)?;
        let base_host = host_of(&self.base);
        for post in posts {
            // A post whose address does not belong to the site is not offered, even when the
            // appid matches.
            let Some(link) = post.link else { continue };
            if base_host.is_none() || host_of(&link) != base_host {
                continue;
            }
            if post
                .acf
                .as_ref()
                .and_then(steam_app_id)
                .is_some_and(|appid| appid == query.appid)
            {
                return Ok(vec![FoundLink {
                    site_id: SITE_ID.into(),
                    url: link,
                    label: "SteamRIP".into(),
                    kind: LinkKind::Page,
                    version: None,
                    size: None,
                    notes: None,
                    score: 1.0,
                    needs_browser: true,
                    direct: false,
                }]);
            }
        }
        Ok(Vec::new())
    }
}

/// The Steam appid declared in a post's custom fields, served as a number or a string.
fn steam_app_id(acf: &serde_json::Value) -> Option<u32> {
    match acf.get("steam_app_id")? {
        serde_json::Value::Number(n) => n.as_u64().and_then(|n| u32::try_from(n).ok()),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// The host of an http(s) URL, lowercased, with its port kept.
fn host_of(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next()?;
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}
