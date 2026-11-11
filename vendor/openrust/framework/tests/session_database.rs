use std::{sync::Arc, time::Duration};

use openrust::{
    config::Repository,
    contracts::session::SessionHandler,
    database::{Connection, DatabaseManager, Value, query::Record},
    session::{DatabaseSessionHandler, DatabaseSessionMetadata, SessionError},
};
use serde_json::{Value as Json, json};

fn connection(config: Json) -> Connection {
    let settings = Repository::new(
        [(
            "database".into(),
            json!({"default":"test","connections":{"test":config}}),
        )]
        .into(),
    );
    DatabaseManager::new(Arc::new(settings), ".")
        .connection(None)
        .unwrap()
}

async fn verify(db: Connection) {
    db.statement("create table sessions (id varchar(255) primary key, user_id bigint null, ip_address varchar(45) null, user_agent text null, payload longtext not null, last_activity integer not null)", &[]).await.unwrap();

    let mut first = DatabaseSessionHandler::new(db.clone(), "sessions", 120).unwrap();
    let mut second = DatabaseSessionHandler::new(db.clone(), "sessions", 120).unwrap();
    assert!(first.read("unknown").await.unwrap().is_none());
    first.setMetadata(DatabaseSessionMetadata {
        user_id: Some(7_i64.into()),
        ip_address: Some(Some("127.0.0.1".to_owned())),
        user_agent: Some("é".repeat(510)),
    });
    first.write("known", b"\x00hello\xff").await.unwrap();
    assert_eq!(
        second.read("known").await.unwrap().as_deref(),
        Some(b"\x00hello\xff".as_slice())
    );
    second.write("known", b"next").await.unwrap();
    let row = db
        .table("sessions")
        .r#where("id", "=", "known")
        .first()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.get("payload"), Some(&"bmV4dA==".into()));
    assert_eq!(row.get("user_id"), Some(&7_i64.into()));
    assert_eq!(row.get("ip_address"), Some(&"127.0.0.1".into()));
    assert_eq!(row.get("user_agent"), Some(&Value::Text("é".repeat(500))));

    let (left, right) = tokio::join!(
        first.write("race", b"first"),
        second.write("race", b"second")
    );
    left.unwrap();
    right.unwrap();
    let race = first.read("race").await.unwrap().unwrap();
    assert!(race == b"first" || race == b"second");
    assert!(first.validateId("race").await.unwrap());
    first.destroy("race").await.unwrap();
    assert!(!second.validateId("race").await.unwrap());

    db.table("sessions")
        .r#where("id", "=", "known")
        .update(Record::from([("last_activity".into(), 1_i64.into())]))
        .await
        .unwrap();
    assert!(first.validateId("known").await.unwrap());
    assert!(first.read("known").await.unwrap().is_none());
    assert_eq!(second.gc(Duration::from_secs(60)).await.unwrap(), Some(1));
    assert!(!first.validateId("known").await.unwrap());
    assert!(matches!(
        first.write("too-large", &vec![0; 1024 * 1024 + 1]).await,
        Err(SessionError::PayloadTooLarge)
    ));
    assert!(!second.validateId("too-large").await.unwrap());

    db.statement("drop table sessions", &[]).await.unwrap();
}

#[tokio::test]
async fn database_session_storage_round_trips_in_sqlite() {
    verify(connection(json!({"driver":"sqlite","database":":memory:"}))).await;
}

#[tokio::test]
async fn database_session_storage_round_trips_in_mysql() {
    let url = std::env::var("OPENRUST_TEST_SESSION_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_SESSION_MYSQL").is_none());
    if let Some(url) = url {
        verify(connection(json!({"url":url,"use_upsert_alias":true}))).await;
    }
}
