use super::timestamp_text;
use jiff::{Timestamp, civil::DateTime, tz::TimeZone};

#[test]
fn database_timestamp_text_preserves_fractional_precision() {
    for (nanoseconds, fraction) in [
        (0, ""),
        (120_000_000, ".120"),
        (123_400_000, ".123400"),
        (123_456_789, ".123456789"),
        (1, ".000000001"),
    ] {
        for (seconds, civil) in [(0, "1970-01-01 00:00:00"), (-1, "1969-12-31 23:59:59")] {
            let value = Timestamp::new(seconds, nanoseconds).unwrap();
            let text = timestamp_text(value);
            assert_eq!(text, format!("{civil}{fraction}"));
            assert_eq!(
                DateTime::strptime("%Y-%m-%d %H:%M:%S%.f", &text)
                    .unwrap()
                    .to_zoned(TimeZone::UTC)
                    .unwrap()
                    .timestamp(),
                value
            );
        }
    }
}
