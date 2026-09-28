//! Catalog writes. Games are always upserted with `ON CONFLICT DO UPDATE` — never
//! `INSERT OR REPLACE`, which deletes the row and would break the FTS bookkeeping.

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;
use crate::record::GameRecord;
use crate::steam::types::RawTag;

pub mod meta_keys {
    /// Next `start` offset of an interrupted full sync.
    pub const SYNC_CURSOR: &str = "sync_cursor";
    /// Unix time the (possibly resumed) full sync started.
    pub const SYNC_RUN_STARTED: &str = "sync_run_started";
    pub const LAST_SYNC_AT: &str = "last_sync_at";
    /// Newest release date known to be fully covered; new-release checks start from here.
    pub const RELEASE_WATERMARK: &str = "release_watermark";
    pub const LAST_NEW_RELEASES_AT: &str = "last_new_releases_at";
}

pub fn upsert_tags(conn: &Connection, tags: &[RawTag]) -> Result<()> {
    let mut stmt = conn.prepare_cached(
        "INSERT INTO tags(tagid, name) VALUES (?1, ?2)
         ON CONFLICT(tagid) DO UPDATE SET name = excluded.name",
    )?;
    for tag in tags {
        let name = tag.name.trim();
        if !name.is_empty() {
            stmt.execute(params![tag.tagid, name])?;
        }
    }
    Ok(())
}

const UPSERT_GAME: &str = "
INSERT INTO games(
  appid, name, search_name, short_description, developers, publishers, franchises, tagids, descriptors,
  release_date, original_release_date, is_free, is_early_access, adult, delisted,
  price_cents, price_formatted, original_price_formatted, discount_pct,
  review_count, review_pct, review_score, rating, win, mac, linux, deck_compat,
  asset_format, img_header, img_capsule, img_capsule_2x, img_hero, assets_modified,
  first_seen_at, synced_at)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 0, ?15, ?16, ?17, ?18,
        ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?33)
ON CONFLICT(appid) DO UPDATE SET
  name = excluded.name, search_name = excluded.search_name, short_description = excluded.short_description,
  developers = excluded.developers, publishers = excluded.publishers, franchises = excluded.franchises,
  tagids = excluded.tagids, descriptors = excluded.descriptors, release_date = excluded.release_date,
  original_release_date = excluded.original_release_date, is_free = excluded.is_free,
  is_early_access = excluded.is_early_access, adult = excluded.adult, delisted = 0,
  price_cents = excluded.price_cents, price_formatted = excluded.price_formatted,
  original_price_formatted = excluded.original_price_formatted, discount_pct = excluded.discount_pct,
  review_count = excluded.review_count, review_pct = excluded.review_pct, review_score = excluded.review_score,
  rating = excluded.rating, win = excluded.win, mac = excluded.mac, linux = excluded.linux,
  deck_compat = excluded.deck_compat, asset_format = excluded.asset_format, img_header = excluded.img_header,
  img_capsule = excluded.img_capsule, img_capsule_2x = excluded.img_capsule_2x, img_hero = excluded.img_hero,
  assets_modified = excluded.assets_modified, synced_at = excluded.synced_at";

/// Inserts or refreshes games; `now` becomes `synced_at` (and `first_seen_at` for new rows).
pub fn upsert_games(conn: &Connection, games: &[GameRecord], now: i64) -> Result<()> {
    let mut stmt = conn.prepare_cached(UPSERT_GAME)?;
    for g in games {
        stmt.execute(params![
            g.appid,
            g.name,
            g.search_name,
            g.short_description,
            g.developers,
            g.publishers,
            g.franchises,
            g.tagids,
            g.descriptors,
            g.release_date,
            g.original_release_date,
            g.is_free,
            g.is_early_access,
            g.adult,
            g.price_cents,
            g.price_formatted,
            g.original_price_formatted,
            g.discount_pct,
            g.review_count,
            g.review_pct,
            g.review_score,
            g.rating,
            g.win,
            g.mac,
            g.linux,
            g.deck_compat,
            g.asset_format,
            g.img_header,
            g.img_capsule,
            g.img_capsule_2x,
            g.img_hero,
            g.assets_modified,
            now,
        ])?;
    }
    Ok(())
}

/// Hides games not refreshed since `run_started` (they left the store). Returns how many.
pub fn mark_delisted(conn: &Connection, run_started: i64) -> Result<u32> {
    let n = conn.execute(
        "UPDATE games SET delisted = 1 WHERE synced_at < ?1 AND delisted = 0",
        params![run_started],
    )?;
    Ok(n as u32)
}

/// Recounts visible (listed, non-adult) games per tag for the filter panel.
pub fn refresh_tag_counts(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE tags SET game_count = (
           SELECT COUNT(*) FROM game_tags t JOIN games g ON g.appid = t.appid
           WHERE t.tagid = tags.tagid AND g.delisted = 0 AND g.adult = 0)",
        [],
    )?;
    Ok(())
}

pub fn set_meta(conn: &Connection, key: &str, value: impl ToString) -> Result<()> {
    conn.execute(
        "INSERT INTO meta(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value.to_string()],
    )?;
    Ok(())
}

pub fn get_meta(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
            r.get(0)
        })
        .optional()?)
}

pub fn get_meta_i64(conn: &Connection, key: &str) -> Result<Option<i64>> {
    Ok(get_meta(conn, key)?.and_then(|v| v.parse().ok()))
}

pub fn delete_meta(conn: &Connection, key: &str) -> Result<()> {
    conn.execute("DELETE FROM meta WHERE key = ?1", params![key])?;
    Ok(())
}

/// Newest release date among listed games.
pub fn newest_release(conn: &Connection) -> Result<Option<i64>> {
    Ok(conn.query_row(
        "SELECT MAX(release_date) FROM games WHERE delisted = 0",
        [],
        |r| r.get(0),
    )?)
}

/// Housekeeping after a large write: merge FTS segments, refresh planner stats, shrink the WAL.
pub fn finalize(conn: &Connection) -> Result<()> {
    conn.execute("INSERT INTO games_fts(games_fts) VALUES ('optimize')", [])?;
    conn.execute_batch("PRAGMA optimize;")?;
    // Returns a status row; a busy reader only makes the checkpoint partial, which is fine.
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    Ok(())
}
