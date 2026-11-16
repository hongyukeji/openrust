use chrono::{NaiveDateTime, Timelike};
use openrust::support::{
    Date, DateError, DateInterval, DateTimezone, MonthOverflow, days, hours, milliseconds, months,
    now, today, weeks, years,
};

fn local(text: &str, zone: DateTimezone) -> Date {
    Date::fromLocal(text.parse::<NaiveDateTime>().unwrap(), zone).unwrap()
}

#[test]
fn month_and_year_arithmetic_respects_the_overflow_policy() -> Result<(), DateError> {
    let original = local("2024-01-31T12:00:00", DateTimezone::UTC);
    assert_eq!(
        original
            .plus(months(1), MonthOverflow::Carry)?
            .asDateTime()
            .format("%F")
            .to_string(),
        "2024-03-02"
    );
    assert_eq!(
        original
            .plus(months(1), MonthOverflow::Clamp)?
            .asDateTime()
            .format("%F")
            .to_string(),
        "2024-02-29"
    );
    let leap_day = local("2024-02-29T12:00:00", DateTimezone::UTC);
    assert_eq!(
        leap_day
            .plus(years(1), MonthOverflow::Carry)?
            .asDateTime()
            .format("%F")
            .to_string(),
        "2025-03-01"
    );
    assert_eq!(
        original.minus(
            DateInterval {
                months: i64::MIN,
                ..Default::default()
            },
            MonthOverflow::Carry
        ),
        Err(DateError::OutOfRange)
    );
    Ok(())
}

#[test]
fn civil_days_and_nonexistent_local_times_follow_timezone_transitions() -> Result<(), DateError> {
    let zone = "America/New_York".parse().unwrap();
    let start = local("2024-03-09T12:00:00", zone);
    let result = start.plus(days(1.0)?, MonthOverflow::Carry)?;
    assert_eq!(
        result.asDateTime().to_rfc3339(),
        "2024-03-10T12:00:00-04:00"
    );
    assert_eq!(
        result
            .asDateTime()
            .signed_duration_since(*start.asDateTime())
            .num_hours(),
        23
    );
    let gap = local("2024-03-10T01:30:00", zone).plus(hours(1.0)?, MonthOverflow::Carry)?;
    assert_eq!(gap.asDateTime().to_rfc3339(), "2024-03-10T03:30:00-04:00");
    let elapsed = local("2024-03-10T01:30:00", zone).plus(hours(2.0)?, MonthOverflow::Carry)?;
    assert_eq!(
        elapsed.asDateTime().to_rfc3339(),
        "2024-03-10T04:30:00-04:00"
    );
    Ok(())
}

#[test]
fn clock_helpers_and_interval_boundaries() -> Result<(), DateError> {
    let zone = "Asia/Shanghai".parse().unwrap();
    assert_eq!(now(Some(zone))?.asDateTime().timezone(), zone);
    let today = today(Some(zone))?;
    assert_eq!(today.asDateTime().time(), chrono::NaiveTime::MIN);
    let start = local("2024-03-09T12:00:00", zone);
    let result = start.plus(milliseconds(1.5)?, MonthOverflow::Carry)?;
    assert_eq!(result.asDateTime().nanosecond(), 1_500_000);
    assert_eq!(weeks(1)?.days, 7);
    assert_eq!(weeks(i64::MAX), Err(DateError::OutOfRange));
    assert_eq!(days(f64::NAN), Err(DateError::OutOfRange));
    Ok(())
}
