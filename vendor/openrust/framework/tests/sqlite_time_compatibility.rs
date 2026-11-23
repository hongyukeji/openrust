use sqlx::{Connection, SqliteConnection};
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

#[tokio::test]
async fn sqlite_time_formats_preserve_optional_parts_and_extended_years() {
    let mut db = SqliteConnection::connect("sqlite::memory:").await.unwrap();
    let date = Date::from_calendar_date(2024, Month::February, 29).unwrap();
    for (input, hour, minute, second, nanos) in [
        ("2024-02-29 23:59", 23, 59, 0, 0),
        ("2024-02-29T23:59:58Z", 23, 59, 58, 0),
        ("2024-02-29 23:59:58.123456789", 23, 59, 58, 123_456_789),
        ("2024-02-29T00:00:00.000001Z", 0, 0, 0, 1_000),
    ] {
        let actual: PrimitiveDateTime = sqlx::query_scalar("SELECT ?")
            .bind(input)
            .fetch_one(&mut db)
            .await
            .unwrap();
        assert_eq!(
            actual,
            date.with_time(Time::from_hms_nano(hour, minute, second, nanos).unwrap()),
            "{input}"
        );
    }
    // 覆盖兼容解析分支，不能仅凭标准 RFC 3339 路径通过证明迁移正确。
    for (input, year) in [("+12000-01-02T03:04Z", 12000), ("-0001-01-02 03:04Z", -1)] {
        let actual: PrimitiveDateTime = sqlx::query_scalar("SELECT ?")
            .bind(input)
            .fetch_one(&mut db)
            .await
            .unwrap();
        assert_eq!(actual.year(), year);
        assert_eq!(actual.as_hms(), (3, 4, 0));
    }
    for input in ["2023-02-29 12:00", "2024-02-29T25:00", "invalid"] {
        assert!(
            sqlx::query_scalar::<_, PrimitiveDateTime>("SELECT ?")
                .bind(input)
                .fetch_one(&mut db)
                .await
                .is_err(),
            "{input}"
        );
    }
}

#[tokio::test]
async fn sqlite_time_formats_preserve_offsets_binding_null_and_unix_seconds() {
    let mut db = SqliteConnection::connect("sqlite::memory:").await.unwrap();
    let local = Date::from_calendar_date(2024, Month::February, 29)
        .unwrap()
        .with_hms(23, 59, 0)
        .unwrap();
    for (input, hours, minutes) in [
        ("2024-02-29 23:59+05:30", 5, 30),
        ("2024-02-29T23:59-03:30", -3, -30),
        ("2024-02-29 23:59", 0, 0),
        ("2024-02-29T23:59:00Z", 0, 0),
    ] {
        let actual: OffsetDateTime = sqlx::query_scalar("SELECT ?")
            .bind(input)
            .fetch_one(&mut db)
            .await
            .unwrap();
        let expected = local.assume_offset(UtcOffset::from_hms(hours, minutes, 0).unwrap());
        assert_eq!(actual, expected, "{input}");
        assert_eq!(actual.offset(), expected.offset(), "{input}");
    }
    for value in [local, local.replace_nanosecond(123_456_789).unwrap()] {
        let actual: PrimitiveDateTime = sqlx::query_scalar("SELECT ?")
            .bind(value)
            .fetch_one(&mut db)
            .await
            .unwrap();
        assert_eq!(actual, value);
    }
    let null: Option<PrimitiveDateTime> = sqlx::query_scalar("SELECT ?")
        .bind(None::<PrimitiveDateTime>)
        .fetch_one(&mut db)
        .await
        .unwrap();
    assert_eq!(null, None);
    for seconds in [-1_i64, 0, 1_709_251_200] {
        let actual: OffsetDateTime = sqlx::query_scalar("SELECT ?")
            .bind(seconds)
            .fetch_one(&mut db)
            .await
            .unwrap();
        assert_eq!(actual.unix_timestamp(), seconds);
    }
}
