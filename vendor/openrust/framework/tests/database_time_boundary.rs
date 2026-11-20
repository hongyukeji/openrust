use std::{collections::BTreeMap, sync::Arc};

use openrust::{
    config::Repository,
    database::{Connection, DatabaseManager, Value},
};
use serde_json::json;

fn connection(config: serde_json::Value) -> Connection {
    let settings = Repository::new(BTreeMap::from([(
        "database".into(),
        json!({"default": "time_boundary", "connections": {"time_boundary": config}}),
    )]));
    DatabaseManager::new(Arc::new(settings), ".")
        .connection(None)
        .unwrap()
}

#[tokio::test]
async fn sqlite_preserves_date_text_and_fractional_timestamp_bindings() {
    let db = connection(json!({"driver":"sqlite", "database":":memory:"}));
    for stamp in ["2024-02-29 12:34:56", "2024-02-29 12:34:56.123456"] {
        let row = db
            .selectOne(
                "SELECT CAST(? AS TEXT) AS stamp, CAST(? AS TEXT) AS event_date",
                &[stamp.into(), "2024-02-29".into()],
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.get("stamp"), Some(&Value::from(stamp)));
        assert_eq!(row.get("event_date"), Some(&Value::from("2024-02-29")));
    }
}

#[tokio::test]
async fn postgres_native_time_keeps_text_microseconds_and_extended_years() {
    let Ok(url) = std::env::var("OPENRUST_TEST_POSTGRES_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_POSTGRES").is_none());
        return;
    };
    for emulate in [false, true] {
        let db = connection(json!({"url":url, "emulate_prepares":emulate}));
        let table = format!("date_boundary_{}", fastrand::u64(..));
        db.statement(
            &format!("CREATE TABLE {table} (stamp timestamp(6), moment timestamptz(6), event_date date, local_time time(6))"),
            &[],
        )
        .await
        .unwrap();
        for (input, expected) in [
            ("2024-02-29 12:34:56", "2024-02-29 12:34:56"),
            ("2024-02-29 12:34:56.120", "2024-02-29 12:34:56.120"),
            ("2024-02-29 12:34:56.123400", "2024-02-29 12:34:56.123400"),
            ("2024-02-29 12:34:56.123456", "2024-02-29 12:34:56.123456"),
        ] {
            db.statement(&format!("DELETE FROM {table}"), &[])
                .await
                .unwrap();
            db.insert(
                &format!("INSERT INTO {table} VALUES (?, TIMESTAMPTZ '2024-03-01 12:34:56.123456+05:30', DATE '2024-02-29', TIME '12:34:56.120000')"),
                &[input.into()],
            )
            .await
            .unwrap();
            let row = db
                .selectOne(&format!("SELECT * FROM {table}"), &[])
                .await
                .unwrap()
                .unwrap();
            assert_eq!(row.get("stamp"), Some(&Value::from(expected)));
            assert_eq!(
                row.get("moment"),
                Some(&Value::from("2024-03-01T07:04:56.123456+00:00"))
            );
            assert_eq!(row.get("event_date"), Some(&Value::from("2024-02-29")));
            assert_eq!(row.get("local_time"), Some(&Value::from("12:34:56.120")));
        }
        // Native binary decoding retains PostgreSQL dates beyond four-digit years.
        if !emulate {
            let row = db
                .selectOne(
                    "SELECT TIMESTAMP '12000-01-02 03:04:05.123456' AS stamp, DATE '12000-01-02' AS event_date",
                    &[],
                )
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                row.get("stamp"),
                Some(&Value::from("+12000-01-02 03:04:05.123456"))
            );
            assert_eq!(row.get("event_date"), Some(&Value::from("+12000-01-02")));
        }
        db.statement(&format!("DROP TABLE {table}"), &[])
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn mysql_native_time_keeps_datetime_and_time_precision() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    for emulate in [false, true] {
        let db = connection(json!({"url":url, "emulate_prepares":emulate, "timezone":"+00:00"}));
        let table = format!("date_boundary_{}", fastrand::u64(..));
        db.statement(
            &format!("CREATE TABLE {table} (stamp DATETIME(6), moment TIMESTAMP(6), event_date DATE, local_time TIME(6))"),
            &[],
        )
        .await
        .unwrap();
        for stamp in [
            "2024-02-29 12:34:56",
            "2024-02-29 12:34:56.120",
            "2024-02-29 12:34:56.123400",
            "2024-02-29 12:34:56.123456",
        ] {
            db.statement(&format!("DELETE FROM {table}"), &[])
                .await
                .unwrap();
            db.insert(
                &format!("INSERT INTO {table} VALUES (?, ?, '2024-02-29', '12:34:56.120000')"),
                &[stamp.into(), stamp.into()],
            )
            .await
            .unwrap();
            let row = db
                .selectOne(&format!("SELECT * FROM {table}"), &[])
                .await
                .unwrap()
                .unwrap();
            assert_eq!(row.get("stamp"), Some(&Value::from(stamp)));
            assert_eq!(row.get("moment"), Some(&Value::from(stamp)));
            assert_eq!(row.get("event_date"), Some(&Value::from("2024-02-29")));
            assert_eq!(row.get("local_time"), Some(&Value::from("12:34:56.12")));
        }
        db.statement(&format!("DROP TABLE {table}"), &[])
            .await
            .unwrap();
    }
}

#[test]
fn tiberius_native_time_decodes_legacy_dates_offsets_fractional_and_null_values() {
    use tiberius::{ColumnData, FromSql as _, time as tds};
    use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};

    let legacy = ColumnData::DateTime(Some(tds::DateTime::new(-1, 300)));
    let legacy = PrimitiveDateTime::from_sql(&legacy).unwrap().unwrap();
    assert_eq!(
        legacy.date(),
        Date::from_calendar_date(1899, Month::December, 31).unwrap()
    );
    assert_eq!(legacy.time(), Time::from_hms(0, 0, 1).unwrap());

    let midnight = tds::DateTime2::new(tds::Date::new(0), tds::Time::new(1234567, 7));
    let precise = PrimitiveDateTime::from_sql(&ColumnData::DateTime2(Some(midnight)))
        .unwrap()
        .unwrap();
    assert_eq!(precise.nanosecond(), 123_456_700);
    let offset = OffsetDateTime::from_sql(&ColumnData::DateTimeOffset(Some(
        tds::DateTimeOffset::new(midnight, 330),
    )))
    .unwrap()
    .unwrap();
    assert_eq!(offset.offset().whole_seconds(), 19_800);
    assert_eq!(
        offset.unix_timestamp_nanos(),
        precise.assume_utc().unix_timestamp_nanos()
    );
    assert_eq!(
        PrimitiveDateTime::from_sql(&ColumnData::DateTime2(None)).unwrap(),
        None
    );
    assert_eq!(Time::from_sql(&ColumnData::Time(None)).unwrap(), None);
    assert!(
        OffsetDateTime::from_sql(&ColumnData::DateTimeOffset(Some(tds::DateTimeOffset::new(
            midnight, 841,
        ))))
        .is_err()
    );
}
