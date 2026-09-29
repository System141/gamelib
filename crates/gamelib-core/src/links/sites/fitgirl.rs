//! FitGirl Repacks: repacked game downloads on a WordPress site, found through its search.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use reqwest::Url;
use reqwest::blocking::Client;

use crate::http::{Counters, Pacer};
use crate::links::SiteHandler;
use crate::links::find::{self, FindQuery};
use crate::model::{FoundLink, LinkKind, SiteInfo};
use crate::stores::matching;
use crate::{Error, Result};

/// Space requests to the site, which sits behind Cloudflare.
const PACE: Duration = Duration::from_millis(250);
const SITE_ID: &str = "fitgirl";
/// Mirrors are secondary to the magnet, so they score below it.
const MIRROR_SCORE: f32 = 0.5;
/// At most this many search results are considered.
const MAX_RESULTS: usize = 3;

/// Searches FitGirl Repacks' WordPress search and takes the magnet from the best game page.
pub struct FitGirl {
    info: SiteInfo,
    base: String,
}

impl FitGirl {
    pub fn new() -> Self {
        Self::with_base("https://fitgirl-repacks.site".into())
    }

    /// Points the handler at a test server instead of the real site.
    pub fn with_base(base: String) -> Self {
        Self {
            info: SiteInfo {
                id: SITE_ID.into(),
                name: "FitGirl Repacks".into(),
                homepage: Some("https://fitgirl-repacks.site".into()),
                domains: vec![
                    "fitgirl-repacks.site".into(),
                    "www.fitgirl-repacks.site".into(),
                ],
                color: "#e91e8c".into(),
                browser_required: false,
            },
            base,
        }
    }
}

impl Default for FitGirl {
    fn default() -> Self {
        Self::new()
    }
}

impl SiteHandler for FitGirl {
    fn info(&self) -> &SiteInfo {
        &self.info
    }

    fn find(&self, query: &FindQuery, http: &Client) -> Result<Vec<FoundLink>> {
        let cancel = AtomicBool::new(false);
        let counters = Counters::default();
        let pacer = Pacer::new(PACE);
        let mut url =
            Url::parse(&format!("{}/", self.base)).map_err(|_| Error::Invalid("url_parse"))?;
        url.query_pairs_mut().append_pair("s", &query.title);
        let search = fetch(url.as_str(), http, &pacer, &cancel, &counters)?;
        let mut results = rank(&search, &query.title);
        results.truncate(MAX_RESULTS);
        let Some((url, score)) = results.into_iter().next() else {
            return Ok(Vec::new());
        };
        let page = fetch(&url, http, &pacer, &cancel, &counters)?;
        let mut out = Vec::new();
        if let Some(magnet) = magnet_link(&page) {
            out.push(FoundLink {
                site_id: SITE_ID.into(),
                url: magnet,
                label: "FitGirl Repacks".into(),
                kind: LinkKind::Download,
                version: version_from_title(&page),
                size: repack_size(&page),
                notes: None,
                score,
                needs_browser: false,
                direct: true,
            });
        }
        for (host, url) in mirror_links(&page) {
            out.push(FoundLink {
                site_id: SITE_ID.into(),
                url,
                label: host,
                kind: LinkKind::Page,
                version: None,
                size: None,
                notes: None,
                score: MIRROR_SCORE,
                needs_browser: true,
                direct: false,
            });
        }
        Ok(out)
    }
}

/// The search results whose game name canonically matches the query, best first.
fn rank(page: &str, title: &str) -> Vec<(String, f32)> {
    let steam_title = matching::canonical_title(title);
    let steam_key = matching::MatchKey::new(std::iter::empty(), [None::<i64>]);
    let mut out: Vec<(String, f32)> = search_results(page)
        .into_iter()
        .map(|(url, result_title)| {
            let key = matching::MatchKey::new(std::iter::empty(), [None::<i64>]);
            let score = if matching::canonical_title(&game_name(&result_title)) == steam_title {
                matching::score(&key, &steam_key)
            } else {
                0.0
            };
            (url, score)
        })
        .filter(|(_, score)| *score >= matching::POSSIBLE)
        .collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out
}

/// The (url, title) pairs of the `entry-title` headings on a WordPress search page.
fn search_results(page: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for marker in ["<h1 class=\"entry-title\">", "<h2 class=\"entry-title\">"] {
        for block in page.split(marker).skip(1) {
            let Some(href) = find::extract_between(block, "href=\"", "\"") else {
                continue;
            };
            let Some(title) = find::extract_between(block, ">", "</a>") else {
                continue;
            };
            out.push((find::decode_entities(href), find::decode_entities(title)));
        }
    }
    out
}

/// The game name of a FitGirl post title: the release-info tail (platform, version, DLCs) is
/// cut off so the title matcher sees only the name.
fn game_name(title: &str) -> String {
    let cut = title.rfind(" \u{2013} ").or_else(|| version_start(title));
    match cut {
        Some(i) => title[..i].trim().to_owned(),
        None => title.to_owned(),
    }
}

/// Where the version marker (` v`/` V` + digit) starts, if the title has one.
fn version_start(title: &str) -> Option<usize> {
    title
        .match_indices(['v', 'V'])
        .rev()
        .find(|(i, _)| {
            title[..*i].ends_with(' ')
                && title[i + 1..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit())
        })
        .map(|(i, _)| i)
}

/// The first magnet link on the page, entity-decoded.
fn magnet_link(page: &str) -> Option<String> {
    let needle = "magnet:?xt=urn:btih:";
    let from = page.find(needle)?;
    let rest = &page[from..];
    let to = rest.find('"')?;
    Some(find::decode_entities(&rest[..to]))
}

/// The repack size from the `Repack Size:` line.
fn repack_size(page: &str) -> Option<String> {
    let needle = "Repack Size: <strong>";
    let from = page.find(needle)? + needle.len();
    let to = page[from..].find("</strong>")? + from;
    let size = page[from..to].trim();
    (size.ends_with("GB") || size.ends_with("MB")).then(|| find::decode_entities(size))
}

/// The version from the `<title>`: the `v4.00 + All DLCs` part before the site suffix.
fn version_from_title(page: &str) -> Option<String> {
    let title = find::extract_between(page, "<title>", "</title>")?;
    let tail = title
        .strip_suffix(" - FitGirl Repacks")
        .or_else(|| title.strip_suffix(" [FitGirl Repack]"))
        .unwrap_or(title);
    let from = version_start(tail)?;
    Some(find::decode_entities(&tail[from..]))
}

/// Hosts that appear inside the mirror section but are not download mirrors: the IDM advert
/// FitGirl links next to the filehoster, and the site's own paste landing page.
const MIRROR_NOISE: [&str; 2] = ["internetdownloadmanager.com", "paste.fitgirl-repacks.site"];

/// The distinct hosts of the direct-download mirrors, with one URL each.
fn mirror_links(page: &str) -> Vec<(String, String)> {
    let Some(section) =
        find::extract_between(page, "<h3>Download Mirrors (Direct Links)</h3>", "<h3>")
    else {
        return Vec::new();
    };
    let mut out: Vec<(String, String)> = Vec::new();
    for href in section.split("href=\"").skip(1) {
        let Some(url) = href.split('"').next() else {
            continue;
        };
        let Some(host) = host_of(url) else {
            continue;
        };
        if MIRROR_NOISE
            .iter()
            .any(|n| host == *n || host.ends_with(&format!(".{n}")))
        {
            continue;
        }
        if out.iter().any(|(h, _)| h == &host) {
            continue;
        }
        out.push((host, find::decode_entities(url)));
    }
    out
}

/// The host of an http(s) URL, lowercased.
fn host_of(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next()?;
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
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
