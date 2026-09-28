//! Validation and clean-up of user-entered link URLs.

use reqwest::Url;

use crate::{Error, Result};

pub const MAX_URL_LEN: usize = 2048;

/// Parses a pasted link. Only `http`/`https` URLs with a host and without embedded credentials
/// are accepted; a missing scheme defaults to `https://`.
///
/// Errors carry stable codes (`url_empty`, `url_too_long`, `url_parse`, `url_scheme`,
/// `url_host`, `url_credentials`) that the UI translates.
pub fn parse_link_url(input: &str) -> Result<Url> {
    let s = input.trim();
    if s.is_empty() {
        return Err(Error::Invalid("url_empty"));
    }
    if s.len() > MAX_URL_LEN {
        return Err(Error::Invalid("url_too_long"));
    }
    let candidate = if s.contains("://") {
        s.to_owned()
    } else {
        format!("https://{s}")
    };
    let url = Url::parse(&candidate).map_err(|_| Error::Invalid("url_parse"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error::Invalid("url_scheme"));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(Error::Invalid("url_host"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::Invalid("url_credentials"));
    }
    Ok(url)
}

/// Plain HTTP link (can be tampered with in transit).
pub fn is_insecure(url: &Url) -> bool {
    url.scheme() == "http"
}

/// Removes common tracking parameters. The query is left untouched when there is nothing to remove.
pub fn strip_tracking(mut url: Url) -> Url {
    if url.query().is_none() {
        return url;
    }
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let kept: Vec<&(String, String)> = pairs
        .iter()
        .filter(|(k, _)| !is_tracking_param(k))
        .collect();
    if kept.len() == pairs.len() {
        return url;
    }
    if kept.is_empty() {
        url.set_query(None);
    } else {
        url.query_pairs_mut()
            .clear()
            .extend_pairs(kept.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    }
    url
}

fn is_tracking_param(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.starts_with("utm_")
        || matches!(
            key.as_str(),
            "fbclid"
                | "gclid"
                | "dclid"
                | "gbraid"
                | "wbraid"
                | "msclkid"
                | "yclid"
                | "igshid"
                | "mc_cid"
                | "mc_eid"
                | "_hsenc"
                | "_hsmi"
        )
}

/// Trims optional free text and enforces a length limit (in characters).
pub fn optional_text(
    value: Option<&str>,
    max_chars: usize,
    too_long: &'static str,
) -> Result<Option<String>> {
    let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    if v.chars().count() > max_chars {
        return Err(Error::Invalid(too_long));
    }
    Ok(Some(v.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(input: &str) -> &'static str {
        match parse_link_url(input) {
            Err(Error::Invalid(code)) => code,
            other => panic!("expected invalid for {input:?}, got {other:?}"),
        }
    }

    #[test]
    fn accepts_http_and_https() {
        assert_eq!(
            parse_link_url(" https://example.com/game.zip ")
                .unwrap()
                .as_str(),
            "https://example.com/game.zip"
        );
        assert_eq!(
            parse_link_url("http://example.com").unwrap().scheme(),
            "http"
        );
        assert!(is_insecure(&parse_link_url("http://example.com").unwrap()));
    }

    #[test]
    fn defaults_to_https() {
        assert_eq!(
            parse_link_url("example.com/files/setup.exe")
                .unwrap()
                .as_str(),
            "https://example.com/files/setup.exe"
        );
    }

    #[test]
    fn rejects_dangerous_or_broken_input() {
        assert_eq!(code(""), "url_empty");
        assert_eq!(code("   "), "url_empty");
        assert_eq!(code("javascript:alert(1)"), "url_parse");
        assert_eq!(code("file:///etc/passwd"), "url_scheme");
        assert_eq!(code("ftp://example.com/x"), "url_scheme");
        assert_eq!(code("steam://run/570"), "url_scheme");
        assert_eq!(code("https://user:pass@example.com/"), "url_credentials");
        assert_eq!(code("mailto:someone@example.com"), "url_credentials");
        assert_eq!(
            code(&format!("https://example.com/{}", "a".repeat(MAX_URL_LEN))),
            "url_too_long"
        );
    }

    #[test]
    fn handles_internationalized_hosts() {
        let url = parse_link_url("https://örnek.com.tr/oyun").unwrap();
        assert_eq!(url.host_str(), Some("xn--rnek-4qa.com.tr"));
    }

    #[test]
    fn strips_tracking_params_only() {
        let url = Url::parse("https://example.com/dl?id=5&utm_source=x&fbclid=abc").unwrap();
        assert_eq!(strip_tracking(url).as_str(), "https://example.com/dl?id=5");
        let url = Url::parse("https://example.com/dl?utm_campaign=y").unwrap();
        assert_eq!(strip_tracking(url).as_str(), "https://example.com/dl");
        let untouched = "https://cdn.example.com/f.zip?X-Amz-Signature=a%2Fb&Expires=1";
        assert_eq!(
            strip_tracking(Url::parse(untouched).unwrap()).as_str(),
            untouched
        );
    }

    #[test]
    fn optional_text_limits() {
        assert_eq!(optional_text(Some("  "), 5, "x").unwrap(), None);
        assert_eq!(
            optional_text(Some(" v1.2 "), 5, "x").unwrap().as_deref(),
            Some("v1.2")
        );
        assert!(matches!(
            optional_text(Some("çok uzun"), 5, "too_long"),
            Err(Error::Invalid("too_long"))
        ));
    }
}
