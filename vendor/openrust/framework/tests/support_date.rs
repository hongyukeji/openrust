use jiff::civil::DateTime;
use openrust::support::{
    Date, DateError, DateInterval, DateTimezone, MonthOverflow, days, hours, milliseconds, months,
    now, today, weeks, years,
};

fn local(text: &str, zone: DateTimezone) -> Date {
    Date::fromLocal(text.parse::<DateTime>().unwrap(), zone).unwrap()
}

#[test]
fn month_and_year_arithmetic_respects_the_overflow_policy() -> Result<(), DateError> {
    let original = local("2024-01-31T12:00:00", DateTimezone::UTC);
    assert_eq!(
        original
            .plus(months(1), MonthOverflow::Carry)?
            .asDateTime()
            .strftime("%F")
            .to_string(),
        "2024-03-02"
    );
    assert_eq!(
        original
            .plus(months(1), MonthOverflow::Clamp)?
            .asDateTime()
            .strftime("%F")
            .to_string(),
        "2024-02-29"
    );
    let leap_day = local("2024-02-29T12:00:00", DateTimezone::UTC);
    assert_eq!(
        leap_day
            .plus(years(1), MonthOverflow::Carry)?
            .asDateTime()
            .strftime("%F")
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
    let zone = Date::timezone("America/New_York").unwrap();
    let start = local("2024-03-09T12:00:00", zone.clone());
    let result = start.plus(days(1.0)?, MonthOverflow::Carry)?;
    assert_eq!(
        result
            .asDateTime()
            .strftime("%Y-%m-%dT%H:%M:%S%.f%:z")
            .to_string(),
        "2024-03-10T12:00:00-04:00"
    );
    assert_eq!(
        result
            .asDateTime()
            .duration_since(start.asDateTime())
            .as_hours(),
        23
    );
    let gap = local("2024-03-10T01:30:00", zone.clone()).plus(hours(1.0)?, MonthOverflow::Carry)?;
    assert_eq!(
        gap.asDateTime()
            .strftime("%Y-%m-%dT%H:%M:%S%.f%:z")
            .to_string(),
        "2024-03-10T03:30:00-04:00"
    );
    let elapsed =
        local("2024-03-10T01:30:00", zone.clone()).plus(hours(2.0)?, MonthOverflow::Carry)?;
    assert_eq!(
        elapsed
            .asDateTime()
            .strftime("%Y-%m-%dT%H:%M:%S%.f%:z")
            .to_string(),
        "2024-03-10T04:30:00-04:00"
    );
    Ok(())
}

#[test]
fn clock_helpers_and_interval_boundaries() -> Result<(), DateError> {
    let zone = Date::timezone("Asia/Shanghai").unwrap();
    assert_eq!(now(Some(zone.clone()))?.asDateTime().time_zone(), &zone);
    let today = today(Some(zone.clone()))?;
    assert_eq!(today.asDateTime().time(), jiff::civil::Time::MIN);
    let start = local("2024-03-09T12:00:00", zone.clone());
    let result = start.plus(milliseconds(1.5)?, MonthOverflow::Carry)?;
    assert_eq!(result.asDateTime().subsec_nanosecond(), 1_500_000);
    assert_eq!(weeks(1)?.days, 7);
    assert_eq!(weeks(i64::MAX), Err(DateError::OutOfRange));
    assert_eq!(days(f64::NAN), Err(DateError::OutOfRange));
    Ok(())
}

#[test]
fn fold_keeps_the_existing_offset_and_local_gap_moves_forward() -> Result<(), DateError> {
    let zone = Date::timezone("America/New_York").unwrap();
    let first = local("2024-11-03T01:30:00", zone.clone());
    assert_eq!(first.asDateTime().offset().seconds(), -4 * 3600);
    let second = Date::fromUtc("2024-11-03T06:30:00Z".parse().unwrap(), zone.clone());
    assert_eq!(
        second
            .plus(days(0.0)?, MonthOverflow::Carry)?
            .asDateTime()
            .offset()
            .seconds(),
        -5 * 3600
    );
    let previous_day = second.minus(days(1.0)?, MonthOverflow::Carry)?;
    assert_eq!(
        previous_day
            .plus(days(1.0)?, MonthOverflow::Carry)?
            .asDateTime()
            .offset()
            .seconds(),
        -4 * 3600
    );
    let gap = local("2024-03-10T02:30:00", zone);
    assert_eq!(gap.asDateTime().hour(), 3);
    assert_eq!(gap.asDateTime().minute(), 30);
    Ok(())
}

#[test]
fn framework_timezone_uses_fixed_2025b_even_when_tzdir_differs() {
    const MARKER: &str = "OPENRUST_TEST_FIXED_TZDB_CHILD";
    if std::env::var_os(MARKER).is_some() {
        let instant: jiff::Timestamp = "2026-01-15T12:00:00Z".parse().unwrap();
        let configured = Date::timezone("America/New_York").unwrap();
        assert_eq!(configured.to_offset(instant).seconds(), -5 * 3600);
        let system = jiff::tz::TimeZone::get("America/New_York").unwrap();
        assert_eq!(system.to_offset(instant).seconds(), 9 * 3600);
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let zone = directory.path().join("America/New_York");
    std::fs::create_dir_all(zone.parent().unwrap()).unwrap();
    let mut tzif = b"TZif\0".to_vec();
    tzif.extend([0; 15]);
    for count in [0_u32, 0, 0, 0, 1, 4] {
        tzif.extend(count.to_be_bytes());
    }
    tzif.extend(32_400_i32.to_be_bytes());
    tzif.extend([0, 0]);
    tzif.extend(b"JST\0");
    std::fs::write(&zone, tzif).unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "framework_timezone_uses_fixed_2025b_even_when_tzdir_differs",
            "--nocapture",
        ])
        .env(MARKER, "1")
        .env("TZDIR", directory.path())
        .status()
        .unwrap();
    assert!(status.success());
}
