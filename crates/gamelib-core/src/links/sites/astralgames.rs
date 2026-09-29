//! AstralGames: game pages found by title slug, confirmed by the Steam appid the page carries.
//! The site's search runs client-side, so a page that cannot be reached by slug is simply not
//! offered.

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
const SITE_ID: &str = "astralgames";

/// Finds the game's page by its title slug and confirms it by the page's Steam appid.
pub struct AstralGames {
    info: SiteInfo,
    base: String,
}

impl AstralGames {
    pub fn new() -> Self {
        Self::with_base("https://astralgames.net".into())
    }

    /// Points the handler at a test server instead of the real site.
    pub fn with_base(base: String) -> Self {
        Self {
            info: SiteInfo {
                id: SITE_ID.into(),
                name: "AstralGames".into(),
                homepage: Some("https://astralgames.net".into()),
                domains: vec!["astralgames.net".into()],
                color: "#8b5cf6".into(),
                browser_required: true,
            },
            base,
        }
    }
}

impl Default for AstralGames {
    fn default() -> Self {
        Self::new()
    }
}

impl SiteHandler for AstralGames {
    fn info(&self) -> &SiteInfo {
        &self.info
    }

    fn find(&self, query: &FindQuery, http: &Client) -> Result<Vec<FoundLink>> {
        let slug = find::slugify(&query.title);
        if slug.is_empty() {
            return Ok(Vec::new());
        }
        let cancel = AtomicBool::new(false);
        let counters = Counters::default();
        let pacer = Pacer::new(PACE);
        let url = format!("{}/game/{slug}", self.base);
        let page = match fetch(&url, http, &pacer, &cancel, &counters) {
            Ok(page) => page,
            // The catalog has no page under this slug.
            Err(Error::Http { status: 404, .. }) => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        // The page's own Steam appid is the confirmation, so a same-titled page for another
        // game or edition is never offered.
        if steam_appid(&page) != Some(query.appid) {
            return Ok(Vec::new());
        }
        Ok(vec![FoundLink {
            site_id: SITE_ID.into(),
            url,
            label: "AstralGames".into(),
            kind: LinkKind::Page,
            version: None,
            size: None,
            notes: None,
            score: 1.0,
            needs_browser: true,
            direct: false,
        }])
    }
}

/// The Steam app id the page carries: its `app_<id>_` asset names first, then a
/// `steam/apps/<id>` URL.
fn steam_appid(page: &str) -> Option<u32> {
    appid_after(page, "app_").or_else(|| appid_after(page, "steam/apps/"))
}

/// The first `<marker><digits>` run on the page.
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
