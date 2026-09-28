//! itch.io: the user's library through their API key (https://itch.io/user/settings/api-keys).
//!
//! These are the endpoints the official itch app uses. An API key goes in the `Authorization`
//! header as is (no "Bearer"), like the official client sends it. Responses are snake_case;
//! camelCase aliases are accepted too.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::AUTHORIZATION;
use serde::Deserialize;

use super::{StoreEndpoints, StoreProduct};
use crate::date::parse_date;
use crate::http::{self, Counters, Pacer, USER_AGENT};
use crate::model::Store;
use crate::{Error, Result};

/// Owned keys per page (the API allows up to 500).
const PAGE_SIZE: u32 = 500;
/// Safety stop for owned-key paging.
const MAX_PAGES: u32 = 100;

pub fn client() -> Result<Client> {
    Ok(Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .gzip(true)
        .build()?)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItchUser {
    pub id: u64,
    pub username: String,
}

/// A game in the user's library, with the download key that unlocks its files.
#[derive(Debug, Clone, PartialEq)]
pub struct OwnedGame {
    pub download_key_id: u64,
    pub product: StoreProduct,
}

fn get(
    http: &Client,
    url: &str,
    key: &str,
    query: &[(&str, String)],
    cancel: &AtomicBool,
    counters: &Counters,
) -> Result<String> {
    http::get_text(
        http,
        url,
        |r| r.header(AUTHORIZATION, key).query(query),
        cancel,
        counters,
    )
    .map_err(|e| match e {
        Error::Http {
            status: 401 | 403, ..
        } => Error::Invalid("itch_key"),
        other => other,
    })
}

/// Checks the key and returns whose it is.
pub fn profile(
    http: &Client,
    endpoints: &StoreEndpoints,
    key: &str,
    cancel: &AtomicBool,
    counters: &Counters,
) -> Result<ItchUser> {
    if key.trim().is_empty() || key.len() > 200 {
        return Err(Error::Invalid("itch_key"));
    }
    let url = format!("{}/profile", endpoints.itch_api);
    parse_profile(&get(http, &url, key.trim(), &[], cancel, counters)?)
}

pub fn parse_profile(json: &str) -> Result<ItchUser> {
    #[derive(Deserialize)]
    struct Raw {
        user: Option<RawUser>,
    }
    let user = serde_json::from_str::<Raw>(json)?
        .user
        .ok_or(Error::Invalid("itch_key"))?;
    Ok(ItchUser {
        id: user.id,
        username: user
            .display_name
            .filter(|d| !d.is_empty())
            .or(user.username)
            .unwrap_or_default(),
    })
}

/// Every game the user owns (bought, claimed or bundled), page by page.
pub fn owned_games(
    http: &Client,
    endpoints: &StoreEndpoints,
    key: &str,
    pacer: &Pacer,
    cancel: &AtomicBool,
    counters: &Counters,
) -> Result<Vec<OwnedGame>> {
    let url = format!("{}/profile/owned-keys", endpoints.itch_api);
    let mut out = Vec::new();
    for page in 1..=MAX_PAGES {
        pacer.wait(cancel)?;
        let body = get(
            http,
            &url,
            key,
            &[
                ("page", page.to_string()),
                ("per_page", PAGE_SIZE.to_string()),
            ],
            cancel,
            counters,
        )?;
        let batch = parse_owned_keys(&body)?;
        if batch.is_empty() {
            break;
        }
        out.extend(batch);
    }
    Ok(out)
}

pub fn parse_owned_keys(json: &str) -> Result<Vec<OwnedGame>> {
    #[derive(Deserialize)]
    struct Raw {
        #[serde(default, alias = "ownedKeys")]
        owned_keys: Vec<RawKey>,
    }
    #[derive(Deserialize)]
    struct RawKey {
        id: u64,
        game: Option<RawGame>,
    }
    let raw: Raw = serde_json::from_str(json)?;
    Ok(raw
        .owned_keys
        .into_iter()
        .filter_map(|k| {
            Some(OwnedGame {
                download_key_id: k.id,
                product: product_from(k.game?)?,
            })
        })
        .collect())
}

/// itch.io's search, for finding a Steam game there.
pub fn search(
    http: &Client,
    endpoints: &StoreEndpoints,
    key: &str,
    query: &str,
    cancel: &AtomicBool,
    counters: &Counters,
) -> Result<Vec<StoreProduct>> {
    let url = format!("{}/search/games", endpoints.itch_api);
    let body = get(
        http,
        &url,
        key,
        &[("query", query.to_owned())],
        cancel,
        counters,
    )?;
    parse_search(&body)
}

pub fn parse_search(json: &str) -> Result<Vec<StoreProduct>> {
    #[derive(Deserialize)]
    struct Raw {
        #[serde(default)]
        games: Vec<RawGame>,
    }
    let raw: Raw = serde_json::from_str(json)?;
    Ok(raw.games.into_iter().filter_map(product_from).collect())
}

fn product_from(g: RawGame) -> Option<StoreProduct> {
    let title = g.title.trim().to_owned();
    if title.is_empty() {
        return None;
    }
    // Only games: not tools, assets, soundtracks, books or comics.
    if g.classification.as_deref().is_some_and(|c| c != "game") {
        return None;
    }
    let has = |trait_name: &str, flag: Option<bool>| {
        flag.unwrap_or(false) || g.traits.iter().any(|t| t == trait_name)
    };
    let min_price = g.min_price.unwrap_or(0);
    Some(StoreProduct {
        store: Store::Itch,
        product_id: g.id.to_string(),
        kind: "game".into(),
        title,
        slug: None,
        url: g.url,
        developers: g
            .user
            .and_then(|u| u.display_name.filter(|d| !d.is_empty()).or(u.username))
            .into_iter()
            .collect(),
        publishers: Vec::new(),
        release_date: g
            .published_at
            .or(g.created_at)
            .as_deref()
            .and_then(parse_date),
        store_release_date: None,
        cover: None,
        cover_wide: g.cover_url.filter(|u| !u.is_empty()),
        win: has("p_windows", g.p_windows),
        mac: has("p_osx", g.p_osx),
        linux: has("p_linux", g.p_linux),
        price: (min_price > 0).then(|| format!("${}.{:02}", min_price / 100, min_price % 100)),
        is_free: min_price == 0,
    })
}

#[derive(Deserialize)]
struct RawUser {
    #[serde(default)]
    id: u64,
    username: Option<String>,
    #[serde(alias = "displayName")]
    display_name: Option<String>,
}

#[derive(Deserialize)]
struct RawGame {
    id: u64,
    #[serde(default)]
    title: String,
    url: Option<String>,
    #[serde(alias = "coverUrl")]
    cover_url: Option<String>,
    classification: Option<String>,
    #[serde(alias = "minPrice")]
    min_price: Option<i64>,
    #[serde(alias = "publishedAt")]
    published_at: Option<String>,
    #[serde(alias = "createdAt")]
    created_at: Option<String>,
    #[serde(default)]
    traits: Vec<String>,
    p_windows: Option<bool>,
    p_osx: Option<bool>,
    p_linux: Option<bool>,
    user: Option<RawUser>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_profile() {
        let u = parse_profile(r#"{"user":{"id":7,"username":"leafo","display_name":"Leaf","url":"https://leafo.itch.io"}}"#)
            .unwrap();
        assert_eq!((u.id, u.username.as_str()), (7, "Leaf"));
        assert!(matches!(
            parse_profile(r#"{"errors":["invalid key"]}"#),
            Err(Error::Invalid("itch_key"))
        ));
    }

    #[test]
    fn parses_owned_keys_with_traits_or_flags() {
        let json = r#"{"page":1,"per_page":500,"owned_keys":[
          {"id":101,"game_id":1,"game":{"id":1,"title":"Celeste Classic","url":"https://mattmakesgames.itch.io/celesteclassic",
            "cover_url":"https://img.itch.zone/a.png","classification":"game","min_price":0,
            "traits":["p_windows","p_osx"],"published_at":"2015-08-20 10:00:00","user":{"id":3,"username":"mattmakesgames","display_name":"Maddy"}}},
          {"id":102,"game_id":2,"game":{"id":2,"title":"Paid Game","classification":"game","min_price":1499,"p_windows":true,"p_linux":true,
            "user":{"id":4,"username":"dev"}}},
          {"id":103,"game_id":3,"game":{"id":3,"title":"A Soundtrack","classification":"soundtrack"}}
        ]}"#;
        let owned = parse_owned_keys(json).unwrap();
        assert_eq!(owned.len(), 2);
        let a = &owned[0];
        assert_eq!(
            (a.download_key_id, a.product.product_id.as_str()),
            (101, "1")
        );
        assert!(a.product.is_free && a.product.win && a.product.mac && !a.product.linux);
        assert_eq!(a.product.developers, ["Maddy"]);
        assert_eq!(a.product.release_date, Some(1_440_028_800));
        let b = &owned[1].product;
        assert_eq!(b.price.as_deref(), Some("$14.99"));
        assert!(b.win && b.linux && !b.is_free);
        assert_eq!(b.developers, ["dev"]);
    }
}
