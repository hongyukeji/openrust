mod support;
use openrust::database::{DatabaseError, Value};
use openrust_cashier::{Billable, Error, Provider, WebhookOutcome};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use support::*;
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn durable_billing_reconciles_out_of_order_events_in_one_business_transaction_and_recovers_cancellation()
 {
    let db = Db::new(true).await;
    db.connection
        .get_schema_builder()
        .create("billing_business", |t| {
            t.string("id", 255).primary();
            t.string("status", 16);
        })
        .await
        .unwrap();
    for provider in [Provider::Stripe, Provider::Paddle] {
        let server = Server::new(provider).await;
        let client = server.client();
        let b = db.billable(client.clone()).await;
        let kind = if provider == Provider::Stripe {
            "customer.subscription.updated"
        } else {
            "subscription.updated"
        };
        let (h, body) = event(
            &client,
            "evt_first",
            kind,
            subscription(provider, "active"),
            0,
        );
        let event = client.verifyWebhook(&h, &body).unwrap();
        let result = db
            .store
            .process(&client, &event, |tx, e| {
                Box::pin(async move {
                    tx.table("billing_business")
                        .insert(&[[
                            ("id".into(), e.id().into()),
                            ("status".into(), "rollback".into()),
                        ]
                        .into_iter()
                        .collect()])
                        .await?;
                    Err(DatabaseError::InvalidQuery("rollback fixture"))
                })
            })
            .await;
        assert!(matches!(result, Err(Error::Database)));
        assert_eq!(
            db.connection
                .table("billing_business")
                .count("*")
                .await
                .unwrap(),
            0
        );
        assert!(b.subscription("default").await.unwrap().is_none());
        let signal = Arc::new(tokio::sync::Notify::new());
        let started = signal.clone();
        let store = db.store.clone();
        let client_copy = client.clone();
        let notice = event.clone();
        let task = tokio::spawn(async move {
            store
                .process(&client_copy, &notice, move |tx, e| {
                    let signal = signal.clone();
                    Box::pin(async move {
                        tx.table("billing_business")
                            .insert(&[[
                                ("id".into(), e.id().into()),
                                ("status".into(), "cancelled".into()),
                            ]
                            .into_iter()
                            .collect()])
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
        let count = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let (store, c, e, count) = (
                db.store.clone(),
                client.clone(),
                event.clone(),
                count.clone(),
            );
            tasks.push(tokio::spawn(async move {
                store
                    .process(&c, &e, move |tx, e| {
                        let count = count.clone();
                        Box::pin(async move {
                            count.fetch_add(1, Ordering::SeqCst);
                            tx.table("billing_business")
                                .insert(&[[
                                    ("id".into(), e.id().into()),
                                    ("status".into(), "done".into()),
                                ]
                                .into_iter()
                                .collect()])
                                .await?;
                            Ok(())
                        })
                    })
                    .await
            }));
        }
        let mut processed = 0;
        for task in tasks {
            if task.await.unwrap().unwrap() == WebhookOutcome::Processed {
                processed += 1;
            }
        }
        assert_eq!(processed, 1);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(b.subscribed("default").await.unwrap());
        // An old signed event describes past_due, while the canonical API already says active.
        let (h, body) = support::event(
            &client,
            "evt_old",
            kind,
            subscription(provider, "past_due"),
            0,
        );
        let old = client.verifyWebhook(&h, &body).unwrap();
        db.store
            .process(&client, &old, |_, event| {
                Box::pin(async move {
                    assert_eq!(event.currentSubscription().unwrap().status, "active");
                    Ok(())
                })
            })
            .await
            .unwrap();
        assert_eq!(
            b.subscription("default").await.unwrap().unwrap().status,
            "active"
        );
        let (h, body) = support::event(
            &client,
            "evt_first",
            kind,
            subscription(provider, "canceled"),
            0,
        );
        let conflict = client.verifyWebhook(&h, &body).unwrap();
        assert!(matches!(
            db.store.handleWebhook(&client, &conflict).await,
            Err(Error::EventConflict)
        ));
        let hkind = if provider == Provider::Stripe {
            "invoice.paid"
        } else {
            "transaction.completed"
        };
        let (h, body) = support::event(&client, "evt_invoice", hkind, invoice(provider), 0);
        let notice = client.verifyWebhook(&h, &body).unwrap();
        db.store.handleWebhook(&client, &notice).await.unwrap();
        let invoices = b.localInvoices().await.unwrap();
        assert_eq!(invoices.len(), 1);
        assert_eq!(invoices[0].total, "1234");
        assert_eq!(invoices[0].subscription.as_deref(), Some("sub_1"));
        let unknown = Billable::new(client.clone(), db.store.clone(), "users:2").unwrap();
        assert!(unknown.attachCustomer("cus_1").await.is_err());
        assert!(unknown.customerId().await.is_err());
        let mut data = subscription(provider, "active");
        data[if provider == Provider::Stripe {
            "customer"
        } else {
            "customer_id"
        }] = json!("cus_attacker");
        let (h, body) = support::event(&client, "evt_attacker", kind, data, 0);
        assert!(matches!(
            db.store
                .handleWebhook(&client, &client.verifyWebhook(&h, &body).unwrap())
                .await,
            Err(Error::Ownership)
        ));
        let record = db
            .connection
            .table("cashier_subscriptions")
            .get()
            .await
            .unwrap();
        for row in record {
            let Some(Value::Text(payload)) = row.get("payload") else {
                panic!()
            };
            for secret in [
                "client_secret",
                "api_secret_fixture",
                "billing_address",
                "card",
            ] {
                assert!(!payload.contains(secret));
            }
        }
        if provider == Provider::Paddle {
            let before = server.requests().len();
            let mut tasks = vec![];
            for _ in 0..8 {
                let (store, c) = (db.store.clone(), client.clone());
                tasks.push(tokio::spawn(async move {
                    store
                        .execute(
                            &c,
                            http::Method::POST,
                            "/customers",
                            &json!({"email":"shared@example.test"}),
                            "parallel-create",
                        )
                        .await
                }));
            }
            let mut successes = 0;
            for task in tasks {
                match task.await.unwrap() {
                    Ok(_) => successes += 1,
                    Err(Error::OutcomeUnknown) => {}
                    Err(e) => panic!("{e}"),
                }
            }
            assert_eq!(successes, 1);
            assert_eq!(server.requests().len(), before + 1);
            assert!(matches!(
                db.store
                    .execute(
                        &client,
                        http::Method::POST,
                        "/customers",
                        &json!({"email":"changed@example.test"}),
                        "parallel-create"
                    )
                    .await,
                Err(Error::EventConflict)
            ));
        }
        if provider == Provider::Stripe {
            let old_handle = b.subscription("default").await.unwrap().unwrap();
            server.state.lock().unwrap().customer_deleted = true;
            let (h, body) = support::event(
                &client,
                "evt_customer_deleted",
                "customer.deleted",
                json!({"id":"cus_1","deleted":true}),
                0,
            );
            let event = client.verifyWebhook(&h, &body).unwrap();
            db.store
                .process(&client, &event, |_, e| {
                    Box::pin(async move {
                        assert!(e.customerDeleted());
                        Ok(())
                    })
                })
                .await
                .unwrap();
            assert!(!b.subscribed("default").await.unwrap());
            assert!(matches!(b.customerId().await, Err(Error::NotFound)));
            let before = server.requests().len();
            assert!(matches!(
                old_handle.preview(json!({}), "deleted-preview").await,
                Err(Error::NotFound)
            ));
            assert!(
                server.requests()[before..]
                    .iter()
                    .all(|r| r.method == http::Method::GET)
            );
        }
        db.connection
            .table("billing_business")
            .delete()
            .await
            .unwrap();
    }
    db.connection
        .get_schema_builder()
        .drop_if_exists("billing_business")
        .await
        .unwrap();
    db.migrator
        .rollback(None, &[], Default::default())
        .await
        .unwrap();
    assert!(
        !db.connection
            .get_schema_builder()
            .has_table("cashier_events")
            .await
            .unwrap()
    );
}
