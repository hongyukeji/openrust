mod support;
use openrust::{
    config::Repository,
    database::{
        DatabaseError, DatabaseManager, Value,
        migrations::{MigrationEntry, MigrationRegistry, Migrator, RunOptions},
    },
};
use openrust_pay::{
    Amount, CreatePaymentNotificationsTable, DatabaseNotifications, Error, NotificationOutcome,
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use support::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn durable_notifications_share_business_transaction_rollback_deduplicate_and_recover_cancellation()
 {
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
        .create("payment_orders", |t| {
            t.string("id", 32).primary();
            t.string("status", 16);
        })
        .await
        .unwrap();
    connection
        .table("payment_orders")
        .insert(&[[
            ("id".into(), "order_001".into()),
            ("status".into(), "pending".into()),
        ]
        .into_iter()
        .collect()])
        .await
        .unwrap();
    let repository = DatabaseNotifications::new(connection.clone());
    let (_, m) = manager("wechat", "https://example.test/");
    let gateway = m.driver("main").unwrap();
    let (h, b) = wechat_callback(json!({}));
    let notification = gateway.callback(&h, &b).unwrap();
    let error = repository
        .process(&notification, &expected(), |tx, n| {
            Box::pin(async move {
                tx.table("payment_orders")
                    .r#where("id", "=", n.out_trade_no())
                    .update([("status".into(), "paid".into())].into_iter().collect())
                    .await?;
                Err(DatabaseError::InvalidConfiguration(
                    "simulated business failure",
                ))
            })
        })
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Database));
    assert_eq!(
        connection
            .table("payment_notifications")
            .count("*")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .table("payment_orders")
            .value("status")
            .await
            .unwrap(),
        Value::from("pending")
    );
    let started = Arc::new(tokio::sync::Notify::new());
    let signal = started.clone();
    let repo = repository.clone();
    let n = notification.clone();
    let task = tokio::spawn(async move {
        repo.process(&n, &expected(), move |tx, n| {
            let signal = signal.clone();
            Box::pin(async move {
                tx.table("payment_orders")
                    .r#where("id", "=", n.out_trade_no())
                    .update(
                        [("status".into(), "cancelled".into())]
                            .into_iter()
                            .collect(),
                    )
                    .await?;
                signal.notify_one();
                std::future::pending::<()>().await;
                Ok(())
            })
        })
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), started.notified())
        .await
        .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let calls = Arc::new(AtomicUsize::new(0));
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let repo = repository.clone();
        let notification = notification.clone();
        let calls = calls.clone();
        tasks.push(tokio::spawn(async move {
            repo.process(&notification, &expected(), move |tx, n| {
                let calls = calls.clone();
                Box::pin(async move {
                    assert_eq!(
                        tx.table("payment_orders")
                            .r#where("id", "=", n.out_trade_no())
                            .value("status")
                            .await?,
                        Value::from("pending")
                    );
                    calls.fetch_add(1, Ordering::SeqCst);
                    tx.table("payment_orders")
                        .r#where("id", "=", n.out_trade_no())
                        .update([("status".into(), "paid".into())].into_iter().collect())
                        .await?;
                    Ok(())
                })
            })
            .await
            .unwrap()
        }));
    }
    let mut processed = 0;
    for task in tasks {
        if task.await.unwrap() == NotificationOutcome::Processed {
            processed += 1;
        }
    }
    assert_eq!(processed, 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        connection
            .table("payment_orders")
            .value("status")
            .await
            .unwrap(),
        Value::from("paid")
    );
    assert_eq!(
        connection
            .table("payment_notifications")
            .count("*")
            .await
            .unwrap(),
        1
    );
    let mut wrong = expected();
    wrong.amount = Amount::from_fen(1).unwrap();
    assert!(matches!(
        repository
            .process(&notification, &wrong, |_, _| Box::pin(async {
                panic!("must not run")
            }))
            .await,
        Err(Error::OrderMismatch)
    ));
    let (h, b) = wechat_callback(json!({"amount":{"total":100,"currency":"CNY"}}));
    let changed = gateway.callback(&h, &b).unwrap();
    let mut other = expected();
    other.amount = Amount::from_fen(100).unwrap();
    assert!(matches!(
        repository
            .process(&changed, &other, |_, _| Box::pin(async {
                panic!("conflict must not run")
            }))
            .await,
        Err(Error::NotificationConflict)
    ));
    println!(
        "{driver}: migration, rollback, cancellation, eight concurrent callbacks, duplicate, amount mismatch and identity reuse passed"
    );
    connection
        .get_schema_builder()
        .drop_if_exists("payment_orders")
        .await
        .unwrap();
    connection
        .get_schema_builder()
        .drop_if_exists("payment_notifications")
        .await
        .unwrap();
    connection
        .get_schema_builder()
        .drop_if_exists("pay_migrations")
        .await
        .unwrap();
    db.disconnect(None).unwrap();
    std::fs::remove_dir_all(path).unwrap();
}
