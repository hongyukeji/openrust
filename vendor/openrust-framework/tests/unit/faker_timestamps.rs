use super::rfc3339_seconds;
use crate::support::Date;
use jiff::Timestamp;

#[test]
fn rfc3339_seconds_rounds_historical_offsets_to_minutes() {
    for (instant, zone, expected) in [
        (
            "1890-01-01T00:00:00Z",
            "Europe/Paris",
            "1890-01-01T00:09:21+00:09",
        ),
        (
            "1890-01-01T00:00:00Z",
            "Africa/Monrovia",
            "1889-12-31T23:16:52-00:43",
        ),
        (
            "1890-01-01T00:00:00Z",
            "America/New_York",
            "1889-12-31T19:00:00-05:00",
        ),
        (
            "1969-12-31T23:59:59.999999Z",
            "UTC",
            "1969-12-31T23:59:59+00:00",
        ),
    ] {
        let date = instant
            .parse::<Timestamp>()
            .unwrap()
            .to_zoned(Date::timezone(zone).unwrap());
        assert_eq!(rfc3339_seconds(&date), expected);
    }
}
