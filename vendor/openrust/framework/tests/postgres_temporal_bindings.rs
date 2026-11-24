use openrust::{
    contracts::{config::Repository, container::Container as _, foundation::Application as _},
    database::{DatabaseManager, Value},
    foundation::Application,
};
use serde_json::json;

#[tokio::test]
#[ignore = "必须通过独占PostgreSQL 18夹具提供连接"]
async fn timestamp_parameters_preserve_civil_time_and_reject_untyped_integers() {
    let url = std::env::var("OPENRUST_TEST_POSTGRES_URL").expect("必须提供独占PostgreSQL连接");
    let mut app = Application::configure(std::env::temp_dir())
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let config = app.container().get::<dyn Repository>("config").unwrap();
    config
        .set(
            "database",
            json!({"default":"temporal","connections":{"temporal":{"driver":"pgsql","url":url}}}),
        )
        .unwrap();
    let db = app
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(None)
        .unwrap();
    db.statement(
        "CREATE TABLE auth_temporal_bindings (id INTEGER PRIMARY KEY, value TIMESTAMP(6) NULL)",
        &[],
    )
    .await
    .unwrap();
    for (id, input, expected) in [
        (
            1,
            "2026-01-02 12:34:56.123456",
            "2026-01-02T12:34:56.123456",
        ),
        (2, "2026-01-02T12:34:56Z", "2026-01-02T12:34:56.000000"),
        (
            3,
            "2026-01-02T12:34:56.123456+08:00",
            "2026-01-02T12:34:56.123456",
        ),
    ] {
        db.statement(
            "INSERT INTO auth_temporal_bindings (id,value) VALUES (?,?)",
            &[Value::Integer(id), Value::Text(input.into())],
        )
        .await
        .unwrap();
        let row=db.select_one("SELECT to_char(value,'YYYY-MM-DD\"T\"HH24:MI:SS.US') AS value FROM auth_temporal_bindings WHERE id=?",&[Value::Integer(id)]).await.unwrap().unwrap();
        assert_eq!(row.get("value"), Some(&Value::Text(expected.into())));
    }
    for value in [Value::Text("invalid-date".into()), Value::Integer(1)] {
        assert!(
            db.statement(
                "INSERT INTO auth_temporal_bindings (id,value) VALUES (4,?)",
                &[value]
            )
            .await
            .is_err()
        );
    }
    db.statement(
        "INSERT INTO auth_temporal_bindings (id,value) VALUES (4,?)",
        &[Value::Null],
    )
    .await
    .unwrap();
    let row = db
        .select_one("SELECT value FROM auth_temporal_bindings WHERE id=4", &[])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.get("value"), Some(&Value::Null));
}
