//! Raw response shapes of the Steam store services.
//!
//! Steam omits fields that are false/empty (`is_free`, `platforms.mac`, …) and sends some numbers
//! as strings, so everything here is optional or defaulted and numbers are read leniently.

use serde::{Deserialize, Deserializer};

#[derive(Debug, Deserialize)]
pub struct QueryEnvelope {
    #[serde(default)]
    pub response: QueryResponse,
}

/// `IStoreQueryService/Query` response. Items stay as raw JSON so one malformed item cannot
/// fail a whole page; they are parsed one by one with [`parse_items`].
#[derive(Debug, Default, Deserialize)]
pub struct QueryResponse {
    pub metadata: Option<QueryMetadata>,
    #[serde(default)]
    pub store_items: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct QueryMetadata {
    #[serde(default)]
    pub total_matching_records: u32,
    #[serde(default)]
    pub start: u32,
    #[serde(default)]
    pub count: u32,
}

/// `IStoreBrowseService/GetItems` response.
#[derive(Debug, Deserialize)]
pub struct ItemsEnvelope {
    #[serde(default)]
    pub response: ItemsResponse,
}

#[derive(Debug, Default, Deserialize)]
pub struct ItemsResponse {
    #[serde(default)]
    pub store_items: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StoreItem {
    pub appid: Option<u32>,
    pub id: Option<u32>,
    pub success: Option<i64>,
    pub name: Option<String>,
    #[serde(default)]
    pub is_free: bool,
    #[serde(default)]
    pub is_early_access: bool,
    #[serde(default)]
    pub content_descriptorids: Vec<u32>,
    #[serde(default)]
    pub tagids: Vec<u32>,
    #[serde(default)]
    pub tags: Vec<TagWeight>,
    pub basic_info: Option<BasicInfo>,
    pub release: Option<Release>,
    pub platforms: Option<Platforms>,
    pub reviews: Option<Reviews>,
    pub best_purchase_option: Option<PurchaseOption>,
    pub assets: Option<Assets>,
    pub screenshots: Option<Screenshots>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TagWeight {
    pub tagid: u32,
    #[serde(default, deserialize_with = "lenient_i64")]
    pub weight: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BasicInfo {
    pub short_description: Option<String>,
    #[serde(default)]
    pub developers: Vec<Named>,
    #[serde(default)]
    pub publishers: Vec<Named>,
    #[serde(default)]
    pub franchises: Vec<Named>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Named {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Release {
    #[serde(default, deserialize_with = "lenient_i64")]
    pub steam_release_date: Option<i64>,
    #[serde(default, deserialize_with = "lenient_i64")]
    pub original_release_date: Option<i64>,
    #[serde(default)]
    pub is_early_access: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Platforms {
    #[serde(default)]
    pub windows: bool,
    #[serde(default)]
    pub mac: bool,
    #[serde(default)]
    pub steamos_linux: bool,
    #[serde(default, deserialize_with = "lenient_i64")]
    pub steam_deck_compat_category: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Reviews {
    pub summary_filtered: Option<ReviewSummary>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReviewSummary {
    #[serde(default, deserialize_with = "lenient_i64")]
    pub review_count: Option<i64>,
    #[serde(default, deserialize_with = "lenient_i64")]
    pub percent_positive: Option<i64>,
    #[serde(default, deserialize_with = "lenient_i64")]
    pub review_score: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PurchaseOption {
    #[serde(default, deserialize_with = "lenient_i64")]
    pub final_price_in_cents: Option<i64>,
    pub formatted_final_price: Option<String>,
    pub formatted_original_price: Option<String>,
    #[serde(default, deserialize_with = "lenient_i64")]
    pub discount_pct: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Assets {
    pub asset_url_format: Option<String>,
    pub header: Option<String>,
    pub library_capsule: Option<String>,
    pub library_capsule_2x: Option<String>,
    pub library_hero: Option<String>,
    #[serde(default, deserialize_with = "lenient_i64")]
    pub last_modified: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Screenshots {
    #[serde(default)]
    pub all_ages_screenshots: Vec<ScreenshotFile>,
    #[serde(default)]
    pub mature_content_screenshots: Vec<ScreenshotFile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScreenshotFile {
    pub filename: String,
    #[serde(default)]
    pub ordinal: i64,
}

/// `IStoreService/GetTagList` response.
#[derive(Debug, Deserialize)]
pub struct TagListEnvelope {
    #[serde(default)]
    pub response: TagListResponse,
}

#[derive(Debug, Default, Deserialize)]
pub struct TagListResponse {
    #[serde(default)]
    pub tags: Vec<RawTag>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RawTag {
    pub tagid: u32,
    pub name: String,
}

/// Parses raw store items, returning the good ones and how many were skipped.
pub fn parse_items(raw: Vec<serde_json::Value>) -> (Vec<StoreItem>, u32) {
    let mut skipped = 0;
    let items = raw
        .into_iter()
        .filter_map(|v| match serde_json::from_value::<StoreItem>(v) {
            Ok(item) => Some(item),
            Err(_) => {
                skipped += 1;
                None
            }
        })
        .collect();
    (items, skipped)
}

/// Accepts a JSON number or a numeric string ("579"); anything else becomes `None`.
fn lenient_i64<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Loose {
        Int(i64),
        Float(f64),
        Text(String),
        Other(serde::de::IgnoredAny),
    }
    Ok(match Option::<Loose>::deserialize(d)? {
        Some(Loose::Int(v)) => Some(v),
        Some(Loose::Float(v)) if v.is_finite() => Some(v as i64),
        Some(Loose::Text(s)) => s.trim().parse().ok(),
        _ => None,
    })
}
