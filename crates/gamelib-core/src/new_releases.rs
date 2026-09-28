//! On-demand refresh of recently released games.
//!
//! Steam's store query can sort released games by release date, newest first, with no gaps
//! (`sort = 40`). Paging from the top until the dates fall behind the last known release (minus a
//! safety margin) picks up everything released since the previous check — usually one request.

use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use crate::db::Db;
use crate::db::write::{get_meta_i64, meta_keys, refresh_tag_counts, set_meta, upsert_games};
use crate::model::{NewReleasesReport, SyncPhase, SyncProgress, WorkerKind};
use crate::record::GameRecord;
use crate::steam::{CatalogSource, MAX_PAGE_SIZE, PageRequest, SORT_RELEASE_DESC};
use crate::{Result, sleep_cancellable, unix_now};

#[derive(Debug, Clone)]
pub struct NewReleasesOptions {
    pub page_size: u32,
    pub overlap: u32,
    pub delay: Duration,
    /// Give up (and suggest a full sync) after this many pages. About 10 days of releases fit in one page.
    pub max_pages: u32,
    /// Re-check this far behind the watermark, for releases Steam indexed late.
    pub margin_secs: i64,
    /// How far back to go when nothing is known yet (empty catalog).
    pub initial_days: u32,
    /// Ignore the watermark and fetch `initial_days` back from now.
    pub days_override: Option<u32>,
}

impl Default for NewReleasesOptions {
    fn default() -> Self {
        Self {
            page_size: MAX_PAGE_SIZE,
            overlap: 20,
            delay: Duration::from_millis(600),
            max_pages: 20,
            margin_secs: 3 * 86_400,
            initial_days: 30,
            days_override: None,
        }
    }
}

pub fn fetch_new_releases(
    db: &mut Db,
    source: &dyn CatalogSource,
    opts: &NewReleasesOptions,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&SyncProgress),
) -> Result<NewReleasesReport> {
    let clock = Instant::now();
    let run_started = unix_now();
    let page_size = opts.page_size.clamp(1, MAX_PAGE_SIZE);
    let overlap = opts.overlap.min(page_size / 2);

    let watermark = get_meta_i64(db.conn(), meta_keys::RELEASE_WATERMARK)?;
    let since = match (opts.days_override, watermark) {
        (Some(days), _) => run_started - i64::from(days) * 86_400,
        (None, Some(mark)) => mark - opts.margin_secs,
        (None, None) => run_started - i64::from(opts.initial_days) * 86_400,
    };

    let mut report = NewReleasesReport {
        since,
        ..Default::default()
    };
    let mut snapshot = SyncProgress {
        kind: WorkerKind::NewReleases,
        phase: SyncPhase::NewReleases,
        fetched: 0,
        total: 0,
        page: 0,
        pages: 1,
        started_at: run_started,
        resumed: false,
    };
    progress(&snapshot);

    let mut seen: HashSet<u32> = HashSet::new();
    let mut newest: Option<i64> = None;
    let mut start = 0u32;
    loop {
        let page = source.page(
            PageRequest {
                start,
                count: page_size,
                sort: SORT_RELEASE_DESC,
            },
            cancel,
        )?;
        report.pages += 1;
        let records: Vec<GameRecord> = page
            .items
            .iter()
            .filter_map(GameRecord::from_item)
            .collect();
        {
            let tx = db.conn_mut().transaction()?;
            upsert_games(&tx, &records, unix_now())?;
            tx.commit()?;
        }
        for r in &records {
            seen.insert(r.appid);
            newest = newest.max(r.release_date);
        }
        let oldest_on_page = records.iter().filter_map(|r| r.release_date).min();

        snapshot.fetched = seen.len() as u32;
        snapshot.page = report.pages;
        snapshot.pages = report.pages + 1;
        progress(&snapshot);

        let exhausted = page.returned < page_size || start + page.returned >= page.total;
        let caught_up = oldest_on_page.is_none_or(|oldest| oldest < since);
        if exhausted || caught_up {
            break;
        }
        if report.pages >= opts.max_pages {
            report.partial = true;
            break;
        }
        start = (start + page.returned)
            .saturating_sub(overlap)
            .max(start + 1);
        sleep_cancellable(opts.delay, cancel)?;
    }

    let conn = db.conn();
    report.fetched = seen.len() as u32;
    report.inserted = conn.query_row(
        "SELECT COUNT(*) FROM games WHERE first_seen_at >= ?1",
        [run_started],
        |r| r.get(0),
    )?;
    report.updated = report.fetched.saturating_sub(report.inserted);

    // Only move the watermark when this run connected to the previous one: a partial run (or a
    // shorter `days_override` window) leaves a gap behind it that a later run must still cover.
    let connected = !report.partial && watermark.is_none_or(|mark| since <= mark);
    report.watermark = if connected {
        newest.max(watermark)
    } else {
        watermark
    };
    if let Some(mark) = report.watermark.filter(|_| connected) {
        set_meta(conn, meta_keys::RELEASE_WATERMARK, mark)?;
    }
    set_meta(conn, meta_keys::LAST_NEW_RELEASES_AT, unix_now())?;
    refresh_tag_counts(conn)?;

    let (requests, retries) = source.stats();
    report.requests = requests;
    report.retries = retries;
    report.duration_ms = clock.elapsed().as_millis() as u64;
    Ok(report)
}
