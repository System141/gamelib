//! Review-based rating used for the "top rated" sort.

/// SteamDB's rating formula: pulls the positive ratio towards 50 % when there are few reviews,
/// so a game with 3/3 positive reviews does not outrank one with 95 % of 100 000.
///
/// Returns 0 for games without reviews so they sink to the bottom of the rating sort.
pub fn steamdb_rating(percent_positive: u8, review_count: u32) -> f64 {
    if review_count == 0 {
        return 0.0;
    }
    let avg = f64::from(percent_positive.min(100)) / 100.0;
    let weight = 2f64.powf(-(f64::from(review_count) + 1.0).log10());
    avg - (avg - 0.5) * weight
}

#[cfg(test)]
mod tests {
    use super::steamdb_rating;

    #[test]
    fn no_reviews_sinks() {
        assert_eq!(steamdb_rating(100, 0), 0.0);
    }

    #[test]
    fn more_reviews_means_more_confidence() {
        let few = steamdb_rating(100, 3);
        let many = steamdb_rating(95, 100_000);
        assert!(many > few, "{many} should beat {few}");
        assert!(steamdb_rating(90, 10_000) > steamdb_rating(90, 100));
        assert!(steamdb_rating(10, 10_000) < steamdb_rating(10, 100));
    }

    #[test]
    fn bounded_between_zero_and_one() {
        for pct in [0u8, 50, 100] {
            for n in [1u32, 10, 1_000_000] {
                let r = steamdb_rating(pct, n);
                assert!((0.0..=1.0).contains(&r), "{pct}% of {n} gave {r}");
            }
        }
        assert!((steamdb_rating(50, 1234) - 0.5).abs() < 1e-9);
    }
}
