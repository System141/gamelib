//! Shared helpers for integration tests: store item builders and a fake catalog source.
#![allow(dead_code)]

pub mod http;

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};

use gamelib_core::steam::types::{
    Assets, BasicInfo, Named, Platforms, RawTag, Release, ReviewSummary, Reviews, StoreItem,
    TagWeight,
};
use gamelib_core::steam::{
    CatalogSource, PageRequest, QueryPage, SORT_APPID, SORT_RELEASE_DESC, SORT_TOP_SELLERS,
};
use gamelib_core::{Error, Result};

pub const DAY: i64 = 86_400;

/// A store item with the fields the catalog cares about.
pub fn item(appid: u32, name: &str, release: i64, reviews: i64, tags: &[u32]) -> StoreItem {
    StoreItem {
        appid: Some(appid),
        id: Some(appid),
        success: Some(1),
        name: Some(name.to_owned()),
        tags: tags
            .iter()
            .enumerate()
            .map(|(i, &tagid)| TagWeight {
                tagid,
                weight: Some(1000 - i as i64),
            })
            .collect(),
        basic_info: Some(BasicInfo {
            short_description: Some(format!("About {name}")),
            developers: vec![Named {
                name: "Dev Studio".into(),
            }],
            publishers: vec![Named {
                name: "Pub House".into(),
            }],
            franchises: vec![],
        }),
        release: Some(Release {
            steam_release_date: Some(release),
            ..Default::default()
        }),
        platforms: Some(Platforms {
            windows: true,
            ..Default::default()
        }),
        reviews: Some(Reviews {
            summary_filtered: Some(ReviewSummary {
                review_count: Some(reviews),
                percent_positive: Some(if reviews > 0 { 80 } else { 0 }),
                review_score: Some(if reviews > 0 { 7 } else { 0 }),
            }),
            ..Default::default()
        }),
        assets: Some(Assets {
            asset_url_format: Some(format!("steam/apps/{appid}/${{FILENAME}}?t=1")),
            header: Some("header.jpg".into()),
            library_capsule: Some("library_capsule.jpg".into()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub fn tag_list() -> Vec<RawTag> {
    [
        (19, "Aksiyon"),
        (21, "Macera"),
        (9, "Strateji"),
        (492, "Bağımsız"),
        (113, "Oynaması Ücretsiz"),
    ]
    .into_iter()
    .map(|(tagid, name)| RawTag {
        tagid,
        name: name.into(),
    })
    .collect()
}

type PageHook = Box<dyn FnMut(u32, &mut Vec<StoreItem>)>;

/// In-memory stand-in for the Steam store query service, with Steam's paging semantics.
pub struct FakeSource {
    pub games: RefCell<Vec<StoreItem>>,
    pub requests: Cell<u32>,
    /// Called before answering the n-th page request (0-based), to mutate the catalog mid-run.
    pub hook: RefCell<Option<PageHook>>,
    /// Fail the n-th page request (0-based) with a network error.
    pub fail_at: Cell<Option<u32>>,
    /// Set the cancel flag when the n-th page request arrives.
    pub cancel_at: Cell<Option<u32>>,
}

impl FakeSource {
    pub fn new(games: Vec<StoreItem>) -> Self {
        Self {
            games: RefCell::new(games),
            requests: Cell::new(0),
            hook: RefCell::new(None),
            fail_at: Cell::new(None),
            cancel_at: Cell::new(None),
        }
    }

    /// `n` games with app ids 10, 20, 30…, released one day apart ending `now`.
    pub fn catalog(n: u32, now: i64) -> Self {
        let games = (1..=n)
            .map(|i| {
                item(
                    i * 10,
                    &format!("Game {i:05}"),
                    now - i64::from(n - i) * DAY,
                    i64::from(i % 50) * 10,
                    &[19, 492],
                )
            })
            .collect();
        Self::new(games)
    }
}

impl CatalogSource for FakeSource {
    fn tags(&self, _cancel: &AtomicBool) -> Result<Vec<RawTag>> {
        Ok(tag_list())
    }

    fn page(&self, req: PageRequest, cancel: &AtomicBool) -> Result<QueryPage> {
        let n = self.requests.get();
        self.requests.set(n + 1);
        if let Some(hook) = self.hook.borrow_mut().as_mut() {
            hook(n, &mut self.games.borrow_mut());
        }
        if self.cancel_at.get() == Some(n) {
            cancel.store(true, Ordering::Relaxed);
            return Err(Error::Cancelled);
        }
        if self.fail_at.get() == Some(n) {
            return Err(Error::Network("simulated failure".into()));
        }
        let mut list = self.games.borrow().clone();
        let release = |i: &StoreItem| {
            i.release
                .as_ref()
                .and_then(|r| r.steam_release_date)
                .unwrap_or(0)
        };
        let reviews = |i: &StoreItem| {
            i.reviews
                .as_ref()
                .and_then(|r| r.summary_filtered.as_ref())
                .and_then(|s| s.review_count)
                .unwrap_or(0)
        };
        match req.sort {
            SORT_APPID => list.sort_by_key(|i| i.appid),
            SORT_TOP_SELLERS => list.sort_by_key(|i| std::cmp::Reverse(reviews(i))),
            SORT_RELEASE_DESC => list.sort_by_key(|i| std::cmp::Reverse(release(i))),
            other => panic!("unexpected sort {other}"),
        }
        let total = list.len() as u32;
        let items: Vec<StoreItem> = list
            .into_iter()
            .skip(req.start as usize)
            .take(req.count as usize)
            .collect();
        Ok(QueryPage {
            total,
            start: req.start,
            returned: items.len() as u32,
            items,
            skipped: 0,
        })
    }

    fn stats(&self) -> (u32, u32) {
        (self.requests.get(), 0)
    }
}
