//! The torrent queue against a local BitTorrent peer.
//!
//! A session of the same engine seeds a file we just made a torrent of, on a random loopback
//! port and at a capped upload speed; GameLib is then told to download it with the swarm (DHT,
//! trackers, incoming connections) switched off. Nothing here touches the internet.

mod common;

use std::net::{Ipv4Addr, SocketAddr};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use common::http::{Request, Response, TestServer};
use gamelib_core::ErrorInfo;
use gamelib_core::app::{App, EventSink, JobOptions};
use gamelib_core::downloads::sources::this_platform;
use gamelib_core::downloads::{EVENT_PROGRESS, EVENT_STATE, torrent};
use gamelib_core::model::{DownloadSourceKind, DownloadState, InstallState, SettingsPatch, Store};
use librqbit::limits::LimitsConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, CreateTorrentOptions, ListenerMode, ListenerOptions,
    ManagedTorrent, Session, SessionOptions, create_torrent, spawn_utils::BlockingSpawner,
};
use serde_json::Value;

const APPID: u32 = 292_030;
const TITLE: &str = "Test Game";
/// The name of the only file inside the torrent.
const FILE: &str = "game.bin";
const SIZE: usize = 6 * 1024 * 1024;
const PIECE: u32 = 16 * 1024;

/// Deterministic contents, big enough that a pause lands while bytes are still moving.
fn data(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i as u32 * 31 + 7) as u8).collect()
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "gamelib-torrents-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Every event the queue emitted, with a way to wait for one.
#[derive(Default)]
struct Events {
    log: Mutex<Vec<(String, Value)>>,
}

impl EventSink for Events {
    fn emit(&self, event: &str, payload: Value) {
        lock(&self.log).push((event.to_owned(), payload));
    }
}

impl Events {
    fn wait(&self, what: &str, pred: impl Fn(&str, &Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let found = lock(&self.log)
                .iter()
                .find(|(e, p)| pred(e, p))
                .map(|(_, p)| p.clone());
            if let Some(payload) = found {
                return payload;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Waits until the queue reports real progress for a download.
    fn downloading(&self, id: i64) {
        self.wait("progress", |e, p| {
            e == EVENT_PROGRESS && p["id"] == id && p["doneBytes"].as_u64().unwrap_or(0) > 0
        });
    }

    fn state(&self, id: i64, state: &str) -> Value {
        self.wait(state, |e, p| {
            e == EVENT_STATE && p["id"] == id && p["state"] == state
        })
    }

    fn removed(&self, id: i64) {
        self.wait("removed", |e, p| {
            e == EVENT_STATE && p["id"] == id && p["removed"] == true
        });
    }
}

fn wait_until(mut ready: impl FnMut() -> bool, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The payload, its torrent, and a peer serving it.
struct Seed {
    _session: Arc<Session>,
    _handle: Arc<ManagedTorrent>,
    addr: SocketAddr,
    payload: Vec<u8>,
    torrent: Vec<u8>,
    /// The magnet link for the same torrent.
    magnet: String,
    /// The info hash, as the queue stores it for a magnet.
    info_hash: String,
}

/// Waits until the seeder has checked its own data and can serve every piece.
async fn wait_until_seeding(handle: &ManagedTorrent) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let stats = handle.stats();
        if stats.finished {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the seeder never finished: {stats}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Creates the payload and a torrent of it, then serves it from a loopback peer.
fn seed(dir: &Path, runtime: &tokio::runtime::Runtime) -> Seed {
    let payload = data(SIZE);
    let seed_dir = dir.join("seed");
    std::fs::create_dir_all(&seed_dir).unwrap();
    std::fs::write(seed_dir.join(FILE), &payload).unwrap();

    runtime.block_on(async {
        let torrent = create_torrent(
            &seed_dir.join(FILE),
            CreateTorrentOptions {
                piece_length: Some(PIECE),
                ..Default::default()
            },
            &BlockingSpawner::new(1),
        )
        .await
        .unwrap();
        let magnet = torrent.as_magnet().to_string();
        let info_hash = torrent.info_hash().as_string();
        let session = Session::new_with_opts(
            seed_dir.clone(),
            SessionOptions {
                dht: None,
                persistence: None,
                disable_local_service_discovery: true,
                listen: Some(ListenerOptions {
                    mode: ListenerMode::TcpOnly,
                    listen_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let handle = session
            .add_torrent(
                AddTorrent::from_bytes(torrent.as_bytes().unwrap()),
                Some(AddTorrentOptions {
                    overwrite: true,
                    output_folder: Some(seed_dir.display().to_string()),
                    // Slow enough that a pause lands while bytes are still moving.
                    ratelimits: LimitsConfig {
                        upload_bps: NonZeroU32::new(1024 * 1024),
                        download_bps: None,
                    },
                    ..Default::default()
                }),
            )
            .await
            .unwrap()
            .into_handle()
            .unwrap();
        wait_until_seeding(&handle).await;
        let addr = session.listen_addr().unwrap();
        assert!(addr.ip().is_loopback(), "the peer must stay local: {addr}");
        Seed {
            _session: session,
            _handle: handle,
            addr,
            payload,
            torrent: torrent.as_bytes().unwrap().to_vec(),
            magnet,
            info_hash,
        }
    })
}

/// An app whose library is `library` and whose torrents only talk to `peers`.
fn app(dir: &Path, library: &Path, events: Arc<Events>, peers: Vec<SocketAddr>) -> App {
    std::fs::create_dir_all(library).unwrap();
    let app = App::with_options(
        dir.join("gamelib.db"),
        events,
        JobOptions {
            torrent: torrent::Options {
                swarm: false,
                initial_peers: peers,
            },
            ..Default::default()
        },
    )
    .unwrap();
    app.update_settings(&SettingsPatch {
        library_dir: Some(library.display().to_string()),
        ..Default::default()
    })
    .unwrap();
    app
}

#[test]
fn a_torrent_downloads_pauses_resumes_and_is_removed() {
    let dir = TempDir::new("queue");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let seed = seed(&dir.0, &runtime);

    // The `.torrent` file itself is served over HTTP, like a site offering the download.
    let served = seed.torrent.clone();
    let server = TestServer::start(move |req: &Request| match req.path.as_str() {
        "/game.torrent" => Response::status(200)
            .with_header("Content-Type", "application/x-bittorrent")
            .with_body(served.clone()),
        _ => Response::status(404),
    });

    let events = Arc::new(Events::default());
    let app = app(
        &dir.0,
        &dir.0.join("Games"),
        events.clone(),
        vec![seed.addr],
    );
    app.start_downloads().unwrap();

    // A `.torrent` address is fetched and then downloaded like any other source.
    let url = format!("{}/game.torrent", server.base);
    let queued = app.enqueue_torrent(APPID, TITLE, &url).unwrap();
    assert_eq!(queued.source_kind, DownloadSourceKind::Torrent);
    assert_eq!(queued.store, Store::Web);
    assert_eq!(queued.appid, Some(APPID));
    assert_eq!(queued.title, TITLE);
    assert_eq!(queued.platform, Some(this_platform()));
    assert_eq!(queued.option_id, url);

    let done = events.state(queued.id, "completed");
    assert_eq!(done["sourceKind"], "torrent");
    assert_eq!(done["totalBytes"].as_u64(), Some(SIZE as u64));
    assert_eq!(done["doneBytes"].as_u64(), Some(SIZE as u64));
    assert_eq!(done["title"], TITLE);
    let out = PathBuf::from(done["dir"].as_str().unwrap());
    assert_eq!(std::fs::read(out.join(FILE)).unwrap(), seed.payload);
    // Torrent content is left in its folder: nothing is unpacked, installed or run.
    let stored = app
        .downloads()
        .unwrap()
        .items
        .into_iter()
        .find(|d| d.id == queued.id)
        .unwrap();
    assert_eq!(stored.install_state, Some(InstallState::Manual));
    assert_eq!(stored.install_kind.as_deref(), Some("torrent"));

    // The same payload as a magnet: the info hash names the torrent, so it is the queue id.
    let d = app.enqueue_torrent(APPID, TITLE, &seed.magnet).unwrap();
    assert_eq!(d.source_kind, DownloadSourceKind::Torrent);
    assert_eq!(d.option_id, seed.info_hash);
    assert_ne!(d.id, queued.id, "a second download of another kind");

    // Pause while the bytes are still moving.
    events.downloading(d.id);
    app.pause_download(d.id).unwrap();
    let paused = events.state(d.id, "paused");
    let transferred = paused["doneBytes"].as_u64().unwrap();
    assert!(
        transferred > 0 && transferred < SIZE as u64,
        "expected a partial download, got {transferred} of {SIZE}"
    );

    // The rest continues from what is already on disk.
    app.resume_download(d.id).unwrap();
    let done = events.state(d.id, "completed");
    assert_eq!(done["doneBytes"].as_u64(), Some(SIZE as u64));
    let out = PathBuf::from(done["dir"].as_str().unwrap());
    assert_eq!(std::fs::read(out.join(FILE)).unwrap(), seed.payload);

    // Removing a download deletes its files.
    app.remove_download(d.id).unwrap();
    events.removed(d.id);
    wait_until(|| !out.exists(), "the download folder to be deleted");
    assert!(
        app.downloads().unwrap().installing.is_none(),
        "a torrent must never be installed"
    );
}

#[test]
fn torrent_sources_are_validated_before_they_are_queued() {
    let dir = TempDir::new("invalid");
    let events = Arc::new(Events::default());
    let app = app(&dir.0, &dir.0.join("Games"), events, Vec::new());

    for bad in [
        "",
        "magnet:",
        "magnet:?dn=game",
        // Too short, not hexadecimal, and an unsupported base32 info hash.
        "magnet:?xt=urn:btih:1234",
        "magnet:?xt=urn:btih:zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
        "magnet:?xt=urn:btmh:1220caf1e1d1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1",
        "not a link at all",
    ] {
        let error = ErrorInfo::from(app.enqueue_torrent(APPID, TITLE, bad).unwrap_err());
        let expected = if bad.is_empty() {
            "url_empty"
        } else if bad.starts_with("magnet:") {
            "torrent_parse"
        } else {
            "url_parse"
        };
        assert_eq!(error.message, expected, "input {bad:?}");
    }

    // An upper-case info hash is kept, lower-cased, as the queue id.
    let queued = app
        .enqueue_torrent(
            APPID,
            TITLE,
            "magnet:?xt=urn:btih:0123456789ABCDEF0123456789ABCDEF01234567&dn=Game",
        )
        .unwrap();
    assert_eq!(queued.option_id, "0123456789abcdef0123456789abcdef01234567");
    assert_eq!(queued.source_kind, DownloadSourceKind::Torrent);
    assert_eq!(queued.state, DownloadState::Queued);

    // Queueing the same magnet again resumes the row instead of adding a second one.
    let again = app
        .enqueue_torrent(
            APPID,
            TITLE,
            &format!("magnet:?xt=urn:btih:{}", queued.option_id),
        )
        .unwrap();
    assert_eq!(again.id, queued.id);
    assert_eq!(app.downloads().unwrap().items.len(), 1);
}
