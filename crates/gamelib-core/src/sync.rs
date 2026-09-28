//! Full catalog download.
//!
//! Pages through every released game ordered by app id, 1000 at a time, upserting each page in
//! its own transaction together with a resume cursor. Consecutive pages overlap a little so a game
//! removed from the store mid-run cannot shift another one out of view.

use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use crate::db::Db;
use crate::db::write::{
    delete_meta, finalize, get_meta_i64, mark_delisted, meta_keys, newest_release,
    refresh_tag_counts, set_meta, upsert_games, upsert_tags,
};
use crate::model::{SyncPhase, SyncProgress, SyncReport, WorkerKind};
use crate::record::GameRecord;
use crate::steam::types::StoreItem;
use crate::steam::{CatalogSource, MAX_PAGE_SIZE, PageRequest, SORT_APPID, SORT_TOP_SELLERS};
use crate::{Result, sleep_cancellable, unix_now};

/// An interrupted run older than this starts over instead of resuming.
const RESUME_MAX_AGE_SECS: i64 = 3 * 86_400;
/// Share of the reported total a run must see before unseen games are marked delisted.
const PRUNE_MIN_COVERAGE: f64 = 0.98;
/// Tag counts are refreshed every this many pages so filters stay useful during the first sync.
const TAG_COUNT_EVERY: u32 = 20;

#[derive(Debug, Clone)]
pub struct SyncOptions {
    pub page_size: u32,
    pub overlap: u32,
    /// Pause between pages, to stay polite with Steam.
    pub delay: Duration,
    /// Stop after this many catalog pages (testing/CLI).
    pub max_pages: Option<u32>,
    /// Fetch a page of top sellers first so the grid fills with well-known games quickly.
    pub featured: bool,
    pub prune: bool,
    /// Ignore any saved cursor.
    pub fresh: bool,
}

impl Default for SyncOptions {
    fn default() -> Self {
        Self {
            page_size: MAX_PAGE_SIZE,
            overlap: 20,
            delay: Duration::from_millis(600),
            max_pages: None,
            featured: true,
            prune: true,
            fresh: false,
        }
    }
}

pub fn run_sync(
    db: &mut Db,
    source: &dyn CatalogSource,
    opts: &SyncOptions,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&SyncProgress),
) -> Result<SyncReport> {
    let clock = Instant::now();
    let now = unix_now();
    let page_size = opts.page_size.clamp(1, MAX_PAGE_SIZE);
    let overlap = opts.overlap.min(page_size / 2);
    let mut report = SyncReport::default();

    // Resume an interrupted run if it is recent enough.
    let (mut start, run_started, resumed) = {
        let conn = db.conn();
        let cursor = get_meta_i64(conn, meta_keys::SYNC_CURSOR)?;
        let started = get_meta_i64(conn, meta_keys::SYNC_RUN_STARTED)?;
        match (cursor, started) {
            (Some(cursor), Some(started))
                if !opts.fresh && now - started < RESUME_MAX_AGE_SECS && cursor >= 0 =>
            {
                (cursor as u32, started, true)
            }
            _ => {
                set_meta(conn, meta_keys::SYNC_RUN_STARTED, now)?;
                delete_meta(conn, meta_keys::SYNC_CURSOR)?;
                (0, now, false)
            }
        }
    };
    report.resumed = resumed;

    let mut snapshot = SyncProgress {
        kind: WorkerKind::Full,
        phase: SyncPhase::Starting,
        fetched: start,
        total: 0,
        page: 0,
        pages: 0,
        started_at: run_started,
        resumed,
    };
    progress(&snapshot);

    snapshot.phase = SyncPhase::Tags;
    progress(&snapshot);
    let tags = source.tags(cancel)?;
    upsert_tags(db.conn(), &tags)?;

    if opts.featured && !resumed {
        snapshot.phase = SyncPhase::Featured;
        progress(&snapshot);
        let page = source.page(
            PageRequest {
                start: 0,
                count: 500.min(page_size),
                sort: SORT_TOP_SELLERS,
            },
            cancel,
        )?;
        report.skipped += page.skipped;
        store_page(db, &page.items, &mut report)?;
        refresh_tag_counts(db.conn())?;
        snapshot.total = page.total;
    }

    snapshot.phase = SyncPhase::Catalog;
    let mut pages_done = 0u32;
    let mut limited = false;
    let mut previous_tail: Vec<u32> = Vec::new();
    loop {
        let page = source.page(
            PageRequest {
                start,
                count: page_size,
                sort: SORT_APPID,
            },
            cancel,
        )?;
        report.total = page.total;
        report.skipped += page.skipped;
        if page.returned == 0 {
            break;
        }

        // The first items of this page should repeat the tail of the previous one.
        if !previous_tail.is_empty() && overlap > 0 {
            let head: HashSet<u32> = page
                .items
                .iter()
                .take(overlap as usize * 2)
                .filter_map(|i| i.appid.or(i.id))
                .collect();
            if !previous_tail.iter().any(|id| head.contains(id)) {
                report.warnings.push(format!(
                    "page at {start} does not overlap the previous page; some games may be missing"
                ));
            }
        }
        previous_tail = page
            .items
            .iter()
            .rev()
            .take(overlap as usize)
            .filter_map(|i| i.appid.or(i.id))
            .collect();

        let reached_end = start + page.returned >= page.total;
        let next = (start + page.returned)
            .saturating_sub(overlap)
            .max(start + 1);
        {
            let records = to_records(&page.items, &mut report);
            let tx = db.conn_mut().transaction()?;
            upsert_games(&tx, &records, unix_now())?;
            set_meta(&tx, meta_keys::SYNC_CURSOR, next)?;
            tx.commit()?;
        }
        pages_done += 1;

        snapshot.total = page.total;
        snapshot.fetched = (start + page.returned).min(page.total);
        snapshot.page = pages_done;
        snapshot.pages = estimate_pages(page.total, start, page_size, overlap, pages_done);
        progress(&snapshot);

        if reached_end {
            break;
        }
        if opts.max_pages.is_some_and(|max| pages_done >= max) {
            limited = true;
            break;
        }
        if pages_done.is_multiple_of(TAG_COUNT_EVERY) {
            refresh_tag_counts(db.conn())?;
        }
        start = next;
        sleep_cancellable(opts.delay, cancel)?;
    }

    snapshot.phase = SyncPhase::Finalizing;
    progress(&snapshot);
    let conn = db.conn();
    report.seen = conn.query_row(
        "SELECT COUNT(*) FROM games WHERE synced_at >= ?1",
        [run_started],
        |r| r.get(0),
    )?;
    report.inserted = conn.query_row(
        "SELECT COUNT(*) FROM games WHERE first_seen_at >= ?1",
        [run_started],
        |r| r.get(0),
    )?;
    let coverage_ok =
        report.total > 0 && f64::from(report.seen) >= PRUNE_MIN_COVERAGE * f64::from(report.total);
    if opts.prune && !limited && coverage_ok {
        report.delisted = mark_delisted(conn, run_started)?;
    } else {
        report.prune_skipped = true;
    }
    refresh_tag_counts(conn)?;
    if !limited {
        let finished = unix_now();
        set_meta(conn, meta_keys::LAST_SYNC_AT, finished)?;
        if let Some(newest) = newest_release(conn)? {
            let current = get_meta_i64(conn, meta_keys::RELEASE_WATERMARK)?.unwrap_or(0);
            set_meta(conn, meta_keys::RELEASE_WATERMARK, newest.max(current))?;
        }
        delete_meta(conn, meta_keys::SYNC_CURSOR)?;
        delete_meta(conn, meta_keys::SYNC_RUN_STARTED)?;
    }
    finalize(conn)?;

    let (requests, retries) = source.stats();
    report.requests = requests;
    report.retries = retries;
    report.duration_ms = clock.elapsed().as_millis() as u64;
    Ok(report)
}

/// Converts store items to rows, counting the ones Steam could not resolve as skipped.
fn to_records(items: &[StoreItem], report: &mut SyncReport) -> Vec<GameRecord> {
    let records: Vec<GameRecord> = items.iter().filter_map(GameRecord::from_item).collect();
    report.skipped += (items.len() - records.len()) as u32;
    records
}

fn store_page(db: &mut Db, items: &[StoreItem], report: &mut SyncReport) -> Result<()> {
    let records = to_records(items, report);
    let tx = db.conn_mut().transaction()?;
    upsert_games(&tx, &records, unix_now())?;
    tx.commit()?;
    Ok(())
}

fn estimate_pages(total: u32, start: u32, page_size: u32, overlap: u32, done: u32) -> u32 {
    let step = (page_size - overlap).max(1);
    let remaining = total.saturating_sub(start + page_size);
    done + remaining.div_ceil(step)
}
