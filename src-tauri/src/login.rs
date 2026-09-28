//! GOG sign-in in its own window.
//!
//! The window shows GOG's own login page (remote content: it has no capabilities, so it cannot
//! reach the app's commands) and is incognito, so no GOG cookies stay behind. When GOG
//! redirects to `embed.gog.com/on_login_success?…&code=…` the navigation is stopped, the window
//! closes and the code is exchanged for tokens in the core.

use std::sync::mpsc;

use gamelib_core::stores::gog_account::REDIRECT_PREFIX;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use crate::error::{CmdError, CmdResult};

const LABEL: &str = "gog-login";

/// Shows the sign-in page and waits for the redirect. `None` if the user closed the window.
pub async fn gog_redirect(handle: &AppHandle, login_url: &str) -> CmdResult<Option<String>> {
    if let Some(open) = handle.get_webview_window(LABEL) {
        let _ = open.set_focus();
        return Err(gamelib_core::Error::Busy.into());
    }
    let url = login_url
        .parse()
        .map_err(|e| CmdError::other(format!("login url: {e}")))?;
    let (tx, rx) = mpsc::channel::<Option<String>>();
    let redirect_tx = tx.clone();
    let window = WebviewWindowBuilder::new(handle, LABEL, WebviewUrl::External(url))
        .title("GOG · Giriş")
        .inner_size(480.0, 760.0)
        .min_inner_size(400.0, 560.0)
        .center()
        .incognito(true)
        .on_navigation(move |url| {
            if url.as_str().starts_with(REDIRECT_PREFIX) {
                let _ = redirect_tx.send(Some(url.to_string()));
                return false;
            }
            true
        })
        .build()
        .map_err(|e| CmdError::other(format!("login window: {e}")))?;
    window.on_window_event(move |event| {
        if let WindowEvent::Destroyed = event {
            let _ = tx.send(None);
        }
    });
    let received = tauri::async_runtime::spawn_blocking(move || rx.recv().ok().flatten())
        .await
        .map_err(|e| CmdError::other(e.to_string()))?;
    let _ = window.destroy();
    Ok(received)
}
