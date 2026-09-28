//! Schema migrations, applied in order and tracked with `PRAGMA user_version`.

use rusqlite::Connection;

use crate::Result;

/// Catalog tables (rebuildable from Steam) and user data (`game_links`, never touched by syncs).
const MIGRATION_1: &str = r#"
CREATE TABLE meta(
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE tags(
  tagid      INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  game_count INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE games(
  appid                    INTEGER PRIMARY KEY,
  name                     TEXT NOT NULL,
  search_name              TEXT NOT NULL,
  short_description        TEXT,
  developers               TEXT NOT NULL DEFAULT '[]',
  publishers               TEXT NOT NULL DEFAULT '[]',
  franchises               TEXT NOT NULL DEFAULT '[]',
  tagids                   TEXT NOT NULL DEFAULT '[]',
  descriptors              TEXT NOT NULL DEFAULT '[]',
  release_date             INTEGER,
  original_release_date    INTEGER,
  is_free                  INTEGER NOT NULL DEFAULT 0,
  is_early_access          INTEGER NOT NULL DEFAULT 0,
  adult                    INTEGER NOT NULL DEFAULT 0,
  delisted                 INTEGER NOT NULL DEFAULT 0,
  price_cents              INTEGER,
  price_formatted          TEXT,
  original_price_formatted TEXT,
  discount_pct             INTEGER NOT NULL DEFAULT 0,
  review_count             INTEGER NOT NULL DEFAULT 0,
  review_pct               INTEGER NOT NULL DEFAULT 0,
  review_score             INTEGER NOT NULL DEFAULT 0,
  rating                   REAL NOT NULL DEFAULT 0,
  win                      INTEGER NOT NULL DEFAULT 0,
  mac                      INTEGER NOT NULL DEFAULT 0,
  linux                    INTEGER NOT NULL DEFAULT 0,
  deck_compat              INTEGER NOT NULL DEFAULT 0,
  asset_format             TEXT,
  img_header               TEXT,
  img_capsule              TEXT,
  img_capsule_2x           TEXT,
  img_hero                 TEXT,
  assets_modified          INTEGER,
  first_seen_at            INTEGER NOT NULL,
  synced_at                INTEGER NOT NULL
);

-- One index per sort order. The trailing delisted/adult columns let SQLite apply the default
-- visibility filter from the index alone, so deep pages skip rows without touching the table.
CREATE INDEX idx_games_popular ON games(review_count DESC, appid, delisted, adult);
CREATE INDEX idx_games_rating  ON games(rating DESC, review_count DESC, appid, delisted, adult);
CREATE INDEX idx_games_release ON games(release_date DESC, appid DESC, delisted, adult);
CREATE INDEX idx_games_name    ON games(search_name, appid, delisted, adult);

CREATE TABLE game_tags(
  tagid INTEGER NOT NULL,
  appid INTEGER NOT NULL,
  PRIMARY KEY (tagid, appid)
) WITHOUT ROWID;

-- External-content index over games.search_name; kept in sync by the triggers below.
CREATE VIRTUAL TABLE games_fts USING fts5(
  search_name,
  content = 'games',
  content_rowid = 'appid',
  tokenize = 'unicode61 remove_diacritics 2',
  prefix = '1 2 3'
);

CREATE TRIGGER games_ai AFTER INSERT ON games BEGIN
  INSERT INTO games_fts(rowid, search_name) VALUES (new.appid, new.search_name);
  INSERT OR IGNORE INTO game_tags(tagid, appid) SELECT value, new.appid FROM json_each(new.tagids);
END;

CREATE TRIGGER games_ad AFTER DELETE ON games BEGIN
  INSERT INTO games_fts(games_fts, rowid, search_name) VALUES ('delete', old.appid, old.search_name);
  DELETE FROM game_tags WHERE appid = old.appid AND tagid IN (SELECT value FROM json_each(old.tagids));
END;

CREATE TRIGGER games_au_name AFTER UPDATE OF search_name ON games
WHEN old.search_name IS NOT new.search_name BEGIN
  INSERT INTO games_fts(games_fts, rowid, search_name) VALUES ('delete', old.appid, old.search_name);
  INSERT INTO games_fts(rowid, search_name) VALUES (new.appid, new.search_name);
END;

CREATE TRIGGER games_au_tags AFTER UPDATE OF tagids ON games
WHEN old.tagids IS NOT new.tagids BEGIN
  DELETE FROM game_tags WHERE appid = old.appid AND tagid IN (SELECT value FROM json_each(old.tagids));
  INSERT OR IGNORE INTO game_tags(tagid, appid) SELECT value, new.appid FROM json_each(new.tagids);
END;

-- User data: links to non-Steam sources. No foreign key to games on purpose, so catalog
-- refreshes can never remove them.
CREATE TABLE game_links(
  id           INTEGER PRIMARY KEY,
  appid        INTEGER NOT NULL,
  site_id      TEXT NOT NULL,
  url          TEXT NOT NULL,
  label        TEXT,
  kind         TEXT NOT NULL DEFAULT 'download',
  platform     TEXT,
  version      TEXT,
  notes        TEXT,
  check_status TEXT,
  http_status  INTEGER,
  resolved_url TEXT,
  final_host   TEXT,
  redirects    INTEGER,
  file_name    TEXT,
  size_bytes   INTEGER,
  content_type TEXT,
  is_file      INTEGER,
  checked_at   INTEGER,
  created_at   INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL
);

CREATE INDEX idx_game_links_appid ON game_links(appid);
"#;

/// Other stores (GOG, itch.io): their products and which Steam games they match. Like links,
/// matches have no foreign key to games, and user decisions (`state`) survive every refresh.
const MIGRATION_2: &str = r#"
CREATE TABLE store_products(
  store               TEXT NOT NULL,
  product_id          TEXT NOT NULL,
  kind                TEXT NOT NULL DEFAULT 'game',
  title               TEXT NOT NULL,
  canonical_title     TEXT NOT NULL,
  slug                TEXT,
  url                 TEXT,
  developers          TEXT NOT NULL DEFAULT '[]',
  publishers          TEXT NOT NULL DEFAULT '[]',
  release_date        INTEGER,
  store_release_date  INTEGER,
  cover               TEXT,
  cover_wide          TEXT,
  win                 INTEGER NOT NULL DEFAULT 0,
  mac                 INTEGER NOT NULL DEFAULT 0,
  linux               INTEGER NOT NULL DEFAULT 0,
  price_formatted     TEXT,
  is_free             INTEGER NOT NULL DEFAULT 0,
  in_catalog          INTEGER NOT NULL DEFAULT 1,
  owned               INTEGER NOT NULL DEFAULT 0,
  owned_key           TEXT,
  external_checked_at INTEGER,
  seen_at             INTEGER NOT NULL,
  PRIMARY KEY (store, product_id)
) WITHOUT ROWID;

CREATE INDEX idx_store_products_title ON store_products(canonical_title);
CREATE INDEX idx_store_products_owned ON store_products(store, owned) WHERE owned = 1;

CREATE TABLE store_matches(
  store      TEXT NOT NULL,
  product_id TEXT NOT NULL,
  appid      INTEGER NOT NULL,
  method     TEXT NOT NULL,
  score      REAL NOT NULL,
  state      TEXT NOT NULL DEFAULT 'auto',
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (store, product_id, appid)
) WITHOUT ROWID;

CREATE INDEX idx_store_matches_appid ON store_matches(appid, store);

-- When a Steam game was last looked up in a store's cross-reference (GamesDB for GOG).
CREATE TABLE store_lookups(
  store      TEXT NOT NULL,
  appid      INTEGER NOT NULL,
  checked_at INTEGER NOT NULL,
  PRIMARY KEY (store, appid)
) WITHOUT ROWID;
"#;

/// The download queue. Files keep a stable reference (a GOG downlink, an itch.io upload id)
/// that is turned into a fresh signed address whenever a transfer (re)starts.
const MIGRATION_3: &str = r#"
CREATE TABLE downloads(
  id           INTEGER PRIMARY KEY,
  store        TEXT NOT NULL,
  product_id   TEXT NOT NULL,
  appid        INTEGER,
  title        TEXT NOT NULL,
  option_id    TEXT NOT NULL,
  option_label TEXT,
  platform     TEXT,
  state        TEXT NOT NULL,
  total_bytes  INTEGER NOT NULL DEFAULT 0,
  done_bytes   INTEGER NOT NULL DEFAULT 0,
  dir          TEXT NOT NULL DEFAULT '',
  error_kind   TEXT,
  error        TEXT,
  created_at   INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL,
  finished_at  INTEGER
);

CREATE INDEX idx_downloads_state ON downloads(state, created_at);

CREATE TABLE download_files(
  id            INTEGER PRIMARY KEY,
  download_id   INTEGER NOT NULL,
  position      INTEGER NOT NULL,
  source        TEXT NOT NULL,
  size          INTEGER,
  file_name     TEXT,
  md5           TEXT,
  etag          TEXT,
  last_modified TEXT,
  done          INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_download_files ON download_files(download_id, position);
"#;

const MIGRATIONS: &[&str] = &[MIGRATION_1, MIGRATION_2, MIGRATION_3];

/// Schema version this build expects.
pub const SCHEMA_VERSION: usize = MIGRATIONS.len();

pub fn migrate(conn: &mut Connection) -> Result<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current.max(0) as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (index + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}
