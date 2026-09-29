//! Steam CDN image URLs.
//!
//! Store items describe their images as `asset_url_format` (e.g.
//! `steam/apps/570/${FILENAME}?t=1769535998`) plus per-image file names, some of which include a
//! content hash (`<hash>/library_capsule.jpg`). Newer games only serve the hashed paths.

/// CDN base for store item assets. (The Cloudflare host answers with a redirect, so avoid it.)
pub const CDN: &str = "https://shared.akamai.steamstatic.com/store_item_assets/";
/// CDN base for trailer streams (HLS playlists and their segments, served with CORS).
pub const VIDEO_CDN: &str = "https://video.akamai.steamstatic.com/store_trailers/";

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

/// The HLS playlist of a trailer stream (`<appid>/<id>/<hash>/<ts>/hls_264_master.m3u8`), with
/// the cache-busting `t=` query of the trailer's URL format.
pub fn trailer_stream_url(cdn_path: &str, url_format: Option<&str>) -> Option<String> {
    let path = cdn_path.trim().trim_start_matches('/');
    if path.is_empty() || path.contains("..") || path.contains("://") {
        return None;
    }
    let query = url_format.and_then(|f| f.split_once('?')).map(|(_, q)| q);
    Some(match query {
        Some(q) => format!("{VIDEO_CDN}{path}?{q}"),
        None => format!("{VIDEO_CDN}{path}"),
    })
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

    #[test]
    fn trailer_streams() {
        assert_eq!(
            trailer_stream_url(
                "292030/1018852715/bf5e/1790683068/hls_264_master.m3u8",
                Some("steam/apps/${FILENAME}?t=1790693333")
            )
            .as_deref(),
            Some(
                "https://video.akamai.steamstatic.com/store_trailers/292030/1018852715/bf5e/1790683068/hls_264_master.m3u8?t=1790693333"
            )
        );
        assert_eq!(
            trailer_stream_url("1/2/hls.m3u8", None).as_deref(),
            Some("https://video.akamai.steamstatic.com/store_trailers/1/2/hls.m3u8")
        );
        assert_eq!(trailer_stream_url("", None), None);
        assert_eq!(trailer_stream_url("../x.m3u8", None), None);
        assert_eq!(trailer_stream_url("https://evil/x.m3u8", None), None);
    }
}
