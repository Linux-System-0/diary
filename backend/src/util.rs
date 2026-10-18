use time::{
    Date, OffsetDateTime, PrimitiveDateTime, UtcOffset,
    format_description::well_known::Rfc3339,
};

pub fn now_unix() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

pub fn to_rfc3339(timestamp: i64) -> String {
    OffsetDateTime::from_unix_timestamp(timestamp)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH)
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

pub fn parse_rfc3339(value: &str) -> Option<i64> {
    OffsetDateTime::parse(value, &Rfc3339)
        .ok()
        .map(|time| time.unix_timestamp())
}

pub fn today_utc() -> Date {
    OffsetDateTime::now_utc().date()
}

/// Calendar dates are opaque days, not instants: no timezone conversion.
/// Accepted range is 1970-01-01 through UTC today + 1 day.
pub fn valid_entry_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        return false;
    }
    let Ok(date) = Date::parse(value, time::macros::format_description!("[year]-[month]-[day]")) else {
        return false;
    };
    valid_calendar_date(date)
}

/// Filters (`from`/`to`) may point into the future; only writes are capped at
/// UTC today + 1 day.
pub fn valid_filter_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
    {
        return false;
    }
    Date::parse(value, time::macros::format_description!("[year]-[month]-[day]"))
        .is_ok_and(|date| date.year() >= 1970)
}

pub fn valid_calendar_date(date: Date) -> bool {
    date.year() >= 1970 && date <= today_utc().next_day().unwrap_or(today_utc())
}

/// Local midnight represented as UTC parts, used only to compare calendar days.
pub fn date_to_midnight_utc(date: Date) -> OffsetDateTime {
    PrimitiveDateTime::new(date, time::Time::MIDNIGHT).assume_offset(UtcOffset::UTC)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_roundtrip() {
        let timestamp = 1_700_000_000;
        let text = to_rfc3339(timestamp);
        assert_eq!(parse_rfc3339(&text), Some(timestamp));
        assert_eq!(parse_rfc3339("not a date"), None);
    }

    #[test]
    fn entry_dates_are_checked() {
        assert!(valid_entry_date("2024-02-29"));
        assert!(!valid_entry_date("2023-02-29"));
        assert!(!valid_entry_date("1969-12-31"));
        assert!(!valid_entry_date("2024-1-1"));
        assert!(!valid_entry_date(""));
    }
}
