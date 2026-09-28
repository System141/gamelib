//! GOG.com: the public catalog and product API (no login needed).
//!
//! The catalog pages with a cursor: products ordered by id, `searchAfter` = the last id seen.
//! Plain page numbers stop after 10 000 results, the cursor does not.

use std::sync::atomic::AtomicBool;

use serde::Deserialize;

use super::{StoreEndpoints, StoreProduct};
use crate::date::parse_date;
use crate::http::{self, Counters, Pacer};
use crate::model::Store;
use crate::{Error, Result};

/// Largest page GOG accepts is 140; 100 keeps responses around 350 kB.
pub const PAGE_SIZE: u32 = 100;
/// Prices are shown for this store region.
pub const COUNTRY: &str = "TR";

#[derive(Debug, Default)]
pub struct CatalogPage {
    pub products: Vec<StoreProduct>,
    /// Products matching the query in the whole catalog.
    pub total: u32,
    /// Cursor for the next page; `None` after the last one.
    pub next: Option<String>,
}

/// One catalog page of games and packs, after the product with id `after` (`None` = start).
pub fn catalog_page(
    http: &reqwest::blocking::Client,
    endpoints: &StoreEndpoints,
    after: Option<&str>,
    pacer: &Pacer,
    cancel: &AtomicBool,
    counters: &Counters,
) -> Result<CatalogPage> {
    pacer.wait(cancel)?;
    let limit = PAGE_SIZE.to_string();
    let after = after.unwrap_or("0");
    let url = format!("{}/v1/catalog", endpoints.gog_catalog);
    let body: RawCatalog = http::get_json(
        http,
        &url,
        |r| {
            r.query(&[
                ("limit", limit.as_str()),
                ("order", "asc:externalProductId"),
                ("productType", "in:game,pack"),
                ("countryCode", COUNTRY),
                ("locale", "en-US"),
                ("currencyCode", "USD"),
                ("searchAfter", after),
            ])
        },
        cancel,
        counters,
    )?;
    Ok(parse_catalog(body))
}

pub fn parse_catalog_json(json: &str) -> Result<CatalogPage> {
    Ok(parse_catalog(serde_json::from_str(json)?))
}

fn parse_catalog(raw: RawCatalog) -> CatalogPage {
    let full = raw.products.len() as u32 >= PAGE_SIZE;
    let next = if full {
        raw.products.last().map(|p| p.id.0.clone())
    } else {
        None
    };
    CatalogPage {
        total: raw.product_count.unwrap_or(0),
        products: raw.products.into_iter().filter_map(product_from).collect(),
        next,
    }
}

fn product_from(p: RawProduct) -> Option<StoreProduct> {
    let title = p.title.trim().to_owned();
    if title.is_empty() || p.id.0.is_empty() {
        return None;
    }
    let os = |name: &str| p.operating_systems.iter().any(|o| o == name);
    let (price, is_free) = match &p.price {
        Some(price) => {
            let amount = price
                .final_money
                .as_ref()
                .and_then(|m| m.amount.as_deref())
                .and_then(|a| a.parse::<f64>().ok());
            (
                price.final_price.clone().filter(|s| !s.is_empty()),
                amount == Some(0.0),
            )
        }
        None => (None, false),
    };
    Some(StoreProduct {
        store: Store::Gog,
        product_id: p.id.0,
        kind: p.product_type.unwrap_or_else(|| "game".into()),
        title,
        slug: p.slug,
        url: p.store_link,
        developers: p.developers,
        publishers: p.publishers,
        release_date: p.release_date.as_deref().and_then(parse_date),
        store_release_date: p.store_release_date.as_deref().and_then(parse_date),
        cover: p.cover_vertical,
        cover_wide: p.cover_horizontal,
        win: os("windows"),
        mac: os("osx"),
        linux: os("linux"),
        price,
        is_free,
    })
}

/// Basic facts about one product from the public product API, for products the catalog does
/// not list (owned editions, region-locked or retired products). `None` if GOG has no such
/// product.
pub fn product_info(
    http: &reqwest::blocking::Client,
    endpoints: &StoreEndpoints,
    id: &str,
    pacer: &Pacer,
    cancel: &AtomicBool,
    counters: &Counters,
) -> Result<Option<StoreProduct>> {
    pacer.wait(cancel)?;
    let url = format!("{}/products/{id}", endpoints.gog_api);
    match http::get_text(http, &url, |r| r, cancel, counters) {
        Ok(body) => Ok(parse_product_info(&body)?),
        Err(Error::Http { status: 404, .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn parse_product_info(json: &str) -> Result<Option<StoreProduct>> {
    let p: RawProductInfo = serde_json::from_str(json)?;
    let title = p.title.trim().to_owned();
    if title.is_empty() {
        return Ok(None);
    }
    let cs = p.content_system_compatibility.unwrap_or_default();
    let icon = |s: Option<String>| {
        s.map(|u| {
            if u.starts_with("//") {
                format!("https:{u}")
            } else {
                u
            }
        })
    };
    Ok(Some(StoreProduct {
        store: Store::Gog,
        product_id: p.id.0,
        kind: p.game_type.unwrap_or_else(|| "game".into()),
        title,
        url: p.links.and_then(|l| l.product_card),
        slug: p.slug,
        developers: Vec::new(),
        publishers: Vec::new(),
        release_date: p.release_date.as_deref().and_then(parse_date),
        store_release_date: None,
        cover: None,
        cover_wide: icon(p.images.and_then(|i| i.logo2x)),
        win: cs.windows,
        mac: cs.osx,
        linux: cs.linux,
        price: None,
        is_free: false,
    }))
}

// --- wire format ----------------------------------------------------------------------------

/// GOG ids are strings in the catalog and numbers in other APIs.
#[derive(Debug, Clone, Default)]
struct Id(String);

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Text(String),
            Number(u64),
        }
        Ok(Id(match Raw::deserialize(d)? {
            Raw::Text(s) => s.trim().to_owned(),
            Raw::Number(n) => n.to_string(),
        }))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCatalog {
    #[serde(default)]
    products: Vec<RawProduct>,
    product_count: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawProduct {
    id: Id,
    #[serde(default)]
    title: String,
    slug: Option<String>,
    product_type: Option<String>,
    release_date: Option<String>,
    store_release_date: Option<String>,
    #[serde(default)]
    developers: Vec<String>,
    #[serde(default)]
    publishers: Vec<String>,
    #[serde(default)]
    operating_systems: Vec<String>,
    price: Option<RawPrice>,
    cover_horizontal: Option<String>,
    cover_vertical: Option<String>,
    store_link: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPrice {
    #[serde(rename = "final")]
    final_price: Option<String>,
    final_money: Option<RawMoney>,
}

#[derive(Deserialize)]
struct RawMoney {
    amount: Option<String>,
}

#[derive(Deserialize)]
struct RawProductInfo {
    id: Id,
    #[serde(default)]
    title: String,
    slug: Option<String>,
    game_type: Option<String>,
    release_date: Option<String>,
    content_system_compatibility: Option<RawCompat>,
    links: Option<RawLinks>,
    images: Option<RawImages>,
}

#[derive(Deserialize, Default)]
struct RawCompat {
    #[serde(default)]
    windows: bool,
    #[serde(default)]
    osx: bool,
    #[serde(default)]
    linux: bool,
}

#[derive(Deserialize)]
struct RawLinks {
    product_card: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawImages {
    logo2x: Option<String>,
}
