//! GameLib core: everything that does not need a window.
//!
//! - [`steam`]: keyless Steam store API client (catalog pages, tag names, media).
//! - [`db`]: local SQLite catalog (WAL, FTS5 search) plus user data such as external links.
//! - [`sync`] / [`new_releases`]: full catalog download and on-demand "new releases" refresh.
//! - [`links`]: validation, per-site handlers and redirect checks for non-Steam links.
//! - [`app`]: the command layer shared by the desktop app and `gamelib-cli serve`.

pub mod app;
pub mod db;
pub mod error;
pub mod links;
pub mod model;
pub mod new_releases;
pub mod rating;
pub mod record;
pub mod search;
pub mod steam;
pub mod sync;
mod text;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub use error::{Error, ErrorInfo, ErrorKind, Result};

/// Current Unix time in seconds.
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Sleeps for `total`, waking every 100 ms to honour `cancel`.
pub fn sleep_cancellable(total: Duration, cancel: &AtomicBool) -> Result<()> {
    const SLICE: Duration = Duration::from_millis(100);
    let mut left = total;
    while !left.is_zero() {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let step = left.min(SLICE);
        std::thread::sleep(step);
        left -= step;
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    Ok(())
}
