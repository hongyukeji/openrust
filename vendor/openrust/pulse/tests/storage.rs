use openrust::database::{
    Connection, DatabaseManager,
    migrations::{MigrationRegistry, Migrator, RunOptions},
};
use openrust_pulse::{
    CreatePulseTables, Entry, Item, Value as PulseValue, storage::DatabaseStorage,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

fn fixture_config(
    root: &std::path::Path,
    _: &openrust::config::Environment,
) -> Result<BTreeMap<String, Value>, openrust::config::ConfigError> {
    let driver = std::env::var("PULSE_TEST_DRIVER").unwrap_or("sqlite".into());
    let db = match driver.as_str() {
        "sqlite" => {
            json!({"driver":"sqlite","database":root.join("test.sqlite"),"busy_timeout":10000,"max_connections":4})
        }
        "pgsql" => {
            assert_eq!(
                std::env::var("OPENRUST_REQUIRE_POSTGRES").as_deref(),
                Ok("1")
            );
            assert!(std::env::var("OPENRUST_ACCEPTANCE_POSTGRES_CONTAINER").is_ok());
            json!({"driver":"pgsql","url":std::env::var("OPENRUST_TEST_POSTGRES_URL").unwrap(),"max_connections":4})
        }
        "mysql" => {
            assert_eq!(std::env::var("OPENRUST_REQUIRE_MYSQL").as_deref(), Ok("1"));
            assert!(std::env::var("OPENRUST_ACCEPTANCE_MYSQL_CONTAINER").is_ok());
            json!({"driver":"mysql","url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap(),"max_connections":4,"use_upsert_alias":std::env::var("PULSE_TEST_UPSERT_ALIAS").as_deref()==Ok("1")})
        }
        _ => panic!("unsupported isolated driver"),
    };
    Ok(BTreeMap::from([(
        "database".into(),
        json!({"default":"testing","connections":{"testing":db}}),
    )]))
}

struct Fixture {
    _app: openrust::foundation::Application,
    connection: Connection,
    migrator: Migrator,
    _root: tempfile::TempDir,
}
impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("test.sqlite"), []).unwrap();
        let mut app = openrust::foundation::Application::configure(root.path().into())
            .with_config(fixture_config)
            .create()
            .unwrap();
        app.boot().await.unwrap();
        let db = app.make::<DatabaseManager>().await.unwrap();
        let connection = db.connection(None).unwrap();
        static ENTRIES: &[openrust::database::migrations::MigrationEntry] =
            &[openrust::database::migrations::MigrationEntry::new(
                "pulse_test",
                &CreatePulseTables,
            )];
        let migrator = Migrator::new(
            db,
            Arc::new(MigrationRegistry(ENTRIES)),
            "pulse_test_migrations".into(),
            root.path().into(),
        );
        migrator.install(None).await.unwrap();
        assert_eq!(
            migrator
                .run(None, &[], RunOptions::default())
                .await
                .unwrap()
                .len(),
            1
        );
        Self {
            _app: app,
            connection,
            migrator,
            _root: root,
        }
    }
    async fn close(self) {
        assert_eq!(
            self.migrator.reset(None, &[], false).await.unwrap().len(),
            1
        );
        assert!(
            !self
                .connection
                .get_schema_builder()
                .has_table("pulse_entries")
                .await
                .unwrap()
        );
        self.connection
            .get_schema_builder()
            .drop_if_exists("pulse_test_migrations")
            .await
            .unwrap();
        self.connection.disconnect();
    }
}

fn numeric(row: &openrust::database::Row, column: &str) -> f64 {
    match row.get(column).unwrap() {
        openrust::database::Value::Integer(v) => *v as f64,
        openrust::database::Value::Unsigned(v) => *v as f64,
        openrust::database::Value::Real(v) => *v,
        openrust::database::Value::Text(v) => v.parse().unwrap(),
        v => panic!("unexpected {v:?}"),
    }
}
fn metric(timestamp: i64, key: &str, value: i64) -> Item {
    Entry::new(timestamp, "type", key, Some(value))
        .count()
        .min()
        .max()
        .sum()
        .avg()
        .into()
}
#[tokio::test]
async fn database_storage_matches_upstream_aggregation_values_and_window_queries() {
    let fixture = Fixture::new().await;
    if fixture.connection.get_driver_name() == "pgsql" {
        use openrust::database::Value as Sql;
        let values = [
            Sql::Text("123456789012.12345678".into()),
            Sql::Real(1.25),
            Sql::Integer(12),
            Sql::Unsigned(13),
            Sql::Null,
        ];
        for (value, expected) in values.into_iter().zip([
            Some("123456789012.12345678"),
            Some("1.25"),
            Some("12"),
            Some("13"),
            None,
        ]) {
            let row = fixture
                .connection
                .select_one("select cast(? as numeric) as value", &[value][..])
                .await
                .unwrap()
                .unwrap();
            match (row.get("value"), expected) {
                (Some(Sql::Text(actual)), Some(expected)) => assert_eq!(
                    if actual.contains('.') {
                        actual.trim_end_matches('0').trim_end_matches('.')
                    } else {
                        actual
                    },
                    expected
                ),
                (Some(Sql::Null), None) => {}
                other => panic!("unexpected numeric value: {other:?}"),
            }
        }
        assert!(
            fixture
                .connection
                .select_one(
                    "select cast(? as numeric)",
                    &[Sql::Text("invalid".into())][..]
                )
                .await
                .is_err()
        );
    }
    let now = 1_795_267_800i64;
    let storage = DatabaseStorage::new(fixture.connection.clone())
        .withClock(move || now)
        .withChunk(2)
        .unwrap();
    storage
        .store(&[
            metric(now, "key1", 200),
            metric(now, "key1", 100),
            metric(now, "key2", 400),
        ])
        .await
        .unwrap_or_else(|e| {
            let mut message = e.to_string();
            let mut source = std::error::Error::source(&e);
            while let Some(e) = source {
                message.push_str(&format!("; {e}"));
                source = e.source();
            }
            panic!("{message}")
        });
    let rows = fixture
        .connection
        .table("pulse_aggregates")
        .order_by("period", "asc")
        .order_by("aggregate", "asc")
        .order_by("key", "asc")
        .get()
        .await
        .unwrap();
    assert_eq!(rows.len(), 40);
    assert_eq!(numeric(&rows[0], "value"), 150.0);
    storage.store(&[metric(now, "key1", 600)]).await.unwrap();
    let rows = fixture
        .connection
        .table("pulse_aggregates")
        .order_by("period", "asc")
        .order_by("aggregate", "asc")
        .order_by("key", "asc")
        .get()
        .await
        .unwrap();
    assert_eq!(numeric(&rows[0], "value"), 300.0);
    assert_eq!(numeric(&rows[0], "count"), 3.0);
    let aggregates = ["count", "min", "max", "sum", "avg"].map(str::to_owned);
    let rows = storage
        .aggregate("type", &aggregates, 3600, Some("count"), "desc", 101)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(numeric(&rows[0], "count"), 3.0);
    assert_eq!(numeric(&rows[0], "min"), 100.0);
    assert_eq!(numeric(&rows[0], "max"), 600.0);
    assert_eq!(numeric(&rows[0], "sum"), 900.0);
    assert_eq!(numeric(&rows[0], "avg"), 300.0);
    assert_eq!(
        storage
            .aggregateTotal(&["type".into()], "count", 3600)
            .await
            .unwrap()["type"],
        Some(4.0)
    );
    let graph = storage
        .graph(&["type".into(), "other".into()], "count", 3600)
        .await
        .unwrap();
    assert_eq!(graph["key1"]["type"].len(), 60);
    assert!(graph["key1"]["type"].values().any(|v| *v == Some(3.0)));
    assert!(graph["key1"]["other"].values().all(Option::is_none));
    assert!(storage.graph(&[], "invalid", 3600).await.is_err());
    storage
        .store(&[
            PulseValue::new(now, "server", "one", "old").into(),
            PulseValue::new(now - 1, "server", "one", "new").into(),
        ])
        .await
        .unwrap();
    let values = storage.values("server", Some(&[])).await.unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(
        values[0].get("value"),
        Some(&openrust::database::Value::Text("new".into()))
    );
    storage.purge(Some(&["type".into()])).await.unwrap();
    assert_eq!(storage.values("server", None).await.unwrap().len(), 1);
    assert!(
        storage
            .aggregate("type", &aggregates, 3600, None, "desc", 101)
            .await
            .unwrap()
            .is_empty()
    );
    storage.purge(None).await.unwrap();
    assert!(storage.values("server", None).await.unwrap().is_empty());
    check_windows_atomicity_and_retention(&fixture.connection, now + 17).await;
    check_php_preaggregation(&fixture.connection, now).await;
    fixture.close().await;
}
#[test]
fn entry_flags_and_stream_serialization_preserve_duplicates_and_shared_builder_state() {
    let e = Entry::new(10, "t", "k", None);
    let buffered = e.clone();
    let e = e.count().count().avg().onlyBuckets();
    assert!(buffered.isCount());
    assert!(buffered.isOnlyBuckets());
    assert!(e.isAvg());
    assert_eq!(e.aggregations(), ["count", "count", "avg"]);
    let wire = serde_json::to_string(&Item::from(e)).unwrap();
    let Item::Entry(decoded) = serde_json::from_str::<Item>(&wire).unwrap() else {
        panic!()
    };
    assert!(decoded.isOnlyBuckets());
    assert_eq!(decoded.aggregations().len(), 3);
    assert!(serde_json::from_str::<Item>(&wire.replace("avg", "invalid")).is_err());
}

async fn check_windows_atomicity_and_retention(connection: &Connection, now: i64) {
    let storage = DatabaseStorage::new(connection.clone())
        .withClock(move || now)
        .withChunk(2)
        .unwrap();
    for interval in [3600, 21600, 86400, 604800] {
        let period = interval / 60;
        let oldest = now / period * period - interval + period;
        let window = now - interval + 1;
        let key = format!("window-{interval}");
        storage
            .store(&[
                metric(window - 1, &key, 999),
                metric(window, &key, 10),
                metric(oldest - 1, &key, 20),
                metric(oldest, &key, 30),
                metric(oldest + 1, &key, 50),
                metric(now, &key, 40),
            ])
            .await
            .unwrap();
        let rows = storage
            .aggregate(
                "type",
                &["count", "min", "max", "sum", "avg"].map(str::to_owned),
                interval,
                Some("count"),
                "desc",
                100,
            )
            .await
            .unwrap();
        let row = rows
            .iter()
            .find(|row| row.get("key") == Some(&openrust::database::Value::Text(key.clone())))
            .unwrap();
        assert_eq!(numeric(row, "count"), 5.0);
        assert_eq!(numeric(row, "min"), 10.0);
        assert_eq!(numeric(row, "max"), 50.0);
        assert_eq!(numeric(row, "sum"), 150.0);
        assert_eq!(
            numeric(row, "avg"),
            27.5,
            "upstream averages the two query groups"
        );
        storage.purge(None).await.unwrap();
    }
    // Two simultaneous transactions must accumulate rather than overwrite each other's counts.
    let first = [metric(now, "concurrent", 10)];
    let second = [metric(now, "concurrent", 20)];
    let (a, b) = tokio::join!(storage.store(&first), storage.store(&second));
    a.unwrap();
    b.unwrap();
    assert_eq!(
        storage
            .aggregateTotal(&["type".into()], "count", 3600)
            .await
            .unwrap()["type"],
        Some(2.0)
    );
    storage.purge(None).await.unwrap();
    let reject = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let failure = reject.clone();
    connection.before_executing(move |sql, _, _, _| {
        let fail = sql.to_ascii_lowercase().contains("insert into")
            && sql.contains("pulse_aggregates")
            && failure.load(std::sync::atomic::Ordering::SeqCst);
        Box::pin(async move {
            if fail {
                Err(openrust::database::DatabaseError::InvalidQuery(
                    "injected aggregate failure",
                ))
            } else {
                Ok(())
            }
        })
    });
    assert!(storage.store(&[metric(now, "rollback", 1)]).await.is_err());
    assert_eq!(
        connection.table("pulse_entries").count("*").await.unwrap(),
        0
    );
    reject.store(false, std::sync::atomic::Ordering::SeqCst);
    storage
        .store(&[Entry::new(now, "queue", "one", None)
            .count()
            .onlyBuckets()
            .into()])
        .await
        .unwrap();
    assert_eq!(
        connection.table("pulse_entries").count("*").await.unwrap(),
        0
    );
    assert_eq!(
        storage
            .aggregateTotal(&["queue".into()], "count", 3600)
            .await
            .unwrap()["queue"],
        Some(1.0)
    );
    storage.purge(None).await.unwrap();
    storage
        .store(&[
            metric(now - 604801, "old", 1),
            metric(now, "new", 2),
            PulseValue::new(now - 604801, "server", "old", "x").into(),
        ])
        .await
        .unwrap();
    storage.trim().await.unwrap();
    assert!(storage.values("server", None).await.unwrap().is_empty());
    let rows = connection.table("pulse_entries").get().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].get("key"),
        Some(&openrust::database::Value::Text("new".into()))
    );
    storage.purge(None).await.unwrap();
}

async fn check_php_preaggregation(connection: &Connection, now: i64) {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/preaggregation.json")).unwrap();
    let cases: Vec<_> = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| {
            case["values"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| !v.is_null())
        })
        .collect();
    let items: Vec<Item> = cases
        .iter()
        .flat_map(|case| {
            case["values"].as_array().unwrap().iter().map(move |value| {
                Entry::new(
                    now,
                    "reference",
                    case["key"].as_str().unwrap(),
                    value.as_i64(),
                )
                .count()
                .min()
                .max()
                .sum()
                .avg()
                .into()
            })
        })
        .collect();
    let storage = DatabaseStorage::new(connection.clone()).withClock(move || now);
    assert!(
        storage
            .store(&[Entry::new(now, "reference", "null", None)
                .min()
                .avg()
                .into()])
            .await
            .is_err()
    );
    assert_eq!(
        connection.table("pulse_entries").count("*").await.unwrap(),
        0
    );
    storage.store(&items).await.unwrap();
    let rows = connection.table("pulse_aggregates").get().await.unwrap();
    assert_eq!(rows.len(), cases.len() * 20);
    for case in &cases {
        for expected in case["expected"].as_array().unwrap() {
            let row = rows
                .iter()
                .find(|row| {
                    row.get("key")
                        == Some(&openrust::database::Value::Text(
                            case["key"].as_str().unwrap().into(),
                        ))
                        && row.get("aggregate")
                            == Some(&openrust::database::Value::Text(
                                expected["aggregate"].as_str().unwrap().into(),
                            ))
                        && numeric(row, "period") == expected["period"].as_f64().unwrap()
                })
                .unwrap();
            if expected["value"].is_null() {
                assert_eq!(row.get("value"), Some(&openrust::database::Value::Null));
            } else {
                let expected_value = expected["value"].as_f64().unwrap();
                let actual = numeric(row, "value");
                assert!(
                    (actual - expected_value).abs() < 0.011,
                    "PHP case {} {}: expected {expected_value}, actual {actual}",
                    case["values"],
                    expected["aggregate"]
                );
            }
            if expected["aggregate"] == "avg" {
                assert_eq!(numeric(row, "count"), 3.0);
            }
        }
    }
    storage.purge(None).await.unwrap();
}
