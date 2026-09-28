//! Full sync against a fake catalog source.

mod common;

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use common::{FakeSource, item};
use gamelib_core::db::Db;
use gamelib_core::db::links::save_link;
use gamelib_core::db::read::{get_game, query_games, status};
use gamelib_core::db::write::{get_meta_i64, meta_keys};
use gamelib_core::links::SiteRegistry;
use gamelib_core::model::{GameQuery, LinkInput, LinkKind, SyncPhase, SyncProgress};
use gamelib_core::sync::{SyncOptions, run_sync};
use gamelib_core::{Error, unix_now};

fn opts() -> SyncOptions {
    SyncOptions {
        page_size: 100,
        overlap: 5,
        delay: Duration::ZERO,
        ..Default::default()
    }
}

fn count(db: &Db) -> u32 {
    db.conn()
        .query_row("SELECT COUNT(*) FROM games WHERE delisted = 0", [], |r| {
            r.get(0)
        })
        .unwrap()
}

#[test]
fn downloads_everything_and_reports_progress() {
    let mut db = Db::open_in_memory().unwrap();
    let source = FakeSource::catalog(950, unix_now());
    let mut events: Vec<SyncProgress> = Vec::new();
    let report = run_sync(
        &mut db,
        &source,
        &opts(),
        &AtomicBool::new(false),
        &mut |p| events.push(p.clone()),
    )
    .unwrap();

    assert_eq!(count(&db), 950);
    assert_eq!(report.total, 950);
    assert_eq!(report.seen, 950);
    assert_eq!(report.inserted, 950);
    assert!(!report.prune_skipped);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // 1 featured + ceil((950 - 5) / 95) catalog pages.
    assert_eq!(report.requests, 1 + 10);

    assert_eq!(events.first().unwrap().phase, SyncPhase::Starting);
    assert!(events.iter().any(|e| e.phase == SyncPhase::Featured));
    let last_catalog = events
        .iter()
        .rev()
        .find(|e| e.phase == SyncPhase::Catalog)
        .unwrap();
    assert_eq!((last_catalog.fetched, last_catalog.total), (950, 950));
    assert_eq!(last_catalog.page, last_catalog.pages);
    assert_eq!(events.last().unwrap().phase, SyncPhase::Finalizing);

    let conn = db.conn();
    assert!(
        get_meta_i64(conn, meta_keys::LAST_SYNC_AT)
            .unwrap()
            .is_some()
    );
    assert!(
        get_meta_i64(conn, meta_keys::RELEASE_WATERMARK)
            .unwrap()
            .is_some()
    );
    assert!(
        get_meta_i64(conn, meta_keys::SYNC_CURSOR)
            .unwrap()
            .is_none()
    );
    assert!(!status(&mut db).unwrap().resumable);
}

#[test]
fn overlap_survives_a_game_removed_mid_run() {
    let mut db = Db::open_in_memory().unwrap();
    let source = FakeSource::catalog(500, unix_now());
    // Before the third catalog page, remove a game that was already fetched: every later game
    // shifts one position towards the start.
    *source.hook.borrow_mut() = Some(Box::new(|n, games| {
        if n == 2 {
            games.retain(|g| g.appid != Some(30));
        }
    }));
    let no_featured = SyncOptions {
        featured: false,
        ..opts()
    };
    let report = run_sync(
        &mut db,
        &source,
        &no_featured,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(report.warnings, Vec::<String>::new());
    assert_eq!(
        count(&db),
        500,
        "nothing after the removed game was skipped"
    );

    // It was stored earlier in the same run, so only the next full run notices it is gone.
    std::thread::sleep(Duration::from_millis(1100));
    let report = run_sync(
        &mut db,
        &source,
        &SyncOptions {
            fresh: true,
            ..no_featured
        },
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(report.delisted, 1);
    assert_eq!(count(&db), 499);
    assert!(get_game(db.conn(), 30).unwrap().unwrap().delisted);
}

#[test]
fn partial_runs_do_not_prune() {
    let mut db = Db::open_in_memory().unwrap();
    let source = FakeSource::catalog(300, unix_now());
    run_sync(
        &mut db,
        &source,
        &opts(),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();

    // Limited run: only 2 pages, must not hide the rest.
    let limited = SyncOptions {
        max_pages: Some(2),
        featured: false,
        fresh: true,
        ..opts()
    };
    let report = run_sync(
        &mut db,
        &source,
        &limited,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    assert!(report.prune_skipped);
    assert_eq!(report.delisted, 0);
    assert_eq!(count(&db), 300);
}

#[test]
fn failure_keeps_cursor_and_resume_continues() {
    let mut db = Db::open_in_memory().unwrap();
    let source = FakeSource::catalog(600, unix_now());
    source.fail_at.set(Some(3)); // featured, page 1, page 2, then fail
    let err = run_sync(
        &mut db,
        &source,
        &opts(),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap_err();
    assert!(matches!(err, Error::Network(_)));
    // Two catalog pages (app id order, 5 overlapping) plus best sellers from anywhere.
    let stored = count(&db);
    assert!((195..295).contains(&stored), "{stored}");
    for appid in (10..=1950).step_by(10) {
        assert!(
            get_game(db.conn(), appid).unwrap().is_some(),
            "{appid} from the first two pages"
        );
    }
    assert!(status(&mut db).unwrap().resumable);
    let cursor = get_meta_i64(db.conn(), meta_keys::SYNC_CURSOR)
        .unwrap()
        .unwrap();
    assert_eq!(cursor, 190);

    source.fail_at.set(None);
    let before = source.requests.get();
    let report = run_sync(
        &mut db,
        &source,
        &opts(),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    assert!(report.resumed);
    assert_eq!(
        source.requests.get() - before,
        5,
        "no featured page and no restart from zero"
    );
    assert_eq!(count(&db), 600);
    assert!(
        !report.prune_skipped,
        "earlier pages of the same run count as seen"
    );
    assert_eq!(report.delisted, 0);
    assert!(!status(&mut db).unwrap().resumable);
}

#[test]
fn cancel_is_reported_and_resumable() {
    let mut db = Db::open_in_memory().unwrap();
    let source = FakeSource::catalog(400, unix_now());
    source.cancel_at.set(Some(2));
    let cancel = AtomicBool::new(false);
    let err = run_sync(&mut db, &source, &opts(), &cancel, &mut |_| {}).unwrap_err();
    assert!(matches!(err, Error::Cancelled));
    assert!(status(&mut db).unwrap().resumable);
}

#[test]
fn second_run_is_idempotent_and_keeps_links() {
    let mut db = Db::open_in_memory().unwrap();
    let source = FakeSource::catalog(250, unix_now());
    run_sync(
        &mut db,
        &source,
        &opts(),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    let sites = SiteRegistry::with_builtin_sites();
    let input = LinkInput {
        id: None,
        appid: 20,
        url: "https://example.com/game.zip".into(),
        label: Some("Windows".into()),
        kind: LinkKind::Download,
        platform: None,
        version: None,
        notes: None,
    };
    save_link(db.conn(), &sites, &input, unix_now()).unwrap();

    // The game with the link disappears from the store, and a new one arrives.
    source.games.borrow_mut().retain(|g| g.appid != Some(20));
    source
        .games
        .borrow_mut()
        .push(item(99_999, "Newcomer", unix_now(), 1, &[19]));
    std::thread::sleep(Duration::from_millis(1100)); // new run timestamp
    let report = run_sync(
        &mut db,
        &source,
        &SyncOptions {
            fresh: true,
            ..opts()
        },
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(report.inserted, 1);
    assert_eq!(report.delisted, 1);
    assert_eq!(count(&db), 250);

    let links: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM game_links WHERE appid = 20",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(links, 1, "user links survive syncs");
    assert!(get_game(db.conn(), 20).unwrap().unwrap().delisted);
    let linked = GameQuery {
        has_links: true,
        ..Default::default()
    };
    assert_eq!(
        query_games(&mut db, &linked, unix_now()).unwrap().total,
        0,
        "delisted games stay out of lists"
    );
}
