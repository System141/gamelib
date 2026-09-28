//! Signing in to GOG and reading the user's library.
//!
//! GOG has no public developer program; like Heroic, Lutris and minigalaxy, GameLib uses the
//! OAuth client of GOG Galaxy. The user signs in on GOG's own page (the desktop app shows it in
//! a separate window), which redirects to `embed.gog.com/on_login_success?…&code=…`; the code is
//! exchanged for an access token (valid one hour) and a refresh token.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::AUTHORIZATION;
use serde::Deserialize;

use super::StoreEndpoints;
use crate::http::{self, Counters, USER_AGENT};
use crate::secrets::GogTokens;
use crate::{Error, Result, unix_now};

pub const CLIENT_ID: &str = "46899977096215655";
pub const CLIENT_SECRET: &str = "9d85c43b1482497dbbce61f6e4aa173a433796eeae2ca8c5f6129f2dc4de46d9";
pub const REDIRECT_URI: &str = "https://embed.gog.com/on_login_success?origin=client";
/// Where the login page sends the browser once signed in.
pub const REDIRECT_PREFIX: &str = "https://embed.gog.com/on_login_success";

/// A client that does not follow redirects: GOG answers requests without a valid token with a
/// redirect to its login page, which must read as "signed out", not as a page.
pub fn client() -> Result<Client> {
    Ok(Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .gzip(true)
        .build()?)
}

/// GOG's sign-in page.
pub fn login_url(endpoints: &StoreEndpoints) -> String {
    format!(
        "{}/auth?client_id={CLIENT_ID}&redirect_uri={}&response_type=code&layout=galaxy",
        endpoints.gog_auth,
        encode(REDIRECT_URI)
    )
}

/// The authorization code from the page GOG redirects to after sign-in, or a code pasted alone.
pub fn code_from_redirect(input: &str) -> Option<String> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if !input.contains("://") {
        return input
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            .then(|| input.to_owned());
    }
    let url = reqwest::Url::parse(input).ok()?;
    url.query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.into_owned())
        .filter(|v| !v.is_empty())
}

/// Exchanges a sign-in code for tokens.
pub fn exchange_code(
    http: &Client,
    endpoints: &StoreEndpoints,
    code: &str,
    cancel: &AtomicBool,
    counters: &Counters,
) -> Result<GogTokens> {
    token_request(
        http,
        endpoints,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", REDIRECT_URI),
        ],
        cancel,
        counters,
    )
    .map_err(|e| match e {
        Error::Http {
            status: 400..=499, ..
        } => Error::Invalid("gog_code"),
        other => other,
    })
}

/// New tokens for an expiring session. A refusal means the session is over.
pub fn refresh(
    http: &Client,
    endpoints: &StoreEndpoints,
    tokens: &GogTokens,
    cancel: &AtomicBool,
    counters: &Counters,
) -> Result<GogTokens> {
    let mut fresh = token_request(
        http,
        endpoints,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", &tokens.refresh_token),
        ],
        cancel,
        counters,
    )
    .map_err(|e| match e {
        Error::Http {
            status: 400..=499, ..
        } => Error::Invalid("gog_session"),
        other => other,
    })?;
    if fresh.username.is_none() {
        fresh.username.clone_from(&tokens.username);
    }
    Ok(fresh)
}

fn token_request(
    http: &Client,
    endpoints: &StoreEndpoints,
    params: &[(&str, &str)],
    cancel: &AtomicBool,
    counters: &Counters,
) -> Result<GogTokens> {
    let url = format!("{}/token", endpoints.gog_auth);
    let body = http::get_text(
        http,
        &url,
        |r| {
            r.query(&[("client_id", CLIENT_ID), ("client_secret", CLIENT_SECRET)])
                .query(params)
        },
        cancel,
        counters,
    )?;
    parse_tokens(&body, unix_now())
}

pub fn parse_tokens(json: &str, now: i64) -> Result<GogTokens> {
    #[derive(Deserialize)]
    struct Raw {
        access_token: Option<String>,
        refresh_token: Option<String>,
        expires_in: Option<i64>,
        user_id: Option<serde_json::Value>,
    }
    let raw: Raw = serde_json::from_str(json)?;
    let (Some(access_token), Some(refresh_token)) = (raw.access_token, raw.refresh_token) else {
        return Err(Error::Parse("GOG token response without tokens".into()));
    };
    Ok(GogTokens {
        access_token,
        refresh_token,
        expires_at: now + raw.expires_in.unwrap_or(3600),
        user_id: match raw.user_id {
            Some(serde_json::Value::String(s)) => s,
            Some(v) => v.to_string(),
            None => String::new(),
        },
        username: None,
    })
}

/// Whether the access token needs refreshing before use (a minute of margin).
pub fn expiring(tokens: &GogTokens, now: i64) -> bool {
    tokens.expires_at - now < 60
}

fn get_with_token(
    http: &Client,
    url: &str,
    token: &str,
    cancel: &AtomicBool,
    counters: &Counters,
) -> Result<String> {
    let bearer = format!("Bearer {token}");
    http::get_text(
        http,
        url,
        |r| r.header(AUTHORIZATION, &bearer),
        cancel,
        counters,
    )
    .map_err(|e| match e {
        // A redirect to the login page or a refusal: the token is not valid (any more).
        Error::Http {
            status: 300..=399 | 401 | 403,
            ..
        } => Error::Invalid("gog_session"),
        other => other,
    })
}

/// The signed-in user's name.
pub fn username(
    http: &Client,
    endpoints: &StoreEndpoints,
    token: &str,
    cancel: &AtomicBool,
    counters: &Counters,
) -> Result<Option<String>> {
    #[derive(Deserialize)]
    struct Raw {
        username: Option<String>,
    }
    let url = format!("{}/userData.json", endpoints.gog_embed);
    let raw: Raw = serde_json::from_str(&get_with_token(http, &url, token, cancel, counters)?)?;
    Ok(raw.username.filter(|u| !u.is_empty()))
}

/// Ids of the products the user owns (games, DLC and packs).
pub fn owned_ids(
    http: &Client,
    endpoints: &StoreEndpoints,
    token: &str,
    cancel: &AtomicBool,
    counters: &Counters,
) -> Result<Vec<String>> {
    let url = format!("{}/user/data/games", endpoints.gog_embed);
    parse_owned(&get_with_token(http, &url, token, cancel, counters)?)
}

pub fn parse_owned(json: &str) -> Result<Vec<String>> {
    #[derive(Deserialize)]
    struct Raw {
        #[serde(default)]
        owned: Vec<serde_json::Value>,
    }
    let raw: Raw = serde_json::from_str(json)?;
    Ok(raw
        .owned
        .into_iter()
        .filter_map(|v| match v {
            serde_json::Value::Number(n) => Some(n.to_string()),
            serde_json::Value::String(s) if !s.is_empty() => Some(s),
            _ => None,
        })
        .collect())
}

fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_url_encodes_the_redirect() {
        let url = login_url(&StoreEndpoints::default());
        assert!(url.starts_with("https://auth.gog.com/auth?client_id=46899977096215655&redirect_uri=https%3A%2F%2Fembed.gog.com%2Fon_login_success%3Forigin%3Dclient&"));
        assert!(url.ends_with("&response_type=code&layout=galaxy"));
    }

    #[test]
    fn codes_from_redirects() {
        assert_eq!(
            code_from_redirect(
                "https://embed.gog.com/on_login_success?origin=client&code=AbC-12_x"
            )
            .as_deref(),
            Some("AbC-12_x")
        );
        assert_eq!(
            code_from_redirect("  AbC-12_x ").as_deref(),
            Some("AbC-12_x")
        );
        assert_eq!(
            code_from_redirect("https://embed.gog.com/on_login_success?origin=client"),
            None
        );
        assert_eq!(code_from_redirect("not a code!"), None);
        assert_eq!(code_from_redirect(""), None);
    }

    #[test]
    fn parses_tokens_and_owned_ids() {
        let t = parse_tokens(
            r#"{"expires_in":3600,"scope":"","token_type":"bearer","access_token":"a","user_id":"4812","refresh_token":"r","session_id":"s"}"#,
            1000,
        )
        .unwrap();
        assert_eq!(
            (
                t.access_token.as_str(),
                t.refresh_token.as_str(),
                t.expires_at
            ),
            ("a", "r", 4600)
        );
        assert_eq!(t.user_id, "4812");
        assert!(!expiring(&t, 1000) && expiring(&t, 4550));
        assert!(parse_tokens(r#"{"error":"invalid_grant"}"#, 0).is_err());

        assert_eq!(
            parse_owned(r#"{"owned":[1207664643, "1453375253", null]}"#).unwrap(),
            ["1207664643", "1453375253"]
        );
    }
}
