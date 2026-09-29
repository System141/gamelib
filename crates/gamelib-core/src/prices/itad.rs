//! IsThereAnyDeal (api.isthereanydeal.com, API v2): a game's prices in legitimate shops for the
//! Turkish storefronts, its lowest prices, the subscriptions and bundles that include it, and
//! Steam's price history. The user's own API key goes in the `ITAD-API-Key` header, so it never
//! appears in an address or in an error message.

use std::sync::atomic::AtomicBool;

use reqwest::Method;
use reqwest::blocking::Client;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::date::{parse_rfc3339, rfc3339_day};
use crate::http::{self, Counters};
use crate::model::{Bundle, Deal, GamePrices, LowestPrice, Money, PricePoint, Subscription};
use crate::{Error, Result};

pub const API: &str = "https://api.isthereanydeal.com";
const COUNTRY: &str = "TR";
/// Steam's shop id, for the price history.
const STEAM: &str = "61";
/// A game IsThereAnyDeal surely knows (The Witcher 3), for checking a key.
const KNOWN_APPID: &str = "292030";
/// Price history shown: two years.
const HISTORY_SECONDS: i64 = 730 * 86_400;

struct Api<'a> {
    client: &'a Client,
    base: &'a str,
    key: &'a str,
    cancel: &'a AtomicBool,
    counters: &'a Counters,
}

impl Api<'_> {
    fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<&Value>,
    ) -> Result<T> {
        let url = format!("{}{path}", self.base.trim_end_matches('/'));
        let text = http::request_text(
            self.client,
            method,
            &url,
            |r| {
                let r = r.header("ITAD-API-Key", self.key).query(query);
                match body {
                    Some(b) => r.json(b),
                    None => r,
                }
            },
            self.cancel,
            self.counters,
        )
        .map_err(|e| match e {
            // A missing, wrong or revoked key.
            Error::Http {
                status: 401 | 403, ..
            } => Error::Invalid("itad_key"),
            other => other,
        })?;
        serde_json::from_str(&text).map_err(|e| Error::Parse(format!("isthereanydeal {path}: {e}")))
    }
}

/// Checks that `key` is accepted.
pub fn check_key(
    client: &Client,
    base: &str,
    key: &str,
    cancel: &AtomicBool,
    counters: &Counters,
) -> Result<()> {
    let key = key.trim();
    if key.is_empty()
        || key.len() > 200
        || !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(Error::Invalid("itad_key"));
    }
    let api = Api {
        client,
        base,
        key,
        cancel,
        counters,
    };
    api.call::<Value>(
        Method::GET,
        "/games/lookup/v1",
        &[("appid", KNOWN_APPID)],
        None,
    )
    .map(drop)
}

#[derive(Deserialize)]
struct Lookup {
    #[serde(default)]
    found: bool,
    game: Option<IdOnly>,
}

#[derive(Deserialize)]
struct IdOnly {
    id: String,
}

#[derive(Deserialize)]
struct RawPrice {
    amount: f64,
    currency: String,
}

impl From<RawPrice> for Money {
    fn from(p: RawPrice) -> Self {
        Money {
            amount: p.amount,
            currency: p.currency,
        }
    }
}

#[derive(Deserialize)]
struct Named {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct RawDeal {
    shop: Named,
    price: RawPrice,
    regular: RawPrice,
    #[serde(default)]
    cut: f64,
    #[serde(rename = "storeLow")]
    store_low: Option<RawPrice>,
    #[serde(default)]
    drm: Vec<Named>,
    expiry: Option<String>,
    url: String,
}

#[derive(Deserialize)]
struct RawPrices {
    #[serde(rename = "historyLow")]
    history_low: Option<HistoryLow>,
    #[serde(default)]
    deals: Vec<RawDeal>,
}

#[derive(Deserialize)]
struct HistoryLow {
    y1: Option<RawPrice>,
    m3: Option<RawPrice>,
}

#[derive(Deserialize)]
struct Overview {
    #[serde(default)]
    prices: Vec<OverviewPrice>,
    #[serde(default)]
    bundles: Vec<RawBundle>,
}

#[derive(Deserialize)]
struct OverviewPrice {
    lowest: Option<RawLow>,
    urls: Option<Urls>,
}

#[derive(Deserialize)]
struct Urls {
    game: Option<String>,
}

#[derive(Deserialize)]
struct RawLow {
    shop: Named,
    price: RawPrice,
    regular: RawPrice,
    #[serde(default)]
    cut: f64,
    timestamp: String,
}

#[derive(Deserialize)]
struct RawBundle {
    #[serde(default)]
    title: String,
    page: Named,
    /// IsThereAnyDeal's page about the bundle; only its pages open from the app.
    details: Option<String>,
    expiry: Option<String>,
    #[serde(default)]
    tiers: Vec<Tier>,
}

#[derive(Deserialize)]
struct Tier {
    price: Option<RawPrice>,
    #[serde(default)]
    games: Vec<IdOnly>,
}

#[derive(Deserialize)]
struct RawSubs {
    #[serde(default)]
    subs: Vec<RawSub>,
}

#[derive(Deserialize)]
struct RawSub {
    name: String,
    leaving: Option<String>,
}

#[derive(Deserialize)]
struct RawChange {
    timestamp: String,
    deal: Option<RawChangeDeal>,
}

#[derive(Deserialize)]
struct RawChangeDeal {
    price: RawPrice,
    regular: RawPrice,
    #[serde(default)]
    cut: f64,
}

fn percent(cut: f64) -> u8 {
    cut.round().clamp(0.0, 100.0) as u8
}

/// Everything shown about a game's prices. Subscriptions and history are extras: when only
/// they fail, the rest is still shown.
pub fn fetch(
    client: &Client,
    base: &str,
    key: &str,
    appid: u32,
    now: i64,
    cancel: &AtomicBool,
    counters: &Counters,
) -> Result<GamePrices> {
    let api = Api {
        client,
        base,
        key,
        cancel,
        counters,
    };
    let appid = appid.to_string();
    let lookup: Lookup = api.call(
        Method::GET,
        "/games/lookup/v1",
        &[("appid", appid.as_str())],
        None,
    )?;
    let Some(id) = lookup.game.filter(|_| lookup.found).map(|g| g.id) else {
        return Ok(GamePrices::default());
    };
    let ids = json!([id]);
    let since = rfc3339_day(now - HISTORY_SECONDS);
    let price_query = [("country", COUNTRY), ("vouchers", "false")];
    let (prices, overview, subs, history) = std::thread::scope(|s| {
        let prices = s.spawn(|| {
            api.call::<Vec<RawPrices>>(Method::POST, "/games/prices/v3", &price_query, Some(&ids))
        });
        let overview = s.spawn(|| {
            api.call::<Overview>(Method::POST, "/games/overview/v2", &price_query, Some(&ids))
        });
        let subs = s.spawn(|| {
            api.call::<Vec<RawSubs>>(
                Method::POST,
                "/games/subs/v1",
                &[("country", COUNTRY)],
                Some(&ids),
            )
        });
        let history = s.spawn(|| {
            api.call::<Vec<RawChange>>(
                Method::GET,
                "/games/history/v2",
                &[
                    ("id", id.as_str()),
                    ("country", COUNTRY),
                    ("shops", STEAM),
                    ("since", since.as_str()),
                ],
                None,
            )
        });
        (join(prices), join(overview), join(subs), join(history))
    });
    Ok(assemble(&id, prices?, overview?, subs.ok(), history.ok()))
}

fn join<T>(handle: std::thread::ScopedJoinHandle<'_, Result<T>>) -> Result<T> {
    handle
        .join()
        .unwrap_or_else(|_| Err(Error::Other("isthereanydeal request panicked".into())))
}

fn assemble(
    id: &str,
    prices: Vec<RawPrices>,
    overview: Overview,
    subs: Option<Vec<RawSubs>>,
    history: Option<Vec<RawChange>>,
) -> GamePrices {
    let (mut deals, lowest_year, lowest_months) = match prices.into_iter().next() {
        Some(p) => {
            let (y1, m3) = p.history_low.map_or((None, None), |h| (h.y1, h.m3));
            let deals: Vec<Deal> = p
                .deals
                .into_iter()
                .map(|d| Deal {
                    shop: d.shop.name,
                    price: d.price.into(),
                    regular: d.regular.into(),
                    cut: percent(d.cut),
                    store_low: d.store_low.map(Money::from),
                    drm: d
                        .drm
                        .into_iter()
                        .map(|n| n.name)
                        .filter(|n| !n.is_empty())
                        .collect(),
                    expiry: d.expiry.as_deref().and_then(parse_rfc3339),
                    url: d.url,
                })
                .collect();
            (deals, y1.map(Money::from), m3.map(Money::from))
        }
        None => (Vec::new(), None, None),
    };
    deals.sort_by(|a, b| a.price.amount.total_cmp(&b.price.amount));
    let game = overview.prices.into_iter().next();
    let lowest = game.as_ref().and_then(|g| g.lowest.as_ref()).and_then(|l| {
        Some(LowestPrice {
            shop: l.shop.name.clone(),
            price: Money {
                amount: l.price.amount,
                currency: l.price.currency.clone(),
            },
            regular: Money {
                amount: l.regular.amount,
                currency: l.regular.currency.clone(),
            },
            cut: percent(l.cut),
            at: parse_rfc3339(&l.timestamp)?,
        })
    });
    let url = game.and_then(|g| g.urls).and_then(|u| u.game);
    let bundles = overview
        .bundles
        .into_iter()
        .map(|b| {
            // The cheapest tier holding this game.
            let price = b
                .tiers
                .into_iter()
                .filter(|t| t.games.iter().any(|g| g.id == id))
                .filter_map(|t| t.price)
                .min_by(|a, b| a.amount.total_cmp(&b.amount))
                .map(Money::from);
            Bundle {
                title: b.title,
                store: b.page.name,
                price,
                expiry: b.expiry.as_deref().and_then(parse_rfc3339),
                url: b.details,
            }
        })
        .collect();
    let subscriptions = subs
        .unwrap_or_default()
        .into_iter()
        .next()
        .map(|s| {
            s.subs
                .into_iter()
                .map(|s| Subscription {
                    name: s.name,
                    leaving: s.leaving.as_deref().and_then(parse_rfc3339),
                })
                .collect()
        })
        .unwrap_or_default();
    let mut history: Vec<PricePoint> = history
        .unwrap_or_default()
        .into_iter()
        .filter_map(|c| {
            let deal = c.deal?;
            Some(PricePoint {
                at: parse_rfc3339(&c.timestamp)?,
                price: deal.price.amount,
                regular: deal.regular.amount,
                cut: percent(deal.cut),
            })
        })
        .collect();
    history.sort_by_key(|p| p.at);
    GamePrices {
        found: true,
        url,
        deals,
        lowest,
        lowest_year,
        lowest_months,
        subscriptions,
        bundles,
        history,
    }
}
