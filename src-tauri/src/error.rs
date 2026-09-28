use gamelib_core::{Error, ErrorKind};
use serde::Serialize;

/// Error returned to the frontend as `{ kind, message }`; the UI shows a Turkish text per kind.
#[derive(Debug, Clone, Serialize)]
pub struct CmdError {
    pub kind: ErrorKind,
    /// For `invalid` errors: a stable code such as `url_scheme`. Otherwise a log-style message.
    pub message: String,
}

pub type CmdResult<T> = Result<T, CmdError>;

impl CmdError {
    pub fn busy() -> Self {
        Self {
            kind: ErrorKind::Busy,
            message: "a catalog download is already running".into(),
        }
    }

    pub fn other(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Other,
            message: message.into(),
        }
    }
}

impl From<Error> for CmdError {
    fn from(e: Error) -> Self {
        let message = match &e {
            Error::Invalid(code) => (*code).to_owned(),
            other => other.to_string(),
        };
        Self {
            kind: e.kind(),
            message,
        }
    }
}

impl From<tauri::Error> for CmdError {
    fn from(e: tauri::Error) -> Self {
        Self::other(e.to_string())
    }
}
