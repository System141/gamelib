//! The store's `appdetails` service (no key needed), for what `GetItems` does not have: system
//! requirements.

use std::sync::atomic::AtomicBool;

use reqwest::blocking::Client;
use serde_json::Value;

use crate::http::{self, Counters};
use crate::model::Platform;
use crate::{Error, Result};

/// Minimum and recommended requirements (HTML lists as Steam writes them) of one system.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequirementsHtml {
    pub minimum: String,
    pub recommended: String,
}

/// Requirements for each system. Steam sends `[]` instead of an object for a system without
/// any; English labels keep them parseable.
pub fn requirements(
    client: &Client,
    base: &str,
    appid: u32,
    cancel: &AtomicBool,
    counters: &Counters,
) -> Result<Vec<(Platform, RequirementsHtml)>> {
    let url = format!("{}/api/appdetails", base.trim_end_matches('/'));
    let appid_text = appid.to_string();
    let body: Value = http::get_json(
        client,
        &url,
        |r| {
            r.query(&[
                ("appids", appid_text.as_str()),
                ("filters", "basic"),
                ("l", "english"),
            ])
        },
        cancel,
        counters,
    )?;
    parse(&body, appid)
}

fn parse(body: &Value, appid: u32) -> Result<Vec<(Platform, RequirementsHtml)>> {
    let entry = &body[appid.to_string()];
    if entry["success"].as_bool() != Some(true) {
        return Err(Error::NotFound);
    }
    let data = &entry["data"];
    Ok([
        (Platform::Win, "pc_requirements"),
        (Platform::Mac, "mac_requirements"),
        (Platform::Linux, "linux_requirements"),
    ]
    .into_iter()
    .map(|(platform, key)| {
        let field = |name: &str| data[key][name].as_str().unwrap_or("").to_owned();
        (
            platform,
            RequirementsHtml {
                minimum: field("minimum"),
                recommended: field("recommended"),
            },
        )
    })
    .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_each_systems_requirements() {
        let body = serde_json::json!({
            "620": { "success": true, "data": {
                "pc_requirements": { "minimum": "<ul><li><strong>OS:</strong> Windows 7</li></ul>" },
                "mac_requirements": [],
                "linux_requirements": { "minimum": "", "recommended": "" }
            }}
        });
        let all = parse(&body, 620).unwrap();
        assert_eq!(all[0].0, Platform::Win);
        assert!(all[0].1.minimum.contains("Windows 7"));
        assert_eq!(all[1].1, RequirementsHtml::default());
        assert_eq!(all[2].1, RequirementsHtml::default());
        let missing = serde_json::json!({ "620": { "success": false } });
        assert!(matches!(parse(&missing, 620), Err(Error::NotFound)));
    }
}
