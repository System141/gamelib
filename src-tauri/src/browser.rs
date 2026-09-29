//! In-app browser window that captures downloads into the app's own queue.
//!
//! The window shows an external site (remote content: the capability file only covers `main`,
//! so it cannot reach the app's commands). A toolbar of its own lives in a shadow root above the
//! page, magnet links and `.torrent` downloads go to the torrent engine, and anything else the
//! user downloads is enqueued as a `Store::Web` HTTP download instead of the webview saving it.
//!
//! The window is reused for other games, so the game a download belongs to is kept in a shared
//! [`BrowserContext`] rather than in the window's own closures.

use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

use gamelib_core::app::App;
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder, webview::DownloadEvent};

use crate::error::{CmdError, CmdResult};

const LABEL: &str = "browser";
/// Shown until a page reports its own title.
const TITLE: &str = "GameLib · Tarayıcı";

/// Which game the window's downloads belong to.
#[derive(Default)]
struct BrowserContext {
    appid: u32,
    title: String,
}

/// The single browser window's context, updated every time the window is opened on a game.
static CONTEXT: LazyLock<Mutex<BrowserContext>> =
    LazyLock::new(|| Mutex::new(BrowserContext::default()));

fn context() -> MutexGuard<'static, BrowserContext> {
    CONTEXT.lock().unwrap_or_else(|e| e.into_inner())
}

/// Opens the in-app browser on `url` and queues anything the user downloads there.
pub async fn open(
    handle: &AppHandle,
    app: Arc<App>,
    appid: u32,
    title: String,
    url: String,
) -> CmdResult<()> {
    let url = gamelib_core::links::validate::parse_link_url(&url).map_err(CmdError::from)?;
    // Before anything can be downloaded from this window: a reused window would otherwise queue
    // under the game it was first opened for.
    set_current(appid, title);
    if let Some(window) = handle.get_webview_window(LABEL) {
        // Reuse the open window: navigate it to the new page instead of stacking windows.
        window
            .navigate(url)
            .map_err(|e| CmdError::other(format!("browser navigate: {e}")))?;
        let _ = window.set_focus();
        return Ok(());
    }
    let navigation_app = Arc::clone(&app);
    let download_app = Arc::clone(&app);
    let title_handle = handle.clone();
    WebviewWindowBuilder::new(handle, LABEL, WebviewUrl::External(url))
        .title(TITLE)
        .inner_size(1100.0, 800.0)
        .min_inner_size(700.0, 500.0)
        .center()
        .initialization_script(TOOLBAR_SCRIPT)
        .on_navigation(move |url| navigate(&navigation_app, url))
        .on_document_title_changed(move |_webview, title| {
            let title = page_title(&title);
            if let Some(window) = title_handle.get_webview_window(LABEL) {
                let _ = window.set_title(&title);
            }
        })
        .on_download(move |_webview, event| on_download(&download_app, event))
        .build()
        .map_err(|e| CmdError::other(format!("browser window: {e}")))?;
    Ok(())
}

/// Navigation policy: pages load, a magnet link becomes a queued torrent instead of a page, and
/// every other scheme is dropped.
fn navigate(app: &App, url: &Url) -> bool {
    match url.scheme() {
        "http" | "https" => true,
        "magnet" => {
            enqueue_torrent(app, url.as_str());
            false
        }
        _ => false,
    }
}

/// What a download request becomes: a torrent when the browser suggests a `.torrent` file,
/// where the name comes from the response (`Content-Disposition`), otherwise an HTTP download.
/// Both are kept by the queue, so the webview's own download is always cancelled.
fn on_download(app: &App, event: DownloadEvent<'_>) -> bool {
    match event {
        DownloadEvent::Requested { url, destination } => {
            let file_name = destination
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| file_name_from_url(&url));
            if is_torrent_file(&file_name) {
                enqueue_torrent(app, url.as_str());
            } else {
                let (appid, title) = current_game();
                if let Err(e) = app.enqueue_captured(appid, &title, url.as_str(), &file_name) {
                    eprintln!("browser: could not enqueue download: {e}");
                }
            }
            // Cancel the webview's own download; the queue handles it instead.
            false
        }
        DownloadEvent::Finished { .. } => true,
        // non_exhaustive: future variants should not cancel anything either.
        _ => true,
    }
}

fn enqueue_torrent(app: &App, source: &str) {
    let (appid, title) = current_game();
    if let Err(e) = app.enqueue_torrent(appid, &title, source) {
        eprintln!("browser: could not queue the torrent: {e}");
    }
}

/// The game the window is currently showing, as set by the last [`open`].
fn current_game() -> (u32, String) {
    let current = context();
    (current.appid, current.title.clone())
}

/// Remembers which game the window's downloads belong to.
fn set_current(appid: u32, title: String) {
    let mut current = context();
    current.appid = appid;
    current.title = title;
}

fn is_torrent_file(file_name: &str) -> bool {
    file_name.to_ascii_lowercase().ends_with(".torrent")
}

/// The window title: the site's own title after the app's.
fn page_title(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() || title == TITLE {
        TITLE.to_owned()
    } else {
        format!("{TITLE} · {title}")
    }
}

/// The last path segment of `url`, percent-decoded; `"download"` when there is none.
fn file_name_from_url(url: &Url) -> String {
    let Some(segment) = url.path_segments().and_then(|mut s| s.next_back()) else {
        return "download".into();
    };
    let decoded = percent_decode(segment);
    if decoded.is_empty() {
        "download".into()
    } else {
        decoded
    }
}

/// Decodes `%XX` escapes; other bytes are kept as-is.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(hi * 16 + lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// The browser's own toolbar: back, forward, reload and an address field.
///
/// It is a shadow root on a fixed-position host, so the remote page's CSS cannot reach into it
/// (and its own styles stay out of the page). The toolbar is the only way to navigate: the
/// window has no other chrome.
const TOOLBAR_SCRIPT: &str = r##"
(() => {
  const HOST_ID = "gamelib-toolbar";
  const build = () => {
    if (document.getElementById(HOST_ID)) return;
    const parent = document.body || document.documentElement;
    if (!parent) return void setTimeout(build, 50);
    const host = document.createElement("div");
    host.id = HOST_ID;
    host.style.cssText = "position:fixed;top:0;left:0;right:0;height:40px;z-index:2147483647";
    const root = host.attachShadow({ mode: "open" });
    root.innerHTML = `
      <style>
        :host { all: initial; }
        .bar { display:flex; align-items:center; gap:6px; height:40px; padding:0 10px; box-sizing:border-box;
               background:#0b0f16; border-bottom:1px solid rgba(255,255,255,.08);
               font:13px/1.2 system-ui, sans-serif; }
        button { width:30px; height:26px; border:0; border-radius:6px; background:rgba(255,255,255,.06);
                 color:#e7ebf2; cursor:pointer; font-size:15px; line-height:1; }
        button:hover { background:rgba(255,255,255,.16); }
        input { flex:1; min-width:0; height:26px; border:0; border-radius:6px; background:#161c26;
                color:#e7ebf2; padding:0 9px; font:12px/1.2 ui-monospace, monospace; outline:none; }
        input:focus { box-shadow:0 0 0 2px rgba(96,165,250,.45); }
      </style>
      <div class="bar">
        <button id="back" title="Geri" aria-label="Geri">‹</button>
        <button id="forward" title="İleri" aria-label="İleri">›</button>
        <button id="reload" title="Yenile" aria-label="Yenile">⟳</button>
        <input id="address" spellcheck="false" placeholder="Adres" aria-label="Adres" />
      </div>`;
    const input = root.getElementById("address");
    root.getElementById("back").addEventListener("click", () => history.back());
    root.getElementById("forward").addEventListener("click", () => history.forward());
    root.getElementById("reload").addEventListener("click", () => location.reload());
    input.addEventListener("keydown", (e) => {
      if (e.key !== "Enter") return;
      const value = input.value.trim();
      if (!value) return;
      location.assign(/^[a-z][a-z0-9+.-]*:/i.test(value) ? value : "https://" + value);
    });
    const sync = () => { input.value = location.href; };
    sync();
    addEventListener("load", sync);
    // `pageshow` also fires when a page is restored from the back/forward cache, where `load`
    // does not run again.
    addEventListener("pageshow", sync);
    addEventListener("popstate", sync);
    addEventListener("hashchange", sync);
    parent.appendChild(host);
  };
  if (document.readyState === "loading") addEventListener("DOMContentLoaded", build);
  else build();
})();
"##;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Mutex, MutexGuard};

    use gamelib_core::app::{App, EventSink, JobOptions};
    use gamelib_core::model::{DownloadSourceKind, SettingsPatch};
    use tauri::Url;

    use super::*;

    /// The window's context is global, so tests that read or write it take turns.
    static CONTEXT_TEST: Mutex<()> = Mutex::new(());

    fn context_test() -> MutexGuard<'static, ()> {
        CONTEXT_TEST.lock().unwrap_or_else(|e| e.into_inner())
    }

    struct Sink;

    impl EventSink for Sink {
        fn emit(&self, _event: &str, _payload: serde_json::Value) {}
    }

    /// An app with a library of its own, so queued rows land in the temp folder.
    fn test_app(tag: &str) -> App {
        let dir =
            std::env::temp_dir().join(format!("gamelib-browser-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let library = dir.join("Games");
        std::fs::create_dir_all(&library).unwrap();
        let app = App::with_options(
            dir.join("gamelib.db"),
            Arc::new(Sink),
            JobOptions::default(),
        )
        .unwrap();
        app.update_settings(&SettingsPatch {
            library_dir: Some(library.display().to_string()),
            ..Default::default()
        })
        .unwrap();
        app
    }

    /// A download request the way the webview reports it: the URL and the name the browser
    /// suggests for it (from `Content-Disposition`).
    fn download_request(url: &str, file_name: &str) -> (Url, PathBuf) {
        (
            Url::parse(url).unwrap(),
            PathBuf::from("C:\\Users\\oyuncu\\Downloads").join(file_name),
        )
    }

    #[test]
    fn pages_load_and_magnet_links_become_torrents() {
        let _guard = context_test();
        let app = test_app("navigate");
        set_current(730, "Counter-Strike 2".into());
        let magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=CS2";

        assert!(navigate(
            &app,
            &Url::parse("https://example.com/page").unwrap()
        ));
        assert!(navigate(
            &app,
            &Url::parse("http://example.com/file.zip").unwrap()
        ));
        assert!(
            !navigate(&app, &Url::parse(magnet).unwrap()),
            "a magnet link must not become a page"
        );
        assert!(!navigate(&app, &Url::parse("file:///etc/passwd").unwrap()));
        assert!(!navigate(&app, &Url::parse("steam://store/730").unwrap()));

        let queued = app.downloads().unwrap().items;
        assert_eq!(queued.len(), 1, "only the magnet is queued");
        assert_eq!(queued[0].source_kind, DownloadSourceKind::Torrent);
        assert_eq!(queued[0].title, "Counter-Strike 2");
        assert_eq!(queued[0].appid, Some(730));
        assert_eq!(
            queued[0].option_id,
            "0123456789abcdef0123456789abcdef01234567"
        );
    }

    #[test]
    fn torrents_and_files_are_queued_instead_of_saved() {
        let _guard = context_test();
        let app = test_app("downloads");
        set_current(570, "Dota 2".into());

        let (url, mut destination) =
            download_request("https://example.com/game.torrent", "game.TORRENT");
        let event = DownloadEvent::Requested {
            url,
            destination: &mut destination,
        };
        assert!(
            !on_download(&app, event),
            "the webview must not save it itself"
        );

        let (url, mut destination) =
            download_request("https://example.com/redirect", "dota_setup.zip");
        let event = DownloadEvent::Requested {
            url,
            destination: &mut destination,
        };
        assert!(!on_download(&app, event));
        assert!(
            on_download(
                &app,
                DownloadEvent::Finished {
                    url: Url::parse("https://example.com/redirect").unwrap(),
                    path: None,
                    success: true,
                }
            ),
            "finishing must not be cancelled"
        );

        let items = app.downloads().unwrap().items;
        assert_eq!(items.len(), 2);
        let torrent = items
            .iter()
            .find(|d| d.source_kind == DownloadSourceKind::Torrent)
            .unwrap();
        assert_eq!(torrent.option_id, "https://example.com/game.torrent");
        // A file that is not a torrent stays an HTTP download, under the browser's own name.
        let captured = items
            .iter()
            .find(|d| d.source_kind == DownloadSourceKind::Http)
            .unwrap();
        assert_eq!(captured.option_id, "dota_setup.zip");
        assert_eq!(captured.title, "Dota 2");
    }

    #[test]
    fn the_window_follows_the_game_it_was_opened_for() {
        let _guard = context_test();
        set_current(730, "Counter-Strike 2".into());
        assert_eq!(current_game(), (730, "Counter-Strike 2".to_string()));
        // The window is reused: the next game's downloads must be queued for that game.
        set_current(570, "Dota 2".into());
        assert_eq!(current_game(), (570, "Dota 2".to_string()));
        assert_eq!(page_title(""), TITLE);
        assert_eq!(page_title("   "), TITLE);
        assert_eq!(page_title("FitGirl"), "GameLib · Tarayıcı · FitGirl");
        assert!(is_torrent_file("Game.TORRENT"));
        assert!(!is_torrent_file("game.torrent.zip"));
    }
}
