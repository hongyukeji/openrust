use std::time::{Duration, SystemTime, UNIX_EPOCH};

use openrust::support::{OrderedUuidError, Str};

fn prefix(uuid: &uuid::Uuid) -> u64 {
    let mut time = [0_u8; 8];
    time[2..].copy_from_slice(&uuid.as_bytes()[..6]);
    u64::from_be_bytes(time)
}

#[test]
fn timestamp_first_comb_preserves_version_variant_and_time_order() -> Result<(), OrderedUuidError> {
    let time = UNIX_EPOCH + Duration::from_secs(1_700_000_000) + Duration::from_micros(123_456);
    let first = Str::ordered_uuid_at(time)?;
    let same_bucket = Str::ordered_uuid_at(time + Duration::from_micros(3))?;
    let next = Str::ordered_uuid_at(time + Duration::from_micros(4))?;
    assert_eq!(prefix(&first), 170_000_000_012_345);
    assert_eq!(prefix(&same_bucket), prefix(&first));
    assert_eq!(prefix(&next), prefix(&first) + 1);
    assert!(first < next);
    assert_eq!(first.get_version_num(), 4);
    assert_eq!(first.get_variant(), uuid::Variant::RFC4122);
    assert!(Str::is_uuid(&first.to_string(), Some(4)));
    assert!(!Str::is_uuid(&first.to_string(), Some(7)));
    Ok(())
}

#[test]
fn timestamp_range_and_system_clock() -> Result<(), OrderedUuidError> {
    let outside = UNIX_EPOCH + Duration::from_micros((1 << 48) * 10);
    assert_eq!(
        Str::ordered_uuid_at(outside),
        Err(OrderedUuidError::TimestampOutOfRange)
    );
    assert_eq!(
        Str::ordered_uuid_at(UNIX_EPOCH - Duration::from_nanos(1)),
        Err(OrderedUuidError::TimestampOutOfRange)
    );
    assert_eq!(
        prefix(&Str::ordered_uuid_at(outside - Duration::from_micros(1))?),
        (1 << 48) - 1
    );
    let ticks = |now: SystemTime| {
        u64::try_from(now.duration_since(UNIX_EPOCH).unwrap().as_micros() / 10).unwrap()
    };
    let before = ticks(SystemTime::now());
    let uuid = Str::ordered_uuid()?;
    let after = ticks(SystemTime::now());
    assert!((before..=after).contains(&prefix(&uuid)));
    Ok(())
}
