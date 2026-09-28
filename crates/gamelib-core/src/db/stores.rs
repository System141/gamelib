//! Store products (GOG, itch.io) and their matches to Steam games.
//!
//! Match precedence: a manual choice beats a GamesDB id match, which beats a title match. The
//! user's verdict (`state`) is never changed by refreshes; only automatic title guesses are
//! replaced when matching runs again.

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;
use crate::db::Db;
use crate::db::write::{get_meta_i64, meta_keys};
use crate::model::{LibraryItem, MatchMethod, MatchState, Store, StoreCounts, StoreMatch};
use crate::steam::assets::asset_url;
use crate::stores::StoreProduct;
use crate::stores::gamesdb::Releases;
use crate::stores::matching::{self, CERTAIN, Candidate, GAMESDB, MatchKey, SteamIndex};

/// Condition (on alias `m`) for a match the UI counts: confirmed, or automatic and confident.
/// The number is [`matching::CONFIDENT`].
pub const CONFIDENT_SQL: &str =
    "m.state != 'rejected' AND (m.state = 'confirmed' OR m.score >= 0.85)";

/// Inserts new products and refreshes known ones from a catalog listing. Ownership and GamesDB
/// bookkeeping are left alone. Returns how many products were new.
pub fn upsert_products(conn: &mut Connection, products: &[StoreProduct], now: i64) -> Result<u32> {
    let tx = conn.transaction()?;
    let mut inserted = 0;
    {
        let mut insert = tx.prepare_cached(
            "INSERT INTO store_products(store, product_id, kind, title, canonical_title, slug, url,
               developers, publishers, release_date, store_release_date, cover, cover_wide,
               win, mac, linux, price_formatted, is_free, in_catalog, seen_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, 1, ?19)
             ON CONFLICT(store, product_id) DO UPDATE SET
               kind = excluded.kind, title = excluded.title, canonical_title = excluded.canonical_title,
               slug = excluded.slug, url = excluded.url, developers = excluded.developers,
               publishers = excluded.publishers, release_date = excluded.release_date,
               store_release_date = excluded.store_release_date, cover = excluded.cover,
               cover_wide = excluded.cover_wide, win = excluded.win, mac = excluded.mac,
               linux = excluded.linux, price_formatted = excluded.price_formatted,
               is_free = excluded.is_free, in_catalog = 1, seen_at = excluded.seen_at",
        )?;
        let mut exists =
            tx.prepare_cached("SELECT 1 FROM store_products WHERE store = ?1 AND product_id = ?2")?;
        for p in products {
            let store = p.store.as_str();
            if !exists.exists(params![store, p.product_id])? {
                inserted += 1;
            }
            insert.execute(params![
                store,
                p.product_id,
                p.kind,
                p.title,
                matching::canonical_title(&p.title),
                p.slug,
                p.url,
                json_list(&p.developers),
                json_list(&p.publishers),
                p.release_date,
                p.store_release_date,
                p.cover,
                p.cover_wide,
                p.win,
                p.mac,
                p.linux,
                p.price,
                p.is_free,
                now,
            ])?;
        }
    }
    tx.commit()?;
    Ok(inserted)
}

/// Adds a product the catalog listing does not contain (found through GamesDB or an account),
/// without touching one that is already known. Returns whether it was new.
pub fn insert_extra_product(conn: &Connection, p: &StoreProduct, now: i64) -> Result<bool> {
    let n = conn.execute(
        "INSERT INTO store_products(store, product_id, kind, title, canonical_title, slug, url,
           developers, publishers, release_date, store_release_date, cover, cover_wide,
           win, mac, linux, price_formatted, is_free, in_catalog, seen_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, 0, ?19)
         ON CONFLICT(store, product_id) DO NOTHING",
        params![
            p.store.as_str(),
            p.product_id,
            p.kind,
            p.title,
            matching::canonical_title(&p.title),
            p.slug,
            p.url,
            json_list(&p.developers),
            json_list(&p.publishers),
            p.release_date,
            p.store_release_date,
            p.cover,
            p.cover_wide,
            p.win,
            p.mac,
            p.linux,
            p.price,
            p.is_free,
            now,
        ],
    )?;
    Ok(n == 1)
}

/// Stamp of the latest catalog listing of `store` (0 if never listed).
pub fn last_seen(conn: &Connection, store: Store) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(seen_at), 0) FROM store_products WHERE store = ?1 AND in_catalog = 1",
        params![store.as_str()],
        |r| r.get(0),
    )?)
}

/// Products listed before `run_started` but missing from this listing are no longer sold.
pub fn mark_unlisted(conn: &Connection, store: Store, run_started: i64) -> Result<u32> {
    let n = conn.execute(
        "UPDATE store_products SET in_catalog = 0
         WHERE store = ?1 AND in_catalog = 1 AND seen_at < ?2",
        params![store.as_str(), run_started],
    )?;
    Ok(n as u32)
}

/// A product's matching input.
#[derive(Debug, Clone)]
pub struct MatchInput {
    pub product_id: String,
    pub canonical_title: String,
    pub key: MatchKey,
}

/// Products of `store`, with what title matching needs. Packs are included: GOG sells many
/// games only as an edition pack ("Cyberpunk 2077", "Fallout 3: Game of the Year Edition"),
/// and a pack only matches a Steam game with the same title.
pub fn products_for_matching(
    conn: &Connection,
    store: Store,
    only: Option<&[String]>,
) -> Result<Vec<MatchInput>> {
    let mut stmt = conn.prepare(
        "SELECT product_id, canonical_title, developers, publishers, release_date, store_release_date
         FROM store_products
         WHERE store = ?1 AND kind IN ('game', 'pack')
           AND (?2 IS NULL OR product_id IN (SELECT value FROM json_each(?2)))",
    )?;
    let only = only.map(|ids| serde_json::to_string(ids).unwrap_or_else(|_| "[]".into()));
    let rows = stmt
        .query_map(params![store.as_str(), only], |r| {
            let developers: String = r.get(2)?;
            let publishers: String = r.get(3)?;
            let companies = parse_list(&developers)
                .into_iter()
                .chain(parse_list(&publishers))
                .collect::<Vec<_>>();
            Ok(MatchInput {
                product_id: r.get(0)?,
                canonical_title: r.get(1)?,
                key: MatchKey::new(companies.iter().map(String::as_str), [r.get(4)?, r.get(5)?]),
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// The listed Steam games, indexed for title matching.
pub fn steam_index(conn: &Connection) -> Result<SteamIndex> {
    let mut index = SteamIndex::default();
    let mut stmt = conn.prepare(
        "SELECT appid, name, developers, publishers, release_date, original_release_date, review_count
         FROM games WHERE delisted = 0",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let developers: String = r.get(2)?;
        let publishers: String = r.get(3)?;
        let companies = parse_list(&developers)
            .into_iter()
            .chain(parse_list(&publishers))
            .collect::<Vec<_>>();
        let name: String = r.get(1)?;
        index.insert(
            r.get(0)?,
            &name,
            MatchKey::new(companies.iter().map(String::as_str), [r.get(4)?, r.get(5)?]),
            r.get(6)?,
        );
    }
    Ok(index)
}

/// Replaces a product's automatic title matches with `candidates`. A product that GamesDB
/// already tied to Steam keeps those matches only.
pub fn replace_title_matches(
    conn: &Connection,
    store: Store,
    product_id: &str,
    candidates: &[Candidate],
    now: i64,
) -> Result<()> {
    conn.execute(
        "DELETE FROM store_matches
         WHERE store = ?1 AND product_id = ?2 AND method = 'title' AND state = 'auto'",
        params![store.as_str(), product_id],
    )?;
    let has_id_match: bool = conn
        .prepare_cached(
            "SELECT 1 FROM store_matches
             WHERE store = ?1 AND product_id = ?2 AND method IN ('gamesdb', 'manual')",
        )?
        .exists(params![store.as_str(), product_id])?;
    if has_id_match {
        return Ok(());
    }
    for c in candidates {
        upsert_match(
            conn,
            store,
            product_id,
            c.appid,
            MatchMethod::Title,
            c.score,
            now,
        )?;
    }
    Ok(())
}

/// Records a match, keeping a stronger method (manual > GamesDB > title) and the user's verdict.
pub fn upsert_match(
    conn: &Connection,
    store: Store,
    product_id: &str,
    appid: u32,
    method: MatchMethod,
    score: f32,
    now: i64,
) -> Result<()> {
    conn.prepare_cached(
        "INSERT INTO store_matches(store, product_id, appid, method, score, state, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'auto', ?6)
         ON CONFLICT(store, product_id, appid) DO UPDATE SET
           method = CASE WHEN store_matches.method = 'manual'
                           OR (store_matches.method = 'gamesdb' AND excluded.method = 'title')
                         THEN store_matches.method ELSE excluded.method END,
           score = CASE WHEN store_matches.method = 'manual'
                          OR (store_matches.method = 'gamesdb' AND excluded.method = 'title')
                        THEN store_matches.score ELSE excluded.score END,
           updated_at = excluded.updated_at",
    )?
    .execute(params![
        store.as_str(),
        product_id,
        appid,
        method.as_str(),
        f64::from(score),
        now
    ])?;
    Ok(())
}

/// Stores a GamesDB answer for one product: its Steam ids become matches, and automatic title
/// guesses it contradicts are dropped. `None` (unknown to GamesDB) only marks it as checked.
pub fn apply_gamesdb(
    conn: &mut Connection,
    store: Store,
    product_id: &str,
    releases: Option<&Releases>,
    now: i64,
) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE store_products SET external_checked_at = ?3 WHERE store = ?1 AND product_id = ?2",
        params![store.as_str(), product_id, now],
    )?;
    if let Some(r) = releases.filter(|r| r.is_game()) {
        let known = existing_games(&tx, &r.steam)?;
        if !known.is_empty() {
            for &appid in &known {
                upsert_match(
                    &tx,
                    store,
                    product_id,
                    appid,
                    MatchMethod::Gamesdb,
                    GAMESDB,
                    now,
                )?;
            }
            tx.execute(
                "DELETE FROM store_matches
                 WHERE store = ?1 AND product_id = ?2 AND method = 'title' AND state = 'auto'
                   AND appid NOT IN (SELECT value FROM json_each(?3))",
                params![store.as_str(), product_id, serde_json::to_string(&known)?],
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Stores the GOG products GamesDB lists for a Steam game as matches of that game. Only
/// products already known (and games, not packs) are used. Returns how many were matched.
pub fn apply_steam_lookup(
    conn: &mut Connection,
    store: Store,
    appid: u32,
    releases: Option<&Releases>,
    now: i64,
) -> Result<u32> {
    let tx = conn.transaction()?;
    let mut matched = 0;
    if let Some(r) = releases.filter(|r| r.is_game()) {
        for id in &r.gog {
            let is_game: bool = tx
                .prepare_cached(
                    "SELECT 1 FROM store_products WHERE store = ?1 AND product_id = ?2 AND kind = 'game'",
                )?
                .exists(params![store.as_str(), id])?;
            if is_game {
                upsert_match(&tx, store, id, appid, MatchMethod::Gamesdb, GAMESDB, now)?;
                matched += 1;
            }
        }
    }
    tx.execute(
        "INSERT INTO store_lookups(store, appid, checked_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(store, appid) DO UPDATE SET checked_at = excluded.checked_at",
        params![store.as_str(), appid, now],
    )?;
    tx.commit()?;
    Ok(matched)
}

/// A product's store page as listed.
pub fn product_url(conn: &Connection, store: Store, product_id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT url FROM store_products WHERE store = ?1 AND product_id = ?2",
            params![store.as_str(), product_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten())
}

/// When `appid` was last looked up in `store`'s cross-reference.
pub fn last_lookup(conn: &Connection, store: Store, appid: u32) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT checked_at FROM store_lookups WHERE store = ?1 AND appid = ?2",
            params![store.as_str(), appid],
            |r| r.get(0),
        )
        .optional()?)
}

/// Which of `ids` are known products of `store`.
pub fn known_products(conn: &Connection, store: Store, ids: &[String]) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare_cached("SELECT 1 FROM store_products WHERE store = ?1 AND product_id = ?2")?;
    let mut known = Vec::new();
    for id in ids {
        if stmt.exists(params![store.as_str(), id])? {
            known.push(id.clone());
        }
    }
    Ok(known)
}

fn existing_games(conn: &Connection, appids: &[u32]) -> Result<Vec<u32>> {
    let mut stmt = conn.prepare_cached("SELECT 1 FROM games WHERE appid = ?1")?;
    let mut out = Vec::new();
    for &appid in appids {
        if stmt.exists(params![appid])? {
            out.push(appid);
        }
    }
    Ok(out)
}

/// GOG products to check against GamesDB: never checked, and without a certain match (only the
/// owned ones with `owned_only`).
pub fn gamesdb_todo(conn: &Connection, store: Store, owned_only: bool) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT p.product_id FROM store_products p
         WHERE p.store = ?1 AND p.kind = 'game' AND p.external_checked_at IS NULL
           AND (?3 = 0 OR p.owned = 1)
           AND NOT EXISTS (
             SELECT 1 FROM store_matches m
             WHERE m.store = p.store AND m.product_id = p.product_id AND m.state != 'rejected'
               AND (m.method != 'title' OR m.state = 'confirmed' OR m.score >= ?2))
         ORDER BY p.in_catalog DESC, p.product_id",
    )?;
    let ids = stmt
        .query_map(
            params![store.as_str(), f64::from(CERTAIN), owned_only],
            |r| r.get(0),
        )?
        .collect::<std::result::Result<Vec<String>, _>>()?;
    Ok(ids)
}

/// Every non-rejected store match of a Steam game: confident ones first, then suggestions.
pub fn matches_for_game(conn: &Connection, appid: u32) -> Result<Vec<StoreMatch>> {
    let mut stmt = conn.prepare_cached(
        "SELECT m.store, m.product_id, p.title, p.url, p.cover, p.cover_wide, p.price_formatted,
                p.is_free, p.owned, p.win, p.mac, p.linux, m.method, m.score, m.state
         FROM store_matches m
         JOIN store_products p ON p.store = m.store AND p.product_id = m.product_id
         WHERE m.appid = ?1 AND m.state != 'rejected'
         ORDER BY m.store, (m.state = 'confirmed' OR m.score >= 0.85) DESC, m.score DESC, p.title",
    )?;
    let matches = stmt
        .query_map(params![appid], |r| {
            let store: String = r.get(0)?;
            let method: String = r.get(12)?;
            let score: f64 = r.get(13)?;
            let state = MatchState::parse(&r.get::<_, String>(14)?);
            let score = score as f32;
            Ok(StoreMatch {
                store: Store::parse(&store).unwrap_or_default(),
                product_id: r.get(1)?,
                title: r.get(2)?,
                url: r.get(3)?,
                cover: r.get(4)?,
                cover_wide: r.get(5)?,
                price: r.get(6)?,
                is_free: r.get(7)?,
                owned: r.get(8)?,
                win: r.get(9)?,
                mac: r.get(10)?,
                linux: r.get(11)?,
                method: MatchMethod::parse(&method),
                score,
                state,
                confident: state == MatchState::Confirmed
                    || (state == MatchState::Auto && score >= matching::CONFIDENT),
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(matches)
}

/// Records the user's verdict on a match. Returns false if there is no such match.
pub fn set_match_state(
    conn: &Connection,
    store: Store,
    product_id: &str,
    appid: u32,
    state: MatchState,
    now: i64,
) -> Result<bool> {
    let n = conn.execute(
        "UPDATE store_matches SET state = ?4, updated_at = ?5
         WHERE store = ?1 AND product_id = ?2 AND appid = ?3",
        params![store.as_str(), product_id, appid, state.as_str(), now],
    )?;
    Ok(n > 0)
}

/// Ties a product to a Steam game by hand (confirmed, full score).
pub fn set_manual_match(
    conn: &Connection,
    store: Store,
    product_id: &str,
    appid: u32,
    now: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO store_matches(store, product_id, appid, method, score, state, updated_at)
         VALUES (?1, ?2, ?3, 'manual', 1.0, 'confirmed', ?4)
         ON CONFLICT(store, product_id, appid) DO UPDATE SET
           method = 'manual', score = 1.0, state = 'confirmed', updated_at = excluded.updated_at",
        params![store.as_str(), product_id, appid, now],
    )?;
    Ok(())
}

/// Marks exactly `owned` (product id, download key) as the user's products in `store`.
pub fn set_owned(
    conn: &mut Connection,
    store: Store,
    owned: &[(String, Option<u64>)],
) -> Result<()> {
    let ids: Vec<&str> = owned.iter().map(|(id, _)| id.as_str()).collect();
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE store_products SET owned = 0, owned_key = NULL
         WHERE store = ?1 AND owned = 1 AND product_id NOT IN (SELECT value FROM json_each(?2))",
        params![store.as_str(), serde_json::to_string(&ids)?],
    )?;
    {
        let mut stmt = tx.prepare(
            "UPDATE store_products SET owned = 1, owned_key = ?3 WHERE store = ?1 AND product_id = ?2",
        )?;
        for (id, key) in owned {
            stmt.execute(params![store.as_str(), id, key.map(|k| k.to_string())])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Forgets ownership in `store` (after signing out).
pub fn clear_owned(conn: &Connection, store: Store) -> Result<()> {
    conn.execute(
        "UPDATE store_products SET owned = 0, owned_key = NULL WHERE store = ?1 AND owned = 1",
        params![store.as_str()],
    )?;
    Ok(())
}

/// Owned products tied to a Steam game.
pub fn owned_matched(conn: &Connection) -> Result<u32> {
    Ok(conn.query_row(
        &format!(
            "SELECT COUNT(DISTINCT p.store || ':' || p.product_id) FROM store_products p
             JOIN store_matches m ON m.store = p.store AND m.product_id = p.product_id
             WHERE p.owned = 1 AND {CONFIDENT_SQL}"
        ),
        [],
        |r| r.get(0),
    )?)
}

/// The user's games (no DLC or packs, which have no files of their own), with the Steam game
/// each one is, if known.
pub fn library(conn: &Connection, store: Option<Store>) -> Result<Vec<LibraryItem>> {
    let mut stmt = conn.prepare_cached(&format!(
        "WITH lib AS (
           SELECT p.*, (SELECT m.appid FROM store_matches m
                        WHERE m.store = p.store AND m.product_id = p.product_id AND {CONFIDENT_SQL}
                        ORDER BY m.score DESC, m.appid LIMIT 1) AS appid
           FROM store_products p
           WHERE p.owned = 1 AND p.kind = 'game' AND (?1 IS NULL OR p.store = ?1))
         SELECT lib.store, lib.product_id, lib.title, lib.url, lib.cover, lib.cover_wide,
                lib.win, lib.mac, lib.linux, lib.appid, g.asset_format, g.img_header, g.img_capsule
         FROM lib LEFT JOIN games g ON g.appid = lib.appid
         ORDER BY lib.title COLLATE NOCASE, lib.store"
    ))?;
    let items = stmt
        .query_map(params![store.map(Store::as_str)], |r| {
            let store: String = r.get(0)?;
            let format: Option<String> = r.get(10)?;
            let header: Option<String> = r.get(11)?;
            let capsule: Option<String> = r.get(12)?;
            Ok(LibraryItem {
                store: Store::parse(&store).unwrap_or_default(),
                product_id: r.get(1)?,
                title: r.get(2)?,
                url: r.get(3)?,
                cover: r.get(4)?,
                cover_wide: r.get(5)?,
                win: r.get(6)?,
                mac: r.get(7)?,
                linux: r.get(8)?,
                appid: r.get(9)?,
                steam_header: asset_url(format.as_deref(), header.as_deref()),
                steam_capsule: asset_url(format.as_deref(), capsule.as_deref()),
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(items)
}

/// A Steam game's header image.
pub fn steam_header(conn: &Connection, appid: u32) -> Result<Option<String>> {
    let row: Option<(Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT asset_format, img_header FROM games WHERE appid = ?1",
            params![appid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(row.and_then(|(format, header)| asset_url(format.as_deref(), header.as_deref())))
}

/// A product's title and the Steam game it is confidently tied to (for a download's record).
pub fn product_summary(
    conn: &Connection,
    store: Store,
    product_id: &str,
) -> Result<Option<(String, Option<u32>)>> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT p.title, (SELECT m.appid FROM store_matches m
                                  WHERE m.store = p.store AND m.product_id = p.product_id
                                    AND {CONFIDENT_SQL}
                                  ORDER BY m.score DESC, m.appid LIMIT 1)
                 FROM store_products p WHERE p.store = ?1 AND p.product_id = ?2"
            ),
            params![store.as_str(), product_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

/// Distinct Steam games with a confident match in `store`.
pub fn matched_games(conn: &Connection, store: Store) -> Result<u32> {
    Ok(conn.query_row(
        &format!(
            "SELECT COUNT(DISTINCT m.appid) FROM store_matches m WHERE m.store = ?1 AND {CONFIDENT_SQL}"
        ),
        params![store.as_str()],
        |r| r.get(0),
    )?)
}

/// Sidebar counts. The per-store numbers follow the grid's default filter (listed, not adult).
pub fn store_counts(db: &mut Db) -> Result<StoreCounts> {
    let mut per_store = |store: Store| {
        db.cached_count(&format!("stores:{}", store.as_str()), |conn| {
            Ok(conn.query_row(
                &format!(
                    "SELECT COUNT(DISTINCT m.appid) FROM store_matches m
                     JOIN games g ON g.appid = m.appid AND g.delisted = 0 AND g.adult = 0
                     WHERE m.store = ?1 AND {CONFIDENT_SQL}"
                ),
                params![store.as_str()],
                |r| r.get(0),
            )?)
        })
    };
    let gog = per_store(Store::Gog)?;
    let itch = per_store(Store::Itch)?;
    let conn = db.conn();
    let owned = conn.query_row(
        "SELECT COUNT(*) FROM store_products WHERE owned = 1",
        [],
        |r| r.get(0),
    )?;
    let gog_products = conn.query_row(
        "SELECT COUNT(*) FROM store_products WHERE store = 'gog'",
        [],
        |r| r.get(0),
    )?;
    Ok(StoreCounts {
        gog,
        itch,
        owned,
        gog_products,
        last_store_sync_at: get_meta_i64(conn, meta_keys::LAST_STORE_SYNC_AT)?,
    })
}

fn json_list(items: &[String]) -> String {
    serde_json::to_string(items).unwrap_or_else(|_| "[]".into())
}

fn parse_list(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confident_sql_matches_the_threshold() {
        assert!(CONFIDENT_SQL.contains(&format!("{:.2}", matching::CONFIDENT)));
    }
}
