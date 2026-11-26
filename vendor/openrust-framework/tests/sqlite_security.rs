use openrust::{
    config::Repository,
    database::{Connection, DatabaseManager, Value},
};
use serde_json::{Value as Json, json};
use std::{collections::BTreeMap, sync::Arc};

fn connection(config: Json) -> Connection {
    let settings = Repository::new(BTreeMap::from([(
        "database".into(),
        json!({"default":"test","connections":{"test":config}}),
    )]));
    DatabaseManager::new(Arc::new(settings), ".")
        .connection(None)
        .unwrap()
}

#[tokio::test]
async fn sqlite_security_runtime_and_wal_round_trip_use_patched_engine() {
    let directory = tempfile::tempdir().unwrap();
    let settings = json!({"driver":"sqlite","database":directory.path().join("security.sqlite")});
    std::fs::File::create(directory.path().join("security.sqlite")).unwrap();
    let writer = connection(settings.clone());
    let Value::Text(version) = writer.scalar("select sqlite_version()", &[]).await.unwrap() else {
        panic!("SQLite must report its runtime version");
    };
    let parts = version
        .split('.')
        .map(|part| part.parse::<u32>().unwrap())
        .collect::<Vec<_>>();
    assert!(
        parts.as_slice() >= [3, 54, 1].as_slice(),
        "unpatched bundled SQLite: {version}"
    );
    assert_eq!(
        writer.scalar("pragma journal_mode=wal", &[]).await.unwrap(),
        Value::from("wal")
    );
    writer
        .statement(
            "create table wal_records (id integer primary key, payload blob not null)",
            &[],
        )
        .await
        .unwrap();
    let bytes = Value::Bytes(b"bound WAL payload\0\xff".to_vec());
    writer
        .statement(
            "insert into wal_records values (?, ?)",
            &[1.into(), bytes.clone()],
        )
        .await
        .unwrap();
    let reader = connection(settings);
    assert_eq!(
        reader
            .scalar("select payload from wal_records where id = ?", &[1.into()])
            .await
            .unwrap(),
        bytes
    );
    assert_eq!(
        reader.scalar("pragma integrity_check", &[]).await.unwrap(),
        Value::from("ok")
    );
    writer.disconnect();
    reader.disconnect();
}
