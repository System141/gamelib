//! Downloading one file into a `.part` file, resuming where a previous attempt stopped.
//!
//! A resumed request sends `Range: bytes=N-` and, when a validator is known, `If-Range` with a
//! strong ETag or Last-Modified: a 206 continues the file, a 200 means the server ignored the
//! range or the file changed, so it starts over. 403/410 mean the signed address expired and
//! must be resolved again.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use md5::{Digest, Md5};
use reqwest::StatusCode;
use reqwest::blocking::Client;
use reqwest::header::{CONTENT_LENGTH, CONTENT_RANGE, ETAG, IF_RANGE, LAST_MODIFIED, RANGE};

use crate::http::USER_AGENT;
use crate::{Error, Result};

const BUFFER: usize = 1 << 20;

/// A client for large downloads. The blocking client applies its timeout to getting the
/// response headers and then to each `read` of the body, so it catches a stalled transfer
/// without cutting a long one short. Redirects are followed (to CDNs), and gzip is off so byte
/// offsets stay exact.
pub fn client() -> Result<Client> {
    Ok(Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(45))
        .redirect(reqwest::redirect::Policy::limited(10))
        .no_gzip()
        .build()?)
}

/// Where to get a file right now: a signed address and the headers it needs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resolved {
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// Expected MD5 (lowercase hex), when the store publishes one.
    pub md5: Option<String>,
    /// The file's name, when the store says it.
    pub name: Option<String>,
}

/// What a previous response said about the file, to resume it safely.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Validator {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

impl Validator {
    fn if_range(&self) -> Option<&str> {
        // Weak ETags are not allowed in If-Range.
        match &self.etag {
            Some(tag) if !tag.starts_with("W/") => Some(tag),
            _ => self.last_modified.as_deref(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Fetched {
    /// The whole file is in the part file.
    Done { size: u64 },
    /// The address expired (403/410); resolve it again and retry.
    Expired,
}

/// Downloads `resolved` into `part`, continuing an existing part. `on_bytes` gets the part's
/// length as it grows. `expected` is the size the store announced, if any.
pub fn fetch(
    client: &Client,
    resolved: &Resolved,
    part: &Path,
    expected: Option<u64>,
    validator: &mut Validator,
    cancel: &AtomicBool,
    on_bytes: &mut dyn FnMut(u64),
) -> Result<Fetched> {
    let mut have = part.metadata().map(|m| m.len()).unwrap_or(0);
    if expected.is_some_and(|size| have > size) {
        truncate(part)?;
        have = 0;
    }
    for _ in 0..2 {
        let mut request = client.get(&resolved.url);
        for (name, value) in &resolved.headers {
            request = request.header(name, value);
        }
        if have > 0 {
            request = request.header(RANGE, format!("bytes={have}-"));
            if let Some(v) = validator.if_range() {
                request = request.header(IF_RANGE, v);
            }
        }
        let response = request.send()?;
        let status = response.status();
        let append = match status {
            StatusCode::PARTIAL_CONTENT => {
                let start = content_range(&response).map(|(start, _)| start);
                if start != Some(have) {
                    // Not the range asked for: start over.
                    truncate(part)?;
                    have = 0;
                    continue;
                }
                true
            }
            s if s.is_success() => {
                have = 0;
                false
            }
            StatusCode::RANGE_NOT_SATISFIABLE => {
                let total = content_range(&response)
                    .and_then(|(_, total)| total)
                    .or(expected);
                if total == Some(have) && have > 0 {
                    return Ok(Fetched::Done { size: have });
                }
                truncate(part)?;
                have = 0;
                continue;
            }
            StatusCode::FORBIDDEN | StatusCode::GONE => return Ok(Fetched::Expired),
            s => {
                return Err(Error::Http {
                    status: s.as_u16(),
                    url: redact(&resolved.url),
                });
            }
        };
        remember(validator, &response);
        let total = if append {
            content_range(&response).and_then(|(_, total)| total)
        } else {
            header_u64(&response, CONTENT_LENGTH.as_str())
        }
        .or(expected);
        let mut file = open(part, append)?;
        let mut body = response;
        let mut buf = vec![0u8; BUFFER];
        loop {
            if cancel.load(Ordering::Relaxed) {
                file.sync_all().ok();
                return Err(Error::Cancelled);
            }
            let n = body.read(&mut buf).map_err(read_error)?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).map_err(|e| io(part, e))?;
            have += n as u64;
            on_bytes(have);
        }
        file.sync_all().map_err(|e| io(part, e))?;
        if let Some(total) = total
            && have != total
        {
            return Err(Error::Network(format!(
                "download ended at {have} of {total} bytes"
            )));
        }
        return Ok(Fetched::Done { size: have });
    }
    Err(Error::Network(
        "the server did not honour the range request".into(),
    ))
}

/// MD5 of a file (lowercase hex), checking `cancel` between blocks.
pub fn md5_file(path: &Path, cancel: &AtomicBool) -> Result<String> {
    let mut file = File::open(path).map_err(|e| io(path, e))?;
    let mut hasher = Md5::new();
    let mut buf = vec![0u8; BUFFER];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let n = file.read(&mut buf).map_err(|e| io(path, e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Renames a finished part file. Antivirus scanners briefly hold new files on Windows (os
/// error 5 or 32), so a failed rename is retried for a few seconds.
pub fn finish_part(part: &Path, target: &Path) -> Result<()> {
    let mut last = None;
    for attempt in 0..25 {
        match std::fs::rename(part, target) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(100 + attempt * 20));
            }
        }
    }
    Err(io(part, last.expect("at least one attempt")))
}

fn open(part: &Path, append: bool) -> Result<File> {
    if let Some(dir) = part.parent() {
        std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    }
    let mut options = OpenOptions::new();
    options.create(true);
    if append {
        options.append(true);
    } else {
        options.write(true).truncate(true);
    }
    options.open(part).map_err(|e| io(part, e))
}

fn truncate(part: &Path) -> Result<()> {
    match File::create(part) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io(part, e)),
    }
}

fn remember(validator: &mut Validator, response: &reqwest::blocking::Response) {
    let get = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    if let Some(etag) = get(ETAG.as_str()) {
        validator.etag = Some(etag);
    }
    if let Some(modified) = get(LAST_MODIFIED.as_str()) {
        validator.last_modified = Some(modified);
    }
}

fn header_u64(response: &reqwest::blocking::Response, name: &str) -> Option<u64> {
    response
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok())
}

/// `Content-Range: bytes 100-199/1000` → (100, Some(1000)); `bytes */1000` → (0, Some(1000)).
fn content_range(response: &reqwest::blocking::Response) -> Option<(u64, Option<u64>)> {
    let value = response.headers().get(CONTENT_RANGE)?.to_str().ok()?;
    parse_content_range(value)
}

pub(crate) fn parse_content_range(value: &str) -> Option<(u64, Option<u64>)> {
    let rest = value.trim().strip_prefix("bytes")?.trim();
    let (range, total) = rest.split_once('/')?;
    let total = total.trim().parse().ok();
    let start = match range.trim() {
        "*" => 0,
        r => r.split_once('-')?.0.trim().parse().ok()?,
    };
    Some((start, total))
}

fn read_error(e: std::io::Error) -> Error {
    if e.kind() == std::io::ErrorKind::TimedOut {
        Error::Timeout("download stalled".into())
    } else {
        Error::Network(format!("download interrupted: {e}"))
    }
}

fn io(path: &Path, e: std::io::Error) -> Error {
    Error::Other(format!("{}: {e}", path.display()))
}

/// A signed address without its query (which carries the signature), for messages.
pub fn redact(url: &str) -> String {
    url.split('?').next().unwrap_or(url).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_ranges() {
        assert_eq!(
            parse_content_range("bytes 100-199/1000"),
            Some((100, Some(1000)))
        );
        assert_eq!(parse_content_range("bytes */1000"), Some((0, Some(1000))));
        assert_eq!(parse_content_range("bytes 0-9/*"), Some((0, None)));
        assert_eq!(parse_content_range("items 1-2/3"), None);
    }

    #[test]
    fn weak_etags_are_not_used_for_if_range() {
        let v = Validator {
            etag: Some("W/\"x\"".into()),
            last_modified: Some("Tue, 01 Sep 2026 10:00:00 GMT".into()),
        };
        assert_eq!(v.if_range(), Some("Tue, 01 Sep 2026 10:00:00 GMT"));
        let v = Validator {
            etag: Some("\"abc\"".into()),
            last_modified: None,
        };
        assert_eq!(v.if_range(), Some("\"abc\""));
    }

    #[test]
    fn redacts_signatures() {
        assert_eq!(
            redact("https://cdn.example/f.exe?token=secret"),
            "https://cdn.example/f.exe"
        );
    }
}
