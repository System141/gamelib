//! The download queue: one transfer at a time on its own thread, independent of catalog jobs,
//! so a catalog sync can run while a game downloads.
//!
//! The queue lives in the database (`downloads`, `download_files`); a download interrupted by
//! closing the app is queued again on the next start and continues from its `.part` files.
//! Files land in `<library>/.gamelib/downloads/<id>/` until they are installed.
//!
//! Every state change (claiming the next download, pausing, resuming, removing, finishing)
//! happens under one lock, so a pause can never be lost to a download that is just starting.

pub mod fetch;
pub mod names;
pub mod sources;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde::Serialize;

use crate::app::EventSink;
use crate::db::Db;
use crate::db::downloads::{self as queue, FileRow, NewDownload};
use crate::http::backoff;
use crate::model::{Download, DownloadProgress, DownloadState};
use crate::secrets::SecretStore;
use crate::stores::StoreSyncOptions;
use crate::{Error, ErrorInfo, Result, check_cancel, sleep_cancellable, unix_now};
use fetch::{Fetched, Resolved, Validator};

/// Live progress of the running download, a few times a second.
pub const EVENT_PROGRESS: &str = "download:progress";
/// A download changed state (payload: the [`Download`]), or was removed (`{id, removed: true}`).
pub const EVENT_STATE: &str = "download:state";

/// Free space to keep on the disk beyond what a download needs.
const DISK_MARGIN: u64 = 256 << 20;
const EMIT_EVERY: Duration = Duration::from_millis(250);
const SAVE_EVERY: Duration = Duration::from_secs(2);
/// Failed attempts in a row (without new bytes) before a download fails.
const MAX_RETRIES: u32 = 5;
/// The subfolder of the library that holds downloads until they are installed.
const DOWNLOADS_SUBDIR: [&str; 2] = [".gamelib", "downloads"];

pub struct DownloadManager {
    shared: Arc<Shared>,
}

struct Shared {
    db_path: PathBuf,
    sink: Arc<dyn EventSink>,
    secrets: Arc<SecretStore>,
    opts: StoreSyncOptions,
    control: Mutex<Control>,
    wake: Condvar,
}

#[derive(Default)]
struct Control {
    started: bool,
    /// Set when the queue changed, so a waiting worker looks again.
    dirty: bool,
    active: Option<Active>,
    live: Option<DownloadProgress>,
}

struct Active {
    id: i64,
    cancel: Arc<AtomicBool>,
    stop: Option<Stop>,
}

/// Why the running transfer was asked to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    Pause,
    /// Paused, then resumed before it had stopped: queue it again.
    Requeue,
    Remove,
}

impl DownloadManager {
    pub fn new(
        db_path: PathBuf,
        sink: Arc<dyn EventSink>,
        secrets: Arc<SecretStore>,
        opts: StoreSyncOptions,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                db_path,
                sink,
                secrets,
                opts,
                control: Mutex::new(Control::default()),
                wake: Condvar::new(),
            }),
        }
    }

    /// Starts the worker thread (once). Downloads that were running when the app closed are
    /// queued again.
    pub fn start(&self, conn: &Connection) -> Result<()> {
        let mut control = lock(&self.shared.control);
        if control.started {
            return Ok(());
        }
        queue::requeue_interrupted(conn, unix_now())?;
        let shared = self.shared.clone();
        std::thread::Builder::new()
            .name("gamelib-downloads".into())
            .spawn(move || worker(&shared))
            .map_err(|e| Error::Other(format!("could not start the download thread: {e}")))?;
        control.started = true;
        control.dirty = true;
        self.shared.wake.notify_all();
        Ok(())
    }

    /// Queues a download under `library_dir`. The same variant is never queued twice: an
    /// unfinished one is resumed and returned instead.
    pub fn enqueue(
        &self,
        conn: &mut Connection,
        new: &NewDownload,
        library_dir: &Path,
    ) -> Result<Download> {
        if let Some(id) = queue::unfinished(conn, new.store, new.product_id, new.option_id)? {
            self.resume(conn, id)?;
            return queue::get(conn, id)?.ok_or(Error::NotFound);
        }
        let root = downloads_root(library_dir);
        let id = queue::insert(
            conn,
            new,
            |id| root.join(id.to_string()).display().to_string(),
            unix_now(),
        )?;
        self.poke();
        let download = queue::get(conn, id)?.ok_or(Error::NotFound)?;
        self.shared.emit(EVENT_STATE, &download);
        Ok(download)
    }

    pub fn list(&self, conn: &Connection) -> Result<Vec<Download>> {
        queue::list(conn)
    }

    /// Live progress of the running download, if any.
    pub fn live(&self) -> Option<DownloadProgress> {
        lock(&self.shared.control).live.clone()
    }

    /// Pauses a download: the running one stops (keeping its part files), a queued one is held
    /// back.
    pub fn pause(&self, conn: &Connection, id: i64) -> Result<()> {
        let mut control = lock(&self.shared.control);
        if let Some(active) = control.active.as_mut().filter(|a| a.id == id) {
            active.stop = Some(Stop::Pause);
            active.cancel.store(true, Ordering::Relaxed);
            return Ok(());
        }
        queue::get(conn, id)?.ok_or(Error::NotFound)?;
        let changed = queue::transition(
            conn,
            id,
            &[DownloadState::Queued],
            DownloadState::Paused,
            unix_now(),
        )?;
        drop(control);
        if changed {
            self.shared.emit_state(conn, id);
        }
        Ok(())
    }

    /// Queues a paused or failed download again.
    pub fn resume(&self, conn: &Connection, id: i64) -> Result<()> {
        let mut control = lock(&self.shared.control);
        if let Some(active) = control.active.as_mut().filter(|a| a.id == id) {
            // Still stopping after a pause: it goes back into the queue once it has stopped.
            if active.stop == Some(Stop::Pause) {
                active.stop = Some(Stop::Requeue);
            }
            return Ok(());
        }
        queue::get(conn, id)?.ok_or(Error::NotFound)?;
        let changed = queue::transition(
            conn,
            id,
            &[DownloadState::Paused, DownloadState::Failed],
            DownloadState::Queued,
            unix_now(),
        )?;
        if changed {
            control.dirty = true;
            self.shared.wake.notify_all();
        }
        drop(control);
        if changed {
            self.shared.emit_state(conn, id);
        }
        Ok(())
    }

    /// Stops a download if it runs, deletes its files and forgets it.
    pub fn remove(&self, conn: &Connection, id: i64) -> Result<()> {
        let mut control = lock(&self.shared.control);
        if let Some(active) = control.active.as_mut().filter(|a| a.id == id) {
            // The worker removes it once the transfer has stopped.
            active.stop = Some(Stop::Remove);
            active.cancel.store(true, Ordering::Relaxed);
            return Ok(());
        }
        let d = queue::get(conn, id)?.ok_or(Error::NotFound)?;
        queue::delete(conn, id)?;
        drop(control);
        remove_files(&d.dir);
        self.shared.emit_removed(id);
        Ok(())
    }

    /// Takes finished downloads off the list. Their files stay (for the installer).
    pub fn clear_completed(&self, conn: &Connection) -> Result<()> {
        for id in queue::clear_completed(conn)? {
            self.shared.emit_removed(id);
        }
        Ok(())
    }

    fn poke(&self) {
        lock(&self.shared.control).dirty = true;
        self.shared.wake.notify_all();
    }
}

impl Shared {
    fn emit(&self, event: &str, payload: &impl Serialize) {
        self.sink.emit(
            event,
            serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
        );
    }

    fn emit_state(&self, conn: &Connection, id: i64) {
        if let Ok(Some(d)) = queue::get(conn, id) {
            self.emit(EVENT_STATE, &d);
        }
    }

    fn emit_removed(&self, id: i64) {
        self.emit(
            EVENT_STATE,
            &serde_json::json!({ "id": id, "removed": true }),
        );
    }
}

/// `<library>/.gamelib/downloads`.
pub fn downloads_root(library_dir: &Path) -> PathBuf {
    DOWNLOADS_SUBDIR
        .iter()
        .fold(library_dir.to_path_buf(), |p, part| p.join(part))
}

/// Deletes a download folder, but only one that is inside a library's downloads folder.
fn remove_files(dir: &str) {
    let path = Path::new(dir);
    let inside = path
        .parent()
        .is_some_and(|p| p.ends_with(Path::new(DOWNLOADS_SUBDIR[0]).join(DOWNLOADS_SUBDIR[1])));
    if inside && !dir.is_empty() {
        let _ = std::fs::remove_dir_all(path);
    }
}

fn worker(shared: &Arc<Shared>) {
    let db = match Db::open(&shared.db_path) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("download worker: {e}");
            return;
        }
    };
    loop {
        match claim(shared, db.conn()) {
            Some((download, cancel)) => run_one(shared, db.conn(), download, &cancel),
            None => {
                let mut control = lock(&shared.control);
                if !control.dirty {
                    control = shared
                        .wake
                        .wait_timeout(control, Duration::from_secs(30))
                        .map(|(c, _)| c)
                        .unwrap_or_else(|p| p.into_inner().0);
                }
                control.dirty = false;
            }
        }
    }
}

/// Takes the oldest queued download and marks it as running.
fn claim(shared: &Shared, conn: &Connection) -> Option<(Download, Arc<AtomicBool>)> {
    let mut control = lock(&shared.control);
    let download = queue::next_queued(conn).ok().flatten()?;
    let claimed = queue::transition(
        conn,
        download.id,
        &[DownloadState::Queued],
        DownloadState::Downloading,
        unix_now(),
    );
    if !matches!(claimed, Ok(true)) {
        return None;
    }
    let cancel = Arc::new(AtomicBool::new(false));
    control.active = Some(Active {
        id: download.id,
        cancel: cancel.clone(),
        stop: None,
    });
    drop(control);
    shared.emit_state(conn, download.id);
    Some((download, cancel))
}

fn run_one(shared: &Shared, conn: &Connection, download: Download, cancel: &AtomicBool) {
    let id = download.id;
    let result = catch_unwind(AssertUnwindSafe(|| {
        transfer(shared, conn, &download, cancel)
    }))
    .unwrap_or_else(|_| Err(Error::Other("the download stopped unexpectedly".into())));

    let mut control = lock(&shared.control);
    control.live = None;
    let stop = control.active.take().and_then(|a| a.stop);
    let (state, error) = match (result, stop) {
        (_, Some(Stop::Remove)) => {
            let _ = queue::delete(conn, id);
            drop(control);
            remove_files(&download.dir);
            shared.emit_removed(id);
            return;
        }
        (Ok(()), _) => (DownloadState::Completed, None),
        (Err(_), Some(Stop::Requeue)) => {
            control.dirty = true;
            (DownloadState::Queued, None)
        }
        (Err(Error::Cancelled), _) => (DownloadState::Paused, None),
        (Err(e), _) => (DownloadState::Failed, Some(failure(e))),
    };
    let _ = queue::set_state(conn, id, state, error.as_ref(), unix_now());
    drop(control);
    shared.emit_state(conn, id);
}

/// What a failed download keeps of its error; signed addresses lose their signature.
fn failure(e: Error) -> ErrorInfo {
    match e {
        Error::Http { status, url } => ErrorInfo::from(Error::Http {
            status,
            url: fetch::redact(&url),
        }),
        other => ErrorInfo::from(other),
    }
}

/// Measures speed and reports progress without flooding the UI or the disk.
struct Tracker<'a> {
    shared: &'a Shared,
    conn: &'a Connection,
    id: i64,
    total: u64,
    done: u64,
    stage: &'static str,
    last_emit: Option<Instant>,
    last_save: Instant,
    sample: (Instant, u64),
    speed: f64,
}

impl<'a> Tracker<'a> {
    fn new(shared: &'a Shared, conn: &'a Connection, id: i64, total: u64, done: u64) -> Self {
        let now = Instant::now();
        Self {
            shared,
            conn,
            id,
            total,
            done,
            stage: "downloading",
            last_emit: None,
            last_save: now,
            sample: (now, done),
            speed: 0.0,
        }
    }

    fn bytes(&mut self, done: u64) {
        self.done = done;
        let now = Instant::now();
        let elapsed = now.duration_since(self.sample.0).as_secs_f64();
        if elapsed >= 0.5 {
            let rate = done.saturating_sub(self.sample.1) as f64 / elapsed;
            self.speed = if self.speed == 0.0 {
                rate
            } else {
                self.speed * 0.7 + rate * 0.3
            };
            self.sample = (now, done);
        }
        if now.duration_since(self.last_save) >= SAVE_EVERY {
            self.save();
            self.last_save = now;
        }
        if self
            .last_emit
            .is_none_or(|t| now.duration_since(t) >= EMIT_EVERY)
        {
            self.emit();
            self.last_emit = Some(now);
        }
    }

    fn stage(&mut self, stage: &'static str) {
        self.stage = stage;
        self.emit();
    }

    fn save(&self) {
        let _ = queue::set_progress(self.conn, self.id, self.done, self.total);
    }

    fn emit(&self) {
        let speed = self.speed.max(0.0) as u64;
        let progress = DownloadProgress {
            id: self.id,
            done_bytes: self.done,
            total_bytes: self.total.max(self.done),
            speed,
            eta: (speed > 0 && self.total > self.done).then(|| (self.total - self.done) / speed),
            stage: self.stage.into(),
        };
        lock(&self.shared.control).live = Some(progress.clone());
        self.shared.emit(EVENT_PROGRESS, &progress);
    }
}

/// Downloads every file of `download` that is not done yet.
fn transfer(
    shared: &Shared,
    conn: &Connection,
    download: &Download,
    cancel: &AtomicBool,
) -> Result<()> {
    let dir = PathBuf::from(&download.dir);
    std::fs::create_dir_all(&dir).map_err(|e| Error::Other(format!("{}: {e}", dir.display())))?;
    let files = queue::files(conn, download.id)?;
    let owned_key = queue::owned_key(conn, download.store, &download.product_id)?;
    let finished: u64 = files.iter().filter(|f| f.done).filter_map(|f| f.size).sum();
    let remaining = download
        .total_bytes
        .saturating_sub(download.done_bytes.max(finished));
    if remaining > 0 {
        let free = fs4::available_space(&dir).unwrap_or(u64::MAX);
        if free < remaining.saturating_add(DISK_MARGIN) {
            return Err(Error::Invalid("disk_space"));
        }
    }
    let client = fetch::client()?;
    let ctx = FileCtx {
        shared,
        conn,
        client: &client,
        dir: &dir,
        owned_key: owned_key.as_deref(),
        cancel,
    };
    let mut tracker = Tracker::new(shared, conn, download.id, download.total_bytes, finished);
    let mut done_before = finished;
    let mut result = Ok(());
    for mut file in files.into_iter().filter(|f| !f.done) {
        match ctx.transfer(&mut file, &mut tracker, done_before) {
            Ok(size) => {
                done_before += size;
                tracker.bytes(done_before);
            }
            Err(e) => {
                result = Err(e);
                break;
            }
        }
    }
    tracker.save();
    result
}

/// What downloading one file needs.
struct FileCtx<'a> {
    shared: &'a Shared,
    conn: &'a Connection,
    client: &'a reqwest::blocking::Client,
    dir: &'a Path,
    owned_key: Option<&'a str>,
    cancel: &'a AtomicBool,
}

impl FileCtx<'_> {
    /// Downloads one file, re-resolving an expired address and retrying transient failures.
    /// Returns its size.
    fn transfer(&self, file: &mut FileRow, tracker: &mut Tracker, done_before: u64) -> Result<u64> {
        let mut resolved: Option<Resolved> = None;
        let mut failures = 0;
        let mut expired = 0;
        let mut bad_checksums = 0;
        loop {
            check_cancel(self.cancel)?;
            let r = match resolved.take() {
                Some(r) => r,
                None => match sources::resolve(
                    &file.source,
                    self.owned_key,
                    &self.shared.secrets,
                    &self.shared.opts,
                    self.cancel,
                ) {
                    Ok(r) => r,
                    Err(e) if e.is_transient() && failures < MAX_RETRIES => {
                        failures += 1;
                        sleep_cancellable(backoff(failures), self.cancel)?;
                        continue;
                    }
                    Err(e) => return Err(e),
                },
            };
            if file.file_name.is_none() {
                let fallback = format!("file-{}", file.position + 1);
                let name = r
                    .name
                    .clone()
                    .or_else(|| url_file_name(&r.url))
                    .unwrap_or_else(|| fallback.clone());
                file.file_name = Some(names::safe_name(&name, &fallback));
            }
            if file.md5.is_none() {
                file.md5.clone_from(&r.md5);
            }
            queue::update_file(self.conn, file)?;
            let name = file.file_name.clone().unwrap_or_default();
            let target = self.dir.join(&name);
            let part = self.dir.join(format!("{name}.part"));
            if let Ok(meta) = target.metadata()
                && file.size.is_none_or(|s| s == meta.len())
            {
                // Finished before the database heard of it (the app closed in between).
                file.done = true;
                file.size = Some(meta.len());
                queue::update_file(self.conn, file)?;
                return Ok(meta.len());
            }

            let mut validator = Validator {
                etag: file.etag.clone(),
                last_modified: file.last_modified.clone(),
            };
            let before = part_len(&part);
            let outcome = fetch::fetch(
                self.client,
                &r,
                &part,
                file.size,
                &mut validator,
                self.cancel,
                &mut |have| tracker.bytes(done_before + have),
            );
            file.etag = validator.etag;
            file.last_modified = validator.last_modified;
            queue::update_file(self.conn, file)?;
            match outcome {
                Ok(Fetched::Done { size }) => {
                    if let Some(expected) = file.md5.clone() {
                        tracker.stage("verifying");
                        let actual = fetch::md5_file(&part, self.cancel)?;
                        tracker.stage("downloading");
                        if actual != expected {
                            let _ = std::fs::remove_file(&part);
                            bad_checksums += 1;
                            if bad_checksums > 1 {
                                return Err(Error::Invalid("checksum"));
                            }
                            // Once more from scratch, from a fresh address.
                            continue;
                        }
                    }
                    fetch::finish_part(&part, &target)?;
                    file.size = Some(size);
                    file.done = true;
                    queue::update_file(self.conn, file)?;
                    return Ok(size);
                }
                Ok(Fetched::Expired) => {
                    expired += 1;
                    if expired > 3 {
                        return Err(Error::Invalid("link_expired"));
                    }
                }
                Err(e) if e.is_transient() => {
                    if part_len(&part) > before {
                        // The connection dropped after new bytes arrived: resume right away.
                        failures = 0;
                    } else {
                        failures += 1;
                        if failures > MAX_RETRIES {
                            return Err(e);
                        }
                        sleep_cancellable(backoff(failures), self.cancel)?;
                    }
                    resolved = Some(r);
                }
                Err(e) => return Err(e),
            }
        }
    }
}

fn part_len(part: &Path) -> u64 {
    part.metadata().map(|m| m.len()).unwrap_or(0)
}

fn url_file_name(url: &str) -> Option<String> {
    let url = reqwest::Url::parse(url).ok()?;
    let last = url.path_segments()?.rfind(|s| !s.is_empty())?;
    let name = crate::text::percent_decode(last);
    name.contains('.').then_some(name)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_folders() {
        let root = downloads_root(Path::new("/games"));
        assert_eq!(root, Path::new("/games/.gamelib/downloads"));
        assert_eq!(
            url_file_name("https://cdn.gog.com/secure/offline/setup_game_1.0%20(x).exe?token=1")
                .as_deref(),
            Some("setup_game_1.0 (x).exe")
        );
        assert_eq!(
            url_file_name("https://cdn.example/download/12345?x=1"),
            None
        );
    }

    #[test]
    fn only_download_folders_are_removed() {
        let base = std::env::temp_dir().join(format!("gamelib-rm-{}", std::process::id()));
        let outside = base.join("important");
        std::fs::create_dir_all(&outside).unwrap();
        remove_files(&outside.display().to_string());
        assert!(outside.exists(), "not inside .gamelib/downloads");
        let inside = downloads_root(&base).join("7");
        std::fs::create_dir_all(&inside).unwrap();
        remove_files(&inside.display().to_string());
        assert!(!inside.exists());
        std::fs::remove_dir_all(&base).unwrap();
    }
}
