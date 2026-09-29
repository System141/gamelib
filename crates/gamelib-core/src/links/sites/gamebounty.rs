//! GameBounty: game pages found through the site's search and confirmed by the Steam appid in
//! the page's cover image. Which game a card is for is decided by title before the appid
//! confirms it, so a card for a namesake is never offered.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use reqwest::Url;
use reqwest::blocking::Client;

use crate::Error;
use crate::Result;
use crate::http::{Counters, Pacer};
use crate::links::SiteHandler;
use crate::links::find::{self, FindQuery};
use crate::model::{FoundLink, LinkKind, SiteInfo};
use crate::stores::matching;

/// Space requests to the site, which sits behind Cloudflare.
const PACE: Duration = Duration::from_millis(250);
const SITE_ID: &str = "gamebounty";
/// At most this many candidate pages are checked.
const MAX_CANDIDATES: usize = 3;

/// Finds the game's page through the search and confirms it by the page's Steam appid.
pub struct GameBounty {
    info: SiteInfo,
    base: String,
}

impl GameBounty {
    pub fn new() -> Self {
        Self::with_base("https://gamebounty.world".into())
    }

    /// Points the handler at a test server instead of the real site.
    pub fn with_base(base: String) -> Self {
        Self {
            info: SiteInfo {
                id: SITE_ID.into(),
                name: "GameBounty".into(),
                homepage: Some("https://gamebounty.world".into()),
                domains: vec!["gamebounty.world".into()],
                color: "#f59e0b".into(),
                browser_required: false,
            },
            base,
        }
    }
}

impl Default for GameBounty {
    fn default() -> Self {
        Self::new()
    }
}

impl SiteHandler for GameBounty {
    fn info(&self) -> &SiteInfo {
        &self.info
    }

    fn find(&self, query: &FindQuery, http: &Client) -> Result<Vec<FoundLink>> {
        let cancel = AtomicBool::new(false);
        let counters = Counters::default();
        let pacer = Pacer::new(PACE);
        let mut url = Url::parse(&format!("{}/search", self.base))
            .map_err(|_| Error::Invalid("url_parse"))?;
        url.query_pairs_mut().append_pair("q", &query.title);
        let search = fetch(url.as_str(), http, &pacer, &cancel, &counters)?;
        let mut candidates = cards(&search, &self.base, &query.title);
        if candidates.is_empty() {
            // The search page can come back as an empty shell; every card address ends in
            // `-free-pc-download`, so the title's own slug is the one guess left.
            let slug = find::slugify(&query.title);
            if slug.is_empty() {
                return Ok(Vec::new());
            }
            candidates.push(format!("{}/{slug}-free-pc-download", self.base));
        }
        for candidate in candidates {
            let page = match fetch(&candidate, http, &pacer, &cancel, &counters) {
                Ok(page) => page,
                // A page that is gone is a miss; the next candidate still gets its turn.
                Err(Error::Http { status: 404, .. }) => continue,
                Err(e) => return Err(e),
            };
            // The cover image's Steam appid is the confirmation.
            if appid_after(&page, "steam/apps/") != Some(query.appid) {
                continue;
            }
            return Ok(vec![FoundLink {
                site_id: SITE_ID.into(),
                url: candidate,
                label: "GameBounty".into(),
                kind: LinkKind::Page,
                version: None,
                size: None,
                notes: None,
                score: 1.0,
                needs_browser: true,
                direct: false,
            }]);
        }
        Ok(Vec::new())
    }
}

/// The card URLs whose displayed title names this game, in page order, at most
/// [`MAX_CANDIDATES`].
fn cards(page: &str, base: &str, title: &str) -> Vec<String> {
    let wanted = matching::canonical_title(title);
    let mut out: Vec<String> = Vec::new();
    let Some(base_host) = host_of(base) else {
        return out;
    };
    for markup in page.split("href=\"").skip(1) {
        let Some(attribute) = markup.split('"').next() else {
            continue;
        };
        let url = find::decode_entities(attribute);
        let Some(host) = host_of(&url) else {
            continue;
        };
        if host != base_host && !host.ends_with(&format!(".{base_host}")) {
            continue;
        }
        if !path_of(&url).is_some_and(|p| p.trim_end_matches('/').ends_with("-free-pc-download")) {
            continue;
        }
        let Some(text) = find::extract_between(markup, ">", "</a>") else {
            continue;
        };
        let name = find::decode_entities(&strip_tags(text));
        if matching::canonical_title(name.trim()) != wanted {
            continue;
        }
        if !out.contains(&url) {
            out.push(url);
        }
        if out.len() == MAX_CANDIDATES {
            break;
        }
    }
    out
}

/// The first Steam app id after `<marker>` on the page.
fn appid_after(page: &str, marker: &str) -> Option<u32> {
    let mut rest = page;
    while let Some(from) = rest.find(marker) {
        let after = &rest[from + marker.len()..];
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(appid) = digits.parse() {
            return Some(appid);
        }
        rest = after;
    }
    None
}

/// The visible text of a markup fragment, without its tags.
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// The host of an http(s) URL, lowercased, with its port kept.
fn host_of(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next()?;
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// The path of an http(s) URL.
fn path_of(url: &str) -> Option<&str> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let slash = rest.find('/')?;
    let path = &rest[slash..];
    Some(&path[..path.find(['?', '#']).unwrap_or(path.len())])
}

/// One paced GET through the shared retry/backoff plumbing.
fn fetch(
    url: &str,
    http: &Client,
    pacer: &Pacer,
    cancel: &AtomicBool,
    counters: &Counters,
) -> Result<String> {
    pacer.wait(cancel)?;
    crate::http::get_text(http, url, |r| r, cancel, counters)
}
