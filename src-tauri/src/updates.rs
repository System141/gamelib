//! App updates from this repository's GitHub releases (`latest.json`), each verified with the
//! public key its release was signed with. Without a public key in the configuration (set when
//! a signed release is built), the updater stays off and says so.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use gamelib_core::app::{App, release_page};
use gamelib_core::{Error, ErrorInfo, ErrorKind, unix_now};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::error::{CmdError, CmdResult};

/// Download progress of an update: `{downloaded, total}`, at most every `PROGRESS_EVERY`.
const EVENT_PROGRESS: &str = "update:progress";
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// The last check: when it ran and the update it found, kept for installing it (again, if a
/// try failed).
#[derive(Default)]
pub struct PendingUpdate {
    last: Mutex<LastCheck>,
    installing: AtomicBool,
}

#[derive(Default)]
struct LastCheck {
    at: Option<i64>,
    found: Option<Update>,
}

impl PendingUpdate {
    fn last(&self) -> MutexGuard<'_, LastCheck> {
        self.last.lock().unwrap_or_else(|p| p.into_inner())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    version: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// Whether this build can update itself (it knows the releases' public key).
    configured: bool,
    current_version: String,
    /// When GitHub was last asked (unix seconds), since the app started.
    checked_at: Option<i64>,
    update: Option<UpdateInfo>,
}

/// Whether the updater has a public key to verify releases with.
fn configured(app: &AppHandle) -> bool {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("pubkey"))
        .and_then(|k| k.as_str())
        .is_some_and(|k| !k.trim().is_empty())
}

fn status(app: &AppHandle, configured: bool, last: &LastCheck) -> UpdateStatus {
    UpdateStatus {
        configured,
        current_version: app.package_info().version.to_string(),
        checked_at: last.at,
        update: last.found.as_ref().map(|u| UpdateInfo {
            version: u.version.clone(),
        }),
    }
}

/// What the user is told when checking or updating fails.
fn update_error(e: tauri_plugin_updater::Error) -> CmdError {
    use tauri_plugin_updater::Error as U;
    match e {
        U::Reqwest(_) | U::Network(_) | U::Http(_) => ErrorInfo {
            kind: ErrorKind::Network,
            message: e.to_string(),
        },
        // Not signed with the key built into this app, or signed for another version.
        U::Minisign(_)
        | U::Base64(_)
        | U::SignatureUtf8(_)
        | U::SignedVersionMismatch { .. }
        | U::MissingSignedVersion => Error::Invalid("update_signature").into(),
        // No latest.json in the latest release (or no release at all).
        U::ReleaseNotFound => Error::Invalid("update_missing").into(),
        // The release has nothing for this system and package type.
        U::TargetNotFound(_) | U::TargetsNotFound(_) => Error::Invalid("update_platform").into(),
        U::AuthenticationFailed => Error::Cancelled.into(),
        other => Error::Failed("update_failed", other.to_string()).into(),
    }
}

/// The version and what the last check found, without asking GitHub.
#[tauri::command]
pub fn get_update_status(app: AppHandle, pending: State<'_, PendingUpdate>) -> UpdateStatus {
    status(&app, configured(&app), &pending.last())
}

/// Looks for a newer release.
#[tauri::command]
pub async fn check_update(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> CmdResult<UpdateStatus> {
    if !configured(&app) {
        return Ok(status(&app, false, &LastCheck::default()));
    }
    let found = app
        .updater()
        .map_err(update_error)?
        .check()
        .await
        .map_err(update_error)?;
    let mut last = pending.last();
    *last = LastCheck {
        at: Some(unix_now()),
        found,
    };
    Ok(status(&app, true, &last))
}

/// Clears the "installing" mark however the install ends.
struct Installing<'a>(&'a AtomicBool);

impl Drop for Installing<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Downloads and installs the update found by the last check, then restarts. On Windows the
/// installer closes the app itself and starts the new version when it is done. Refused while a
/// game is being installed: closing the app would leave that install half done.
#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    games: State<'_, Arc<App>>,
    pending: State<'_, PendingUpdate>,
) -> CmdResult<()> {
    if games.install_running() {
        return Err(Error::Invalid("install_running").into());
    }
    if pending.installing.swap(true, Ordering::SeqCst) {
        return Err(Error::Busy.into());
    }
    let _installing = Installing(&pending.installing);
    let update = pending.last().found.clone().ok_or(Error::NotFound)?;
    let emitter = app.clone();
    let mut downloaded: u64 = 0;
    let mut emitted: Option<Instant> = None;
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let done = total.is_some_and(|t| downloaded >= t);
                if done || emitted.is_none_or(|at| at.elapsed() >= PROGRESS_EVERY) {
                    emitted = Some(Instant::now());
                    let _ = emitter.emit(
                        EVENT_PROGRESS,
                        serde_json::json!({ "downloaded": downloaded, "total": total }),
                    );
                }
            },
            || {},
        )
        .await
        .map_err(update_error)?;
    app.restart()
}

/// Opens a release's page (what is new), or the list of releases.
#[tauri::command]
pub fn open_release_page(handle: AppHandle, version: Option<String>) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let url = release_page(version.as_deref())?;
    handle
        .opener()
        .open_url(url, None::<&str>)
        .map_err(|e| CmdError::other(e.to_string()))
}
