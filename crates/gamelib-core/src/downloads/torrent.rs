//! BitTorrent transfers (a magnet link or a `.torrent` address), downloaded with librqbit.
//!
//! This is the only asynchronous part of the core: a librqbit session needs a Tokio runtime, so
//! one is created for the transfer and dropped with it. The queue itself stays synchronous and
//! calls [`run`] from its own thread.
//!
//! The session lives for one transfer only. Everything it writes goes to the download's own
//! folder, so a paused or interrupted transfer is re-queued with the same source and continues
//! from the bytes already on disk (`overwrite` lets librqbit pick up what it finds there).

use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, Session, SessionOptions,
    ValidatedTorrentMetaV1Info,
};

use super::names;
use crate::{Error, Result, check_cancel};

/// How often the cancel flag is looked at while the transfer runs.
const CANCEL_POLL: Duration = Duration::from_millis(100);
/// How often progress is reported.
const REPORT_EVERY: Duration = Duration::from_millis(250);

/// Where a running transfer reports what it has done (the queue's progress tracker).
pub trait Progress {
    /// Total bytes, once the metadata has been read.
    fn set_total(&mut self, total: u64);
    /// Bytes downloaded so far.
    fn bytes(&mut self, done: u64);
}

/// How a transfer looks for peers. The app uses the swarm; a test connects to a local peer only.
#[derive(Debug, Clone)]
pub struct Options {
    /// Ask trackers and the DHT for peers, and accept incoming connections.
    pub swarm: bool,
    /// Peers to connect to first.
    pub initial_peers: Vec<SocketAddr>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            swarm: true,
            initial_peers: Vec::new(),
        }
    }
}

/// Downloads `source` (a magnet link or a `.torrent` address) into `output_dir`.
///
/// Returns `Error::Cancelled` when `cancel` is set; the bytes already written stay, so running
/// the same transfer again continues where this one stopped.
pub fn run(
    source: &str,
    output_dir: &Path,
    cancel: &AtomicBool,
    opts: &Options,
    progress: &mut dyn Progress,
) -> Result<()> {
    check_cancel(cancel)?;
    std::fs::create_dir_all(output_dir)
        .map_err(|e| Error::Other(format!("{}: {e}", output_dir.display())))?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| Error::Other(format!("could not start the torrent runtime: {e}")))?;
    let result = runtime.block_on(transfer(source, output_dir, cancel, opts, progress));
    // Dropping the runtime shuts the session's tasks down, closing its files before the caller
    // can delete the folder (Windows refuses to delete open files).
    drop(runtime);
    result
}

async fn transfer(
    source: &str,
    output_dir: &Path,
    cancel: &AtomicBool,
    opts: &Options,
    progress: &mut dyn Progress,
) -> Result<()> {
    let session = Session::new_with_opts(output_dir.to_path_buf(), session_options(opts))
        .await
        .map_err(broken_torrent)?;

    // A magnet has no metadata yet: `list_only` asks the swarm for it (and only it) first, so a
    // torrent with an unusable file list is refused before anything is written.
    let listed = session
        .add_torrent(
            AddTorrent::from_url(source),
            Some(AddTorrentOptions {
                list_only: true,
                disable_trackers: !opts.swarm,
                initial_peers: peers(opts),
                ..Default::default()
            }),
        )
        .await
        .map_err(broken_torrent)?;
    let AddTorrentResponse::ListOnly(listed) = listed else {
        return Err(Error::Invalid("torrent_parse"));
    };
    check_paths(&listed.info)?;
    progress.set_total(listed.info.lengths().total_length());

    let added = session
        .add_torrent(
            // The bytes just read, so the metadata is not fetched twice.
            AddTorrent::from_bytes(listed.torrent_bytes),
            Some(AddTorrentOptions {
                output_folder: Some(output_dir.to_string_lossy().into_owned()),
                // Resume (or finish) whatever the folder already holds.
                overwrite: true,
                disable_trackers: !opts.swarm,
                initial_peers: peers(opts),
                ..Default::default()
            }),
        )
        .await
        .map_err(broken_torrent)?;
    let handle = added.into_handle().ok_or(Error::Invalid("torrent_parse"))?;

    let finished = tokio::spawn({
        let handle = handle.clone();
        async move { handle.wait_until_completed().await }
    });
    let mut finished = finished;
    let mut last_report = std::time::Instant::now();
    let outcome = loop {
        tokio::select! {
            result = &mut finished => break Ok(result),
            () = tokio::time::sleep(CANCEL_POLL) => {
                if cancel.load(Ordering::Relaxed) {
                    break Err(());
                }
                if last_report.elapsed() >= REPORT_EVERY {
                    progress.bytes(handle.stats().progress_bytes);
                    last_report = std::time::Instant::now();
                }
            }
        }
    };
    match outcome {
        Ok(result) => {
            result.map_err(broken_torrent)?.map_err(broken_torrent)?;
            progress.bytes(handle.stats().progress_bytes);
            Ok(())
        }
        Err(()) => {
            // Stop feeding the swarm, then let `run` shut the session down.
            let _ = session.pause(&handle).await;
            Err(Error::Cancelled)
        }
    }
}

/// The session for one transfer: the app's transfers use the swarm, a test only its own peers.
fn session_options(opts: &Options) -> SessionOptions {
    if opts.swarm {
        SessionOptions::default()
    } else {
        SessionOptions {
            dht: None,
            listen: None,
            disable_trackers: true,
            disable_local_service_discovery: true,
            ..Default::default()
        }
    }
}

fn peers(opts: &Options) -> Option<Vec<SocketAddr>> {
    (!opts.initial_peers.is_empty()).then(|| opts.initial_peers.clone())
}

/// Refuses metadata whose files could be written outside the download folder.
///
/// This is the same boundary archive extraction draws: every path component must be one
/// Windows accepts unchanged, so `..`, `a:b`, `NUL`, a leading `/` or a `C:` drive prefix are
/// all rejected instead of being silently rewritten.
fn check_paths(info: &ValidatedTorrentMetaV1Info<librqbit::ByteBufOwned>) -> Result<()> {
    for file in info.iter_file_details() {
        for component in file.filename.to_vec() {
            if component.is_empty() || names::safe_name(&component, "") != component {
                return Err(Error::Invalid("torrent_path"));
            }
        }
    }
    Ok(())
}

/// A librqbit failure; the UI shows the kind, so only the log sees the message.
fn broken_torrent(e: impl std::fmt::Display) -> Error {
    Error::Network(format!("torrent: {e}"))
}
