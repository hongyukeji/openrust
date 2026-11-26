mod support;
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    database::{Connection, DatabaseManager, Value},
    foundation::{Application, configuration::Routing},
    support::facades::Artisan,
    testing::TestApp,
};
use openrust_pay::{Amount, DatabaseNotifications, Error, NotificationOutcome, PayServiceProvider};
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use support::*;

// 只在独占测试数据库安装触发器，模拟业务回调成功后的收件箱完成写入失败。
async fn completion_failure(connection: &Connection, driver: &str, enabled: bool) {
    let statements: &[&str] = match (driver, enabled) {
        ("sqlite", true) => &[
            "CREATE TRIGGER fail_payment_done BEFORE UPDATE OF status ON payment_notifications WHEN NEW.status = 'done' BEGIN SELECT RAISE(ABORT, 'completion failure'); END",
        ],
        ("sqlite", false) | ("mysql", false) => &["DROP TRIGGER fail_payment_done"],
        ("mysql", true) => &[
            "CREATE TRIGGER fail_payment_done BEFORE UPDATE ON payment_notifications FOR EACH ROW SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT = 'completion failure'",
        ],
        ("pgsql", true) => &[
            "CREATE FUNCTION fail_payment_done_fn() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'completion failure'; END; $$",
            "CREATE TRIGGER fail_payment_done BEFORE UPDATE OF status ON payment_notifications FOR EACH ROW WHEN (NEW.status = 'done') EXECUTE FUNCTION fail_payment_done_fn()",
        ],
        ("pgsql", false) => &[
            "DROP TRIGGER fail_payment_done ON payment_notifications",
            "DROP FUNCTION fail_payment_done_fn()",
        ],
        _ => panic!("unsupported isolated database"),
    };
    for sql in statements {
        connection.unprepared(sql).await.unwrap();
    }
}
struct NotificationCommand(CommandDefinition);
impl Command for NotificationCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        app: &'a Application,
        _: &'a Input,
        _: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let connection = app
                .make::<DatabaseManager>()
                .await
                .unwrap()
                .connection(None)
                .unwrap();
            let driver = std::env::var("PAY_TEST_DRIVER").unwrap_or("sqlite".into());
            connection
                .get_schema_builder()
                .create("artisan_pay_orders", |t| {
                    t.string("id", 32).primary();
                })
                .await
                .unwrap();
            let store = DatabaseNotifications::new(connection.clone());
            for provider in ["wechat", "alipay"] {
                let (_, manager) = manager(provider, "https://example.test/");
                let gateway = manager.driver("main").unwrap();
                let (headers, body) = if provider == "wechat" {
                    wechat_callback(json!({}))
                } else {
                    alipay_callback(json!({}))
                };
                let notice = gateway.callback(&headers, &body).unwrap();
                let mut wrong = expected();
                wrong.amount = Amount::from_fen(1).unwrap();
                assert!(matches!(
                    store
                        .process(&notice, &wrong, |_, _| Box::pin(async {
                            panic!("订单不匹配时不能执行回调")
                        }))
                        .await,
                    Err(Error::OrderMismatch)
                ));
                assert_eq!(
                    connection
                        .table("payment_notifications")
                        .count("*")
                        .await
                        .unwrap(),
                    0
                );
                completion_failure(&connection, &driver, true).await;
                let callbacks = Arc::new(AtomicUsize::new(0));
                let called = callbacks.clone();
                let result = store
                    .process(&notice, &expected(), move |tx, n| {
                        let called = called.clone();
                        Box::pin(async move {
                            called.fetch_add(1, Ordering::SeqCst);
                            assert_eq!(
                                tx.table("payment_notifications").value("status").await?,
                                Value::from("pending")
                            );
                            tx.table("artisan_pay_orders")
                                .insert(&[[("id".into(), n.out_trade_no().into())]
                                    .into_iter()
                                    .collect()])
                                .await?;
                            Ok(())
                        })
                    })
                    .await;
                assert!(matches!(result, Err(Error::Database)));
                assert_eq!(callbacks.load(Ordering::SeqCst), 1);
                assert_eq!(
                    connection
                        .table("artisan_pay_orders")
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
                completion_failure(&connection, &driver, false).await;
                let called = callbacks.clone();
                assert_eq!(
                    store
                        .process(&notice, &expected(), move |tx, n| {
                            let called = called.clone();
                            Box::pin(async move {
                                called.fetch_add(1, Ordering::SeqCst);
                                tx.table("artisan_pay_orders")
                                    .insert(&[[("id".into(), n.out_trade_no().into())]
                                        .into_iter()
                                        .collect()])
                                    .await?;
                                Ok(())
                            })
                        })
                        .await
                        .unwrap(),
                    NotificationOutcome::Processed
                );
                assert_eq!(callbacks.load(Ordering::SeqCst), 2);
                assert_eq!(
                    connection
                        .table("payment_notifications")
                        .value("status")
                        .await
                        .unwrap(),
                    Value::from("done")
                );
                assert_eq!(
                    store
                        .process(&notice, &expected(), |_, _| Box::pin(async {
                            panic!("重复通知不能执行回调")
                        }))
                        .await
                        .unwrap(),
                    NotificationOutcome::Duplicate
                );
                let (headers, body) = if provider == "wechat" {
                    wechat_callback(json!({"amount":{"total":100,"currency":"CNY"}}))
                } else {
                    alipay_callback(json!({"total_amount":"1.00"}))
                };
                let changed = gateway.callback(&headers, &body).unwrap();
                let mut changed_expected = expected();
                changed_expected.amount = Amount::from_fen(100).unwrap();
                assert!(matches!(
                    store
                        .process(&changed, &changed_expected, |_, _| Box::pin(async {
                            panic!("指纹冲突不能执行回调")
                        }))
                        .await,
                    Err(Error::NotificationConflict)
                ));
                assert_eq!(
                    connection
                        .table("artisan_pay_orders")
                        .count("*")
                        .await
                        .unwrap(),
                    1
                );
                assert_eq!(
                    connection
                        .table("payment_notifications")
                        .value("status")
                        .await
                        .unwrap(),
                    Value::from("done")
                );
                for table in ["artisan_pay_orders", "payment_notifications"] {
                    connection.table(table).delete().await.unwrap();
                }
                // 保留既有受影响行语义：回调删除收件箱行时，完成更新为零行仍返回 Processed。
                assert_eq!(
                    store
                        .process(&notice, &expected(), |tx, n| Box::pin(async move {
                            tx.table("artisan_pay_orders")
                                .insert(&[[("id".into(), n.out_trade_no().into())]
                                    .into_iter()
                                    .collect()])
                                .await?;
                            tx.table("payment_notifications").delete().await?;
                            Ok(())
                        }))
                        .await
                        .unwrap(),
                    NotificationOutcome::Processed
                );
                assert_eq!(
                    connection
                        .table("artisan_pay_orders")
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
                    0
                );
                connection
                    .table("artisan_pay_orders")
                    .delete()
                    .await
                    .unwrap();
            }
            connection
                .get_schema_builder()
                .drop_if_exists("artisan_pay_orders")
                .await
                .unwrap();
            Ok(0)
        })
    }
}
fn commands() -> Result<(), ConsoleError> {
    Artisan::add(Box::new(NotificationCommand(CommandDefinition::new(
        clap::Command::new("pay:notifications-probe"),
    ))))
}
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn native_artisan_preserves_callback_and_completion_transaction_and_failures() {
    let mut app = TestApp::create(|builder| builder.with_config(|root, _| {
        let driver = std::env::var("PAY_TEST_DRIVER").unwrap_or("sqlite".into());
        let options = match driver.as_str() {
            "sqlite" => json!({"driver":"sqlite","database":root.join("pay.sqlite"),"busy_timeout":10000,"max_connections":8}),
            "mysql" => {
                assert_eq!(std::env::var("OPENRUST_REQUIRE_MYSQL").as_deref(), Ok("1"));
                assert!(std::env::var("OPENRUST_ACCEPTANCE_MYSQL_CONTAINER").is_ok());
                json!({"driver":"mysql","url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap(),"max_connections":8})
            },
            "pgsql" => {
                assert_eq!(std::env::var("OPENRUST_REQUIRE_POSTGRES").as_deref(), Ok("1"));
                assert!(std::env::var("OPENRUST_ACCEPTANCE_POSTGRES_CONTAINER").is_ok());
                json!({"driver":"pgsql","url":std::env::var("OPENRUST_TEST_POSTGRES_URL").unwrap(),"max_connections":8})
            },
            _ => panic!("unsupported isolated database"),
        };
        Ok(BTreeMap::from([("database".into(),json!({"default":"testing","connections":{"testing":options}}))]))
    }).with_providers(vec![Box::new(PayServiceProvider)]).with_routing(Routing::default().commands(commands))).unwrap();
    std::fs::File::create(app.root().join("pay.sqlite")).unwrap();
    app.artisan(["migrate", "--force"])
        .await
        .assert_successful();
    app.artisan(["pay:notifications-probe"])
        .await
        .assert_successful();
    app.artisan(["migrate:rollback", "--force"])
        .await
        .assert_successful();
}
