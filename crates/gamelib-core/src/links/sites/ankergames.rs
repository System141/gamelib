//! AnkerGames: pre-installed PC game downloads, found by title slug and confirmed by the Steam
//! appid the game's page links to. The site's sitemaps are gone, so there is no crawl index
//! left to search; a page that cannot be reached by slug is simply not offered.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use reqwest::blocking::Client;

use crate::Error;
use crate::Result;
use crate::http::{Counters, Pacer};
use crate::links::SiteHandler;
use crate::links::find::{self, FindQuery};
use crate::model::{FoundLink, LinkKind, SiteInfo};

/// Space requests to the site, which sits behind Cloudflare.
const PACE: Duration = Duration::from_millis(250);
const SITE_ID: &str = "ankergames";

/// Finds the game's page by its title slug and confirms it by the page's Steam appid.
pub struct AnkerGames {
    info: SiteInfo,
    base: String,
}

impl AnkerGames {
    pub fn new() -> Self {
        Self::with_base("https://ankergames.net".into())
    }

    /// Points the handler at a test server instead of the real site.
    pub fn with_base(base: String) -> Self {
        Self {
            info: SiteInfo {
                id: SITE_ID.into(),
                name: "AnkerGames".into(),
                homepage: Some("https://ankergames.net".into()),
                domains: vec!["ankergames.net".into(), "www.ankergames.net".into()],
                color: "#4f5b93".into(),
                browser_required: true,
            },
            base,
        }
    }
}

impl Default for AnkerGames {
    fn default() -> Self {
        Self::new()
    }
}

impl SiteHandler for AnkerGames {
    fn info(&self) -> &SiteInfo {
        &self.info
    }

    fn find(&self, query: &FindQuery, http: &Client) -> Result<Vec<FoundLink>> {
        let cancel = AtomicBool::new(false);
        let counters = Counters::default();
        let pacer = Pacer::new(PACE);
        for slug in slugs(&query.title) {
            let url = format!("{}/game/{slug}", self.base);
            let page = match fetch(&url, http, &pacer, &cancel, &counters) {
                Ok(page) => page,
                // A slug the catalog no longer has is a miss, not a failure of the search.
                Err(Error::Http { status: 404, .. }) => continue,
                Err(e) => return Err(e),
            };
            // The page's own Steam link is the confirmation, so a same-titled page for another
            // game or edition is never offered.
            if steam_appid(&page) != Some(query.appid) {
                continue;
            }
            return Ok(vec![FoundLink {
                site_id: SITE_ID.into(),
                url,
                label: "AnkerGames".into(),
                kind: LinkKind::Page,
                version: version_from_title(&page),
                size: size_from_description(&page),
                notes: Some("download needs a browser verification step".into()),
                score: 1.0,
                needs_browser: true,
                direct: false,
            }]);
        }
        Ok(Vec::new())
    }
}

/// The page slugs to try for a title, in order. The site builds slugs from its own stored title,
/// which spells some apostrophes as a dash (`sid-meier-s-civilization-vi`) and others as nothing
/// (`assassins-creed-iv-black-flag`), so both spellings get at most one request each.
fn slugs(title: &str) -> Vec<String> {
    let exact = find::slugify(title);
    if exact.is_empty() {
        return Vec::new();
    }
    let dropped = find::slugify(&title.replace(['\'', '\u{2019}'], ""));
    if dropped.is_empty() || dropped == exact {
        vec![exact]
    } else {
        vec![exact, dropped]
    }
}

/// The first Steam app id the page links to.
fn steam_appid(page: &str) -> Option<u32> {
    let needle = "store.steampowered.com/app/";
    let from = page.find(needle)? + needle.len();
    let digits: String = page[from..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// The parenthesised part of the `<title>`, when it looks like a version.
fn version_from_title(page: &str) -> Option<String> {
    let title = find::extract_between(page, "<title>", "</title>")?;
    let paren = find::extract_between(title, "(", ")")?.trim();
    let looks_like_version = paren
        .strip_prefix('v')
        .or_else(|| paren.strip_prefix('V'))
        .is_some_and(|rest| rest.chars().next().is_some_and(|c| c.is_ascii_digit()));
    looks_like_version.then(|| find::decode_entities(paren))
}

/// The size in the meta description's trailing "(version, size)" group.
fn size_from_description(page: &str) -> Option<String> {
    let desc = find::extract_between(page, r#"name="description" content=""#, "\"")?;
    let paren = last_paren(desc)?;
    let size = paren.rsplit(',').next()?.trim();
    (size.ends_with("GB") || size.ends_with("MB")).then(|| size.to_owned())
}

/// The last parenthesised group, where the description puts version and size.
fn last_paren(s: &str) -> Option<&str> {
    let open = s.rfind('(')?;
    let rest = &s[open + 1..];
    let close = rest.find(')')?;
    Some(&rest[..close])
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
