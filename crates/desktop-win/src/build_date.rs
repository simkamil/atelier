// SPDX-License-Identifier: AGPL-3.0-or-later
// Civil date from Unix days, using the Gregorian 400-year era decomposition.
pub(crate) fn build_date(seconds: u64) -> String {
    let days = i64::try_from(seconds / 86_400).expect("build epoch within date range");
    assert!(days <= 2_932_896, "build date must be before year 10000");
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::build_date;
    #[test]
    fn build_dates_follow_utc_and_reproducible_epoch() {
        assert_eq!(build_date(0), "1970-01-01");
        assert_eq!(build_date(86_399), "1970-01-01");
        assert_eq!(build_date(86_400), "1970-01-02");
        assert_eq!(build_date(951_782_400), "2000-02-29");
        assert_eq!(build_date(4_107_542_400), "2100-03-01");
        assert_eq!(build_date(253_402_300_799), "9999-12-31");
    }
}
