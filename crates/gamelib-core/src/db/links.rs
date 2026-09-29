//! Storage for user-added external links.

use reqwest::Url;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::links::SiteRegistry;
use crate::links::sites::generic::GENERIC_SITE_ID;
use crate::links::validate::{is_insecure, optional_text, parse_link_url, parse_magnet_url};
use crate::model::{
    CheckStatus, GameLink, LinkCheck, LinkCheckSummary, LinkInput, LinkKind, Platform,
};
use crate::{Error, Result};

pub const MAX_LABEL_CHARS: usize = 120;
pub const MAX_VERSION_CHARS: usize = 60;
pub const MAX_NOTES_CHARS: usize = 1000;

const COLUMNS: &str = "id, appid, site_id, url, label, kind, platform, version, notes, check_status, http_status,
  resolved_url, final_host, redirects, file_name, size_bytes, content_type, is_file, checked_at, created_at, updated_at, hops";

fn link_from_row(row: &Row<'_>) -> rusqlite::Result<GameLink> {
    let url: String = row.get(3)?;
    let magnet = url.trim_start().to_ascii_lowercase().starts_with("magnet:");
    let parsed = Url::parse(&url).ok();
    let kind: String = row.get(5)?;
    let platform: Option<String> = row.get(6)?;
    let check_status: Option<String> = row.get(9)?;
    let last_check = match check_status {
        Some(status) => Some(LinkCheckSummary {
            status: CheckStatus::parse(&status),
            http_status: row.get(10)?,
            resolved_url: row.get(11)?,
            final_host: row.get(12)?,
            redirects: row.get::<_, Option<u32>>(13)?.unwrap_or(0),
            hops: row
                .get::<_, Option<String>>(21)?
                .and_then(|raw| serde_json::from_str(&raw).ok())
                .unwrap_or_default(),
            file_name: row.get(14)?,
            size_bytes: row
                .get::<_, Option<i64>>(15)?
                .and_then(|n| u64::try_from(n).ok()),
            content_type: row.get(16)?,
            is_file: row.get::<_, Option<bool>>(17)?.unwrap_or(false),
            checked_at: row.get::<_, Option<i64>>(18)?.unwrap_or(0),
        }),
        None => None,
    };
    Ok(GameLink {
        id: row.get(0)?,
        appid: row.get(1)?,
        site_id: row.get(2)?,
        // A magnet link has no host: the UI shows "magnet" where other links show theirs.
        host: if magnet {
            "magnet".to_owned()
        } else {
            parsed
                .as_ref()
                .and_then(|u| u.host_str())
                .unwrap_or_default()
                .to_owned()
        },
        insecure: !magnet && parsed.as_ref().is_some_and(is_insecure),
        url,
        label: row.get(4)?,
        kind: LinkKind::parse(&kind),
        platform: platform.as_deref().and_then(Platform::parse),
        version: row.get(7)?,
        notes: row.get(8)?,
        last_check,
        created_at: row.get(19)?,
        updated_at: row.get(20)?,
    })
}

/// Links of one game, oldest first.
pub fn list_links(conn: &Connection, appid: u32) -> Result<Vec<GameLink>> {
    let mut stmt = conn.prepare_cached(&format!(
        "SELECT {COLUMNS} FROM game_links WHERE appid = ?1 ORDER BY created_at, id"
    ))?;
    let links = stmt
        .query_map(params![appid], link_from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(links)
}

pub fn get_link(conn: &Connection, id: i64) -> Result<Option<GameLink>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM game_links WHERE id = ?1"),
            params![id],
            link_from_row,
        )
        .optional()?)
}

/// Creates or updates a link after validating it and detecting its site. Changing the URL
/// clears the previous check result.
pub fn save_link(
    conn: &Connection,
    sites: &SiteRegistry,
    input: &LinkInput,
    now: i64,
) -> Result<GameLink> {
    let (url, site_id) = if input.url.trim().to_ascii_lowercase().starts_with("magnet:") {
        // A magnet link has no page to normalise: it is stored exactly as validated, and the
        // UI offers to queue it for download instead of opening it.
        (parse_magnet_url(&input.url)?, GENERIC_SITE_ID.to_owned())
    } else {
        let parsed = parse_link_url(&input.url)?;
        let handler = sites.detect(&parsed);
        (
            handler.normalize(parsed).to_string(),
            handler.info().id.clone(),
        )
    };
    let label = optional_text(input.label.as_deref(), MAX_LABEL_CHARS, "label_too_long")?;
    let version = optional_text(
        input.version.as_deref(),
        MAX_VERSION_CHARS,
        "version_too_long",
    )?;
    let notes = optional_text(input.notes.as_deref(), MAX_NOTES_CHARS, "notes_too_long")?;
    let kind = input.kind.as_str();
    let platform = input.platform.map(Platform::as_str);

    let game_exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM games WHERE appid = ?1)",
        params![input.appid],
        |r| r.get(0),
    )?;
    if !game_exists {
        return Err(Error::NotFound);
    }

    let id = match input.id {
        Some(id) => {
            let old_url: Option<String> = conn
                .query_row("SELECT url FROM game_links WHERE id = ?1 AND appid = ?2", params![id, input.appid], |r| r.get(0))
                .optional()?;
            let old_url = old_url.ok_or(Error::NotFound)?;
            conn.execute(
                "UPDATE game_links SET site_id = ?2, url = ?3, label = ?4, kind = ?5, platform = ?6, version = ?7,
                   notes = ?8, updated_at = ?9 WHERE id = ?1",
                params![id, site_id, url.as_str(), label, kind, platform, version, notes, now],
            )?;
            if old_url != url.as_str() {
                clear_check(conn, id)?;
            }
            id
        }
        None => conn.query_row(
            "INSERT INTO game_links(appid, site_id, url, label, kind, platform, version, notes, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9) RETURNING id",
            params![input.appid, site_id, url.as_str(), label, kind, platform, version, notes, now],
            |r| r.get(0),
        )?,
    };
    get_link(conn, id)?.ok_or(Error::NotFound)
}

pub fn delete_link(conn: &Connection, id: i64) -> Result<bool> {
    Ok(conn.execute("DELETE FROM game_links WHERE id = ?1", params![id])? > 0)
}

pub fn record_check(conn: &Connection, id: i64, check: &LinkCheck) -> Result<()> {
    conn.execute(
        "UPDATE game_links SET check_status = ?2, http_status = ?3, resolved_url = ?4, final_host = ?5, redirects = ?6,
           file_name = ?7, size_bytes = ?8, content_type = ?9, is_file = ?10, checked_at = ?11, hops = ?12 WHERE id = ?1",
        params![
            id,
            check.status.as_str(),
            check.http_status,
            check.final_url,
            check.final_host,
            check.redirects(),
            check.file_name,
            check.size_bytes.map(|n| i64::try_from(n).unwrap_or(i64::MAX)),
            check.content_type,
            check.is_file,
            check.checked_at,
            serde_json::to_string(&check.hops).unwrap_or_else(|_| "[]".into()),
        ],
    )?;
    Ok(())
}

fn clear_check(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE game_links SET check_status = NULL, http_status = NULL, resolved_url = NULL, final_host = NULL,
           redirects = NULL, file_name = NULL, size_bytes = NULL, content_type = NULL, is_file = NULL, checked_at = NULL,
           hops = NULL
         WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}
