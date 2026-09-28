//! Catalog storage and queries on an in-memory database.

mod common;

use common::{DAY, item};
use gamelib_core::db::Db;
use gamelib_core::db::read::{get_game, list_tags, query_games, status};
use gamelib_core::db::write::{mark_delisted, refresh_tag_counts, upsert_games, upsert_tags};
use gamelib_core::model::{DeckFilter, GameQuery, Platform, SortKey};
use gamelib_core::record::GameRecord;
use gamelib_core::steam::types::{PurchaseOption, StoreItem};

const NOW: i64 = 1_790_000_000;

fn store(db: &mut Db, items: &[StoreItem], at: i64) {
    let records: Vec<GameRecord> = items.iter().filter_map(GameRecord::from_item).collect();
    upsert_games(db.conn(), &records, at).unwrap();
    upsert_tags(db.conn(), &common::tag_list()).unwrap();
    refresh_tag_counts(db.conn()).unwrap();
}

fn sample_db() -> Db {
    let mut db = Db::open_in_memory().unwrap();
    let mut witcher = item(
        292030,
        "The Witcher 3: Wild Hunt",
        NOW - 3000 * DAY,
        800_000,
        &[21, 19],
    );
    witcher
        .platforms
        .as_mut()
        .unwrap()
        .steam_deck_compat_category = Some(3);
    let mut bg3 = item(
        1086940,
        "Baldur's Gate 3",
        NOW - 700 * DAY,
        600_000,
        &[21, 9],
    );
    bg3.platforms.as_mut().unwrap().mac = true;
    bg3.platforms.as_mut().unwrap().steam_deck_compat_category = Some(3);
    let mut stalker = item(
        4500,
        "S.T.A.L.K.E.R.: Shadow of Chernobyl",
        NOW - 6000 * DAY,
        50_000,
        &[19],
    );
    stalker
        .platforms
        .as_mut()
        .unwrap()
        .steam_deck_compat_category = Some(2);
    let mut dota = item(570, "Dota 2", NOW - 4000 * DAY, 2_700_000, &[113, 9]);
    dota.is_free = true;
    dota.best_purchase_option = None;
    let mut fresh = item(5_000_000, "Brand New Indie", NOW - DAY, 3, &[492]);
    fresh.best_purchase_option = Some(PurchaseOption {
        final_price_in_cents: Some(499),
        formatted_final_price: Some("$4.99".into()),
        ..Default::default()
    });
    let mut adult = item(6_000_000, "Hidden Adult Game", NOW - 2 * DAY, 10, &[492]);
    adult.content_descriptorids = vec![3, 5];
    let witcher_dlc_like = item(
        292031,
        "Witcher Adventure Game",
        NOW - 2500 * DAY,
        2_000,
        &[9],
    );
    store(
        &mut db,
        &[witcher, bg3, stalker, dota, fresh, adult, witcher_dlc_like],
        NOW,
    );
    db
}

fn names(db: &mut Db, q: &GameQuery) -> Vec<String> {
    query_games(db, q, NOW)
        .unwrap()
        .items
        .into_iter()
        .map(|g| g.name)
        .collect()
}

fn search(text: &str) -> GameQuery {
    GameQuery {
        search: Some(text.into()),
        sort: SortKey::Relevance,
        ..Default::default()
    }
}

#[test]
fn search_finds_normalized_names() {
    let mut db = sample_db();
    assert_eq!(names(&mut db, &search("baldurs")), ["Baldur's Gate 3"]);
    assert_eq!(
        names(&mut db, &search("stalker")),
        ["S.T.A.L.K.E.R.: Shadow of Chernobyl"]
    );
    assert_eq!(
        names(&mut db, &search("BALDUR'S gate")),
        ["Baldur's Gate 3"]
    );
    assert_eq!(
        names(&mut db, &search("wit")),
        ["The Witcher 3: Wild Hunt", "Witcher Adventure Game"],
        "800k reviews outweigh the other game's name prefix"
    );
    assert_eq!(
        names(&mut db, &search("570")),
        ["Dota 2"],
        "digits also match the app id"
    );
    assert!(names(&mut db, &search("zzzz")).is_empty());
}

#[test]
fn relevance_balances_match_quality_and_popularity() {
    let mut db = sample_db();
    // Equally popular: exact name, then name prefix, then a match elsewhere in the name.
    store(
        &mut db,
        &[
            item(1, "Portal Knights Arena", NOW, 500, &[9]),
            item(2, "Portal", NOW, 500, &[9]),
            item(3, "Super Portal", NOW, 500, &[9]),
        ],
        NOW,
    );
    assert_eq!(
        names(&mut db, &search("portal")),
        ["Portal", "Portal Knights Arena", "Super Portal"]
    );
    // An obscure exact match does not bury a hugely popular game.
    store(&mut db, &[item(4, "Witcher", NOW, 1, &[9])], NOW);
    assert_eq!(
        names(&mut db, &search("witcher")),
        [
            "The Witcher 3: Wild Hunt",
            "Witcher Adventure Game",
            "Witcher"
        ]
    );
}

#[test]
fn oldest_sort_skips_undated_games() {
    let mut db = sample_db();
    let mut undated = item(8, "No Date Yet", NOW, 5, &[9]);
    undated.release = None;
    store(&mut db, &[undated], NOW);
    let q = GameQuery {
        sort: SortKey::Oldest,
        ..Default::default()
    };
    let oldest = names(&mut db, &q);
    assert_eq!(oldest[0], "S.T.A.L.K.E.R.: Shadow of Chernobyl");
    assert!(!oldest.contains(&"No Date Yet".to_string()));
    let newest = GameQuery {
        sort: SortKey::Newest,
        ..Default::default()
    };
    assert_eq!(names(&mut db, &newest).last().unwrap(), "No Date Yet");
}

#[test]
fn adult_games_hidden_by_default() {
    let mut db = sample_db();
    let all = GameQuery::default();
    assert!(!names(&mut db, &all).contains(&"Hidden Adult Game".to_string()));
    let with_adult = GameQuery {
        show_adult: true,
        ..Default::default()
    };
    assert!(names(&mut db, &with_adult).contains(&"Hidden Adult Game".to_string()));
}

#[test]
fn sorts() {
    let mut db = sample_db();
    let q = |sort| GameQuery {
        sort,
        ..Default::default()
    };
    assert_eq!(names(&mut db, &q(SortKey::Popular))[0], "Dota 2");
    assert_eq!(names(&mut db, &q(SortKey::Newest))[0], "Brand New Indie");
    assert_eq!(
        names(&mut db, &q(SortKey::Oldest))[0],
        "S.T.A.L.K.E.R.: Shadow of Chernobyl"
    );
    assert_eq!(names(&mut db, &q(SortKey::Name))[0], "Baldur's Gate 3");
    let rated = names(&mut db, &q(SortKey::Rating));
    assert_eq!(
        rated.last().unwrap(),
        "Brand New Indie",
        "3 reviews rank below big games at the same %"
    );
    // Relevance without a search falls back to popularity.
    assert_eq!(names(&mut db, &q(SortKey::Relevance))[0], "Dota 2");
}

#[test]
fn filters() {
    let mut db = sample_db();
    let tags = GameQuery {
        tags: vec![21, 19],
        ..Default::default()
    };
    assert_eq!(
        names(&mut db, &tags),
        ["The Witcher 3: Wild Hunt"],
        "all tags must match"
    );
    let mac = GameQuery {
        platforms: vec![Platform::Mac],
        ..Default::default()
    };
    assert_eq!(names(&mut db, &mac), ["Baldur's Gate 3"]);
    let verified = GameQuery {
        deck: Some(DeckFilter::Verified),
        ..Default::default()
    };
    assert_eq!(names(&mut db, &verified).len(), 2);
    let playable = GameQuery {
        deck: Some(DeckFilter::Playable),
        ..Default::default()
    };
    assert_eq!(names(&mut db, &playable).len(), 3);
    let free = GameQuery {
        free_only: true,
        ..Default::default()
    };
    assert_eq!(names(&mut db, &free), ["Dota 2"]);
    let recent = GameQuery {
        released_within_days: Some(7),
        ..Default::default()
    };
    assert_eq!(names(&mut db, &recent), ["Brand New Indie"]);
    let well_rated = GameQuery {
        min_review_score: Some(8),
        ..Default::default()
    };
    assert!(
        names(&mut db, &well_rated).is_empty(),
        "fixture scores are 7"
    );
}

#[test]
fn paging_is_stable_and_counted() {
    let mut db = sample_db();
    let page = |offset| GameQuery {
        offset,
        limit: 2,
        sort: SortKey::Name,
        ..Default::default()
    };
    let first = query_games(&mut db, &page(0), NOW).unwrap();
    let second = query_games(&mut db, &page(2), NOW).unwrap();
    assert_eq!(first.total, 6);
    assert_eq!(second.total, 6);
    assert_eq!(first.items.len(), 2);
    assert!(
        first
            .items
            .iter()
            .all(|a| second.items.iter().all(|b| a.appid != b.appid))
    );
}

#[test]
fn cards_have_image_urls_and_prices() {
    let mut db = sample_db();
    let page = query_games(&mut db, &search("brand new"), NOW).unwrap();
    let card = &page.items[0];
    assert_eq!(
        card.capsule.as_deref(),
        Some(
            "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/5000000/library_capsule.jpg?t=1"
        )
    );
    assert_eq!(card.capsule_2x, None);
    assert!(card.header.as_deref().unwrap().ends_with("/header.jpg?t=1"));
    assert_eq!(card.price.as_deref(), Some("$4.99"));
    assert_eq!(card.top_tags, [492]);
    assert_eq!(card.link_count, 0);
}

#[test]
fn rename_updates_search_index() {
    let mut db = sample_db();
    store(
        &mut db,
        &[item(
            570,
            "Dota Two Reborn",
            NOW - 4000 * DAY,
            2_700_000,
            &[113],
        )],
        NOW + 10,
    );
    assert_eq!(names(&mut db, &search("reborn")), ["Dota Two Reborn"]);
    assert!(names(&mut db, &search("dota 2")).is_empty());
    db.conn()
        .execute(
            "INSERT INTO games_fts(games_fts, rank) VALUES ('integrity-check', 1)",
            [],
        )
        .unwrap();
}

#[test]
fn tag_triggers_follow_updates_and_deletes() {
    let mut db = sample_db();
    let count = |db: &Db, tag: u32| -> i64 {
        db.conn()
            .query_row(
                "SELECT COUNT(*) FROM game_tags WHERE tagid = ?1",
                [tag],
                |r| r.get(0),
            )
            .unwrap()
    };
    assert_eq!(count(&db, 113), 1);
    store(
        &mut db,
        &[item(570, "Dota 2", NOW - 4000 * DAY, 2_700_000, &[9])],
        NOW + 10,
    );
    assert_eq!(count(&db, 113), 0, "tag removed from game");
    db.conn()
        .execute("DELETE FROM games WHERE appid = 570", [])
        .unwrap();
    assert_eq!(names(&mut db, &search("dota")).len(), 0);
    db.conn()
        .execute(
            "INSERT INTO games_fts(games_fts, rank) VALUES ('integrity-check', 1)",
            [],
        )
        .unwrap();
}

#[test]
fn delisted_games_are_hidden_but_kept() {
    let mut db = sample_db();
    store(
        &mut db,
        &[item(570, "Dota 2", NOW - 4000 * DAY, 2_700_000, &[113])],
        NOW + 100,
    );
    let hidden = mark_delisted(db.conn(), NOW + 100).unwrap();
    assert_eq!(hidden, 6, "everything not refreshed in the run");
    assert_eq!(names(&mut db, &GameQuery::default()), ["Dota 2"]);
    let kept = get_game(db.conn(), 1086940).unwrap().unwrap();
    assert!(kept.delisted);
    // Seen again later: listed again.
    store(
        &mut db,
        &[item(
            1086940,
            "Baldur's Gate 3",
            NOW - 700 * DAY,
            600_000,
            &[21],
        )],
        NOW + 200,
    );
    assert!(!get_game(db.conn(), 1086940).unwrap().unwrap().delisted);
}

#[test]
fn detail_and_tags_and_status() {
    let mut db = sample_db();
    let detail = get_game(db.conn(), 292030).unwrap().unwrap();
    assert_eq!(detail.card.name, "The Witcher 3: Wild Hunt");
    assert_eq!(detail.developers, ["Dev Studio"]);
    assert_eq!(detail.tags, [21, 19]);
    assert_eq!(
        detail.store_url,
        "https://store.steampowered.com/app/292030/"
    );
    assert_eq!(detail.first_seen_at, NOW);
    assert!(get_game(db.conn(), 1).unwrap().is_none());

    let tags = list_tags(db.conn()).unwrap();
    let action = tags.iter().find(|t| t.tagid == 19).unwrap();
    assert_eq!((action.name.as_str(), action.game_count), ("Aksiyon", 2));
    assert!(tags.iter().all(|t| t.game_count > 0));

    let st = status(&mut db).unwrap();
    assert_eq!(
        st.game_count, 7,
        "status counts every listed game, adult included"
    );
    assert_eq!(st.linked_game_count, 0);
    assert!(!st.resumable);
}

#[test]
fn count_cache_sees_new_rows() {
    let mut db = sample_db();
    let q = GameQuery::default();
    assert_eq!(query_games(&mut db, &q, NOW).unwrap().total, 6);
    store(
        &mut db,
        &[item(7_000_000, "Another One", NOW, 1, &[19])],
        NOW,
    );
    assert_eq!(query_games(&mut db, &q, NOW).unwrap().total, 7);
}
