use std::sync::Arc;

use serde_json::json;
use sqlx::{
    Connection as _,
    postgres::{PgConnectOptions, PgConnection, PgSslMode},
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpListener,
};

use super::{DatabaseCreation, DatabaseError, MissingDatabase, Pool, create_sqlite_database};
use crate::{config::Repository, database::DatabaseManager};

#[tokio::test]
async fn concurrent_sqlite_creation_preserves_the_winning_file() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("database.sqlite");
    let (first, second) = tokio::join!(
        create_sqlite_database(path.clone()),
        create_sqlite_database(path.clone()),
    );
    first.unwrap();
    second.unwrap();
    std::fs::write(&path, b"existing database bytes").unwrap();
    create_sqlite_database(path.clone()).await.unwrap();
    assert_eq!(std::fs::read(path).unwrap(), b"existing database bytes");
}

async fn postgres_startup_error(code: &str) -> DatabaseError {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let response = format!("SFATAL\0C{code}\0Mstartup rejected\0\0");
    let startup = async {
        let (mut socket, _) = listener.accept().await.unwrap();
        let length = socket.read_u32().await.unwrap();
        let mut startup = vec![0; (length - 4) as usize];
        socket.read_exact(&mut startup).await.unwrap();
        socket.write_u8(b'E').await.unwrap();
        socket.write_u32((response.len() + 4) as u32).await.unwrap();
        socket.write_all(response.as_bytes()).await.unwrap();
    };
    let options = PgConnectOptions::new()
        .host("127.0.0.1")
        .port(port)
        .database("app")
        .ssl_mode(PgSslMode::Disable);
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::join!(PgConnection::connect_with(&options), startup)
    })
    .await
    .unwrap();
    let Err(error) = result else {
        panic!("PostgreSQL startup unexpectedly succeeded");
    };
    DatabaseError::driver("connect")(error)
}

#[tokio::test]
async fn postgres_missing_database_selects_the_configured_direct_writer_only() {
    let missing = postgres_startup_error("3D000").await;
    let Some(missing @ MissingDatabase::Postgres) = MissingDatabase::from_error(&missing) else {
        panic!("PostgreSQL invalid catalog was not recognized");
    };
    for code in ["28P01", "42501", "08006"] {
        assert!(MissingDatabase::from_error(&postgres_startup_error(code).await).is_none());
    }

    let manager = DatabaseManager::new(
        Arc::new(Repository::new(
            [(
                "database".into(),
                json!({"default":"pgsql","connections":{"pgsql":{
                    "driver":"pgsql", "database":"app\"quoted", "host":"runtime",
                    "username":"migrator", "sslmode":"require",
                    "read":{"host":"reader"},
                    "write":{"database":"override"},
                    "direct":{"host":"direct", "port":5433}
                }}}),
            )]
            .into(),
        )),
        ".",
    );
    let direct = manager.migration_connection(None).unwrap();
    let Some(DatabaseCreation::Server { generation, sql }) = missing.prepare(&direct).unwrap()
    else {
        panic!("expected a server creation request");
    };
    assert_eq!(sql, "create database \"app\"\"quoted\"");
    let Pool::Postgres(pool) = &generation.writer else {
        panic!("expected the PostgreSQL writer");
    };
    let selected = pool.connect_options();
    assert_eq!(selected.get_host(), "direct");
    assert_eq!(selected.get_port(), 5433);
    assert_eq!(selected.get_username(), "migrator");
    assert!(matches!(selected.get_ssl_mode(), PgSslMode::Require));
    assert_eq!(selected.get_database(), Some("app\"quoted"));
    for name in ["pgsql::read", "pgsql::write"] {
        assert!(
            missing
                .prepare(&manager.connection(Some(name)).unwrap())
                .unwrap()
                .is_none()
        );
    }
}
