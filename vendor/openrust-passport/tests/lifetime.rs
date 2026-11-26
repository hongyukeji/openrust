use openrust::support::{Date, DateInterval};
use openrust_passport::GrantLifetime;
fn stamp(date: &str) -> i64 {
    date.parse::<jiff::Timestamp>().unwrap().as_second()
}
#[test]
fn calendar_years_carry_leap_days_and_keep_the_selected_timezone() {
    let start = stamp("2024-02-29T12:00:00Z");
    assert_eq!(
        GrantLifetime::CalendarYears(1).expires_at(start).unwrap(),
        stamp("2025-03-01T12:00:00Z")
    );
    assert_eq!(
        GrantLifetime::CalendarYears(4).expires_at(start).unwrap(),
        stamp("2028-02-29T12:00:00Z")
    );
    let start = stamp("2024-03-09T12:00:00-05:00");
    assert_eq!(
        GrantLifetime::CalendarYears(1)
            .expires_at_in(start, Date::timezone("America/New_York").unwrap())
            .unwrap(),
        stamp("2025-03-09T12:00:00-04:00")
    );
}
#[test]
fn seconds_and_native_signed_intervals_preserve_their_distinct_meaning() {
    let start = stamp("2024-01-31T12:00:00Z");
    assert_eq!(
        GrantLifetime::Seconds(3600).expires_at(start).unwrap(),
        start + 3600
    );
    assert_eq!(
        GrantLifetime::Interval(DateInterval {
            months: 1,
            ..Default::default()
        })
        .expires_at(start)
        .unwrap(),
        stamp("2024-03-02T12:00:00Z")
    );
    assert_eq!(
        GrantLifetime::Interval(DateInterval {
            microseconds: -1_000_000,
            ..Default::default()
        })
        .expires_at(start)
        .unwrap(),
        start - 1
    );
    assert!(GrantLifetime::Seconds(u64::MAX).expires_at(start).is_err());
    assert!(
        GrantLifetime::CalendarYears(u32::MAX)
            .expires_at(start)
            .is_err()
    );
}
