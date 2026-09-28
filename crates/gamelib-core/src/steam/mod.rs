//! Keyless Steam store API client.
//!
//! Valve removed `ISteamApps/GetAppList`, and `IStoreService/GetAppList` needs an API key, but the
//! store's own `IStoreQueryService/Query` works without one and returns up to 1000 games per page
//! together with names, reviews, prices and image file names.

pub mod assets;
pub mod types;

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::StatusCode;
use reqwest::blocking::Client;
use reqwest::header::RETRY_AFTER;
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::model::{GameMedia, Screenshot};
use crate::text::clean;
use crate::{Error, Result, sleep_cancellable};
use types::{ItemsEnvelope, QueryEnvelope, RawTag, StoreItem, TagListEnvelope, parse_items};

pub const QUERY_URL: &str = "https://api.steampowered.com/IStoreQueryService/Query/v1/";
pub const TAG_LIST_URL: &str = "https://api.steampowered.com/IStoreService/GetTagList/v1/";
pub const ITEMS_URL: &str = "https://api.steampowered.com/IStoreBrowseService/GetItems/v1/";

/// Store region: decides prices (USD with Turkish regional pricing) and which region-locked
/// titles are visible. "US" would add roughly 33 games that are not sold in Turkey.
pub const COUNTRY: &str = "TR";
/// Catalog text language. Turkish descriptions are missing for most games, so fetch English.
pub const LANGUAGE: &str = "english";
/// Tag names and per-game media are requested in Turkish.
pub const UI_LANGUAGE: &str = "turkish";

pub const MAX_PAGE_SIZE: u32 = 1000;
/// appid ascending: stable order for paging through the whole catalog.
pub const SORT_APPID: u32 = 2;
/// Top sellers.
pub const SORT_TOP_SELLERS: u32 = 10;
/// Release date, newest first (strictly ordered).
pub const SORT_RELEASE_DESC: u32 = 40;

const USER_AGENT: &str = concat!(
    "GameLib/",
    env!("CARGO_PKG_VERSION"),
    " (desktop; +https://github.com/System141/gamelib)"
);
const MAX_ATTEMPTS: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRequest {
    pub start: u32,
    pub count: u32,
    pub sort: u32,
}

#[derive(Debug, Clone, Default)]
pub struct QueryPage {
    /// Games matching the query in the whole catalog.
    pub total: u32,
    pub start: u32,
    /// How many items Steam returned (including ones that failed to parse).
    pub returned: u32,
    pub items: Vec<StoreItem>,
    pub skipped: u32,
}

/// Where catalog pages come from. The Steam client implements it; tests use a fake.
pub trait CatalogSource {
    fn tags(&self, cancel: &AtomicBool) -> Result<Vec<RawTag>>;
    fn page(&self, req: PageRequest, cancel: &AtomicBool) -> Result<QueryPage>;
    /// (requests sent, retries) so far.
    fn stats(&self) -> (u32, u32) {
        (0, 0)
    }
}

pub struct SteamClient {
    http: Client,
    requests: AtomicU32,
    retries: AtomicU32,
}

impl SteamClient {
    /// Honours `HTTPS_PROXY`/system proxy settings and uses the platform certificate store.
    pub fn new() -> Result<Self> {
        let http = Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(60))
            .gzip(true)
            .build()?;
        Ok(Self {
            http,
            requests: AtomicU32::new(0),
            retries: AtomicU32::new(0),
        })
    }

    fn get_json<T: DeserializeOwned>(
        &self,
        url: &str,
        query: &[(&str, &str)],
        cancel: &AtomicBool,
    ) -> Result<T> {
        let body = self.get_text(url, query, cancel)?;
        Ok(serde_json::from_str(&body)?)
    }

    /// GET with retries on network errors, timeouts, 429 and 5xx.
    fn get_text(&self, url: &str, query: &[(&str, &str)], cancel: &AtomicBool) -> Result<String> {
        let mut attempt = 0;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            attempt += 1;
            self.requests.fetch_add(1, Ordering::Relaxed);
            let mut retry_after = None;
            let err = match self.http.get(url).query(query).send() {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        match resp.text() {
                            Ok(body) => return Ok(body),
                            Err(e) => Error::from(e),
                        }
                    } else if status == StatusCode::TOO_MANY_REQUESTS {
                        retry_after = resp
                            .headers()
                            .get(RETRY_AFTER)
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.trim().parse::<u64>().ok())
                            .map(|s| Duration::from_secs(s.min(120)));
                        Error::RateLimited
                    } else {
                        Error::Http {
                            status: status.as_u16(),
                            url: url.to_owned(),
                        }
                    }
                }
                Err(e) => Error::from(e),
            };
            if !err.is_transient() || attempt >= MAX_ATTEMPTS {
                return Err(err);
            }
            self.retries.fetch_add(1, Ordering::Relaxed);
            sleep_cancellable(retry_after.unwrap_or_else(|| backoff(attempt)), cancel)?;
        }
    }

    /// Turkish description (if any) and screenshots for one game.
    pub fn fetch_media(&self, appid: u32, cancel: &AtomicBool) -> Result<GameMedia> {
        let input = json!({
            "ids": [{ "appid": appid }],
            "context": { "language": UI_LANGUAGE, "country_code": COUNTRY },
            "data_request": { "include_basic_info": true, "include_screenshots": true },
        })
        .to_string();
        let env: ItemsEnvelope = self.get_json(ITEMS_URL, &[("input_json", &input)], cancel)?;
        let (items, _) = parse_items(env.response.store_items);
        let Some(item) = items.into_iter().find(|i| i.appid.or(i.id) == Some(appid)) else {
            return Ok(GameMedia::default());
        };
        Ok(media_from_item(&item))
    }
}

impl CatalogSource for SteamClient {
    fn tags(&self, cancel: &AtomicBool) -> Result<Vec<RawTag>> {
        let env: TagListEnvelope =
            self.get_json(TAG_LIST_URL, &[("language", UI_LANGUAGE)], cancel)?;
        Ok(env.response.tags)
    }

    fn page(&self, req: PageRequest, cancel: &AtomicBool) -> Result<QueryPage> {
        let input = query_input(req).to_string();
        let env: QueryEnvelope = self.get_json(QUERY_URL, &[("input_json", &input)], cancel)?;
        let Some(meta) = env.response.metadata else {
            return Err(Error::Parse("query response without metadata".into()));
        };
        let returned = env.response.store_items.len() as u32;
        let (items, skipped) = parse_items(env.response.store_items);
        Ok(QueryPage {
            total: meta.total_matching_records,
            start: meta.start,
            returned,
            items,
            skipped,
        })
    }

    fn stats(&self) -> (u32, u32) {
        (
            self.requests.load(Ordering::Relaxed),
            self.retries.load(Ordering::Relaxed),
        )
    }
}

/// The `input_json` for one page of released games.
pub fn query_input(req: PageRequest) -> serde_json::Value {
    json!({
        "query": {
            "start": req.start,
            "count": req.count.min(MAX_PAGE_SIZE),
            "sort": req.sort,
            "filters": { "released_only": true, "type_filters": { "include_games": true } },
        },
        "context": { "language": LANGUAGE, "country_code": COUNTRY },
        "data_request": {
            "include_assets": true,
            "include_release": true,
            "include_platforms": true,
            "include_reviews": true,
            "include_basic_info": true,
            "include_tag_count": 20,
        },
    })
}

pub fn media_from_item(item: &StoreItem) -> GameMedia {
    let description_tr = clean(
        item.basic_info
            .as_ref()
            .and_then(|b| b.short_description.as_deref()),
    );
    let mut screenshots = Vec::new();
    if let Some(s) = &item.screenshots {
        for (files, mature) in [
            (&s.all_ages_screenshots, false),
            (&s.mature_content_screenshots, true),
        ] {
            let mut files: Vec<_> = files.iter().collect();
            files.sort_by_key(|f| f.ordinal);
            for f in files {
                let (thumb, full) = assets::screenshot_urls(&f.filename);
                screenshots.push(Screenshot {
                    thumb,
                    full,
                    mature,
                });
            }
        }
    }
    GameMedia {
        description_tr,
        screenshots,
    }
}

/// 2, 4, 8, 16… seconds (capped at 30) plus up to 0.5 s of jitter.
fn backoff(attempt: u32) -> Duration {
    let base = 2u64.saturating_pow(attempt).min(30);
    let jitter = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_millis() % 500)
        .unwrap_or(0);
    Duration::from_secs(base) + Duration::from_millis(u64::from(jitter))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_input_shape() {
        let v = query_input(PageRequest {
            start: 2000,
            count: 5000,
            sort: SORT_APPID,
        });
        assert_eq!(v["query"]["start"], 2000);
        assert_eq!(v["query"]["count"], MAX_PAGE_SIZE);
        assert_eq!(v["query"]["filters"]["released_only"], true);
        assert_eq!(v["query"]["filters"]["type_filters"]["include_games"], true);
        assert_eq!(v["context"]["country_code"], COUNTRY);
        assert_eq!(v["data_request"]["include_assets"], true);
    }

    #[test]
    fn backoff_grows_and_caps() {
        assert!(backoff(1) >= Duration::from_secs(2));
        assert!(backoff(3) >= Duration::from_secs(8));
        assert!(backoff(10) < Duration::from_secs(31));
    }
}
