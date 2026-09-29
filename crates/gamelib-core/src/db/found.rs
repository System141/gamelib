//! Games found on this computer outside GameLib (see [`crate::install::found`]).

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::Result;
use crate::model::{FoundMatch, FoundSource};

const COLUMNS: &str =
    "f.id, f.source, f.title, f.dir, f.root, f.appid, f.matched_by, f.exe, f.args,
  f.workdir, f.exe_by_user, f.launch_url, f.candidates, f.hidden, f.found_at, f.seen_at";

/// A found game as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct FoundRow {
    /// `steam:<appid>`, `epic:<app name>` or `folder:<path>`.
    pub id: String,
    pub source: FoundSource,
    pub title: String,
    pub dir: String,
    /// The Steam library, manifests folder or scanned folder it was found in.
    pub root: String,
    pub appid: Option<u32>,
    pub matched_by: Option<FoundMatch>,
    pub exe: Option<String>,
    pub args: String,
    pub workdir: Option<String>,
    /// The user chose what to start; scans keep it while the file is there.
    pub exe_by_user: bool,
    pub launch_url: Option<String>,
    pub candidates: Vec<String>,
    /// The user took it off the list.
    pub hidden: bool,
    pub found_at: i64,
    pub seen_at: i64,
}

pub fn upsert(conn: &Connection, r: &FoundRow) -> Result<()> {
    conn.execute(
        "INSERT INTO found_games(id, source, title, dir, root, appid, matched_by, exe, args, workdir,
           exe_by_user, launch_url, candidates, hidden, found_at, seen_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
         ON CONFLICT(id) DO UPDATE SET
           source = excluded.source, title = excluded.title, dir = excluded.dir,
           root = excluded.root, appid = excluded.appid, matched_by = excluded.matched_by,
           exe = excluded.exe, args = excluded.args, workdir = excluded.workdir,
           exe_by_user = excluded.exe_by_user, launch_url = excluded.launch_url,
           candidates = excluded.candidates, hidden = excluded.hidden,
           found_at = excluded.found_at, seen_at = excluded.seen_at",
        params![
            r.id,
            r.source.as_str(),
            r.title,
            r.dir,
            r.root,
            r.appid,
            r.matched_by.map(FoundMatch::as_str),
            r.exe,
            r.args,
            r.workdir,
            r.exe_by_user,
            r.launch_url,
            serde_json::to_string(&r.candidates)?,
            r.hidden,
            r.found_at,
            r.seen_at,
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<FoundRow>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM found_games f WHERE f.id = ?1"),
            [id],
            row,
        )
        .optional()?)
}

/// Every found game.
pub fn all(conn: &Connection) -> Result<Vec<FoundRow>> {
    let mut stmt = conn.prepare_cached(&format!("SELECT {COLUMNS} FROM found_games f"))?;
    let rows = stmt
        .query_map([], row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Found games that are shown (or, with `hidden`, the ones taken off the list), each with the
/// name of its Steam game.
pub fn listed(conn: &Connection, hidden: bool) -> Result<Vec<(FoundRow, Option<String>)>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {COLUMNS}, g.name FROM found_games f LEFT JOIN games g ON g.appid = f.appid
         WHERE f.hidden = ?1 ORDER BY COALESCE(g.name, f.title) COLLATE NOCASE"
    ))?;
    let rows = stmt
        .query_map([hidden], |r| Ok((row(r)?, r.get(16)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn delete(conn: &Connection, id: &str) -> Result<bool> {
    Ok(conn.execute("DELETE FROM found_games WHERE id = ?1", [id])? > 0)
}

pub fn set_hidden(conn: &Connection, id: &str, hidden: bool) -> Result<bool> {
    Ok(conn.execute(
        "UPDATE found_games SET hidden = ?2 WHERE id = ?1",
        params![id, hidden],
    )? > 0)
}

/// The Steam game the user says it is (`None`: none).
pub fn set_match(conn: &Connection, id: &str, appid: Option<u32>) -> Result<bool> {
    Ok(conn.execute(
        "UPDATE found_games SET appid = ?2, matched_by = 'manual' WHERE id = ?1",
        params![id, appid],
    )? > 0)
}

/// What "Oyna" starts, chosen by the user.
pub fn set_launch(conn: &Connection, id: &str, exe: &str, workdir: Option<&str>) -> Result<bool> {
    Ok(conn.execute(
        "UPDATE found_games SET exe = ?2, workdir = ?3, args = '', exe_by_user = 1 WHERE id = ?1",
        params![id, exe, workdir],
    )? > 0)
}

/// Whether the catalog has this Steam game.
pub fn in_catalog(conn: &Connection, appid: u32) -> Result<bool> {
    Ok(conn
        .prepare_cached("SELECT 1 FROM games WHERE appid = ?1")?
        .exists([appid])?)
}

fn row(r: &Row<'_>) -> rusqlite::Result<FoundRow> {
    let source: String = r.get(1)?;
    let matched_by: Option<String> = r.get(6)?;
    let candidates: String = r.get(12)?;
    Ok(FoundRow {
        id: r.get(0)?,
        source: FoundSource::parse(&source).unwrap_or(FoundSource::Folder),
        title: r.get(2)?,
        dir: r.get(3)?,
        root: r.get(4)?,
        appid: r.get(5)?,
        matched_by: matched_by.as_deref().and_then(FoundMatch::parse),
        exe: r.get(7)?,
        args: r.get(8)?,
        workdir: r.get(9)?,
        exe_by_user: r.get(10)?,
        launch_url: r.get(11)?,
        candidates: serde_json::from_str(&candidates).unwrap_or_default(),
        hidden: r.get(13)?,
        found_at: r.get(14)?,
        seen_at: r.get(15)?,
    })
}
