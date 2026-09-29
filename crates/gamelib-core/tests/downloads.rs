//! The download queue and installs against fake GOG and itch.io services and a CDN that
//! supports ranges.

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use common::http::{Request, Response, TestServer};
use gamelib_core::app::{App, EventSink, JobOptions};
use gamelib_core::db::Db;
use gamelib_core::db::downloads as download_db;
use gamelib_core::db::stores::{insert_extra_product, set_owned};
use gamelib_core::downloads::sources::{Source, this_platform};
use gamelib_core::downloads::{EVENT_PROGRESS, EVENT_STATE};
use gamelib_core::install::EVENT_CHANGED;
use gamelib_core::model::{
    DownloadState, InstallMethod, InstallState, Platform, SettingsPatch, Store,
};
use gamelib_core::secrets::{GogTokens, ItchKey, SecretStore};
use gamelib_core::stores::{StoreEndpoints, StoreProduct, StoreSyncOptions};
use gamelib_core::{Error, unix_now};
use md5::{Digest, Md5};
use serde_json::{Value, json};

const GOG_TOKEN: &str = "gog-access";
const ITCH_KEY: &str = "itch-key";
const WITCHER: &str = "1207664643";
const ETAG: &str = "\"v1\"";

/// Deterministic file contents.
fn data(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u32 * 31 + u32::from(seed)) as u8)
        .collect()
}

fn md5_hex(bytes: &[u8]) -> String {
    Md5::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn base(req: &Request) -> String {
    format!("http://{}", req.header("host").unwrap())
}

/// Serves `bytes` like a CDN: `Range` with `If-Range` against a strong ETag.
fn serve_file(req: &Request, bytes: &[u8]) -> Response {
    let full = || {
        Response::status(200)
            .with_header("ETag", ETAG)
            .with_header("Content-Type", "application/octet-stream")
            .with_body(bytes.to_vec())
    };
    let Some(range) = req.header("range") else {
        return full();
    };
    if req.header("if-range").is_some_and(|v| v != ETAG) {
        return full();
    }
    let start: usize = range
        .trim_start_matches("bytes=")
        .trim_end_matches('-')
        .parse()
        .unwrap();
    if start >= bytes.len() {
        return Response::status(416)
            .with_header("Content-Range", format!("bytes */{}", bytes.len()));
    }
    Response::status(206)
        .with_header("ETag", ETAG)
        .with_header(
            "Content-Range",
            format!("bytes {start}-{}/{}", bytes.len() - 1, bytes.len()),
        )
        .with_body(bytes[start..].to_vec())
}

/// One GOG installer file on the fake CDN.
struct GogFile {
    id: &'static str,
    name: &'static str,
    bytes: Vec<u8>,
    /// The MD5 the checksum file claims (the real one unless a test lies).
    md5: String,
}

impl GogFile {
    fn new(id: &'static str, name: &'static str, bytes: Vec<u8>) -> Self {
        let md5 = md5_hex(&bytes);
        Self {
            id,
            name,
            bytes,
            md5,
        }
    }
}

/// GOG's product API (a Windows installer of two files, a Linux and a macOS one of one file
/// each), downlinks that answer with a signed CDN address and a checksum file, and the CDN.
/// `cdn` decides each CDN response: (request, file, how many times this file was requested).
struct GogFake {
    server: TestServer,
    downlinks: Arc<AtomicU32>,
}

fn gog_fake(
    files: Vec<GogFile>,
    cdn: impl Fn(&Request, &GogFile, u32) -> Response + Send + Sync + 'static,
) -> GogFake {
    let files = Arc::new(files);
    let downlinks = Arc::new(AtomicU32::new(0));
    let hits: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(vec![0; files.len()]));
    let counter = downlinks.clone();
    let server = TestServer::start(move |req: &Request| {
        let base = base(req);
        let authorized = req.header("authorization") == Some(&format!("Bearer {GOG_TOKEN}"));
        let path = req.path.as_str();
        if path == format!("/products/{WITCHER}") {
            assert_eq!(req.param("expand"), Some("downloads"));
            let installer = |id: &str, os: &str, picked: &[usize]| {
                let list: Vec<Value> = picked
                    .iter()
                    .map(|&i| {
                        let f = &files[i];
                        json!({"id": f.id, "size": f.bytes.len(),
                               "downlink": format!("{base}/products/{WITCHER}/downlink/installer/{}", f.id)})
                    })
                    .collect();
                let size: usize = picked.iter().map(|&i| files[i].bytes.len()).sum();
                json!({"id": id, "os": os, "language": "en", "language_full": "English", "version": "4.04b",
                       "total_size": size, "files": list})
            };
            let windows: Vec<usize> = (0..files.len())
                .filter(|&i| files[i].id.starts_with("en1"))
                .collect();
            return Response::json(
                json!({"id": WITCHER.parse::<u64>().unwrap(), "title": "The Witcher 3", "downloads": {"installers": [
                    installer("installer_windows_en", "windows", &windows),
                    installer("installer_linux_en", "linux", &[files.iter().position(|f| f.id == "en3installer0").unwrap()]),
                    installer("installer_mac_en", "mac", &[files.iter().position(|f| f.id == "en2installer0").unwrap()]),
                ]}})
                .to_string(),
            );
        }
        if let Some(id) = path.strip_prefix(&format!("/products/{WITCHER}/downlink/installer/")) {
            if !authorized {
                return Response::status(401);
            }
            let n = counter.fetch_add(1, Ordering::SeqCst) + 1;
            let f = files.iter().find(|f| f.id == id).unwrap();
            return Response::json(
                json!({"downlink": format!("{base}/cdn/{}?token={n}", f.name),
                       "checksum": format!("{base}/checksum/{}.xml", f.name)})
                .to_string(),
            );
        }
        if let Some(name) = path.strip_prefix("/checksum/") {
            let f = files
                .iter()
                .find(|f| format!("{}.xml", f.name) == name)
                .unwrap();
            return Response::status(200).with_body(format!(
                r#"<file name="{}" available="1" md5="{}" chunks="1" total_size="{}"></file>"#,
                f.name,
                f.md5,
                f.bytes.len()
            ));
        }
        if let Some(name) = path.strip_prefix("/cdn/") {
            let i = files.iter().position(|f| f.name == name).unwrap();
            let n = {
                let mut hits = hits.lock().unwrap();
                hits[i] += 1;
                hits[i]
            };
            return cdn(req, &files[i], n);
        }
        Response::status(404)
    });
    GogFake { server, downlinks }
}

fn witcher_files() -> Vec<GogFile> {
    vec![
        GogFile::new("en1installer0", "setup_witcher.exe", data(200_000, 1)),
        GogFile::new("en1installer1", "setup_witcher-1.bin", data(50_000, 2)),
        GogFile::new("en3installer0", "witcher_linux.sh", data(30_000, 3)),
        GogFile::new("en2installer0", "witcher_mac.pkg", data(20_000, 4)),
    ]
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "gamelib-downloads-{tag}-{}-{}",
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

/// Every event, with a way to wait for one.
#[derive(Default)]
struct Events {
    log: Mutex<Vec<(String, Value)>>,
    ready: Condvar,
}

impl EventSink for Events {
    fn emit(&self, event: &str, payload: Value) {
        self.log.lock().unwrap().push((event.to_owned(), payload));
        self.ready.notify_all();
    }
}

impl Events {
    fn wait(&self, what: &str, pred: impl Fn(&str, &Value) -> bool) -> Value {
        let guard = self.log.lock().unwrap();
        let (guard, timeout) = self
            .ready
            .wait_timeout_while(guard, Duration::from_secs(30), |log| {
                !log.iter().any(|(e, p)| pred(e, p))
            })
            .unwrap();
        assert!(
            !timeout.timed_out(),
            "timed out waiting for {what}: {:?}",
            *guard
        );
        guard.iter().find(|(e, p)| pred(e, p)).unwrap().1.clone()
    }

    fn state(&self, id: i64, state: &str) -> Value {
        self.wait(state, |e, p| {
            e == EVENT_STATE && p["id"] == id && p["state"] == state
        })
    }

    fn count(&self, event: &str) -> usize {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|(e, _)| e == event)
            .count()
    }
}

struct Setup {
    dir: TempDir,
    app: App,
    events: Arc<Events>,
}

impl Setup {
    fn library(&self) -> PathBuf {
        self.dir.0.join("Games")
    }
}

/// An app whose stores point at `base`, owning the Witcher on GOG and Celeste on itch.io.
fn setup(tag: &str, base: &str, gog: bool, itch: bool) -> Setup {
    let dir = TempDir::new(tag);
    let path = dir.0.join("gamelib.db");
    let mut db = Db::open(&path).unwrap();
    let now = unix_now();
    for (store, id, title) in [
        (Store::Gog, WITCHER, "The Witcher 3: Wild Hunt"),
        (Store::Itch, "11", "Celeste"),
    ] {
        let product = StoreProduct {
            store,
            product_id: id.into(),
            kind: "game".into(),
            title: title.into(),
            win: true,
            ..Default::default()
        };
        insert_extra_product(db.conn(), &product, now).unwrap();
    }
    set_owned(db.conn_mut(), Store::Gog, &[(WITCHER.into(), None)]).unwrap();
    set_owned(db.conn_mut(), Store::Itch, &[("11".into(), Some(900))]).unwrap();
    drop(db);

    let secrets = SecretStore::new(&dir.0);
    secrets
        .update(|s| {
            if gog {
                s.gog = Some(GogTokens {
                    access_token: GOG_TOKEN.into(),
                    refresh_token: "refresh".into(),
                    expires_at: unix_now() + 3600,
                    user_id: "1".into(),
                    username: Some("tester".into()),
                });
            }
            if itch {
                s.itch = Some(ItchKey {
                    api_key: ITCH_KEY.into(),
                    user_id: 42,
                    username: "player".into(),
                });
            }
        })
        .unwrap();

    let options = JobOptions {
        stores: StoreSyncOptions {
            endpoints: StoreEndpoints::all_at(base),
            ..Default::default()
        },
        ..Default::default()
    };
    let events = Arc::new(Events::default());
    let app = App::with_options(path, events.clone(), options).unwrap();
    let setup = Setup { dir, app, events };
    let library = setup.library().display().to_string();
    setup
        .app
        .update_settings(&gamelib_core::model::SettingsPatch {
            library_dir: Some(library),
            ..Default::default()
        })
        .unwrap();
    setup
}

fn read(dir: &str, name: &str) -> Vec<u8> {
    std::fs::read(Path::new(dir).join(name)).unwrap()
}

fn leftovers(dir: &str) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".part"))
        .collect()
}

#[test]
fn gog_files_are_listed_best_first() {
    let fake = gog_fake(witcher_files(), |req, f, _| serve_file(req, &f.bytes));
    let s = setup("list", &fake.server.base, true, false);
    let options = s.app.store_files(Store::Gog, WITCHER).unwrap();
    assert_eq!(options.len(), 3);
    assert!(options[0].recommended);
    assert_eq!(options[0].platform, Some(this_platform()));
    assert!(options[1..].iter().all(|o| !o.recommended));
    let windows = options
        .iter()
        .find(|o| o.platform == Some(Platform::Win))
        .unwrap();
    assert_eq!(windows.id, "installer_windows_en");
    assert_eq!(windows.label, "Windows · English · 4.04b");
    assert_eq!((windows.files, windows.size), (2, 250_000));
    assert!(matches!(
        s.app.store_files(Store::Gog, "404"),
        Err(Error::Invalid("no_files"))
    ));
}

#[test]
fn a_dropped_connection_resumes_and_the_files_are_verified() {
    // The first transfer of the installer stops halfway; the retry asks for the rest.
    let fake = gog_fake(witcher_files(), |req, f, n| {
        if f.id == "en1installer0" && n == 1 {
            serve_file(req, &f.bytes).cut_at(f.bytes.len() / 2)
        } else {
            serve_file(req, &f.bytes)
        }
    });
    let s = setup("resume", &fake.server.base, true, false);
    s.app.start_downloads().unwrap();
    let d = s
        .app
        .enqueue_download(Store::Gog, WITCHER, "installer_windows_en")
        .unwrap();
    assert_eq!(d.title, "The Witcher 3: Wild Hunt");
    assert_eq!((d.total_bytes, d.files), (250_000, 2));
    assert_eq!(d.option_label.as_deref(), Some("Windows · English · 4.04b"));
    assert!(d.dir.starts_with(&s.library().display().to_string()));

    let done = s.events.state(d.id, "completed");
    assert_eq!(done["doneBytes"], 250_000, "{done}");
    assert!(done["finishedAt"].is_i64());
    let files = witcher_files();
    assert_eq!(read(&d.dir, "setup_witcher.exe"), files[0].bytes);
    assert_eq!(read(&d.dir, "setup_witcher-1.bin"), files[1].bytes);
    assert!(leftovers(&d.dir).is_empty());

    let cdn: Vec<Request> = fake
        .server
        .requests()
        .into_iter()
        .filter(|r| r.path == "/cdn/setup_witcher.exe")
        .collect();
    assert_eq!(cdn.len(), 2);
    assert_eq!(cdn[0].header("range"), None);
    assert_eq!(cdn[1].header("range"), Some("bytes=100000-"));
    assert_eq!(cdn[1].header("if-range"), Some(ETAG));
    assert!(s.events.count(EVENT_PROGRESS) > 0);

    let list = s.app.downloads().unwrap();
    assert_eq!(list.items.len(), 1);
    assert_eq!(list.items[0].state, DownloadState::Completed);
    assert!(list.live.is_none());
    s.app.clear_finished_downloads().unwrap();
    assert!(s.app.downloads().unwrap().items.is_empty());
    assert!(
        Path::new(&d.dir).join("setup_witcher.exe").exists(),
        "files stay for the installer"
    );
}

#[test]
fn a_server_that_ignores_ranges_starts_over() {
    let fake = gog_fake(witcher_files(), |_req, f, n| {
        let full = Response::status(200).with_body(f.bytes.clone());
        if n == 1 { full.cut_at(1000) } else { full }
    });
    let s = setup("norange", &fake.server.base, true, false);
    s.app.start_downloads().unwrap();
    let d = s
        .app
        .enqueue_download(Store::Gog, WITCHER, "installer_linux_en")
        .unwrap();
    s.events.state(d.id, "completed");
    assert_eq!(read(&d.dir, "witcher_linux.sh"), witcher_files()[2].bytes);
}

#[test]
fn expired_addresses_are_resolved_again() {
    // The first signed address is refused, as an expired one would be.
    let fake = gog_fake(witcher_files(), |req, f, _| {
        if req.param("token") == Some("1") {
            Response::status(403)
        } else {
            serve_file(req, &f.bytes)
        }
    });
    let s = setup("expired", &fake.server.base, true, false);
    s.app.start_downloads().unwrap();
    let d = s
        .app
        .enqueue_download(Store::Gog, WITCHER, "installer_mac_en")
        .unwrap();
    s.events.state(d.id, "completed");
    assert_eq!(fake.downlinks.load(Ordering::SeqCst), 2);
    assert_eq!(read(&d.dir, "witcher_mac.pkg"), witcher_files()[3].bytes);
}

#[test]
fn a_bad_checksum_fails_after_one_retry() {
    let mut files = witcher_files();
    files[2].md5 = md5_hex(b"something else");
    let fake = gog_fake(files, |req, f, _| serve_file(req, &f.bytes));
    let s = setup("checksum", &fake.server.base, true, false);
    s.app.start_downloads().unwrap();
    let d = s
        .app
        .enqueue_download(Store::Gog, WITCHER, "installer_linux_en")
        .unwrap();
    let failed = s.events.state(d.id, "failed");
    assert_eq!(failed["error"]["kind"], "invalid");
    assert_eq!(failed["error"]["message"], "checksum");
    assert_eq!(fake.server.count("/cdn/"), 2);
    assert!(!Path::new(&d.dir).join("witcher_linux.sh").exists());
    assert!(leftovers(&d.dir).is_empty(), "a corrupt file is not kept");
}

#[test]
fn signed_out_downloads_fail_with_a_clear_reason() {
    let fake = gog_fake(witcher_files(), |req, f, _| serve_file(req, &f.bytes));
    let s = setup("signedout", &fake.server.base, false, false);
    s.app.start_downloads().unwrap();
    let d = s
        .app
        .enqueue_download(Store::Gog, WITCHER, "installer_linux_en")
        .unwrap();
    let failed = s.events.state(d.id, "failed");
    assert_eq!(failed["error"]["message"], "gog_signed_out");
    assert!(matches!(
        s.app
            .enqueue_download(Store::Gog, WITCHER, "installer_nope"),
        Err(Error::Invalid("no_files"))
    ));
    assert!(matches!(
        s.app
            .enqueue_download(Store::Gog, "999", "installer_linux_en"),
        Err(Error::NotFound)
    ));
}

#[test]
fn pause_resume_and_remove() {
    // A slow CDN: 256 KB in 8 KB pieces.
    let mut files = witcher_files();
    files[2].bytes = data(256 * 1024, 9);
    files[2].md5 = md5_hex(&files[2].bytes);
    files[3].bytes = data(256 * 1024, 7);
    files[3].md5 = md5_hex(&files[3].bytes);
    let expected = files[2].bytes.clone();
    let fake = gog_fake(files, |req, f, _| {
        serve_file(req, &f.bytes).paced(8 * 1024, Duration::from_millis(15))
    });
    let s = setup("pause", &fake.server.base, true, false);
    s.app.start_downloads().unwrap();
    let d = s
        .app
        .enqueue_download(Store::Gog, WITCHER, "installer_linux_en")
        .unwrap();
    s.events.wait("progress", |e, p| {
        e == EVENT_PROGRESS && p["id"] == d.id && p["doneBytes"].as_u64() > Some(0)
    });
    assert!(s.app.downloads().unwrap().live.is_some());
    s.app.pause_download(d.id).unwrap();
    s.events.state(d.id, "paused");
    let part = Path::new(&d.dir).join("witcher_linux.sh.part");
    let kept = std::fs::metadata(&part).unwrap().len();
    assert!(
        kept > 0 && kept < expected.len() as u64,
        "kept {kept} bytes"
    );
    let paused = s.app.downloads().unwrap();
    assert_eq!(paused.items[0].state, DownloadState::Paused);
    assert!(paused.items[0].done_bytes > 0);

    // Queuing the same variant again resumes it instead of starting a second copy.
    let again = s
        .app
        .enqueue_download(Store::Gog, WITCHER, "installer_linux_en")
        .unwrap();
    assert_eq!(again.id, d.id);
    s.events.state(d.id, "completed");
    assert_eq!(read(&d.dir, "witcher_linux.sh"), expected);
    let ranged = fake
        .server
        .requests()
        .into_iter()
        .filter(|r| r.path == "/cdn/witcher_linux.sh")
        .filter_map(|r| r.header("range").map(str::to_owned))
        .collect::<Vec<_>>();
    assert_eq!(ranged.len(), 1, "resumed with a range request");

    // Removing a running download stops it and deletes its folder.
    let other = s
        .app
        .enqueue_download(Store::Gog, WITCHER, "installer_mac_en")
        .unwrap();
    s.events.wait("progress", |e, p| {
        e == EVENT_PROGRESS && p["id"] == other.id && p["doneBytes"].as_u64() > Some(0)
    });
    s.app.remove_download(other.id).unwrap();
    s.events.wait("removal", |e, p| {
        e == EVENT_STATE && p["id"] == other.id && p["removed"] == true
    });
    assert!(!Path::new(&other.dir).exists());
    let ids: Vec<i64> = s
        .app
        .downloads()
        .unwrap()
        .items
        .iter()
        .map(|d| d.id)
        .collect();
    assert_eq!(ids, [d.id]);
    assert!(matches!(
        s.app.remove_download(other.id),
        Err(Error::NotFound)
    ));
}

#[test]
fn downloads_interrupted_by_closing_continue_on_start() {
    let fake = gog_fake(witcher_files(), |req, f, _| serve_file(req, &f.bytes));
    let s = setup("restart", &fake.server.base, true, false);
    // Queued while the worker is not running, then "interrupted" with the whole file already
    // in its part file: the server answers the range request with 416.
    let d = s
        .app
        .enqueue_download(Store::Gog, WITCHER, "installer_mac_en")
        .unwrap();
    std::fs::create_dir_all(&d.dir).unwrap();
    std::fs::write(
        Path::new(&d.dir).join("witcher_mac.pkg.part"),
        &witcher_files()[3].bytes,
    )
    .unwrap();
    let db = Db::open(&s.dir.0.join("gamelib.db")).unwrap();
    db.conn()
        .execute(
            "UPDATE downloads SET state = 'downloading' WHERE id = ?1",
            [d.id],
        )
        .unwrap();

    s.app.start_downloads().unwrap();
    s.events.state(d.id, "completed");
    let cdn: Vec<Request> = fake
        .server
        .requests()
        .into_iter()
        .filter(|r| r.path.starts_with("/cdn/"))
        .collect();
    assert_eq!(cdn.len(), 1);
    assert_eq!(cdn[0].header("range"), Some("bytes=20000-"));
    assert_eq!(read(&d.dir, "witcher_mac.pkg"), witcher_files()[3].bytes);
}

#[test]
fn itch_uploads_follow_the_redirect_with_the_download_key() {
    let game = data(40_000, 5);
    let served = game.clone();
    let server = TestServer::start(move |req: &Request| {
        let authorized = req.header("authorization") == Some(ITCH_KEY);
        match req.path.as_str() {
            "/games/11/uploads" if authorized => {
                assert_eq!(req.param("download_key_id"), Some("900"));
                Response::json(
                    json!({"uploads": [
                        {"id": 5, "filename": "celeste-win.zip", "size": 40_000, "traits": ["p_windows"], "type": "default"},
                        {"id": 6, "filename": "celeste-linux.tar.gz", "size": 41_000, "traits": ["p_linux"], "type": "default"},
                        {"id": 7, "filename": "celeste-mac.zip", "size": 42_000, "traits": ["p_osx"], "type": "default"},
                        {"id": 8, "filename": "ost.zip", "size": 1, "type": "soundtrack"}
                    ]})
                    .to_string(),
                )
            }
            "/games/12/uploads" if authorized => Response::status(403),
            "/uploads/5/download" if authorized => {
                assert_eq!(req.param("download_key_id"), Some("900"));
                Response::status(302).with_header(
                    "Location",
                    format!("{}/cdn/files/celeste-win.zip?sig=abc", base(req)),
                )
            }
            "/cdn/files/celeste-win.zip" => {
                assert_eq!(
                    req.header("authorization"),
                    None,
                    "the key never goes to the CDN"
                );
                serve_file(req, &served)
            }
            _ => Response::status(404),
        }
    });
    let s = setup("itch", &server.base, false, true);
    let options = s.app.store_files(Store::Itch, "11").unwrap();
    assert_eq!(options.len(), 3);
    assert!(options[0].recommended);
    assert!(matches!(
        s.app.store_files(Store::Itch, "12"),
        Err(Error::Invalid("not_owned"))
    ));

    s.app.start_downloads().unwrap();
    let d = s.app.enqueue_download(Store::Itch, "11", "5").unwrap();
    assert_eq!(d.title, "Celeste");
    s.events.state(d.id, "completed");
    assert_eq!(read(&d.dir, "celeste-win.zip"), game);
}

#[test]
fn direct_sources_round_trip() {
    let s = Source::Direct("https://cdn.example.com/game.zip".into());
    assert_eq!(Source::from_db(&s.to_db()), Some(s));
    assert_eq!(Source::from_db("other:1"), None);
}

#[test]
fn captured_downloads_are_queued_and_downloaded() {
    let bytes = data(60_000, 6);
    let served = bytes.clone();
    let server = TestServer::start(move |req: &Request| {
        if req.path == "/cdn/captured.bin" {
            serve_file(req, &served)
        } else {
            Response::status(404)
        }
    });
    let s = setup("captured", &server.base, false, false);
    let url = format!("{}/cdn/captured.bin", server.base);
    let d = s
        .app
        .enqueue_captured(570, "Doki Doki Literature Club", &url, "doki-doki.zip")
        .unwrap();
    assert_eq!(d.store, Store::Web);
    assert_eq!(d.appid, Some(570));
    assert_eq!(d.option_id, "doki-doki.zip");
    assert_eq!(d.option_label.as_deref(), Some("doki-doki.zip"));
    assert_eq!(d.platform, Some(Platform::Win));
    assert_eq!(d.state, DownloadState::Queued);
    assert_eq!(d.files, 1);

    let db = Db::open(&s.dir.0.join("gamelib.db")).unwrap();
    let rows = download_db::files(db.conn(), d.id).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].source, Source::Direct(url));

    s.app.start_downloads().unwrap();
    s.events.state(d.id, "completed");
    assert_eq!(read(&d.dir, "captured.bin"), bytes);
}

#[test]
fn captured_downloads_reject_non_http_urls() {
    let s = setup("captured-bad", "http://127.0.0.1:1", false, false);
    assert!(matches!(
        s.app
            .enqueue_captured(570, "Game", "ftp://example.com/game.zip", "game.zip"),
        Err(Error::Invalid("url_scheme"))
    ));
    assert!(matches!(
        s.app
            .enqueue_captured(570, "Game", "javascript:alert(1)", "game.zip"),
        Err(Error::Invalid("url_parse"))
    ));
}

// --- installs ------------------------------------------------------------------------------------

/// An itch.io upload on the fake service.
struct Upload {
    id: u64,
    name: &'static str,
    bytes: Vec<u8>,
}

/// itch.io's API for game 11 (owned with download key 900) and a CDN serving its uploads.
fn itch_fake(uploads: Vec<Upload>) -> TestServer {
    let uploads = Arc::new(uploads);
    TestServer::start(move |req: &Request| {
        let authorized = req.header("authorization") == Some(ITCH_KEY);
        if req.path == "/games/11/uploads" && authorized {
            let list: Vec<Value> = uploads
                .iter()
                .map(|u| json!({"id": u.id, "filename": u.name, "size": u.bytes.len(), "traits": ["p_windows", "p_linux"], "type": "default"}))
                .collect();
            return Response::json(json!({ "uploads": list }).to_string());
        }
        if let Some(rest) = req.path.strip_prefix("/uploads/")
            && let Some(id) = rest.strip_suffix("/download")
            && authorized
        {
            let u = uploads.iter().find(|u| u.id.to_string() == id).unwrap();
            return Response::status(302)
                .with_header("Location", format!("{}/cdn/{}", base(req), u.name));
        }
        if let Some(name) = req.path.strip_prefix("/cdn/")
            && let Some(u) = uploads.iter().find(|u| u.name == name)
        {
            return serve_file(req, &u.bytes);
        }
        Response::status(404)
    })
}

fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;
    let mut out = std::io::Cursor::new(Vec::new());
    {
        let mut w = zip::ZipWriter::new(&mut out);
        let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
        for (name, data) in entries {
            w.start_file(*name, opts).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
    }
    out.into_inner()
}

impl Events {
    fn install_state(&self, id: i64, state: &str) -> Value {
        self.wait(state, |e, p| {
            e == EVENT_STATE && p["id"] == id && p["installState"] == state
        })
    }
}

#[cfg(unix)]
fn wait_for_file(path: &Path) -> bool {
    for _ in 0..150 {
        if std::fs::read_to_string(path).is_ok_and(|s| !s.is_empty()) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn archives_are_installed_started_and_uninstalled() {
    let game = zip_bytes(&[
        ("Celeste/Celeste.exe", b"MZ not really"),
        (
            "Celeste/celeste.x86_64",
            b"#!/bin/sh\necho started > started.txt\n",
        ),
        ("Celeste/Content/level1.bin", &[1u8; 3000]),
    ]);
    let server = itch_fake(vec![Upload {
        id: 5,
        name: "celeste.zip",
        bytes: game,
    }]);
    let s = setup("install-zip", &server.base, false, true);
    s.app.start_downloads().unwrap();
    let d = s.app.enqueue_download(Store::Itch, "11", "5").unwrap();

    let installed = s.events.install_state(d.id, "installed");
    assert_eq!(installed["installKind"], "zip");
    let games = s.app.installs().unwrap();
    assert_eq!(games.len(), 1);
    let game = &games[0];
    let dir = s.library().join("Celeste");
    assert_eq!(game.method, InstallMethod::Archive);
    assert_eq!(
        game.dir.as_deref(),
        Some(dir.display().to_string().as_str())
    );
    assert!(
        dir.join("Content").join("level1.bin").exists(),
        "the wrapping folder is unwrapped"
    );
    let program = if cfg!(windows) {
        "Celeste.exe"
    } else {
        "celeste.x86_64"
    };
    assert_eq!(game.exe, Some(dir.join(program).display().to_string()));
    assert!(
        !Path::new(&d.dir).exists(),
        "the download is removed once installed"
    );
    assert!(s.events.count(EVENT_CHANGED) >= 1);
    assert_eq!(s.app.install_folder(Store::Itch, "11").unwrap(), dir);

    #[cfg(unix)]
    {
        s.app.launch_game(Store::Itch, "11").unwrap();
        assert!(
            wait_for_file(&dir.join("started.txt")),
            "the game started in its folder"
        );
    }

    // Another program can be chosen; a relative path cannot.
    let other = dir.join("Content").join("level1.bin").display().to_string();
    let changed = s.app.set_launch_target(Store::Itch, "11", &other).unwrap();
    assert_eq!(changed.exe.as_deref(), Some(other.as_str()));
    assert!(matches!(
        s.app.set_launch_target(Store::Itch, "11", "Celeste.exe"),
        Err(Error::Invalid("launch_target"))
    ));

    s.app.uninstall_game(Store::Itch, "11").unwrap();
    assert!(!dir.exists());
    assert!(s.app.installs().unwrap().is_empty());
    assert_eq!(s.app.downloads().unwrap().items[0].install_state, None);
    assert!(matches!(
        s.app.launch_game(Store::Itch, "11"),
        Err(Error::NotFound)
    ));
}

#[test]
fn updates_go_into_the_same_folder_and_keep_saves() {
    let v1 = zip_bytes(&[("Game/game.x86_64", b"v1"), ("Game/Game.exe", b"v1")]);
    let v2 = zip_bytes(&[
        ("Game/game.x86_64", b"v2"),
        ("Game/Game.exe", b"v2"),
        ("Game/new.pak", b"new"),
    ]);
    let server = itch_fake(vec![
        Upload {
            id: 5,
            name: "game-1.0.zip",
            bytes: v1,
        },
        Upload {
            id: 6,
            name: "game-1.1.zip",
            bytes: v2,
        },
    ]);
    let s = setup("install-update", &server.base, false, true);
    s.app
        .update_settings(&SettingsPatch {
            keep_installers: Some(true),
            ..Default::default()
        })
        .unwrap();
    s.app.start_downloads().unwrap();
    let first = s.app.enqueue_download(Store::Itch, "11", "5").unwrap();
    s.events.install_state(first.id, "installed");
    let dir = s.library().join("Celeste");
    std::fs::write(dir.join("save.dat"), b"progress").unwrap();

    let second = s.app.enqueue_download(Store::Itch, "11", "6").unwrap();
    s.events.install_state(second.id, "installed");
    assert_eq!(std::fs::read(dir.join("Game.exe")).unwrap(), b"v2");
    assert_eq!(std::fs::read(dir.join("new.pak")).unwrap(), b"new");
    assert_eq!(std::fs::read(dir.join("save.dat")).unwrap(), b"progress");
    assert!(!s.library().join("Celeste (2)").exists());
    assert!(
        Path::new(&second.dir).join("game-1.1.zip").exists(),
        "installers are kept when asked"
    );
    assert_eq!(s.app.installs().unwrap().len(), 1);
}

#[test]
fn what_cannot_be_installed_is_left_for_the_user() {
    let mut rar = b"Rar!\x1A\x07\x00".to_vec();
    rar.extend(data(2000, 3));
    let mut nsis = b"MZ".to_vec();
    nsis.resize(4096, 0);
    nsis.extend_from_slice(b"NullsoftInst");
    let full = zip_bytes(&[("Game/game.x86_64", b"x")]);
    let broken = full[..full.len() - 30].to_vec();
    let server = itch_fake(vec![
        Upload {
            id: 5,
            name: "game.rar",
            bytes: rar,
        },
        Upload {
            id: 6,
            name: "game-setup.exe",
            bytes: nsis,
        },
        Upload {
            id: 7,
            name: "broken.zip",
            bytes: broken,
        },
    ]);
    let s = setup("install-manual", &server.base, false, true);
    s.app.start_downloads().unwrap();

    let rar = s.app.enqueue_download(Store::Itch, "11", "5").unwrap();
    let manual = s.events.install_state(rar.id, "manual");
    assert_eq!(manual["installKind"], "rar");
    assert!(
        Path::new(&rar.dir).join("game.rar").exists(),
        "kept for the user"
    );

    // Someone else's installer waits for approval on Windows; elsewhere it cannot run at all.
    let setup_exe = s.app.enqueue_download(Store::Itch, "11", "6").unwrap();
    let state = if cfg!(windows) { "confirm" } else { "manual" };
    let waiting = s.events.install_state(setup_exe.id, state);
    assert_eq!(waiting["installKind"], "nsis");

    let broken = s.app.enqueue_download(Store::Itch, "11", "7").unwrap();
    let failed = s.events.install_state(broken.id, "failed");
    assert_eq!(
        failed["installError"]["message"], "archive_corrupt",
        "{failed}"
    );
    assert!(s.app.installs().unwrap().is_empty());
    // Retrying runs it again (and fails again).
    s.app.retry_install(broken.id).unwrap();
    s.events.wait("second failure", |e, p| {
        e == EVENT_STATE && p["id"] == broken.id && p["installState"] == "installing"
    });
    let list = s.app.downloads().unwrap();
    assert!(
        list.items
            .iter()
            .all(|d| d.state == DownloadState::Completed)
    );
    assert!(
        matches!(s.app.approve_install(rar.id), Ok(())),
        "approving a manual one does nothing"
    );
    let rar_now = list.items.iter().find(|d| d.id == rar.id).unwrap();
    assert_eq!(rar_now.install_state, Some(InstallState::Manual));
}

#[cfg(not(windows))]
#[test]
fn gog_windows_installers_are_not_run_elsewhere() {
    let mut files = witcher_files();
    let mut setup_exe = b"MZ".to_vec();
    setup_exe.resize(8192, 0);
    setup_exe.extend_from_slice(b"Inno Setup Setup Data (6.2.2)");
    files[0].bytes = setup_exe;
    files[0].md5 = md5_hex(&files[0].bytes);
    let fake = gog_fake(files, |req, f, _| serve_file(req, &f.bytes));
    let s = setup("install-gog", &fake.server.base, true, false);
    s.app.start_downloads().unwrap();
    let d = s
        .app
        .enqueue_download(Store::Gog, WITCHER, "installer_windows_en")
        .unwrap();
    let manual = s.events.install_state(d.id, "manual");
    assert_eq!(manual["installKind"], "inno_setup");
    assert!(Path::new(&d.dir).join("setup_witcher.exe").exists());
}
