//! Small text helpers for data coming from Steam and from HTTP headers.

/// Decodes the handful of HTML entities Steam leaves in plain-text fields.
pub(crate) fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];
        // Entities are short; look for the `;` byte-wise so multi-byte text after a lone `&`
        // never gets sliced mid-character.
        let Some(end) = rest.bytes().take(12).position(|b| b == b';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => entity
                .strip_prefix('#')
                .and_then(|n| match n.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => n.parse().ok(),
                })
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Trims and turns empty strings into `None`, decoding entities along the way.
pub(crate) fn clean(s: Option<&str>) -> Option<String> {
    let s = s?.trim();
    if s.is_empty() {
        None
    } else {
        Some(decode_entities(s))
    }
}

/// Decodes `%XX` escapes; invalid sequences are kept verbatim.
pub(crate) fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_common_entities() {
        assert_eq!(
            decode_entities("Tom &amp; Jerry &quot;GOTY&quot;"),
            "Tom & Jerry \"GOTY\""
        );
        assert_eq!(decode_entities("it&#39;s &#x2764;"), "it's ❤");
        assert_eq!(decode_entities("a & b"), "a & b");
        assert_eq!(decode_entities("&unknown; &"), "&unknown; &");
    }

    #[test]
    fn lone_ampersand_before_multibyte_text() {
        // From a real store description: `&` followed by a curly apostrophe within 12 bytes.
        let s = "& Anastasia’s unique playstyles.";
        assert_eq!(decode_entities(s), s);
        assert_eq!(
            decode_entities("Tom&Jerry’s çılgın & güzel;"),
            "Tom&Jerry’s çılgın & güzel;"
        );
        assert_eq!(decode_entities("&ç;"), "&ç;");
    }

    #[test]
    fn cleans_blank_strings() {
        assert_eq!(clean(Some("  ")), None);
        assert_eq!(clean(None), None);
        assert_eq!(clean(Some(" Dota 2 ")), Some("Dota 2".into()));
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(
            percent_decode("Game%20Setup%201.2.zip"),
            "Game Setup 1.2.zip"
        );
        assert_eq!(percent_decode("%C3%A7%C4%B1lg%C4%B1n"), "çılgın");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }
}
