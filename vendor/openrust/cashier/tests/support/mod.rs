#![allow(dead_code, reason = "Shared billing fixtures")]
use axum::{
    Router,
    body::Bytes,
    extract::State,
    response::{IntoResponse, Response},
};
use openrust::{
    config::Repository,
    database::{
        Connection, DatabaseManager,
        migrations::{MigrationEntry, MigrationRegistry, Migrator, RunOptions},
    },
    http::client::Factory,
};
use openrust_cashier::{Billable, BillingStore, Client, CreateCashierTables, Provider};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
pub const SECRET: &str = "whsec_fixture_secret";
static COUNTER: AtomicUsize = AtomicUsize::new(0);
#[derive(Clone, Debug)]
pub struct Captured {
    pub method: http::Method,
    pub path: String,
    pub params: Value,
    pub headers: http::HeaderMap,
}
pub struct Machine {
    pub provider: Provider,
    pub requests: Vec<Captured>,
    pub subscription: Value,
    pub invoice: Value,
    pub failure: Option<u16>,
    pub delay: bool,
    pub customer_deleted: bool,
}
pub struct Server {
    pub state: Arc<Mutex<Machine>>,
    pub url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    pub async fn new(provider: Provider) -> Self {
        let state = Arc::new(Mutex::new(Machine {
            provider,
            requests: vec![],
            subscription: subscription(provider, "active"),
            invoice: invoice(provider),
            failure: None,
            delay: false,
            customer_deleted: false,
        }));
        let router = Router::new().fallback(handler).with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self { state, url, task }
    }
    pub fn client(&self) -> Client {
        let provider = self.state.lock().unwrap().provider;
        Client::new(
            "test",
            &profile(provider, &self.url),
            Arc::new(Factory::new().unwrap()),
        )
        .unwrap()
    }
    pub fn requests(&self) -> Vec<Captured> {
        self.state.lock().unwrap().requests.clone()
    }
}
pub fn profile(provider: Provider, url: &str) -> Value {
    json!({"driver":provider.as_str(),"secret":"api_secret_fixture","webhook_secret":SECRET,"endpoint":url,"live":false})
}
pub fn subscription(p: Provider, status: &str) -> Value {
    if p == Provider::Stripe {
        json!({"id":"sub_1","object":"subscription","customer":"cus_1","status":status,"metadata":{"cashier_type":"default"},"items":{"data":[{"id":"si_1","price":{"id":"price_basic"},"quantity":1,"current_period_end":jiff::Timestamp::now().as_second()+86400}]},"trial_end":null,"cancel_at_period_end":false})
    } else {
        json!({"id":"sub_1","customer_id":"cus_1","status":status,"custom_data":{"cashier_type":"default"},"items":[{"price":{"id":"price_basic"},"quantity":1,"trial_dates":null}],"scheduled_change":null,"paused_at":null,"canceled_at":null})
    }
}
pub fn invoice(p: Provider) -> Value {
    if p == Provider::Stripe {
        json!({"id":"in_1","customer":"cus_1","status":"paid","currency":"usd","total":1234,"parent":{"subscription_details":{"subscription":"sub_1"}},"invoice_pdf":"https://files.stripe.com/invoice.pdf","hosted_invoice_url":"https://invoice.stripe.com/i"})
    } else {
        json!({"id":"txn_1","customer_id":"cus_1","status":"completed","currency_code":"USD","details":{"totals":{"total":"1234"}},"subscription_id":"sub_1"})
    }
}
fn form(body: &[u8]) -> Value {
    let mut result = json!({});
    for (k, v) in url::form_urlencoded::parse(body) {
        result[k.as_ref()] = json!(v.as_ref());
    }
    result
}
async fn handler(
    State(state): State<Arc<Mutex<Machine>>>,
    method: http::Method,
    uri: http::Uri,
    headers: http::HeaderMap,
    body: Bytes,
) -> Response {
    let path = uri.path().to_owned();
    let query = uri.query().unwrap_or("");
    let (provider, delay) = {
        let s = state.lock().unwrap();
        (s.provider, s.delay)
    };
    let params = if method == http::Method::GET {
        form(query.as_bytes())
    } else if provider == Provider::Stripe {
        form(&body)
    } else {
        serde_json::from_slice(&body).unwrap_or(json!({}))
    };
    state.lock().unwrap().requests.push(Captured {
        method: method.clone(),
        path: path.clone(),
        params: params.clone(),
        headers,
    });
    if delay {
        tokio::time::sleep(std::time::Duration::from_secs(16)).await;
    }
    let mut s = state.lock().unwrap();
    if let Some(code) = s.failure.take() {
        return (
            http::StatusCode::from_u16(code).unwrap(),
            json!({"error":{"message":"api_secret_fixture private response"}}).to_string(),
        )
            .into_response();
    }
    let stripe = provider == Provider::Stripe;
    let p = path.strip_prefix("/v1").unwrap_or(&path);
    let value = if p == "/customers" && method == http::Method::POST || p == "/customers/cus_1" {
        if s.customer_deleted {
            json!({"id":"cus_1","deleted":true})
        } else {
            json!({"id":"cus_1","balance":-300,"invoice_settings":{"default_payment_method":"pm_1"}})
        }
    } else if p == "/payment_methods/pm_1" || p.starts_with("/payment_methods/pm_1/") {
        json!({"id":"pm_1","customer":"cus_1"})
    } else if p == "/payment_methods/pm_other" {
        json!({"id":"pm_other","customer":"cus_other"})
    } else if p == "/subscriptions" && method == http::Method::POST {
        s.subscription.clone()
    } else if p == "/subscriptions/sub_1" || p.starts_with("/subscriptions/sub_1/") {
        if method != http::Method::GET && !p.ends_with("preview") && !p.ends_with("charge") {
            if stripe {
                if method == http::Method::DELETE {
                    s.subscription["status"] = json!("canceled");
                    s.subscription["ended_at"] = json!(jiff::Timestamp::now().as_second());
                }
                if let Some(v) = params["cancel_at_period_end"].as_str() {
                    s.subscription["cancel_at_period_end"] = json!(v == "true");
                }
                if let Some(v) = params["items[0][quantity]"].as_str() {
                    s.subscription["items"]["data"][0]["quantity"] =
                        json!(v.parse::<u64>().unwrap());
                }
                if let Some(v) = params["trial_end"].as_str() {
                    s.subscription["trial_end"] = json!(v.parse::<i64>().unwrap());
                }
            } else {
                if p.ends_with("/cancel") {
                    if params["effective_from"] == "immediately" {
                        s.subscription["status"] = json!("canceled");
                        s.subscription["canceled_at"] = json!(jiff::Timestamp::now().to_string());
                    } else {
                        s.subscription["scheduled_change"] = json!({"action":"cancel","effective_at":(jiff::Timestamp::now()+jiff::SignedDuration::from_secs(86400)).to_string()});
                    }
                }
                if p.ends_with("/pause") {
                    s.subscription["status"] = json!("paused");
                    s.subscription["paused_at"] = json!(jiff::Timestamp::now().to_string());
                }
                if p.ends_with("/resume") {
                    s.subscription["status"] = json!("active");
                    s.subscription["paused_at"] = Value::Null;
                }
                if params.get("scheduled_change") == Some(&Value::Null) {
                    s.subscription["scheduled_change"] = Value::Null;
                }
                if let Some(items) = params.get("items") {
                    s.subscription["items"] = Value::Array(
                        items
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|i| json!({"price":{"id":i["price_id"]},"quantity":i["quantity"]}))
                            .collect(),
                    );
                }
            }
        }
        s.subscription.clone()
    } else if p == "/invoices/in_1" || p == "/transactions/txn_1" {
        s.invoice.clone()
    } else if p == "/checkout/sessions" {
        json!({"id":"cs_1","url":"https://checkout.stripe.com/c/pay"})
    } else if p == "/transactions" && method == http::Method::POST {
        json!({"id":"txn_1","customer_id":"cus_1","checkout":{"url":"https://checkout.paddle.com/pay"},"status":"ready"})
    } else if p == "/payment_intents" {
        json!({"id":"pi_1","customer":"cus_1","status":"requires_action","client_secret":"pi_client_secret_fixture","next_action":{"type":"use_stripe_sdk"}})
    } else if p == "/payment_intents/pi_1" || p == "/payment_intents/pi_1/confirm" {
        json!({"id":"pi_1","customer":"cus_1","status":"succeeded"})
    } else if p == "/billing_portal/sessions" || p.ends_with("/portal-sessions") {
        json!({"id":"portal_1","url":"https://billing.example/portal"})
    } else if p == "/refunds" || p == "/adjustments" {
        json!({"id":"refund_1","status":"pending"})
    } else {
        json!({"id":"resource_1","data":[],"meta":{"pagination":{"has_more":false}}})
    };
    (
        http::StatusCode::OK,
        [("content-type", "application/json")],
        (if stripe {
            value
        } else {
            json!({"data":value,"meta":{"request_id":"request_1"}})
        })
        .to_string(),
    )
        .into_response()
}
pub struct Db {
    pub store: BillingStore,
    pub connection: Connection,
    pub manager: Arc<DatabaseManager>,
    pub path: std::path::PathBuf,
    pub migrator: Migrator,
}
impl Drop for Db {
    fn drop(&mut self) {
        let _ = self.manager.disconnect(None);
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
impl Db {
    pub async fn new(external: bool) -> Self {
        let path = std::env::temp_dir().join(format!(
            "openrust-cashier-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&path).unwrap();
        let file = path.join("db.sqlite");
        std::fs::File::create(&file).unwrap();
        let driver = if external {
            std::env::var("CASHIER_TEST_DRIVER").unwrap_or("sqlite".into())
        } else {
            "sqlite".into()
        };
        let params = match driver.as_str() {
            "sqlite" => {
                json!({"driver":"sqlite","database":file,"busy_timeout":10000,"max_connections":8})
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
            _ => panic!("unsupported driver"),
        };
        let config = Arc::new(Repository::new(BTreeMap::from([(
            "database".into(),
            json!({"default":"test","connections":{"test":params}}),
        )])));
        let manager = Arc::new(DatabaseManager::new(config, &path));
        let connection = manager.connection(None).unwrap();
        println!(
            "{driver} runtime {:?}",
            connection
                .scalar(
                    match driver.as_str() {
                        "pgsql" => "show server_version_num",
                        "mysql" => "select version()",
                        _ => "select sqlite_version()",
                    },
                    &[]
                )
                .await
                .unwrap()
        );
        static ENTRIES: &[MigrationEntry] = &[MigrationEntry::new(
            "2026_11_21_000001_create_cashier_tables.rs",
            &CreateCashierTables,
        )];
        let migrator = Migrator::new(
            manager.clone(),
            Arc::new(MigrationRegistry(ENTRIES)),
            "cashier_migrations".into(),
            path.clone(),
        );
        migrator
            .run(None, &[], RunOptions::default())
            .await
            .unwrap();
        Self {
            store: BillingStore::new(connection.clone()),
            connection,
            manager,
            path,
            migrator,
        }
    }
    pub async fn billable(&self, client: Client) -> Billable {
        let b = Billable::new(client, self.store.clone(), "users:1").unwrap();
        b.attach_customer("cus_1").await.unwrap();
        b
    }
}
pub fn event(
    client: &Client,
    id: &str,
    kind: &str,
    data: Value,
    offset: i64,
) -> (http::HeaderMap, Vec<u8>) {
    let body=serde_json::to_vec(&if client.provider()==Provider::Stripe{json!({"id":id,"object":"event","type":kind,"livemode":false,"created":1,"data":{"object":data}})}else{json!({"event_id":id,"event_type":kind,"occurred_at":"2020-01-01T00:00:00Z","data":data})}).unwrap();
    (headers(client.provider(), &body, offset, SECRET), body)
}
pub fn headers(provider: Provider, body: &[u8], offset: i64, secret: &str) -> http::HeaderMap {
    let t = jiff::Timestamp::now().as_second() + offset;
    let stripe = provider == Provider::Stripe;
    let mut msg = format!("{t}{}", if stripe { '.' } else { ':' }).into_bytes();
    msg.extend_from_slice(body);
    let key = aws_lc_rs::hmac::Key::new(aws_lc_rs::hmac::HMAC_SHA256, secret.as_bytes());
    let sig: String = aws_lc_rs::hmac::sign(&key, &msg)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let mut h = http::HeaderMap::new();
    h.insert(
        if stripe {
            "stripe-signature"
        } else {
            "paddle-signature"
        },
        http::HeaderValue::from_str(&if stripe {
            format!("t={t},v1={sig}")
        } else {
            format!("ts={t};h1={sig}")
        })
        .unwrap(),
    );
    h
}
