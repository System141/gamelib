//! Talks to the real Steam API. Ignored by default: `cargo test -p gamelib-core -- --ignored`.

use std::sync::atomic::AtomicBool;

use gamelib_core::record::GameRecord;
use gamelib_core::steam::{CatalogSource, PageRequest, SORT_RELEASE_DESC, SteamClient};

#[test]
#[ignore = "needs network access to api.steampowered.com"]
fn live_tags_page_and_media() {
    let client = SteamClient::new().unwrap();
    let cancel = AtomicBool::new(false);

    let tags = client.tags(&cancel).unwrap();
    assert!(tags.len() > 300, "{} tags", tags.len());
    assert!(tags.iter().any(|t| t.name == "Aksiyon"));

    let page = client
        .page(
            PageRequest {
                start: 0,
                count: 50,
                sort: SORT_RELEASE_DESC,
            },
            &cancel,
        )
        .unwrap();
    assert!(page.total > 100_000, "{} games", page.total);
    assert_eq!(page.returned, 50);
    let records: Vec<GameRecord> = page
        .items
        .iter()
        .filter_map(GameRecord::from_item)
        .collect();
    assert!(records.len() >= 48);
    let dates: Vec<i64> = records.iter().filter_map(|r| r.release_date).collect();
    assert!(dates.windows(2).all(|w| w[0] >= w[1]), "newest first");

    let media = client.fetch_media(1245620, &cancel).unwrap();
    assert!(!media.screenshots.is_empty());
    assert!(media.screenshots[0].thumb.contains(".600x338.jpg"));
}
