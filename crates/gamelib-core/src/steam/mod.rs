//! Keyless Steam store API client.
//!
//! Valve removed `ISteamApps/GetAppList`, and `IStoreService/GetAppList` needs an API key, but the
//! store's own `IStoreQueryService/Query` works without one and returns up to 1000 games per page
//! together with names, reviews, prices and image file names.

pub mod assets;
pub mod reviews;
pub mod types;

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use reqwest::blocking::Client;
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::http::{self, Counters};
use crate::model::{GameMedia, ReviewScore, ReviewSummaries, Screenshot, Trailer};
use crate::text::clean;
use crate::{Error, Result};
use types::{
    ItemsEnvelope, QueryEnvelope, RawTag, ReviewSummary, StoreItem, TagListEnvelope, parse_items,
};

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
    counters: Counters,
}

impl SteamClient {
    /// Honours `HTTPS_PROXY`/system proxy settings and uses the platform certificate store.
    pub fn new() -> Result<Self> {
        Ok(Self {
            http: http::api_client(Duration::from_secs(60))?,
            counters: Counters::default(),
        })
    }

    fn get_json<T: DeserializeOwned>(
        &self,
        url: &str,
        query: &[(&str, &str)],
        cancel: &AtomicBool,
    ) -> Result<T> {
        http::get_json(&self.http, url, |r| r.query(query), cancel, &self.counters)
    }

    /// Turkish description (if any), screenshots, trailers and review summaries for one game.
    pub fn fetch_media(&self, appid: u32, cancel: &AtomicBool) -> Result<GameMedia> {
        let input = json!({
            "ids": [{ "appid": appid }],
            "context": { "language": UI_LANGUAGE, "country_code": COUNTRY },
            "data_request": {
                "include_basic_info": true,
                "include_screenshots": true,
                "include_trailers": true,
                "include_reviews": true,
            },
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
        self.counters.get()
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
    let trailers = item
        .trailers
        .iter()
        .flat_map(|t| t.highlights.iter().chain(&t.other_trailers))
        .filter_map(|t| {
            let hls = t
                .adaptive_trailers
                .iter()
                .find(|a| a.encoding == "hls_h264")?;
            Some(Trailer {
                name: clean(t.trailer_name.as_deref()).unwrap_or_default(),
                poster: assets::asset_url(
                    t.trailer_url_format.as_deref(),
                    t.screenshot_medium.as_deref(),
                ),
                stream: assets::trailer_stream_url(&hls.cdn_path, t.trailer_url_format.as_deref())?,
                mature: t.all_ages == Some(false),
            })
        })
        .collect();
    let reviews = item.reviews.as_ref().map(|r| ReviewSummaries {
        all: r.summary_filtered.as_ref().and_then(review_score),
        turkish: r.summary_language_specific.as_ref().and_then(review_score),
    });
    GameMedia {
        description_tr,
        screenshots,
        trailers,
        reviews,
    }
}

fn review_score(s: &ReviewSummary) -> Option<ReviewScore> {
    let count = u32::try_from(s.review_count?).ok().filter(|&c| c > 0)?;
    Some(ReviewScore {
        count,
        percent: s.percent_positive.unwrap_or(0).clamp(0, 100) as u8,
        score: s.review_score.unwrap_or(0).clamp(0, 9) as u8,
    })
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
    fn media_with_trailers_and_review_summaries() {
        let env: ItemsEnvelope =
            serde_json::from_str(include_str!("../../tests/fixtures/items_media.json")).unwrap();
        let (items, skipped) = parse_items(env.response.store_items);
        assert_eq!(skipped, 0);
        let media = media_from_item(&items[0]);
        assert!(media.description_tr.unwrap().starts_with("The Witcher 3"));
        assert_eq!(media.screenshots.len(), 2);
        assert_eq!(media.trailers.len(), 2);
        let t = &media.trailers[0];
        assert!(t.name.contains("Launch Trailer"), "{}", t.name);
        assert!(
            t.stream.starts_with(assets::VIDEO_CDN) && t.stream.contains("hls_264_master.m3u8?t=")
        );
        assert!(t.poster.as_deref().unwrap().starts_with(assets::CDN));
        assert!(t.mature, "the first trailer is not for all ages");
        let reviews = media.reviews.unwrap();
        assert_eq!(
            reviews.all,
            Some(ReviewScore {
                count: 826_852,
                percent: 96,
                score: 9
            })
        );
        assert_eq!(reviews.turkish.map(|r| r.count), Some(46_186));
    }
}
