use super::date_interval;
use chrono::{DateTime, Utc};
use chrono_tz::UTC;

#[test]
fn php_relative_intervals_preserve_the_start_calendar() {
    let reference = DateTime::UNIX_EPOCH;
    for (date, interval, expected) in [
        (
            "2024-01-31 12:34:56 UTC",
            "first day of next month",
            "2024-03-02T12:34:56Z",
        ),
        (
            "2024-01-31 12:34:56 UTC",
            "first Monday of next month",
            "2024-02-05T12:34:56Z",
        ),
        (
            "2024-01-31 12:34:56 UTC",
            "tomorrow",
            "2024-02-01T12:34:56Z",
        ),
        (
            "2024-01-31 12:34:56 UTC",
            "5 days ago",
            "2024-01-26T12:34:56Z",
        ),
        (
            "2024-03-09 12:00 America/New_York",
            "+1 day",
            "2024-03-10T16:00:00Z",
        ),
        (
            "2024-03-09 12:00 America/New_York",
            "+24 hours",
            "2024-03-10T16:00:00Z",
        ),
        (
            "2024-11-02 01:30 America/New_York",
            "+1 day",
            "2024-11-03T05:30:00Z",
        ),
        (
            "2024-11-04 01:30 America/New_York",
            "-1 day",
            "2024-11-03T06:30:00Z",
        ),
        (
            "2024-10-26 02:30 Europe/Paris",
            "+1 day",
            "2024-10-27T00:30:00Z",
        ),
        (
            "2024-10-28 02:30 Europe/Paris",
            "-1 day",
            "2024-10-27T01:30:00Z",
        ),
    ] {
        let (_, end) = date_interval(date, interval, reference, UTC).unwrap();
        let expected = DateTime::parse_from_rfc3339(expected)
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(end, expected, "{date} + {interval}");
    }

    for invalid in [
        "",
        "P1D",
        "noon",
        "1 day UTC",
        "1 day 12:00",
        "1 day 2024",
        "1.5 days",
    ] {
        assert!(
            date_interval("now", invalid, reference, UTC).is_err(),
            "{invalid:?}"
        );
    }
}
