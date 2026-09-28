//! Installed games.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::Result;
use crate::model::{InstallMethod, Installed, Store};

const COLUMNS: &str =
    "store, product_id, appid, title, dir, exe, args, workdir, method, candidates,
  option_label, installed_at, uninstaller";

/// An install as stored, with what the UI does not need.
#[derive(Debug, Clone, PartialEq)]
pub struct InstallRow {
    pub installed: Installed,
    pub uninstaller: Option<String>,
}

/// Adds or replaces the install of a product.
pub fn upsert(conn: &Connection, row: &InstallRow) -> Result<()> {
    let i = &row.installed;
    conn.execute(
        "INSERT INTO installs(store, product_id, appid, title, dir, exe, args, workdir, method, candidates,
           uninstaller, option_label, installed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT(store, product_id) DO UPDATE SET
           appid = excluded.appid, title = excluded.title, dir = excluded.dir, exe = excluded.exe,
           args = excluded.args, workdir = excluded.workdir, method = excluded.method,
           candidates = excluded.candidates, uninstaller = excluded.uninstaller,
           option_label = excluded.option_label, installed_at = excluded.installed_at",
        params![
            i.store.as_str(),
            i.product_id,
            i.appid,
            i.title,
            i.dir,
            i.exe,
            i.args,
            i.workdir,
            i.method.as_str(),
            serde_json::to_string(&i.candidates)?,
            row.uninstaller,
            i.option_label,
            i.installed_at,
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, store: Store, product_id: &str) -> Result<Option<InstallRow>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM installs WHERE store = ?1 AND product_id = ?2"),
            params![store.as_str(), product_id],
            row,
        )
        .optional()?)
}

/// Every install, by title.
pub fn list(conn: &Connection) -> Result<Vec<InstallRow>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM installs ORDER BY title COLLATE NOCASE, store"
    ))?;
    let rows = stmt
        .query_map([], row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Chooses what "Oyna" starts (and from where).
pub fn set_launch(
    conn: &Connection,
    store: Store,
    product_id: &str,
    exe: &str,
    workdir: Option<&str>,
) -> Result<bool> {
    Ok(conn.execute(
        "UPDATE installs SET exe = ?3, workdir = ?4, args = '' WHERE store = ?1 AND product_id = ?2",
        params![store.as_str(), product_id, exe, workdir],
    )? > 0)
}

pub fn delete(conn: &Connection, store: Store, product_id: &str) -> Result<bool> {
    Ok(conn.execute(
        "DELETE FROM installs WHERE store = ?1 AND product_id = ?2",
        params![store.as_str(), product_id],
    )? > 0)
}

fn row(r: &Row<'_>) -> rusqlite::Result<InstallRow> {
    let store: String = r.get(0)?;
    let method: String = r.get(8)?;
    let candidates: String = r.get(9)?;
    Ok(InstallRow {
        installed: Installed {
            store: Store::parse(&store).unwrap_or_default(),
            product_id: r.get(1)?,
            appid: r.get(2)?,
            title: r.get(3)?,
            dir: r.get(4)?,
            exe: r.get(5)?,
            args: r.get(6)?,
            workdir: r.get(7)?,
            method: InstallMethod::parse(&method).unwrap_or(InstallMethod::Installer),
            candidates: serde_json::from_str(&candidates).unwrap_or_default(),
            option_label: r.get(10)?,
            installed_at: r.get(11)?,
            external: false,
            steam_header: None,
        },
        uninstaller: r.get(12)?,
    })
}
