mod support;
use axum::{Router, body::Bytes, extract::State, response::IntoResponse, routing::post};
use openrust::{database::DatabaseError, http::client::Factory};
use openrust_cashier::{BillingStore, Client, Provider};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use support::*;

#[derive(Clone)]
struct Endpoint {
    client: Client,
    store: BillingStore,
    fail: Arc<AtomicBool>,
}
async fn callback(
    State(s): State<Endpoint>,
    headers: http::HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    let Ok(event) = s.client.verifyWebhook(&headers, &body) else {
        return http::StatusCode::BAD_REQUEST.into_response();
    };
    let fail = s.fail.clone();
    match s
        .store
        .process(&s.client, &event, move |tx, event| {
            let fail = fail.clone();
            Box::pin(async move {
                tx.table("callback_business")
                    .insert(&[[
                        ("id".into(), event.id().into()),
                        ("status".into(), "paid".into()),
                    ]
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
        Ok(_) => event.success().into_response(),
        Err(_) => http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn signed_http_webhooks_rollback_retry_duplicate_and_preserve_current_subscription() {
    let db = Db::new(true).await;
    db.connection
        .get_schema_builder()
        .create("callback_business", |t| {
            t.string("id", 255).primary();
            t.string("status", 16);
        })
        .await
        .unwrap();
    for provider in [Provider::Stripe, Provider::Paddle] {
        let upstream = Server::new(provider).await;
        let client = upstream.client();
        let billable = db.billable(client.clone()).await;
        let kind = if provider == Provider::Stripe {
            "customer.subscription.updated"
        } else {
            "subscription.updated"
        };
        let (headers, body) = event(
            &client,
            "evt_http",
            kind,
            subscription(provider, "past_due"),
            0,
        );
        let state = Endpoint {
            client,
            store: db.store.clone(),
            fail: Arc::new(AtomicBool::new(true)),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/webhook", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/webhook", post(callback))
            .with_state(state);
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let http = Factory::new().unwrap();
        let send = |bytes: Vec<u8>| {
            http.pending_request()
                .with_headers(headers.clone())
                .with_body(bytes, http::HeaderValue::from_static("application/json"))
                .send(http::Method::POST, &url)
        };
        let mut corrupt = body.clone();
        corrupt.push(b' ');
        assert_eq!(send(corrupt).await.unwrap().status(), 400);
        assert_eq!(
            db.connection
                .table("callback_business")
                .count("*")
                .await
                .unwrap(),
            0
        );
        assert_eq!(send(body.clone()).await.unwrap().status(), 500);
        assert_eq!(
            db.connection
                .table("callback_business")
                .count("*")
                .await
                .unwrap(),
            0
        );
        assert!(!billable.subscribed("default").await.unwrap());
        assert_eq!(send(body.clone()).await.unwrap().status(), 200);
        assert_eq!(send(body).await.unwrap().status(), 200);
        assert_eq!(
            db.connection
                .table("callback_business")
                .count("*")
                .await
                .unwrap(),
            1
        );
        assert!(billable.subscribed("default").await.unwrap());
        println!(
            "{provider:?}: TCP raw webhook tamper400, transaction failure500, retry200, duplicate200, canonical active subscription PASS"
        );
        server.abort();
        let _ = server.await;
        db.connection
            .table("callback_business")
            .delete()
            .await
            .unwrap();
    }
    db.connection
        .get_schema_builder()
        .drop_if_exists("callback_business")
        .await
        .unwrap();
    db.migrator
        .rollback(None, &[], Default::default())
        .await
        .unwrap();
}
