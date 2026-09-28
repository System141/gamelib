//! On-demand new-release checks against a fake catalog source.

mod common;

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use common::{FakeSource, item};
use gamelib_core::db::Db;
use gamelib_core::db::write::{get_meta_i64, meta_keys, set_meta};
use gamelib_core::model::{NewReleasesReport, WorkerKind};
use gamelib_core::new_releases::{NewReleasesOptions, fetch_new_releases};
use gamelib_core::unix_now;

const HOUR: i64 = 3600;

/// `n` games, one released every 6 hours going back from `now`.
fn releases(n: u32, now: i64) -> FakeSource {
    FakeSource::new(
        (0..n)
            .map(|k| {
                item(
                    100_000 + k * 10,
                    &format!("Release {k}"),
                    now - i64::from(k) * 6 * HOUR,
                    5,
                    &[19],
                )
            })
            .collect(),
    )
}

fn opts() -> NewReleasesOptions {
    NewReleasesOptions {
        page_size: 100,
        overlap: 5,
        delay: Duration::ZERO,
        ..Default::default()
    }
}

fn run(db: &mut Db, source: &FakeSource, opts: &NewReleasesOptions) -> NewReleasesReport {
    let mut kinds = Vec::new();
    let report = fetch_new_releases(db, source, opts, &AtomicBool::new(false), &mut |p| {
        kinds.push(p.kind)
    })
    .unwrap();
    assert!(kinds.iter().all(|k| *k == WorkerKind::NewReleases));
    report
}

#[test]
fn empty_catalog_gets_the_initial_window() {
    let now = unix_now();
    let mut db = Db::open_in_memory().unwrap();
    let source = releases(3000, now);
    let report = run(&mut db, &source, &opts());
    // 30 days ≈ 120 releases: the first page reaches ~25 days back, the second passes 30 days.
    assert_eq!(report.pages, 2);
    assert_eq!(report.fetched, 195);
    assert_eq!(report.inserted, 195);
    assert!(!report.partial);
    assert_eq!(report.watermark, Some(now));
    assert_eq!(
        get_meta_i64(db.conn(), meta_keys::RELEASE_WATERMARK).unwrap(),
        Some(now)
    );
    assert!(
        get_meta_i64(db.conn(), meta_keys::LAST_NEW_RELEASES_AT)
            .unwrap()
            .is_some()
    );
}

#[test]
fn next_check_needs_one_page_and_counts_new_games() {
    let now = unix_now();
    let mut db = Db::open_in_memory().unwrap();
    let source = releases(3000, now - 2 * HOUR);
    run(&mut db, &source, &opts());

    std::thread::sleep(Duration::from_millis(1100));
    for k in 0..3 {
        source.games.borrow_mut().push(item(
            900_000 + k,
            &format!("Fresh {k}"),
            now - 60 + i64::from(k),
            1,
            &[21],
        ));
    }
    let before = source.requests.get();
    let report = run(&mut db, &source, &opts());
    assert_eq!(source.requests.get() - before, 1);
    assert_eq!(report.inserted, 3);
    assert_eq!(report.updated, 97);
    assert_eq!(report.watermark, Some(now - 58));
}

#[test]
fn far_behind_stops_at_the_page_limit_without_moving_the_watermark() {
    let now = unix_now();
    let mut db = Db::open_in_memory().unwrap();
    let old_mark = now - 400 * 86_400;
    set_meta(db.conn(), meta_keys::RELEASE_WATERMARK, old_mark).unwrap();
    let source = releases(3000, now);
    let report = run(
        &mut db,
        &source,
        &NewReleasesOptions {
            max_pages: 3,
            ..opts()
        },
    );
    assert!(report.partial);
    assert_eq!(report.pages, 3);
    assert_eq!(report.watermark, Some(old_mark));
    assert_eq!(
        get_meta_i64(db.conn(), meta_keys::RELEASE_WATERMARK).unwrap(),
        Some(old_mark)
    );
}

#[test]
fn a_shorter_window_does_not_skip_the_gap() {
    let now = unix_now();
    let mut db = Db::open_in_memory().unwrap();
    let old_mark = now - 100 * 86_400;
    set_meta(db.conn(), meta_keys::RELEASE_WATERMARK, old_mark).unwrap();
    let source = releases(3000, now);
    let report = run(
        &mut db,
        &source,
        &NewReleasesOptions {
            days_override: Some(7),
            ..opts()
        },
    );
    assert_eq!(report.pages, 1);
    assert!(!report.partial);
    assert_eq!(
        get_meta_i64(db.conn(), meta_keys::RELEASE_WATERMARK).unwrap(),
        Some(old_mark)
    );
}

#[test]
fn stops_when_the_list_runs_out() {
    let now = unix_now();
    let mut db = Db::open_in_memory().unwrap();
    let source = releases(40, now);
    let report = run(
        &mut db,
        &source,
        &NewReleasesOptions {
            initial_days: 365,
            ..opts()
        },
    );
    assert_eq!(report.pages, 1);
    assert_eq!(report.fetched, 40);
}
