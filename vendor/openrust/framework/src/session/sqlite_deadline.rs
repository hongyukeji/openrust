//! SQLite Session 截止时间的纯值转换，不打开数据库。

use super::invalid_record;
use crate::Result;
use chrono::{DateTime, Utc};
use time::OffsetDateTime;

pub(super) fn to_offset(expiry: DateTime<Utc>) -> Result<OffsetDateTime> {
    OffsetDateTime::from_unix_timestamp(expiry.timestamp())
        .map_err(|_| invalid_record())?
        .replace_nanosecond(expiry.timestamp_subsec_nanos())
        .map_err(|_| invalid_record())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::SecondsFormat;

    #[test]
    fn utc_deadline_decoding_preserves_microsecond_cutoff_and_offsets() -> Result<()> {
        let expiry = DateTime::parse_from_rfc3339("2026-11-03T12:34:56.123456+08:00")
            .map_err(|_| invalid_record())?
            .with_timezone(&Utc);
        let offset = to_offset(expiry)?;
        assert_eq!(offset.nanosecond(), 123_456_000);
        assert_eq!(offset.unix_timestamp(), expiry.timestamp());
        assert_eq!(
            expiry.to_rfc3339_opts(SecondsFormat::Micros, true),
            "2026-11-03T04:34:56.123456Z"
        );
        Ok(())
    }
}
