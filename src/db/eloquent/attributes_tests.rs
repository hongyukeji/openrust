//! 检出 raw 水合的数值舍入、NULL 伪装、类型自动转换和重名列错误索引。

use super::*;
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
use sea_orm::DbBackend;
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};
use serde_json::json;

#[test]
fn non_finite_values_never_become_json_null() -> Result<()> {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(
            finite(value),
            Err(error) if error.code() == "model_database_attribute_non_finite"
                && error.status().as_u16() == 500
        ));
    }
    assert_eq!(finite(1.125)?, json!(1.125));
    assert_eq!(bytes(vec![0, 255, 128]), json!([0, 255, 128]));
    Ok(())
}

async fn query(connection: &DatabaseConnection, sql: &str) -> Result<QueryResult> {
    connection
        .query_one_raw(Statement::from_string(
            connection.get_database_backend(),
            sql.to_owned(),
        ))
        .await
        .map_err(Error::internal)?
        .ok_or_else(|| Error::custom("test_row_missing", "属性适配测试未取得行"))
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn sqlite_storage_class_preserves_large_integer_text_blob_and_null() -> Result<()> {
    let connection = sea_orm::Database::connect("sqlite::memory:")
        .await
        .map_err(Error::internal)?;
    let row = query(&connection, "SELECT 9007199254740993 AS big, -10 AS negative, 1.125 AS real_value, '{\"id\":9007199254740993}' AS json_text, '2026-11-06 12:34:56.120000' AS date_text, X'00ff80' AS binary_value, NULL AS nullable").await?;
    let attributes = attributes_from_row(&row)?;
    assert_eq!(attributes.len(), 7);
    assert_eq!(attributes["big"], json!(9_007_199_254_740_993i64));
    assert_eq!(attributes["negative"], json!(-10));
    assert_eq!(attributes["real_value"], json!(1.125));
    assert_eq!(attributes["json_text"], json!("{\"id\":9007199254740993}"));
    assert_eq!(attributes["date_text"], json!("2026-11-06 12:34:56.120000"));
    assert_eq!(attributes["binary_value"], json!([0, 255, 128]));
    assert_eq!(attributes["nullable"], Value::Null);
    Ok(())
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn sqlite_declared_boolean_does_not_cast_storage_class_and_duplicate_names_use_last()
-> Result<()> {
    let connection = sea_orm::Database::connect("sqlite::memory:")
        .await
        .map_err(Error::internal)?;
    connection
        .execute_unprepared("CREATE TABLE raw_attributes (enabled BOOLEAN, label TEXT)")
        .await
        .map_err(Error::internal)?;
    connection
        .execute_unprepared("INSERT INTO raw_attributes VALUES (7, '00042'), ('text', 'text')")
        .await
        .map_err(Error::internal)?;
    let first = attributes_from_row(
        &query(
            &connection,
            "SELECT * FROM raw_attributes WHERE label = '00042'",
        )
        .await?,
    )?;
    assert_eq!(first["enabled"], json!(7));
    assert_eq!(first["label"], json!("00042"));
    let second = attributes_from_row(
        &query(
            &connection,
            "SELECT * FROM raw_attributes WHERE label = 'text'",
        )
        .await?,
    )?;
    assert_eq!(second["enabled"], json!("text"));
    let duplicate = attributes_from_row(
        &query(&connection, "SELECT 1 AS repeated, 'last' AS repeated").await?,
    )?;
    assert_eq!(
        duplicate,
        Map::from_iter([("repeated".into(), json!("last"))])
    );
    Ok(())
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn sqlite_real_infinity_and_invalid_utf8_are_explicit_errors() -> Result<()> {
    let connection = sea_orm::Database::connect("sqlite::memory:")
        .await
        .map_err(Error::internal)?;
    assert!(
        matches!(attributes_from_row(&query(&connection, "SELECT 1e999 AS non_finite").await?), Err(error) if error.code() == "model_database_attribute_non_finite")
    );
    assert!(
        matches!(attributes_from_row(&query(&connection, "SELECT CAST(X'ff' AS TEXT) AS bad_text").await?), Err(error) if error.code() == "model_database_attribute_invalid")
    );
    Ok(())
}

#[cfg(feature = "db-postgres")]
fn numeric_wire(weight: i16, sign: u16, scale: u16, digits: &[u16]) -> Result<Vec<u8>> {
    let count = u16::try_from(digits.len()).map_err(Error::internal)?;
    let mut payload = Vec::with_capacity(8 + digits.len() * 2);
    payload.extend_from_slice(&count.to_be_bytes());
    payload.extend_from_slice(&weight.to_be_bytes());
    payload.extend_from_slice(&sign.to_be_bytes());
    payload.extend_from_slice(&scale.to_be_bytes());
    for digit in digits {
        payload.extend_from_slice(&digit.to_be_bytes());
    }
    Ok(payload)
}

#[cfg(feature = "db-postgres")]
#[test]
fn postgres_numeric_wire_preserves_scale_large_weight_and_more_than_28_decimal_places() -> Result<()>
{
    for (weight, sign, scale, digits, expected) in [
        (0, 0, 0, vec![], "0"),
        (0, 0, 4, vec![], "0.0000"),
        (0, 0x4000, 3, vec![], "0.000"),
        (0, 0, 4, vec![1, 2345], "1.2345"),
        (0, 0x4000, 4, vec![1, 2500], "-1.2500"),
        (2, 0, 0, vec![1], "100000000"),
        (-2, 0, 8, vec![1234], "0.00001234"),
        (-8, 0, 30, vec![100], "0.000000000000000000000000000001"),
        (
            2,
            0,
            30,
            vec![
                12, 3456, 7890, 1234, 5678, 9012, 3456, 7890, 1234, 5678, 9000,
            ],
            "1234567890.123456789012345678901234567890",
        ),
    ] {
        assert_eq!(
            postgres::numeric(&numeric_wire(weight, sign, scale, &digits)?)?,
            expected
        );
    }
    // ndigits 是 uint16，不可误按有符号 i16 拒绝有效长数值。
    let value = postgres::numeric(&numeric_wire(i16::MAX, 0, 0, &vec![9999; 32768])?)?;
    assert_eq!(value.len(), 131_072);
    assert!(value.bytes().all(|byte| byte == b'9'));
    let zero = postgres::numeric(&numeric_wire(0, 0, 16383, &[])?)?;
    assert_eq!(zero.len(), 16385);
    assert!(zero.starts_with("0."));
    Ok(())
}

#[cfg(feature = "db-postgres")]
#[test]
fn postgres_numeric_special_values_remain_text_and_malformed_wire_never_truncates() -> Result<()> {
    for (sign, expected) in [(0xc000, "NaN"), (0xd000, "Infinity"), (0xf000, "-Infinity")] {
        assert_eq!(
            postgres::numeric(&numeric_wire(0, sign, 0, &[])?)?,
            expected
        );
    }
    for payload in [
        vec![],
        vec![0; 7],
        vec![0; 9],
        numeric_wire(0, 0x8000, 0, &[1])?,
        numeric_wire(0, 0, 0x4000, &[1])?,
        numeric_wire(0, 0, 0, &[10000])?,
        numeric_wire(-1, 0, 2, &[1234])?,
        numeric_wire(0, 0xc000, 0, &[1])?,
    ] {
        assert!(
            matches!(postgres::numeric(&payload), Err(error) if error.code() == "model_database_attribute_invalid")
        );
    }
    let mut truncated = numeric_wire(0, 0, 0, &[42])?;
    truncated.pop();
    assert!(postgres::numeric(&truncated).is_err());
    Ok(())
}

#[cfg(feature = "db-mysql")]
#[test]
fn mysql_unsigned_is_php_integer_or_exact_text_and_float_uses_mysqlnd_decimal_conversion()
-> Result<()> {
    assert_eq!(
        mysql::unsigned(9_007_199_254_740_993),
        json!(9_007_199_254_740_993i64)
    );
    assert_eq!(mysql::unsigned(u64::MAX), json!("18446744073709551615"));
    assert_eq!(mysql::mysql_float(1.1, 31)?, json!(1.1));
    assert_eq!(mysql::mysql_float(1.123_456_7, 31)?, json!(1.12346));
    assert_eq!(mysql::mysql_float(1.125, 2)?, json!(1.12));
    assert!(
        matches!(mysql::mysql_float(f32::INFINITY, 31), Err(error) if error.code() == "model_database_attribute_non_finite")
    );
    Ok(())
}

#[cfg(feature = "db-mysql")]
#[test]
fn mysql_binary_temporal_keeps_zero_dates_negative_long_time_and_fixed_fraction_width() -> Result<()>
{
    assert_eq!(mysql::temporal(&[0], 10, 0, true)?, "0000-00-00");
    assert_eq!(
        mysql::temporal(&[0], 12, 6, true)?,
        "0000-00-00 00:00:00.000000"
    );
    assert_eq!(
        mysql::temporal(&[4, 0xea, 0x07, 11, 6], 10, 0, true)?,
        "2026-11-06"
    );
    let mut timestamp = vec![11, 0xea, 0x07, 11, 6, 12, 34, 56];
    timestamp.extend_from_slice(&120_000u32.to_le_bytes());
    assert_eq!(
        mysql::temporal(&timestamp, 12, 6, true)?,
        "2026-11-06 12:34:56.120000"
    );
    let mut duration = vec![12, 1];
    duration.extend_from_slice(&34u32.to_le_bytes());
    duration.extend_from_slice(&[22, 59, 59]);
    duration.extend_from_slice(&120_000u32.to_le_bytes());
    assert_eq!(
        mysql::temporal(&duration, 11, 6, true)?,
        "-838:59:59.120000"
    );
    assert_eq!(
        mysql::temporal(b"2026-11-06 12:34:56.120000", 12, 6, false)?,
        "2026-11-06 12:34:56.120000"
    );
    assert!(mysql::temporal(&timestamp, 12, 1, true).is_err());
    assert!(mysql::temporal(&[0], 12, 7, true).is_err());
    assert!(mysql::temporal(&[7, 1], 12, 0, true).is_err());
    Ok(())
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
struct Migrator;
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
impl sea_orm_migration::MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn sea_orm_migration::MigrationTrait>> {
        Vec::new()
    }
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
#[tokio::test]
async fn real_raw_row_types_are_complete_and_pdo_categories_are_not_serde_casts() -> Result<()> {
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("跳过真实 raw 属性适配：未配置 OPENRUST_TEST_DATABASE_URL");
        return Ok(());
    };
    let database = crate::testing::TestDatabase::create::<Migrator>(&url).await?;
    let outcome = async {
        let connection = database.connection()?;
        match connection.get_database_backend() {
            #[cfg(feature = "db-mysql")]
            DbBackend::MySql => real_mysql(&connection).await,
            #[cfg(feature = "db-postgres")]
            DbBackend::Postgres => real_postgres(&connection).await,
            _ => Err(unsupported()),
        }
    }
    .await;
    let cleanup = database.close().await;
    outcome?;
    cleanup
}

#[cfg(feature = "db-mysql")]
async fn real_mysql(connection: &DatabaseConnection) -> Result<()> {
    connection.execute_unprepared("CREATE TABLE raw_attributes (tiny TINYINT(1), unsigned_big BIGINT UNSIGNED, amount DECIMAL(65,30), payload JSON, binary_value BLOB, happened_at DATETIME(6), duration TIME(6), zero_padded INT(6) UNSIGNED ZEROFILL, float_value FLOAT, nullable VARCHAR(20), binary_collation VARCHAR(20) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin)").await.map_err(Error::internal)?;
    connection.execute_unprepared("INSERT INTO raw_attributes VALUES (7,18446744073709551615,123456789012345678901234567890.123456789012345678901234567890,'{\"id\":18446744073709551615}',X'00ff80','2026-11-06 12:34:56.120000','-838:59:58.120000',42,1.1,NULL,'文本')").await.map_err(Error::internal)?;
    let attributes =
        attributes_from_row(&query(connection, "SELECT * FROM raw_attributes").await?)?;
    assert_eq!(attributes.len(), 11);
    assert_eq!(attributes["tiny"], json!(7));
    assert_eq!(attributes["unsigned_big"], json!("18446744073709551615"));
    assert_eq!(
        attributes["amount"],
        json!("123456789012345678901234567890.123456789012345678901234567890")
    );
    let payload = attributes["payload"].as_str().ok_or_else(invalid)?;
    let stored: String = query(
        connection,
        "SELECT CAST(payload AS CHAR) AS payload_text FROM raw_attributes",
    )
    .await?
    .try_get("", "payload_text")
    .map_err(Error::internal)?;
    assert_eq!(payload, stored);
    let decoded: Value = serde_json::from_str(payload).map_err(Error::internal)?;
    assert_eq!(decoded["id"].as_u64(), Some(u64::MAX));
    assert_eq!(attributes["binary_value"], json!([0, 255, 128]));
    assert_eq!(
        attributes["happened_at"],
        json!("2026-11-06 12:34:56.120000")
    );
    assert_eq!(attributes["duration"], json!("-838:59:58.120000"));
    assert_eq!(attributes["zero_padded"], json!("000042"));
    assert_eq!(attributes["float_value"], json!(1.1));
    assert_eq!(attributes["nullable"], Value::Null);
    assert_eq!(attributes["binary_collation"], json!("文本"));
    let duplicate =
        attributes_from_row(&query(connection, "SELECT 1 AS repeated, 'last' AS repeated").await?)?;
    assert_eq!(
        duplicate,
        Map::from_iter([("repeated".into(), json!("last"))])
    );
    Ok(())
}

#[cfg(feature = "db-postgres")]
async fn real_postgres(connection: &DatabaseConnection) -> Result<()> {
    let row = query(connection, "SELECT 9007199254740993::bigint AS big, 4294967295::oid AS object_id, true AS enabled, 1.125::double precision AS finite_value, 1.123456789012345678901234567891::numeric(40,30) AS amount, '{\"id\":123456789012345678901234567890}'::json AS payload, '{\"id\":123456789012345678901234567890}'::jsonb AS payload_binary, '550e8400-e29b-41d4-a716-446655440000'::uuid AS uuid_value, decode('00ff80','hex') AS binary_value, DATE '2026-11-06' AS date_value, TIME '24:00:00' AS time_value, TIMESTAMP '2026-11-06 12:34:56.12' AS timestamp_value, NULL::numeric AS nullable").await?;
    let attributes = attributes_from_row(&row)?;
    assert_eq!(attributes.len(), 13);
    assert_eq!(attributes["big"], json!(9_007_199_254_740_993i64));
    assert_eq!(attributes["object_id"], json!(4_294_967_295u32));
    assert_eq!(attributes["enabled"], json!(true));
    assert_eq!(attributes["finite_value"], json!(1.125));
    assert_eq!(
        attributes["amount"],
        json!("1.123456789012345678901234567891")
    );
    assert_eq!(
        attributes["payload"],
        json!("{\"id\":123456789012345678901234567890}")
    );
    assert!(
        attributes["payload_binary"]
            .as_str()
            .ok_or_else(invalid)?
            .contains("123456789012345678901234567890")
    );
    assert_eq!(
        attributes["uuid_value"],
        json!("550e8400-e29b-41d4-a716-446655440000")
    );
    assert_eq!(attributes["binary_value"], json!([0, 255, 128]));
    assert_eq!(attributes["date_value"], json!("2026-11-06"));
    assert_eq!(attributes["time_value"], json!("24:00:00"));
    assert_eq!(
        attributes["timestamp_value"],
        json!("2026-11-06 12:34:56.12")
    );
    assert_eq!(attributes["nullable"], Value::Null);
    let timestamp = attributes_from_row(&query(connection, "SELECT TIMESTAMPTZ '2026-11-06 12:34:56.12+08' AS utc_value, DATE '0001-01-01 BC' AS bc_value, 123::int2 AS small_value, 2147483647::int4 AS integer_value, 1.125::real AS real_value").await?)?;
    assert_eq!(timestamp["utc_value"], json!("2026-11-06 04:34:56.12+00"));
    assert_eq!(timestamp["bc_value"], json!("0001-01-01 BC"));
    assert_eq!(timestamp["small_value"], json!(123));
    assert_eq!(timestamp["integer_value"], json!(2_147_483_647));
    assert_eq!(timestamp["real_value"], json!(1.125));
    for literal in ["'Infinity'", "'-Infinity'", "'NaN'"] {
        let row = query(
            connection,
            &format!("SELECT {literal}::double precision AS bad"),
        )
        .await?;
        assert!(
            matches!(attributes_from_row(&row),Err(error) if error.code()=="model_database_attribute_non_finite")
        );
        let row = query(connection, &format!("SELECT {literal}::numeric AS special")).await?;
        assert!(attributes_from_row(&row)?["special"].is_string());
    }
    let unsupported_row = query(connection, "SELECT INTERVAL '1 day' AS unsupported").await?;
    assert!(
        matches!(attributes_from_row(&unsupported_row),Err(error) if error.code()=="model_database_attribute_unsupported")
    );
    let duplicate =
        attributes_from_row(&query(connection, "SELECT 1 AS repeated, 'last' AS repeated").await?)?;
    assert_eq!(
        duplicate,
        Map::from_iter([("repeated".into(), json!("last"))])
    );
    Ok(())
}
