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

CREATE INDEX idx_games_popular ON games(review_count DESC, appid);
CREATE INDEX idx_games_rating  ON games(rating DESC, review_count DESC, appid);
CREATE INDEX idx_games_release ON games(release_date DESC, appid DESC);
CREATE INDEX idx_games_name    ON games(search_name, appid);

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

const MIGRATIONS: &[&str] = &[MIGRATION_1];

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
