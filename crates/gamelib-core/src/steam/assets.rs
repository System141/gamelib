//! Steam CDN image URLs.
//!
//! Store items describe their images as `asset_url_format` (e.g.
//! `steam/apps/570/${FILENAME}?t=1769535998`) plus per-image file names, some of which include a
//! content hash (`<hash>/library_capsule.jpg`). Newer games only serve the hashed paths.

/// CDN base for store item assets. (The Cloudflare host answers with a redirect, so avoid it.)
pub const CDN: &str = "https://shared.akamai.steamstatic.com/store_item_assets/";

const PLACEHOLDER: &str = "${FILENAME}";

/// Builds a full image URL, or `None` when either part is missing.
pub fn asset_url(format: Option<&str>, file: Option<&str>) -> Option<String> {
    let format = format.filter(|f| f.contains(PLACEHOLDER))?;
    let file = file.map(str::trim).filter(|f| !f.is_empty())?;
    Some(format!("{CDN}{}", format.replacen(PLACEHOLDER, file, 1)))
}

/// Thumbnail (600×338) and large (1920×1080) URLs for a screenshot file such as
/// `steam/apps/1245620/ss_<hash>.jpg?t=1790290043`.
pub fn screenshot_urls(filename: &str) -> (String, String) {
    let (path, query) = match filename.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (filename, None),
    };
    let sized = |size: &str| {
        let resized = match path.strip_suffix(".jpg") {
            Some(base) => format!("{base}.{size}.jpg"),
            None => path.to_owned(),
        };
        match query {
            Some(q) => format!("{CDN}{resized}?{q}"),
            None => format!("{CDN}{resized}"),
        }
    };
    (sized("600x338"), sized("1920x1080"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORMAT: &str = "steam/apps/570/${FILENAME}?t=1769535998";

    #[test]
    fn builds_plain_and_hashed_urls() {
        assert_eq!(
            asset_url(Some(FORMAT), Some("header.jpg")).as_deref(),
            Some(
                "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/570/header.jpg?t=1769535998"
            )
        );
        assert_eq!(
            asset_url(
                Some(FORMAT),
                Some("6843027380c3bfd0952449fd9174f492ef2e7b40/library_capsule.jpg")
            )
            .as_deref(),
            Some(
                "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/570/6843027380c3bfd0952449fd9174f492ef2e7b40/library_capsule.jpg?t=1769535998"
            )
        );
    }

    #[test]
    fn missing_parts_give_none() {
        assert_eq!(asset_url(None, Some("header.jpg")), None);
        assert_eq!(asset_url(Some(FORMAT), None), None);
        assert_eq!(asset_url(Some(FORMAT), Some("  ")), None);
        assert_eq!(
            asset_url(Some("steam/apps/570/header.jpg"), Some("x.jpg")),
            None
        );
    }

    #[test]
    fn screenshot_sizes() {
        let (thumb, full) = screenshot_urls("steam/apps/1245620/ss_943b.jpg?t=1790290043");
        assert_eq!(
            thumb,
            "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/1245620/ss_943b.600x338.jpg?t=1790290043"
        );
        assert_eq!(
            full,
            "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/1245620/ss_943b.1920x1080.jpg?t=1790290043"
        );
        let (thumb, _) = screenshot_urls("steam/apps/1/ss_1.png");
        assert!(thumb.ends_with("ss_1.png"));
    }
}
