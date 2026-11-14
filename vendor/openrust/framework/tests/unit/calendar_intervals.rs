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

#[test]
fn php_date_display_preserves_extended_years_and_historical_offsets() {
    use super::format_php_date;
    let timezone = chrono_tz::Europe::Paris;
    for (year, offset, expected) in [
        (
            -1,
            561,
            "-0001 -1 -0001 -0001 -2 53 -001-01-01T00:00:00+00:09 Fri, 01 Jan -001 00:00:00 +0009 +00:09 +0009 +00:09 561 LMT",
        ),
        (
            10000,
            3600,
            "10000 00 +10000 +10000 9999 52 10000-01-01T00:00:00+01:00 Sat, 01 Jan 10000 00:00:00 +0100 +01:00 +0100 +01:00 3600 CET",
        ),
    ] {
        let local = chrono::NaiveDate::from_ymd_opt(year, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let utc = local.and_utc() - chrono::TimeDelta::seconds(offset);
        assert_eq!(
            format_php_date(utc, timezone, "Y y X x o W c r P O p Z T").unwrap(),
            expected,
        );
    }
}

#[test]
fn php_timestamp_expressions_follow_named_timezone_token_boundaries() {
    use super::parse_timestamp_expression;

    let reference = DateTime::UNIX_EPOCH;
    for (suffix, expected) in [
        ("America/New_York", Some(1_704_130_496)),
        ("America/NEW_YORK", Some(1_704_130_496)),
        ("Us/Eastern", Some(1_704_130_496)),
        ("america/New_York", None),
        ("AMERICA/New_York", None),
        ("US/Eastern", None),
        ("America/New_York-5", Some(1_704_130_496)),
        ("America/New_York- 5", None),
        ("Etc/GMT-5", Some(1_704_112_496)),
        ("Etc/GMT+5", Some(1_704_112_496)),
        ("Etc/GMT-", None),
        ("Asia/Ho_Chi_Minh", Some(1_704_087_296)),
    ] {
        let expression = format!("2024-01-01 12:34:56 {suffix}");
        assert_eq!(
            parse_timestamp_expression(&expression, reference, UTC).ok(),
            expected,
            "{expression}"
        );
    }
}
