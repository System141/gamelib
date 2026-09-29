//! `gamelib-cli serve`: the desktop app's commands over local HTTP, for the browser preview.
//!
//! `pnpm dev` proxies `/api` to this server (see `vite.config.ts`) and `src/mocks/server.ts`
//! forwards the UI's `invoke` calls as `POST /api/<command>` with the same JSON arguments.
//! Catalog job events stream from `GET /api/events` as server-sent events.
//!
//! Only this computer can use it: the server listens on 127.0.0.1 and rejects requests whose
//! `Host` or `Origin` is not a loopback name (DNS rebinding, other web pages), posts that are not
//! JSON (cross-site forms) and bodies over 1 MiB. It sends no CORS headers.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use gamelib_core::app::account_page;
use gamelib_core::app::{App, EventSink, release_page, search_url, steam_url};
use gamelib_core::model::{
    GameQuery, LinkInput, MatchState, OpenTarget, SearchSite, SettingsPatch, Store,
};
use gamelib_core::{Error, ErrorInfo, ErrorKind, Result};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tiny_http::{Header, Method, Request, Response, Server};

pub const DEFAULT_PORT: u16 = 1430;
const MAX_BODY: usize = 1 << 20;
/// Keeps idle event streams (and the dev proxy in front of them) alive.
const PING_EVERY: Duration = Duration::from_secs(15);
/// Events queued per browser tab; a tab that stops reading misses the rest.
const EVENT_BACKLOG: usize = 64;

pub fn run(db_path: &Path, port: u16) -> Result<()> {
    let bus = Arc::new(EventBus::default());
    let app = Arc::new(App::open(db_path, bus.clone())?);
    let server = Server::http(("127.0.0.1", port))
        .map_err(|e| Error::Other(format!("cannot listen on 127.0.0.1:{port}: {e}")))?;
    let games = app.status()?.catalog.game_count;
    eprintln!(
        "GameLib API: http://127.0.0.1:{port}/api  ({games} games in {})",
        db_path.display()
    );
    eprintln!("Browser preview: run `pnpm dev` and open http://localhost:1420. Stop with Ctrl+C.");
    serve(&server, &app, &bus);
    Ok(())
}

/// Answers requests, each on its own thread, until the server is unblocked.
fn serve(server: &Server, app: &Arc<App>, bus: &Arc<EventBus>) {
    for request in server.incoming_requests() {
        let app = app.clone();
        let bus = bus.clone();
        // If the thread cannot start, dropping the request answers 500.
        let _ = std::thread::Builder::new()
            .name("gamelib-http".into())
            .spawn(move || handle(request, &app, &bus));
    }
}

fn handle(mut request: Request, app: &App, bus: &EventBus) {
    if !is_local(&request) {
        return reply(request, 403, &error(ErrorKind::Invalid, "forbidden"));
    }
    let path = request
        .url()
        .split('?')
        .next()
        .unwrap_or_default()
        .to_owned();
    let Some(command) = path.strip_prefix("/api/") else {
        return reply(request, 404, &error(ErrorKind::NotFound, "not_found"));
    };
    let method = request.method().clone();
    match (method, command) {
        (Method::Get, "health") => {
            let body = json!({
                "ok": true,
                "version": env!("CARGO_PKG_VERSION"),
                "dbPath": app.db_path().display().to_string(),
            });
            reply(request, 200, &body);
        }
        (Method::Get, "events") => stream_events(request, bus),
        (Method::Post, _) => {
            let (status, body) = call(&mut request, app, command);
            reply(request, status, &body);
        }
        _ => reply(request, 405, &error(ErrorKind::Invalid, "method")),
    }
}

/// Runs one command. Arguments and results are the JSON the Tauri `invoke` calls use.
fn call(request: &mut Request, app: &App, command: &str) -> (u16, Value) {
    if !header(request, "Content-Type").is_some_and(is_json) {
        return (415, error(ErrorKind::Invalid, "content_type"));
    }
    if request.body_length().is_some_and(|n| n > MAX_BODY) {
        return (413, error(ErrorKind::Invalid, "too_large"));
    }
    let mut body = Vec::new();
    if request
        .as_reader()
        .take(MAX_BODY as u64 + 1)
        .read_to_end(&mut body)
        .is_err()
    {
        return (400, error(ErrorKind::Invalid, "body"));
    }
    if body.len() > MAX_BODY {
        return (413, error(ErrorKind::Invalid, "too_large"));
    }
    let args = if body.iter().all(u8::is_ascii_whitespace) {
        json!({})
    } else {
        match serde_json::from_slice(&body) {
            Ok(args) => args,
            Err(_) => return (400, error(ErrorKind::Invalid, "json")),
        }
    };
    match dispatch(app, command, args) {
        Ok(value) => (200, value),
        Err(e) => (http_status(e.kind), to_json(&e)),
    }
}

#[derive(Deserialize)]
struct FreshArgs {
    fresh: Option<bool>,
}

#[derive(Deserialize)]
struct DaysArgs {
    days: Option<u32>,
}

#[derive(Deserialize)]
struct QueryArgs {
    params: GameQuery,
}

#[derive(Deserialize)]
struct AppidArgs {
    appid: u32,
}

#[derive(Deserialize)]
struct LinkArgs {
    input: LinkInput,
}

#[derive(Deserialize)]
struct IdArgs {
    id: i64,
}

#[derive(Deserialize)]
struct SteamArgs {
    appid: u32,
    target: OpenTarget,
}

#[derive(Deserialize)]
struct SearchArgs {
    site: SearchSite,
    query: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MatchStateArgs {
    store: Store,
    product_id: String,
    appid: u32,
    state: MatchState,
}

#[derive(Deserialize)]
struct VersionArgs {
    version: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProductArgs {
    store: Store,
    product_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinkProductArgs {
    store: Store,
    product_id: String,
    appid: u32,
}

#[derive(Deserialize)]
struct StoreAppidArgs {
    store: Store,
    appid: u32,
}

#[derive(Deserialize)]
struct StoreArgs {
    store: Store,
}

#[derive(Deserialize)]
struct OptionalStoreArgs {
    store: Option<Store>,
}

#[derive(Deserialize)]
struct RedirectArgs {
    redirect: String,
}

#[derive(Deserialize)]
struct KeyArgs {
    key: String,
}

#[derive(Deserialize)]
struct PatchArgs {
    patch: SettingsPatch,
}

/// Commands that need the desktop app's windows or dialogs.
fn desktop_only() -> ErrorInfo {
    ErrorInfo {
        kind: ErrorKind::Invalid,
        message: "desktop_only".into(),
    }
}

fn dispatch(app: &App, command: &str, args: Value) -> std::result::Result<Value, ErrorInfo> {
    let value = match command {
        "get_status" => to_json(&app.status()?),
        "start_sync" => {
            let a: FreshArgs = parse(args)?;
            app.start_sync(a.fresh.unwrap_or(false))?;
            Value::Null
        }
        "fetch_new_releases" => {
            let a: DaysArgs = parse(args)?;
            app.fetch_new_releases(a.days)?;
            Value::Null
        }
        "cancel_sync" => {
            app.cancel_job();
            Value::Null
        }
        "query_games" => {
            let a: QueryArgs = parse(args)?;
            to_json(&app.query_games(&a.params)?)
        }
        "get_game" => to_json(&app.get_game(parse::<AppidArgs>(args)?.appid)?),
        "list_tags" => to_json(&app.list_tags()?),
        "get_game_media" => to_json(&app.game_media(parse::<AppidArgs>(args)?.appid)?),
        "get_game_reviews" => to_json(&app.game_reviews(parse::<AppidArgs>(args)?.appid)?),
        "start_store_sync" => {
            app.start_store_sync()?;
            Value::Null
        }
        "get_store_matches" => to_json(&app.store_matches(parse::<AppidArgs>(args)?.appid)?),
        "refresh_store_matches" => {
            to_json(&app.refresh_store_matches(parse::<AppidArgs>(args)?.appid)?)
        }
        "set_match_state" => {
            let a: MatchStateArgs = parse(args)?;
            app.set_match_state(a.store, &a.product_id, a.appid, a.state)?;
            Value::Null
        }
        "open_store_page" => {
            let a: ProductArgs = parse(args)?;
            json!({ "url": app.store_product_url(a.store, &a.product_id)? })
        }
        "search_store" => {
            let a: StoreAppidArgs = parse(args)?;
            to_json(&app.search_store(a.store, a.appid)?)
        }
        "link_store_product" => {
            let a: LinkProductArgs = parse(args)?;
            app.link_store_product(a.store, &a.product_id, a.appid)?;
            Value::Null
        }
        "get_accounts" => to_json(&app.accounts()?),
        "gog_login_url" => json!(app.gog_login_url()),
        "open_gog_login_page" => json!({ "url": app.gog_login_url() }),
        // The sign-in window is the desktop app's; the preview pastes the redirect instead.
        "gog_login" | "pick_library_dir" => return Err(desktop_only()),
        "gog_login_with_code" => {
            to_json(&app.gog_login_with_code(&parse::<RedirectArgs>(args)?.redirect)?)
        }
        "itch_set_key" => to_json(&app.itch_set_key(&parse::<KeyArgs>(args)?.key)?),
        "sign_out" => to_json(&app.sign_out(parse::<StoreArgs>(args)?.store)?),
        "start_library_sync" => {
            app.start_library_sync()?;
            Value::Null
        }
        "get_library" => to_json(&app.library(parse::<OptionalStoreArgs>(args)?.store)?),
        "open_account_page" => json!({ "url": account_page(parse::<StoreArgs>(args)?.store) }),
        "get_settings" => to_json(&app.settings()?),
        "update_settings" => to_json(&app.update_settings(&parse::<PatchArgs>(args)?.patch)?),
        "get_store_files" => {
            let a: ProductArgs = parse(args)?;
            to_json(&app.store_files(a.store, &a.product_id)?)
        }
        // The preview server cannot update itself; the desktop app can.
        "get_update_status" | "check_update" => json!({
            "configured": false,
            "currentVersion": env!("CARGO_PKG_VERSION"),
            "checkedAt": null,
            "update": null,
        }),
        "install_update" => return Err(desktop_only()),
        "open_release_page" => {
            let a: VersionArgs = parse(args)?;
            json!({ "url": release_page(a.version.as_deref())? })
        }
        "get_downloads" => to_json(&app.downloads()?),
        "get_installs" => to_json(&app.installs()?),
        // Only the desktop app downloads (the preview server never starts the queue).
        "enqueue_download"
        | "enqueue_torrent"
        | "pause_download"
        | "resume_download"
        | "remove_download"
        | "clear_finished_downloads"
        | "open_download_folder"
        | "approve_install"
        | "retry_install"
        | "launch_game"
        | "uninstall_game"
        | "open_install_folder"
        | "set_launch_target"
        | "pick_launch_target" => return Err(desktop_only()),
        "list_sites" => to_json(&app.list_sites()),
        "list_links" => to_json(&app.list_links(parse::<AppidArgs>(args)?.appid)?),
        "find_links" => to_json(&app.find_links(parse::<AppidArgs>(args)?.appid)?),
        "save_link" => to_json(&app.save_link(&parse::<LinkArgs>(args)?.input)?),
        "delete_link" => to_json(&app.delete_link(parse::<IdArgs>(args)?.id)?),
        "check_link" => to_json(&app.check_link(parse::<IdArgs>(args)?.id)?),
        // The browser opens these itself.
        "open_link" => json!({ "url": app.link_url(parse::<IdArgs>(args)?.id)? }),
        "open_in_steam" => {
            let a: SteamArgs = parse(args)?;
            json!({ "url": steam_url(a.appid, a.target) })
        }
        "open_search" => {
            let a: SearchArgs = parse(args)?;
            json!({ "url": search_url(a.site, &a.query)? })
        }
        _ => return Err(ErrorInfo::from(Error::NotFound)),
    };
    Ok(value)
}

fn parse<T: DeserializeOwned>(args: Value) -> std::result::Result<T, ErrorInfo> {
    serde_json::from_value(args).map_err(|e| ErrorInfo {
        kind: ErrorKind::Invalid,
        message: format!("arguments: {e}"),
    })
}

fn http_status(kind: ErrorKind) -> u16 {
    match kind {
        ErrorKind::Invalid => 400,
        ErrorKind::NotFound => 404,
        ErrorKind::Busy | ErrorKind::Cancelled => 409,
        ErrorKind::Network
        | ErrorKind::Timeout
        | ErrorKind::RateLimited
        | ErrorKind::Http
        | ErrorKind::Parse => 502,
        ErrorKind::Database | ErrorKind::Other => 500,
    }
}

fn error(kind: ErrorKind, message: &str) -> Value {
    to_json(&ErrorInfo {
        kind,
        message: message.to_owned(),
    })
}

fn to_json(value: &impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn reply(request: Request, status: u16, body: &Value) {
    let response = Response::from_string(body.to_string())
        .with_status_code(status)
        .with_header(make_header(
            "Content-Type",
            "application/json; charset=utf-8",
        ))
        .with_header(make_header("Cache-Control", "no-store"));
    let _ = request.respond(response);
}

fn make_header(name: &str, value: &str) -> Header {
    Header::from_bytes(name, value).expect("static header is valid")
}

fn header<'r>(request: &'r Request, name: &str) -> Option<&'r str> {
    request
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

fn is_json(content_type: &str) -> bool {
    content_type
        .split(';')
        .next()
        .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
}

/// The request comes from this computer's own pages: loopback `Host`, and a loopback `Origin`
/// when the browser sends one.
fn is_local(request: &Request) -> bool {
    let host = header(request, "Host").is_some_and(is_loopback_authority);
    let origin = header(request, "Origin").is_none_or(|origin| {
        origin
            .strip_prefix("http://")
            .or_else(|| origin.strip_prefix("https://"))
            .is_some_and(is_loopback_authority)
    });
    host && origin
}

/// `localhost`, `127.0.0.1` or `[::1]`, with or without a port.
fn is_loopback_authority(authority: &str) -> bool {
    let host = match authority.strip_prefix('[') {
        Some(rest) => rest.split_once(']').map_or("", |(host, _)| host),
        None => authority
            .split_once(':')
            .map_or(authority, |(host, _)| host),
    };
    host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "::1"
}

// --- events ---------------------------------------------------------------------------------

/// Fans job events out to every open `/api/events` stream.
#[derive(Default)]
pub struct EventBus {
    subscribers: Mutex<Vec<SyncSender<String>>>,
}

impl EventBus {
    fn subscribe(&self) -> Receiver<String> {
        let (tx, rx) = sync_channel(EVENT_BACKLOG);
        lock(&self.subscribers).push(tx);
        rx
    }
}

impl EventSink for EventBus {
    fn emit(&self, event: &str, payload: Value) {
        // Compact JSON has no raw newlines, so it fits one `data:` line.
        let frame = format!("event: {event}\ndata: {payload}\n\n");
        lock(&self.subscribers).retain(|tx| {
            !matches!(
                tx.try_send(frame.clone()),
                Err(TrySendError::Disconnected(_))
            )
        });
    }
}

/// Streams events until the browser goes away. tiny_http buffers normal responses, so the
/// stream writes the raw response itself and flushes every event.
fn stream_events(request: Request, bus: &EventBus) {
    let events = bus.subscribe();
    let mut out = request.into_writer();
    let mut send = |text: &str| {
        out.write_all(text.as_bytes())
            .and_then(|()| out.flush())
            .is_ok()
    };
    let head = "HTTP/1.1 200 OK\r\n\
                Content-Type: text/event-stream; charset=utf-8\r\n\
                Cache-Control: no-store\r\n\
                Connection: close\r\n\
                X-Accel-Buffering: no\r\n\r\n\
                retry: 3000\n\n";
    if !send(head) {
        return;
    }
    loop {
        let sent = match events.recv_timeout(PING_EVERY) {
            Ok(frame) => send(&frame),
            Err(RecvTimeoutError::Timeout) => send(": ping\n\n"),
            Err(RecvTimeoutError::Disconnected) => false,
        };
        if !sent {
            return;
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::net::{SocketAddr, TcpStream};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    struct TestServer {
        addr: SocketAddr,
        bus: Arc<EventBus>,
        server: Arc<Server>,
        dir: PathBuf,
    }

    impl TestServer {
        fn start() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let dir = std::env::temp_dir().join(format!(
                "gamelib-serve-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let bus = Arc::new(EventBus::default());
            let app = Arc::new(App::open(dir.join("gamelib.db"), bus.clone()).unwrap());
            let server = Arc::new(Server::http("127.0.0.1:0").unwrap());
            let addr = server.server_addr().to_ip().unwrap();
            let (thread_server, thread_bus) = (server.clone(), bus.clone());
            std::thread::spawn(move || serve(&thread_server, &app, &thread_bus));
            Self {
                addr,
                bus,
                server,
                dir,
            }
        }

        /// Sends a raw request and returns the status code and body.
        fn send(&self, raw: &str) -> (u16, String) {
            let mut stream = TcpStream::connect(self.addr).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            stream.write_all(raw.as_bytes()).unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            let status = response[9..12].parse().unwrap();
            let body = response
                .split_once("\r\n\r\n")
                .map(|(_, body)| body.to_owned())
                .unwrap_or_default();
            (status, body)
        }

        fn post(&self, command: &str, content_type: &str, body: &str) -> (u16, Value) {
            let (status, body) = self.send(&format!(
                "POST /api/{command} HTTP/1.1\r\nHost: localhost:1420\r\nOrigin: http://localhost:1420\r\n\
                 Content-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            ));
            (status, serde_json::from_str(&body).unwrap_or(Value::Null))
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            self.server.unblock();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn health() {
        let t = TestServer::start();
        let (status, body) =
            t.send("GET /api/health HTTP/1.1\r\nHost: 127.0.0.1:1430\r\nConnection: close\r\n\r\n");
        assert_eq!(status, 200);
        let body: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body["ok"], true);
        assert!(body["dbPath"].as_str().unwrap().ends_with("gamelib.db"));
    }

    #[test]
    fn rejects_foreign_hosts_and_origins() {
        let t = TestServer::start();
        let (status, _) = t.send(
            "GET /api/health HTTP/1.1\r\nHost: evil.example:1430\r\nConnection: close\r\n\r\n",
        );
        assert_eq!(status, 403);
        let (status, _) = t.send(
            "GET /api/health HTTP/1.1\r\nHost: localhost:1430\r\nOrigin: https://evil.example\r\nConnection: close\r\n\r\n",
        );
        assert_eq!(status, 403);
        let (status, _) = t.send("GET /api/health HTTP/1.1\r\nConnection: close\r\n\r\n");
        assert_eq!(status, 403);
        let (status, _) = t.send(
            "GET /api/health HTTP/1.1\r\nHost: [::1]:1430\r\nOrigin: http://127.0.0.1:1420\r\nConnection: close\r\n\r\n",
        );
        assert_eq!(status, 200);
    }

    #[test]
    fn commands_need_json() {
        let t = TestServer::start();
        let (status, _) = t.post("get_status", "text/plain", "{}");
        assert_eq!(status, 415);
        let (status, _) = t.post("get_status", "application/x-www-form-urlencoded", "a=1");
        assert_eq!(status, 415);
        let (status, body) = t.post("get_status", "application/json", "{nope");
        assert_eq!(status, 400);
        assert_eq!(body["kind"], "invalid");
        let big = format!("{{\"x\":\"{}\"}}", "a".repeat(MAX_BODY));
        let (status, _) = t.post("get_status", "application/json", &big);
        assert_eq!(status, 413);
    }

    #[test]
    fn runs_commands() {
        let t = TestServer::start();
        let (status, body) = t.post("get_status", "application/json; charset=utf-8", "");
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["gameCount"], 0);
        assert_eq!(body["worker"], Value::Null);

        let (status, body) = t.post(
            "query_games",
            "application/json",
            r#"{"params":{"search":"portal","sort":"relevance","limit":10}}"#,
        );
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["total"], 0);

        let (status, body) = t.post("get_game", "application/json", r#"{"appid":620}"#);
        assert_eq!(status, 200);
        assert_eq!(body, Value::Null);

        let (status, body) = t.post("get_game", "application/json", r#"{"appid":"x"}"#);
        assert_eq!(status, 400);
        assert_eq!(body["kind"], "invalid");

        let (status, body) = t.post("list_sites", "application/json", "{}");
        assert_eq!(status, 200);
        assert!(body.as_array().is_some_and(|sites| !sites.is_empty()));

        let (status, body) = t.post(
            "open_in_steam",
            "application/json",
            r#"{"appid":620,"target":"client"}"#,
        );
        assert_eq!(status, 200);
        assert_eq!(body["url"], "steam://store/620");

        let (status, body) = t.post(
            "open_search",
            "application/json",
            r#"{"site":"youtube","query":"Portal 2 gameplay"}"#,
        );
        assert_eq!(status, 200);
        assert_eq!(
            body["url"],
            "https://www.youtube.com/results?search_query=Portal+2+gameplay"
        );

        let (status, body) = t.post("open_link", "application/json", r#"{"id":99}"#);
        assert_eq!(status, 404);
        assert_eq!(body["kind"], "not_found");

        let (status, body) = t.post("drop_tables", "application/json", "{}");
        assert_eq!(status, 404);
        assert_eq!(body["kind"], "not_found");

        let (status, _) =
            t.send("GET /api/get_status HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
        assert_eq!(status, 405);
        let (status, _) =
            t.send("GET /index.html HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
        assert_eq!(status, 404);
    }

    #[test]
    fn streams_events() {
        let t = TestServer::start();
        let stream = TcpStream::connect(t.addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        (&stream)
            .write_all(b"GET /api/events HTTP/1.1\r\nHost: localhost:1420\r\n\r\n")
            .unwrap();
        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.starts_with("HTTP/1.1 200"), "{line}");
        // Headers, then the `retry:` field; after that the stream is subscribed.
        loop {
            line.clear();
            reader.read_line(&mut line).unwrap();
            if line.starts_with("retry:") {
                break;
            }
            if line.to_ascii_lowercase().starts_with("content-type") {
                assert!(line.contains("text/event-stream"));
            }
        }

        t.bus
            .emit("sync:progress", json!({ "fetched": 1000, "text": "a\nb" }));
        let mut frame = String::new();
        while !frame.ends_with("\n\n") || !frame.contains("data:") {
            reader.read_line(&mut frame).unwrap();
        }
        assert!(frame.contains("event: sync:progress\n"), "{frame}");
        let data = frame
            .lines()
            .find_map(|l| l.strip_prefix("data: "))
            .unwrap();
        let payload: Value = serde_json::from_str(data).unwrap();
        assert_eq!(payload["fetched"], 1000);
        assert_eq!(payload["text"], "a\nb");
    }

    #[test]
    fn loopback_names() {
        for ok in [
            "localhost",
            "LOCALHOST:1420",
            "127.0.0.1",
            "127.0.0.1:1430",
            "[::1]:80",
            "[::1]",
        ] {
            assert!(is_loopback_authority(ok), "{ok}");
        }
        for bad in [
            "evil.example",
            "localhost.evil.example",
            "127.0.0.1.evil",
            "10.0.0.2:1430",
            "[::2]",
            "",
        ] {
            assert!(!is_loopback_authority(bad), "{bad}");
        }
    }
}
