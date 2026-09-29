//! UTC calendar helpers, enough for store release dates without a date crate.

/// Days since 1970-01-01 for a proleptic Gregorian date (H. Hinnant's `days_from_civil`).
pub(crate) fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Year, month and day of a Unix time (H. Hinnant's `civil_from_days`).
pub(crate) fn civil_from_unix(unix: i64) -> (i64, u32, u32) {
    let z = unix.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The year of a Unix time.
pub(crate) fn year_of(unix: i64) -> i32 {
    civil_from_unix(unix).0 as i32
}

/// Unix time of an RFC 3339 time: `2026-09-29T17:00:00+02:00`, `…Z`, fractions of a second
/// allowed.
pub(crate) fn parse_rfc3339(s: &str) -> Option<i64> {
    let (date, rest) = s.trim().split_once(['T', 't', ' '])?;
    let mut d = date.split('-');
    let year: i64 = d.next()?.parse().ok()?;
    let month: u32 = d.next()?.parse().ok()?;
    let day: u32 = d.next()?.parse().ok()?;
    if d.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (time, offset) = match rest.strip_suffix(['Z', 'z']) {
        Some(time) => (time, 0),
        None => {
            let at = rest.rfind(['+', '-'])?;
            let (time, zone) = rest.split_at(at);
            let (hours, minutes) = zone[1..].split_once(':')?;
            let seconds = hours.parse::<i64>().ok()? * 3_600 + minutes.parse::<i64>().ok()? * 60;
            (
                time,
                if zone.starts_with('-') {
                    -seconds
                } else {
                    seconds
                },
            )
        }
    };
    let mut t = time.split('.').next()?.split(':');
    let hour: i64 = t.next()?.parse().ok()?;
    let minute: i64 = t.next()?.parse().ok()?;
    let second: i64 = t.next().unwrap_or("0").parse().ok()?;
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second - offset)
}

/// Midnight UTC of the day of a Unix time, as RFC 3339 (`2026-09-29T00:00:00Z`).
pub(crate) fn rfc3339_day(unix: i64) -> String {
    let (year, month, day) = civil_from_unix(unix);
    format!("{year:04}-{month:02}-{day:02}T00:00:00Z")
}

/// Unix time (midnight UTC) of `YYYY.MM.DD`, `YYYY-MM-DD` (a time part is ignored) or `YYYY`.
pub(crate) fn parse_date(s: &str) -> Option<i64> {
    let s = s.trim();
    let date = s.split(['T', ' ']).next()?;
    let mut parts = date.split(['.', '-', '/']);
    let year: i64 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next().map_or(Some(1), |p| p.parse().ok())?;
    let day: u32 = parts.next().map_or(Some(1), |p| p.parse().ok())?;
    if !(1970..=2200).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_both_ways() {
        for (y, m, d) in [
            (1970, 1, 1),
            (2000, 2, 29),
            (2024, 12, 31),
            (2026, 9, 29),
            (2100, 3, 1),
        ] {
            let unix = days_from_civil(y, m, d) * 86_400;
            assert_eq!(civil_from_unix(unix + 3_600 * 13), (y, m, d));
        }
        assert_eq!(rfc3339_day(1_790_708_014), "2026-09-29T00:00:00Z");
        let noon = days_from_civil(2026, 9, 29) * 86_400 + 12 * 3_600;
        assert_eq!(parse_rfc3339("2026-09-29T12:00:00Z"), Some(noon));
        assert_eq!(parse_rfc3339("2026-09-29T14:00:00+02:00"), Some(noon));
        assert_eq!(parse_rfc3339("2026-09-29T09:30:00.123-02:30"), Some(noon));
        assert_eq!(parse_rfc3339("2026-09-29"), None);
        assert_eq!(parse_rfc3339("yesterday"), None);
    }

    #[test]
    fn parses_store_dates() {
        assert_eq!(parse_date("1970.01.01"), Some(0));
        assert_eq!(parse_date("2009.11.30"), Some(1_259_539_200));
        assert_eq!(parse_date("2015-05-19T00:00:00+0300"), Some(1_431_993_600));
        assert_eq!(parse_date("2024"), Some(1_704_067_200));
        assert_eq!(parse_date(""), None);
        assert_eq!(parse_date("soon"), None);
        assert_eq!(parse_date("2020.13.01"), None);
    }

    #[test]
    fn years_round_trip() {
        for (year, month, day) in [(1970, 1, 1), (1998, 7, 31), (2000, 2, 29), (2024, 12, 31)] {
            let ts = days_from_civil(year, month, day) * 86_400;
            assert_eq!(year_of(ts), year as i32);
            assert_eq!(year_of(ts + 86_399), year as i32);
        }
        assert_eq!(year_of(-1), 1969);
    }
}
