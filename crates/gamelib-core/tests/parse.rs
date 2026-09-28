//! Parsing real (trimmed) Steam responses saved in `tests/fixtures`.

use gamelib_core::record::GameRecord;
use gamelib_core::steam::types::{QueryEnvelope, TagListEnvelope, parse_items};

fn records() -> Vec<GameRecord> {
    let env: QueryEnvelope =
        serde_json::from_str(include_str!("fixtures/query_page.json")).unwrap();
    assert!(env.response.metadata.unwrap().total_matching_records > 100_000);
    let (items, skipped) = parse_items(env.response.store_items);
    assert_eq!(skipped, 0);
    items.iter().filter_map(GameRecord::from_item).collect()
}

fn by_id(records: &[GameRecord], appid: u32) -> &GameRecord {
    records
        .iter()
        .find(|r| r.appid == appid)
        .unwrap_or_else(|| panic!("fixture lacks {appid}"))
}

#[test]
fn all_fixture_items_convert() {
    assert_eq!(records().len(), 8);
}

#[test]
fn bundle_price_as_string_and_missing_description() {
    let rs = records();
    let cs = by_id(&rs, 10);
    assert_eq!(cs.name, "Counter-Strike");
    assert_eq!(cs.search_name, "counter strike");
    assert_eq!(cs.price_cents, Some(579));
    assert_eq!(cs.price_formatted.as_deref(), Some("$5.79"));
    assert_eq!(cs.original_price_formatted, None);
    assert_eq!(cs.short_description, None);
    assert_eq!(cs.release_date, Some(973_065_600));
    assert!(cs.win && cs.mac && cs.linux);
    assert_eq!(cs.review_score, 9);
    assert!(cs.rating > 0.9);
    assert!(!cs.adult);
    assert_eq!(cs.descriptors, "[2,5]");
}

#[test]
fn free_game_without_purchase_option() {
    let rs = records();
    let tf2 = by_id(&rs, 440);
    assert!(tf2.is_free);
    assert_eq!(tf2.price_formatted, None);
    assert!(
        tf2.win && tf2.linux && !tf2.mac,
        "omitted `mac` must default to false"
    );
    assert_eq!(tf2.deck_compat, 2);
}

#[test]
fn discount_keeps_original_price() {
    let rs = records();
    let discounted = by_id(&rs, 1670);
    assert_eq!(discounted.discount_pct, 80);
    assert_eq!(discounted.price_formatted.as_deref(), Some("$0.71"));
    assert_eq!(
        discounted.original_price_formatted.as_deref(),
        Some("$3.59")
    );
    assert_eq!(
        discounted.name, "Iron Warriors: T-72 Tank Command",
        "trailing space trimmed"
    );
}

#[test]
fn no_reviews_rate_zero() {
    let rs = records();
    let unreviewed = by_id(&rs, 1620);
    assert_eq!(unreviewed.review_count, 0);
    assert_eq!(unreviewed.review_score, 0);
    assert_eq!(unreviewed.rating, 0.0);
}

#[test]
fn missing_capsule_keeps_header() {
    let rs = records();
    let g = by_id(&rs, 23455);
    assert_eq!(g.img_capsule, None);
    assert_eq!(g.img_capsule_2x, None);
    assert_eq!(g.img_header.as_deref(), Some("header.jpg"));
    assert!(g.asset_format.as_deref().unwrap().contains("${FILENAME}"));
}

#[test]
fn early_access_and_adult_flags() {
    let rs = records();
    assert!(by_id(&rs, 4964730).is_early_access);
    let adult = rs.iter().find(|r| r.name == "Adult Fixture Game").unwrap();
    assert!(adult.adult);
    assert_eq!(adult.descriptors, "[1,3,4,5]");
}

#[test]
fn tags_are_ordered_by_weight() {
    let rs = records();
    let dota = by_id(&rs, 570);
    let tags: Vec<u32> = serde_json::from_str(&dota.tagids).unwrap();
    assert_eq!(tags.len(), 20);
    assert_eq!(tags[0], 113, "Free to Play is Dota's top tag");
}

#[test]
fn turkish_tag_names() {
    let env: TagListEnvelope =
        serde_json::from_str(include_str!("fixtures/tag_list.json")).unwrap();
    let names: Vec<&str> = env.response.tags.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"Strateji"));
    assert!(names.contains(&"Aksiyon"));
}

#[test]
fn malformed_items_are_skipped_not_fatal() {
    let raw = vec![
        serde_json::json!({"appid": 1, "name": "Ok"}),
        serde_json::json!({"appid": "not a number", "name": "Broken"}),
        serde_json::json!({"appid": 2, "success": 2}),
    ];
    let (items, skipped) = parse_items(raw);
    assert_eq!(skipped, 1);
    let records: Vec<GameRecord> = items.iter().filter_map(GameRecord::from_item).collect();
    assert_eq!(records.len(), 1, "success != 1 is dropped");
}
