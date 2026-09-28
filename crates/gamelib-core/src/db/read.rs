//! Catalog queries for the UI.

use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};

use crate::Result;
use crate::db::Db;
use crate::db::stores::{CONFIDENT_SQL, store_counts};
use crate::db::write::{get_meta_i64, meta_keys};
use crate::model::{
    CatalogStatus, DeckFilter, GameCard, GameDetail, GamePage, GameQuery, Platform, SortKey, Store,
    TagInfo,
};
use crate::search::{fts_query, normalize, parse_appid};
use crate::steam::assets::asset_url;

pub const DEFAULT_PAGE_SIZE: u32 = 60;
pub const MAX_PAGE_SIZE: u32 = 200;
const MAX_TAG_FILTERS: usize = 10;
const TOP_TAGS: usize = 3;

const CARD_COLUMNS: &str = "g.appid, g.name, g.asset_format, g.img_capsule, g.img_capsule_2x, g.img_header,
  g.release_date, g.is_free, g.is_early_access, g.price_formatted, g.original_price_formatted, g.discount_pct,
  g.review_score, g.review_pct, g.review_count, g.win, g.mac, g.linux, g.deck_compat, g.tagids,
  (SELECT COUNT(*) FROM game_links l WHERE l.appid = g.appid) AS link_count,
  (SELECT group_concat(DISTINCT m.store) FROM store_matches m
   WHERE m.appid = g.appid AND m.state != 'rejected' AND (m.state = 'confirmed' OR m.score >= 0.85)) AS stores";

/// Number of columns in [`CARD_COLUMNS`]; detail columns follow.
const CARD_WIDTH: usize = 22;

struct Filter {
    sql: String,
    params: Vec<Value>,
}

fn build_filter(q: &GameQuery, now: i64) -> Filter {
    let mut conds: Vec<String> = vec!["g.delisted = 0".into()];
    let mut params: Vec<Value> = Vec::new();

    if !q.show_adult {
        conds.push("g.adult = 0".into());
    }
    if let Some(search) = q.search.as_deref()
        && let Some(fts) = fts_query(search)
    {
        match parse_appid(search) {
            Some(appid) => {
                conds.push("(g.appid IN (SELECT rowid FROM games_fts WHERE games_fts MATCH ?) OR g.appid = ?)".into());
                params.push(Value::Text(fts));
                params.push(Value::Integer(i64::from(appid)));
            }
            None => {
                conds.push(
                    "g.appid IN (SELECT rowid FROM games_fts WHERE games_fts MATCH ?)".into(),
                );
                params.push(Value::Text(fts));
            }
        }
    }
    let mut tags = q.tags.clone();
    tags.sort_unstable();
    tags.dedup();
    for tag in tags.into_iter().take(MAX_TAG_FILTERS) {
        conds.push(
            "EXISTS (SELECT 1 FROM game_tags t WHERE t.tagid = ? AND t.appid = g.appid)".into(),
        );
        params.push(Value::Integer(i64::from(tag)));
    }
    for platform in &q.platforms {
        conds.push(
            match platform {
                Platform::Win => "g.win = 1",
                Platform::Mac => "g.mac = 1",
                Platform::Linux => "g.linux = 1",
            }
            .into(),
        );
    }
    match q.deck {
        Some(DeckFilter::Playable) => conds.push("g.deck_compat >= 2".into()),
        Some(DeckFilter::Verified) => conds.push("g.deck_compat = 3".into()),
        None => {}
    }
    if q.free_only {
        conds.push("g.is_free = 1".into());
    }
    if let Some(score) = q.min_review_score.filter(|&s| s > 0) {
        conds.push("g.review_score >= ?".into());
        params.push(Value::Integer(i64::from(score.min(9))));
    }
    if let Some(days) = q.released_within_days.filter(|&d| d > 0) {
        // Rounded to the hour so the count cache stays warm between requests.
        let cutoff = (now - i64::from(days) * 86_400).div_euclid(3600) * 3600;
        conds.push("g.release_date >= ?".into());
        params.push(Value::Integer(cutoff));
    }
    if q.has_links {
        conds.push("EXISTS (SELECT 1 FROM game_links l WHERE l.appid = g.appid)".into());
    }
    let mut stores = q.stores.clone();
    stores.sort_unstable();
    stores.dedup();
    if !stores.is_empty() {
        let marks = vec!["?"; stores.len()].join(", ");
        conds.push(format!(
            "g.appid IN (SELECT m.appid FROM store_matches m WHERE m.store IN ({marks}) AND {CONFIDENT_SQL})"
        ));
        params.extend(stores.iter().map(|s| Value::Text(s.as_str().into())));
    }
    if q.owned {
        conds.push(format!(
            "g.appid IN (SELECT m.appid FROM store_matches m JOIN store_products p
               ON p.store = m.store AND p.product_id = m.product_id
             WHERE p.owned = 1 AND {CONFIDENT_SQL})"
        ));
    }
    if q.sort == SortKey::Oldest {
        // A handful of games have no release date; they would otherwise all lead the list.
        conds.push("g.release_date IS NOT NULL".into());
    }
    Filter {
        sql: conds.join(" AND "),
        params,
    }
}

/// ORDER BY clause and its parameters. Every order ends with the app id so paging is stable.
fn build_order(q: &GameQuery) -> (String, Vec<Value>) {
    let search = q.search.as_deref().map(normalize).filter(|s| !s.is_empty());
    let sort = match (q.sort, &search) {
        (SortKey::Relevance, None) => SortKey::Popular,
        (sort, _) => sort,
    };
    match sort {
        SortKey::Relevance => {
            // Match quality (exact 0, name prefix 2, elsewhere 4) minus two points per order of
            // magnitude of reviews: an obscure exact match must not bury a hugely popular game
            // ("stalker" should show S.T.A.L.K.E.R. before a 15-review game named "Stalker").
            let term = search.unwrap_or_default();
            (
                "(CASE WHEN g.search_name = ? THEN 0 WHEN g.search_name LIKE ? ESCAPE '\\' THEN 2 ELSE 4 END)
                 - (CASE WHEN g.review_count >= 1000000 THEN 12 WHEN g.review_count >= 100000 THEN 10
                         WHEN g.review_count >= 10000 THEN 8 WHEN g.review_count >= 1000 THEN 6
                         WHEN g.review_count >= 100 THEN 4 WHEN g.review_count >= 10 THEN 2 ELSE 0 END),
                 g.review_count DESC, g.appid"
                    .into(),
                vec![
                    Value::Text(term.clone()),
                    Value::Text(format!("{}%", escape_like(&term))),
                ],
            )
        }
        SortKey::Popular => ("g.review_count DESC, g.appid".into(), vec![]),
        SortKey::Rating => ("g.rating DESC, g.review_count DESC, g.appid".into(), vec![]),
        SortKey::Newest => ("g.release_date DESC, g.appid DESC".into(), vec![]),
        SortKey::Oldest => ("g.release_date ASC, g.appid ASC".into(), vec![]),
        SortKey::Name => ("g.search_name, g.appid".into(), vec![]),
    }
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

pub fn query_games(db: &mut Db, q: &GameQuery, now: i64) -> Result<GamePage> {
    let filter = build_filter(q, now);
    let count_key = format!("{}|{:?}", filter.sql, filter.params);
    let total = db.cached_count(&count_key, |conn| {
        let sql = format!("SELECT COUNT(*) FROM games g WHERE {}", filter.sql);
        Ok(
            conn.query_row(&sql, params_from_iter(filter.params.iter()), |r| {
                r.get::<_, u32>(0)
            })?,
        )
    })?;

    let limit = match q.limit {
        0 => DEFAULT_PAGE_SIZE,
        n => n.min(MAX_PAGE_SIZE),
    };
    let (order, order_params) = build_order(q);
    let sql = format!(
        "SELECT {CARD_COLUMNS} FROM games g WHERE {} ORDER BY {order} LIMIT ? OFFSET ?",
        filter.sql
    );
    let mut params = filter.params;
    params.extend(order_params);
    params.push(Value::Integer(i64::from(limit)));
    params.push(Value::Integer(i64::from(q.offset)));

    let conn = db.conn();
    let mut stmt = conn.prepare_cached(&sql)?;
    let items = stmt
        .query_map(params_from_iter(params.iter()), card_from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(GamePage { total, items })
}

fn card_from_row(row: &Row<'_>) -> rusqlite::Result<GameCard> {
    let format: Option<String> = row.get(2)?;
    let img = |i: usize| -> rusqlite::Result<Option<String>> {
        let file: Option<String> = row.get(i)?;
        Ok(asset_url(format.as_deref(), file.as_deref()))
    };
    let tagids: String = row.get(19)?;
    let mut top_tags: Vec<u32> = serde_json::from_str(&tagids).unwrap_or_default();
    top_tags.truncate(TOP_TAGS);
    Ok(GameCard {
        appid: row.get(0)?,
        name: row.get(1)?,
        capsule: img(3)?,
        capsule_2x: img(4)?,
        header: img(5)?,
        release_date: row.get(6)?,
        is_free: row.get(7)?,
        is_early_access: row.get(8)?,
        price: row.get(9)?,
        original_price: row.get(10)?,
        discount_pct: row.get(11)?,
        review_score: row.get(12)?,
        review_pct: row.get(13)?,
        review_count: row.get(14)?,
        win: row.get(15)?,
        mac: row.get(16)?,
        linux: row.get(17)?,
        deck: row.get(18)?,
        top_tags,
        link_count: row.get(20)?,
        stores: parse_stores(row.get::<_, Option<String>>(21)?.as_deref()),
    })
}

/// `group_concat` of store ids, in a stable order.
fn parse_stores(list: Option<&str>) -> Vec<Store> {
    let mut stores: Vec<Store> = list
        .unwrap_or_default()
        .split(',')
        .filter_map(Store::parse)
        .collect();
    stores.sort_unstable();
    stores.dedup();
    stores
}

pub fn get_game(conn: &Connection, appid: u32) -> Result<Option<GameDetail>> {
    let sql = format!(
        "SELECT {CARD_COLUMNS}, g.short_description, g.developers, g.publishers, g.franchises, g.tagids,
                g.descriptors, g.original_release_date, g.img_hero, g.adult, g.delisted, g.first_seen_at, g.synced_at
         FROM games g WHERE g.appid = ?1"
    );
    let detail = conn
        .query_row(&sql, params![appid], |row| {
            let card = card_from_row(row)?;
            let json_list = |i: usize| -> rusqlite::Result<Vec<String>> {
                let s: String = row.get(i)?;
                Ok(serde_json::from_str(&s).unwrap_or_default())
            };
            let json_ids = |i: usize| -> rusqlite::Result<Vec<u32>> {
                let s: String = row.get(i)?;
                Ok(serde_json::from_str(&s).unwrap_or_default())
            };
            let format: Option<String> = row.get(2)?;
            let hero_file: Option<String> = row.get(CARD_WIDTH + 7)?;
            Ok(GameDetail {
                short_description: row.get(CARD_WIDTH)?,
                developers: json_list(CARD_WIDTH + 1)?,
                publishers: json_list(CARD_WIDTH + 2)?,
                franchises: json_list(CARD_WIDTH + 3)?,
                tags: json_ids(CARD_WIDTH + 4)?,
                descriptors: json_ids(CARD_WIDTH + 5)?,
                original_release_date: row.get(CARD_WIDTH + 6)?,
                hero: asset_url(format.as_deref(), hero_file.as_deref()),
                store_url: format!("https://store.steampowered.com/app/{}/", card.appid),
                adult: row.get(CARD_WIDTH + 8)?,
                delisted: row.get(CARD_WIDTH + 9)?,
                first_seen_at: row.get(CARD_WIDTH + 10)?,
                synced_at: row.get(CARD_WIDTH + 11)?,
                card,
            })
        })
        .optional()?;
    Ok(detail)
}

/// Tags that have at least one visible game, most used first.
pub fn list_tags(conn: &Connection) -> Result<Vec<TagInfo>> {
    let mut stmt =
        conn.prepare_cached("SELECT tagid, name, game_count FROM tags WHERE game_count > 0 ORDER BY game_count DESC, name")?;
    let tags = stmt
        .query_map([], |r| {
            Ok(TagInfo {
                tagid: r.get(0)?,
                name: r.get(1)?,
                game_count: r.get(2)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(tags)
}

pub fn status(db: &mut Db) -> Result<CatalogStatus> {
    let store_counts = store_counts(db)?;
    let game_count = db.cached_count("status:games", |conn| {
        Ok(
            conn.query_row("SELECT COUNT(*) FROM games WHERE delisted = 0", [], |r| {
                r.get(0)
            })?,
        )
    })?;
    let conn = db.conn();
    let tag_count = conn.query_row("SELECT COUNT(*) FROM tags WHERE game_count > 0", [], |r| {
        r.get(0)
    })?;
    let linked_game_count =
        conn.query_row("SELECT COUNT(DISTINCT appid) FROM game_links", [], |r| {
            r.get(0)
        })?;
    Ok(CatalogStatus {
        game_count,
        tag_count,
        linked_game_count,
        last_sync_at: get_meta_i64(conn, meta_keys::LAST_SYNC_AT)?,
        last_new_releases_at: get_meta_i64(conn, meta_keys::LAST_NEW_RELEASES_AT)?,
        resumable: get_meta_i64(conn, meta_keys::SYNC_CURSOR)?.is_some(),
        store_counts,
    })
}
