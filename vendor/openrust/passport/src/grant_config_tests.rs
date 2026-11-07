//! 授权期限默认值、UTC 日期进位和范围边界。

use super::*;

fn timestamp(value: &str) -> openrust::Result<i64> {
    DateTime::parse_from_rfc3339(value)
        .map(|date| date.timestamp())
        .map_err(openrust::Error::internal)
}

#[test]
fn defaults_match_fixed_passport_and_league() {
    let config = GrantConfig::default();
    assert_eq!(
        config.authorization_code_lifetime,
        GrantLifetime::Seconds(600)
    );
    assert_eq!(
        config.access_token_lifetime,
        GrantLifetime::CalendarYears(1)
    );
    assert_eq!(
        config.refresh_token_lifetime,
        GrantLifetime::CalendarYears(1)
    );
    assert!(config.revoke_refresh_tokens);
    assert_eq!(config.default_scopes, [] as [String; 0]);
}

#[test]
fn calendar_years_follow_php_leap_day_overflow_and_preserve_utc_time() -> openrust::Result<()> {
    for (start, years, end) in [
        ("2024-02-29T12:34:56Z", 1, "2025-03-01T12:34:56Z"),
        ("2024-02-29T12:34:56Z", 4, "2028-02-29T12:34:56Z"),
        ("2023-02-28T23:59:59Z", 1, "2024-02-28T23:59:59Z"),
        ("2023-03-01T00:00:00Z", 1, "2024-03-01T00:00:00Z"),
        ("2024-12-31T23:59:59Z", 1, "2025-12-31T23:59:59Z"),
        ("2096-02-29T00:00:00Z", 4, "2100-03-01T00:00:00Z"),
        ("2024-02-29T12:34:56Z", 0, "2024-02-29T12:34:56Z"),
    ] {
        assert_eq!(
            GrantLifetime::CalendarYears(years).expires_at(timestamp(start)?)?,
            timestamp(end)?
        );
    }
    let start = timestamp("2023-03-01T00:00:00Z")?;
    let end = GrantLifetime::CalendarYears(1).expires_at(start)?;
    assert_eq!(end - start, 366 * 24 * 60 * 60);
    Ok(())
}

#[test]
fn seconds_are_elapsed_time_and_allow_pre_epoch_dates() -> openrust::Result<()> {
    assert_eq!(GrantLifetime::Seconds(600).expires_at(0)?, 600);
    assert_eq!(GrantLifetime::Seconds(1).expires_at(-1)?, 0);
    assert_eq!(GrantLifetime::Seconds(0).expires_at(-1)?, -1);
    assert_eq!(
        GrantLifetime::CalendarYears(1).expires_at(timestamp("1969-01-01T00:00:00Z")?)?,
        0
    );
    Ok(())
}

#[test]
fn invalid_timestamps_and_year_or_second_overflow_are_rejected() {
    for now in [i64::MIN, i64::MAX] {
        assert!(GrantLifetime::Seconds(0).expires_at(now).is_err());
        assert!(GrantLifetime::CalendarYears(0).expires_at(now).is_err());
    }
    assert!(GrantLifetime::Seconds(u64::MAX).expires_at(0).is_err());
    assert!(
        GrantLifetime::CalendarYears(u32::MAX)
            .expires_at(0)
            .is_err()
    );
    let last = DateTime::<Utc>::MAX_UTC.timestamp();
    assert!(GrantLifetime::Seconds(1).expires_at(last).is_err());
    assert!(GrantLifetime::CalendarYears(1).expires_at(last).is_err());
}
