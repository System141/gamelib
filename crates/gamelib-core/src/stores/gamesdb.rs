//! GOG's GamesDB: a public cross-reference of one game's releases on GOG, Steam, consoles…
//!
//! `/platforms/<platform>/external_releases/<id>` returns the release plus its game with all
//! releases (`platform_id`, `external_id`). It is undocumented, so every lookup is optional:
//! title matching works without it.

use std::sync::atomic::AtomicBool;

use serde::Deserialize;

use super::StoreEndpoints;
use crate::http::{self, Counters, Pacer};
use crate::{Error, Result};

/// The releases GamesDB links to one GOG product or Steam app.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Releases {
    /// Kind of the looked-up release: "game", "dlc", "pack"… ("spam" for bundles).
    pub kind: String,
    pub steam: Vec<u32>,
    pub gog: Vec<String>,
}

impl Releases {
    /// Bundles and DLC are never tied to a Steam game.
    pub fn is_game(&self) -> bool {
        self.kind == "game"
    }
}

/// Releases of the game that GOG product `id` belongs to. `None` if GamesDB does not know it.
pub fn for_gog(
    http: &reqwest::blocking::Client,
    endpoints: &StoreEndpoints,
    id: &str,
    pacer: &Pacer,
    cancel: &AtomicBool,
    counters: &Counters,
) -> Result<Option<Releases>> {
    lookup(http, endpoints, "gog", id, pacer, cancel, counters)
}

/// Releases of the game that Steam app `appid` belongs to.
pub fn for_steam(
    http: &reqwest::blocking::Client,
    endpoints: &StoreEndpoints,
    appid: u32,
    pacer: &Pacer,
    cancel: &AtomicBool,
    counters: &Counters,
) -> Result<Option<Releases>> {
    lookup(
        http,
        endpoints,
        "steam",
        &appid.to_string(),
        pacer,
        cancel,
        counters,
    )
}

fn lookup(
    http: &reqwest::blocking::Client,
    endpoints: &StoreEndpoints,
    platform: &str,
    id: &str,
    pacer: &Pacer,
    cancel: &AtomicBool,
    counters: &Counters,
) -> Result<Option<Releases>> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(None);
    }
    pacer.wait(cancel)?;
    let url = format!(
        "{}/platforms/{platform}/external_releases/{id}",
        endpoints.gamesdb
    );
    match http::get_text(http, &url, |r| r, cancel, counters) {
        Ok(body) => parse(&body).map(Some),
        Err(Error::Http { status: 404, .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn parse(json: &str) -> Result<Releases> {
    let raw: RawRelease = serde_json::from_str(json)?;
    let mut out = Releases {
        kind: raw.kind.unwrap_or_default(),
        ..Releases::default()
    };
    for r in raw.game.map(|g| g.releases).unwrap_or_default() {
        let id = r.external_id.trim();
        // Some entries are junk ("steam_292030", test ids); keep plain numeric ids only.
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        match r.platform_id.as_str() {
            "steam" => {
                if let Ok(appid) = id.parse::<u32>()
                    && appid > 0
                    && !out.steam.contains(&appid)
                {
                    out.steam.push(appid);
                }
            }
            "gog" => {
                if !out.gog.iter().any(|g| g == id) {
                    out.gog.push(id.to_owned());
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

#[derive(Deserialize)]
struct RawRelease {
    #[serde(rename = "type")]
    kind: Option<String>,
    game: Option<RawGame>,
}

#[derive(Deserialize)]
struct RawGame {
    #[serde(default)]
    releases: Vec<RawExternal>,
}

#[derive(Deserialize)]
struct RawExternal {
    #[serde(default)]
    platform_id: String,
    #[serde(default)]
    external_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_numeric_steam_and_gog_ids() {
        let json = r#"{"type":"game","title":{"*":"The Witcher 3: Wild Hunt"},"game":{"releases":[
            {"platform_id":"steam","external_id":"292030"},
            {"platform_id":"xboxone","external_id":"1799887933"},
            {"platform_id":"gog","external_id":"1207664643"},
            {"platform_id":"gog","external_id":"1425895904"},
            {"platform_id":"steam","external_id":"steam_292030"},
            {"platform_id":"steam","external_id":"292030"},
            {"platform_id":"origin","external_id":"Origin.OFR.50.0001017"}]}}"#;
        let r = parse(json).unwrap();
        assert!(r.is_game());
        assert_eq!(r.steam, [292030]);
        assert_eq!(r.gog, ["1207664643", "1425895904"]);
    }

    #[test]
    fn bundles_are_not_games() {
        let r = parse(
            r#"{"type":"spam","game":{"releases":[{"platform_id":"gog","external_id":"10"}]}}"#,
        )
        .unwrap();
        assert!(!r.is_game());
        assert!(r.steam.is_empty());
    }

    #[test]
    fn missing_game_is_empty() {
        assert_eq!(
            parse(r#"{"type":"game"}"#).unwrap().steam,
            Vec::<u32>::new()
        );
    }
}
