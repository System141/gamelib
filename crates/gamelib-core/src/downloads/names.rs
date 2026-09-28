//! File and folder names that are safe on Windows (and everywhere else).

const MAX_LEN: usize = 150;
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// A name usable as one path component: no separators, no `<>:"|?*` (a `:` would create an
/// NTFS alternate data stream), no control characters, no trailing dots or spaces, not a
/// reserved device name, at most 150 characters (keeping the extension).
pub fn safe_name(name: &str, fallback: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let mut cleaned = cleaned.trim().trim_end_matches(['.', ' ']).to_owned();
    if cleaned.is_empty() || cleaned.chars().all(|c| c == '.' || c == '_') {
        cleaned = fallback.to_owned();
    }
    let stem = cleaned.split('.').next().unwrap_or_default();
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        cleaned.insert(0, '_');
    }
    if cleaned.chars().count() > MAX_LEN {
        let ext: String = cleaned
            .rsplit_once('.')
            .map(|(_, e)| e)
            .filter(|e| e.len() <= 10 && !e.contains(' '))
            .map(|e| format!(".{e}"))
            .unwrap_or_default();
        let keep = MAX_LEN - ext.chars().count();
        cleaned = cleaned.chars().take(keep).collect::<String>() + &ext;
    }
    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_safe() {
        assert_eq!(
            safe_name("setup_the_witcher_3_4.04b_(64bit).exe", "f"),
            "setup_the_witcher_3_4.04b_(64bit).exe"
        );
        assert_eq!(
            safe_name("a:b<c>d|e?f*g\"h.zip", "f"),
            "a_b_c_d_e_f_g_h.zip"
        );
        assert_eq!(safe_name("../../etc/passwd", "f"), ".._.._etc_passwd");
        assert_eq!(safe_name("name. . ", "f"), "name");
        assert_eq!(safe_name("CON.txt", "f"), "_CON.txt");
        assert_eq!(safe_name("com1", "f"), "_com1");
        assert_eq!(safe_name("  ", "file-1"), "file-1");
        assert_eq!(safe_name("..", "file-1"), "file-1");
        assert_eq!(safe_name("tab\there.exe", "f"), "tab_here.exe");
        let long = format!("{}.zip", "x".repeat(300));
        let safe = safe_name(&long, "f");
        assert_eq!(safe.chars().count(), 150);
        assert!(safe.ends_with(".zip"));
    }
}
