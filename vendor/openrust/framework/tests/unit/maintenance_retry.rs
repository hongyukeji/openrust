use super::retry_datetime;
use crate::support::Date;
use jiff::Timestamp;

#[test]
fn local_retry_dates_reject_gaps_and_choose_the_earlier_fold() {
    let zone = Date::timezone("America/New_York").unwrap();
    let now = Timestamp::UNIX_EPOCH;
    assert!(retry_datetime("2024-03-10 02:30:00", zone.clone(), now).is_none());
    assert_eq!(
        retry_datetime("2024-11-03 01:30:00", zone, now).unwrap(),
        "2024-11-03T05:30:00Z".parse::<Timestamp>().unwrap()
    );
}

#[test]
fn explicit_retry_offsets_preserve_the_instant() {
    let zone = Date::timezone("Asia/Tokyo").unwrap();
    let expected = "1969-12-31T23:59:59Z".parse::<Timestamp>().unwrap();
    for value in ["1969-12-31T23:59:59Z", "1970-01-01T01:59:59+02:00", "@-1"] {
        assert_eq!(
            retry_datetime(value, zone.clone(), Timestamp::UNIX_EPOCH),
            Some(expected)
        );
    }
}
