//! The command layer: background jobs, their events and the job slot.

mod common;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use common::FakeSource;
use gamelib_core::app::{App, EventSink, JobOptions, steam_url};
use gamelib_core::model::{GameQuery, LinkInput, LinkKind, OpenTarget, WorkerKind};
use gamelib_core::new_releases::NewReleasesOptions;
use gamelib_core::steam::types::RawTag;
use gamelib_core::steam::{CatalogSource, PageRequest, QueryPage};
use gamelib_core::sync::SyncOptions;
use gamelib_core::{Error, Result, unix_now};
use serde_json::Value;

/// A unique directory under the system temp dir, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "gamelib-app-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn db(&self) -> PathBuf {
        self.0.join("gamelib.db")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Records every event and lets a test wait for the next `sync:finished`.
#[derive(Default)]
struct Events {
    log: Mutex<Vec<(String, Value)>>,
    changed: Condvar,
}

impl EventSink for Events {
    fn emit(&self, event: &str, payload: Value) {
        self.log.lock().unwrap().push((event.to_owned(), payload));
        self.changed.notify_all();
    }
}

impl Events {
    /// Waits for the `n`-th (1-based) `sync:finished` event.
    fn finished(&self, n: usize) -> Value {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut log = self.log.lock().unwrap();
        loop {
            let finished: Vec<&Value> = log
                .iter()
                .filter(|(e, _)| e == "sync:finished")
                .map(|(_, p)| p)
                .collect();
            if let Some(payload) = finished.get(n - 1) {
                return (*payload).clone();
            }
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "no sync:finished event #{n}");
            log = self.changed.wait_timeout(log, left).unwrap().0;
        }
    }

    fn progress_count(&self) -> usize {
        let log = self.log.lock().unwrap();
        log.iter().filter(|(e, _)| e == "sync:progress").count()
    }
}

/// Holds page requests until opened; a cancelled job gets `Cancelled` instead of waiting.
#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    fn open(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }

    fn pass(&self, cancel: &AtomicBool) -> Result<()> {
        let mut open = self.open.lock().unwrap();
        while !*open {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            open = self
                .changed
                .wait_timeout(open, Duration::from_millis(20))
                .unwrap()
                .0;
        }
        Ok(())
    }
}

struct GatedSource {
    inner: FakeSource,
    gate: Arc<Gate>,
}

impl CatalogSource for GatedSource {
    fn tags(&self, cancel: &AtomicBool) -> Result<Vec<RawTag>> {
        self.inner.tags(cancel)
    }

    fn page(&self, req: PageRequest, cancel: &AtomicBool) -> Result<QueryPage> {
        self.gate.pass(cancel)?;
        self.inner.page(req, cancel)
    }
}

struct PanickingSource;

impl CatalogSource for PanickingSource {
    fn tags(&self, _cancel: &AtomicBool) -> Result<Vec<RawTag>> {
        Ok(Vec::new())
    }

    fn page(&self, _req: PageRequest, _cancel: &AtomicBool) -> Result<QueryPage> {
        panic!("simulated bug in a page handler");
    }
}

fn options(
    source: impl Fn() -> Result<Box<dyn CatalogSource>> + Send + Sync + 'static,
) -> JobOptions {
    JobOptions {
        sync: SyncOptions {
            page_size: 100,
            overlap: 5,
            delay: Duration::ZERO,
            ..Default::default()
        },
        new_releases: NewReleasesOptions {
            page_size: 100,
            overlap: 5,
            delay: Duration::ZERO,
            ..Default::default()
        },
        source: Arc::new(source),
        ..Default::default()
    }
}

fn catalog(n: u32) -> Result<Box<dyn CatalogSource>> {
    Ok(Box::new(FakeSource::catalog(n, unix_now())))
}

#[test]
fn full_sync_runs_in_the_background() {
    let dir = TempDir::new("sync");
    let events = Arc::new(Events::default());
    let app = App::with_options(dir.db(), events.clone(), options(|| catalog(250))).unwrap();
    assert_eq!(app.status().unwrap().catalog.game_count, 0);

    app.start_sync(false).unwrap();
    let finished = events.finished(1);
    assert_eq!(finished["kind"], "full");
    assert_eq!(finished["outcome"], "completed");
    assert_eq!(finished["report"]["seen"], 250);
    assert_eq!(finished["error"], Value::Null);
    assert!(events.progress_count() > 1);

    let status = app.status().unwrap();
    assert_eq!(status.catalog.game_count, 250);
    assert!(status.catalog.last_sync_at.is_some());
    assert_eq!(status.worker, None);
    assert!(status.progress.is_none());
    let page = app.query_games(&GameQuery::default()).unwrap();
    assert_eq!(page.total, 250);
}

#[test]
fn one_job_at_a_time_across_processes_and_cancel() {
    let dir = TempDir::new("busy");
    let gate = Arc::new(Gate::default());
    let source_gate = gate.clone();
    let events = Arc::new(Events::default());
    let app = App::with_options(
        dir.db(),
        events.clone(),
        options(move || {
            Ok(Box::new(GatedSource {
                inner: FakeSource::catalog(150, unix_now()),
                gate: source_gate.clone(),
            }))
        }),
    )
    .unwrap();

    app.start_sync(false).unwrap();
    assert_eq!(app.status().unwrap().worker, Some(WorkerKind::Full));
    assert!(matches!(app.fetch_new_releases(None), Err(Error::Busy)));
    assert!(matches!(app.start_sync(true), Err(Error::Busy)));

    // A second process on the same database (here: a second App) is refused too.
    let other = App::with_options(
        dir.db(),
        Arc::new(Events::default()),
        options(|| catalog(10)),
    )
    .unwrap();
    assert!(matches!(other.start_sync(false), Err(Error::Busy)));

    app.cancel_job();
    let finished = events.finished(1);
    assert_eq!(finished["outcome"], "cancelled");
    assert_eq!(finished["error"], Value::Null);
    assert_eq!(app.status().unwrap().worker, None);

    // The slot and the file lock are free again.
    gate.open();
    app.fetch_new_releases(Some(365)).unwrap();
    let finished = events.finished(2);
    assert_eq!(finished["kind"], "new_releases");
    assert_eq!(finished["outcome"], "completed");
    assert_eq!(finished["newReleases"]["fetched"], 150);
    other.start_sync(false).unwrap();
}

#[test]
fn a_panicking_job_fails_cleanly_and_frees_the_slot() {
    let dir = TempDir::new("panic");
    let calls = Arc::new(AtomicU32::new(0));
    let source_calls = calls.clone();
    let events = Arc::new(Events::default());
    let app = App::with_options(
        dir.db(),
        events.clone(),
        options(move || {
            if source_calls.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(Box::new(PanickingSource))
            } else {
                catalog(40)
            }
        }),
    )
    .unwrap();

    app.start_sync(false).unwrap();
    let finished = events.finished(1);
    assert_eq!(finished["outcome"], "failed");
    assert_eq!(finished["error"]["kind"], "other");
    let message = finished["error"]["message"].as_str().unwrap();
    assert!(message.contains("simulated bug"), "{message}");
    assert_eq!(app.status().unwrap().worker, None);

    app.start_sync(false).unwrap();
    assert_eq!(events.finished(2)["outcome"], "completed");
    assert_eq!(app.status().unwrap().catalog.game_count, 40);
}

#[test]
fn a_source_error_is_reported_as_failed() {
    let dir = TempDir::new("error");
    let events = Arc::new(Events::default());
    let app = App::with_options(
        dir.db(),
        events.clone(),
        options(|| Err(Error::Network("no route to Steam".into()))),
    )
    .unwrap();
    app.fetch_new_releases(None).unwrap();
    let finished = events.finished(1);
    assert_eq!(finished["kind"], "new_releases");
    assert_eq!(finished["outcome"], "failed");
    assert_eq!(finished["error"]["kind"], "network");
    assert_eq!(finished["newReleases"], Value::Null);
}

#[test]
fn links_and_urls() {
    let dir = TempDir::new("links");
    let events = Arc::new(Events::default());
    let app = App::with_options(dir.db(), events.clone(), options(|| catalog(10))).unwrap();
    // Links belong to catalog games.
    app.start_sync(false).unwrap();
    assert_eq!(events.finished(1)["outcome"], "completed");

    let link = app
        .save_link(&LinkInput {
            id: None,
            appid: 10,
            url: "https://example.com/files/game.zip?utm_source=x".into(),
            label: Some("Resmi site".into()),
            kind: LinkKind::Download,
            platform: None,
            version: None,
            notes: None,
        })
        .unwrap();
    assert_eq!(app.list_links(10).unwrap().len(), 1);
    assert_eq!(
        app.link_url(link.id).unwrap(),
        "https://example.com/files/game.zip"
    );
    assert!(app.delete_link(link.id).unwrap());
    assert!(matches!(app.link_url(link.id), Err(Error::NotFound)));
    assert!(matches!(app.check_link(link.id), Err(Error::NotFound)));
    assert!(!app.list_sites().is_empty());

    assert_eq!(
        steam_url(620, OpenTarget::Web),
        "https://store.steampowered.com/app/620/"
    );
    assert_eq!(steam_url(620, OpenTarget::Client), "steam://store/620");
}
