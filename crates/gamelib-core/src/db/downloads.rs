//! The download queue.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::downloads::sources::Source;
use crate::model::{Download, DownloadSourceKind, DownloadState, InstallState, Platform, Store};
use crate::{ErrorInfo, ErrorKind, Result};

const COLUMNS: &str =
    "d.id, d.store, d.product_id, d.appid, d.title, d.option_id, d.option_label, d.platform,
  d.state, d.total_bytes, d.done_bytes, d.dir, d.error_kind, d.error, d.created_at, d.finished_at,
  (SELECT COUNT(*) FROM download_files f WHERE f.download_id = d.id),
  d.install_state, d.install_kind, d.install_error_kind, d.install_error, d.source_kind";

/// A new queued download and its files (all pending).
pub struct NewDownload<'a> {
    pub store: Store,
    pub source_kind: DownloadSourceKind,
    pub product_id: &'a str,
    pub appid: Option<u32>,
    pub title: &'a str,
    pub option_id: &'a str,
    pub option_label: Option<&'a str>,
    pub platform: Option<Platform>,
    pub files: &'a [(Source, Option<u64>)],
}

pub fn insert(
    conn: &mut Connection,
    d: &NewDownload,
    dir_for: impl Fn(i64) -> String,
    now: i64,
) -> Result<i64> {
    let total: u64 = d.files.iter().filter_map(|(_, s)| *s).sum();
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO downloads(store, product_id, appid, title, option_id, option_label, platform,
           state, total_bytes, created_at, updated_at, source_kind)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'queued', ?8, ?9, ?9, ?10)",
        params![
            d.store.as_str(),
            d.product_id,
            d.appid,
            d.title,
            d.option_id,
            d.option_label,
            d.platform.map(Platform::as_str),
            total as i64,
            now,
            d.source_kind.as_str()
        ],
    )?;
    let id = tx.last_insert_rowid();
    tx.execute(
        "UPDATE downloads SET dir = ?2 WHERE id = ?1",
        params![id, dir_for(id)],
    )?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO download_files(download_id, position, source, size) VALUES (?1, ?2, ?3, ?4)",
        )?;
        for (i, (source, size)) in d.files.iter().enumerate() {
            stmt.execute(params![
                id,
                i as i64,
                source.to_db(),
                size.map(|s| s as i64)
            ])?;
        }
    }
    tx.commit()?;
    Ok(id)
}

pub fn get(conn: &Connection, id: i64) -> Result<Option<Download>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM downloads d WHERE d.id = ?1"),
            params![id],
            row,
        )
        .optional()?)
}

/// Every download, newest first.
pub fn list(conn: &Connection) -> Result<Vec<Download>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM downloads d ORDER BY d.created_at DESC, d.id DESC"
    ))?;
    let rows = stmt
        .query_map([], row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// The next download to run: the oldest queued one.
pub fn next_queued(conn: &Connection) -> Result<Option<Download>> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {COLUMNS} FROM downloads d WHERE d.state = 'queued'
                 ORDER BY d.created_at, d.id LIMIT 1"
            ),
            [],
            row,
        )
        .optional()?)
}

/// A download of this variant that is not finished (to avoid queueing it twice).
pub fn unfinished(
    conn: &Connection,
    store: Store,
    product_id: &str,
    option_id: &str,
) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT id FROM downloads WHERE store = ?1 AND product_id = ?2 AND option_id = ?3
               AND state IN ('queued', 'downloading', 'paused', 'failed')",
            params![store.as_str(), product_id, option_id],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn set_state(
    conn: &Connection,
    id: i64,
    state: DownloadState,
    error: Option<&ErrorInfo>,
    now: i64,
) -> Result<()> {
    let finished = (state == DownloadState::Completed).then_some(now);
    conn.execute(
        "UPDATE downloads SET state = ?2, error_kind = ?3, error = ?4, updated_at = ?5,
           finished_at = COALESCE(?6, finished_at)
         WHERE id = ?1",
        params![
            id,
            state.as_str(),
            error.map(|e| kind_str(e.kind)),
            error.map(|e| e.message.as_str()),
            now,
            finished
        ],
    )?;
    Ok(())
}

/// Moves a download to `to` if it is in one of the `from` states (clearing its error).
/// Returns whether it moved.
pub fn transition(
    conn: &Connection,
    id: i64,
    from: &[DownloadState],
    to: DownloadState,
    now: i64,
) -> Result<bool> {
    let from: Vec<&str> = from.iter().map(|s| s.as_str()).collect();
    let from = serde_json::to_string(&from)?;
    let changed = conn.execute(
        "UPDATE downloads SET state = ?2, error_kind = NULL, error = NULL, updated_at = ?3
         WHERE id = ?1 AND state IN (SELECT value FROM json_each(?4))",
        params![id, to.as_str(), now, from],
    )?;
    Ok(changed > 0)
}

pub fn set_progress(conn: &Connection, id: i64, done: u64, total: u64) -> Result<()> {
    conn.execute(
        "UPDATE downloads SET done_bytes = ?2, total_bytes = MAX(total_bytes, ?3) WHERE id = ?1",
        params![id, done as i64, total as i64],
    )?;
    Ok(())
}

/// Sets a download's install state, kind and error (the kind is kept when `None`).
pub fn set_install_state(
    conn: &Connection,
    id: i64,
    state: Option<InstallState>,
    kind: Option<&str>,
    error: Option<&ErrorInfo>,
    now: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE downloads SET install_state = ?2, install_kind = COALESCE(?3, install_kind),
           install_error_kind = ?4, install_error = ?5, updated_at = ?6
         WHERE id = ?1",
        params![
            id,
            state.map(InstallState::as_str),
            kind,
            error.map(|e| kind_str(e.kind)),
            error.map(|e| e.message.as_str()),
            now
        ],
    )?;
    Ok(())
}

/// Moves a finished download's install to `to` if it is in one of the `from` states (`None`
/// meaning never installed), clearing its error. Returns whether it moved.
pub fn transition_install(
    conn: &Connection,
    id: i64,
    from: &[Option<InstallState>],
    to: InstallState,
    now: i64,
) -> Result<bool> {
    let states: Vec<&str> = from.iter().flatten().map(|s| s.as_str()).collect();
    let allow_none = from.contains(&None);
    let changed = conn.execute(
        "UPDATE downloads SET install_state = ?2, install_error_kind = NULL, install_error = NULL,
           updated_at = ?3
         WHERE id = ?1 AND state = 'completed'
           AND (install_state IN (SELECT value FROM json_each(?4)) OR (?5 AND install_state IS NULL))",
        params![id, to.as_str(), now, serde_json::to_string(&states)?, allow_none],
    )?;
    Ok(changed > 0)
}

/// The next finished download to install: waiting or approved, oldest first.
pub fn next_install(conn: &Connection) -> Result<Option<Download>> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {COLUMNS} FROM downloads d
                 WHERE d.state = 'completed' AND d.install_state IN ('waiting', 'approved')
                 ORDER BY d.finished_at, d.id LIMIT 1"
            ),
            [],
            row,
        )
        .optional()?)
}

/// After a game is uninstalled, its downloads no longer count as installed.
pub fn forget_installed(conn: &Connection, store: Store, product_id: &str, now: i64) -> Result<()> {
    conn.execute(
        "UPDATE downloads SET install_state = NULL, updated_at = ?3
         WHERE store = ?1 AND product_id = ?2 AND install_state = 'installed'",
        params![store.as_str(), product_id, now],
    )?;
    Ok(())
}

/// Installs that were running when the app closed are tried again.
pub fn requeue_installing(conn: &Connection, now: i64) -> Result<u32> {
    Ok(conn.execute(
        "UPDATE downloads SET install_state = 'waiting', updated_at = ?1 WHERE install_state = 'installing'",
        params![now],
    )? as u32)
}

/// Downloads that were running when the app closed go back into the queue.
pub fn requeue_interrupted(conn: &Connection, now: i64) -> Result<u32> {
    Ok(conn.execute(
        "UPDATE downloads SET state = 'queued', updated_at = ?1 WHERE state = 'downloading'",
        params![now],
    )? as u32)
}

pub fn delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM download_files WHERE download_id = ?1",
        params![id],
    )?;
    conn.execute("DELETE FROM downloads WHERE id = ?1", params![id])?;
    Ok(())
}

/// Removes finished downloads from the list (their files stay until installed).
pub fn clear_completed(conn: &Connection) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare("SELECT id FROM downloads WHERE state = 'completed'")?;
    let ids = stmt
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<Vec<i64>, _>>()?;
    for id in &ids {
        delete(conn, *id)?;
    }
    Ok(ids)
}

/// One file of a download.
#[derive(Debug, Clone)]
pub struct FileRow {
    pub id: i64,
    pub position: u32,
    pub source: Source,
    pub size: Option<u64>,
    pub file_name: Option<String>,
    pub md5: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub done: bool,
}

pub fn files(conn: &Connection, download_id: i64) -> Result<Vec<FileRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, position, source, size, file_name, md5, etag, last_modified, done
         FROM download_files WHERE download_id = ?1 ORDER BY position",
    )?;
    let rows = stmt
        .query_map(params![download_id], |r| {
            let source: String = r.get(2)?;
            Ok((
                FileRow {
                    id: r.get(0)?,
                    position: r.get(1)?,
                    source: Source::ItchUpload(0),
                    size: r.get::<_, Option<i64>>(3)?.map(|s| s as u64),
                    file_name: r.get(4)?,
                    md5: r.get(5)?,
                    etag: r.get(6)?,
                    last_modified: r.get(7)?,
                    done: r.get(8)?,
                },
                source,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(mut f, source)| {
            f.source = Source::from_db(&source)?;
            Some(f)
        })
        .collect())
}

/// Saves what the first response told about a file.
pub fn update_file(conn: &Connection, f: &FileRow) -> Result<()> {
    conn.execute(
        "UPDATE download_files SET size = ?2, file_name = ?3, md5 = ?4, etag = ?5, last_modified = ?6, done = ?7
         WHERE id = ?1",
        params![
            f.id,
            f.size.map(|s| s as i64),
            f.file_name,
            f.md5,
            f.etag,
            f.last_modified,
            f.done
        ],
    )?;
    Ok(())
}

/// The owned download key of an itch.io product.
pub fn owned_key(conn: &Connection, store: Store, product_id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT owned_key FROM store_products WHERE store = ?1 AND product_id = ?2",
            params![store.as_str(), product_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten())
}

fn row(r: &Row<'_>) -> rusqlite::Result<Download> {
    let store: String = r.get(1)?;
    let platform: Option<String> = r.get(7)?;
    let state: String = r.get(8)?;
    let error_kind: Option<String> = r.get(12)?;
    let error: Option<String> = r.get(13)?;
    Ok(Download {
        id: r.get(0)?,
        store: Store::parse(&store).unwrap_or_default(),
        source_kind: r
            .get::<_, Option<String>>(21)?
            .as_deref()
            .map_or(DownloadSourceKind::Http, DownloadSourceKind::parse),
        product_id: r.get(2)?,
        appid: r.get(3)?,
        title: r.get(4)?,
        option_id: r.get(5)?,
        option_label: r.get(6)?,
        platform: platform.as_deref().and_then(Platform::parse),
        state: DownloadState::parse(&state),
        total_bytes: r.get::<_, i64>(9)?.max(0) as u64,
        done_bytes: r.get::<_, i64>(10)?.max(0) as u64,
        dir: r.get(11)?,
        error: error.map(|message| ErrorInfo {
            kind: error_kind.as_deref().map_or(ErrorKind::Other, parse_kind),
            message,
        }),
        created_at: r.get(14)?,
        finished_at: r.get(15)?,
        files: r.get(16)?,
        install_state: r
            .get::<_, Option<String>>(17)?
            .as_deref()
            .and_then(InstallState::parse),
        install_kind: r.get(18)?,
        install_error: r.get::<_, Option<String>>(20)?.map(|message| {
            let kind: Option<String> = r.get(19).ok().flatten();
            ErrorInfo {
                kind: kind.as_deref().map_or(ErrorKind::Other, parse_kind),
                message,
            }
        }),
    })
}

fn kind_str(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::Network => "network",
        ErrorKind::Timeout => "timeout",
        ErrorKind::RateLimited => "rate_limited",
        ErrorKind::Http => "http",
        ErrorKind::Parse => "parse",
        ErrorKind::Database => "database",
        ErrorKind::Cancelled => "cancelled",
        ErrorKind::Invalid => "invalid",
        ErrorKind::NotFound => "not_found",
        ErrorKind::Busy => "busy",
        ErrorKind::Other => "other",
    }
}

fn parse_kind(s: &str) -> ErrorKind {
    match s {
        "network" => ErrorKind::Network,
        "timeout" => ErrorKind::Timeout,
        "rate_limited" => ErrorKind::RateLimited,
        "http" => ErrorKind::Http,
        "parse" => ErrorKind::Parse,
        "database" => ErrorKind::Database,
        "cancelled" => ErrorKind::Cancelled,
        "invalid" => ErrorKind::Invalid,
        "not_found" => ErrorKind::NotFound,
        "busy" => ErrorKind::Busy,
        _ => ErrorKind::Other,
    }
}
