mod support;
use axum::{Router, body::Bytes, extract::State, response::IntoResponse, routing::post};
use openrust::{
    config::Repository,
    database::{
        DatabaseError, DatabaseManager,
        migrations::{MigrationEntry, MigrationRegistry, Migrator, RunOptions},
    },
    http::client::Factory,
};
use openrust_pay::{CreatePaymentNotificationsTable, DatabaseNotifications, Gateway};
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use support::*;
#[derive(Clone)]
struct Endpoint {
    gateway: Gateway,
    store: DatabaseNotifications,
    fail: Arc<AtomicBool>,
}
async fn callback(
    State(s): State<Endpoint>,
    headers: http::HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    let Ok(notice) = s.gateway.callback(&headers, &body) else {
        return http::StatusCode::BAD_REQUEST.into_response();
    };
    let fail = s.fail.clone();
    match s
        .store
        .process(&notice, &expected(), move |tx, n| {
            let fail = fail.clone();
            Box::pin(async move {
                tx.table("callback_business")
                    .insert(&[[("id".into(), n.out_trade_no().into())]
                        .into_iter()
                        .collect()])
                    .await?;
                if fail.swap(false, Ordering::SeqCst) {
                    return Err(DatabaseError::InvalidQuery("simulated business failure"));
                }
                Ok(())
            })
        })
        .await
    {
        Ok(_) => notice.success().into_response(),
        Err(_) => http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn signed_http_callbacks_acknowledge_only_committed_business_and_recover_delivery() {
    let path = std::env::temp_dir().join(format!("openrust-pay-db-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    let sqlite = path.join("payments.sqlite");
    std::fs::File::create(&sqlite).unwrap();
    let driver = std::env::var("PAY_TEST_DRIVER").unwrap_or("sqlite".into());
    let connection = match driver.as_str() {
        "sqlite" => {
            json!({"driver":"sqlite","database":sqlite,"busy_timeout":10000,"max_connections":8})
        }
        "pgsql" => {
            assert_eq!(
                std::env::var("OPENRUST_REQUIRE_POSTGRES").as_deref(),
                Ok("1")
            );
            assert!(std::env::var("OPENRUST_ACCEPTANCE_POSTGRES_CONTAINER").is_ok());
            json!({"driver":"pgsql","url":std::env::var("OPENRUST_TEST_POSTGRES_URL").unwrap(),"max_connections":8})
        }
        "mysql" => {
            assert_eq!(std::env::var("OPENRUST_REQUIRE_MYSQL").as_deref(), Ok("1"));
            assert!(std::env::var("OPENRUST_ACCEPTANCE_MYSQL_CONTAINER").is_ok());
            json!({"driver":"mysql","url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap(),"max_connections":8})
        }
        _ => panic!("unsupported database"),
    };
    let config = Arc::new(Repository::new(BTreeMap::from([(
        "database".into(),
        json!({"default":"test","connections":{"test":connection}}),
    )])));
    let db = Arc::new(DatabaseManager::new(config, &path));
    let connection = db.connection(None).unwrap();
    let version_sql = match driver.as_str() {
        "sqlite" => "select sqlite_version()",
        "pgsql" => "show server_version_num",
        _ => "select version()",
    };
    println!(
        "database runtime {:?}",
        connection.scalar(version_sql, &[]).await.unwrap()
    );
    static ENTRIES: [MigrationEntry; 1] = [MigrationEntry::new(
        "2026_11_21_000001_create_payment_notifications_table.rs",
        &CreatePaymentNotificationsTable,
    )];
    let migrator = Migrator::new(
        db.clone(),
        Arc::new(MigrationRegistry(&ENTRIES)),
        "pay_migrations".into(),
        path.clone(),
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

    connection
        .get_schema_builder()
        .create("callback_business", |t| {
            t.string("id", 32).primary();
        })
        .await
        .unwrap();
    for provider in ["wechat", "alipay"] {
        let (_, manager) = manager(provider, "https://example.test/");
        let (headers, body) = if provider == "wechat" {
            wechat_callback(json!({}))
        } else {
            alipay_callback(json!({}))
        };
        let state = Endpoint {
            gateway: manager.driver("main").unwrap(),
            store: DatabaseNotifications::new(connection.clone()),
            fail: Arc::new(AtomicBool::new(true)),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/notify", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/notify", post(callback))
            .with_state(state);
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let http = Factory::new().unwrap();
        let send = |bytes: Vec<u8>| {
            http.pending_request()
                .with_headers(headers.clone())
                .with_body(
                    bytes,
                    headers
                        .get("content-type")
                        .cloned()
                        .unwrap_or_else(|| http::HeaderValue::from_static("application/json")),
                )
                .send(http::Method::POST, &url)
        };
        let mut corrupt = body.clone();
        corrupt.push(b' ');
        assert_eq!(send(corrupt).await.unwrap().status(), 400);
        assert_eq!(send(body.clone()).await.unwrap().status(), 500);
        assert_eq!(
            connection
                .table("callback_business")
                .count("*")
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .table("payment_notifications")
                .count("*")
                .await
                .unwrap(),
            0
        );
        let status = if provider == "wechat" { 204 } else { 200 };
        let response = send(body.clone()).await.unwrap();
        assert_eq!(response.status(), status);
        assert_eq!(
            response.body().unwrap(),
            if provider == "wechat" {
                b"".as_slice()
            } else {
                b"success".as_slice()
            }
        );
        assert_eq!(send(body).await.unwrap().status(), status);
        assert_eq!(
            connection
                .table("callback_business")
                .count("*")
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            connection
                .table("payment_notifications")
                .count("*")
                .await
                .unwrap(),
            1
        );
        println!(
            "{provider}: TCP raw callback tamper400, transaction failure500, retry/duplicate native ACK only after commit PASS"
        );
        server.abort();
        let _ = server.await;
        connection
            .table("callback_business")
            .delete()
            .await
            .unwrap();
        connection
            .table("payment_notifications")
            .delete()
            .await
            .unwrap();
    }
    for table in [
        "callback_business",
        "payment_notifications",
        "pay_migrations",
    ] {
        connection
            .get_schema_builder()
            .drop_if_exists(table)
            .await
            .unwrap();
    }
    db.disconnect(None).unwrap();
    std::fs::remove_dir_all(path).unwrap();
}
