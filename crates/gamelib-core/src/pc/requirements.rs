//! Steam's system requirements: its HTML lists turned into lines, the amounts they ask for, and
//! how this computer measures up to them.

use serde::Serialize;

use super::hardware::ThisPc;
use crate::text::decode_entities;

/// What a requirement line is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementKind {
    Os,
    Processor,
    Memory,
    Graphics,
    #[serde(rename = "directx")]
    DirectX,
    Storage,
    Sound,
    Network,
    Notes,
    Other,
}

/// One line of a requirements list, as the store wrote it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequirementLine {
    pub kind: RequirementKind,
    /// The store's own label, for lines of a kind GameLib does not know ("VR Support").
    pub label: Option<String>,
    pub text: String,
}

/// Turns one of Steam's requirement lists (HTML) into lines. Steam writes most lists as
/// `<li><strong>Memory:</strong> 8 GB RAM</li>`, some as `<li><strong>Memory: 2.5GB</strong></li>`
/// and a few as `<br>`-separated lines.
pub fn parse_list(html: &str) -> Vec<RequirementLine> {
    let items: Vec<&str> = if html.contains("<li") {
        html.split("<li")
            .skip(1)
            .map(|chunk| {
                let chunk = chunk.split_once('>').map_or("", |(_, rest)| rest);
                chunk.split("</li>").next().unwrap_or(chunk)
            })
            .collect()
    } else {
        html.split("<br")
            .map(|chunk| match chunk.split_once('>') {
                // The rest of a `<br>` / `<br />` tag.
                Some((tag, rest)) if !tag.contains('<') && tag.len() <= 2 => rest,
                _ => chunk,
            })
            .collect()
    };
    items
        .into_iter()
        .map(plain_text)
        .filter(|text| !text.is_empty() && !is_heading(text))
        .map(|text| classify(&text))
        .collect()
}

/// Text without tags and entities, on one line.
fn plain_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                // Tags separate words (`OS:<br>Windows`).
                text.push(' ');
            }
            '>' if in_tag => in_tag = false,
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    decode_entities(&text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(" :", ":")
}

fn is_heading(text: &str) -> bool {
    let t = text.trim_end_matches(':').trim().to_lowercase();
    matches!(
        t.as_str(),
        "minimum"
            | "recommended"
            | "required"
            | "minimum requirements"
            | "recommended requirements"
    )
}

fn classify(text: &str) -> RequirementLine {
    if let Some((key, value)) = text.split_once(':') {
        let value = value.trim();
        if key.chars().count() <= 30 && !value.is_empty() {
            let normalized: String = key
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == ' ')
                .collect::<String>()
                .trim()
                .to_lowercase();
            let (kind, text) = match normalized.as_str() {
                "os" | "operating system" | "os version" => (RequirementKind::Os, value.to_owned()),
                "processor" | "cpu" => (RequirementKind::Processor, value.to_owned()),
                "memory" | "ram" | "system memory" => (RequirementKind::Memory, value.to_owned()),
                "graphics" | "video card" | "video" | "graphics card" | "gpu" => {
                    (RequirementKind::Graphics, value.to_owned())
                }
                // A line of its own for video memory ("Video Memory: 2 GB").
                "video memory" | "vram" | "graphics memory" | "video ram" => {
                    (RequirementKind::Graphics, format!("{value} VRAM"))
                }
                "directx" | "direct x" => (RequirementKind::DirectX, value.to_owned()),
                "storage" | "hard drive" | "hard disk space" | "hard disk" | "disk space"
                | "hdd" | "free disk space" | "hard drive space" => {
                    (RequirementKind::Storage, value.to_owned())
                }
                "sound card" | "sound" | "audio" => (RequirementKind::Sound, value.to_owned()),
                "network" => (RequirementKind::Network, value.to_owned()),
                "additional notes" | "notes" | "additional" | "other" => {
                    (RequirementKind::Notes, value.to_owned())
                }
                _ => {
                    return RequirementLine {
                        kind: RequirementKind::Other,
                        label: Some(key.trim().to_owned()),
                        text: value.to_owned(),
                    };
                }
            };
            return RequirementLine {
                kind,
                label: None,
                text,
            };
        }
    }
    RequirementLine {
        kind: RequirementKind::Other,
        label: None,
        text: text.to_owned(),
    }
}

/// A size in bytes ("12 GB RAM", "8192 MB", "2.5GB", "60 SSD GB available space").
pub fn parse_size(text: &str) -> Option<u64> {
    sizes(text).first().map(|s| s.bytes)
}

/// A size and where it was found in the text.
#[derive(Debug, Clone, Copy)]
struct Size {
    bytes: u64,
    start: usize,
    end: usize,
}

/// Every "number unit" in `text` (byte offsets into it).
fn sizes(text: &str) -> Vec<Size> {
    // ASCII lowercasing keeps byte offsets, and sizes are ASCII.
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit()
            || (i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'.'))
        {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.' || bytes[i] == b',')
        {
            i += 1;
        }
        let number = lower[start..i]
            .trim_end_matches(['.', ','])
            .replace(',', ".");
        let Ok(value) = number.parse::<f64>() else {
            continue;
        };
        // The unit, possibly after one word such as "SSD" ("60 SSD GB").
        let mut j = i;
        let mut words = 0;
        let unit = loop {
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'+') {
                j += 1;
            }
            let word_start = j;
            while j < bytes.len() && bytes[j].is_ascii_alphabetic() {
                j += 1;
            }
            let word = &lower[word_start..j];
            let multiplier = match word {
                "tb" => Some(1u64 << 40),
                "gb" | "gib" | "g" | "go" => Some(1u64 << 30),
                "mb" | "mib" | "m" | "mo" => Some(1u64 << 20),
                _ => None,
            };
            if let Some(m) = multiplier {
                break Some(m);
            }
            words += 1;
            if words > 1 || !matches!(word, "ssd" | "hdd" | "of") {
                break None;
            }
        };
        if let Some(multiplier) = unit {
            found.push(Size {
                bytes: (value * multiplier as f64).round() as u64,
                start,
                end: j,
            });
            i = j;
        }
    }
    found
}

/// Video memory a line asks for: "(4GB+ of VRAM)", "VRAM 6 GB", "2 GB VRAM",
/// "128mb Video Memory". Only sizes next to such a word count, so "RX 5500 XT 8GB" (a card
/// model) does not.
pub fn parse_vram(text: &str) -> Option<u64> {
    let lower = text.to_ascii_lowercase();
    let keywords: Vec<usize> = [
        "vram",
        "video memory",
        "video ram",
        "graphics memory",
        "dedicated",
    ]
    .iter()
    .flat_map(|k| lower.match_indices(k).map(|(i, _)| i).collect::<Vec<_>>())
    .collect();
    if keywords.is_empty() {
        return None;
    }
    sizes(text)
        .into_iter()
        .filter(|s| {
            keywords
                .iter()
                .any(|&k| (k >= s.end && k - s.end <= 12) || (k < s.start && s.start - k <= 16))
        })
        .map(|s| s.bytes)
        .max()
}

/// The DirectX version a line asks for ("Version 11", "9.0c or Greater", "DirectX 12").
fn parse_directx(text: &str) -> Option<u32> {
    text.split(|c: char| !c.is_ascii_digit())
        .filter_map(|n| n.parse::<u32>().ok())
        .find(|v| (7..=12).contains(v))
}

/// The oldest Windows a line accepts ("Windows 10 64-bit" → 10, "Windows XP, Vista, 7, 8/8.1,
/// 10" → XP). Windows versions as numbers: XP 5.1, Vista 6, 7, 8, 8.1, 10, 11.
fn parse_windows(text: &str) -> Option<f32> {
    let lower = text.to_lowercase();
    if !lower.contains("win") {
        return None;
    }
    lower
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '.'))
        .filter_map(|word| match word.trim_end_matches('.') {
            "xp" => Some(5.1),
            "vista" => Some(6.0),
            "7" | "win7" => Some(7.0),
            "8" | "win8" => Some(8.0),
            "8.1" => Some(8.1),
            "10" | "win10" => Some(10.0),
            "11" | "win11" => Some(11.0),
            _ => None,
        })
        .reduce(f32::min)
}

fn mentions_64_bit(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("64-bit")
        || lower.contains("64 bit")
        || lower.contains("64bit")
        || lower.contains("x64")
}

fn mentions_ssd(text: &str) -> bool {
    text.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| w == "ssd")
}

/// The amounts a requirements list asks for, where it states them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Needs {
    pub memory: Option<u64>,
    pub video_memory: Option<u64>,
    pub storage: Option<u64>,
    pub ssd: bool,
    pub directx: Option<u32>,
    pub windows: Option<f32>,
    pub bits64: bool,
}

pub fn needs(lines: &[RequirementLine]) -> Needs {
    let mut needs = Needs::default();
    for line in lines {
        match line.kind {
            RequirementKind::Memory if needs.memory.is_none() => {
                needs.memory = parse_size(&line.text)
            }
            RequirementKind::Storage if needs.storage.is_none() => {
                needs.storage = parse_size(&line.text);
                needs.ssd |= mentions_ssd(&line.text);
            }
            RequirementKind::DirectX if needs.directx.is_none() => {
                needs.directx = parse_directx(&line.text)
            }
            RequirementKind::Os if needs.windows.is_none() => {
                needs.windows = parse_windows(&line.text)
            }
            _ => {}
        }
        if matches!(
            line.kind,
            RequirementKind::Graphics | RequirementKind::Notes | RequirementKind::Other
        ) && let Some(vram) = parse_vram(&line.text)
        {
            needs.video_memory = Some(needs.video_memory.map_or(vram, |v| v.max(vram)));
        }
        if line.kind == RequirementKind::Notes && mentions_ssd(&line.text) {
            needs.ssd = true;
        }
        if line.kind == RequirementKind::Graphics && needs.directx.is_none() {
            let lower = line.text.to_ascii_lowercase();
            if let Some(pos) = lower.find("directx").or_else(|| lower.find("dx")) {
                let near: String = lower[pos..].chars().take(14).collect();
                needs.directx = parse_directx(&near);
            }
        }
        needs.bits64 |= mentions_64_bit(&line.text);
    }
    needs
}

/// What a check compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    Memory,
    VideoMemory,
    Storage,
    Ssd,
    #[serde(rename = "directx")]
    DirectX,
    Windows,
    Bits64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Ok,
    Short,
    Unknown,
}

/// One requirement measured against this computer. `need` and `have` are bytes for sizes, the
/// version for DirectX and Windows, and 1/0 for yes-or-no checks.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub kind: CheckKind,
    pub need: f64,
    pub have: Option<f64>,
    pub verdict: Verdict,
}

/// Measures `needs` against this computer. Memory the system reports is a little below the
/// installed amount, so memory checks allow a small margin.
pub fn check(needs: &Needs, pc: &ThisPc) -> Vec<Check> {
    let mut checks = Vec::new();
    let mut push = |kind, need: f64, have: Option<f64>, ok: &dyn Fn(f64) -> bool| {
        let verdict = match have {
            Some(h) if ok(h) => Verdict::Ok,
            Some(_) => Verdict::Short,
            None => Verdict::Unknown,
        };
        checks.push(Check {
            kind,
            need,
            have,
            verdict,
        });
    };
    if let Some(need) = needs.memory {
        let need = need as f64;
        push(CheckKind::Memory, need, pc.memory.map(|m| m as f64), &|h| {
            h >= need * 0.93
        });
    }
    if let Some(need) = needs.video_memory {
        let need = need as f64;
        push(
            CheckKind::VideoMemory,
            need,
            pc.video_memory.map(|m| m as f64),
            &|h| h >= need * 0.9,
        );
    }
    if let Some(need) = needs.storage {
        let need = need as f64;
        push(
            CheckKind::Storage,
            need,
            pc.disk_free.map(|d| d as f64),
            &|h| h >= need,
        );
    }
    if needs.ssd {
        push(
            CheckKind::Ssd,
            1.0,
            pc.disk_ssd.map(|s| if s { 1.0 } else { 0.0 }),
            &|h| h >= 1.0,
        );
    }
    if let Some(need) = needs.directx {
        let need = need as f64;
        push(CheckKind::DirectX, need, pc.directx.map(f64::from), &|h| {
            h >= need
        });
    }
    if let Some(need) = needs.windows {
        let need = f64::from(need);
        push(CheckKind::Windows, need, pc.windows.map(f64::from), &|h| {
            h >= need - 0.001
        });
    }
    if needs.bits64 {
        push(
            CheckKind::Bits64,
            1.0,
            Some(if pc.bits64 { 1.0 } else { 0.0 }),
            &|h| h >= 1.0,
        );
    }
    checks
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1 << 30;
    const MB: u64 = 1 << 20;

    fn fixture(appid: &str, which: &str) -> Vec<RequirementLine> {
        let all: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/requirements.json")).unwrap();
        parse_list(all[appid][which].as_str().unwrap())
    }

    fn line(kind: RequirementKind, text: &str) -> RequirementLine {
        RequirementLine {
            kind,
            label: None,
            text: text.into(),
        }
    }

    #[test]
    fn parses_steams_usual_lists() {
        let lines = fixture("292030", "minimum");
        assert_eq!(
            lines,
            vec![
                line(RequirementKind::Os, "64-bit Windows 11"),
                line(RequirementKind::Processor, "Core i5-8400 / Ryzen 5 2600"),
                line(RequirementKind::Memory, "12 GB RAM"),
                line(
                    RequirementKind::Graphics,
                    "GeForce GTX 1660 / Radeon RX 5500 XT 8GB / Arc A580"
                ),
                line(RequirementKind::Storage, "60 SSD GB available space"),
                line(RequirementKind::Notes, "VRAM 6 GB"),
            ]
        );
        let n = needs(&lines);
        assert_eq!(
            n,
            Needs {
                memory: Some(12 * GB),
                video_memory: Some(6 * GB),
                storage: Some(60 * GB),
                ssd: true,
                directx: None,
                windows: Some(11.0),
                bits64: true,
            }
        );
    }

    #[test]
    fn keeps_lines_without_a_label() {
        let lines = fixture("1086940", "minimum");
        assert_eq!(
            lines[0],
            RequirementLine {
                kind: RequirementKind::Other,
                label: None,
                text: "Requires a 64-bit processor and operating system".into()
            }
        );
        let n = needs(&lines);
        assert_eq!(n.memory, Some(8 * GB));
        assert_eq!(n.video_memory, Some(4 * GB));
        assert_eq!(n.storage, Some(150 * GB));
        assert_eq!(n.directx, Some(11));
        assert_eq!(n.windows, Some(10.0));
        assert!(n.ssd && n.bits64);
        let rec = needs(&fixture("1086940", "recommended"));
        assert_eq!(rec.video_memory, Some(8 * GB));
        assert_eq!(rec.memory, Some(16 * GB));
    }

    #[test]
    fn parses_old_lists_with_values_inside_the_label() {
        let lines = fixture("105600", "minimum");
        let kinds: Vec<_> = lines.iter().map(|l| l.kind).collect();
        assert_eq!(
            kinds,
            [
                RequirementKind::Os,
                RequirementKind::Processor,
                RequirementKind::Memory,
                RequirementKind::Storage,
                RequirementKind::Graphics,
                RequirementKind::DirectX,
            ]
        );
        assert_eq!(lines[0].text, "Windows Xp, Vista, 7, 8/8.1, 10");
        let n = needs(&lines);
        assert_eq!(n.memory, Some((2.5 * GB as f64) as u64));
        assert_eq!(n.storage, Some(200 * MB));
        assert_eq!(n.video_memory, Some(128 * MB));
        assert_eq!(n.directx, Some(9));
        assert_eq!(n.windows, Some(5.1));
        assert!(!n.ssd && !n.bits64);
    }

    #[test]
    fn reads_sizes_and_versions() {
        assert_eq!(parse_size("8192 MB RAM"), Some(8 * GB));
        assert_eq!(parse_size("1,5 GB"), Some((1.5 * GB as f64) as u64));
        assert_eq!(parse_size("2 TB"), Some(2 << 40));
        assert_eq!(parse_size("Intel Core i5-8400"), None);
        assert_eq!(parse_size("DirectX 11"), None);
        assert_eq!(parse_vram("Radeon RX 5500 XT 8GB"), None);
        assert_eq!(parse_vram("GTX 970 (4GB+ of VRAM)"), Some(4 * GB));
        assert_eq!(parse_vram("2 GB VRAM"), Some(2 * GB));
        assert_eq!(parse_vram("Video Memory: 512 MB"), Some(512 * MB));
        assert_eq!(parse_directx("9.0c or Greater"), Some(9));
        assert_eq!(parse_directx("Version 12"), Some(12));
        assert_eq!(parse_windows("Windows 7 SP1 or later"), Some(7.0));
        assert_eq!(
            parse_windows("Windows® 10 (64-bit) version 1909"),
            Some(10.0)
        );
        assert_eq!(parse_windows("macOS 12"), None);
        let br =
            parse_list("<strong>Minimum:</strong><br>OS: Windows 10<br />Memory: 4 GB RAM<br>");
        assert_eq!(
            br,
            vec![
                line(RequirementKind::Os, "Windows 10"),
                line(RequirementKind::Memory, "4 GB RAM")
            ]
        );
        let own = parse_list(
            "<ul><li><strong>Video Memory:</strong> 2 GB</li><li><strong>VR Support:</strong> SteamVR</li></ul>",
        );
        assert_eq!(own[0], line(RequirementKind::Graphics, "2 GB VRAM"));
        assert_eq!(own[1].label.as_deref(), Some("VR Support"));
        assert_eq!(needs(&own).video_memory, Some(2 * GB));
        // Multi-byte text around the numbers.
        let odd = [line(
            RequirementKind::Graphics,
            "Kart® DirectX® 11 uyumlu — 1 Go VRAM ✓",
        )];
        let n = needs(&odd);
        assert_eq!((n.directx, n.video_memory), (Some(11), Some(GB)));
    }

    #[test]
    fn measures_this_computer() {
        let n = needs(&fixture("292030", "minimum"));
        let pc = ThisPc {
            memory: Some(16 * GB - 300 * MB),
            video_memory: Some(4 * GB),
            disk_free: Some(500 * GB),
            disk_ssd: None,
            directx: Some(12),
            windows: Some(10.0),
            bits64: true,
            ..ThisPc::default()
        };
        let verdicts: Vec<_> = check(&n, &pc)
            .into_iter()
            .map(|c| (c.kind, c.verdict))
            .collect();
        assert_eq!(
            verdicts,
            [
                (CheckKind::Memory, Verdict::Ok),
                (CheckKind::VideoMemory, Verdict::Short),
                (CheckKind::Storage, Verdict::Ok),
                (CheckKind::Ssd, Verdict::Unknown),
                (CheckKind::Windows, Verdict::Short),
                (CheckKind::Bits64, Verdict::Ok),
            ]
        );
        // 12 GB asked, 11.2 GB reported for 12 GB installed: close enough.
        let pc = ThisPc {
            memory: Some((11.2 * GB as f64) as u64),
            ..ThisPc::default()
        };
        assert_eq!(check(&n, &pc)[0].verdict, Verdict::Ok);
    }
}
