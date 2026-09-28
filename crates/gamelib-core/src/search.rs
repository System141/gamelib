//! Name normalization and FTS5 query building.
//!
//! Names are stored twice: as shown, and normalized in `games.search_name`, which feeds the FTS
//! index. The same normalization is applied to what the user types, so "baldurs" finds
//! "Baldur's Gate 3", "stalker" finds "S.T.A.L.K.E.R." and "ılık" matches "Ilık".

/// Maximum number of words turned into FTS terms.
const MAX_TERMS: usize = 8;

/// Lowercases, maps Turkish dotted/dotless i to `i`, drops apostrophes and dots, and turns any
/// other punctuation into single spaces.
pub fn normalize(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pending_space = false;
    for ch in input.chars() {
        match ch {
            'İ' | 'I' | 'ı' | 'i' => push_char(&mut out, &mut pending_space, 'i'),
            '\'' | '’' | '‘' | '`' | '´' | 'ʼ' | '.' => {}
            // Combining marks (decomposed accents) carry no search value.
            '\u{0300}'..='\u{036f}' => {}
            c if c.is_alphanumeric() => {
                for lower in c.to_lowercase() {
                    push_char(&mut out, &mut pending_space, lower);
                }
            }
            _ => pending_space = !out.is_empty(),
        }
    }
    out
}

fn push_char(out: &mut String, pending_space: &mut bool, c: char) {
    if *pending_space {
        out.push(' ');
        *pending_space = false;
    }
    out.push(c);
}

/// Builds an FTS5 MATCH expression: every word becomes a quoted prefix term, all must match.
pub fn fts_query(input: &str) -> Option<String> {
    let normalized = normalize(input);
    let terms: Vec<String> = normalized
        .split_whitespace()
        .take(MAX_TERMS)
        .map(|t| format!("\"{}\"*", t.replace('"', "\"\"")))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

/// A search made only of digits may be a Steam app id.
pub fn parse_appid(input: &str) -> Option<u32> {
    let t = input.trim();
    if t.is_empty() || t.len() > 10 || !t.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    t.parse().ok().filter(|&n| n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_punctuation_and_case() {
        assert_eq!(normalize("Baldur's Gate 3"), "baldurs gate 3");
        assert_eq!(
            normalize("S.T.A.L.K.E.R.: Shadow of Chernobyl"),
            "stalker shadow of chernobyl"
        );
        assert_eq!(normalize("  Half-Life   2 "), "half life 2");
        assert_eq!(normalize("!AnyWay!"), "anyway");
        assert_eq!(normalize("DOOM: The Dark Ages™"), "doom the dark ages");
    }

    #[test]
    fn normalizes_turkish_i() {
        assert_eq!(normalize("İSTANBUL Kıyamet"), "istanbul kiyamet");
        assert_eq!(normalize("IŞIK"), "işik");
        assert_eq!(normalize("ılık"), normalize("Ilık"));
    }

    #[test]
    fn keeps_non_latin_scripts() {
        assert_eq!(normalize("出发吧冒险家"), "出发吧冒险家");
        assert_eq!(normalize("Ведьмак 3"), "ведьмак 3");
    }

    #[test]
    fn drops_combining_marks() {
        assert_eq!(normalize("Cafe\u{0301}"), "cafe");
    }

    #[test]
    fn builds_prefix_terms() {
        assert_eq!(
            fts_query("witcher 3").as_deref(),
            Some("\"witcher\"* \"3\"*")
        );
        assert_eq!(fts_query("Baldur's").as_deref(), Some("\"baldurs\"*"));
        assert_eq!(
            fts_query("\"quoted\" OR x").as_deref(),
            Some("\"quoted\"* \"or\"* \"x\"*")
        );
        assert_eq!(fts_query("  ...  "), None);
        assert_eq!(fts_query(""), None);
        let many = fts_query("a b c d e f g h i j").unwrap();
        assert_eq!(many.matches('*').count(), MAX_TERMS);
    }

    #[test]
    fn parses_appids() {
        assert_eq!(parse_appid(" 570 "), Some(570));
        assert_eq!(parse_appid("0"), None);
        assert_eq!(parse_appid("57a"), None);
        assert_eq!(parse_appid("99999999999"), None);
    }
}
