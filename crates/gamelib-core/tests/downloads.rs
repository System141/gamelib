//! The download queue against fake GOG and itch.io services and a CDN that supports ranges.

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use common::http::{Request, Response, TestServer};
use gamelib_core::app::{App, EventSink, JobOptions};
use gamelib_core::db::Db;
use gamelib_core::db::stores::{insert_extra_product, set_owned};
use gamelib_core::downloads::sources::this_platform;
use gamelib_core::downloads::{EVENT_PROGRESS, EVENT_STATE};
use gamelib_core::model::{DownloadState, Platform, Store};
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
