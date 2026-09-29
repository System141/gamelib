//! GoG Revived: game pages indexed by the site's own sitemaps. The pages carry no store id, so
//! a result is only offered when the page's heading matches the game's canonical title, at a
//! lower score than the sites that confirm with a Steam appid.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reqwest::blocking::Client;

use crate::Result;
use crate::http::{Counters, Pacer};
use crate::links::SiteHandler;
use crate::links::find::{self, FindQuery};
use crate::model::{FoundLink, LinkKind, SiteInfo};
use crate::stores::matching;

/// Space requests to the site, which sits behind Cloudflare.
const PACE: Duration = Duration::from_millis(250);
const SITE_ID: &str = "gog-rev";
/// How long the sitemap catalog is reused before it is downloaded again.
const CATALOG_TTL: Duration = Duration::from_secs(6 * 60 * 60);
/// At most this many title candidates are checked.
const MAX_CANDIDATES: usize = 3;
/// A page confirmed only by its own heading is not a store-confirmed match.
const SCORE: f32 = 0.75;

/// Finds the game's page through the sitemaps and confirms it by the page's heading.
pub struct GoGRevived {
    info: SiteInfo,
    base: String,
    catalog: Mutex<Option<(Instant, Arc<Vec<String>>)>>,
}

impl GoGRevived {
    pub fn new() -> Self {
        Self::with_base("https://gog-rev.com".into())
    }

    /// Points the handler at a test server instead of the real site.
    pub fn with_base(base: String) -> Self {
        Self {
            info: SiteInfo {
                id: SITE_ID.into(),
                name: "GoG Revived".into(),
                homepage: Some("https://gog-rev.com".into()),
                domains: vec!["gog-rev.com".into()],
                color: "#ef4444".into(),
                browser_required: true,
            },
            base,
            catalog: Mutex::new(None),
        }
    }
}

impl Default for GoGRevived {
    fn default() -> Self {
        Self::new()
    }
}

impl SiteHandler for GoGRevived {
    fn info(&self) -> &SiteInfo {
        &self.info
    }

    fn find(&self, query: &FindQuery, http: &Client) -> Result<Vec<FoundLink>> {
        let cancel = AtomicBool::new(false);
        let counters = Counters::default();
        let pacer = Pacer::new(PACE);
        let catalog = self.catalog(http, &pacer, &cancel, &counters)?;
        let wanted = matching::canonical_title(&query.title);
        let mut candidates: Vec<String> = Vec::new();
        for url in catalog.iter() {
            if title_of(url).is_none_or(|title| title != wanted) {
                continue;
            }
            candidates.push(url.clone());
            if candidates.len() == MAX_CANDIDATES {
                break;
            }
        }
        for url in candidates {
            let page = fetch(&url, http, &pacer, &cancel, &counters)?;
            if heading(&page).is_some_and(|h| matching::canonical_title(&h) == wanted) {
                return Ok(vec![FoundLink {
                    site_id: SITE_ID.into(),
                    url,
                    label: "GoG Revived".into(),
                    kind: LinkKind::Page,
                    version: None,
                    size: None,
                    notes: None,
                    score: SCORE,
                    needs_browser: true,
                    direct: false,
                }]);
            }
        }
        Ok(Vec::new())
    }
}

impl GoGRevived {
    /// The cached game-page addresses, downloaded from the sitemaps when missing or stale. The
    /// catalog only changes when the site adds games, so a search never re-reads it.
    fn catalog(
        &self,
        http: &Client,
        pacer: &Pacer,
        cancel: &AtomicBool,
        counters: &Counters,
    ) -> Result<Arc<Vec<String>>> {
        let mut cache = self.catalog.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, urls)) = cache.as_ref()
            && !urls.is_empty()
            && at.elapsed() < CATALOG_TTL
        {
            return Ok(urls.clone());
        }
        let urls = Arc::new(self.load_catalog(http, pacer, cancel, counters)?);
        if !urls.is_empty() {
            *cache = Some((Instant::now(), urls.clone()));
        }
        Ok(urls)
    }

    /// Downloads the sitemap index and every sitemap it lists, keeping the game pages.
    fn load_catalog(
        &self,
        http: &Client,
        pacer: &Pacer,
        cancel: &AtomicBool,
        counters: &Counters,
    ) -> Result<Vec<String>> {
        let index = fetch(
            &format!("{}/sitemap-index.xml", self.base),
            http,
            pacer,
            cancel,
            counters,
        )?;
        let mut urls: Vec<String> = Vec::new();
        for sitemap in locs(&index, &self.base, None) {
            let body = fetch(&sitemap, http, pacer, cancel, counters)?;
            for url in locs(&body, &self.base, Some("/games/")) {
                if !urls.contains(&url) {
                    urls.push(url);
                }
            }
        }
        Ok(urls)
    }
}

/// The `<loc>` URLs of a sitemap that belong to the site's host, optionally restricted to a
/// path prefix. Broken entries and foreign hosts are skipped.
fn locs(xml: &str, base: &str, prefix: Option<&str>) -> Vec<String> {
    let Some(base_host) = host_of(base) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for piece in xml.split("<loc>").skip(1) {
        let Some(raw) = piece.split("</loc>").next() else {
            continue;
        };
        let url = find::decode_entities(raw.trim());
        if host_of(&url).as_deref() != Some(base_host.as_str()) {
            continue;
        }
        if prefix.is_some_and(|p| !path_of(&url).is_some_and(|path| path.starts_with(p))) {
            continue;
        }
        if !out.contains(&url) {
            out.push(url);
        }
    }
    out
}

/// The canonical title a game-page URL claims: its last path segment with underscores as
/// spaces.
fn title_of(url: &str) -> Option<String> {
    let path = path_of(url)?;
    let slug = path.trim_end_matches('/').rsplit('/').next()?;
    (!slug.is_empty()).then(|| matching::canonical_title(&slug.replace('_', " ")))
}

/// The visible text of the page's first `<h1>`.
fn heading(page: &str) -> Option<String> {
    let open = page.find("<h1")?;
    let text = find::extract_between(&page[open..], ">", "</h1>")?;
    let text = find::decode_entities(&strip_tags(text));
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
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
