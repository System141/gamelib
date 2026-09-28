//! Conversion from a raw Steam store item to the row stored in `games`.

use crate::rating::steamdb_rating;
use crate::search::normalize;
use crate::steam::types::{Named, StoreItem};
use crate::text::clean;

/// Content descriptors that mark adult sexual content (hidden by default, like on Steam).
pub const ADULT_DESCRIPTORS: [u32; 2] = [3, 4];

/// One row of the `games` table.
#[derive(Debug, Clone, PartialEq)]
pub struct GameRecord {
    pub appid: u32,
    pub name: String,
    pub search_name: String,
    pub short_description: Option<String>,
    /// JSON arrays.
    pub developers: String,
    pub publishers: String,
    pub franchises: String,
    /// Tag ids, most relevant first (JSON array).
    pub tagids: String,
    pub descriptors: String,
    pub release_date: Option<i64>,
    pub original_release_date: Option<i64>,
    pub is_free: bool,
    pub is_early_access: bool,
    pub adult: bool,
    pub price_cents: Option<i64>,
    pub price_formatted: Option<String>,
    pub original_price_formatted: Option<String>,
    pub discount_pct: u8,
    pub review_count: u32,
    pub review_pct: u8,
    pub review_score: u8,
    pub rating: f64,
    pub win: bool,
    pub mac: bool,
    pub linux: bool,
    pub deck_compat: u8,
    pub asset_format: Option<String>,
    pub img_header: Option<String>,
    pub img_capsule: Option<String>,
    pub img_capsule_2x: Option<String>,
    pub img_hero: Option<String>,
    pub assets_modified: Option<i64>,
}

impl GameRecord {
    /// `None` for items Steam could not resolve or that lack an id or a name.
    pub fn from_item(item: &StoreItem) -> Option<Self> {
        if item.success.is_some_and(|s| s != 1) {
            return None;
        }
        let appid = item.appid.or(item.id).filter(|&id| id > 0)?;
        let name = clean(item.name.as_deref())?;

        let basic = item.basic_info.clone().unwrap_or_default();
        let release = item.release.clone().unwrap_or_default();
        let platforms = item.platforms.clone().unwrap_or_default();
        let reviews = item
            .reviews
            .as_ref()
            .and_then(|r| r.summary_filtered.clone())
            .unwrap_or_default();
        let assets = item.assets.clone().unwrap_or_default();

        // Prefer the weighted tag list (most voted first); fall back to plain ids.
        let mut tags: Vec<(u32, i64)> = item
            .tags
            .iter()
            .map(|t| (t.tagid, t.weight.unwrap_or(0)))
            .collect();
        tags.sort_by(|a, b| b.1.cmp(&a.1));
        let mut tagids: Vec<u32> = if tags.is_empty() {
            item.tagids.clone()
        } else {
            tags.into_iter().map(|t| t.0).collect()
        };
        dedup_keep_order(&mut tagids);

        let mut descriptors = item.content_descriptorids.clone();
        descriptors.sort_unstable();
        descriptors.dedup();
        let adult = descriptors.iter().any(|d| ADULT_DESCRIPTORS.contains(d));

        let is_free = item.is_free;
        let (price_cents, price_formatted, original_price_formatted, discount_pct) =
            match (&item.best_purchase_option, is_free) {
                (Some(p), false) => {
                    let discount = clamp_u8(p.discount_pct.unwrap_or(0), 100);
                    let original = if discount > 0 {
                        clean(p.formatted_original_price.as_deref())
                    } else {
                        None
                    };
                    (
                        p.final_price_in_cents,
                        clean(p.formatted_final_price.as_deref()),
                        original,
                        discount,
                    )
                }
                _ => (None, None, None, 0),
            };

        let review_count = reviews
            .review_count
            .unwrap_or(0)
            .clamp(0, i64::from(u32::MAX)) as u32;
        let review_pct = clamp_u8(reviews.percent_positive.unwrap_or(0), 100);

        Some(GameRecord {
            appid,
            search_name: normalize(&name),
            name,
            short_description: clean(basic.short_description.as_deref()),
            developers: names_json(&basic.developers),
            publishers: names_json(&basic.publishers),
            franchises: names_json(&basic.franchises),
            tagids: serde_json::to_string(&tagids).unwrap_or_else(|_| "[]".into()),
            descriptors: serde_json::to_string(&descriptors).unwrap_or_else(|_| "[]".into()),
            release_date: release.steam_release_date.filter(|&t| t > 0),
            original_release_date: release.original_release_date.filter(|&t| t > 0),
            is_free,
            is_early_access: item.is_early_access || release.is_early_access,
            adult,
            price_cents,
            price_formatted,
            original_price_formatted,
            discount_pct,
            review_count,
            review_pct,
            review_score: clamp_u8(reviews.review_score.unwrap_or(0), 9),
            rating: steamdb_rating(review_pct, review_count),
            win: platforms.windows,
            mac: platforms.mac,
            linux: platforms.steamos_linux,
            deck_compat: clamp_u8(platforms.steam_deck_compat_category.unwrap_or(0), 3),
            asset_format: clean(assets.asset_url_format.as_deref()),
            img_header: clean(assets.header.as_deref()),
            img_capsule: clean(assets.library_capsule.as_deref()),
            img_capsule_2x: clean(assets.library_capsule_2x.as_deref()),
            img_hero: clean(assets.library_hero.as_deref()),
            assets_modified: assets.last_modified,
        })
    }
}

fn clamp_u8(v: i64, max: u8) -> u8 {
    v.clamp(0, i64::from(max)) as u8
}

fn names_json(names: &[Named]) -> String {
    let mut list: Vec<String> = names.iter().filter_map(|n| clean(Some(&n.name))).collect();
    dedup_keep_order(&mut list);
    serde_json::to_string(&list).unwrap_or_else(|_| "[]".into())
}

fn dedup_keep_order<T: PartialEq + Clone>(v: &mut Vec<T>) {
    let mut seen: Vec<T> = Vec::with_capacity(v.len());
    v.retain(|x| {
        if seen.contains(x) {
            false
        } else {
            seen.push(x.clone());
            true
        }
    });
}
