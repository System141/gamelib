use serde::Serialize;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors surfaced by the core. Messages are for logs; the UI translates [`ErrorKind`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network error: {0}")]
    Network(String),
    #[error("request timed out: {0}")]
    Timeout(String),
    #[error("rate limited by the server")]
    RateLimited,
    #[error("HTTP {status} from {url}")]
    Http { status: u16, url: String },
    #[error("could not parse response: {0}")]
    Parse(String),
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("cancelled")]
    Cancelled,
    /// Invalid user input. The payload is a stable code (e.g. `url_scheme`) the UI maps to text.
    #[error("invalid input: {0}")]
    Invalid(&'static str),
    /// A failure the UI explains by its stable code, with a detail such as an exit code. Reaches
    /// the UI as `invalid` with the message `code:detail`.
    #[error("{0}: {1}")]
    Failed(&'static str, String),
    #[error("not found")]
    NotFound,
    /// A catalog job is already running (in this process or another one using the same database).
    #[error("another catalog job is running")]
    Busy,
    #[error("{0}")]
    Other(String),
}

/// Stable, serializable error category shared with the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Network,
    Timeout,
    RateLimited,
    Http,
    Parse,
    Database,
    Cancelled,
    Invalid,
    NotFound,
    Busy,
    Other,
}

impl Error {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Error::Network(_) => ErrorKind::Network,
            Error::Timeout(_) => ErrorKind::Timeout,
            Error::RateLimited => ErrorKind::RateLimited,
            Error::Http { .. } => ErrorKind::Http,
            Error::Parse(_) => ErrorKind::Parse,
            Error::Database(_) => ErrorKind::Database,
            Error::Cancelled => ErrorKind::Cancelled,
            Error::Invalid(_) | Error::Failed(..) => ErrorKind::Invalid,
            Error::NotFound => ErrorKind::NotFound,
            Error::Busy => ErrorKind::Busy,
            Error::Other(_) => ErrorKind::Other,
        }
    }

    /// Whether a failed request is worth retrying.
    pub fn is_transient(&self) -> bool {
        match self {
            Error::Network(_) | Error::Timeout(_) | Error::RateLimited => true,
            Error::Http { status, .. } => *status >= 500,
            _ => false,
        }
    }
}

/// An error as the frontend receives it: `{ kind, message }`. The UI shows a Turkish text per
/// kind, so the message is only for logs, except for `invalid` where it is the stable code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct ErrorInfo {
    pub kind: ErrorKind,
    pub message: String,
}

impl ErrorInfo {
    pub fn other(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Other,
            message: message.into(),
        }
    }
}

impl From<Error> for ErrorInfo {
    fn from(e: Error) -> Self {
        let message = match &e {
            Error::Invalid(code) => (*code).to_owned(),
            Error::Failed(code, detail) => format!("{code}:{detail}"),
            other => other.to_string(),
        };
        Self {
            kind: e.kind(),
            message,
        }
    }
}

impl std::fmt::Display for ErrorInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for ErrorInfo {}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        let url = e.url().map(|u| u.to_string()).unwrap_or_default();
        if e.is_timeout() {
            Error::Timeout(url)
        } else if let Some(status) = e.status() {
            Error::Http {
                status: status.as_u16(),
                url,
            }
        } else if e.is_decode() {
            // A body cut off mid-transfer shows up as a decode error; treat it as transient.
            Error::Network(format!("incomplete response from {url}: {e}"))
        } else {
            Error::Network(error_chain(&e))
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Parse(e.to_string())
    }
}

/// Flattens an error and its sources into one line (reqwest hides the useful part in sources).
pub fn error_chain(e: &dyn std::error::Error) -> String {
    let mut out = e.to_string();
    let mut source = e.source();
    while let Some(s) = source {
        let text = s.to_string();
        if !out.contains(&text) {
            out.push_str(": ");
            out.push_str(&text);
        }
        source = s.source();
    }
    out
}
