//! Shared HTTP plumbing: the client every API call uses, GET with retries, and request pacing.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use reqwest::StatusCode;
use reqwest::blocking::{Client, RequestBuilder, Response};
use reqwest::header::RETRY_AFTER;
use serde::de::DeserializeOwned;

use crate::{Error, Result, sleep_cancellable};

pub const USER_AGENT: &str = concat!(
    "GameLib/",
    env!("CARGO_PKG_VERSION"),
    " (desktop; +https://github.com/System141/gamelib)"
);
const MAX_ATTEMPTS: u32 = 5;
/// Longest `Retry-After` we are willing to wait.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(120);

/// A client for JSON APIs. Honours `HTTPS_PROXY`/system proxy settings and uses the platform
/// certificate store.
pub fn api_client(timeout: Duration) -> Result<Client> {
    Ok(Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .timeout(timeout)
        .gzip(true)
        .build()?)
}

/// Requests sent and retries made, for reports.
#[derive(Debug, Default)]
pub struct Counters {
    requests: AtomicU32,
    retries: AtomicU32,
}

impl Counters {
    pub fn get(&self) -> (u32, u32) {
        (
            self.requests.load(Ordering::Relaxed),
            self.retries.load(Ordering::Relaxed),
        )
    }
}

/// Sends a GET built by `build` and reads the body, retrying network errors, timeouts, bodies
/// that break off, 429 and 5xx with backoff (or the server's `Retry-After`). Other statuses fail
/// at once as [`Error::Http`].
pub fn get_text(
    client: &Client,
    url: &str,
    build: impl Fn(RequestBuilder) -> RequestBuilder,
    cancel: &AtomicBool,
    counters: &Counters,
) -> Result<String> {
    let mut attempt = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        attempt += 1;
        counters.requests.fetch_add(1, Ordering::Relaxed);
        let mut wait = None;
        let err = match build(client.get(url)).send() {
            Ok(resp) if resp.status().is_success() => match resp.text() {
                Ok(body) => return Ok(body),
                Err(e) => Error::from(e),
            },
            Ok(resp) if resp.status() == StatusCode::TOO_MANY_REQUESTS => {
                wait = retry_after(&resp);
                Error::RateLimited
            }
            Ok(resp) => Error::Http {
                status: resp.status().as_u16(),
                url: url.to_owned(),
            },
            Err(e) => Error::from(e),
        };
        if !err.is_transient() || attempt >= MAX_ATTEMPTS {
            return Err(err);
        }
        counters.retries.fetch_add(1, Ordering::Relaxed);
        sleep_cancellable(wait.unwrap_or_else(|| backoff(attempt)), cancel)?;
    }
}

/// [`get_text`] parsed as JSON.
pub fn get_json<T: DeserializeOwned>(
    client: &Client,
    url: &str,
    build: impl Fn(RequestBuilder) -> RequestBuilder,
    cancel: &AtomicBool,
    counters: &Counters,
) -> Result<T> {
    let body = get_text(client, url, build, cancel, counters)?;
    serde_json::from_str(&body).map_err(|e| Error::Parse(format!("{url}: {e}")))
}

fn retry_after(resp: &Response) -> Option<Duration> {
    resp.headers()
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|s| Duration::from_secs(s).min(MAX_RETRY_AFTER))
}

/// 2, 4, 8, 16… seconds (capped at 30) plus up to 0.5 s of jitter.
pub fn backoff(attempt: u32) -> Duration {
    let base = 2u64.saturating_pow(attempt).min(30);
    let jitter = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_millis() % 500)
        .unwrap_or(0);
    Duration::from_secs(base) + Duration::from_millis(u64::from(jitter))
}

/// Keeps at least `interval` between requests to one service, so bulk jobs stay polite.
#[derive(Debug)]
pub struct Pacer {
    interval: Duration,
    last: Mutex<Option<Instant>>,
}

impl Pacer {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            last: Mutex::new(None),
        }
    }

    /// Waits until the next request may go out.
    pub fn wait(&self, cancel: &AtomicBool) -> Result<()> {
        let mut last = self.last.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(prev) = *last {
            let due = prev + self.interval;
            let now = Instant::now();
            if due > now {
                sleep_cancellable(due - now, cancel)?;
            }
        }
        *last = Some(Instant::now());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_caps() {
        assert!(backoff(1) >= Duration::from_secs(2));
        assert!(backoff(3) >= Duration::from_secs(8));
        assert!(backoff(10) < Duration::from_secs(31));
    }

    #[test]
    fn pacer_spaces_requests() {
        let pacer = Pacer::new(Duration::from_millis(60));
        let cancel = AtomicBool::new(false);
        let start = Instant::now();
        for _ in 0..3 {
            pacer.wait(&cancel).unwrap();
        }
        assert!(start.elapsed() >= Duration::from_millis(120));
    }

    #[test]
    fn pacer_honours_cancel() {
        let pacer = Pacer::new(Duration::from_secs(30));
        let cancel = AtomicBool::new(false);
        pacer.wait(&cancel).unwrap();
        cancel.store(true, Ordering::Relaxed);
        assert!(matches!(pacer.wait(&cancel), Err(Error::Cancelled)));
    }
}
