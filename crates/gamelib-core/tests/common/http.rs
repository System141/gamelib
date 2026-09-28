//! A tiny HTTP/1.1 server for integration tests: each request goes to a handler closure.
#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    /// Header names in lower case.
    pub headers: HashMap<String, String>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.query.get(name).map(String::as_str)
    }
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// Sends the body in pieces of this size with a pause after each (a slow server).
    pub pace: Option<(usize, Duration)>,
    /// Closes the connection after this many body bytes (a dropped transfer).
    pub cut_at: Option<usize>,
}

impl Response {
    pub fn json(body: impl Into<String>) -> Self {
        Self {
            status: 200,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: body.into().into_bytes(),
            pace: None,
            cut_at: None,
        }
    }

    pub fn status(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
            pace: None,
            cut_at: None,
        }
    }

    pub fn with_header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    pub fn paced(mut self, chunk: usize, pause: Duration) -> Self {
        self.pace = Some((chunk, pause));
        self
    }

    pub fn cut_at(mut self, bytes: usize) -> Self {
        self.cut_at = Some(bytes);
        self
    }
}

pub struct TestServer {
    pub base: String,
    log: Arc<Mutex<Vec<Request>>>,
}

impl TestServer {
    /// Starts a server on a free local port; every connection is handled on its own thread.
    pub fn start(handler: impl Fn(&Request) -> Response + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handler = Arc::new(handler);
        let log: Arc<Mutex<Vec<Request>>> = Arc::default();
        let server_log = log.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let handler = handler.clone();
                let log = server_log.clone();
                std::thread::spawn(move || serve(stream, &*handler, &log));
            }
        });
        Self { base, log }
    }

    /// Requests received so far.
    pub fn requests(&self) -> Vec<Request> {
        self.log.lock().unwrap().clone()
    }

    pub fn count(&self, path_prefix: &str) -> usize {
        self.requests()
            .iter()
            .filter(|r| r.path.starts_with(path_prefix))
            .count()
    }
}

fn serve(mut stream: TcpStream, handler: &dyn Fn(&Request) -> Response, log: &Mutex<Vec<Request>>) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let text = String::from_utf8_lossy(&buf).to_string();
    let mut lines = text.split("\r\n");
    let mut first = lines.next().unwrap_or_default().split_whitespace();
    let method = first.next().unwrap_or_default().to_owned();
    let target = first.next().unwrap_or("/").to_owned();
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_owned(), parse_query(q)),
        None => (target.clone(), HashMap::new()),
    };
    let headers = lines
        .take_while(|l| !l.is_empty())
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let request = Request {
        method,
        path,
        query,
        headers,
    };
    log.lock().unwrap().push(request.clone());
    let response = handler(&request);

    let mut head = format!("HTTP/1.1 {} X\r\nConnection: close\r\n", response.status);
    for (k, v) in &response.headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    if !response
        .headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-length"))
    {
        head.push_str(&format!("Content-Length: {}\r\n", response.body.len()));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    if request.method == "HEAD" {
        return;
    }
    let body = match response.cut_at {
        Some(n) => &response.body[..n.min(response.body.len())],
        None => &response.body[..],
    };
    match response.pace {
        Some((chunk, pause)) => {
            for piece in body.chunks(chunk.max(1)) {
                if stream.write_all(piece).is_err() {
                    return;
                }
                let _ = stream.flush();
                std::thread::sleep(pause);
            }
        }
        None => {
            let _ = stream.write_all(body);
        }
    }
    let _ = stream.flush();
    if response.cut_at.is_some() {
        let _ = stream.shutdown(std::net::Shutdown::Both);
    }
}

fn parse_query(q: &str) -> HashMap<String, String> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (decode(k), decode(v)),
            None => (decode(p), String::new()),
        })
        .collect()
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
