use super::timestamp_text;
use jiff::Timestamp;

#[test]
fn token_attribute_timestamps_preserve_fractional_precision() {
    for (nanoseconds, fraction) in [
        (0, ""),
        (120_000_000, ".120"),
        (123_400_000, ".123400"),
        (123_456_789, ".123456789"),
        (1, ".000000001"),
    ] {
        for (seconds, civil) in [(0, "1970-01-01T00:00:00"), (-1, "1969-12-31T23:59:59")] {
            let value = Timestamp::new(seconds, nanoseconds).unwrap();
            let text = timestamp_text(value);
            assert_eq!(text, format!("{civil}{fraction}+00:00"));
            assert_eq!(text.parse::<Timestamp>().unwrap(), value);
        }
    }
}
