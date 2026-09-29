//! Follows a link's HTTP redirects to report where it really ends up.
//!
//! Only standard 3xx redirects are followed. Pages that need JavaScript, a captcha or a countdown
//! before the real link appears are reported as they are (usually an HTML page) and are meant to
//! be opened in the browser; site-specific steps belong in that site's [`super::SiteHandler`].

use std::collections::HashSet;
use std::time::Duration;

use reqwest::blocking::{Client, Response};
use reqwest::header::{
    CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, HeaderMap, LOCATION, RANGE,
};
use reqwest::redirect::Policy;
use reqwest::{StatusCode, Url};

use crate::error::error_chain;
use crate::model::{CheckStatus, Hop, LinkCheck};
use crate::text::percent_decode;
use crate::{Result, unix_now};

pub const MAX_REDIRECTS: usize = 10;

const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (compatible; GameLib/",
    env!("CARGO_PKG_VERSION"),
    " link-check)"
);

/// HTTP client for link checks: never follows redirects on its own so each hop can be recorded.
pub fn client() -> Result<Client> {
    Ok(Client::builder()
        .user_agent(USER_AGENT)
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build()?)
}

pub fn follow_redirects(start: &Url, http: &Client) -> LinkCheck {
    let checked_at = unix_now();
    let mut hops: Vec<Hop> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut current = start.clone();

    loop {
        if !seen.insert(current.as_str().to_owned()) {
            return failure(
                CheckStatus::Loop,
                &current,
                hops,
                checked_at,
                "redirect loop".into(),
            );
        }
        let resp = match request(http, &current) {
            Ok(resp) => resp,
            Err(e) => return failure(classify(&e), &current, hops, checked_at, error_chain(&e)),
        };
        let status = resp.status();
        hops.push(Hop {
            url: current.to_string(),
            status: status.as_u16(),
        });

        if status.is_redirection() && status != StatusCode::NOT_MODIFIED {
            let location = resp
                .headers()
                .get(LOCATION)
                .and_then(|v| v.to_str().ok())
                .map(str::trim);
            let Some(next) = location.and_then(|loc| current.join(loc).ok()) else {
                return failure(
                    CheckStatus::Broken,
                    &current,
                    hops,
                    checked_at,
                    "redirect without a valid Location".into(),
                );
            };
            if !matches!(next.scheme(), "http" | "https") {
                let mut check = failure(
                    CheckStatus::UnsupportedScheme,
                    &next,
                    hops,
                    checked_at,
                    format!("redirects to {}:", next.scheme()),
                );
                check.http_status = Some(status.as_u16());
                return check;
            }
            if hops.len() > MAX_REDIRECTS {
                return failure(
                    CheckStatus::TooManyRedirects,
                    &next,
                    hops,
                    checked_at,
                    format!("more than {MAX_REDIRECTS} redirects"),
                );
            }
            current = next;
            continue;
        }

        return final_check(status, resp.headers(), &current, hops, checked_at);
    }
}

/// HEAD first; servers that refuse or misreport HEAD get a 1-byte ranged GET whose body is never read.
fn request(http: &Client, url: &Url) -> reqwest::Result<Response> {
    let resp = http.head(url.clone()).send()?;
    match resp.status().as_u16() {
        400 | 403 | 404 | 405 | 501 => {
            drop(resp);
            http.get(url.clone()).header(RANGE, "bytes=0-0").send()
        }
        _ => Ok(resp),
    }
}

fn final_check(
    status: StatusCode,
    headers: &HeaderMap,
    url: &Url,
    hops: Vec<Hop>,
    checked_at: i64,
) -> LinkCheck {
    let header = |name| {
        headers
            .get(name)
            .and_then(|v: &reqwest::header::HeaderValue| v.to_str().ok())
    };
    let content_type = header(CONTENT_TYPE)
        .and_then(|ct| ct.split(';').next())
        .map(|ct| ct.trim().to_ascii_lowercase())
        .filter(|ct| !ct.is_empty());
    let disposition = header(CONTENT_DISPOSITION);
    let attachment = disposition.is_some_and(|d| {
        d.trim_start()
            .to_ascii_lowercase()
            .starts_with("attachment")
    });
    let is_page = content_type.as_deref().is_none_or(|ct| {
        ct.starts_with("text/") || ct.contains("html") || ct.contains("json") || ct.contains("xml")
    });
    let is_file = status.is_success() && (attachment || !is_page);

    let size = header(CONTENT_RANGE)
        .and_then(|r| r.rsplit('/').next())
        .and_then(|total| total.trim().parse::<u64>().ok())
        .or_else(|| {
            if status == StatusCode::OK {
                header(CONTENT_LENGTH).and_then(|l| l.trim().parse().ok())
            } else {
                None
            }
        });

    let file_name = if is_file {
        disposition
            .and_then(disposition_file_name)
            .or_else(|| file_name_from_url(url))
    } else {
        None
    };

    LinkCheck {
        status: final_status(status),
        http_status: Some(status.as_u16()),
        final_url: Some(url.to_string()),
        final_host: url.host_str().map(str::to_owned),
        hops,
        file_name,
        size_bytes: if is_file { size } else { None },
        content_type,
        is_file,
        checked_at,
        message: None,
    }
}

fn failure(
    status: CheckStatus,
    url: &Url,
    hops: Vec<Hop>,
    checked_at: i64,
    message: String,
) -> LinkCheck {
    LinkCheck {
        status,
        http_status: hops.last().map(|h| h.status),
        final_url: Some(url.to_string()),
        final_host: url.host_str().map(str::to_owned),
        hops,
        file_name: None,
        size_bytes: None,
        content_type: None,
        is_file: false,
        checked_at,
        message: Some(message),
    }
}

/// The final response's status code as a user-facing kind — a refusal, a gone page or a server
/// fault each suggest a different next step, so they are not collapsed into one "broken".
fn final_status(code: StatusCode) -> CheckStatus {
    if code.is_success() {
        return CheckStatus::Ok;
    }
    match code.as_u16() {
        401 | 403 | 429 => CheckStatus::Restricted,
        404 | 410 => CheckStatus::NotFound,
        500..=599 => CheckStatus::ServerError,
        _ => CheckStatus::Broken,
    }
}

fn classify(e: &reqwest::Error) -> CheckStatus {
    if e.is_timeout() {
        return CheckStatus::Timeout;
    }
    let text = error_chain(e).to_ascii_lowercase();
    if ["certificate", "tls", "ssl", "handshake"]
        .iter()
        .any(|k| text.contains(k))
    {
        CheckStatus::Tls
    } else {
        CheckStatus::Network
    }
}

/// File name from a `Content-Disposition` header; RFC 5987 `filename*=` wins over `filename=`.
pub fn disposition_file_name(value: &str) -> Option<String> {
    let mut plain = None;
    for part in value.split(';') {
        let Some((key, val)) = part.split_once('=') else {
            continue;
        };
        let val = val.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            "filename*" => {
                let encoded = val.splitn(3, '\'').nth(2).unwrap_or(val);
                if let Some(name) = sanitize_file_name(&percent_decode(encoded.trim_matches('"'))) {
                    return Some(name);
                }
            }
            "filename" => plain = sanitize_file_name(val.trim_matches('"')),
            _ => {}
        }
    }
    plain
}

fn file_name_from_url(url: &Url) -> Option<String> {
    let last = url.path_segments()?.rfind(|s| !s.is_empty())?;
    let name = percent_decode(last);
    if name.contains('.') {
        sanitize_file_name(&name)
    } else {
        None
    }
}

/// Keeps only the last path component, trimmed, at most 200 characters.
fn sanitize_file_name(name: &str) -> Option<String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).trim();
    if base.is_empty() || base == "." || base == ".." {
        return None;
    }
    Some(base.chars().take(200).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_statuses_map_to_kinds() {
        assert_eq!(final_status(StatusCode::OK), CheckStatus::Ok);
        assert_eq!(final_status(StatusCode::FORBIDDEN), CheckStatus::Restricted);
        assert_eq!(final_status(StatusCode::NOT_FOUND), CheckStatus::NotFound);
        assert_eq!(
            final_status(StatusCode::INTERNAL_SERVER_ERROR),
            CheckStatus::ServerError
        );
        assert_eq!(
            final_status(StatusCode::NOT_MODIFIED),
            CheckStatus::Broken,
            "a 3xx that is not a redirect is still broken"
        );
    }

    #[test]
    fn parses_disposition_names() {
        assert_eq!(
            disposition_file_name("attachment; filename=\"Game Setup.exe\"").as_deref(),
            Some("Game Setup.exe")
        );
        assert_eq!(
            disposition_file_name("attachment; filename=game.zip").as_deref(),
            Some("game.zip")
        );
        assert_eq!(
            disposition_file_name("attachment; filename=\"fallback.zip\"; filename*=UTF-8''%C3%A7%C4%B1lg%C4%B1n%20oyun.zip").as_deref(),
            Some("çılgın oyun.zip")
        );
        assert_eq!(
            disposition_file_name("attachment; filename=\"../../etc/passwd\"").as_deref(),
            Some("passwd")
        );
        assert_eq!(disposition_file_name("inline").as_deref(), None);
    }

    #[test]
    fn file_name_from_path() {
        let url = Url::parse("https://cdn.example.com/files/My%20Game%201.2.zip?sig=x").unwrap();
        assert_eq!(file_name_from_url(&url).as_deref(), Some("My Game 1.2.zip"));
        let url = Url::parse("https://example.com/download/").unwrap();
        assert_eq!(file_name_from_url(&url), None);
    }
}
