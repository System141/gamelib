//! Reviews from Steam's store (`appreviews`, no key needed): the most helpful ones of the past
//! year and how the latest ones lean. The summaries (overall and Turkish) come with the game's
//! media instead.

use std::sync::atomic::AtomicBool;

use reqwest::blocking::Client;
use serde::Deserialize;

use crate::Result;
use crate::http::{self, Counters};
use crate::model::{GameReviews, RecentReviews, Review};
use crate::text::decode_entities;

/// Helpful reviews shown per game.
const TOP: usize = 4;
/// Reviews this short say little ("10/10", ASCII art); skipped among the helpful ones.
const MIN_TEXT: usize = 40;
/// Longer reviews are cut at a word boundary.
const MAX_TEXT: usize = 1200;

#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(default)]
    reviews: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct RawReview {
    #[serde(default)]
    recommendationid: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    review: String,
    #[serde(default)]
    voted_up: bool,
    #[serde(default)]
    votes_up: u32,
    #[serde(default)]
    timestamp_created: i64,
    #[serde(default)]
    received_for_free: bool,
    #[serde(default)]
    written_during_early_access: bool,
    #[serde(default)]
    author: Author,
}

#[derive(Debug, Default, Deserialize)]
struct Author {
    /// Minutes.
    #[serde(default)]
    playtime_forever: u32,
    #[serde(default)]
    playtime_at_review: u32,
}

/// Reviews of one page; a malformed review is skipped rather than failing the page.
fn parse(body: &str) -> Result<Vec<RawReview>> {
    let env: Envelope =
        serde_json::from_str(body).map_err(|e| crate::Error::Parse(format!("appreviews: {e}")))?;
    Ok(env
        .reviews
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect())
}

/// The most helpful reviews (Turkish first, then English) and the latest 100.
pub fn fetch(
    client: &Client,
    base: &str,
    appid: u32,
    cancel: &AtomicBool,
    counters: &Counters,
) -> Result<GameReviews> {
    let url = format!("{}/appreviews/{appid}", base.trim_end_matches('/'));
    let page = |params: &[(&str, &str)]| -> Result<Vec<RawReview>> {
        let body = http::get_text(
            client,
            &url,
            |r| {
                r.query(&[
                    ("json", "1"),
                    ("purchase_type", "all"),
                    ("review_type", "all"),
                ])
                .query(params)
            },
            cancel,
            counters,
        )?;
        parse(&body)
    };
    let helpful = [
        ("filter", "all"),
        ("day_range", "365"),
        ("num_per_page", "10"),
    ];
    let mut top = helpful_reviews(page(&[
        helpful[0],
        helpful[1],
        helpful[2],
        ("language", "turkish"),
    ])?);
    if top.len() < TOP {
        top.extend(helpful_reviews(page(&[
            helpful[0],
            helpful[1],
            helpful[2],
            ("language", "english"),
        ])?));
    }
    top.truncate(TOP);
    let recent = page(&[
        ("filter", "recent"),
        ("language", "all"),
        ("num_per_page", "100"),
    ])?;
    Ok(GameReviews {
        top,
        recent: recent_summary(&recent),
    })
}

fn helpful_reviews(raw: Vec<RawReview>) -> Vec<Review> {
    raw.into_iter()
        .filter_map(|r| {
            let text = plain_review(&r.review);
            (text.chars().count() >= MIN_TEXT).then(|| Review {
                id: r.recommendationid,
                language: r.language,
                positive: r.voted_up,
                text,
                helpful: r.votes_up,
                hours_at_review: hours(r.author.playtime_at_review),
                hours_total: hours(r.author.playtime_forever),
                created: r.timestamp_created,
                early_access: r.written_during_early_access,
                received_for_free: r.received_for_free,
            })
        })
        .collect()
}

fn hours(minutes: u32) -> f32 {
    (minutes as f32 / 6.0).round() / 10.0
}

fn recent_summary(reviews: &[RawReview]) -> Option<RecentReviews> {
    let times = reviews
        .iter()
        .map(|r| r.timestamp_created)
        .filter(|&t| t > 0);
    Some(RecentReviews {
        count: reviews.len() as u32,
        positive: reviews.iter().filter(|r| r.voted_up).count() as u32,
        from: times.clone().min()?,
        to: times.max()?,
    })
}

/// Review text without Steam's BBCode (`[b]`, `[h1]`, `[url=…]`, lists), with spoilers left
/// out and long reviews shortened.
fn plain_review(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut spoiler = 0u32;
    while let Some(open) = rest.find('[') {
        if spoiler == 0 {
            out.push_str(&rest[..open]);
        }
        let after = &rest[open..];
        let Some(close) = after.find(']').filter(|&c| c <= 64) else {
            // Not a tag: keep the bracket.
            if spoiler == 0 {
                out.push('[');
            }
            rest = &after[1..];
            continue;
        };
        let tag = after[1..close].trim().to_ascii_lowercase();
        let name = tag
            .trim_start_matches('/')
            .split(['=', ' '])
            .next()
            .unwrap_or("");
        let known = matches!(
            name,
            "b" | "i"
                | "u"
                | "s"
                | "strike"
                | "h1"
                | "h2"
                | "h3"
                | "spoiler"
                | "noparse"
                | "url"
                | "list"
                | "olist"
                | "*"
                | "quote"
                | "code"
                | "table"
                | "tr"
                | "td"
                | "th"
                | "hr"
                | "img"
                | "previewyoutube"
        );
        if !known {
            if spoiler == 0 {
                out.push('[');
            }
            rest = &after[1..];
            continue;
        }
        match (tag.starts_with('/'), name) {
            (false, "spoiler") => spoiler += 1,
            (true, "spoiler") => spoiler = spoiler.saturating_sub(1),
            // List items and block tags break lines.
            (false, "*") if spoiler == 0 => out.push_str("\n• "),
            (_, "h1" | "h2" | "h3" | "list" | "olist" | "quote" | "table" | "tr" | "hr")
                if spoiler == 0 =>
            {
                out.push('\n')
            }
            _ => {}
        }
        rest = &after[close + 1..];
    }
    if spoiler == 0 {
        out.push_str(rest);
    }
    let text = decode_entities(&out);
    // At most one empty line in a row, no trailing spaces.
    let mut lines: Vec<&str> = Vec::new();
    for line in text.lines().map(str::trim_end) {
        if line.trim().is_empty() && lines.last().is_none_or(|l| l.trim().is_empty()) {
            continue;
        }
        lines.push(line);
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    shorten(lines.join("\n").trim())
}

fn shorten(text: &str) -> String {
    if text.chars().count() <= MAX_TEXT {
        return text.to_owned();
    }
    let cut: String = text.chars().take(MAX_TEXT).collect();
    let end = cut
        .rfind(char::is_whitespace)
        .filter(|&i| i > MAX_TEXT / 2)
        .unwrap_or(cut.len());
    format!("{}…", cut[..end].trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpful_reviews_from_steam() {
        let raw = parse(include_str!("../../tests/fixtures/appreviews_top.json")).unwrap();
        let top = helpful_reviews(raw);
        assert_eq!(top.len(), 3);
        let first = &top[0];
        assert_eq!(first.id, "233727365");
        assert_eq!(first.language, "turkish");
        assert!(first.positive);
        assert_eq!(first.helpful, 380);
        assert_eq!(first.hours_at_review, 17.1);
        assert!(first.text.starts_with("Kaç sene önce"), "{}", first.text);
    }

    #[test]
    fn latest_reviews_lean() {
        let raw = parse(include_str!("../../tests/fixtures/appreviews_recent.json")).unwrap();
        let recent = recent_summary(&raw).unwrap();
        assert_eq!(recent.count, 6);
        assert_eq!(recent.positive, 5);
        assert!(recent.from <= recent.to);
        assert_eq!(recent_summary(&[]), None);
    }

    #[test]
    fn strips_formatting_and_spoilers() {
        assert_eq!(
            plain_review("[h1]Harika[/h1]\n[b]Kısaca:[/b] [url=https://x]oyna[/url]"),
            "Harika\n\nKısaca: oyna"
        );
        assert_eq!(
            plain_review("Son [spoiler]kötü adam babası[/spoiler] çok iyi"),
            "Son  çok iyi"
        );
        assert_eq!(
            plain_review("[list][*]Grafik[*]Hikâye[/list]"),
            "• Grafik\n• Hikâye"
        );
        assert_eq!(
            plain_review("Puan [10/10] &amp; [kesinlikle]"),
            "Puan [10/10] & [kesinlikle]"
        );
        assert_eq!(plain_review("a\n\n\n\nb   \n"), "a\n\nb");
        let long = "kelime ".repeat(400);
        let short = plain_review(&long);
        assert!(short.ends_with('…') && short.chars().count() <= MAX_TEXT + 1);
    }

    #[test]
    fn short_reviews_are_skipped() {
        let raw = parse(r#"{"success":1,"reviews":[{"recommendationid":"1","review":"10/10","voted_up":true},{"bad":true,"votes_up":"x"}]}"#).unwrap();
        assert_eq!(raw.len(), 1);
        assert!(helpful_reviews(raw).is_empty());
    }
}
