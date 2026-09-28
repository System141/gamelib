//! Matching store products to Steam games by title, company and release year.
//!
//! Titles are compared after [`canonical_title`], which builds on the search normalization and
//! also drops edition suffixes ("Game of the Year Edition", "Director's Cut"…), a leading "the"
//! and turns "&" and Roman numerals into words and digits. Only equal canonical titles become
//! candidates; companies and release years then decide how sure the match is.

use std::collections::HashMap;

use crate::date::year_of;
use crate::search::normalize;

/// A match at or above this score counts as a match; below it, it is only a suggestion.
pub const CONFIDENT: f32 = 0.85;
/// Suggestions below this score are not stored.
pub const POSSIBLE: f32 = 0.6;
/// Title matches at or above this score are not re-checked with GamesDB.
pub const CERTAIN: f32 = 0.95;
/// Score of a GamesDB id match.
pub const GAMESDB: f32 = 1.0;

const BASE: f32 = 0.75;
const COMPANY_BONUS: f32 = 0.15;
const YEAR_BONUS: f32 = 0.10;
const YEAR_PENALTY: f32 = 0.25;
const TIE_PENALTY: f32 = 0.2;
/// Title, company and year all agree.
const FULL_EVIDENCE: f32 = BASE + COMPANY_BONUS + YEAR_BONUS - 1e-4;
const MAX_SUGGESTIONS: usize = 3;

/// Trailing phrases naming an edition of the same game. Longest first, so "game of the year
/// edition" goes before "edition"-less variants.
const EDITION_SUFFIXES: &[&str] = &[
    "game of the year edition",
    "digital deluxe edition",
    "game of the year",
    "goty edition",
    "complete edition",
    "definitive edition",
    "enhanced edition",
    "directors cut",
    "gold edition",
    "deluxe edition",
    "special edition",
    "anniversary edition",
    "ultimate edition",
    "legendary edition",
    "premium edition",
    "collectors edition",
    "standard edition",
    "goty",
];

/// Words that say what kind of company it is, not which one.
const COMPANY_NOISE: &[&str] = &[
    "inc",
    "ltd",
    "llc",
    "gmbh",
    "co",
    "corp",
    "corporation",
    "sa",
    "srl",
    "sro",
    "ab",
    "oy",
    "bv",
    "kg",
    "plc",
    "pty",
    "limited",
    "games",
    "game",
    "studio",
    "studios",
    "entertainment",
    "interactive",
    "software",
    "publishing",
    "digital",
    "productions",
    "production",
    "media",
    "group",
    "team",
    "the",
];

/// The title used for matching: normalized, without edition suffixes or a leading "the".
pub fn canonical_title(title: &str) -> String {
    let mut words: Vec<String> = normalize(&title.replace('&', " and "))
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    loop {
        let joined = words.join(" ");
        let Some(suffix) = EDITION_SUFFIXES
            .iter()
            .find(|s| joined.ends_with(*s) && joined.len() > s.len())
        else {
            break;
        };
        let keep = words.len() - suffix.split(' ').count();
        words.truncate(keep);
    }
    if words.len() > 1 && words[0] == "the" {
        words.remove(0);
    }
    for (i, word) in words.iter_mut().enumerate() {
        if let Some(digit) = roman(word, i == 0) {
            *word = digit.to_owned();
        }
    }
    words.join(" ")
}

/// Roman numerals used in sequel titles. "v" and "x" only after the first word, and never "i",
/// which is also a word.
fn roman(word: &str, first: bool) -> Option<&'static str> {
    Some(match word {
        "ii" => "2",
        "iii" => "3",
        "iv" => "4",
        "vi" => "6",
        "vii" => "7",
        "viii" => "8",
        "ix" => "9",
        "v" if !first => "5",
        "x" if !first => "10",
        _ => return None,
    })
}

/// A company name without legal forms and generic words ("CD PROJEKT RED S.A." → "cd projekt red").
pub fn canonical_company(name: &str) -> String {
    normalize(name)
        .split_whitespace()
        .filter(|w| !COMPANY_NOISE.contains(w))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Forms of one company name that are compared: without generic words, and without spaces
/// (with and without the generic words), so "StudioMDHR Entertainment Inc." meets
/// "Studio MDHR Entertainment Inc.".
fn company_forms(name: &str) -> Vec<String> {
    let stripped = canonical_company(name);
    let mut forms = vec![
        stripped.replace(' ', ""),
        normalize(name).replace(' ', ""),
        stripped,
    ];
    forms.retain(|f| f.len() >= 2);
    forms.sort();
    forms.dedup();
    forms
}

/// What matching needs to know about one game or product.
#[derive(Debug, Clone, Default)]
pub struct MatchKey {
    /// Comparable forms of the company names (developers and publishers).
    pub companies: Vec<String>,
    /// Release years known for it (original release, store release…).
    pub years: Vec<i32>,
}

impl MatchKey {
    pub fn new<'a>(
        companies: impl IntoIterator<Item = &'a str>,
        dates: impl IntoIterator<Item = Option<i64>>,
    ) -> Self {
        let mut companies: Vec<String> = companies.into_iter().flat_map(company_forms).collect();
        companies.sort();
        companies.dedup();
        let mut years: Vec<i32> = dates.into_iter().flatten().map(year_of).collect();
        years.sort_unstable();
        years.dedup();
        Self { companies, years }
    }
}

#[derive(Debug, Clone)]
struct Entry {
    appid: u32,
    key: MatchKey,
    reviews: u32,
}

/// Steam games indexed by canonical title.
#[derive(Debug, Default)]
pub struct SteamIndex {
    by_title: HashMap<String, Vec<Entry>>,
}

impl SteamIndex {
    pub fn insert(&mut self, appid: u32, title: &str, key: MatchKey, reviews: u32) {
        let canonical = canonical_title(title);
        if canonical.is_empty() {
            return;
        }
        self.by_title.entry(canonical).or_default().push(Entry {
            appid,
            key,
            reviews,
        });
    }

    pub fn len(&self) -> usize {
        self.by_title.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.by_title.is_empty()
    }

    /// Steam games that may be this product, best first: the confident winner (or winners, for
    /// editions of one game), otherwise up to three suggestions above [`POSSIBLE`].
    pub fn candidates(&self, canonical: &str, key: &MatchKey) -> Vec<Candidate> {
        let Some(entries) = self.by_title.get(canonical) else {
            return Vec::new();
        };
        let mut scored: Vec<(Candidate, u32)> = entries
            .iter()
            .map(|e| {
                (
                    Candidate {
                        appid: e.appid,
                        score: score(key, &e.key),
                    },
                    e.reviews,
                )
            })
            .collect();
        scored.sort_by(|(a, ra), (b, rb)| {
            b.score
                .total_cmp(&a.score)
                .then(rb.cmp(ra))
                .then(a.appid.cmp(&b.appid))
        });
        let mut out: Vec<Candidate> = scored.into_iter().map(|(c, _)| c).collect();
        let best = out[0].score;
        let tied = out
            .iter()
            .take_while(|c| (c.score - best).abs() < 1e-4)
            .count();
        // Equally strong candidates with the same title, company and year are editions of one
        // game on Steam (base game and GOTY): the product is all of them. Without both clues a
        // tie stays ambiguous.
        if best >= CONFIDENT && (tied == 1 || best >= FULL_EVIDENCE) {
            out.truncate(tied.min(MAX_SUGGESTIONS));
            return out;
        }
        if tied > 1 {
            for c in &mut out[..tied] {
                c.score -= TIE_PENALTY;
            }
        }
        out.retain(|c| c.score >= POSSIBLE);
        out.truncate(MAX_SUGGESTIONS);
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    pub appid: u32,
    pub score: f32,
}

/// How sure an equal-title pair is: companies in common and release years close together raise
/// it, years far apart (a remake, a namesake) lower it.
pub fn score(product: &MatchKey, game: &MatchKey) -> f32 {
    let mut s = BASE;
    if companies_overlap(&product.companies, &game.companies) {
        s += COMPANY_BONUS;
    }
    let closest = product
        .years
        .iter()
        .flat_map(|a| game.years.iter().map(move |b| (a - b).abs()))
        .min();
    match closest {
        Some(d) if d <= 1 => s += YEAR_BONUS,
        Some(d) if d >= 3 => s -= YEAR_PENALTY,
        _ => {}
    }
    s.clamp(0.0, 1.0)
}

fn companies_overlap(a: &[String], b: &[String]) -> bool {
    a.iter().any(|x| {
        b.iter().any(|y| {
            x == y
                || (x.len() >= 4
                    && y.len() >= 4
                    && (x.starts_with(y.as_str()) || y.starts_with(x.as_str())))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date::parse_date;

    #[test]
    fn canonical_titles() {
        let cases = [
            (
                "The Witcher 3: Wild Hunt - Complete Edition",
                "witcher 3 wild hunt",
            ),
            ("The Witcher 3: Wild Hunt", "witcher 3 wild hunt"),
            (
                "The Witcher 3: Wild Hunt - Game of the Year Edition",
                "witcher 3 wild hunt",
            ),
            (
                "Heroes of Might & Magic III - HD Edition",
                "heroes of might and magic 3 hd edition",
            ),
            (
                "Heroes of Might and Magic® 3: Complete",
                "heroes of might and magic 3 complete",
            ),
            ("Baldur's Gate: Enhanced Edition", "baldurs gate"),
            (
                "Disco Elysium - The Final Cut",
                "disco elysium the final cut",
            ),
            (
                "Deus Ex: Human Revolution - Director’s Cut",
                "deus ex human revolution",
            ),
            ("Grand Theft Auto V", "grand theft auto 5"),
            ("X: Beyond the Frontier", "x beyond the frontier"),
            ("I Am Bread", "i am bread"),
            ("The Room", "room"),
            ("The", "the"),
            ("GOTY", "goty"),
            (
                "S.T.A.L.K.E.R.: Shadow of Chernobyl",
                "stalker shadow of chernobyl",
            ),
        ];
        for (title, expected) in cases {
            assert_eq!(canonical_title(title), expected, "{title}");
        }
    }

    #[test]
    fn canonical_companies() {
        assert_eq!(canonical_company("CD PROJEKT RED"), "cd projekt red");
        assert_eq!(canonical_company("Interplay Inc."), "interplay");
        assert_eq!(canonical_company("Interplay Entertainment"), "interplay");
        assert_eq!(canonical_company("Larian Studios"), "larian");
        assert_eq!(canonical_company("Games"), "");
    }

    #[test]
    fn company_spelling_variants_overlap() {
        let a = key(&["StudioMDHR Entertainment Inc."], &[]);
        let b = key(&["Studio MDHR Entertainment Inc."], &[]);
        assert!(companies_overlap(&a.companies, &b.companies));
        let c = key(&["Maddy Makes Games Inc."], &[]);
        assert!(!companies_overlap(&a.companies, &c.companies));
    }

    fn key(companies: &[&str], dates: &[&str]) -> MatchKey {
        MatchKey::new(
            companies.iter().copied(),
            dates.iter().map(|d| parse_date(d)),
        )
    }

    #[test]
    fn scores() {
        let witcher_gog = key(&["CD PROJEKT RED"], &["2015.05.19"]);
        let witcher_steam = key(&["CD PROJEKT RED", "CD PROJEKT RED"], &["2015-05-18"]);
        assert!((score(&witcher_gog, &witcher_steam) - 1.0).abs() < 1e-6);

        // Same title, no company in common, released decades apart: a namesake.
        let old = key(&["id Software"], &["1993.12.10"]);
        let new = key(&["Some Indie"], &["2016-05-13"]);
        assert!(score(&old, &new) < POSSIBLE);

        // A classic re-released years later: the store release dates line up.
        let gog = key(&["Interplay"], &["1997.09.30", "2008.10.10"]);
        let steam = key(&["Interplay Inc."], &["2009-02-12"]);
        assert!(score(&gog, &steam) >= CONFIDENT);

        // Nothing known but the title.
        assert!((score(&MatchKey::default(), &MatchKey::default()) - BASE).abs() < 1e-6);
    }

    fn index(games: &[(u32, &str, &[&str], &str, u32)]) -> SteamIndex {
        let mut index = SteamIndex::default();
        for &(appid, title, companies, date, reviews) in games {
            index.insert(appid, title, key(companies, &[date]), reviews);
        }
        index
    }

    #[test]
    fn a_clear_winner_comes_alone() {
        let idx = index(&[
            (
                292030,
                "The Witcher 3: Wild Hunt",
                &["CD PROJEKT RED"],
                "2015-05-18",
                900_000,
            ),
            (
                1,
                "The Witcher 3: Wild Hunt",
                &["Someone Else"],
                "2021-01-01",
                3,
            ),
        ]);
        let got = idx.candidates(
            &canonical_title("The Witcher 3: Wild Hunt - Complete Edition"),
            &key(&["CD PROJEKT RED"], &["2015.05.19"]),
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].appid, 292030);
        assert!(got[0].score >= CONFIDENT);
    }

    #[test]
    fn editions_of_one_game_all_match() {
        let idx = index(&[
            (
                22300,
                "Fallout 3",
                &["Bethesda Game Studios"],
                "2008-10-28",
                18_000,
            ),
            (
                22370,
                "Fallout 3: Game of the Year Edition",
                &["Bethesda Game Studios"],
                "2009-10-13",
                44_000,
            ),
        ]);
        let got = idx.candidates(
            &canonical_title("Fallout 3: Game of the Year Edition"),
            &key(&["Bethesda Game Studios"], &["2009.10.13", "2017.06.01"]),
        );
        let mut appids: Vec<u32> = got.iter().map(|c| c.appid).collect();
        appids.sort();
        assert_eq!(appids, [22300, 22370]);
        assert!(got.iter().all(|c| c.score >= CONFIDENT));
    }

    #[test]
    fn ties_become_suggestions() {
        let idx = index(&[(10, "Doom", &[], "", 50), (20, "DOOM", &[], "", 900)]);
        let got = idx.candidates("doom", &MatchKey::default());
        assert_eq!(got.len(), 0, "0.75 - 0.2 is below the suggestion threshold");

        let idx = index(&[
            (10, "Doom", &["id Software"], "", 50),
            (20, "DOOM", &["id Software"], "", 900),
        ]);
        let got = idx.candidates("doom", &key(&["id Software"], &[]));
        assert_eq!(got.iter().map(|c| c.appid).collect::<Vec<_>>(), [20, 10]);
        assert!(
            got.iter()
                .all(|c| c.score < CONFIDENT && c.score >= POSSIBLE)
        );
    }

    #[test]
    fn unknown_titles_have_no_candidates() {
        let idx = index(&[(1, "Portal", &[], "", 1)]);
        assert!(idx.candidates("portal 2", &MatchKey::default()).is_empty());
        assert_eq!(idx.len(), 1);
    }
}
